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
    use std::sync::OnceLock;

    // Store the UI weak reference globally for callbacks
    static UI_WEAK: OnceLock<Weak<MainWindow>> = OnceLock::new();

    // Store status item reference
    static mut STATUS_ITEM: *mut Object = std::ptr::null_mut();

    // Store main window reference (retained)
    static mut MAIN_WINDOW: *mut Object = std::ptr::null_mut();

    // Store delegate reference for button action
    static mut BUTTON_DELEGATE: *mut Object = std::ptr::null_mut();
    // Store observer for app-activation notifications (Dock click, Cmd-Tab, etc.)
    static mut REOPEN_OBSERVER: *mut Object = std::ptr::null_mut();

    // Store menu reference for programmatic display (right-click menu)
    static mut TRAY_MENU: *mut Object = std::ptr::null_mut();

    /// Setup system tray icon with context menu on macOS
    pub fn setup_tray_macos(ui_weak: Weak<MainWindow>) -> Result<()> {
        unsafe {
            // Store UI weak reference for callbacks
            let _ = UI_WEAK.set(ui_weak);

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

            // Register delegate class first
            let delegate_class = register_menu_delegate_class();
            let delegate: *mut Object = msg_send![delegate_class, alloc];
            let delegate: *mut Object = msg_send![delegate, init];
            let _: () = msg_send![delegate, retain];
            BUTTON_DELEGATE = delegate;
            register_reopen_observer(ns_app)?;

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
                } else {
                    log::warn!("Failed to load tray icon, using text fallback 'U'");
                }

                // Set click action on the button
                let _: () = msg_send![button, setTarget: delegate];
                let _: () = msg_send![button, setAction: sel!(trayIconClicked:)];
                // Send action on both left and right mouse down events
                // NSEventMaskLeftMouseDown = 1 << 1 = 2
                // NSEventMaskRightMouseDown = 1 << 3 = 8
                // Combined: 2 | 8 = 10
                let _: () = msg_send![button, sendActionOn: 10u64];
            }

            // Create simple menu with only Quit option (shown on right-click)
            let menu = create_quit_menu(delegate)?;
            let _: () = msg_send![menu, retain];
            TRAY_MENU = menu;

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

            log::info!("macOS tray icon created successfully");
            Ok(())
        }
    }

    /// Find logo.png file in various possible locations
    fn find_logo_path() -> Option<std::path::PathBuf> {
        // Try multiple locations:
        // 1. Current working directory (for development)
        let cwd_path = std::path::Path::new("logo.png");
        if cwd_path.exists() {
            return Some(cwd_path.to_path_buf());
        }

        // 2. Executable directory
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            let exe_dir_logo = exe_dir.join("logo.png");
            if exe_dir_logo.exists() {
                return Some(exe_dir_logo);
            }
        }

        // 3. .app bundle Resources directory (for packaged app)
        if let Ok(exe_path) = std::env::current_exe() {
            let exe_str = exe_path.to_string_lossy();
            if let Some(app_pos) = exe_str.find(".app/") {
                // We're in a .app bundle
                let app_path = std::path::Path::new(&exe_str[..app_pos + 4]);
                let resources_path = app_path.join("Contents/Resources/logo.png");
                if resources_path.exists() {
                    return Some(resources_path);
                }
            }
        }

        // 4. Try to find from executable path going up to find project root
        if let Ok(exe_path) = std::env::current_exe() {
            let mut current = exe_path.parent();
            while let Some(dir) = current {
                let logo_path = dir.join("logo.png");
                if logo_path.exists() {
                    return Some(logo_path);
                }
                // Stop if we've gone too far up (e.g., reached /)
                if dir.parent() == Some(dir) {
                    break;
                }
                current = dir.parent();
            }
        }

        None
    }

    /// Load icon for status bar
    unsafe fn load_status_bar_icon() -> Option<*mut Object> {
        // Find logo.png in various locations
        let logo_path = find_logo_path()?;
        let path_str = logo_path.to_string_lossy();

        let path = CString::new(path_str.as_ref()).ok()?;
        let ns_string: *mut Object =
            msg_send![Class::get("NSString")?, stringWithUTF8String: path.as_ptr()];

        let ns_image_class = Class::get("NSImage")?;
        let image: *mut Object = msg_send![ns_image_class, alloc];
        let image: *mut Object = msg_send![image, initWithContentsOfFile: ns_string];

        if image.is_null() {
            log::error!("Failed to load tray icon from: {}", path_str);
            return None;
        }

        // Resize to appropriate size for menu bar (18x18 is standard)
        let size = NSSize {
            width: 18.0,
            height: 18.0,
        };
        let _: () = msg_send![image, setSize: size];

        // Do not force template rendering to preserve logo shape/colors
        let _: () = msg_send![image, setTemplate: YES];

        log::info!("Successfully loaded tray icon from: {}", path_str);
        Some(image)
    }

    /// Create simple quit menu (only Quit option)
    unsafe fn create_quit_menu(delegate: *mut Object) -> Result<*mut Object> {
        unsafe {
            let ns_menu_class = Class::get("NSMenu")
                .ok_or_else(|| anyhow::anyhow!("Failed to get NSMenu class"))?;

            let menu: *mut Object = msg_send![ns_menu_class, alloc];
            let menu: *mut Object = msg_send![menu, init];

            // Only Quit menu item
            let quit_item = create_menu_item("Quit", sel!(quitApp:), delegate)?;
            let _: () = msg_send![menu, addItem: quit_item];

            Ok(menu)
        }
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

    /// Show and activate the main window
    unsafe fn show_main_window() {
        unsafe {
            // Prefer Slint API to keep internal visibility state in sync.
            if let Some(ui_weak) = UI_WEAK.get()
                && let Some(ui) = ui_weak.upgrade()
            {
                use slint::ComponentHandle;
                if let Err(e) = ui.show() {
                    log::error!("[Tray] Failed to show window via Slint API: {}", e);
                } else {
                    log::info!("[Tray] Window shown via Slint API");
                }
            }

            if MAIN_WINDOW.is_null() {
                return;
            }

            let ns_app_class = Class::get("NSApplication").unwrap();
            let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];

            // If window is minimized, deminiaturize it
            let is_miniaturized: BOOL = msg_send![MAIN_WINDOW, isMiniaturized];
            if is_miniaturized == YES {
                let _: () = msg_send![MAIN_WINDOW, deminiaturize: std::ptr::null::<Object>()];
            }

            // Show and bring to front
            let _: () = msg_send![MAIN_WINDOW, makeKeyAndOrderFront: std::ptr::null::<Object>()];
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: YES];
        }
    }

    /// Show the tray menu at the status item location
    unsafe fn show_tray_menu() {
        unsafe {
            if TRAY_MENU.is_null() || STATUS_ITEM.is_null() {
                return;
            }
            // Pop up the menu below the status item
            let _: () = msg_send![STATUS_ITEM, popUpStatusItemMenu: TRAY_MENU];
        }
    }

    /// Check if current event is a right-click
    unsafe fn is_right_click() -> bool {
        unsafe {
            let ns_app_class = Class::get("NSApplication").unwrap();
            let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];
            let current_event: *mut Object = msg_send![ns_app, currentEvent];

            if current_event.is_null() {
                return false;
            }

            // NSEventTypeRightMouseDown = 3
            // NSEventTypeRightMouseUp = 4
            let event_type: u64 = msg_send![current_event, type];

            // Also check for control+click (which is also right-click on macOS)
            let modifier_flags: u64 = msg_send![current_event, modifierFlags];
            // NSEventModifierFlagControl = 1 << 18 = 262144
            let control_pressed = (modifier_flags & 262144) != 0;

            event_type == 3 || event_type == 4 || control_pressed
        }
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

            // Add trayIconClicked: method
            // Left click: show window
            // Right click: show menu
            extern "C" fn tray_icon_clicked(_this: &Object, _cmd: Sel, _sender: *mut Object) {
                unsafe {
                    if is_right_click() {
                        // Right click: show menu
                        show_tray_menu();
                    } else {
                        // Left click: show window
                        show_main_window();
                    }
                }
            }

            // Add quitApp: method
            extern "C" fn quit_app(_this: &Object, _cmd: Sel, _sender: *mut Object) {
                crate::shutdown::request_shutdown("tray-quit");
            }

            unsafe {
                decl.add_method(
                    sel!(trayIconClicked:),
                    tray_icon_clicked as extern "C" fn(&Object, Sel, *mut Object),
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

    /// Register observer for application activation notifications.
    /// This avoids replacing Winit's NSApplication delegate.
    unsafe fn register_reopen_observer(_ns_app: *mut Object) -> Result<()> {
        use objc::declare::ClassDecl;
        use std::sync::Once;

        static mut OBSERVER_CLASS: *const Class = std::ptr::null();
        static REGISTER: Once = Once::new();

        REGISTER.call_once(|| {
            let superclass = Class::get("NSObject").unwrap();
            let mut decl = ClassDecl::new("UsherReopenObserver", superclass).unwrap();

            extern "C" fn app_did_become_active(
                _this: &Object,
                _cmd: Sel,
                _notification: *mut Object,
            ) {
                unsafe {
                    if MAIN_WINDOW.is_null() {
                        return;
                    }
                    let is_visible: BOOL = msg_send![MAIN_WINDOW, isVisible];
                    if is_visible == YES {
                        return;
                    }
                    log::info!("[Dock] Reopen handled");
                    show_main_window();
                }
            }

            // Handle Dock "reopen" AppleEvent directly.
            // This event is delivered even when the app is already active,
            // which avoids the timing gap where DidBecomeActive may not fire.
            extern "C" fn handle_reopen_event(
                _this: &Object,
                _cmd: Sel,
                _event: *mut Object,
                _reply_event: *mut Object,
            ) {
                unsafe {
                    if MAIN_WINDOW.is_null() {
                        return;
                    }
                    let is_visible: BOOL = msg_send![MAIN_WINDOW, isVisible];
                    if is_visible == YES {
                        return;
                    }
                    log::info!("[Dock] Reopen handled");
                    show_main_window();
                }
            }

            unsafe {
                decl.add_method(
                    sel!(appDidBecomeActive:),
                    app_did_become_active as extern "C" fn(&Object, Sel, *mut Object),
                );
                decl.add_method(
                    sel!(handleReopenEvent:withReplyEvent:),
                    handle_reopen_event as extern "C" fn(&Object, Sel, *mut Object, *mut Object),
                );
                OBSERVER_CLASS = decl.register();
            }
        });

        let observer_class = unsafe { &*OBSERVER_CLASS };
        let observer: *mut Object = msg_send![observer_class, alloc];
        let observer: *mut Object = msg_send![observer, init];
        unsafe {
            REOPEN_OBSERVER = observer;
        }

        let center_class = Class::get("NSNotificationCenter")
            .ok_or_else(|| anyhow::anyhow!("Failed to get NSNotificationCenter class"))?;
        let center: *mut Object = msg_send![center_class, defaultCenter];

        let ns_string_class = Class::get("NSString")
            .ok_or_else(|| anyhow::anyhow!("Failed to get NSString class"))?;
        let name_cstr = CString::new("NSApplicationDidBecomeActiveNotification")
            .map_err(|_| anyhow::anyhow!("Invalid notification name"))?;
        let name: *mut Object =
            msg_send![ns_string_class, stringWithUTF8String: name_cstr.as_ptr()];

        let _: () = msg_send![
            center,
            addObserver: observer
            selector: sel!(appDidBecomeActive:)
            name: name
            object: std::ptr::null::<Object>()
        ];

        let apple_event_manager_class = Class::get("NSAppleEventManager")
            .ok_or_else(|| anyhow::anyhow!("Failed to get NSAppleEventManager class"))?;
        let apple_event_manager: *mut Object =
            msg_send![apple_event_manager_class, sharedAppleEventManager];
        // Four-char OSType constants:
        // kCoreEventClass = 'aevt', kAEReopenApplication = 'rapp'
        const K_CORE_EVENT_CLASS: u32 = 0x6165_7674;
        const K_AE_REOPEN_APPLICATION: u32 = 0x7261_7070;
        let _: () = msg_send![
            apple_event_manager,
            setEventHandler: observer
            andSelector: sel!(handleReopenEvent:withReplyEvent:)
            forEventClass: K_CORE_EVENT_CLASS
            andEventID: K_AE_REOPEN_APPLICATION
        ];

        Ok(())
    }

    /// Remove tray icon
    #[allow(dead_code)]
    pub fn remove_tray_macos() {
        unsafe {
            if !REOPEN_OBSERVER.is_null() {
                if let Some(center_class) = Class::get("NSNotificationCenter") {
                    let center: *mut Object = msg_send![center_class, defaultCenter];
                    if !center.is_null() {
                        let _: () = msg_send![center, removeObserver: REOPEN_OBSERVER];
                    }
                }
                if let Some(manager_class) = Class::get("NSAppleEventManager") {
                    let manager: *mut Object = msg_send![manager_class, sharedAppleEventManager];
                    if !manager.is_null() {
                        const K_CORE_EVENT_CLASS: u32 = 0x6165_7674;
                        const K_AE_REOPEN_APPLICATION: u32 = 0x7261_7070;
                        let _: () = msg_send![
                            manager,
                            removeEventHandlerForEventClass: K_CORE_EVENT_CLASS
                            andEventID: K_AE_REOPEN_APPLICATION
                        ];
                    }
                }
                let _: () = msg_send![REOPEN_OBSERVER, release];
                REOPEN_OBSERVER = std::ptr::null_mut();
            }
            if !STATUS_ITEM.is_null() {
                if let Some(status_bar_class) = Class::get("NSStatusBar") {
                    let status_bar: *mut Object = msg_send![status_bar_class, systemStatusBar];
                    let _: () = msg_send![status_bar, removeStatusItem: STATUS_ITEM];
                }
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
    use slint::ComponentHandle;
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

    /// Find logo.png file in various possible locations
    fn find_logo_path() -> Option<std::path::PathBuf> {
        // Try multiple locations:
        // 1. Current working directory (for development)
        let cwd_path = std::path::Path::new("logo.png");
        if cwd_path.exists() {
            return Some(cwd_path.to_path_buf());
        }

        // 2. Executable directory
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let exe_dir_logo = exe_dir.join("logo.png");
                if exe_dir_logo.exists() {
                    return Some(exe_dir_logo);
                }
            }
        }

        // 3. Try to find from executable path going up to find project root
        if let Ok(exe_path) = std::env::current_exe() {
            let mut current = exe_path.parent();
            while let Some(dir) = current {
                let logo_path = dir.join("logo.png");
                if logo_path.exists() {
                    return Some(logo_path);
                }
                // Stop if we've gone too far up (e.g., reached /)
                if dir.parent() == Some(dir) {
                    break;
                }
                current = dir.parent();
            }
        }

        None
    }

    fn load_tray_icon() -> Result<tray_icon::Icon> {
        // Try to find logo.png in various locations
        if let Some(logo_path) = find_logo_path() {
            if let Ok(img) = image::open(&logo_path) {
                log::info!("Successfully loaded tray icon from: {:?}", logo_path);
                let resized = img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3);
                let rgba = resized.to_rgba8();
                let (width, height) = rgba.dimensions();
                return Ok(tray_icon::Icon::from_rgba(rgba.into_raw(), width, height)?);
            } else {
                log::error!("Failed to open logo.png from: {:?}", logo_path);
            }
        } else {
            log::warn!("Could not find logo.png in any expected location");
        }

        // Fallback: create a default blue icon
        log::info!("Using fallback icon for tray");
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
                            toggle_window_visibility(&ui);
                        }
                    })
                    .ok();
                } else if event.id == quit_id {
                    crate::shutdown::request_shutdown("tray-quit");
                }
            }

            if let Ok(_event) = TrayIconEvent::receiver().try_recv() {
                let ui_weak_clone = ui_weak.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak_clone.upgrade() {
                        toggle_window_visibility(&ui);
                    }
                })
                .ok();
            }

            thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    fn toggle_window_visibility(ui: &MainWindow) {
        let window = ui.window();
        let result = if window.is_visible() {
            window.hide()
        } else {
            window.show()
        };
        if let Err(error) = result {
            log::warn!("Failed to toggle the window from the tray: {error}");
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
                if let Err(error) = icon.set_tooltip(Some(tooltip.to_string())) {
                    log::warn!("Failed to update the tray tooltip: {error}");
                }
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
