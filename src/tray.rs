use anyhow::Result;
use slint::Weak;

use crate::MainWindow;

// ==================== macOS Native Implementation ====================
#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc::runtime::{BOOL, Class, Object, Sel, YES};
    use objc::{msg_send, sel, sel_impl};
    use std::ffi::CString;
    use std::sync::Once;

    // Store the UI weak reference globally for callbacks
    static mut UI_WEAK: Option<Weak<MainWindow>> = None;
    static INIT: Once = Once::new();

    // Store status item reference
    static mut STATUS_ITEM: *mut Object = std::ptr::null_mut();

    // Store main window reference (retained)
    static mut MAIN_WINDOW: *mut Object = std::ptr::null_mut();

    // Store show/hide menu item reference for dynamic text update
    static mut SHOW_HIDE_MENU_ITEM: *mut Object = std::ptr::null_mut();

    /// Setup system tray icon with context menu on macOS
    pub fn setup_tray_macos(ui_weak: Weak<MainWindow>) -> Result<()> {
        unsafe {
            // Store UI weak reference for callbacks
            INIT.call_once(|| {});
            UI_WEAK = Some(ui_weak);

            // Set activation policy to prevent app from quitting when window is hidden
            // NSApplicationActivationPolicyAccessory = 1 (app runs in background, no dock icon)
            // NSApplicationActivationPolicyRegular = 0 (normal app with dock icon)
            let ns_app_class = Class::get("NSApplication")
                .ok_or_else(|| anyhow::anyhow!("Failed to get NSApplication class"))?;
            let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];
            // Use Regular policy (0) to keep dock icon, or Accessory (1) to hide dock icon
            let _: () = msg_send![ns_app, setActivationPolicy: 0i64]; // NSApplicationActivationPolicyRegular

            // Get NSStatusBar
            let status_bar_class = Class::get("NSStatusBar")
                .ok_or_else(|| anyhow::anyhow!("Failed to get NSStatusBar class"))?;
            let status_bar: *mut Object = msg_send![status_bar_class, systemStatusBar];

            // Create status item with variable length
            let status_item: *mut Object = msg_send![status_bar, statusItemWithLength: -1.0_f64]; // NSVariableStatusItemLength
            let _: () = msg_send![status_item, retain];
            STATUS_ITEM = status_item;

            // Get the button of the status item
            let button: *mut Object = msg_send![status_item, button];
            if !button.is_null() {
                // Set title (fallback if no icon)
                let title = create_nsstring("U");
                let _: () = msg_send![button, setTitle: title];

                // Try to load icon
                if let Some(image) = load_status_bar_icon() {
                    let _: () = msg_send![button, setImage: image];
                    let _: () = msg_send![button, setTitle: create_nsstring("")];
                }
            }

            // Create and set menu
            let menu = create_menu()?;
            let _: () = msg_send![status_item, setMenu: menu];

            // Store main window reference for later use (for show/hide functionality)
            let main_window: *mut Object = msg_send![ns_app, mainWindow];
            if !main_window.is_null() {
                let _: () = msg_send![main_window, retain];
                MAIN_WINDOW = main_window;
            } else {
                // If mainWindow is null, try to get the first window
                let windows: *mut Object = msg_send![ns_app, windows];
                let count: usize = msg_send![windows, count];
                if count > 0 {
                    let first_window: *mut Object = msg_send![windows, objectAtIndex: 0usize];
                    let _: () = msg_send![first_window, retain];
                    MAIN_WINDOW = first_window;
                }
            }

            println!("macOS tray icon created successfully");
            Ok(())
        }
    }

    /// Load icon for status bar
    unsafe fn load_status_bar_icon() -> Option<*mut Object> {
        // Try to load from logo.png
        let path = CString::new("logo.png").ok()?;
        let ns_string: *mut Object =
            msg_send![Class::get("NSString")?, stringWithUTF8String: path.as_ptr()];

        let ns_image_class = Class::get("NSImage")?;
        let image: *mut Object = msg_send![ns_image_class, alloc];
        let image: *mut Object = msg_send![image, initWithContentsOfFile: ns_string];

        if image.is_null() {
            return None;
        }

        // Resize to appropriate size for menu bar (18x18 is standard)
        let size = NSSize {
            width: 18.0,
            height: 18.0,
        };
        let _: () = msg_send![image, setSize: size];

        // Set as template image for proper dark/light mode support
        let _: () = msg_send![image, setTemplate: YES];

        Some(image)
    }

    /// Create context menu
    unsafe fn create_menu() -> Result<*mut Object> {
        let ns_menu_class =
            Class::get("NSMenu").ok_or_else(|| anyhow::anyhow!("Failed to get NSMenu class"))?;

        let menu: *mut Object = msg_send![ns_menu_class, alloc];
        let menu: *mut Object = msg_send![menu, init];

        // Register our custom delegate class
        let delegate_class = register_menu_delegate_class();
        let delegate: *mut Object = msg_send![delegate_class, alloc];
        let delegate: *mut Object = msg_send![delegate, init];

        // Show/Hide Window menu item (initially "Hide Window" since window starts visible)
        let show_item = unsafe { create_menu_item("Hide Window", sel!(showWindow:), delegate)? };
        let _: () = msg_send![show_item, retain];
        unsafe {
            SHOW_HIDE_MENU_ITEM = show_item;
        }
        let _: () = msg_send![menu, addItem: show_item];

        // Separator
        let ns_menu_item_class = Class::get("NSMenuItem")
            .ok_or_else(|| anyhow::anyhow!("Failed to get NSMenuItem class"))?;
        let separator: *mut Object = msg_send![ns_menu_item_class, separatorItem];
        let _: () = msg_send![menu, addItem: separator];

        // Quit menu item
        let quit_item = unsafe { create_menu_item("Quit", sel!(quitApp:), delegate)? };
        let _: () = msg_send![menu, addItem: quit_item];

        Ok(menu)
    }

    /// Create a menu item
    unsafe fn create_menu_item(
        title: &str,
        action: Sel,
        target: *mut Object,
    ) -> Result<*mut Object> {
        let ns_menu_item_class = Class::get("NSMenuItem")
            .ok_or_else(|| anyhow::anyhow!("Failed to get NSMenuItem class"))?;

        let item: *mut Object = msg_send![ns_menu_item_class, alloc];
        let title_ns = unsafe { create_nsstring(title) };
        let key_equiv = unsafe { create_nsstring("") };
        let item: *mut Object =
            msg_send![item, initWithTitle: title_ns action: action keyEquivalent: key_equiv];
        let _: () = msg_send![item, setTarget: target];

        Ok(item)
    }

    /// Create NSString from Rust string
    unsafe fn create_nsstring(s: &str) -> *mut Object {
        let c_str = CString::new(s).unwrap();
        let ns_string_class = Class::get("NSString").unwrap();
        msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()]
    }

    /// Register custom delegate class for menu actions
    fn register_menu_delegate_class() -> &'static Class {
        use objc::declare::ClassDecl;
        use std::sync::Once;

        static mut DELEGATE_CLASS: *const Class = std::ptr::null();
        static REGISTER: Once = Once::new();

        REGISTER.call_once(|| {
            let superclass = Class::get("NSObject").unwrap();
            let mut decl = ClassDecl::new("UsherMenuDelegate", superclass).unwrap();

            // Add showWindow: method - toggle window visibility
            extern "C" fn show_window(_this: &Object, _cmd: Sel, _sender: *mut Object) {
                unsafe {
                    // Use the saved window reference
                    if !MAIN_WINDOW.is_null() {
                        let ns_app_class = Class::get("NSApplication").unwrap();
                        let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];

                        let is_visible: BOOL = msg_send![MAIN_WINDOW, isVisible];

                        if is_visible == YES {
                            // Hide window
                            let _: () = msg_send![MAIN_WINDOW, orderOut: std::ptr::null::<Object>()];
                            // Update menu item text to "Show Window"
                            update_menu_item_title("Show Window");
                        } else {
                            // Show window and bring to front
                            let _: () = msg_send![MAIN_WINDOW, makeKeyAndOrderFront: std::ptr::null::<Object>()];
                            let _: () = msg_send![ns_app, activateIgnoringOtherApps: YES];
                            // Update menu item text to "Hide Window"
                            update_menu_item_title("Hide Window");
                        }
                    }
                }
            }

            // Helper function to update menu item title
            fn update_menu_item_title(title: &str) {
                unsafe {
                    if !SHOW_HIDE_MENU_ITEM.is_null() {
                        let c_str = std::ffi::CString::new(title).unwrap();
                        let ns_string_class = Class::get("NSString").unwrap();
                        let title_ns: *mut Object = msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
                        let _: () = msg_send![SHOW_HIDE_MENU_ITEM, setTitle: title_ns];
                    }
                }
            }

            // Add quitApp: method
            extern "C" fn quit_app(_this: &Object, _cmd: Sel, _sender: *mut Object) {
                std::process::exit(0);
            }

            unsafe {
                decl.add_method(
                    sel!(showWindow:),
                    show_window as extern "C" fn(&Object, Sel, *mut Object),
                );
                decl.add_method(
                    sel!(quitApp:),
                    quit_app as extern "C" fn(&Object, Sel, *mut Object),
                );

                DELEGATE_CLASS = decl.register();
            }
        });

        unsafe { &*DELEGATE_CLASS }
    }

    /// Remove tray icon
    #[allow(dead_code)]
    pub fn remove_tray_macos() {
        unsafe {
            if !STATUS_ITEM.is_null() {
                let status_bar_class = Class::get("NSStatusBar").unwrap();
                let status_bar: *mut Object = msg_send![status_bar_class, systemStatusBar];
                let _: () = msg_send![status_bar, removeStatusItem: STATUS_ITEM];
                let _: () = msg_send![STATUS_ITEM, release];
                STATUS_ITEM = std::ptr::null_mut();
            }
        }
    }

    /// Update tray tooltip (title on macOS)
    #[allow(dead_code)]
    pub fn update_tray_tooltip_macos(tooltip: &str) {
        unsafe {
            if !STATUS_ITEM.is_null() {
                let button: *mut Object = msg_send![STATUS_ITEM, button];
                if !button.is_null() {
                    let tooltip_ns = create_nsstring(tooltip);
                    let _: () = msg_send![button, setToolTip: tooltip_ns];
                }
            }
        }
    }

    #[repr(C)]
    struct NSSize {
        width: f64,
        height: f64,
    }
}

