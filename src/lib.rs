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

mod constants;
use constants::shutdown_priority::PRIORITY_HIGH;

mod chat;
use chat::setup_chat;

mod ai;

mod app_lifecycle;
mod app_runtime;
mod memory_diagnostics;
mod platform;
use platform::{
    effective_dark_mode as get_effective_dark_mode, install_titlebar_settings_button,
    set_app_appearance, set_title_bar_color, setup_application_font, setup_pr_review_fonts,
};
mod terminal;

mod logger;
use logger::error_handing;
use logger::tracing_logger;
use logger::{setup_debug_logger, setup_tracing_logger};

mod router;
use router::{Router, route_for_navigation_index};

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

fn ui() -> Result<MainWindow> {
    memory_diagnostics::initialize_baseline();
    log::info!("Usher v{} starting...", env!("CARGO_PKG_VERSION"));

    // ========= Pre-Window Settings ========= //
    // Load settings BEFORE creating window to set app appearance early (prevents theme flash)
    let loaded_settings = settings::load_settings();

    // Set app-level appearance before window is created (prevents flash on macOS)
    let is_dark = get_effective_dark_mode(loaded_settings.theme_mode);
    set_app_appearance(is_dark);

    // Now create the MainWindow (will inherit app appearance)
    // Using native macOS title bar
    let app = MainWindow::new().context("Failed to create MainWindow")?;
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

    // ========= Git Tokens ========= //
    let app_weak_git_tokens = app.as_weak();
    AppState::get(&app).on_save_git_tokens(
        move |github_token, gitlab_token, gitee_token, gitee_enterprise_token| {
            settings::save_git_platform_tokens(
                github_token.as_str(),
                gitlab_token.as_str(),
                gitee_token.as_str(),
                gitee_enterprise_token.as_str(),
            );
            if let Some(win) = app_weak_git_tokens.upgrade() {
                win.set_settings_git_tokens_saved(true);
                let win_weak = win.as_weak();
                Timer::single_shot(std::time::Duration::from_secs(2), move || {
                    if let Some(win) = win_weak.upgrade() {
                        win.set_settings_git_tokens_saved(false);
                    }
                });
            }
            log::info!(
                "[Settings] Git tokens updated: github={}, gitlab={}, gitee_public={}, gitee_enterprise={}",
                if github_token.is_empty() { "empty" } else { "set" },
                if gitlab_token.is_empty() { "empty" } else { "set" },
                if gitee_token.is_empty() { "empty" } else { "set" },
                if gitee_enterprise_token.is_empty() {
                    "empty"
                } else {
                    "set"
                }
            );
        },
    );

    // ========= Code Hosting API Configuration ========= //
    let app_weak_git_hosting_config = app.as_weak();
    AppState::get(&app).on_save_git_hosting_config(
        move |github_api_base_url,
              github_enterprise,
              gitlab_api_base_url,
              gitlab_enterprise,
              gitee_api_base_url,
              gitee_enterprise| {
            settings::save_git_hosted_repository_configs(
                github_api_base_url.as_str(),
                github_enterprise.as_str(),
                gitlab_api_base_url.as_str(),
                gitlab_enterprise.as_str(),
                gitee_api_base_url.as_str(),
                gitee_enterprise.as_str(),
            );
            if let Some(win) = app_weak_git_hosting_config.upgrade() {
                win.set_settings_git_hosting_config_saved(true);
                let win_weak = win.as_weak();
                Timer::single_shot(std::time::Duration::from_secs(2), move || {
                    if let Some(win) = win_weak.upgrade() {
                        win.set_settings_git_hosting_config_saved(false);
                    }
                });
            }
            log::info!(
                "[Settings] Code hosting API configuration updated: github={}, gitlab={}, gitee={}",
                if github_api_base_url.is_empty() {
                    "empty"
                } else {
                    "set"
                },
                if gitlab_api_base_url.is_empty() {
                    "empty"
                } else {
                    "set"
                },
                if gitee_api_base_url.is_empty() {
                    "empty"
                } else {
                    "set"
                },
            );
        },
    );

    // ========= AI Agent Settings ========= //
    let app_weak_ai_agent_save = app.as_weak();
    AppState::get(&app).on_save_ai_agent_settings(move |provider, base_url, api_key, model| {
        settings::save_ai_agent_settings(
            provider.as_str(),
            base_url.as_str(),
            api_key.as_str(),
            model.as_str(),
        );
        if let Some(win) = app_weak_ai_agent_save.upgrade() {
            win.set_settings_ai_agent_saved(true);
            let win_weak = win.as_weak();
            Timer::single_shot(std::time::Duration::from_secs(2), move || {
                if let Some(win) = win_weak.upgrade() {
                    win.set_settings_ai_agent_saved(false);
                }
            });
        }
        log::info!(
            "[Settings] AI Agent settings updated: provider={}, base_url={}, api_key={}, model={}",
            provider,
            if base_url.is_empty() { "empty" } else { "set" },
            if api_key.is_empty() { "empty" } else { "set" },
            if model.is_empty() { "empty" } else { "set" },
        );
    });

    let app_weak_ai_agent_test = app.as_weak();
    AppState::get(&app).on_test_ai_agent_connection(move |base_url, api_key, model| {
        let settings = settings::AiAgentSettings {
            provider: "custom".to_string(),
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        };
        let ui_weak = app_weak_ai_agent_test.clone();
        app_runtime::shared_runtime().spawn(async move {
            let result = ai::agent::test_connection(&settings).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                ui.set_settings_ai_agent_testing(false);
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
                            position: "top".into(),
                            icon: "".into(),
                            overlay: false,
                            close_on_click: true,
                            loading_type: "".into(),
                            width: "auto".into(),
                        });
                    }
                    Err(error) => {
                        // HTTP failures already use the centralized Toast feedback configured
                        // by the shared HTTP module. Local validation failures need a UI message.
                        if matches!(
                            error.kind,
                            http::HttpErrorKind::Request | http::HttpErrorKind::Build
                        ) {
                            ToastManager::get(&ui).invoke_error(ToastOptions {
                                message: error.message.into(),
                                duration: 3_500,
                                position: "top".into(),
                                icon: "".into(),
                                overlay: false,
                                close_on_click: true,
                                loading_type: "".into(),
                                width: "auto".into(),
                            });
                        }
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
                let ui_weak = ui.as_weak();
                Timer::single_shot(std::time::Duration::from_millis(1), move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_settings_ai_agent_provider_index(index);
                    }
                });
            }
        }
    });

    let router_for_back = router.clone();
    let app_for_back = app.as_weak();
    app.on_navigate_back(move || {
        if let Some(state) = router_for_back.back() {
            if let Some(ui) = app_for_back.upgrade() {
                ui.invoke_chat_route_visible_changed(state.chat_route_visible);
            }
        }
    });

    // ========= Theme ========= //
    let app_weak = app.as_weak();
    AppState::get(&app).on_toggle_theme(move |mode| {
        if let Some(win) = app_weak.upgrade() {
            let app_state = AppState::get(&win);

            // Update UI state
            app_state.set_theme_mode(mode);

            // Save to config file
            settings::save_theme_mode(mode);

            // Get effective dark mode (resolves Auto to actual light/dark)
            let is_dark = get_effective_dark_mode(mode);

            // For Auto mode, set the effective-dark-mode property so Slint can update Palette
            if mode == 2 {
                app_state.set_effective_dark_mode(is_dark);
            }

            // Set title bar color
            set_title_bar_color(&win, is_dark);
            set_terminal_dark_mode(&win, is_dark);

            log::info!(
                "[Theme] Changed to mode: {} (effective dark: {})",
                mode,
                is_dark
            );
        }
    });

    // ========= Auto Theme Mode ========= //
    // For Auto mode, set effective-dark-mode so Slint can update Palette
    let initial_theme = loaded_settings.theme_mode;
    if initial_theme == 2 {
        let is_dark = get_effective_dark_mode(initial_theme);
        AppState::get(&app).set_effective_dark_mode(is_dark);
    }

    // ========= App Info ========= //
    // Set application version number
    AppState::get(&app).set_version(env!("CARGO_PKG_VERSION").into());

    // ========= Notification ========= //
    // Single window: don't pass shared queue (use None)
    setup_notifications(&app, None);
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
    shutdown::register_ui_shutdown_task_with_priority("remove_tray", PRIORITY_HIGH, || {
        tray::remove_tray();
    });
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
    ui.show().unwrap();
    memory_diagnostics::log_checkpoint("startup.window_shown");
    memory_diagnostics::start_periodic_logging();
    install_titlebar_settings_button(&ui);
    let ui_weak_titlebar_retry_1 = ui.as_weak();
    Timer::single_shot(std::time::Duration::from_millis(300), move || {
        if let Some(win) = ui_weak_titlebar_retry_1.upgrade() {
            install_titlebar_settings_button(&win);
        }
    });
    let ui_weak_titlebar_retry_2 = ui.as_weak();
    Timer::single_shot(std::time::Duration::from_millis(900), move || {
        if let Some(win) = ui_weak_titlebar_retry_2.upgrade() {
            install_titlebar_settings_button(&win);
        }
    });
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
