#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

use slint::{ComponentHandle, Weak};

// 必须在模块声明之前调用，以便生成的类型在所有模块中可用
slint::include_modules!();

mod notification;

use notification::setup_notifications;

mod toast;
use toast::setup_toast;

#[cfg(target_os = "macos")]
fn set_title_bar_color(_window: &MainWindow, is_dark: bool) {
    use objc::runtime::{Class, Object};
    use objc::msg_send;
    use std::ptr;
    use std::ffi::CString;
    
    // 使用延迟执行确保窗口已创建，并在主线程上执行
    slint::invoke_from_event_loop(move || {
        unsafe {
            // 获取应用程序的主窗口
            let ns_app_class = match Class::get("NSApplication") {
                Some(cls) => cls,
                None => return,
            };
            
            let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];
            if ns_app.is_null() {
                return;
            }
            
            let ns_window: *mut Object = msg_send![ns_app, mainWindow];
            if ns_window.is_null() {
                return;
            }
            
            if is_dark {
                // 设置标题栏为深色模式
                // 使标题栏透明，这样背景色会显示出来
                let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: objc::runtime::YES];
                
                // 设置窗口背景为 Material Design 暗黑主题背景色 rgb(17, 19, 24)
                if let Some(ns_color_class) = Class::get("NSColor") {
                    // 使用 colorWithRed:green:blue:alpha: 创建颜色
                    // RGB(17, 19, 24) 转换为 0-1 范围: (17/255.0, 19/255.0, 24/255.0)
                    let r = 17.0 / 255.0;
                    let g = 19.0 / 255.0;
                    let b = 24.0 / 255.0;
                    let dark_bg_color: *mut Object = msg_send![ns_color_class, colorWithRed: r green: g blue: b alpha: 1.0];
                    let _: () = msg_send![ns_window, setBackgroundColor: dark_bg_color];
                }
                
                // 设置窗口外观为深色模式，这样标题文本会变成白色
                if let Some(ns_appearance_class) = Class::get("NSAppearance") {
                    if let Some(ns_string_class) = Class::get("NSString") {
                        // 创建 NSString "NSAppearanceNameDarkAqua"
                        if let Ok(c_str) = CString::new("NSAppearanceNameDarkAqua") {
                            let appearance_name: *mut Object = msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
                            if !appearance_name.is_null() {
                                let dark_appearance: *mut Object = msg_send![ns_appearance_class, appearanceNamed: appearance_name];
                                if !dark_appearance.is_null() {
                                    let _: () = msg_send![ns_window, setAppearance: dark_appearance];
                                }
                            }
                        }
                    }
                }
            } else {
                // 恢复浅色模式
                let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: objc::runtime::NO];
                
                // 恢复默认背景色（nil 表示使用系统默认）
                let _: () = msg_send![ns_window, setBackgroundColor: ptr::null_mut::<Object>()];
                
                // 设置窗口外观为浅色模式
                if let Some(ns_appearance_class) = Class::get("NSAppearance") {
                    if let Some(ns_string_class) = Class::get("NSString") {
                        // 创建 NSString "NSAppearanceNameAqua"
                        if let Ok(c_str) = CString::new("NSAppearanceNameAqua") {
                            let appearance_name: *mut Object = msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
                            if !appearance_name.is_null() {
                                let light_appearance: *mut Object = msg_send![ns_appearance_class, appearanceNamed: appearance_name];
                                if !light_appearance.is_null() {
                                    let _: () = msg_send![ns_window, setAppearance: light_appearance];
                                }
                            }
                        }
                    }
                }
            }
        }
    }).ok();
}

#[cfg(not(target_os = "macos"))]
fn set_title_bar_color(_window: &MainWindow, _is_dark: bool) {
    // 其他平台暂不支持
}

#[derive(Clone)]
struct Router {
    window: Weak<MainWindow>,
}

impl Router {
    fn new(window: Weak<MainWindow>) -> Self {
        Self { window }
    }

    fn navigate(&self, route: Route) {
        if let Some(win) = self.window.upgrade() {
            win.set_current_route(route);
            // 可在此处添加其他全局副作用，如：
            // - 记录路由历史
            // - 发送分析事件
            // - 延迟加载某些资源等
            println!("→ 导航到: {:?}", route);
        }
    }
}

fn ui() -> MainWindow {
    let app = MainWindow::new().unwrap();

    // ========= Router ========= //
    let router = Router::new(app.as_weak());

    // 连接 slint 里的 navigate 回调
    let router_clone = router.clone();
    app.on_navigate(move |route| {
        router_clone.navigate(route);
    });

    // 可选：程序启动时默认进入某个页面（已经由 .slint 默认设置）

    // 可选：示例 - 3秒后自动跳转（演示用）
    // let router_clone = router.clone();
    // slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
    //     router_clone.navigate(Route::Profile);
    // });

    // ========= Theme ========= //
    let app_weak = app.as_weak();
    AppState::get(&app).on_toggle_theme(move |_mode| {
        if let Some(win) = app_weak.upgrade() {
            let app_state = AppState::get(&win);
            let current = app_state.get_theme_mode();
            let new_mode = if current == 0 { 1 } else { 0 };
            app_state.set_theme_mode(new_mode);
            // 主题切换逻辑在 app_state.slint 的 changed theme-mode 回调中处理
            
            // 设置标题栏颜色
            set_title_bar_color(&win, new_mode == 1);
        }
    });

    // ========= Title Bar ========= //
    // 初始化时根据当前主题设置标题栏颜色
    let initial_theme = AppState::get(&app).get_theme_mode();
    set_title_bar_color(&app, initial_theme == 1);


    // ========= Notification ========= //
    // 单窗口：不传共享队列（使用 None）
    setup_notifications(&app, None);

    // 如果是多窗口应用，想真正共享队列：
    // let shared_queue = Arc::new(RwLock::new(SharedQueue::default()));
    // setup_notifications(&window1, Some(shared_queue.clone()));
    // setup_notifications(&window2, Some(shared_queue));

    // ========= Toast ========= //
    setup_toast(&app);

    // 示例调用 - 延迟执行以确保窗口已完全初始化
    // let app_weak = app.as_weak();
    // slint::invoke_from_event_loop(move || {
    //     if let Some(ui) = app_weak.upgrade() {
    //         toast::success("操作成功！");
    //         toast::show(toast::ToastOptions {
    //             message: "这是一个很长的提示消息...".to_string(),
    //             duration: 4000,
    //             position: "bottom".to_string(),
    //             overlay: true,
    //             ..Default::default()
    //         });
    //     }
    // }).ok();

    return app;
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn main() {
    let ui: MainWindow = ui();
    ui.run().unwrap();
}

// #[cfg(target_os = "android")]
// #[unsafe(no_mangle)]
// fn android_main(android_app: slint::android::AndroidApp) {
//     slint::android::init(android_app).unwrap();
//     let ui = ui();
//     MaterialWindowAdapter::get(&ui).set_disable_hover(true);
//     ui.run().unwrap();
// }
