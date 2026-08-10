use std::ffi::CString;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use block::ConcreteBlock;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use raw_window_handle::HasWindowHandle;
use slint::ComponentHandle;

use crate::{MainWindow, Route};

static TITLEBAR_UI: std::sync::OnceLock<slint::Weak<MainWindow>> = std::sync::OnceLock::new();
static WINDOW_MOVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WINDOW_INTERACTING: AtomicBool = AtomicBool::new(false);
static MEMORY_WINDOW_CONTROL_INSTALLED: AtomicBool = AtomicBool::new(false);

static mut TITLEBAR_DELEGATE: *mut Object = std::ptr::null_mut();
static mut TITLEBAR_INSTALLED: bool = false;
static mut TITLEBAR_TOGGLE_BUTTON: *mut Object = std::ptr::null_mut();
static mut TITLEBAR_BACK_BUTTON: *mut Object = std::ptr::null_mut();
static mut TITLEBAR_FORWARD_BUTTON: *mut Object = std::ptr::null_mut();
static mut TITLEBAR_ACCESSORY_ANCHOR: *mut Object = std::ptr::null_mut();
static mut TITLEBAR_CONTROLS_CONTAINER: *mut Object = std::ptr::null_mut();
static mut TITLEBAR_MODAL_SHIELD: *mut Object = std::ptr::null_mut();
static mut TITLEBAR_EVENT_MONITOR: *mut Object = std::ptr::null_mut();
static mut TRAFFIC_LIGHT_BASE_X: [f64; 3] = [f64::NAN; 3];
static mut TRAFFIC_LIGHT_BASE_Y: [f64; 3] = [f64::NAN; 3];
/// Shift the AppKit traffic-light cluster right so the close button shares the
/// sidebar nav-icon centerline (24px). Default close center is 16px.
const TRAFFIC_LIGHT_X_OFFSET: f64 = 8.0;
/// Lower the compact AppKit control band onto the 46pt title-bar midline.
const TRAFFIC_LIGHT_Y_OFFSET: f64 = 7.0;
const NS_WINDOW_STYLE_MASK_FULLSCREEN: u64 = 1u64 << 14;
static mut SIDEBAR_VIBRANCY_INSTALLED: bool = false;
static mut SIDEBAR_VIBRANCY_VIEW: *mut Object = std::ptr::null_mut();
static mut WINDOW_SURFACE_BACKDROP_VIEW: *mut Object = std::ptr::null_mut();
static mut SIDEBAR_DARK_MODE: bool = false;

pub(crate) fn install_memory_benchmark_window_control(window: &MainWindow) {
    if std::env::var("USHER_MEMORY_WINDOW_CONTROL").as_deref() != Ok("signal-file-v1")
        || MEMORY_WINDOW_CONTROL_INSTALLED.swap(true, Ordering::SeqCst)
    {
        return;
    }
    let Ok(command_file) = std::env::var("USHER_MEMORY_WINDOW_CONTROL_FILE") else {
        log::error!("[Memory][WindowControl] Command file path is missing");
        return;
    };

    use signal_hook::consts::signal::SIGUSR1;
    use signal_hook::iterator::Signals;

    let mut signals = match Signals::new([SIGUSR1]) {
        Ok(signals) => signals,
        Err(error) => {
            log::error!("[Memory][WindowControl] Failed to register SIGUSR1: {error}");
            return;
        }
    };
    let handle = signals.handle();
    crate::shutdown::register_pre_shutdown_task("stop_memory_window_control", move || {
        handle.close();
    });

    let weak_window = window.as_weak();
    if let Err(error) = std::thread::Builder::new()
        .name("memory-window-control".to_string())
        .spawn(move || {
            for _ in signals.forever() {
                let command = match std::fs::read_to_string(&command_file) {
                    Ok(command) => command,
                    Err(error) => {
                        log::error!("[Memory][WindowControl] Failed to read command: {error}");
                        continue;
                    }
                };
                let fields = command.split_whitespace().collect::<Vec<_>>();
                let resize = match fields.as_slice() {
                    ["resize", width, height] => {
                        match (width.parse::<f64>(), height.parse::<f64>()) {
                            (Ok(width), Ok(height)) if width > 0.0 && height > 0.0 => {
                                Some((width, height))
                            }
                            _ => {
                                log::error!(
                                    "[Memory][WindowControl] Invalid resize command: {command:?}"
                                );
                                continue;
                            }
                        }
                    }
                    ["toggle-fullscreen"] => None,
                    _ => {
                        log::error!(
                            "[Memory][WindowControl] Unsupported command: {command:?}"
                        );
                        continue;
                    }
                };
                let weak_window = weak_window.clone();
                if let Err(error) = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    let Some(native_window) = native_window(&window) else {
                        log::error!("[Memory][WindowControl] Native window is unavailable");
                        return;
                    };
                    unsafe {
                        let sender: *mut Object = std::ptr::null_mut();
                        if let Some(app_class) = Class::get("NSApplication") {
                            let app: *mut Object = msg_send![app_class, sharedApplication];
                            if !app.is_null() {
                                let _: () = msg_send![app, activateIgnoringOtherApps: true];
                            }
                        }
                        let _: () = msg_send![native_window, makeKeyAndOrderFront: sender];
                        if let Some((width, height)) = resize {
                            let mut frame: NSRect = msg_send![native_window, frame];
                            frame.origin.y += frame.size.height - height;
                            frame.size = NSSize { width, height };
                            let _: () = msg_send![
                                native_window,
                                setFrame: frame
                                display: true
                                animate: false
                            ];
                            log::info!(
                                "[Memory][WindowControl] Resized native window to {width:.0}x{height:.0}"
                            );
                        } else {
                            let _: () = msg_send![native_window, toggleFullScreen: sender];
                            log::info!(
                                "[Memory][WindowControl] Requested native fullscreen toggle"
                            );
                        }
                    }
                }) {
                    log::error!(
                        "[Memory][WindowControl] Failed to schedule fullscreen toggle: {error}"
                    );
                }
            }
        })
    {
        log::error!("[Memory][WindowControl] Failed to start signal worker: {error}");
        return;
    }
    log::info!("[Memory][WindowControl] SIGUSR1 command-file control is active");
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

fn nsstring(value: &str) -> Option<*mut Object> {
    let string_class = Class::get("NSString")?;
    let value = CString::new(value).ok()?;
    let string: *mut Object =
        unsafe { msg_send![string_class, stringWithUTF8String: value.as_ptr()] };
    (!string.is_null()).then_some(string)
}

fn appearance(is_dark: bool) -> Option<*mut Object> {
    let appearance_class = Class::get("NSAppearance")?;
    let name = nsstring(if is_dark {
        "NSAppearanceNameDarkAqua"
    } else {
        "NSAppearanceNameAqua"
    })?;
    let appearance: *mut Object = unsafe { msg_send![appearance_class, appearanceNamed: name] };
    (!appearance.is_null()).then_some(appearance)
}

pub fn is_window_interacting() -> bool {
    WINDOW_INTERACTING.load(Ordering::SeqCst)
}

pub(crate) fn set_app_appearance(is_dark: bool) {
    let Some(app_class) = Class::get("NSApplication") else {
        return;
    };
    let app: *mut Object = unsafe { msg_send![app_class, sharedApplication] };
    if app.is_null() {
        return;
    }
    let Some(appearance) = appearance(is_dark) else {
        return;
    };
    unsafe {
        let _: () = msg_send![app, setAppearance: appearance];
    }
    log::info!(
        "[Theme] Set app appearance to: {}",
        if is_dark { "dark" } else { "light" }
    );
}

