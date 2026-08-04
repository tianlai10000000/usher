#![allow(unexpected_cfgs)]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use anyhow::{Context, Result};

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

use slint::{CloseRequestResponse, ComponentHandle, Global, Timer};

// Re-export generated UI types at the composition-root namespace so feature
// modules retain their existing `crate::{MainWindow, Route, ...}` imports.
pub use usher_ui::*;

mod notification;

use notification::setup_notifications;

mod toast;
use toast::setup_toast;

mod dialog;
use dialog::setup_dialog;

mod tray;
use tray::setup_tray;

mod update;
use update::{setup_updater, start_auto_update, stop_auto_update};

mod network;
use network::setup_network_monitor;

mod http;
use http::setup_http;

mod settings;
use settings::setup_settings;
mod settings_page;
use settings_page::SettingsPageBridge;
mod message_center;
use message_center::MessagePageBridge;
use usher_messages::MessageCenter;

mod constants;
use constants::shutdown_priority::PRIORITY_HIGH;

mod chat;
use chat::setup_chat;

mod ai;

mod app_lifecycle;
mod app_runtime;
mod memory_diagnostics;
mod platform;
mod shortcuts;
use platform::{
    THEME_MODE_AUTO, apply_native_appearance_for_mode, apply_native_chrome_for_mode,
    effective_dark_mode as get_effective_dark_mode, install_system_theme_watch,
    install_titlebar_settings_button, play_ui_click, prepare_startup_reveal,
    set_auto_theme_watch_enabled, set_startup_presentation_hidden, set_title_bar_color,
    setup_application_font, setup_pr_review_fonts, startup_native_chrome_ready,
};
mod terminal;

mod logger;
use logger::error_handing;
use logger::tracing_logger;
use logger::{setup_debug_logger, setup_tracing_logger};

mod router;
use router::{Router, route_for_navigation_index};

mod jobs;
mod shutdown;
mod single_instance;
mod workflow;
use workflow::setup_workflow;
mod codereview;
mod git;
mod project;
use project::setup_project;
use terminal::{set_dark_mode as set_terminal_dark_mode, setup_terminal, shutdown_terminal};

pub use platform::is_window_interacting;

#[derive(Clone, Copy)]
enum SettingsSaveTarget {
    GitTokens,
    GitHosting,
    AiAgent,
}

fn set_settings_saved(
    ui: &MainWindow,
    page: &SettingsPageBridge,
    target: SettingsSaveTarget,
    saved: bool,
) {
    match target {
        SettingsSaveTarget::GitTokens => {
            ui.set_settings_git_tokens_saved(saved);
            page.set_git_tokens_saved(saved);
        }
        SettingsSaveTarget::GitHosting => {
            ui.set_settings_git_hosting_config_saved(saved);
            page.set_git_hosting_config_saved(saved);
        }
        SettingsSaveTarget::AiAgent => {
            ui.set_settings_ai_agent_saved(saved);
            page.set_ai_agent_saved(saved);
        }
    }
}

fn settings_toast(message: &str) -> ToastOptions {
    ToastOptions {
        message: message.into(),
        duration: 3_000,
        position: "bottom".into(),
        icon: "".into(),
        overlay: false,
        close_on_click: true,
        loading_type: "".into(),
        width: "auto".into(),
    }
}

fn save_settings_with_feedback<F>(
    ui_weak: slint::Weak<MainWindow>,
    settings_page: SettingsPageBridge,
    target: SettingsSaveTarget,
    operation: &'static str,
    success_message: &'static str,
    save: F,
) where
    F: FnOnce() -> std::result::Result<(), String> + Send + 'static,
{
    app_runtime::shared_runtime().spawn(async move {
        let result = tokio::task::spawn_blocking(save)
            .await
            .unwrap_or_else(|error| Err(format!("settings worker failed: {error}")));
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            match result {
                Ok(()) => {
                    set_settings_saved(&ui, &settings_page, target, true);
                    ToastManager::get(&ui).invoke_success(settings_toast(success_message));
                    let reset_weak = ui.as_weak();
                    let reset_settings_page = settings_page.clone();
                    Timer::single_shot(std::time::Duration::from_secs(2), move || {
                        if let Some(ui) = reset_weak.upgrade() {
                            set_settings_saved(&ui, &reset_settings_page, target, false);
                        }
                    });
                    log::info!("[Settings] {operation} saved");
                }
                Err(error) => {
                    set_settings_saved(&ui, &settings_page, target, false);
                    ToastManager::get(&ui)
                        .invoke_error(settings_toast(&format!("Failed to save {operation}.")));
                    log::error!("[Settings] Failed to save {operation}: {error}");
                }
            }
        });
    });
}

