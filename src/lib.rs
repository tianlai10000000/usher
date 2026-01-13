#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

use slint::{ComponentHandle, Timer, Weak};

// 必须在模块声明之前调用，以便生成的类型在所有模块中可用
slint::include_modules!();

mod notification;

use notification::setup_notifications;

mod toast;
use toast::setup_toast;

mod dialog;
use dialog::setup_dialog;

mod update;

use update::updater::{check_for_update, perform_update_with_progress, reset_update_state, UpdateState, CancelFlag};
use update::check_update::init;
use self_update::cargo_crate_version;
use std::sync::atomic::{AtomicBool, Ordering};

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

fn ui() -> Result<MainWindow> {

    let app = MainWindow::new().context("Failed to create MainWindow")?;

    // Update logic
    let state = Arc::new(Mutex::new(UpdateState::default()));
    let rt = Arc::new(Runtime::new().context("Failed to create Tokio runtime")?);
    let app_weak = app.as_weak();
    // 初始化更新检查（定时 + 通知逻辑）
    init(app_weak.clone(), state.clone(), rt.clone());

    // 手动检查更新
    let state_clone_check = state.clone();
    let app_weak_clone_check = app_weak.clone();
    let rt_clone_check = rt.clone();
    app.on_check_update(move || {
        println!("[DEBUG] check-update 回调被调用");
        let state = state_clone_check.clone();
        let ui_weak = app_weak_clone_check.clone();
        let rt = rt_clone_check.clone();
        rt.spawn(async move {
            println!("[DEBUG] 开始检查更新...");
            // 重置状态
            {
                let mut s = state.lock().unwrap();
                s.available = false;
                s.version.clear();
                s.error.clear();
                s.progress = 0.0;
            }

            println!("[DEBUG] 调用 check_for_update...");
            let result = check_for_update(&state).await;
            println!("[DEBUG] check_for_update 完成，结果: {:?}", result.is_ok());
            let (is_up_to_date, version) = {
                match &result {
                    Ok(up_to_date) => {
                        let v = if !*up_to_date {
                            let s = state.lock().unwrap();
                            s.version.clone()
                        } else {
                            String::new()
                        };
                        println!("[DEBUG] is_up_to_date: {}, version: {}", *up_to_date, v);
                        (*up_to_date, v)
                    }
                    Err(_) => (false, String::new()),
                }
            };
            
            match result {
                Ok(_) => {
                    let version_clone = version.clone();
                    println!("[DEBUG] 处理结果: is_up_to_date={}, version={}", is_up_to_date, version_clone);
                    
                    if is_up_to_date {
                        // 当前版本是最新版本
                        let ui_weak_for_notification = ui_weak.clone();
                        let ui_weak_for_logic = ui_weak.clone();
                        let ui_weak_for_reset = ui_weak.clone();
                        
                        // 先调用通知
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_notification.upgrade() {
                                {
                                    let manager = NotificationManager::get(&ui);
                                    manager.invoke_success(NotificationOptions {
                                        message: "当前程序已是最新版本".into(),
                                        title: "".into(),
                                        duration: 3000,
                                        icon: "".into(),
                                        component: CustomComponentType::None,
                                    });
                                } // manager 借用在这里释放
                            }
                        }).ok();
                        
                        // 使用 Timer 延迟执行逻辑重置，确保在下一个事件循环迭代中执行
                        Timer::single_shot(std::time::Duration::from_millis(100), move || {
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak_for_logic.upgrade() {
                                    {
                                        let logic = ui.global::<UpdateLogic>();
                                        logic.set_error("".into());
                                        logic.set_update_available(false);
                                    } // logic 借用在这里释放
                                }
                            }).ok();
                        });
                        
                        // 立即重置按钮状态，确保用户可以看到按钮恢复
                        println!("[DEBUG] 准备重置按钮状态（已是最新版本）");
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_reset.upgrade() {
                                println!("[DEBUG] 重置 checking-update 状态（已是最新版本）");
                                let logic = ui.global::<UpdateLogic>();
                                logic.set_checking_update(false);
                            } else {
                                println!("[DEBUG] ui_weak_for_reset.upgrade() 失败（已是最新版本）");
                            }
                        }).ok();
                    } else {
                        println!("[DEBUG] 进入有新版本可用分支");
                        // 有新版本可用
                        let ui_weak_for_all = ui_weak.clone();
                        let version_for_all = version.clone();
                        
                        // 设置状态
                        let ui_weak_for_state = ui_weak_for_all.clone();
                        let version_for_state = version_for_all.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_state.upgrade() {
                                println!("[DEBUG] 设置状态");
                                let logic = ui.global::<UpdateLogic>();
                                logic.set_update_available(true);
                                logic.set_new_version(version_for_state.clone().into());
                                logic.set_checking_update(false);
                                println!("[DEBUG] 已设置 update-available=true, new-version={}", version_for_state);
                            }
                        }).ok();
                        
                        // 延迟显示对话框，避免 RefCell 借用冲突
                        // 使用 tokio sleep 代替 Slint Timer（在异步任务中 Slint Timer 无法正确触发）
                        println!("[DEBUG] 准备延迟显示对话框...");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        println!("[DEBUG] 延迟完成，显示对话框");
                        
                        let ui_weak_for_dialog = ui_weak_for_all.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_dialog.upgrade() {
                                println!("[DEBUG] 调用 MainWindow.invoke_show_update_dialog()");
                                let current_version = cargo_crate_version!();
                                ui.invoke_show_update_dialog(
                                    version_for_all.clone().into(),
                                    current_version.into()
                                );
                                println!("[DEBUG] MainWindow.invoke_show_update_dialog() 已调用");
                            } else {
                                println!("[DEBUG] ui_weak_for_dialog.upgrade() 失败");
                            }
                        }).ok();
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    println!("[DEBUG] 检查更新出错: {}", error_msg);
                    // 提供更友好的错误信息
                    let friendly_msg = if error_msg.contains("404") || error_msg.contains("Not Found") {
                        format!("仓库未找到或没有可用的发布版本。请检查仓库是否存在并已创建 releases, 失败原因：{}", error_msg)
                    } else if error_msg.contains("NetworkError") || error_msg.contains("network") 
                        || error_msg.contains("ReqwestError") || error_msg.contains("error sending request") {
                        format!("网络连接失败，请检查网络连接后重试, 失败原因：{}", error_msg)
                    } else {
                        format!("未知错误, 错误原因：{}", error_msg)
                    };
                    let ui_weak_for_error = ui_weak.clone();
                    let ui_weak_for_logic = ui_weak.clone();
                    let ui_weak_for_reset = ui_weak.clone();
                    
                    // 显示错误通知
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_for_error.upgrade() {
                            {
                                let manager = NotificationManager::get(&ui);
                                manager.invoke_error(NotificationOptions {
                                    message: friendly_msg.clone().into(),
                                    title: "检查更新失败".into(),
                                    duration: 5000,
                                    icon: "".into(),
                                    component: CustomComponentType::None,
                                });
                            } // manager 借用在这里释放，避免与回调中的借用冲突
                        }
                    }).ok();
                    
                    // 使用 Timer 延迟执行逻辑重置，确保在下一个事件循环迭代中执行
                    Timer::single_shot(std::time::Duration::from_millis(100), move || {
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_logic.upgrade() {
                                {
                                    let logic = ui.global::<UpdateLogic>();
                                    logic.set_error("".into());
                                    logic.set_update_available(false);
                                } // logic 借用在这里释放
                            }
                        }).ok();
                    });
                    
                    // 立即重置按钮状态，确保用户可以看到按钮恢复
                    println!("[DEBUG] 准备重置按钮状态（检查失败）");
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_for_reset.upgrade() {
                            println!("[DEBUG] 重置 checking-update 状态（检查失败）");
                            let logic = ui.global::<UpdateLogic>();
                            logic.set_checking_update(false);
                        } else {
                            println!("[DEBUG] ui_weak_for_reset.upgrade() 失败（检查失败）");
                        }
                    }).ok();
                }
            }
        });
    });

    // 全局取消标志
    let cancel_flag: CancelFlag = Arc::new(AtomicBool::new(false));
    
    // 用户点击"立即更新"按钮
    let state_clone = state.clone();
    let app_weak_clone = app_weak.clone();
    let rt_clone = rt.clone();
    let cancel_flag_clone = cancel_flag.clone();
    app.on_install(move || {
        println!("[DEBUG] on_install 回调被调用");
        let state = state_clone.clone();
        let ui_weak = app_weak_clone.clone();
        let rt = rt_clone.clone();
        let cancel_flag = cancel_flag_clone.clone();
        
        // 重置取消标志
        cancel_flag.store(false, Ordering::Relaxed);
        
        // 使用 Timer 延迟执行所有操作，避免 RefCell 借用冲突
        Timer::single_shot(std::time::Duration::from_millis(100), move || {
            println!("[DEBUG] Timer 回调执行，准备显示下载进度模态框");
            
            // 显示下载进度模态框
            if let Some(ui) = ui_weak.upgrade() {
                let logic = ui.global::<UpdateLogic>();
                logic.set_is_downloading(true);
                logic.set_download_progress(0.0);
                logic.set_download_status("Preparing...".into());
                println!("[DEBUG] 已显示下载进度模态框");
            }
            
            // 创建进度回调
            let ui_weak_for_progress = ui_weak.clone();
            let progress_callback = Arc::new(move |progress: f32, downloaded: u64, total: u64, status: &str| {
                let ui_weak = ui_weak_for_progress.clone();
                let status_str = status.to_string();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let logic = ui.global::<UpdateLogic>();
                        logic.set_download_progress(progress);
                        logic.set_download_status(status_str.into());
                        logic.set_downloaded_bytes(downloaded as i32);
                        logic.set_total_bytes(total as i32);
                    }
                }).ok();
            });
            
            let ui_weak_for_error = ui_weak.clone();
            rt.spawn(async move {
                println!("[DEBUG] 开始执行带进度的更新...");
                let update_result = perform_update_with_progress(&state, progress_callback, cancel_flag.clone()).await;
                
                match &update_result {
                    Ok(_) => {
                        println!("[DEBUG] 更新成功，程序即将重启");
                        // 更新成功后程序会自动重启，这里不需要额外处理
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        println!("[DEBUG] 更新失败: {}", error_msg);
                        
                        // 重置状态
                        reset_update_state(&state);
                        
                        let is_cancelled = error_msg.contains("cancelled");
                        let ui_weak_for_error = ui_weak_for_error.clone();
                        
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_error.upgrade() {
                                // 隐藏下载进度模态框
                                let logic = ui.global::<UpdateLogic>();
                                logic.set_is_downloading(false);
                                logic.set_download_progress(0.0);
                                logic.set_download_status("".into());
                                
                                if !is_cancelled {
                                    // 只有非取消的情况才显示错误通知
                                    let manager = NotificationManager::get(&ui);
                                    manager.invoke_error(NotificationOptions {
                                        message: format!("更新失败: {}", error_msg).into(),
                                        title: "".into(),
                                        duration: 5000,
                                        icon: "".into(),
                                        component: CustomComponentType::None,
                                    });
                                }
                            }
                        }).ok();
                    }
                }
            });
        });
    });
    
    // 用户点击取消下载按钮
    let cancel_flag_clone = cancel_flag.clone();
    let state_clone = state.clone();
    let app_weak_clone = app_weak.clone();
    app.on_cancel_download(move || {
        println!("[DEBUG] on_cancel_download 回调被调用");
        
        // 设置取消标志
        cancel_flag_clone.store(true, Ordering::Relaxed);
        
        // 重置状态
        reset_update_state(&state_clone);
        
        // 使用 Timer 延迟执行 UI 更新，避免 RefCell 借用冲突
        let ui_weak = app_weak_clone.clone();
        Timer::single_shot(std::time::Duration::from_millis(10), move || {
            if let Some(ui) = ui_weak.upgrade() {
                let logic = ui.global::<UpdateLogic>();
                logic.set_is_downloading(false);
                logic.set_download_progress(0.0);
                logic.set_download_status("".into());
                logic.set_update_available(false);
                
                // 显示取消通知
                let manager = NotificationManager::get(&ui);
                manager.invoke_info(NotificationOptions {
                    message: "更新已取消".into(),
                    title: "".into(),
                    duration: 3000,
                    icon: "".into(),
                    component: CustomComponentType::None,
                });
            }
        });
    });

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

    // ========= App Info ========= //
    // 设置应用版本号
    AppState::get(&app).set_version(env!("CARGO_PKG_VERSION").into());


    // ========= Notification ========= //
    // 单窗口：不传共享队列（使用 None）
    setup_notifications(&app, None);

    // 如果是多窗口应用，想真正共享队列：
    // let shared_queue = Arc::new(RwLock::new(SharedQueue::default()));
    // setup_notifications(&window1, Some(shared_queue.clone()));
    // setup_notifications(&window2, Some(shared_queue));

    // ========= Toast ========= //
    setup_toast(&app);

    // ========= Dialog ========= //
    setup_dialog(&app);

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

    Ok(app)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn main() {
    let ui: MainWindow = ui().unwrap();
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