/// Leave NSApplication appearance unset so AppKit tracks the system setting.
/// Required for Auto mode; a forced Aqua/DarkAqua locks the process appearance.
pub(crate) fn follow_system_appearance() {
    let Some(app_class) = Class::get("NSApplication") else {
        return;
    };
    let app: *mut Object = unsafe { msg_send![app_class, sharedApplication] };
    if app.is_null() {
        return;
    }
    let nil: *mut Object = std::ptr::null_mut();
    unsafe {
        let _: () = msg_send![app, setAppearance: nil];
    }
    log::info!("[Theme] App appearance follows the system setting");
}

pub(crate) fn detect_system_dark_mode() -> bool {
    let Some(defaults_class) = Class::get("NSUserDefaults") else {
        return false;
    };
    let defaults: *mut Object = unsafe { msg_send![defaults_class, standardUserDefaults] };
    let Some(key) = nsstring("AppleInterfaceStyle") else {
        return false;
    };
    let Some(dark) = nsstring("Dark") else {
        return false;
    };
    if defaults.is_null() {
        return false;
    }
    let value: *mut Object = unsafe { msg_send![defaults, stringForKey: key] };
    if value.is_null() {
        return false;
    }
    unsafe { msg_send![value, isEqualToString: dark] }
}

fn native_view(window: &MainWindow) -> Option<*mut Object> {
    let window_handle = window.window().window_handle();
    let handle = window_handle.window_handle().ok()?;
    let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = handle.as_ref() else {
        return None;
    };
    let view = appkit_handle.ns_view.as_ptr() as *mut Object;
    (!view.is_null()).then_some(view)
}

fn native_window(window: &MainWindow) -> Option<*mut Object> {
    let view = native_view(window)?;
    let window: *mut Object = unsafe { msg_send![view, window] };
    (!window.is_null()).then_some(window)
}

/// Point size for sidebar/back/forward SF Symbols inside the 28 pt accessory
/// buttons. Must stay on a whole point — half sizes like 12.5 rasterize soft.
const TITLEBAR_SYMBOL_POINT_SIZE: f64 = 14.0;
/// `NSImageScaleNone`: the symbol is already sized; do not let the bezel
/// bilinear-scale it again inside the 28 pt control.
const NS_IMAGE_SCALE_NONE: u64 = 2;

unsafe fn configured_system_symbol(image: *mut Object, point_size: f64) -> *mut Object {
    let Some(configuration_class) = Class::get("NSImageSymbolConfiguration") else {
        return image;
    };
    // weight 0 = NSFontWeightRegular; scale 2 = NSImageSymbolScaleMedium.
    let configuration: *mut Object = unsafe {
        msg_send![
            configuration_class,
            configurationWithPointSize: point_size
            weight: 0.0f64
            scale: 2isize
        ]
    };
    if configuration.is_null() {
        return image;
    }
    let configured: *mut Object =
        unsafe { msg_send![image, imageWithSymbolConfiguration: configuration] };
    if configured.is_null() {
        image
    } else {
        configured
    }
}

pub(crate) fn set_title_bar_color(window: &MainWindow, is_dark: bool) {
    unsafe {
        SIDEBAR_DARK_MODE = is_dark;
    }
    let Some(window) = native_window(window) else {
        return;
    };
    unsafe {
        let _: () = msg_send![window, setTitlebarAppearsTransparent: true];
        // Stay opaque until Menu vibrancy and the content backdrop are installed.
        // Opening the transparent path earlier lets the translucent Slint sidebar
        // flash desktop content on the first ordered frame.
        if SIDEBAR_VIBRANCY_INSTALLED {
            let _: () = msg_send![window, setOpaque: false];
        }
        // Do not let terminal/diff drag surfaces move the native window.
        let _: () = msg_send![window, setMovableByWindowBackground: false];
    }

    if let Some(appearance) = appearance(is_dark) {
        unsafe {
            let _: () = msg_send![window, setAppearance: appearance];
        }
    }

    if unsafe { SIDEBAR_VIBRANCY_INSTALLED } {
        unsafe {
            let is_key: bool = msg_send![window, isKeyWindow];
            update_sidebar_window_surface(window, is_key);
        }
        return;
    }

    let (red, green, blue) = if is_dark {
        (12.0 / 255.0, 14.0 / 255.0, 19.0 / 255.0)
    } else {
        (1.0, 1.0, 1.0)
    };
    let color = unsafe { nscolor_srgb(red, green, blue, 1.0) };
    if !color.is_null() {
        unsafe {
            let _: () = msg_send![window, setBackgroundColor: color];
        }
    }

    log::info!("[Theme] Window appearance updated (dark: {})", is_dark);
}

fn set_window_live_resizing(active: bool) {
    if let Some(ui) = TITLEBAR_UI.get().and_then(slint::Weak::upgrade) {
        // AppKit delivers resize notifications on Slint's owning thread.
        ui.set_window_live_resizing(active);
    }
}

fn set_native_window_active(active: bool) {
    if let Some(ui) = TITLEBAR_UI.get().and_then(slint::Weak::upgrade) {
        // AppKit key-window notifications run on Slint's owning event loop.
        ui.set_native_window_active(active);
    }
}

fn style_mask_is_fullscreen(style_mask: u64) -> bool {
    style_mask & NS_WINDOW_STYLE_MASK_FULLSCREEN != 0
}

fn set_native_window_fullscreen(fullscreen: bool) {
    if let Some(ui) = TITLEBAR_UI.get().and_then(slint::Weak::upgrade) {
        // AppKit window notifications run on Slint's owning event loop.
        ui.set_native_window_fullscreen(fullscreen);
    }
}

unsafe fn sync_native_window_fullscreen(window: *mut Object) {
    if window.is_null() {
        return;
    }
    let style_mask: u64 = unsafe { msg_send![window, styleMask] };
    set_native_window_fullscreen(style_mask_is_fullscreen(style_mask));
}

