#![allow(unexpected_cfgs)]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use anyhow::{Context, Result};

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

use slint::{CloseRequestResponse, ComponentHandle, Global, Model, Timer};

// Must be called before module declarations so generated types are available in all modules
slint::include_modules!();

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

mod app_runtime;
mod memory_diagnostics;
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

fn log_ui_model_memory(ui: &MainWindow, route: Route) {
    if !memory_diagnostics::diagnostics_enabled() {
        return;
    }

    let chat = ui.get_chat_messages();
    let mut chat_text_bytes = 0usize;
    let mut chat_blocks = 0usize;
    let mut chat_inline_lines = 0usize;
    let mut chat_spans = 0usize;
    for index in 0..chat.row_count() {
        let Some(message) = chat.row_data(index) else {
            continue;
        };
        chat_text_bytes = chat_text_bytes
            .saturating_add(message.content.len())
            .saturating_add(message.rendered_content.len())
            .saturating_add(message.timestamp.len());
        chat_blocks = chat_blocks.saturating_add(message.markdown_blocks.row_count());
        for block_index in 0..message.markdown_blocks.row_count() {
            let Some(block) = message.markdown_blocks.row_data(block_index) else {
                continue;
            };
            chat_text_bytes = chat_text_bytes.saturating_add(block.text.len());
            chat_inline_lines = chat_inline_lines.saturating_add(block.inline_lines.row_count());
            for line_index in 0..block.inline_lines.row_count() {
                let Some(line) = block.inline_lines.row_data(line_index) else {
                    continue;
                };
                chat_spans = chat_spans.saturating_add(line.spans.row_count());
                for span_index in 0..line.spans.row_count() {
                    if let Some(span) = line.spans.row_data(span_index) {
                        chat_text_bytes = chat_text_bytes
                            .saturating_add(span.text.len())
                            .saturating_add(span.href.len());
                    }
                }
            }
        }
    }

    let projects = ui.get_project_items();
    let mut project_text_bytes = 0usize;
    let mut project_tags = 0usize;
    for index in 0..projects.row_count() {
        let Some(project) = projects.row_data(index) else {
            continue;
        };
        project_text_bytes = project_text_bytes
            .saturating_add(project.project_name.len())
            .saturating_add(project.project_alias.len())
            .saturating_add(project.local_repo.len())
            .saturating_add(project.remote_repo.len())
            .saturating_add(project.logo.len())
            .saturating_add(project.project_tags_text.len());
        project_tags = project_tags.saturating_add(project.project_tags.row_count());
        for tag_index in 0..project.project_tags.row_count() {
            if let Some(tag) = project.project_tags.row_data(tag_index) {
                project_text_bytes = project_text_bytes.saturating_add(tag.len());
            }
        }
    }

    let pull_requests = ui.get_workflow_pr_list_items();
    let mut pull_request_text_bytes = 0usize;
    for index in 0..pull_requests.row_count() {
        if let Some(item) = pull_requests.row_data(index) {
            pull_request_text_bytes = pull_request_text_bytes
                .saturating_add(item.number.len())
                .saturating_add(item.title.len())
                .saturating_add(item.author.len())
                .saturating_add(item.source_branch.len())
                .saturating_add(item.target_branch.len())
                .saturating_add(item.updated_at.len())
                .saturating_add(item.web_url.len());
        }
    }

    let tag_steps = ui.get_workflow_tag_steps();
    let mut tag_step_text_bytes = 0usize;
    for index in 0..tag_steps.row_count() {
        if let Some(step) = tag_steps.row_data(index) {
            tag_step_text_bytes = tag_step_text_bytes
                .saturating_add(step.name.len())
                .saturating_add(step.status.len())
                .saturating_add(step.detail.len())
                .saturating_add(step.log.len());
        }
    }

    let terminal_lines = ui.get_shell_drawer_lines();
    let mut terminal_spans = 0usize;
    let mut terminal_text_bytes = 0usize;
    for line_index in 0..terminal_lines.row_count() {
        let Some(line) = terminal_lines.row_data(line_index) else {
            continue;
        };
        terminal_spans = terminal_spans.saturating_add(line.spans.row_count());
        for span_index in 0..line.spans.row_count() {
            if let Some(span) = line.spans.row_data(span_index) {
                terminal_text_bytes = terminal_text_bytes.saturating_add(span.text.len());
            }
        }
    }

    log::info!(
        "[Memory][UI Models] route={:?} chat_rows={} chat_blocks={} chat_inline_lines={} chat_spans={} chat_text_bytes={} project_rows={} project_tags={} project_text_bytes={} pr_list_rows={} pr_list_text_bytes={} pr_review_files={} pr_review_tree_items={} pr_review_extension_filters={} pr_review_lines={} tag_steps={} tag_step_text_bytes={} terminal_lines={} terminal_spans={} terminal_text_bytes={} terminal_output_bytes={}",
        route,
        chat.row_count(),
        chat_blocks,
        chat_inline_lines,
        chat_spans,
        chat_text_bytes,
        projects.row_count(),
        project_tags,
        project_text_bytes,
        pull_requests.row_count(),
        pull_request_text_bytes,
        ui.get_workflow_pr_review_files().row_count(),
        ui.get_workflow_pr_review_tree_items().row_count(),
        ui.get_workflow_pr_review_extension_filters().row_count(),
        ui.get_workflow_pr_review_lines().row_count(),
        tag_steps.row_count(),
        tag_step_text_bytes,
        terminal_lines.row_count(),
        terminal_spans,
        terminal_text_bytes,
        ui.get_shell_drawer_output().len(),
    );
}

