use anyhow::Result;
use slint::Weak;

use crate::MainWindow;

mod animation;
#[cfg(all(target_os = "macos", feature = "test-fixtures"))]
mod locale_fixture;

// ==================== macOS Native Implementation ====================
#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc::runtime::{BOOL, Class, NO, Object, Sel, YES};
    use objc::{msg_send, sel, sel_impl};
    use std::cell::{Cell, RefCell};
    use std::ffi::CString;
    use std::rc::Rc;
    use std::sync::OnceLock;

    const STATUS_ICON_SIZE: f64 = 22.0;
    const LEFT_MOUSE_UP: u64 = 2;
    const RIGHT_MOUSE_UP: u64 = 4;
    const CONTROL_MODIFIER: u64 = 1 << 18;
    const CLICK_ACTION_MASK: u64 = (1 << LEFT_MOUSE_UP) | (1 << RIGHT_MOUSE_UP);

    thread_local! {
        // AppKit images and the Slint timer remain on the event-loop thread.
        static JUMP: RefCell<Option<Rc<animation::Player<NativeImage>>>> = const { RefCell::new(None) };
        static MENU_TRACKING: Cell<bool> = const { Cell::new(false) };
    }

    /// Own the +1 reference returned by NSImage's init methods.
    struct NativeImage(*mut Object);

    impl Drop for NativeImage {
        fn drop(&mut self) {
            unsafe {
                let _: () = msg_send![self.0, release];
            }
        }
    }

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
                    let still = NativeImage(image);
                    apply_image(&still);
                    let _: () = msg_send![button, setTitle: create_nsstring("")];
                    let frames: Option<Vec<_>> = animation::FRAME_PNGS
                        .iter()
                        .map(|bytes| load_jump_image(bytes))
                        .collect();
                    if let Some(frames) = frames {
                        JUMP.with(|jump| {
                            *jump.borrow_mut() =
                                Some(animation::Player::new(still, frames, apply_image));
                        });
                    } else {
                        log::warn!("[Tray] Failed to load jump frames; keeping the static icon");
                    }
                } else {
                    log::warn!("Failed to load tray icon, using text fallback 'U'");
                }

                // Set click action on the button
                let _: () = msg_send![button, setTarget: delegate];
                let _: () = msg_send![button, setAction: sel!(trayIconClicked:)];
                // Open after the completed click, so its release cannot dismiss
                // the menu's newly entered tracking loop.
                let _: i64 = msg_send![button, sendActionOn: CLICK_ACTION_MASK];
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

        unsafe { configure_status_bar_image(image) };

        log::info!("Successfully loaded tray icon from: {}", path_str);
        Some(image)
    }

    /// Frames are bundled with the executable; clicks never perform image I/O.
    unsafe fn load_jump_image(bytes: &[u8]) -> Option<NativeImage> {
        unsafe {
            let data: *mut Object = msg_send![Class::get("NSData")?,
                dataWithBytes: bytes.as_ptr() length: bytes.len()];
            let image: *mut Object = msg_send![Class::get("NSImage")?, alloc];
            let image: *mut Object = msg_send![image, initWithData: data];
            if image.is_null() {
                return None;
            }
            configure_status_bar_image(image);
            Some(NativeImage(image))
        }
    }

    /// Keep the orca's original colours and one size during playback and at rest.
    unsafe fn configure_status_bar_image(image: *mut Object) {
        unsafe {
            let size = NSSize {
                width: STATUS_ICON_SIZE,
                height: STATUS_ICON_SIZE,
            };
            let _: () = msg_send![image, setSize: size];
            let _: () = msg_send![image, setTemplate: NO];
        }
    }

    fn apply_image(image: &NativeImage) {
        unsafe {
            if !STATUS_ITEM.is_null() {
                let button: *mut Object = msg_send![STATUS_ITEM, button];
                if !button.is_null() {
                    let _: () = msg_send![button, setImage: image.0];
                }
            }
        }
    }

    fn play_jump() {
        JUMP.with(|jump| {
            if let Some(player) = jump.borrow().as_ref() {
                player.play();
            }
        });
    }

    /// Create simple quit menu (only Quit option)
    unsafe fn create_quit_menu(delegate: *mut Object) -> Result<*mut Object> {
        unsafe {
            let ns_menu_class = Class::get("NSMenu")
                .ok_or_else(|| anyhow::anyhow!("Failed to get NSMenu class"))?;

            let menu: *mut Object = msg_send![ns_menu_class, alloc];
            let menu: *mut Object = msg_send![menu, init];

            // Only Quit menu item
            let quit_item = create_menu_item(
                &usher_i18n::translate("tray.quit"),
                sel!(quitApp:),
                delegate,
            )?;
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
        let c_str = CString::new(s).unwrap_or_default();
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

    pub(super) fn sync_language() {
        unsafe {
            if !TRAY_MENU.is_null() {
                let quit_item: *mut Object = msg_send![TRAY_MENU, itemAtIndex: 0i64];
                let title = create_nsstring(&usher_i18n::translate("tray.quit"));
                let _: () = msg_send![quit_item, setTitle: title];
            }
        }
    }

    #[cfg(feature = "test-fixtures")]
    pub(super) fn menu_locale_snapshot() -> Result<(usize, String)> {
        use std::ffi::{CStr, c_char};

        unsafe {
            anyhow::ensure!(!TRAY_MENU.is_null(), "native tray menu is unavailable");
            let count: i64 = msg_send![TRAY_MENU, numberOfItems];
            anyhow::ensure!(count > 0, "native tray menu has no items");
            let item: *mut Object = msg_send![TRAY_MENU, itemAtIndex: 0i64];
            anyhow::ensure!(!item.is_null(), "native Quit item is unavailable");
            let title: *mut Object = msg_send![item, title];
            anyhow::ensure!(!title.is_null(), "native Quit title is unavailable");
            let utf8: *const c_char = msg_send![title, UTF8String];
            anyhow::ensure!(!utf8.is_null(), "native Quit title has no UTF-8 value");
            Ok((
                TRAY_MENU as usize,
                CStr::from_ptr(utf8).to_str()?.to_owned(),
            ))
        }
    }

    /// Own native tracking across performClick's nested AppKit event loop.
    /// Quit/teardown may remove the global status item before tracking returns.
    struct MenuTracking {
        status_item: *mut Object,
        button: *mut Object,
    }

    impl Drop for MenuTracking {
        fn drop(&mut self) {
            unsafe {
                // Restore primary-click routing even after Escape/outside click.
                let _: () = msg_send![self.status_item, setMenu: std::ptr::null::<Object>()];
                let _: () = msg_send![self.button, release];
                let _: () = msg_send![self.status_item, release];
            }
            MENU_TRACKING.set(false);
        }
    }

    /// Show the tray menu with the supported NSStatusItem.menu path.
    unsafe fn show_tray_menu() {
        unsafe {
            if TRAY_MENU.is_null() || STATUS_ITEM.is_null() || MENU_TRACKING.get() {
                return;
            }
            let button: *mut Object = msg_send![STATUS_ITEM, button];
            if button.is_null() {
                return;
            }
            let _: () = msg_send![STATUS_ITEM, retain];
            let _: () = msg_send![button, retain];
            let tracking = MenuTracking {
                status_item: STATUS_ITEM,
                button,
            };
            MENU_TRACKING.set(true);
            // A non-nil menu overrides the button action. performClick enters
            // native menu tracking without recursively calling trayIconClicked.
            let _: () = msg_send![tracking.status_item, setMenu: TRAY_MENU];
            let _: () = msg_send![tracking.button, performClick: std::ptr::null::<Object>()];
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ClickAction {
        ShowWindow,
        ShowMenu,
    }

    fn click_action(event_type: u64, modifier_flags: u64) -> Option<ClickAction> {
        match event_type {
            RIGHT_MOUSE_UP => Some(ClickAction::ShowMenu),
            LEFT_MOUSE_UP if modifier_flags & CONTROL_MODIFIER != 0 => Some(ClickAction::ShowMenu),
            LEFT_MOUSE_UP => Some(ClickAction::ShowWindow),
            _ => None,
        }
    }

    unsafe fn current_click_action() -> Option<ClickAction> {
        unsafe {
            let ns_app_class = Class::get("NSApplication").unwrap();
            let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];
            let current_event: *mut Object = msg_send![ns_app, currentEvent];

            if current_event.is_null() {
                return None;
            }
            let event_type: u64 = msg_send![current_event, type];
            let modifier_flags: u64 = msg_send![current_event, modifierFlags];
            click_action(event_type, modifier_flags)
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
                    match current_click_action() {
                        Some(ClickAction::ShowMenu) => show_tray_menu(),
                        Some(ClickAction::ShowWindow) => {
                            play_jump();
                            show_main_window();
                        }
                        None => {}
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
                    // Menu activation is not a Dock/Cmd-Tab reopen. Bringing
                    // the hidden main window forward here interrupts tracking.
                    if MENU_TRACKING.get() || MAIN_WINDOW.is_null() {
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
        JUMP.with(|jump| {
            if let Some(player) = jump.borrow_mut().take() {
                player.stop();
            }
        });
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn native_menu_copy_resolves_in_each_packaged_language() {
            assert_eq!(usher_i18n::translate_for("en", "tray.quit"), "Quit");
            assert_eq!(usher_i18n::translate_for("zh-CN", "tray.quit"), "退出");
        }

        #[test]
        fn completed_click_routes_once_to_the_menu_or_window() {
            for (events, expected) in [
                ([(1, 0), (2, 0)], ClickAction::ShowWindow),
                ([(3, 0), (4, 0)], ClickAction::ShowMenu),
                (
                    [(1, CONTROL_MODIFIER), (2, CONTROL_MODIFIER)],
                    ClickAction::ShowMenu,
                ),
            ] {
                let actions = events
                    .into_iter()
                    .filter(|(event, _)| CLICK_ACTION_MASK & (1 << event) != 0)
                    .filter_map(|(event, flags)| click_action(event, flags))
                    .collect::<Vec<_>>();
                assert_eq!(actions, vec![expected]);
            }
        }

        #[test]
        fn unrelated_events_never_reveal_the_window_or_open_the_menu() {
            for event in [0, 1, 3, 5, 10, 11, 12, 25, 26, 27] {
                assert_eq!(click_action(event, 0), None);
                assert_eq!(click_action(event, CONTROL_MODIFIER), None);
            }
        }
    }
}

// ==================== Other Platforms Implementation ====================
#[cfg(not(target_os = "macos"))]
mod other {
    use super::*;
    use anyhow::Context;
    use slint::ComponentHandle;
    use std::rc::Rc;
    use std::thread;
    use tray_icon::{
        MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
        menu::{Menu, MenuEvent, MenuId, MenuItem},
    };

    // Global tray icon reference
    thread_local! {
        static TRAY_ICON: std::cell::RefCell<Option<TrayIcon>> = std::cell::RefCell::new(None);
        static JUMP: std::cell::RefCell<Option<Rc<animation::Player<tray_icon::Icon>>>> = const { std::cell::RefCell::new(None) };
    }

    /// Setup system tray icon on other platforms
    pub fn setup_tray_other(ui_weak: Weak<MainWindow>) -> Result<()> {
        let (icon, has_logo) = load_tray_icon()?;
        let (menu, show_hide_id, quit_id) = create_tray_menu()?;

        let tray_icon = TrayIconBuilder::new()
            .with_icon(icon.clone())
            .with_tooltip("Usher")
            .with_menu(Box::new(menu))
            .build()
            .context("Failed to create tray icon")?;

        TRAY_ICON.with(|tray| {
            *tray.borrow_mut() = Some(tray_icon);
        });
        if has_logo {
            match load_jump_frames() {
                Ok(frames) => JUMP.with(|jump| {
                    *jump.borrow_mut() = Some(animation::Player::new(icon, frames, apply_image));
                }),
                Err(error) => log::warn!("[Tray] Failed to load jump frames: {error}"),
            }
        }

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

    fn load_tray_icon() -> Result<(tray_icon::Icon, bool)> {
        // Try to find logo.png in various locations
        if let Some(logo_path) = find_logo_path() {
            if let Ok(img) = image::open(&logo_path) {
                log::info!("Successfully loaded tray icon from: {:?}", logo_path);
                let resized = img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3);
                let rgba = resized.to_rgba8();
                let (width, height) = rgba.dimensions();
                return Ok((
                    tray_icon::Icon::from_rgba(rgba.into_raw(), width, height)?,
                    true,
                ));
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
        Ok((tray_icon::Icon::from_rgba(rgba_data, 32, 32)?, false))
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

    fn load_jump_frames() -> Result<Vec<tray_icon::Icon>> {
        animation::FRAME_PNGS
            .iter()
            .map(|bytes| {
                let image = image::load_from_memory(bytes)?
                    .resize_exact(32, 32, image::imageops::FilterType::Lanczos3)
                    .to_rgba8();
                Ok(tray_icon::Icon::from_rgba(image.into_raw(), 32, 32)?)
            })
            .collect()
    }

    fn apply_image(image: &tray_icon::Icon) {
        TRAY_ICON.with(|tray| {
            if let Some(icon) = tray.borrow().as_ref()
                && let Err(error) = icon.set_icon(Some(image.clone()))
            {
                log::warn!("[Tray] Failed to set jump frame: {error}");
            }
        });
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

            if let Ok(event) = TrayIconEvent::receiver().try_recv() {
                let animate = matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Down,
                        ..
                    } | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    }
                );
                let ui_weak_clone = ui_weak.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak_clone.upgrade() {
                        if animate {
                            JUMP.with(|jump| {
                                if let Some(player) = jump.borrow().as_ref() {
                                    player.play();
                                }
                            });
                        }
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
        JUMP.with(|jump| {
            if let Some(player) = jump.borrow_mut().take() {
                player.stop();
            }
        });
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
        #[cfg(feature = "test-fixtures")]
        let fixture_window = ui_weak.clone();
        macos::setup_tray_macos(ui_weak)?;
        #[cfg(feature = "test-fixtures")]
        locale_fixture::start(fixture_window);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        other::setup_tray_other(ui_weak)
    }
}

/// Refresh an already-created native menu on the Slint event loop.
pub(crate) fn sync_language() {
    #[cfg(target_os = "macos")]
    macos::sync_language();
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