fn register_titlebar_delegate_class() -> &'static Class {
    static mut DELEGATE_CLASS: *const Class = std::ptr::null();
    static REGISTER: Once = Once::new();

    REGISTER.call_once(|| {
        let superclass = Class::get("NSObject").expect("NSObject class not found");
        let mut declaration = ClassDecl::new("UsherTitlebarSettingsDelegate", superclass)
            .expect("failed to declare delegate class");

        extern "C" fn open_settings(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            navigate(Route::Settings);
        }

        extern "C" fn open_about(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            navigate(Route::About);
        }

        extern "C" fn toggle_sidebar(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            // NSControl actions are delivered on the AppKit main thread, which
            // is also Slint's owning event-loop thread. Invoke the callback
            // directly so rapid clicks retarget the current motion value
            // instead of building an invoke_from_event_loop backlog.
            if let Some(ui) = TITLEBAR_UI.get().and_then(slint::Weak::upgrade) {
                ui.invoke_toggle_navigation_drawer();
            }
        }

        extern "C" fn navigate_back(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            invoke_titlebar_action(|ui| ui.invoke_navigate_back());
        }

        extern "C" fn navigate_forward(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            invoke_titlebar_action(|ui| ui.invoke_navigate_forward());
        }

        extern "C" fn interaction_begin(_this: &Object, _cmd: Sel, _notification: *mut Object) {
            WINDOW_INTERACTING.store(true, Ordering::SeqCst);
        }

        extern "C" fn window_did_move(_this: &Object, _cmd: Sel, _notification: *mut Object) {
            let sequence = WINDOW_MOVE_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = slint::invoke_from_event_loop(move || {
                slint::Timer::single_shot(std::time::Duration::from_millis(180), move || {
                    if WINDOW_MOVE_SEQUENCE.load(Ordering::SeqCst) == sequence {
                        WINDOW_INTERACTING.store(false, Ordering::SeqCst);
                    }
                });
            });
        }

        extern "C" fn interaction_end(_this: &Object, _cmd: Sel, _notification: *mut Object) {
            WINDOW_INTERACTING.store(false, Ordering::SeqCst);
        }

        extern "C" fn live_resize_begin(_this: &Object, _cmd: Sel, _notification: *mut Object) {
            WINDOW_INTERACTING.store(true, Ordering::SeqCst);
            set_window_live_resizing(true);
        }

        extern "C" fn live_resize_end(_this: &Object, _cmd: Sel, _notification: *mut Object) {
            WINDOW_INTERACTING.store(false, Ordering::SeqCst);
            set_window_live_resizing(false);
        }

        extern "C" fn align_traffic_lights(_this: &Object, _cmd: Sel, window: *mut Object) {
            unsafe { align_standard_window_buttons(window) };
        }

        extern "C" fn window_did_resize(_this: &Object, _cmd: Sel, notification: *mut Object) {
            if notification.is_null() {
                return;
            }
            let window: *mut Object = unsafe { msg_send![notification, object] };
            unsafe {
                sync_native_window_fullscreen(window);
                align_standard_window_buttons(window);
                layout_sidebar_vibrancy_views();
            }
        }

        extern "C" fn fullscreen_transition_completed(
            this: &Object,
            _cmd: Sel,
            notification: *mut Object,
        ) {
            if notification.is_null() {
                return;
            }
            let window: *mut Object = unsafe { msg_send![notification, object] };
            if window.is_null() {
                return;
            }
            unsafe {
                sync_native_window_fullscreen(window);
                align_standard_window_buttons(window);
                // Full-screen transitions can perform one more private title-bar
                // layout after the did-enter/did-exit notification. Recheck on
                // the next run-loop turn and once after the animation settles.
                let _: () = msg_send![
                    this,
                    performSelector: sel!(usherAlignTrafficLights:)
                    withObject: window
                    afterDelay: 0.0f64
                ];
                let _: () = msg_send![
                    this,
                    performSelector: sel!(usherAlignTrafficLights:)
                    withObject: window
                    afterDelay: 0.25f64
                ];
            }
        }

        extern "C" fn window_did_become_key(_this: &Object, _cmd: Sel, notification: *mut Object) {
            if notification.is_null() {
                return;
            }
            let window: *mut Object = unsafe { msg_send![notification, object] };
            if !window.is_null() {
                unsafe { update_sidebar_window_surface(window, true) };
            }
        }

        extern "C" fn window_did_resign_key(_this: &Object, _cmd: Sel, notification: *mut Object) {
            if notification.is_null() {
                return;
            }
            let window: *mut Object = unsafe { msg_send![notification, object] };
            if !window.is_null() {
                unsafe { update_sidebar_window_surface(window, false) };
            }
        }

        unsafe {
            declaration.add_method(
                sel!(usherOpenSettings:),
                open_settings as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherOpenAbout:),
                open_about as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherToggleSidebar:),
                toggle_sidebar as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherNavigateBack:),
                navigate_back as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherNavigateForward:),
                navigate_forward as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherWindowInteractionBegin:),
                interaction_begin as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherWindowDidMove:),
                window_did_move as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherWindowInteractionEnd:),
                interaction_end as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherWindowLiveResizeBegin:),
                live_resize_begin as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherWindowLiveResizeEnd:),
                live_resize_end as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherAlignTrafficLights:),
                align_traffic_lights as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherWindowDidResize:),
                window_did_resize as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherFullscreenTransitionCompleted:),
                fullscreen_transition_completed as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherWindowDidBecomeKey:),
                window_did_become_key as extern "C" fn(&Object, Sel, *mut Object),
            );
            declaration.add_method(
                sel!(usherWindowDidResignKey:),
                window_did_resign_key as extern "C" fn(&Object, Sel, *mut Object),
            );
            DELEGATE_CLASS = declaration.register();
        }
    });

    unsafe { &*DELEGATE_CLASS }
}

fn navigate(route: Route) {
    let Some(ui_weak) = TITLEBAR_UI.get() else {
        return;
    };
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.invoke_navigate(route);
        }
    });
}

fn invoke_titlebar_action(action: impl FnOnce(&MainWindow) + Send + 'static) {
    let Some(ui_weak) = TITLEBAR_UI.get() else {
        return;
    };
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            action(&ui);
        }
    });
}

fn observe_window_interaction(window: *mut Object, delegate: *mut Object) {
    let Some(center_class) = Class::get("NSNotificationCenter") else {
        return;
    };
    let center: *mut Object = unsafe { msg_send![center_class, defaultCenter] };
    if center.is_null() {
        return;
    }

    let notifications = [
        (
            "NSWindowWillMoveNotification",
            sel!(usherWindowInteractionBegin:),
        ),
        ("NSWindowDidMoveNotification", sel!(usherWindowDidMove:)),
        (
            "NSWindowWillStartLiveResizeNotification",
            sel!(usherWindowLiveResizeBegin:),
        ),
        (
            "NSWindowDidEndLiveResizeNotification",
            sel!(usherWindowLiveResizeEnd:),
        ),
        ("NSWindowDidResizeNotification", sel!(usherWindowDidResize:)),
        (
            "NSWindowDidEnterFullScreenNotification",
            sel!(usherFullscreenTransitionCompleted:),
        ),
        (
            "NSWindowDidExitFullScreenNotification",
            sel!(usherFullscreenTransitionCompleted:),
        ),
        (
            "NSWindowDidBecomeKeyNotification",
            sel!(usherWindowDidBecomeKey:),
        ),
        (
            "NSWindowDidResignKeyNotification",
            sel!(usherWindowDidResignKey:),
        ),
    ];
    for (name, selector) in notifications {
        let Some(name) = nsstring(name) else {
            continue;
        };
        unsafe {
            let _: () = msg_send![
                center,
                addObserver: delegate
                selector: selector
                name: name
                object: window
            ];
        }
    }
}

unsafe fn create_view(class_name: &str, frame: NSRect) -> Option<*mut Object> {
    let class = Class::get(class_name)?;
    let view: *mut Object = unsafe { msg_send![class, alloc] };
    let view: *mut Object = unsafe { msg_send![view, initWithFrame: frame] };
    (!view.is_null()).then_some(view)
}

fn macos_shortcut_key(key_code: u16) -> Option<&'static str> {
    // Hardware-independent virtual key codes used by NSEvent on macOS. This is
    // platform normalization only; executable bindings remain exclusively in
    // `src/shortcuts/mod.rs`.
    match key_code {
        11 => Some("b"),
        38 => Some("j"),
        33 => Some("["),
        30 => Some("]"),
        43 => Some(","),
        _ => None,
    }
}