#[cfg(target_os = "macos")]
static TITLEBAR_SETTINGS_UI_WEAK: std::sync::OnceLock<slint::Weak<MainWindow>> =
    std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
static mut TITLEBAR_SETTINGS_DELEGATE: *mut objc::runtime::Object = std::ptr::null_mut();

#[cfg(target_os = "macos")]
static mut TITLEBAR_SETTINGS_INSTALLED: bool = false;

#[cfg(target_os = "macos")]
static WINDOW_INTERACTION_MOVE_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "macos")]
static WINDOW_IS_INTERACTING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
pub fn is_window_interacting() -> bool {
    use std::sync::atomic::Ordering;
    WINDOW_IS_INTERACTING.load(Ordering::SeqCst)
}

#[cfg(not(target_os = "macos"))]
pub fn is_window_interacting() -> bool {
    false
}

/// Set application-level appearance on macOS (called BEFORE window is shown)
/// This prevents the flash of wrong theme on startup
#[cfg(target_os = "macos")]
fn set_app_appearance(is_dark: bool) {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use std::ffi::CString;

    unsafe {
        let ns_app_class = match Class::get("NSApplication") {
            Some(cls) => cls,
            None => return,
        };

        let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];
        if ns_app.is_null() {
            return;
        }

        let ns_appearance_class = match Class::get("NSAppearance") {
            Some(cls) => cls,
            None => return,
        };

        let ns_string_class = match Class::get("NSString") {
            Some(cls) => cls,
            None => return,
        };

        let appearance_name = if is_dark {
            "NSAppearanceNameDarkAqua"
        } else {
            "NSAppearanceNameAqua"
        };

        if let Ok(c_str) = CString::new(appearance_name) {
            let name_obj: *mut Object =
                msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
            if !name_obj.is_null() {
                let appearance: *mut Object =
                    msg_send![ns_appearance_class, appearanceNamed: name_obj];
                if !appearance.is_null() {
                    let _: () = msg_send![ns_app, setAppearance: appearance];
                    log::info!(
                        "[Theme] Set app appearance to: {}",
                        if is_dark { "dark" } else { "light" }
                    );
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_app_appearance(_is_dark: bool) {
    // Other platforms not supported yet
}

/// Detect system theme preference on macOS
/// Returns true if system is in dark mode, false for light mode
#[cfg(target_os = "macos")]
fn detect_system_dark_mode() -> bool {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use std::ffi::CString;

    unsafe {
        // Get NSUserDefaults.standardUserDefaults
        let ns_user_defaults_class = match Class::get("NSUserDefaults") {
            Some(cls) => cls,
            None => return false,
        };

        let user_defaults: *mut Object = msg_send![ns_user_defaults_class, standardUserDefaults];
        if user_defaults.is_null() {
            return false;
        }

        // Get "AppleInterfaceStyle" key
        let ns_string_class = match Class::get("NSString") {
            Some(cls) => cls,
            None => return false,
        };

        let key_cstr = match CString::new("AppleInterfaceStyle") {
            Ok(s) => s,
            Err(_) => return false,
        };

        let key: *mut Object = msg_send![ns_string_class, stringWithUTF8String: key_cstr.as_ptr()];
        if key.is_null() {
            return false;
        }

        // Get the value
        let value: *mut Object = msg_send![user_defaults, stringForKey: key];
        if value.is_null() {
            // If nil, system is in light mode
            return false;
        }

        // Check if value is "Dark"
        let dark_cstr = match CString::new("Dark") {
            Ok(s) => s,
            Err(_) => return false,
        };

        let dark_str: *mut Object =
            msg_send![ns_string_class, stringWithUTF8String: dark_cstr.as_ptr()];
        if dark_str.is_null() {
            return false;
        }

        let is_equal: bool = msg_send![value, isEqualToString: dark_str];
        is_equal
    }
}

#[cfg(not(target_os = "macos"))]
fn detect_system_dark_mode() -> bool {
    // Default to light mode on other platforms
    // TODO: Implement for Windows and Linux
    false
}

/// Get the effective theme (resolves Auto mode to actual light/dark)
/// Returns true if dark mode should be used
fn get_effective_dark_mode(theme_mode: i32) -> bool {
    match theme_mode {
        0 => false,                     // Light
        1 => true,                      // Dark
        2 => detect_system_dark_mode(), // Auto - use system preference
        _ => false,                     // Default to light
    }
}

/// Update window appearance when theme changes (native title bar)
#[cfg(target_os = "macos")]
fn set_title_bar_color(window: &MainWindow, is_dark: bool) {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use raw_window_handle::HasWindowHandle;
    use std::ffi::CString;

    if let Ok(handle) = window.window().window_handle().window_handle() {
        if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = handle.as_ref() {
            let ns_view = appkit_handle.ns_view.as_ptr() as *mut Object;

            unsafe {
                let ns_window: *mut Object = msg_send![ns_view, window];
                if ns_window.is_null() {
                    return;
                }

                // Make the native title bar visually merge with the window content.
                let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: true];
                let _: () = msg_send![ns_window, setOpaque: false];
                // Keep dragging restricted to the native title bar so interactive content
                // such as the terminal resize handle does not start moving the whole window.
                let _: () = msg_send![ns_window, setMovableByWindowBackground: false];

                // Set window appearance (light/dark)
                if let Some(ns_appearance_class) = Class::get("NSAppearance") {
                    if let Some(ns_string_class) = Class::get("NSString") {
                        let appearance_name = if is_dark {
                            "NSAppearanceNameDarkAqua"
                        } else {
                            "NSAppearanceNameAqua"
                        };
                        if let Ok(c_str) = CString::new(appearance_name) {
                            let name_obj: *mut Object =
                                msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
                            if !name_obj.is_null() {
                                let appearance: *mut Object =
                                    msg_send![ns_appearance_class, appearanceNamed: name_obj];
                                if !appearance.is_null() {
                                    let _: () = msg_send![ns_window, setAppearance: appearance];
                                }
                            }
                        }
                    }
                }

                if let Some(ns_color_class) = Class::get("NSColor") {
                    let (red, green, blue) = if is_dark {
                        // Matches MaterialPalette.surface_container_lowest in dark mode.
                        (12.0 / 255.0, 14.0 / 255.0, 19.0 / 255.0)
                    } else {
                        // Matches MaterialPalette.surface_container_lowest in light mode.
                        (1.0, 1.0, 1.0)
                    };
                    let background_color: *mut Object = msg_send![
                        ns_color_class,
                        colorWithSRGBRed: red
                        green: green
                        blue: blue
                        alpha: 1.0
                    ];
                    if !background_color.is_null() {
                        let _: () = msg_send![ns_window, setBackgroundColor: background_color];
                    }
                }

                log::info!("[Theme] Window appearance updated (dark: {})", is_dark);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_title_bar_color(_window: &MainWindow, _is_dark: bool) {
    // Other platforms not supported yet
}

#[cfg(target_os = "macos")]
fn register_titlebar_settings_delegate_class() -> &'static objc::runtime::Class {
    use objc::declare::ClassDecl;
    use objc::runtime::{Class, Object, Sel};
    use std::sync::Once;

    static mut DELEGATE_CLASS: *const Class = std::ptr::null();
    static REGISTER: Once = Once::new();

    REGISTER.call_once(|| {
        let superclass = Class::get("NSObject").expect("NSObject class not found");
        let mut decl = ClassDecl::new("UsherTitlebarSettingsDelegate", superclass)
            .expect("failed to declare delegate class");

        extern "C" fn open_settings(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            if let Some(ui_weak) = TITLEBAR_SETTINGS_UI_WEAK.get() {
                let ui_weak_local = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak_local.upgrade() {
                        ui.invoke_navigate(Route::Settings);
                    }
                });
            }
        }

        extern "C" fn open_about(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            if let Some(ui_weak) = TITLEBAR_SETTINGS_UI_WEAK.get() {
                let ui_weak_local = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak_local.upgrade() {
                        ui.invoke_navigate(Route::About);
                    }
                });
            }
        }

        extern "C" fn window_interaction_begin(
            _this: &Object,
            _cmd: Sel,
            _notification: *mut Object,
        ) {
            use std::sync::atomic::Ordering;
            WINDOW_IS_INTERACTING.store(true, Ordering::SeqCst);
        }

        extern "C" fn window_did_move(_this: &Object, _cmd: Sel, _notification: *mut Object) {
            use std::sync::atomic::Ordering;
            let seq = WINDOW_INTERACTION_MOVE_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = slint::invoke_from_event_loop(move || {
                slint::Timer::single_shot(std::time::Duration::from_millis(180), move || {
                    use std::sync::atomic::Ordering;
                    if WINDOW_INTERACTION_MOVE_SEQ.load(Ordering::SeqCst) != seq {
                        return;
                    }
                    WINDOW_IS_INTERACTING.store(false, Ordering::SeqCst);
                });
            });
        }

        extern "C" fn window_interaction_end(
            _this: &Object,
            _cmd: Sel,
            _notification: *mut Object,
        ) {
            use std::sync::atomic::Ordering;
            WINDOW_IS_INTERACTING.store(false, Ordering::SeqCst);
        }

        unsafe {
            decl.add_method(
                sel!(usherOpenSettings:),
                open_settings as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(usherOpenAbout:),
                open_about as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(usherWindowInteractionBegin:),
                window_interaction_begin as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(usherWindowDidMove:),
                window_did_move as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(usherWindowInteractionEnd:),
                window_interaction_end as extern "C" fn(&Object, Sel, *mut Object),
            );
            DELEGATE_CLASS = decl.register();
        }
    });

    unsafe { &*DELEGATE_CLASS }
}

#[cfg(target_os = "macos")]
fn install_titlebar_settings_button(window: &MainWindow) {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use raw_window_handle::HasWindowHandle;
    use std::ffi::CString;

    #[repr(C)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    struct NSSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }

    unsafe fn create_nsstring(s: &str) -> Option<*mut Object> {
        let ns_string_class = Class::get("NSString")?;
        let c_str = CString::new(s).ok()?;
        let ns: *mut Object = msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
        if ns.is_null() { None } else { Some(ns) }
    }

    let _ = TITLEBAR_SETTINGS_UI_WEAK.set(window.as_weak());

    unsafe {
        if TITLEBAR_SETTINGS_INSTALLED {
            return;
        }

        let window_handle = window.window().window_handle();
        let Ok(handle) = window_handle.window_handle() else {
            return;
        };
        let raw = handle.as_ref();
        let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = raw else {
            return;
        };

        let ns_view = appkit_handle.ns_view.as_ptr() as *mut Object;
        if ns_view.is_null() {
            log::warn!("[TitleBar] ns_view is null, skip install this round");
            return;
        }
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            log::warn!("[TitleBar] ns_window is null, skip install this round");
            return;
        }

        if TITLEBAR_SETTINGS_DELEGATE.is_null() {
            let delegate_class = register_titlebar_settings_delegate_class();
            let delegate: *mut Object = msg_send![delegate_class, alloc];
            let delegate: *mut Object = msg_send![delegate, init];
            let _: () = msg_send![delegate, retain];
            TITLEBAR_SETTINGS_DELEGATE = delegate;
        }

        // Reuse the existing native delegate to observe window interaction events.
        // During drag/resize, temporarily downgrade Tag loading animation to reduce repaint pressure.
        if let Some(center_class) = Class::get("NSNotificationCenter") {
            let center: *mut Object = msg_send![center_class, defaultCenter];
            if !center.is_null() {
                if let Some(name) = create_nsstring("NSWindowWillMoveNotification") {
                    let _: () = msg_send![
                        center,
                        addObserver: TITLEBAR_SETTINGS_DELEGATE
                        selector: sel!(usherWindowInteractionBegin:)
                        name: name
                        object: ns_window
                    ];
                }
                if let Some(name) = create_nsstring("NSWindowDidMoveNotification") {
                    let _: () = msg_send![
                        center,
                        addObserver: TITLEBAR_SETTINGS_DELEGATE
                        selector: sel!(usherWindowDidMove:)
                        name: name
                        object: ns_window
                    ];
                }
                if let Some(name) = create_nsstring("NSWindowWillStartLiveResizeNotification") {
                    let _: () = msg_send![
                        center,
                        addObserver: TITLEBAR_SETTINGS_DELEGATE
                        selector: sel!(usherWindowInteractionBegin:)
                        name: name
                        object: ns_window
                    ];
                }
                if let Some(name) = create_nsstring("NSWindowDidEndLiveResizeNotification") {
                    let _: () = msg_send![
                        center,
                        addObserver: TITLEBAR_SETTINGS_DELEGATE
                        selector: sel!(usherWindowInteractionEnd:)
                        name: name
                        object: ns_window
                    ];
                }
            }
        }

        let accessory_class = match Class::get("NSTitlebarAccessoryViewController") {
            Some(c) => c,
            None => return,
        };
        let view_class = match Class::get("NSView") {
            Some(c) => c,
            None => return,
        };
        let button_class = match Class::get("NSButton") {
            Some(c) => c,
            None => return,
        };

        let accessory: *mut Object = msg_send![accessory_class, alloc];
        let accessory: *mut Object = msg_send![accessory, init];
        if accessory.is_null() {
            return;
        }

        let container_frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: 104.0,
                height: 32.0,
            },
        };
        let container: *mut Object = msg_send![view_class, alloc];
        let container: *mut Object = msg_send![container, initWithFrame: container_frame];
        if container.is_null() {
            return;
        }

        let settings_button_frame = NSRect {
            // Keep super-large icon button vertically centered in the titlebar.
            // y = (container_h - button_h) / 2 = (32 - 36) / 2 = -2
            origin: NSPoint { x: 20.0, y: -2.0 },
            size: NSSize {
                width: 48.0,
                height: 36.0,
            },
        };
        let settings_button: *mut Object = msg_send![button_class, alloc];
        let settings_button: *mut Object =
            msg_send![settings_button, initWithFrame: settings_button_frame];
        if settings_button.is_null() {
            return;
        }

        let about_button_frame = NSRect {
            origin: NSPoint { x: 52.0, y: -1.0 },
            size: NSSize {
                width: 48.0,
                height: 36.0,
            },
        };
        let about_button: *mut Object = msg_send![button_class, alloc];
        let about_button: *mut Object = msg_send![about_button, initWithFrame: about_button_frame];
        if about_button.is_null() {
            return;
        }

        // Prefer native gear-style images.
        // If unavailable on current macOS runtime, fallback to a gear glyph ("⚙︎")
        // so the control is always icon-like instead of text-like.
        let ns_image_class = Class::get("NSImage");
        if let Some(ns_image_class) = ns_image_class {
            let settings_symbol_candidates = [
                "NSImageNamePreferencesGeneral",
                "NSImageNameAdvanced",
                "NSImageNameActionTemplate",
            ];
            let mut settings_image_set = false;
            for name in settings_symbol_candidates {
                if let Some(symbol_name) = create_nsstring(name) {
                    let image: *mut Object = msg_send![ns_image_class, imageNamed: symbol_name];
                    if !image.is_null() {
                        let _: () = msg_send![settings_button, setImage: image];
                        if let Some(empty) = create_nsstring("") {
                            let _: () = msg_send![settings_button, setTitle: empty];
                        }
                        // NSImageOnly
                        let _: () = msg_send![settings_button, setImagePosition: 2u64];
                        // NSImageScaleProportionallyUpOrDown
                        let _: () = msg_send![settings_button, setImageScaling: 3u64];
                        settings_image_set = true;
                        break;
                    }
                }
            }
            if !settings_image_set {
                if let Some(gear) = create_nsstring("⚙︎") {
                    let _: () = msg_send![settings_button, setTitle: gear];
                }
            }
            let about_symbol_candidates = ["NSImageNameInfo", "NSImageNameActionTemplate"];
            let mut about_image_set = false;
            for name in about_symbol_candidates {
                if let Some(symbol_name) = create_nsstring(name) {
                    let image: *mut Object = msg_send![ns_image_class, imageNamed: symbol_name];
                    if !image.is_null() {
                        let _: () = msg_send![about_button, setImage: image];
                        if let Some(empty) = create_nsstring("") {
                            let _: () = msg_send![about_button, setTitle: empty];
                        }
                        let _: () = msg_send![about_button, setImagePosition: 2u64];
                        let _: () = msg_send![about_button, setImageScaling: 3u64];
                        about_image_set = true;
                        break;
                    }
                }
            }
            if !about_image_set {
                if let Some(info) = create_nsstring("ℹ︎") {
                    let _: () = msg_send![about_button, setTitle: info];
                }
            }
        } else {
            if let Some(gear) = create_nsstring("⚙︎") {
                let _: () = msg_send![settings_button, setTitle: gear];
            }
            if let Some(info) = create_nsstring("ℹ︎") {
                let _: () = msg_send![about_button, setTitle: info];
            }
        }

        // Increase fallback glyph readability for compact icon button.
        if let Some(ns_font_class) = Class::get("NSFont") {
            let font: *mut Object = msg_send![ns_font_class, systemFontOfSize: 24.0f64];
            if !font.is_null() {
                let _: () = msg_send![settings_button, setFont: font];
                let _: () = msg_send![about_button, setFont: font];
            }
        }

        // True icon-button style: remove default capsule background/border.
        let _: () = msg_send![settings_button, setBordered: false];
        let _: () = msg_send![about_button, setBordered: false];
        if let Some(tooltip) = create_nsstring("Settings") {
            let _: () = msg_send![settings_button, setToolTip: tooltip];
        }
        if let Some(tooltip) = create_nsstring("About") {
            let _: () = msg_send![about_button, setToolTip: tooltip];
        }
        let _: () = msg_send![settings_button, setTarget: TITLEBAR_SETTINGS_DELEGATE];
        let _: () = msg_send![settings_button, setAction: sel!(usherOpenSettings:)];
        let _: () = msg_send![about_button, setTarget: TITLEBAR_SETTINGS_DELEGATE];
        let _: () = msg_send![about_button, setAction: sel!(usherOpenAbout:)];

        let _: () = msg_send![container, addSubview: settings_button];
        let _: () = msg_send![container, addSubview: about_button];
        let _: () = msg_send![accessory, setView: container];
        // NSTitlebarAccessoryLayoutAttributeRight
        // Different SDK headers may map raw values differently; `2` works as right-side in current target.
        let _: () = msg_send![accessory, setLayoutAttribute: 2u64];
        let _: () = msg_send![ns_window, addTitlebarAccessoryViewController: accessory];

        TITLEBAR_SETTINGS_INSTALLED = true;
        log::info!("[TitleBar] Added native Settings/About buttons to macOS title bar");
    }
}

