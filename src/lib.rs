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

mod diagnostics;
mod persistence_benchmark;

mod tray;
use tray::setup_tray;

mod update;
use update::{setup_updater, start_auto_update, stop_auto_update};

mod network;
use network::setup_network_monitor;

mod http;
use http::setup_http;

mod i18n;
mod settings;
use settings::setup_settings;
mod settings_page;
use settings_page::SettingsPageBridge;
mod message_center;
use message_center::MessagePageBridge;

mod constants;
use constants::shutdown_priority::PRIORITY_HIGH;

mod chat;
use chat::setup_chat;
pub mod artifact_preview;

#[cfg(feature = "ai-code-review")]
mod agent_response;
mod ai;
#[cfg(feature = "ai-code-review")]
mod ai_review;

mod app_lifecycle;
mod app_paths;
mod app_runtime;
mod memory_diagnostics;
mod platform;
mod shortcuts;
mod sidebar_motion;
mod startup;
mod transient_input;
use platform::{
    THEME_MODE_AUTO, apply_native_appearance_for_mode, apply_native_chrome_for_mode,
    effective_dark_mode as get_effective_dark_mode, install_memory_benchmark_window_control,
    install_system_theme_watch, install_titlebar_settings_button, play_ui_click,
    prepare_startup_reveal, set_auto_theme_watch_enabled, set_startup_presentation_hidden,
    set_title_bar_color, setup_application_font, setup_pr_review_fonts,
    startup_native_chrome_ready,
};
#[doc(hidden)]
pub use transient_input::install_for_window as install_transient_input_bridge;

mod terminal;

mod logger;
use logger::error_handing;
use logger::tracing_logger;
use logger::{debug_logger, setup_debug_logger, setup_tracing_logger};

mod router;
use router::{Router, route_for_navigation_index};

mod hooks;
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

mod settings_feedback;
use settings_feedback::{SettingsSaveTarget, save_settings_with_feedback};