fn install_app_event_monitor(window: *mut Object) -> bool {
    unsafe {
        if !TITLEBAR_EVENT_MONITOR.is_null() {
            return true;
        }
    }
    let Some(event_class) = Class::get("NSEvent") else {
        log::warn!(
            "[TitleBar] NSEvent unavailable; transient overlays cannot observe title-bar clicks"
        );
        return false;
    };

    // Observe title-bar clicks and key-down events in one application-local
    // monitor. Key codes avoid input-method-dependent logical punctuation.
    // NSEventMaskLeftMouseDown = 1 << 1; NSEventMaskKeyDown = 1 << 10.
    let handler = ConcreteBlock::new(move |event: *mut Object| -> *mut Object {
        if event.is_null() {
            return event;
        }
        let event_window: *mut Object = unsafe { msg_send![event, window] };
        if event_window != window {
            return event;
        }
        let event_type: u64 = unsafe { msg_send![event, type] };
        if event_type == 10 {
            let key_code: u16 = unsafe { msg_send![event, keyCode] };
            let Some(key) = macos_shortcut_key(key_code) else {
                return event;
            };
            let flags: u64 = unsafe { msg_send![event, modifierFlags] };
            let handled = TITLEBAR_UI
                .get()
                .and_then(slint::Weak::upgrade)
                .is_some_and(|ui| {
                    crate::shortcuts::handle_key_pressed(
                        &ui,
                        key,
                        flags & (1 << 20) != 0, // NSEventModifierFlagCommand
                        flags & (1 << 19) != 0, // NSEventModifierFlagOption
                        flags & (1 << 17) != 0, // NSEventModifierFlagShift
                        flags & (1 << 18) != 0, // NSEventModifierFlagControl
                    )
                });
            if handled {
                return std::ptr::null_mut();
            }
            return event;
        }
        let location: NSPoint = unsafe { msg_send![event, locationInWindow] };
        let content_layout: NSRect = unsafe { msg_send![window, contentLayoutRect] };
        let titlebar_start = content_layout.origin.y + content_layout.size.height;
        if location.y >= titlebar_start {
            let _ = slint::invoke_from_event_loop(|| {
                if let Some(ui) = TITLEBAR_UI.get().and_then(slint::Weak::upgrade) {
                    ui.invoke_dismiss_transient_overlays();
                }
            });
        }
        event
    })
    .copy();

    let monitor: *mut Object = unsafe {
        msg_send![
            event_class,
            addLocalMonitorForEventsMatchingMask: 2u64 | (1u64 << 10)
            handler: &*handler
        ]
    };
    if monitor.is_null() {
        log::warn!("[TitleBar] Failed to install native click bridge for transient overlays");
        return false;
    }
    unsafe {
        TITLEBAR_EVENT_MONITOR = monitor;
    }
    true
}

/// Install the input-method-independent macOS shortcut bridge.
///
/// The native window may not exist until after `show()`, so shortcut startup
/// retries this readiness boundary before reporting completion in the status
/// bar.
pub(crate) fn install_shortcut_input_bridge(window: &MainWindow) -> bool {
    let _ = TITLEBAR_UI.set(window.as_weak());
    let Some(native_window) = native_window(window) else {
        return false;
    };
    install_app_event_monitor(native_window)
}

unsafe fn configure_icon_button(
    button: *mut Object,
    image_names: &[&str],
    fallback: &str,
    tooltip: &str,
    delegate: *mut Object,
    action: Sel,
) {
    let mut image_set = false;
    if let Some(image_class) = Class::get("NSImage") {
        for image_name in image_names {
            let (image_name, system_symbol) = image_name
                .strip_prefix("symbol:")
                .map_or((*image_name, false), |name| (name, true));
            let Some(image_name) = nsstring(image_name) else {
                continue;
            };
            let image: *mut Object = if system_symbol {
                let nil: *mut Object = std::ptr::null_mut();
                unsafe {
                    msg_send![
                        image_class,
                        imageWithSystemSymbolName: image_name
                        accessibilityDescription: nil
                    ]
                }
            } else {
                unsafe { msg_send![image_class, imageNamed: image_name] }
            };
            if image.is_null() {
                continue;
            }
            let image = if system_symbol {
                unsafe { configured_system_symbol(image, TITLEBAR_SYMBOL_POINT_SIZE) }
            } else {
                image
            };
            unsafe {
                let _: () = msg_send![button, setImage: image];
                if let Some(empty) = nsstring("") {
                    let _: () = msg_send![button, setTitle: empty];
                }
                let _: () = msg_send![button, setImagePosition: 2u64];
                let _: () = msg_send![button, setImageScaling: NS_IMAGE_SCALE_NONE];
            }
            image_set = true;
            break;
        }
    }
    if !image_set {
        if let Some(fallback) = nsstring(fallback) {
            unsafe {
                let _: () = msg_send![button, setTitle: fallback];
            }
        }
    }

    if let Some(font_class) = Class::get("NSFont") {
        let font: *mut Object = unsafe { msg_send![font_class, systemFontOfSize: 16.0f64] };
        if !font.is_null() {
            unsafe {
                let _: () = msg_send![button, setFont: font];
            }
        }
    }

    unsafe {
        // Native accessory-bar buttons supply the same hover-only rounded
        // background used by Codex's title-bar controls.
        let _: () = msg_send![button, setBordered: true];
        let _: () = msg_send![button, setBezelStyle: 12u64];
        let _: () = msg_send![button, setShowsBorderOnlyWhileMouseInside: true];
        if let Some(tooltip) = nsstring(tooltip) {
            let _: () = msg_send![button, setToolTip: tooltip];
            let _: () = msg_send![button, setAccessibilityLabel: tooltip];
        }
        let _: () = msg_send![button, setTarget: delegate];
        let _: () = msg_send![button, setAction: action];
    }
}

fn sidebar_toggle_image_names(expanded: bool) -> &'static [&'static str] {
    if expanded {
        &[
            "symbol:rectangle.leftthird.inset.filled",
            "symbol:sidebar.left",
            "NSImageNameTouchBarSidebarTemplate",
            "NSImageNameListViewTemplate",
        ]
    } else {
        &[
            "symbol:sidebar.left",
            "NSImageNameTouchBarSidebarTemplate",
            "NSImageNameListViewTemplate",
        ]
    }
}

fn shortcut_tooltip(action: &str, command: crate::shortcuts::CommandId) -> String {
    crate::shortcuts::display_binding(command)
        .map(|binding| format!("{action} ({binding})"))
        .unwrap_or_else(|| action.to_string())
}

fn sidebar_toggle_tooltip(expanded: bool) -> String {
    shortcut_tooltip(
        if expanded {
            "Hide sidebar"
        } else {
            "Show sidebar"
        },
        crate::shortcuts::CommandId::ToggleSidebar,
    )
}

fn sidebar_content_surface_components() -> (f64, f64, f64) {
    if unsafe { SIDEBAR_DARK_MODE } {
        (12.0 / 255.0, 14.0 / 255.0, 19.0 / 255.0)
    } else {
        (1.0, 1.0, 1.0)
    }
}

unsafe fn nscolor_srgb(red: f64, green: f64, blue: f64, alpha: f64) -> *mut Object {
    let Some(color_class) = Class::get("NSColor") else {
        return std::ptr::null_mut();
    };
    unsafe {
        msg_send![
            color_class,
            colorWithSRGBRed: red
            green: green
            blue: blue
            alpha: alpha
        ]
    }
}

unsafe fn apply_content_surface_backdrop_fill() {
    if unsafe { WINDOW_SURFACE_BACKDROP_VIEW.is_null() } {
        return;
    }
    let (red, green, blue) = sidebar_content_surface_components();
    let surface_color = unsafe { nscolor_srgb(red, green, blue, 1.0) };
    if surface_color.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![WINDOW_SURFACE_BACKDROP_VIEW, setFillColor: surface_color];
    }
}