// ==================== Other Platforms Implementation ====================
#[cfg(not(target_os = "macos"))]
mod other {
    use super::*;
    use anyhow::Context;
    use std::thread;
    use tray_icon::{
        TrayIcon, TrayIconBuilder, TrayIconEvent,
        menu::{Menu, MenuEvent, MenuId, MenuItem},
    };

    // Global tray icon reference
    thread_local! {
        static TRAY_ICON: std::cell::RefCell<Option<TrayIcon>> = std::cell::RefCell::new(None);
    }

    /// Setup system tray icon on other platforms
    pub fn setup_tray_other(ui_weak: Weak<MainWindow>) -> Result<()> {
        let icon = load_tray_icon()?;
        let (menu, show_hide_id, quit_id) = create_tray_menu()?;

        let tray_icon = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("Usher")
            .with_menu(Box::new(menu))
            .build()
            .context("Failed to create tray icon")?;

        TRAY_ICON.with(|tray| {
            *tray.borrow_mut() = Some(tray_icon);
        });

        let ui_weak_clone = ui_weak.clone();
        thread::spawn(move || {
            handle_tray_events(ui_weak_clone, show_hide_id, quit_id);
        });

        Ok(())
    }

    fn load_tray_icon() -> Result<tray_icon::Icon> {
        if let Ok(img) = image::open("logo.png") {
            let resized = img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3);
            let rgba = resized.to_rgba8();
            let (width, height) = rgba.dimensions();
            return Ok(tray_icon::Icon::from_rgba(rgba.into_raw(), width, height)?);
        }