fn ui(
    sidebar_motion_mode: sidebar_motion::SidebarMotionMode,
    startup_timeline: &startup::StartupTimeline,
) -> Result<MainWindow> {
    let loaded_settings = settings::load_settings();
    startup_timeline.checkpoint("settings_loaded");
    let benchmark_offline = app_paths::benchmark_offline_mode();
    memory_diagnostics::set_logging_enabled(loaded_settings.diagnostics.memory_diagnostics_logging);
    memory_diagnostics::initialize_baseline();
    log::info!("Usher v{} starting...", env!("CARGO_PKG_VERSION"));

    // ========= Pre-Window Settings ========= //
    // Load settings BEFORE creating window to set app appearance early (prevents theme flash)

    // Set app-level appearance before window is created (prevents flash on macOS).
    // Auto clears any forced AppKit appearance so the process can track the OS.
    let is_dark = apply_native_appearance_for_mode(loaded_settings.theme_mode);
    set_auto_theme_watch_enabled(loaded_settings.theme_mode == THEME_MODE_AUTO);
    startup_timeline.checkpoint("native_appearance_applied");

    // Now create the MainWindow (will inherit app appearance)
    // Using native macOS title bar
    let app = MainWindow::new().context("Failed to create MainWindow")?;
    install_floating_overlay_positioning(&app);
    startup_timeline.checkpoint("main_window_created");
    i18n::initialize(&app, &loaded_settings.language)?;
    startup_timeline.checkpoint("language_initialized");
    app.set_sidebar_motion_display_linked(sidebar_motion_mode.is_display_linked());
    app.on_advance_sidebar_motion(sidebar_motion::advance);
    let settings_page = SettingsPageBridge::default();
    let ai_agent_settings =
        ai::settings_state::AiAgentSettingsFacade::open(loaded_settings.ai.agent.clone())
            .context("Failed to start AI Agent settings state")?;
    settings_page.configure_ai_agent_settings(ai_agent_settings.clone());
    let (message_center, message_page) = MessagePageBridge::open_deferred(startup_timeline)?;
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
    setup_debug_logger(
        &app,
        loaded_settings.diagnostics.application_console_logging,
        loaded_settings.diagnostics.application_file_logging,
    )
    .unwrap_or_else(|e| eprintln!("Debug Logger init failed: {}", e)); // eprintln here since logger not yet initialized
    startup_timeline.logger_ready();
    startup_timeline.checkpoint("debug_logger_ready");
    log::info!("[SidebarMotion] mode={}", sidebar_motion_mode.label());
    memory_diagnostics::log_checkpoint("startup.debug_logger_ready");

    // ========= Trace Logger Initialization ========= //
    setup_tracing_logger(&app, loaded_settings.diagnostics.tracing_logger_active)
        .unwrap_or_else(|e| eprintln!("Tracing Logger init failed: {}", e));
    memory_diagnostics::log_checkpoint("startup.tracing_logger_ready");

    // ========= Settings ========= //
    // Apply loaded settings to AppState
    setup_settings(&app, &loaded_settings);
    let _artifact_preview_handle = artifact_preview::setup(&app, &loaded_settings.preview);
    #[cfg(feature = "test-fixtures")]
    artifact_preview::setup_fixtures(&app, _artifact_preview_handle.clone());
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
    // setup_updater must be called AFTER setup_settings so it reads the correct auto_update value.
    // The Settings page bridge is installed later; the updater only needs the handle so manual
    // check completion can clear the isolated page busy flag.
    let updater_ctx = setup_updater(&app, &settings_page)?;
    memory_diagnostics::log_checkpoint("startup.updater_ready");

    // ========= Auto Update ========= //
    // Initialize auto-update based on loaded settings
    if loaded_settings.update.auto_update && !benchmark_offline {
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
        if enabled && !benchmark_offline {
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

    AppState::get(&app).on_set_update_channel(move |channel| {
        settings::save_update_channel(channel.as_str());
        log::info!("[Settings] Update channel changed to: {channel}");
    });

    let app_weak_language = app.as_weak();
    let settings_page_for_language = settings_page.clone();
    let message_page_for_language = message_page.clone();
    AppState::get(&app).on_set_language(move |preference| {
        let Some(app) = app_weak_language.upgrade() else {
            return;
        };
        match i18n::apply_preference(preference.as_str()) {
            Ok(canonical) => {
                i18n::sync_main_window(&app, canonical);
                settings_page_for_language.sync_language(canonical);
                message_page_for_language.sync_language(canonical);
                workflow::sync_language(&app);
                artifact_preview::sync_language(&app);
                terminal::sync_language(&app);
                project::sync_language(&app);
                hooks::sync_job_language(&app);
                chat::sync_language(&app);
                #[cfg(feature = "ai-code-review")]
                ai_review::sync_language(&app);
                settings::save_language(canonical);
                log::info!(
                    "[I18n] Language preference changed to {canonical}; active locale={}",
                    usher_i18n::active_locale().id
                );
            }
            Err(error) => {
                log::error!("[I18n] Failed to change language: {error:#}");
                let current = settings::get_settings_manager().get().language;
                i18n::sync_main_window(&app, &current);
                settings_page_for_language.sync_language(&current);
                message_page_for_language.sync_language(&current);
            }
        }
    });

    // ========= Tracing File Logs ========= //
    AppState::get(&app).on_toggle_application_console_logging(move |enabled| {
        settings::save_application_console_logging(enabled);
        debug_logger::set_console_output_enabled(enabled);
        log::info!("[Settings] Console logging changed to: {}", enabled);
    });

    AppState::get(&app).on_toggle_application_file_logging(move |enabled| {
        settings::save_application_file_logging(enabled);
        debug_logger::set_file_output_enabled(enabled);
        log::info!(
            "[Settings] Application file logging changed to: {}",
            enabled
        );
    });

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

    AppState::get(&app).on_toggle_memory_diagnostics_logging(move |enabled| {
        settings::save_memory_diagnostics_logging(enabled);
        memory_diagnostics::set_logging_enabled(enabled);
        log::info!("[Settings] Memory diagnostics changed to: {}", enabled);
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
                "settings.feedback.tokens_saved",
                "settings.feedback.tokens_save_failed",
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
                "settings.feedback.hosting_api_saved",
                "settings.feedback.hosting_api_save_failed",
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
    let ai_agent_settings_for_save = ai_agent_settings.clone();
    AppState::get(&app).on_save_ai_agent_settings(move |_, _, _, _, _| {
        let ai_agent_settings = ai_agent_settings_for_save.clone();
        save_settings_with_feedback(
            app_weak_ai_agent_save.clone(),
            settings_page_for_ai_save.clone(),
            SettingsSaveTarget::AiAgent,
            "AI Agent settings",
            "settings.feedback.ai_agent_saved",
            "settings.feedback.ai_agent_save_failed",
            move || ai_agent_settings.flush(),
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
            reply_language: usher_i18n::AUTO_LANGUAGE.to_string(),
        };
        let ui_weak = app_weak_ai_agent_test.clone();
        let settings_page = settings_page_for_ai_test.clone();
        // Operation dual-write begin: host + isolated Settings page.
        if let Some(ui) = ui_weak.upgrade() {
            settings_page::mirror_ai_agent_testing(&ui, &settings_page, true);
        }
        app_runtime::shared_runtime().spawn(async move {
            let result = ai::agent::test_connection(&settings).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    settings_page.set_ai_agent_testing(false);
                    return;
                };
                settings_page::mirror_ai_agent_testing(&ui, &settings_page, false);
                match result {
                    Ok(result) => {
                        let message = if result.model_listed {
                            i18n::tr("settings.feedback.ai_agent_connected")
                        } else {
                            i18n::tr("settings.feedback.ai_agent_connected_model_unlisted")
                        };
                        ToastManager::get(&ui).invoke_success(ToastOptions {
                            message: message.as_ref().into(),
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
                            message: error.localized_user_message().into(),
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
    let lifecycle_hooks = app_lifecycle::hooks(app.get_current_route());
    let settings_page_for_lifecycle = settings_page.clone();
    lifecycle_hooks.on_transition(std::rc::Rc::new(move |window, event| {
        if event.current == Route::Settings {
            settings_page_for_lifecycle.reset_drafts_from_committed(window);
        }
    }));
    let router = Router::new(app.as_weak(), lifecycle_hooks);

    let router_for_nav_index = router.clone();
    app.on_navigate_from_nav_index(move |item_index| {
        if let Some(route) = route_for_navigation_index(item_index) {
            router_for_nav_index.navigate(route);
        }
    });

    // Connect navigate callback in slint
    let router_clone = router.clone();
    app.on_navigate(move |route| {
        router_clone.navigate(route);
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
            router_for_back.back();
        }
    });

    let router_for_feature_back = router.clone();
    app.on_router_back(move || {
        router_for_feature_back.back();
    });

    let router_for_forward = router.clone();
    let app_for_forward = app.as_weak();
    app.on_navigate_forward(move || {
        if let Some(ui) = app_for_forward.upgrade() {
            if AppState::get(&ui).get_ui_sound_effects() {
                play_ui_click();
            }
            router_for_forward.forward();
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
                            artifact_preview::sync_detached_effective_theme(is_dark);
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
            artifact_preview::sync_detached_effective_theme(is_dark);
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
        artifact_preview::sync_detached_effective_theme(is_dark);
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
        artifact_preview::sync_detached_effective_theme(is_dark);
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
    if !benchmark_offline {
        setup_network_monitor(&app);
    }

    // ========= Dialog ========= //
    setup_dialog(&app);
    memory_diagnostics::log_checkpoint("startup.toast_http_network_dialog_ready");

    // ========= Project ========= //
    let project_controller = setup_project(&app);
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
            if let Some(ui) = ui.as_ref() {
                artifact_preview::maybe_mount_memory_benchmark_fixture(ui, route);
            }
            memory_diagnostics::record_ui_context(
                route,
                width_px,
                height_px,
                scale_factor,
                terminal_visible,
            );
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
    hooks::setup(&app, &settings_page, message_center.clone())?;

    // ========= Workflow ========= //
    setup_workflow(&app);
    memory_diagnostics::log_checkpoint("startup.workflow_ready");

    #[cfg(feature = "ai-code-review")]
    let ai_review_controller = ai_review::setup_ai_review(
        &app,
        &settings_page,
        message_center.clone(),
        _artifact_preview_handle.clone(),
    );
    #[cfg(feature = "ai-code-review")]
    memory_diagnostics::log_checkpoint("startup.ai_review_ready");

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
    shutdown::register_pre_shutdown_task_with_priority(
        "shutdown_ai_agent_settings",
        PRIORITY_HIGH,
        move || ai_agent_settings.shutdown(),
    );
    shutdown::register_pre_shutdown_task_with_priority(
        "shutdown_project_state",
        PRIORITY_HIGH,
        move || project_controller.shutdown(),
    );
    #[cfg(feature = "ai-code-review")]
    shutdown::register_pre_shutdown_task_with_priority(
        "shutdown_ai_review",
        PRIORITY_HIGH,
        move || ai_review_controller.shutdown(),
    );
    let message_center_for_shutdown = message_center.clone();
    shutdown::register_pre_shutdown_task_with_priority(
        "shutdown_message_center",
        PRIORITY_HIGH,
        move || message_center_for_shutdown.shutdown(),
    );
    // Normal priority flushes preferences after the high-priority feature workers stop.
    shutdown::register_pre_shutdown_task("flush_settings", settings::shutdown_saves);
    shutdown::register_ui_shutdown_task_with_priority("remove_tray", PRIORITY_HIGH, || {
        tray::remove_tray();
    });
    shutdown::register_ui_shutdown_task_with_priority(
        "shutdown_native_integrations",
        PRIORITY_HIGH,
        platform::shutdown_native_integrations,
    );
    settings_page.install(&app);
    message_page.install(&app);
    persistence_benchmark::setup(&app, &message_center);
    memory_diagnostics::log_checkpoint("startup.ui_bindings_complete");
    startup_timeline.checkpoint("ui_bindings_complete");

    Ok(app)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn main() {
    let Some((startup_timeline, run_guard)) = startup::initialize_process() else {
        return;
    };

    let sidebar_motion_mode = sidebar_motion::SidebarMotionMode::from_environment();
    let ui: MainWindow = ui(sidebar_motion_mode, &startup_timeline)
        .unwrap_or_else(|error| startup::exit_failure("Application initialization failed", &error));
    diagnostics::setup(&ui, run_guard);
    startup_timeline.checkpoint("diagnostics_ready");
    let sidebar_motion_trace_path =
        std::env::var_os("USHER_SIDEBAR_MOTION_TRACE").map(std::path::PathBuf::from);
    let sidebar_motion_trace_started = std::time::Instant::now();
    let sidebar_motion_trace_samples = std::rc::Rc::new(std::cell::RefCell::new(Vec::<
        sidebar_motion::TraceSample,
    >::new()));
    // The winit window is created only once the event loop runs, so hiding
    // after show() is too late for the very first frame. Arm a one-shot hook
    // on the first render pass: at that point the native window exists and the
    // alpha change commits in the same transaction as the first frame.
    let ui_weak_first_frame = ui.as_weak();
    let first_frame_gate = std::cell::Cell::new(false);
    let first_frame_completed = std::cell::Cell::new(false);
    let timeline_for_render = startup_timeline.clone();
    let sidebar_motion_trace_enabled = sidebar_motion_trace_path.is_some();
    let sidebar_motion_trace_samples_for_render = sidebar_motion_trace_samples.clone();
    let chat_frame_trace = chat::performance::FrameTrace::from_environment();
    artifact_preview::performance::initialize();
    let chat_frame_trace_for_render = chat_frame_trace.clone();
    if let Err(error) = ui.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::BeforeRendering) {
            if let Some(win) = ui_weak_first_frame.upgrade() {
                if !first_frame_gate.get() {
                    first_frame_gate.set(true);
                    timeline_for_render.checkpoint("first_frame_before_rendering");
                    set_startup_presentation_hidden(&win, true);
                    install_titlebar_settings_button(&win);
                }
                if sidebar_motion_trace_enabled {
                    sidebar_motion_trace_samples_for_render.borrow_mut().push(
                        sidebar_motion::TraceSample {
                            elapsed_microseconds: sidebar_motion_trace_started
                                .elapsed()
                                .as_micros(),
                            progress: win.get_sidebar_motion_observed_progress(),
                            target_expanded: win.get_sidebar_motion_target_expanded(),
                            active: win.get_sidebar_motion_active(),
                        },
                    );
                }
                chat_frame_trace_for_render
                    .borrow_mut()
                    .before_rendering(&win);
                artifact_preview::performance::before_main_rendering(&win);
            }
        } else if matches!(state, slint::RenderingState::AfterRendering) {
            if !first_frame_completed.replace(true) {
                timeline_for_render.checkpoint("first_frame_after_rendering");
                startup::mark_first_frame_rendered();
            }
            chat_frame_trace_for_render.borrow_mut().after_rendering();
            artifact_preview::performance::after_main_rendering();
        }
    }) {
        log::warn!("[Startup] Rendering notifier unavailable: {error:?}");
    }
    // Prefer installing Menu vibrancy before the first ordered frame so the
    // translucent macOS sidebar never reveals desktop content at startup.
    install_titlebar_settings_button(&ui);
    if let Err(error) = ui.show() {
        startup::exit_failure("The main window could not be shown", &error);
    }
    startup_timeline.checkpoint("window_show_requested");
    let _activation_listener = match single_instance::start_activation_listener(&ui) {
        Ok(listener) => Some(listener),
        Err(error) => {
            log::error!("[SingleInstance] Activation listener unavailable: {error:#}");
            None
        }
    };
    // The production backend creates its real window adapter during show().
    // macOS uses its existing application-local event monitor because native
    // StyledText consumes the Slint event; other backends use the generic hook.
    #[cfg(not(target_os = "macos"))]
    if let Err(error) = transient_input::install(&ui) {
        startup::exit_failure("The native input bridge could not be installed", &error);
    }
    install_memory_benchmark_window_control(&ui);
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
    if let Err(error) = slint::run_event_loop_until_quit() {
        startup::exit_failure("The application event loop stopped unexpectedly", &error);
    }
    if let Some(path) = sidebar_motion_trace_path {
        if let Err(error) = sidebar_motion::write_trace(
            &path,
            sidebar_motion_mode,
            &sidebar_motion_trace_samples.borrow(),
        ) {
            log::error!("[SidebarMotion] {error:#}");
        } else {
            log::info!(
                "[SidebarMotion] wrote {} render samples to {}",
                sidebar_motion_trace_samples.borrow().len(),
                path.display()
            );
        }
    }
    match chat_frame_trace.borrow().write() {
        Ok(Some((sample_count, path))) => {
            log::info!(
                "[Chat::Performance] wrote {sample_count} render samples to {}",
                path.display()
            );
        }
        Ok(None) => {}
        Err(error) => log::error!("[Chat::Performance] failed to write trace: {error}"),
    }
    match artifact_preview::performance::write() {
        Ok(Some((sample_count, path))) => {
            log::info!(
                "[ArtifactPreview::Performance] wrote {sample_count} render samples to {}",
                path.display()
            );
        }
        Ok(None) => {}
        Err(error) => {
            log::error!("[ArtifactPreview::Performance] failed to write trace: {error}")
        }
    }
    app_runtime::shared_runtime().block_on(shutdown::drain_async_tasks());
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