unsafe fn layout_sidebar_vibrancy_views() {
    let effect = unsafe { SIDEBAR_VIBRANCY_VIEW };
    let backdrop = unsafe { WINDOW_SURFACE_BACKDROP_VIEW };
    if effect.is_null() || backdrop.is_null() {
        return;
    }
    let parent_view: *mut Object = unsafe { msg_send![effect, superview] };
    if parent_view.is_null() {
        return;
    }
    // Use the sibling host's bounds, not the render view's pre-full-size frame.
    // Before FullSizeContentView is enabled the render frame excludes the
    // title bar, which leaves a permanently transparent strip above the effect.
    let bounds: NSRect = unsafe { msg_send![parent_view, bounds] };
    let sidebar_frame = NSRect {
        origin: bounds.origin,
        size: NSSize {
            width: 240.0,
            height: bounds.size.height,
        },
    };
    unsafe {
        let _: () = msg_send![backdrop, setFrame: bounds];
        let _: () = msg_send![effect, setFrame: sidebar_frame];
    }
}

unsafe fn install_sidebar_vibrancy(window: *mut Object, render_view: *mut Object) -> bool {
    if unsafe { SIDEBAR_VIBRANCY_INSTALLED } {
        return true;
    }

    let parent_view: *mut Object = unsafe { msg_send![render_view, superview] };
    if parent_view.is_null() {
        log::warn!("[TitleBar] Native render view has no parent for sidebar vibrancy");
        return false;
    }

    unsafe {
        // Adopt the final full-size transparent-title-bar style before adding
        // native underlays. A later styleMask change can rebuild AppKit's
        // border-view hierarchy and drop or reorder manually inserted siblings,
        // which briefly exposed desktop content through the translucent shell.
        let style_mask: u64 = msg_send![window, styleMask];
        let _: () = msg_send![window, setStyleMask: style_mask | (1u64 << 15)];
        let _: () = msg_send![window, setTitlebarAppearsTransparent: true];
    }

    // The parent bounds include the full-size title-bar area. The render view
    // frame can still exclude that strip at this point in window creation.
    let frame: NSRect = unsafe { msg_send![parent_view, bounds] };

    // The Slint GPU surface can be resized one compositor frame after AppKit
    // changes the native window. Keep an opaque native canvas below it so a
    // rapid shrink never exposes the desktop through the transparent window.
    let Some(backdrop) = (unsafe { create_view("NSBox", frame) }) else {
        log::warn!("[TitleBar] Native window-surface backdrop is unavailable");
        return false;
    };
    unsafe {
        let _: () = msg_send![backdrop, setBoxType: 4u64];
        let _: () = msg_send![backdrop, setBorderType: 0u64];
        let _: () = msg_send![backdrop, setBorderWidth: 0.0f64];
        let _: () = msg_send![backdrop, setAutoresizingMask: 18u64];
        // Fill before the window becomes non-opaque. An empty custom NSBox is
        // transparent and would otherwise flash desktop content through the
        // translucent Slint sidebar on the first ordered frame.
        WINDOW_SURFACE_BACKDROP_VIEW = backdrop;
        apply_content_surface_backdrop_fill();
    }

    let Some(effect_class) = Class::get("NSVisualEffectView") else {
        log::warn!("[TitleBar] NSVisualEffectView is unavailable");
        unsafe {
            WINDOW_SURFACE_BACKDROP_VIEW = std::ptr::null_mut();
        }
        return false;
    };
    let effect: *mut Object = unsafe { msg_send![effect_class, alloc] };
    let sidebar_frame = NSRect {
        origin: frame.origin,
        size: NSSize {
            width: 240.0,
            height: frame.size.height,
        },
    };
    let effect: *mut Object = unsafe { msg_send![effect, initWithFrame: sidebar_frame] };
    if effect.is_null() {
        unsafe {
            WINDOW_SURFACE_BACKDROP_VIEW = std::ptr::null_mut();
        }
        return false;
    }
    unsafe {
        // The material belongs only to the 240 pt navigation column. Keeping it
        // out of the right content region prevents a late GPU resize from
        // exposing a wallpaper-tinted rectangle there.
        let _: () = msg_send![effect, setMaterial: 5isize];
        let _: () = msg_send![effect, setBlendingMode: 0isize];
        let _: () = msg_send![effect, setState: 0isize];
        let _: () = msg_send![effect, setAutoresizingMask: 16u64];
    }

    unsafe {
        // Keep Slint/winit's render view as the NSWindow content view. Replacing
        // or reparenting it invalidates the renderer after the first frame.
        // A sibling ordered below the render view supplies the native material
        // while the transparent Slint surface remains fully interactive.
        let _: () = msg_send![
            parent_view,
            addSubview: backdrop
            positioned: -1isize
            relativeTo: render_view
        ];
        let _: () = msg_send![
            parent_view,
            addSubview: effect
            positioned: -1isize
            relativeTo: render_view
        ];

        SIDEBAR_VIBRANCY_VIEW = effect;
        SIDEBAR_VIBRANCY_INSTALLED = true;
        // Re-read the host bounds so the effect and canvas include the
        // title-bar strip of the already-final full-size hierarchy.
        layout_sidebar_vibrancy_views();
        // Only open the transparent window path after the opaque canvas and
        // Menu material are part of the hierarchy; everything commits in one
        // AppKit transaction, so no frame can sample through to the desktop.
        let _: () = msg_send![window, setOpaque: false];
        let is_key: bool = msg_send![window, isKeyWindow];
        update_sidebar_window_surface(window, is_key);
    }
    if let Some(ui) = TITLEBAR_UI.get().and_then(slint::Weak::upgrade) {
        ui.set_native_sidebar_vibrancy_ready(true);
    }
    log::info!("[TitleBar] Installed bounded Menu vibrancy and opaque content backdrop");
    true
}

unsafe fn update_sidebar_window_surface(window: *mut Object, is_focused: bool) {
    set_native_window_active(is_focused);

    if window.is_null() || unsafe { SIDEBAR_VIBRANCY_VIEW.is_null() } {
        return;
    }

    unsafe {
        // Codex disables Menu vibrancy when its macOS window loses focus and
        // switches the transparent window surface to #f9f9f9 / #000000.
        // Hiding the retained view is the bounded-sidebar equivalent of
        // Electron's setVibrancy(null), without rebuilding AppKit hierarchy.
        let _: () = msg_send![SIDEBAR_VIBRANCY_VIEW, setHidden: !is_focused];
    }

    // AppKit treats true clearColor as a special case that can disable corner
    // masking and briefly expose desktop content before the backdrop layer is
    // ready. A near-clear color keeps BehindWindow sampling without that flash.
    let color = if is_focused {
        let (red, green, blue) = sidebar_content_surface_components();
        unsafe { nscolor_srgb(red, green, blue, 0.001) }
    } else {
        let component = if unsafe { SIDEBAR_DARK_MODE } {
            0.0
        } else {
            249.0 / 255.0
        };
        unsafe { nscolor_srgb(component, component, component, 1.0) }
    };
    if !color.is_null() {
        unsafe {
            let _: () = msg_send![window, setBackgroundColor: color];
        }
    }

    unsafe {
        apply_content_surface_backdrop_fill();
    }
}

/// AppKit keeps the standard traffic lights in its compact default control
/// band even when the content/title-bar inset is taller. Usher's breadcrumb
/// and accessory controls use the geometric center of the full 46 pt inset,
/// so lower the three standard buttons by the same Y delta. The cluster is
/// also shifted right so the close button shares the sidebar icon centerline.
fn titlebar_controls_x(anchor_x: f64, traffic_right: f64) -> f64 {
    anchor_x.max(traffic_right + 8.0)
}

