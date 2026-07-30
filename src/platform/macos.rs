use std::ffi::CString;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use raw_window_handle::HasWindowHandle;
use slint::ComponentHandle;

use crate::{MainWindow, Route};

static TITLEBAR_UI: std::sync::OnceLock<slint::Weak<MainWindow>> = std::sync::OnceLock::new();
static WINDOW_MOVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WINDOW_INTERACTING: AtomicBool = AtomicBool::new(false);

static mut TITLEBAR_DELEGATE: *mut Object = std::ptr::null_mut();
static mut TITLEBAR_INSTALLED: bool = false;

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

fn native_window(window: &MainWindow) -> Option<*mut Object> {
    let window_handle = window.window().window_handle();
    let handle = window_handle.window_handle().ok()?;
    let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = handle.as_ref() else {
        return None;
    };
    let view = appkit_handle.ns_view.as_ptr() as *mut Object;
    if view.is_null() {
        return None;
    }
    let window: *mut Object = unsafe { msg_send![view, window] };
    (!window.is_null()).then_some(window)
}

pub(crate) fn set_title_bar_color(window: &MainWindow, is_dark: bool) {
    let Some(window) = native_window(window) else {
        return;
    };
    unsafe {
        let _: () = msg_send![window, setTitlebarAppearsTransparent: true];
        let _: () = msg_send![window, setOpaque: false];
        // Do not let terminal/diff drag surfaces move the native window.
        let _: () = msg_send![window, setMovableByWindowBackground: false];
    }

    if let Some(appearance) = appearance(is_dark) {
        unsafe {
            let _: () = msg_send![window, setAppearance: appearance];
        }
    }

    if let Some(color_class) = Class::get("NSColor") {
        let (red, green, blue) = if is_dark {
            (12.0 / 255.0, 14.0 / 255.0, 19.0 / 255.0)
        } else {
            (1.0, 1.0, 1.0)
        };
        let color: *mut Object = unsafe {
            msg_send![
                color_class,
                colorWithSRGBRed: red
                green: green
                blue: blue
                alpha: 1.0
            ]
        };
        if !color.is_null() {
            unsafe {
                let _: () = msg_send![window, setBackgroundColor: color];
            }
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
            let Some(image_name) = nsstring(image_name) else {
                continue;
            };
            let image: *mut Object = unsafe { msg_send![image_class, imageNamed: image_name] };
            if image.is_null() {
                continue;
            }
            unsafe {
                let _: () = msg_send![button, setImage: image];
                if let Some(empty) = nsstring("") {
                    let _: () = msg_send![button, setTitle: empty];
                }
                let _: () = msg_send![button, setImagePosition: 2u64];
                let _: () = msg_send![button, setImageScaling: 3u64];
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
        let font: *mut Object = unsafe { msg_send![font_class, systemFontOfSize: 24.0f64] };
        if !font.is_null() {
            unsafe {
                let _: () = msg_send![button, setFont: font];
            }
        }
    }

    unsafe {
        let _: () = msg_send![button, setBordered: false];
        if let Some(tooltip) = nsstring(tooltip) {
            let _: () = msg_send![button, setToolTip: tooltip];
        }
        let _: () = msg_send![button, setTarget: delegate];
        let _: () = msg_send![button, setAction: action];
    }
}

pub(crate) fn install_titlebar_settings_button(window: &MainWindow) {
    let _ = TITLEBAR_UI.set(window.as_weak());

    unsafe {
        if TITLEBAR_INSTALLED {
            return;
        }
    }

    let Some(window) = native_window(window) else {
        log::warn!("[TitleBar] native window is unavailable, skip install this round");
        return;
    };

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
                    width: 104.0,
                    height: 32.0,
                },
            },
        )
    };
    let settings_button = unsafe {
        create_view(
            "NSButton",
            NSRect {
                origin: NSPoint { x: 20.0, y: -2.0 },
                size: NSSize {
                    width: 48.0,
                    height: 36.0,
                },
            },
        )
    };
    let about_button = unsafe {
        create_view(
            "NSButton",
            NSRect {
                origin: NSPoint { x: 52.0, y: -1.0 },
                size: NSSize {
                    width: 48.0,
                    height: 36.0,
                },
            },
        )
    };
    let (Some(container), Some(settings_button), Some(about_button)) =
        (container, settings_button, about_button)
    else {
        return;
    };

    unsafe {
        configure_icon_button(
            settings_button,
            &[
                "NSImageNamePreferencesGeneral",
                "NSImageNameAdvanced",
                "NSImageNameActionTemplate",
            ],
            "⚙︎",
            "Settings",
            delegate,
            sel!(usherOpenSettings:),
        );
        configure_icon_button(
            about_button,
            &["NSImageNameInfo", "NSImageNameActionTemplate"],
            "ℹ︎",
            "About",
            delegate,
            sel!(usherOpenAbout:),
        );
        let _: () = msg_send![container, addSubview: settings_button];
        let _: () = msg_send![container, addSubview: about_button];
        let _: () = msg_send![accessory, setView: container];
        // NSTitlebarAccessoryLayoutAttributeRight in the current target SDK.
        let _: () = msg_send![accessory, setLayoutAttribute: 2u64];
        let _: () = msg_send![window, addTitlebarAccessoryViewController: accessory];
        TITLEBAR_INSTALLED = true;
    }
    log::info!("[TitleBar] Added native Settings/About buttons to macOS title bar");
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