fn ui() -> Result<MainWindow> {
    memory_diagnostics::initialize_baseline();
    log::info!("Usher v{} starting...", env!("CARGO_PKG_VERSION"));

    // ========= Pre-Window Settings ========= //
    // Load settings BEFORE creating window to set app appearance early (prevents theme flash)
    let loaded_settings = settings::load_settings();

    // Set app-level appearance before window is created (prevents flash on macOS).
    // Auto clears any forced AppKit appearance so the process can track the OS.
    let is_dark = apply_native_appearance_for_mode(loaded_settings.theme_mode);
    set_auto_theme_watch_enabled(loaded_settings.theme_mode == THEME_MODE_AUTO);

    // Now create the MainWindow (will inherit app appearance)
    // Using native macOS title bar
    let app = MainWindow::new().context("Failed to create MainWindow")?;
    let settings_page = SettingsPageBridge::default();
    let message_center = MessageCenter::open_default().context("Failed to open message center")?;
    let message_page = MessagePageBridge::new(message_center.clone());
    app.set_platform_is_macos(cfg!(target_os = "macos"));
    message_center.set_application_active(app.get_native_window_active());
    let message_center_for_activity = message_center.clone();
    app.on_message_window_activity_changed(move |active| {
        message_center_for_activity.set_application_active(active);
    });
    setup_application_font(&app);
    setup_pr_review_fonts(&app);

    // Intercept title-bar close button: hide window instead of terminating process.
    app.window().on_close_requested(|| {
        log::info!("[Window] Close button clicked, hiding window to tray");
        CloseRequestResponse::HideWindow
    });

    // Initialize debug logger after window is created (needs app for callbacks)
    setup_debug_logger(&app, true).unwrap_or_else(|e| eprintln!("Debug Logger init failed: {}", e)); // eprintln here since logger not yet initialized
    memory_diagnostics::log_checkpoint("startup.debug_logger_ready");

    // ========= Trace Logger Initialization ========= //
    setup_tracing_logger(&app, loaded_settings.tracing_logger_active)
        .unwrap_or_else(|e| eprintln!("Tracing Logger init failed: {}", e));
    memory_diagnostics::log_checkpoint("startup.tracing_logger_ready");

    // ========= Settings ========= //
    // Apply loaded settings to AppState
    setup_settings(&app, &loaded_settings);
    set_terminal_dark_mode(&app, is_dark);
    // Seed native chrome / sidebar material colors before the first ordered frame.
    set_title_bar_color(&app, is_dark);
    Router::apply_route_ui_state(&app, app.get_current_route());
    memory_diagnostics::log_checkpoint("startup.settings_and_initial_route_ready");

    // ========= Debug: Simulate Crash ========= //
    app.on_simulate_crash(|| {
        if let Err(e) = error_handing::do_something_that_might_fail() {
            log::error!("[Debug] simulate crash returned error: {}", e);
        }
    });

    // ========= Update ========= //
    // setup_updater must be called AFTER setup_settings so it reads the correct auto_update value
    let updater_ctx = setup_updater(&app)?;
    memory_diagnostics::log_checkpoint("startup.updater_ready");

    // ========= Auto Update ========= //
    // Initialize auto-update based on loaded settings
    if loaded_settings.auto_update {
        start_auto_update(
            app.as_weak(),
            updater_ctx.state.clone(),
            updater_ctx.rt.clone(),
        );
    }

    // Set up auto-update toggle callback (saves settings AND controls auto-update)
    let app_weak_auto = app.as_weak();
    let rt_for_auto = updater_ctx.rt.clone();
    let state_for_auto = updater_ctx.state.clone();
    AppState::get(&app).on_toggle_auto_update(move |enabled| {
        // Save to config file
        settings::save_auto_update(enabled);

        // Control auto-update
        if enabled {
            start_auto_update(
                app_weak_auto.clone(),
                state_for_auto.clone(),
                rt_for_auto.clone(),
            );
        } else {
            stop_auto_update();
        }

        log::info!("[Settings] Auto update changed to: {}", enabled);
    });

    // ========= Tracing File Logs ========= //
    AppState::get(&app).on_toggle_tracing_logger_active(move |enabled| {
        settings::save_tracing_logger_active(enabled);
        if let Err(e) = tracing_logger::set_tracing_active(enabled) {
            log::error!("[Logger] Failed to toggle tracing logs: {}", e);
        }
        log::info!("[Settings] Tracing logger active changed to: {}", enabled);
    });

    // ========= Terminal Verbose Logs Active ========= //
    AppState::get(&app).on_toggle_terminal_verbose_logging(move |enabled| {
        settings::save_terminal_verbose_logging(enabled);
        terminal::set_verbose_logging(enabled);
        log::info!(
            "[Settings] Terminal verbose logging changed to: {}",
            enabled
        );
    });

    // ========= UI Sound Effects ========= //
    AppState::get(&app).on_toggle_ui_sound_effects(move |enabled| {
        settings::save_ui_sound_effects(enabled);
        log::info!("[Settings] UI sound effects changed to: {}", enabled);
    });

    // ========= Git Tokens ========= //
    let app_weak_git_tokens = app.as_weak();
    let settings_page_for_git_tokens = settings_page.clone();
    AppState::get(&app).on_save_git_tokens(
        move |github_token, gitlab_token, gitee_token, gitee_enterprise_token| {
            let github_token = github_token.to_string();
            let gitlab_token = gitlab_token.to_string();
            let gitee_token = gitee_token.to_string();
            let gitee_enterprise_token = gitee_enterprise_token.to_string();
            save_settings_with_feedback(
                app_weak_git_tokens.clone(),
                settings_page_for_git_tokens.clone(),
                SettingsSaveTarget::GitTokens,
                "code hosting tokens",
                "Code hosting tokens saved.",
                move || {
                    settings::save_git_platform_tokens(
                        &github_token,
                        &gitlab_token,
                        &gitee_token,
                        &gitee_enterprise_token,
                    )
                },
            );
        },
    );

    // ========= Code Hosting API Configuration ========= //
    let app_weak_git_hosting_config = app.as_weak();
    let settings_page_for_git_hosting = settings_page.clone();
    AppState::get(&app).on_save_git_hosting_config(
        move |github_api_base_url,
              github_enterprise,
              gitlab_api_base_url,
              gitlab_enterprise,
              gitee_api_base_url,
              gitee_enterprise| {
            let github_api_base_url = github_api_base_url.to_string();
            let github_enterprise = github_enterprise.to_string();
            let gitlab_api_base_url = gitlab_api_base_url.to_string();
            let gitlab_enterprise = gitlab_enterprise.to_string();
            let gitee_api_base_url = gitee_api_base_url.to_string();
            let gitee_enterprise = gitee_enterprise.to_string();
            save_settings_with_feedback(
                app_weak_git_hosting_config.clone(),
                settings_page_for_git_hosting.clone(),
                SettingsSaveTarget::GitHosting,
                "code hosting API configuration",
                "Code hosting API configuration saved.",
                move || {
                    settings::save_git_hosted_repository_configs(
                        &github_api_base_url,
                        &github_enterprise,
                        &gitlab_api_base_url,
                        &gitlab_enterprise,
                        &gitee_api_base_url,
                        &gitee_enterprise,
                    )
                },
            );
        },
    );

    // ========= AI Agent Settings ========= //
    let app_weak_ai_agent_save = app.as_weak();
    let settings_page_for_ai_save = settings_page.clone();
    AppState::get(&app).on_save_ai_agent_settings(move |provider, base_url, api_key, model| {
        let provider = provider.to_string();
        let base_url = base_url.to_string();
        let api_key = api_key.to_string();
        let model = model.to_string();
        save_settings_with_feedback(
            app_weak_ai_agent_save.clone(),
            settings_page_for_ai_save.clone(),
            SettingsSaveTarget::AiAgent,
            "AI Agent settings",
            "AI Agent settings saved.",
            move || settings::save_ai_agent_settings(&provider, &base_url, &api_key, &model),
        );
    });

    let app_weak_ai_agent_test = app.as_weak();
    let settings_page_for_ai_test = settings_page.clone();
    AppState::get(&app).on_test_ai_agent_connection(move |base_url, api_key, model| {
        let settings = settings::AiAgentSettings {
            provider: "custom".to_string(),
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        };
        let ui_weak = app_weak_ai_agent_test.clone();
        let settings_page = settings_page_for_ai_test.clone();
        app_runtime::shared_runtime().spawn(async move {
            let result = ai::agent::test_connection(&settings).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_settings_ai_agent_testing(false);
                settings_page.set_ai_agent_testing(false);
                match result {
                    Ok(result) => {
                        let message = if result.model_listed {
                            "AI Agent connected; the selected model is available."
                        } else {
                            "AI Agent connected. The provider did not list the selected model."
                        };
                        ToastManager::get(&ui).invoke_success(ToastOptions {
                            message: message.into(),
                            duration: 3_500,
                            position: "bottom".into(),
                            icon: "".into(),
                            overlay: false,
                            close_on_click: true,
                            loading_type: "".into(),
                            width: "auto".into(),
                        });
                    }
                    Err(error) => {
                        ToastManager::get(&ui).invoke_error(ToastOptions {
                            message: error.user_message().into(),
                            duration: 3_500,
                            position: "bottom".into(),
                            icon: "".into(),
                            overlay: false,
                            close_on_click: true,
                            loading_type: "".into(),
                            width: "auto".into(),
                        });
                    }
                }
            });
        });
    });

    // ========= Router ========= //
    let router = Router::new(
        app.as_weak(),
        std::rc::Rc::new(app_lifecycle::handle_route_transition),
    );

    let router_for_nav_index = router.clone();
    let app_for_nav_index = app.as_weak();
    app.on_navigate_from_nav_index(move |item_index| {
        if let Some(route) = route_for_navigation_index(item_index) {
            if let Some(state) = router_for_nav_index.navigate(route) {
                if let Some(ui) = app_for_nav_index.upgrade() {
                    ui.invoke_chat_route_visible_changed(state.chat_route_visible);
                }
            }
        }
    });

    // Connect navigate callback in slint
    let router_clone = router.clone();
    let app_for_chat_route = app.as_weak();
    let app_for_ai_agent_provider = app.as_weak();
    let settings_page_for_navigation = settings_page.clone();
    app.on_navigate(move |route| {
        if let Some(state) = router_clone.navigate(route) {
            if let Some(ui) = app_for_chat_route.upgrade() {
                ui.invoke_chat_route_visible_changed(state.chat_route_visible);
            }
        }

        if route == Route::Settings {
            if let Some(ui) = app_for_ai_agent_provider.upgrade() {
                let index = match AppState::get(&ui).get_ai_agent_provider().as_str() {
                    "deepseek" => 1,
                    "openai" => 2,
                    _ => 0,
                };
                // SettingsPage is conditionally created by the route. Force a
                // transition after it exists so the native dropdown populates.
                ui.set_settings_ai_agent_provider_index(-1);
                settings_page_for_navigation.set_ai_agent_provider_index(-1);
                let ui_weak = ui.as_weak();
                let settings_page = settings_page_for_navigation.clone();
                Timer::single_shot(std::time::Duration::from_millis(1), move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_settings_ai_agent_provider_index(index);
                        settings_page.set_ai_agent_provider_index(index);
                    }
                });
            }
        }
    });

    let router_for_back = router.clone();
    let app_for_back = app.as_weak();
    app.on_navigate_back(move || {
        if let Some(ui) = app_for_back.upgrade() {
            if AppState::get(&ui).get_ui_sound_effects() {
                play_ui_click();
            }
            if ui.get_current_route() == Route::PrReview {
                ui.invoke_workflow_pr_review_back();
                return;
            }
            if let Some(state) = router_for_back.back() {
                ui.invoke_chat_route_visible_changed(state.chat_route_visible);
            }
        }
    });

    let router_for_feature_back = router.clone();
    let app_for_feature_back = app.as_weak();
    app.on_router_back(move || {
        if let Some(state) = router_for_feature_back.back() {
            if let Some(ui) = app_for_feature_back.upgrade() {
                ui.invoke_chat_route_visible_changed(state.chat_route_visible);
            }
        }
    });

    let router_for_forward = router.clone();
    let app_for_forward = app.as_weak();
    app.on_navigate_forward(move || {
        if let Some(ui) = app_for_forward.upgrade() {
            if AppState::get(&ui).get_ui_sound_effects() {
                play_ui_click();
            }
            if let Some(state) = router_for_forward.forward() {
                ui.invoke_chat_route_visible_changed(state.chat_route_visible);
            }
        }
    });

    let sidebar_toggle_target =
        std::rc::Rc::new(std::cell::Cell::new(app.get_navigation_drawer_expanded()));
    let app_for_sidebar_toggle = app.as_weak();
    app.on_toggle_navigation_drawer(move || {
        sidebar_toggle_target.set(!sidebar_toggle_target.get());
        let Some(ui) = app_for_sidebar_toggle.upgrade() else {
            return;
        };
        let expanded = sidebar_toggle_target.get();
        if ui.get_navigation_drawer_expanded() != expanded {
            // Retarget the one in-flight spring. The route adopts the target
            // width once, then shell surfaces move without per-frame width
            // reflow; PR Review keeps its extra stable internal geometry.
            ui.set_navigation_drawer_expanded(expanded);
            platform::update_titlebar_navigation_controls(&ui);
        }
    });

    app.on_navigation_drawer_motion_settled(|expanded| {
        settings::save_navigation_drawer_expanded(expanded);
    });

    let app_for_modal_overlay = app.as_weak();
    app.on_modal_overlay_visibility_changed(move |_| {
        if let Some(ui) = app_for_modal_overlay.upgrade() {
            platform::update_titlebar_navigation_controls(&ui);
        }
    });

    // ========= Theme ========= //
    let app_weak = app.as_weak();
    let settings_page_for_theme = settings_page.clone();
    let message_page_for_theme = message_page.clone();
    AppState::get(&app).on_toggle_theme(move |mode| {
        if let Some(win) = app_weak.upgrade() {
            let app_state = AppState::get(&win);

            app_state.set_theme_mode(mode);
            settings::save_theme_mode(mode);
            set_auto_theme_watch_enabled(mode == THEME_MODE_AUTO);
            apply_native_chrome_for_mode(mode);

            // The Linux detector talks to the XDG Desktop Portal over D-Bus.
            // Resolve Auto away from the Slint event loop so a slow or missing
            // portal cannot stall the UI.
            #[cfg(target_os = "linux")]
            if mode == THEME_MODE_AUTO {
                let win_weak = win.as_weak();
                let settings_page = settings_page_for_theme.clone();
                let message_page = message_page_for_theme.clone();
                if let Err(error) = std::thread::Builder::new()
                    .name("system-theme-detect".to_string())
                    .spawn(move || {
                        let is_dark = get_effective_dark_mode(THEME_MODE_AUTO);
                        let _ = slint::invoke_from_event_loop(move || {
                            let Some(win) = win_weak.upgrade() else {
                                return;
                            };
                            let app_state = AppState::get(&win);
                            if app_state.get_theme_mode() != THEME_MODE_AUTO {
                                return;
                            }
                            app_state.set_effective_dark_mode(is_dark);
                            settings_page.sync_theme(THEME_MODE_AUTO, is_dark);
                            message_page.sync_theme(THEME_MODE_AUTO, is_dark);
                            set_title_bar_color(&win, is_dark);
                            set_terminal_dark_mode(&win, is_dark);
                            log::info!(
                                "[Theme] Changed to mode: {THEME_MODE_AUTO} (effective dark: {is_dark})"
                            );
                        });
                    })
                {
                    log::warn!("[Theme] Failed to start Linux theme detector: {error}");
                }
                return;
            }

            let is_dark = get_effective_dark_mode(mode);
            if mode == THEME_MODE_AUTO {
                app_state.set_effective_dark_mode(is_dark);
            }
            settings_page_for_theme.sync_theme(mode, is_dark);
            message_page_for_theme.sync_theme(mode, is_dark);
            set_title_bar_color(&win, is_dark);
            set_terminal_dark_mode(&win, is_dark);

            log::info!("[Theme] Changed to mode: {mode} (effective dark: {is_dark})");
        }
    });

    // ========= Auto Theme Mode ========= //
    // Seed Auto's resolved palette and install the live OS theme watcher.
    let initial_theme = loaded_settings.theme_mode;
    if initial_theme == THEME_MODE_AUTO {
        AppState::get(&app).set_effective_dark_mode(is_dark);
    }
    let app_weak_theme_watch = app.as_weak();
    let settings_page_for_theme_watch = settings_page.clone();
    let message_page_for_theme_watch = message_page.clone();
    install_system_theme_watch(move |is_dark| {
        let Some(win) = app_weak_theme_watch.upgrade() else {
            return;
        };
        let app_state = AppState::get(&win);
        if app_state.get_theme_mode() != THEME_MODE_AUTO {
            return;
        }
        app_state.set_effective_dark_mode(is_dark);
        settings_page_for_theme_watch.sync_theme(THEME_MODE_AUTO, is_dark);
        message_page_for_theme_watch.sync_theme(THEME_MODE_AUTO, is_dark);
        set_title_bar_color(&win, is_dark);
        set_terminal_dark_mode(&win, is_dark);
        log::info!("[Theme] System appearance changed (effective dark: {is_dark})");
    });

    // ========= App Info ========= //
    // Set application version number
    AppState::get(&app).set_version(env!("CARGO_PKG_VERSION").into());

    // ========= Notification ========= //
    // Single window: don't pass shared queue (use None)
    setup_notifications(&app, None, message_center.clone());
    memory_diagnostics::log_checkpoint("startup.notifications_ready");

    // If multi-window application, want to truly share queue:
    // let shared_queue = Arc::new(RwLock::new(SharedQueue::default()));
    // setup_notifications(&window1, Some(shared_queue.clone()));
    // setup_notifications(&window2, Some(shared_queue));

    // ========= Toast ========= //
    setup_toast(&app);

    // ========= HTTP ========= //
    // Bind error feedback only after Notification and Toast callbacks are ready.
    setup_http(&app);

    // ========= Network ========= //
    setup_network_monitor(&app);

    // ========= Dialog ========= //
    setup_dialog(&app);
    memory_diagnostics::log_checkpoint("startup.toast_http_network_dialog_ready");

    // ========= Project ========= //
    setup_project(&app);
    memory_diagnostics::log_checkpoint("startup.project_ready");

    // ========= Chat ========= //
    setup_chat(&app);
    memory_diagnostics::log_checkpoint("startup.chat_ready");

    // ========= Terminal Drawer ========= //
    setup_terminal(&app);
    memory_diagnostics::log_checkpoint("startup.terminal_ready");

    AppState::get(&app); // keep nearby state init ordering explicit
    let app_for_page_memory_diag = app.as_weak();
    let last_page_memory_diag = std::sync::Arc::new(std::sync::Mutex::new(
        None::<(std::time::Instant, Route, i32, i32, bool)>,
    ));
    let last_page_memory_diag_for_callback = last_page_memory_diag.clone();
    app.on_page_memory_diag(move |route, width_px, height_px, terminal_visible| {
        let ui = app_for_page_memory_diag.upgrade();
        let scale_factor = ui
            .as_ref()
            .map(|ui| ui.window().scale_factor())
            .unwrap_or(1.0);
        terminal::log_page_memory_diag(route, width_px, height_px, terminal_visible, scale_factor);
        if memory_diagnostics::diagnostics_enabled() {
            let now = std::time::Instant::now();
            let should_log = last_page_memory_diag_for_callback
                .lock()
                .map(|mut previous| {
                    let changed_context = previous.as_ref().is_none_or(
                        |(_, previous_route, _, _, previous_terminal_visible)| {
                            *previous_route != route
                                || *previous_terminal_visible != terminal_visible
                        },
                    );
                    let interval_elapsed = previous.as_ref().is_none_or(|(at, _, _, _, _)| {
                        now.duration_since(*at) >= std::time::Duration::from_secs(1)
                    });
                    if changed_context || interval_elapsed {
                        *previous = Some((now, route, width_px, height_px, terminal_visible));
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(true);
            if should_log {
                if let Some(ui) = ui.as_ref() {
                    memory_diagnostics::log_ui_models(ui, route);
                }
                log::info!(
                    "[Memory][UI] route={:?} logical_width={} logical_height={} scale_factor={:.2} physical_width={:.0} physical_height={:.0} terminal_visible={}",
                    route,
                    width_px,
                    height_px,
                    scale_factor,
                    width_px as f32 * scale_factor,
                    height_px as f32 * scale_factor,
                    terminal_visible,
                );
                memory_diagnostics::log_checkpoint("ui.layout_or_route_changed");
            }
        }
    });

    // ========= Application-wide Job Run ========= //
    jobs::setup_job_run(&app);

    // ========= Workflow ========= //
    setup_workflow(&app);
    memory_diagnostics::log_checkpoint("startup.workflow_ready");

    // ========= System Tray ========= //
    // Setup system tray icon with context menu
    // Using native macOS API implementation to avoid Objective-C class registration conflicts
    let app_weak_tray = app.as_weak();
    // Delay tray icon creation to ensure window is fully initialized
    // and to run on main thread (required on macOS)
    Timer::single_shot(std::time::Duration::from_millis(500), move || {
        if let Err(e) = setup_tray(app_weak_tray) {
            log::error!("Failed to setup tray icon: {}", e);
        }
    });

    // Example call - delay execution to ensure window is fully initialized
    // let app_weak = app.as_weak();
    // slint::invoke_from_event_loop(move || {
    //     if let Some(ui) = app_weak.upgrade() {
    //         toast::success("Operation successful!");
    //         toast::show(toast::ToastOptions {
    //             message: "This is a very long message...".to_string(),
    //             duration: 4000,
    //             position: "bottom".to_string(),
    //             overlay: true,
    //             ..Default::default()
    //         });
    //     }
    // }).ok();

    shutdown::init_signal_handlers();
    shutdown::register_pre_shutdown_task_with_priority("stop_auto_update", PRIORITY_HIGH, || {
        stop_auto_update();
    });
    shutdown::register_pre_shutdown_task_with_priority(
        "stop_memory_diagnostics",
        PRIORITY_HIGH,
        || {
            memory_diagnostics::stop_periodic_logging();
        },
    );
    shutdown::register_pre_shutdown_task_with_priority("shutdown_terminal", PRIORITY_HIGH, || {
        shutdown_terminal();
    });
    let message_center_for_shutdown = message_center.clone();
    shutdown::register_pre_shutdown_task_with_priority(
        "shutdown_message_center",
        PRIORITY_HIGH,
        move || message_center_for_shutdown.shutdown(),
    );
    shutdown::register_ui_shutdown_task_with_priority("remove_tray", PRIORITY_HIGH, || {
        tray::remove_tray();
    });
    settings_page.install(&app);
    message_page.install(&app);
    memory_diagnostics::log_checkpoint("startup.ui_bindings_complete");

    Ok(app)
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn main() {
    match single_instance::acquire_instance_lock() {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("[Usher] Another instance is already running, exiting this process.");
            return;
        }
        Err(e) => {
            eprintln!(
                "[Usher] Failed to acquire instance lock (continuing anyway): {}",
                e
            );
        }
    }

    let ui: MainWindow = ui().unwrap();
    // The winit window is created only once the event loop runs, so hiding
    // after show() is too late for the very first frame. Arm a one-shot hook
    // on the first render pass: at that point the native window exists and the
    // alpha change commits in the same transaction as the first frame.
    let ui_weak_first_frame = ui.as_weak();
    let first_frame_gate = std::cell::Cell::new(false);
    if let Err(error) = ui.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::BeforeRendering) && !first_frame_gate.get() {
            first_frame_gate.set(true);
            if let Some(win) = ui_weak_first_frame.upgrade() {
                set_startup_presentation_hidden(&win, true);
                install_titlebar_settings_button(&win);
            }
        }
    }) {
        log::warn!("[Startup] Rendering notifier unavailable: {error:?}");
    }
    // Prefer installing Menu vibrancy before the first ordered frame so the
    // translucent macOS sidebar never reveals desktop content at startup.
    install_titlebar_settings_button(&ui);
    ui.show().unwrap();
    // Keep the window invisible while native chrome is attached so the first
    // visible frame is the final shell rather than a staged transition.
    set_startup_presentation_hidden(&ui, true);
    memory_diagnostics::log_checkpoint("startup.window_shown");
    memory_diagnostics::start_periodic_logging();
    shortcuts::start_initialization(&ui);
    // The native handle may still be unavailable this early; retry below.
    install_titlebar_settings_button(&ui);

    // Reveal the window only after the native chrome is confirmed installed.
    // The native handle can lag the first event-loop turns, so a fixed delay
    // may fire before installation and expose staged startup states. After
    // readiness, wait a few extra ticks: a just-attached NSVisualEffectView
    // composites its first behind-window sample unblurred, and AppKit's
    // post-accessory title-bar layout resets the traffic lights; both must
    // settle while the window is still invisible. Each settle tick and the
    // reveal itself re-run the traffic-light alignment so the first visible
    // frame matches the accessory-button centerline. A deadline guarantees the
    // window always appears even if a native step failed.
    let reveal_timer = std::rc::Rc::new(Timer::default());
    let reveal_timer_handle = reveal_timer.clone();
    let ui_weak_reveal = ui.as_weak();
    let reveal_started = std::time::Instant::now();
    let reveal_settle_ticks = std::cell::Cell::new(0u32);
    reveal_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(30),
        move || {
            let Some(win) = ui_weak_reveal.upgrade() else {
                reveal_timer_handle.stop();
                return;
            };
            let deadline_passed =
                reveal_started.elapsed() >= std::time::Duration::from_millis(1200);
            if !startup_native_chrome_ready() && !deadline_passed {
                set_startup_presentation_hidden(&win, true);
                install_titlebar_settings_button(&win);
                return;
            }
            if !deadline_passed && reveal_settle_ticks.get() < 4 {
                reveal_settle_ticks.set(reveal_settle_ticks.get() + 1);
                prepare_startup_reveal(&win);
                return;
            }
            prepare_startup_reveal(&win);
            set_startup_presentation_hidden(&win, false);
            reveal_timer_handle.stop();
            log::info!(
                "[Startup] Window revealed (native chrome ready: {})",
                startup_native_chrome_ready()
            );
        },
    );
    slint::run_event_loop_until_quit().unwrap();
    shutdown::run_cleanup_once("event-loop-exit");
    drop(ui);
    log::info!("[Shutdown] MainWindow dropped, native resources released");
}

// #[cfg(target_os = "android")]
// #[unsafe(no_mangle)]
// fn android_main(android_app: slint::android::AndroidApp) {
//     slint::android::init(android_app).unwrap();
//     let ui = ui();
//     MaterialWindowAdapter::get(&ui).set_disable_hover(true);
//     ui.run().unwrap();
// }