fn titlebar_controls_y(traffic_y: f64, traffic_height: f64, fullscreen: bool) -> f64 {
    // Full-screen AppKit uses a different title-bar host whose RoundRect
    // NSButton content sits about 7 pt below the traffic-light center. The
    // ordinary accessory host is already centered and must not be adjusted.
    let fullscreen_content_offset = if fullscreen { 7.0 } else { 0.0 };
    traffic_y + (traffic_height - 28.0) / 2.0 + fullscreen_content_offset
}

fn traffic_light_origin(base_x: f64, base_y: f64) -> NSPoint {
    NSPoint {
        x: base_x + TRAFFIC_LIGHT_X_OFFSET,
        y: base_y - TRAFFIC_LIGHT_Y_OFFSET,
    }
}

unsafe fn align_standard_window_buttons(window: *mut Object) {
    if window.is_null() {
        return;
    }

    for button_kind in 0u64..=2u64 {
        let button: *mut Object = unsafe { msg_send![window, standardWindowButton: button_kind] };
        if button.is_null() {
            continue;
        }
        let frame: NSRect = unsafe { msg_send![button, frame] };
        let index = button_kind as usize;
        let stored_base_x = unsafe { TRAFFIC_LIGHT_BASE_X[index] };
        let stored_base_y = unsafe { TRAFFIC_LIGHT_BASE_Y[index] };
        let target = traffic_light_origin(stored_base_x, stored_base_y);
        if stored_base_x.is_finite()
            && stored_base_y.is_finite()
            && (frame.origin.x - target.x).abs() < 0.1
            && (frame.origin.y - target.y).abs() < 0.1
        {
            continue;
        }
        // If AppKit laid the title bar out again, the current position is the
        // new system baseline. Save it and apply the optical delta exactly
        // once instead of accumulating offsets across notifications.
        unsafe {
            TRAFFIC_LIGHT_BASE_X[index] = frame.origin.x;
            TRAFFIC_LIGHT_BASE_Y[index] = frame.origin.y;
        }
        let origin = traffic_light_origin(frame.origin.x, frame.origin.y);
        unsafe {
            let _: () = msg_send![button, setFrameOrigin: origin];
        }
    }

    let anchor = unsafe { TITLEBAR_ACCESSORY_ANCHOR };
    let controls = unsafe { TITLEBAR_CONTROLS_CONTAINER };
    if anchor.is_null() || controls.is_null() {
        return;
    }
    let traffic: *mut Object = unsafe { msg_send![window, standardWindowButton: 0u64] };
    if traffic.is_null() {
        return;
    }
    let titlebar_view: *mut Object = unsafe { msg_send![traffic, superview] };
    if titlebar_view.is_null() {
        return;
    }
    let traffic_frame: NSRect = unsafe { msg_send![traffic, frame] };
    let anchor_origin: NSPoint = unsafe {
        msg_send![
            anchor,
            convertPoint: NSPoint { x: 0.0, y: 0.0 }
            toView: titlebar_view
        ]
    };
    // During a full-screen transition AppKit can temporarily place the left
    // accessory anchor at the title-bar origin. Never allow the detached
    // controls container to overlap the system traffic-light cluster.
    let mut traffic_right = traffic_frame.origin.x + traffic_frame.size.width;
    for button_kind in 1u64..=2u64 {
        let button: *mut Object = unsafe { msg_send![window, standardWindowButton: button_kind] };
        if button.is_null() {
            continue;
        }
        let frame: NSRect = unsafe { msg_send![button, frame] };
        traffic_right = traffic_right.max(frame.origin.x + frame.size.width);
    }
    let style_mask: u64 = unsafe { msg_send![window, styleMask] };
    let fullscreen = style_mask_is_fullscreen(style_mask);
    let titlebar_origin = NSPoint {
        x: titlebar_controls_x(anchor_origin.x, traffic_right),
        y: titlebar_controls_y(
            traffic_frame.origin.y,
            traffic_frame.size.height,
            fullscreen,
        ),
    };
    // Full-screen AppKit may rebuild the title-bar hierarchy. The standard
    // buttons then report coordinates in the new titlebar view while our
    // detached controls container can remain attached to the previous
    // superview. Convert through the actual host before assigning the frame;
    // writing titlebar-view coordinates directly causes the 7.5 pt vertical
    // offset visible on Retina displays.
    let controls_superview: *mut Object = unsafe { msg_send![controls, superview] };
    if controls_superview.is_null() {
        return;
    }
    let origin: NSPoint = unsafe {
        msg_send![
            titlebar_view,
            convertPoint: titlebar_origin
            toView: controls_superview
        ]
    };
    unsafe {
        let _: () = msg_send![controls, setFrameOrigin: origin];
    }
}

/// Whether the startup-critical native chrome is fully installed: the Menu
/// vibrancy underlays plus the title-bar accessory controls.
pub(crate) fn startup_native_chrome_ready() -> bool {
    unsafe { TITLEBAR_INSTALLED && SIDEBAR_VIBRANCY_INSTALLED }
}

/// Re-apply the traffic-light and accessory alignment synchronously.
///
/// AppKit performs another title-bar layout shortly after accessories are
/// attached, which resets the standard buttons to their compact default band.
/// The startup reveal must run this realignment first so the first visible
/// frame already shows the traffic lights on the accessory-button centerline.
pub(crate) fn prepare_startup_reveal(window: &MainWindow) {
    let Some(window) = native_window(window) else {
        return;
    };
    unsafe {
        align_standard_window_buttons(window);
    }
}

/// Hide or reveal the native window during startup configuration.
///
/// `show()` presents the window on the next run-loop commit. Keeping the
/// window at alpha 0 until native chrome (vibrancy, backdrop, title-bar
/// accessories) and the first themed Slint frame are ready means the first
/// visible frame is already the final shell instead of a staged transition.
pub(crate) fn set_startup_presentation_hidden(window: &MainWindow, hidden: bool) {
    let Some(window) = native_window(window) else {
        return;
    };
    unsafe {
        let alpha: f64 = if hidden { 0.0 } else { 1.0 };
        let _: () = msg_send![window, setAlphaValue: alpha];
    }
}