        let mut rgba_data = Vec::new();
        for _ in 0..32 {
            for _ in 0..32 {
                rgba_data.extend_from_slice(&[100, 150, 255, 255]);
            }
        }
        Ok(tray_icon::Icon::from_rgba(rgba_data, 32, 32)?)
    }

    fn create_tray_menu() -> Result<(Menu, MenuId, MenuId)> {
        let menu = Menu::new();
        let show_hide_item = MenuItem::new("Show Window", true, None);
        let show_hide_id = show_hide_item.id().clone();
        menu.append(&show_hide_item)?;
        menu.append(&MenuItem::new("---", true, None))?;
        let quit_item = MenuItem::new("Quit", true, None);
        let quit_id = quit_item.id().clone();
        menu.append(&quit_item)?;
        Ok((menu, show_hide_id, quit_id))
    }

    fn handle_tray_events(ui_weak: Weak<MainWindow>, show_hide_id: MenuId, quit_id: MenuId) {
        loop {
            if let Ok(event) = MenuEvent::receiver().try_recv() {
                let ui_weak_clone = ui_weak.clone();
                if event.id == show_hide_id {
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_clone.upgrade() {
                            let window = ui.window();
                            if window.is_visible() {
                                window.hide();
                            } else {
                                window.show();
                            }
                        }
                    })
                    .ok();
                } else if event.id == quit_id {
                    std::process::exit(0);
                }
            }

            if let Ok(_event) = TrayIconEvent::receiver().try_recv() {
                let ui_weak_clone = ui_weak.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak_clone.upgrade() {
                        let window = ui.window();
                        if window.is_visible() {
                            window.hide();
                        } else {
                            window.show();
                        }
                    }
                })
                .ok();
            }

            thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    pub fn remove_tray_other() {
        TRAY_ICON.with(|tray| {
            *tray.borrow_mut() = None;
        });
    }

    pub fn update_tray_tooltip_other(tooltip: &str) {
        TRAY_ICON.with(|tray| {
            if let Some(ref mut icon) = *tray.borrow_mut() {
                icon.set_tooltip(Some(tooltip.to_string()));
            }
        });
    }
}

// ==================== Public API ====================

/// Setup system tray icon with context menu
pub fn setup_tray(ui_weak: Weak<MainWindow>) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::setup_tray_macos(ui_weak)
    }

    #[cfg(not(target_os = "macos"))]
    {
        other::setup_tray_other(ui_weak)
    }
}

/// Update tray icon tooltip
#[allow(dead_code)]
pub fn update_tray_tooltip(tooltip: &str) {
    #[cfg(target_os = "macos")]
    {
        macos::update_tray_tooltip_macos(tooltip);
    }

    #[cfg(not(target_os = "macos"))]
    {
        other::update_tray_tooltip_other(tooltip);
    }
}

/// Remove tray icon
#[allow(dead_code)]
pub fn remove_tray() {
    #[cfg(target_os = "macos")]
    {
        macos::remove_tray_macos();
    }

    #[cfg(not(target_os = "macos"))]
    {
        other::remove_tray_other();
    }
}