#[cfg(not(target_os = "macos"))]
fn install_titlebar_settings_button(_window: &MainWindow) {}

/// Register GitHub's macOS UI and monospace faces without bundling Apple's
/// fonts in the application. Slint 1.17 accepts one family name per text item,
/// so Rust resolves the platform-specific families for the review workspace.
#[cfg(target_os = "macos")]
fn setup_pr_review_fonts(app: &MainWindow) {
    use slint::fontique_010::fontique;

    const SF_MONO_FONTS: &[&str] = &[
        "/System/Applications/Utilities/Terminal.app/Contents/Resources/Fonts/SF-Mono-Regular.otf",
        "/System/Applications/Utilities/Terminal.app/Contents/Resources/Fonts/SF-Mono-RegularItalic.otf",
        "/System/Applications/Utilities/Terminal.app/Contents/Resources/Fonts/SF-Mono-Semibold.otf",
    ];
    let mut collection = slint::fontique_010::shared_collection();
    let mut regular_registered = false;

    for (index, path) in SF_MONO_FONTS.iter().enumerate() {
        let Ok(bytes) = std::fs::read(path) else {
            log::debug!("[PR Review] SF Mono face is unavailable: {path}");
            continue;
        };
        let blob = fontique::Blob::new(std::sync::Arc::new(bytes));
        let registered = collection.register_fonts(blob, None);
        if !registered.is_empty() && index == 0 {
            regular_registered = true;
        }
    }

    if regular_registered {
        // Apple's OTF records `SF Mono` as the family and
        // `SFMono-Regular` as the PostScript face. GitHub puts that
        // PostScript face first in its macOS code font stack, while Slint
        // resolves the registered font by family name.
        app.set_workflow_pr_review_code_font_family("SF Mono".into());
        log::info!("[PR Review] Using GitHub-compatible SFMono-Regular code font");
    } else {
        app.set_workflow_pr_review_code_font_family("Menlo".into());
        log::warn!("[PR Review] SFMono-Regular unavailable; falling back to Menlo");
    }

    // Fontique's CoreText backend explicitly discovers Apple's hidden SFNS
    // files and exposes `.SF NS` as the system-ui family. Registering the same
    // files here retained another 8.5 MiB of font blobs for the full process
    // lifetime and also bypassed the backend's normal on-demand source cache.
    if collection.family_id(".SF NS").is_some() {
        app.set_workflow_pr_review_tree_font_family(".SF NS".into());
        log::info!("[PR Review] Using Fontique's GitHub-compatible macOS system UI font");
    } else {
        app.set_workflow_pr_review_tree_font_family("Helvetica Neue".into());
        log::warn!("[PR Review] macOS system UI font unavailable; falling back to Helvetica Neue");
    }
}

#[cfg(target_os = "windows")]
fn setup_pr_review_fonts(app: &MainWindow) {
    app.set_workflow_pr_review_code_font_family("Consolas".into());
    app.set_workflow_pr_review_tree_font_family("Segoe UI".into());
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn setup_pr_review_fonts(app: &MainWindow) {
    app.set_workflow_pr_review_code_font_family("Liberation Mono".into());
    app.set_workflow_pr_review_tree_font_family("Noto Sans".into());
}

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
    let router = Router::new(app.as_weak());

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
                    log_ui_model_memory(ui, route);
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