pub(crate) fn install_titlebar_settings_button(window: &MainWindow) {
    let _ = TITLEBAR_UI.set(window.as_weak());

    unsafe {
        if TITLEBAR_INSTALLED {
            return;
        }
    }

    let Some(render_view) = native_view(window) else {
        log::warn!("[TitleBar] native render view is unavailable, skip install this round");
        return;
    };
    let Some(window) = native_window(window) else {
        log::warn!("[TitleBar] native window is unavailable, skip install this round");
        return;
    };
    unsafe { sync_native_window_fullscreen(window) };

    let delegate = unsafe {
        if TITLEBAR_DELEGATE.is_null() {
            let class = register_titlebar_delegate_class();
            let delegate: *mut Object = msg_send![class, alloc];
            let delegate: *mut Object = msg_send![delegate, init];
            let _: () = msg_send![delegate, retain];
            TITLEBAR_DELEGATE = delegate;
        }
        TITLEBAR_DELEGATE
    };
    observe_window_interaction(window, delegate);

    unsafe {
        // The Slint window starts with an empty title so AppKit cannot flash a
        // visible "Usher" label before this installation pass. Restore the
        // metadata title only after hiding the standard title presentation.
        let _: () = msg_send![window, setTitleVisibility: 1u64];
        if let Some(title) = nsstring("Usher") {
            let _: () = msg_send![window, setTitle: title];
        }
        install_sidebar_vibrancy(window, render_view);
    }

    let Some(accessory_class) = Class::get("NSTitlebarAccessoryViewController") else {
        return;
    };
    let accessory: *mut Object = unsafe { msg_send![accessory_class, alloc] };
    let accessory: *mut Object = unsafe { msg_send![accessory, init] };
    if accessory.is_null() {
        return;
    }

    let container = unsafe {
        create_view(
            "NSView",
            NSRect {
                origin: NSPoint { x: 0.0, y: 0.0 },
                size: NSSize {
                    width: 92.0,
                    // Use the full title-bar inset instead of AppKit's compact
                    // accessory baseline. This lets the 28 px controls sit on
                    // the same visual center as the traffic lights and
                    // breadcrumb without extending outside their container.
                    height: 46.0,
                },
            },
        )
    };
    let controls_container = unsafe {
        create_view(
            "NSView",
            NSRect {
                origin: NSPoint { x: 0.0, y: 0.0 },
                size: NSSize {
                    width: 92.0,
                    height: 28.0,
                },
            },
        )
    };
    let toggle_button = unsafe {
        create_view(
            "NSButton",
            NSRect {
                origin: NSPoint { x: 0.0, y: 0.0 },
                size: NSSize {
                    width: 28.0,
                    height: 28.0,
                },
            },
        )
    };
    let back_button = unsafe {
        create_view(
            "NSButton",
            NSRect {
                origin: NSPoint { x: 32.0, y: 0.0 },
                size: NSSize {
                    width: 28.0,
                    height: 28.0,
                },
            },
        )
    };
    let forward_button = unsafe {
        create_view(
            "NSButton",
            NSRect {
                origin: NSPoint { x: 64.0, y: 0.0 },
                size: NSSize {
                    width: 28.0,
                    height: 28.0,
                },
            },
        )
    };
    let modal_shield = unsafe {
        create_view(
            "NSView",
            NSRect {
                origin: NSPoint { x: 0.0, y: 0.0 },
                size: NSSize {
                    width: 92.0,
                    height: 28.0,
                },
            },
        )
    };
    let (
        Some(container),
        Some(controls_container),
        Some(toggle_button),
        Some(back_button),
        Some(forward_button),
        Some(modal_shield),
    ) = (
        container,
        controls_container,
        toggle_button,
        back_button,
        forward_button,
        modal_shield,
    )
    else {
        return;
    };

    let toggle_tooltip =
        shortcut_tooltip("Toggle sidebar", crate::shortcuts::CommandId::ToggleSidebar);
    let back_tooltip = shortcut_tooltip("Back", crate::shortcuts::CommandId::NavigateBack);
    let forward_tooltip = shortcut_tooltip("Forward", crate::shortcuts::CommandId::NavigateForward);
    unsafe {
        configure_icon_button(
            toggle_button,
            sidebar_toggle_image_names(false),
            "◫",
            &toggle_tooltip,
            delegate,
            sel!(usherToggleSidebar:),
        );
        configure_icon_button(
            back_button,
            &["symbol:arrow.left", "NSImageNameGoLeftTemplate"],
            "‹",
            &back_tooltip,
            delegate,
            sel!(usherNavigateBack:),
        );
        configure_icon_button(
            forward_button,
            &["symbol:arrow.right", "NSImageNameGoRightTemplate"],
            "›",
            &forward_tooltip,
            delegate,
            sel!(usherNavigateForward:),
        );
        let _: () = msg_send![controls_container, addSubview: toggle_button];
        let _: () = msg_send![controls_container, addSubview: back_button];
        let _: () = msg_send![controls_container, addSubview: forward_button];
        // This transparent native shield sits above application-owned title-bar
        // controls while a modal backdrop is visible. It intercepts hover,
        // Tooltip and click handling without covering AppKit's traffic lights.
        let _: () = msg_send![modal_shield, setHidden: true];
        let _: () = msg_send![controls_container, addSubview: modal_shield];
        let _: () = msg_send![accessory, setView: container];
        // NSTitlebarAccessoryLayoutAttributeLeft in the current target SDK.
        let _: () = msg_send![accessory, setLayoutAttribute: 1u64];
        let _: () = msg_send![window, addTitlebarAccessoryViewController: accessory];
        let traffic: *mut Object = msg_send![window, standardWindowButton: 0u64];
        if traffic.is_null() {
            return;
        }
        let titlebar_view: *mut Object = msg_send![traffic, superview];
        if titlebar_view.is_null() {
            return;
        }
        let _: () = msg_send![titlebar_view, addSubview: controls_container];
        TITLEBAR_ACCESSORY_ANCHOR = container;
        TITLEBAR_CONTROLS_CONTAINER = controls_container;
        // AppKit performs another title-bar layout after accessories are
        // attached. Defer alignment to the run loop, then repeat once after
        // the window has fully presented; resize notifications keep it stable.
        let _: () = msg_send![
            delegate,
            performSelector: sel!(usherAlignTrafficLights:)
            withObject: window
            afterDelay: 0.0f64
        ];
        let _: () = msg_send![
            delegate,
            performSelector: sel!(usherAlignTrafficLights:)
            withObject: window
            afterDelay: 0.25f64
        ];
        TITLEBAR_TOGGLE_BUTTON = toggle_button;
        TITLEBAR_BACK_BUTTON = back_button;
        TITLEBAR_FORWARD_BUTTON = forward_button;
        TITLEBAR_MODAL_SHIELD = modal_shield;
        TITLEBAR_INSTALLED = true;
        align_standard_window_buttons(window);
    }
    install_app_event_monitor(window);
    if let Some(ui) = TITLEBAR_UI.get().and_then(slint::Weak::upgrade) {
        update_titlebar_navigation_controls(&ui);
    }
    log::info!("[TitleBar] Added native sidebar and history controls to macOS title bar");
}

pub(crate) fn update_titlebar_navigation_controls(window: &MainWindow) {
    unsafe {
        if TITLEBAR_TOGGLE_BUTTON.is_null()
            || TITLEBAR_BACK_BUTTON.is_null()
            || TITLEBAR_FORWARD_BUTTON.is_null()
        {
            return;
        }

        let modal_visible = window.get_modal_overlay_visible();
        let _: () = msg_send![TITLEBAR_TOGGLE_BUTTON, setEnabled: !modal_visible];
        let _: () = msg_send![
            TITLEBAR_BACK_BUTTON,
            setEnabled: !modal_visible && window.get_navigation_can_go_back()
        ];
        let _: () = msg_send![
            TITLEBAR_FORWARD_BUTTON,
            setEnabled: !modal_visible && window.get_navigation_can_go_forward()
        ];
        if !TITLEBAR_MODAL_SHIELD.is_null() {
            let _: () = msg_send![TITLEBAR_MODAL_SHIELD, setHidden: !modal_visible];
        }
        let expanded = window.get_navigation_drawer_expanded();
        configure_icon_button(
            TITLEBAR_TOGGLE_BUTTON,
            sidebar_toggle_image_names(expanded),
            "◫",
            &sidebar_toggle_tooltip(expanded),
            TITLEBAR_DELEGATE,
            sel!(usherToggleSidebar:),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NS_WINDOW_STYLE_MASK_FULLSCREEN, SIDEBAR_DARK_MODE, TITLEBAR_SYMBOL_POINT_SIZE,
        macos_shortcut_key, sidebar_content_surface_components, sidebar_toggle_image_names,
        sidebar_toggle_tooltip, style_mask_is_fullscreen, titlebar_controls_x, titlebar_controls_y,
        traffic_light_origin,
    };

    #[test]
    fn physical_shortcut_keys_are_input_method_independent() {
        assert_eq!(macos_shortcut_key(11), Some("b"));
        assert_eq!(macos_shortcut_key(38), Some("j"));
        assert_eq!(macos_shortcut_key(33), Some("["));
        assert_eq!(macos_shortcut_key(30), Some("]"));
        assert_eq!(macos_shortcut_key(43), Some(","));
        assert_eq!(macos_shortcut_key(0), None);
    }

    #[test]
    fn sidebar_toggle_uses_distinct_open_and_closed_symbols() {
        assert_eq!(sidebar_toggle_image_names(false)[0], "symbol:sidebar.left");
        assert_eq!(
            sidebar_toggle_image_names(true)[0],
            "symbol:rectangle.leftthird.inset.filled"
        );
        assert_ne!(
            sidebar_toggle_image_names(false)[0],
            sidebar_toggle_image_names(true)[0]
        );
        assert_eq!(sidebar_toggle_tooltip(false), "Show sidebar (⌘B)");
        assert_eq!(sidebar_toggle_tooltip(true), "Hide sidebar (⌘B)");
    }

    #[test]
    fn titlebar_controls_stay_beyond_traffic_lights_when_fullscreen_resets_anchor() {
        assert_eq!(titlebar_controls_x(7.0, 69.0), 77.0);
        assert_eq!(titlebar_controls_x(82.0, 69.0), 82.0);
    }

    #[test]
    fn native_fullscreen_style_mask_is_classified_without_other_bits() {
        assert!(!style_mask_is_fullscreen(0));
        assert!(style_mask_is_fullscreen(NS_WINDOW_STYLE_MASK_FULLSCREEN));
        assert!(style_mask_is_fullscreen(
            NS_WINDOW_STYLE_MASK_FULLSCREEN | 1u64
        ));
    }

    #[test]
    fn titlebar_controls_compensate_for_native_button_image_baseline() {
        assert_eq!(titlebar_controls_y(10.0, 14.0, true), 10.0);
        assert_eq!(titlebar_controls_y(10.0, 14.0, false), 3.0);
    }

    #[test]
    fn traffic_lights_shift_right_onto_sidebar_icon_centerline() {
        // Default AppKit close origin x=9 (center 16). After +8, center is 24,
        // matching GitHubMetrics.sidebar-nav-icon-center-x.
        let origin = traffic_light_origin(9.0, 16.0);
        assert_eq!(origin.x, 17.0);
        assert_eq!(origin.y, 9.0);
    }

    #[test]
    fn titlebar_symbol_point_size_stays_on_whole_points() {
        assert_eq!(TITLEBAR_SYMBOL_POINT_SIZE, 14.0);
        assert_eq!(TITLEBAR_SYMBOL_POINT_SIZE.fract(), 0.0);
    }

    #[test]
    fn content_surface_components_follow_sidebar_theme() {
        unsafe {
            SIDEBAR_DARK_MODE = false;
            assert_eq!(sidebar_content_surface_components(), (1.0, 1.0, 1.0));
            SIDEBAR_DARK_MODE = true;
            assert_eq!(
                sidebar_content_surface_components(),
                (12.0 / 255.0, 14.0 / 255.0, 19.0 / 255.0)
            );
            SIDEBAR_DARK_MODE = false;
        }
    }
}

/// Register GitHub's macOS UI and monospace faces without bundling Apple fonts.
pub(crate) fn setup_pr_review_fonts(app: &MainWindow) {
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
        app.set_workflow_pr_review_code_font_family("SF Mono".into());
        log::info!("[PR Review] Using GitHub-compatible SFMono-Regular code font");
    } else {
        app.set_workflow_pr_review_code_font_family("Menlo".into());
        log::warn!("[PR Review] SFMono-Regular unavailable; falling back to Menlo");
    }

    if collection.family_id(".SF NS").is_some() {
        app.set_workflow_pr_review_tree_font_family(".SF NS".into());
        log::info!("[PR Review] Using Fontique's GitHub-compatible macOS system UI font");
    } else {
        app.set_workflow_pr_review_tree_font_family("Helvetica Neue".into());
        log::warn!("[PR Review] macOS system UI font unavailable; falling back to Helvetica Neue");
    }
}

/// Observe macOS appearance flips and refresh Auto theme promptly.
///
/// `AppleInterfaceThemeChangedNotification` is delivered on the distributed
/// notification center. A short delay lets `NSUserDefaults` settle before we
/// re-read `AppleInterfaceStyle`.
pub(crate) fn install_system_theme_change_observer() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        static mut OBSERVER_CLASS: *const Class = std::ptr::null();
        static mut OBSERVER: *mut Object = std::ptr::null_mut();

        unsafe {
            let Some(superclass) = Class::get("NSObject") else {
                log::warn!("[Theme] NSObject unavailable for theme observer");
                return;
            };
            let Some(mut decl) = ClassDecl::new("UsherSystemThemeObserver", superclass) else {
                log::warn!("[Theme] Failed to declare system theme observer class");
                return;
            };

            extern "C" fn theme_changed(_this: &Object, _cmd: Sel, _notification: *mut Object) {
                let _ = std::thread::Builder::new()
                    .name("system-theme-notify".to_string())
                    .spawn(|| {
                        // Defaults can lag the distributed notification slightly.
                        std::thread::sleep(std::time::Duration::from_millis(120));
                        super::publish_system_theme_if_changed();
                    });
            }

            decl.add_method(
                sel!(usherSystemThemeChanged:),
                theme_changed as extern "C" fn(&Object, Sel, *mut Object),
            );
            OBSERVER_CLASS = decl.register();

            let observer_class = &*OBSERVER_CLASS;
            let observer: *mut Object = msg_send![observer_class, alloc];
            let observer: *mut Object = msg_send![observer, init];
            if observer.is_null() {
                log::warn!("[Theme] Failed to allocate system theme observer");
                return;
            }
            let _: () = msg_send![observer, retain];
            OBSERVER = observer;

            let Some(center_class) = Class::get("NSDistributedNotificationCenter") else {
                log::warn!("[Theme] NSDistributedNotificationCenter unavailable");
                return;
            };
            let center: *mut Object = msg_send![center_class, defaultCenter];
            if center.is_null() {
                return;
            }
            let Some(name) = nsstring("AppleInterfaceThemeChangedNotification") else {
                return;
            };
            let _: () = msg_send![
                center,
                addObserver: observer
                selector: sel!(usherSystemThemeChanged:)
                name: name
                object: std::ptr::null::<Object>()
            ];
            log::info!("[Theme] Listening for AppleInterfaceThemeChangedNotification");
        }
    });
}

/// Play a short system UI click via AppKit `NSSound`.
///
/// `Tink` is the lightest built-in alert; suitable for a breadcrumb Back press.
pub(crate) fn play_ui_click() {
    let Some(sound_class) = Class::get("NSSound") else {
        return;
    };
    let Some(name) = nsstring("Tink") else {
        return;
    };
    let sound: *mut Object = unsafe { msg_send![sound_class, soundNamed: name] };
    if sound.is_null() {
        return;
    }
    let played: bool = unsafe { msg_send![sound, play] };
    if !played {
        log::debug!("[Sound] NSSound play returned false for Tink");
    }
}
