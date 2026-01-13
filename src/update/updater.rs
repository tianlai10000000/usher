use anyhow::{Context, Result};
use self_update::cargo_crate_version;
use semver::Version;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct UpdateState {
    pub available: bool,
    pub version: String,
    pub progress: f32,
    pub error: String,
    // 下载状态
    pub is_downloading: bool,
    pub download_progress: f32,
    pub download_status: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// 进度回调类型
pub type ProgressCallback = Arc<dyn Fn(f32, u64, u64, &str) + Send + Sync>;

/// 取消标志
pub type CancelFlag = Arc<AtomicBool>;

fn create_updater() -> Result<Box<dyn self_update::update::ReleaseUpdate>> {
    println!(
        "[DEBUG] 创建 updater，repo: tianlai10000000/usher, 当前版本: {}",
        cargo_crate_version!()
    );
    self_update::backends::github::Update::configure()
        .repo_owner("tianlai10000000")
        .repo_name("usher")
        .bin_name("usher") // Windows 可改为 "myapp.exe"
        .current_version(cargo_crate_version!())
        .build()
        .context("Failed to configure updater")
}

pub async fn check_for_update(state: &Arc<Mutex<UpdateState>>) -> Result<bool> {
    println!("[DEBUG] check_for_update 开始执行");

    let state_clone = state.clone();
    // 在阻塞线程池中执行同步操作
    let result = tokio::task::spawn_blocking(move || -> Result<(bool, String), anyhow::Error> {
        let updater = create_updater().map_err(|e| {
            println!("[DEBUG] 创建 updater 失败: {}", e);
            e
        })?;
        println!("[DEBUG] updater 创建成功");

        // 获取最新版本信息
        println!("[DEBUG] 获取最新版本信息...");
        let latest_release = updater.get_latest_release().map_err(|e| {
            println!("[DEBUG] 获取最新版本失败: {}", e);
            let error_msg = e.to_string();
            // 提供更友好的错误信息
            if error_msg.contains("404") || error_msg.contains("Not Found") {
                anyhow::anyhow!(
                    "仓库未找到或没有可用的发布版本。请检查仓库是否存在并已创建 releases。"
                )
            } else if error_msg.contains("NetworkError") {
                anyhow::anyhow!(
                    "网络连接失败，请检查网络连接后重试。失败原因： {}",
                    error_msg
                )
            } else {
                anyhow::anyhow!("检查更新失败: {}", error_msg)
            }
        })?;
        let current_version_str = cargo_crate_version!();
        println!(
            "[DEBUG] 当前版本: {}, 最新版本: {}",
            current_version_str, latest_release.version
        );

        // 使用 semver 进行语义化版本比较
        let current_ver = Version::parse(current_version_str)
            .map_err(|e| anyhow::anyhow!("无法解析当前版本 '{}': {}", current_version_str, e))?;

        let latest_ver = Version::parse(&latest_release.version)
            .map_err(|e| anyhow::anyhow!("无法解析远程版本 '{}': {}", latest_release.version, e))?;

        // 如果当前版本 >= 最新版本，则认为已是最新
        if current_ver >= latest_ver {
            println!(
                "[DEBUG] 已是最新版本 (当前: {} >= 远程: {})",
                current_ver, latest_ver
            );
            return Ok((true, String::new()));
        }

        println!("[DEBUG] 发现新版本: {} (当前: {})", latest_ver, current_ver);
        Ok((false, latest_release.version))
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
    .map_err(|e| anyhow::anyhow!("Update check error: {}", e))?;

    let (is_up_to_date, version) = result;

    if !is_up_to_date {
        let mut s = state_clone.lock().unwrap();
        s.available = true;
        s.version = version;
    }

    Ok(is_up_to_date)
}

/// 带进度回调的更新下载（使用真实下载进度）
///
/// # Arguments
/// * `state` - 更新状态
/// * `progress_callback` - 进度回调函数 (progress: f32, downloaded: u64, total: u64, status: &str)
/// * `cancel_flag` - 取消标志
pub async fn perform_update_with_progress(
    state: &Arc<Mutex<UpdateState>>,
    progress_callback: ProgressCallback,
    cancel_flag: CancelFlag,
) -> Result<()> {
    println!("[DEBUG] perform_update_with_progress 开始执行");

    // 设置下载状态
    {
        let mut s = state.lock().unwrap();
        s.is_downloading = true;
        s.download_progress = 0.0;
        s.download_status = "Preparing...".to_string();
    }

    progress_callback(0.0, 0, 0, "Preparing download...");

    // 检查是否已取消
    if cancel_flag.load(Ordering::Relaxed) {
        println!("[DEBUG] 下载被取消（准备阶段）");
        return Err(anyhow::anyhow!("Download cancelled"));
    }

    let state_clone = state.clone();
    let progress_callback_clone = progress_callback.clone();

    // 获取 release 信息（在阻塞线程中）
    let release_info =
        tokio::task::spawn_blocking(move || -> Result<(String, String), anyhow::Error> {
            progress_callback_clone(0.05, 0, 0, "Connecting to server...");

            let updater = create_updater()?;

            progress_callback_clone(0.1, 0, 0, "Fetching release info...");

            let latest_release = updater.get_latest_release()?;
            println!("[DEBUG] 获取到最新版本: {}", latest_release.version);

            // 获取当前平台的下载 URL
            let target = self_update::get_target();
            println!("[DEBUG] 当前平台: {}", target);
            println!("[DEBUG] 可用资产列表: {:?}", 
                latest_release.assets.iter().map(|a| &a.name).collect::<Vec<_>>());

            // 查找匹配的 asset
            // self_update 需要 tar.gz 或 zip 格式，优先查找这些格式
            // 支持多种命名格式：
            // 1. 完整 target triple (如 aarch64-apple-darwin.tar.gz)
            // 2. 简化格式 (如 arm64, x64)
            // 3. apple-darwin 通用匹配
            
            // 首先尝试查找 tar.gz 格式（self_update 首选）
            let asset = latest_release
                .assets
                .iter()
                .filter(|a| {
                    let name_lower = a.name.to_lowercase();
                    name_lower.ends_with(".tar.gz") || name_lower.ends_with(".zip")
                })
                .find(|a| {
                    let name_lower = a.name.to_lowercase();
                    let target_lower = target.to_lowercase();
                    
                    // 精确匹配完整 target
                    name_lower.contains(&target_lower) ||
                    // 匹配 apple-darwin
                    name_lower.contains("apple-darwin") ||
                    // 匹配简化格式：aarch64 -> arm64, x86_64 -> x64
                    (target_lower.contains("aarch64") && name_lower.contains("arm64")) ||
                    (target_lower.contains("x86_64") && (name_lower.contains("x64") || name_lower.contains("x86_64")))
                })
                .or_else(|| {
                    // 如果找不到精确匹配的 tar.gz/zip，尝试查找任何包含平台相关关键词的压缩包
                    latest_release.assets.iter()
                        .filter(|a| {
                            let name_lower = a.name.to_lowercase();
                            name_lower.ends_with(".tar.gz") || name_lower.ends_with(".zip")
                        })
                        .find(|a| {
                            let name_lower = a.name.to_lowercase();
                            name_lower.contains("darwin") || 
                            name_lower.contains("macos") ||
                            name_lower.contains("osx")
                        })
                })
                .or_else(|| {
                    // 如果还是找不到，尝试查找任何 tar.gz 或 zip 文件
                    latest_release.assets.iter()
                        .find(|a| {
                            let name_lower = a.name.to_lowercase();
                            name_lower.ends_with(".tar.gz") || name_lower.ends_with(".zip")
                        })
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("No suitable asset found for platform: {}. Available assets: {:?}", 
                        target, 
                        latest_release.assets.iter().map(|a| &a.name).collect::<Vec<_>>())
                })?;

            println!("[DEBUG] 下载 URL: {}", asset.download_url);
            Ok((asset.download_url.clone(), latest_release.version))
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;

    let (download_url, version) = release_info;

    // 检查是否已取消
    if cancel_flag.load(Ordering::Relaxed) {
        println!("[DEBUG] 下载被取消（获取版本信息后）");
        return Err(anyhow::anyhow!("Download cancelled"));
    }

    progress_callback(0.15, 0, 0, &format!("Downloading v{}...", version));

    // 使用 reqwest 进行流式下载以获取真实进度
    let client = reqwest::Client::new();
    let response = client
        .get(&download_url)
        .header("User-Agent", format!("usher/{}", cargo_crate_version!()))
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .context("Failed to start download")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let total_size = response.content_length().unwrap_or(0);
    println!("[DEBUG] 文件大小: {} bytes", total_size);

    // 创建临时文件保存下载内容
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("usher_update_{}.tar.gz", version));
    let mut temp_file = tokio::fs::File::create(&temp_file_path)
        .await
        .context("Failed to create temp file")?;

    // 流式下载并报告进度
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    while let Some(chunk_result) = stream.next().await {
        // 检查是否已取消
        if cancel_flag.load(Ordering::Relaxed) {
            println!("[DEBUG] 下载被取消");
            // 清理临时文件
            let _ = tokio::fs::remove_file(&temp_file_path).await;
            return Err(anyhow::anyhow!("Download cancelled"));
        }

        let chunk = chunk_result.context("Failed to read chunk")?;
        temp_file
            .write_all(&chunk)
            .await
            .context("Failed to write chunk")?;

        downloaded += chunk.len() as u64;

        // 计算进度 (15% - 85% 用于下载)
        let progress = if total_size > 0 {
            0.15 + (downloaded as f32 / total_size as f32) * 0.70
        } else {
            0.5 // 如果不知道总大小，显示50%
        };

        // 格式化已下载大小
        let downloaded_mb = downloaded as f64 / 1_048_576.0;
        let total_mb = total_size as f64 / 1_048_576.0;
        let status = if total_size > 0 {
            format!("Downloading... {:.1} / {:.1} MB", downloaded_mb, total_mb)
        } else {
            format!("Downloading... {:.1} MB", downloaded_mb)
        };

        progress_callback(progress, downloaded, total_size, &status);

        // 更新状态
        {
            let mut s = state_clone.lock().unwrap();
            s.download_progress = progress;
            s.downloaded_bytes = downloaded;
            s.total_bytes = total_size;
            s.download_status = status;
        }
    }

    temp_file.flush().await?;
    drop(temp_file);

    println!("[DEBUG] 下载完成，开始安装...");
    progress_callback(
        0.85,
        downloaded,
        total_size,
        "Download complete, installing...",
    );

    // 检查是否已取消
    if cancel_flag.load(Ordering::Relaxed) {
        let _ = tokio::fs::remove_file(&temp_file_path).await;
        return Err(anyhow::anyhow!("Download cancelled"));
    }

    // 在阻塞线程中执行安装
    let temp_file_path_clone = temp_file_path.clone();
    let progress_callback_for_install = progress_callback.clone();
    let state_for_install = state_clone.clone();

    let install_result = tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
        progress_callback_for_install(0.90, downloaded, total_size, "Extracting update...");

        // 使用 self_update 的内置安装功能
        let updater = create_updater()?;

        progress_callback_for_install(0.95, downloaded, total_size, "Installing update...");

        // 执行更新（这会替换当前可执行文件）
        updater.update()?;

        progress_callback_for_install(1.0, downloaded, total_size, "Update complete!");

        // 更新状态
        {
            let mut s = state_for_install.lock().unwrap();
            s.download_progress = 1.0;
            s.download_status = "Update complete! Restarting...".to_string();
        }

        // 清理临时文件
        let _ = std::fs::remove_file(&temp_file_path_clone);

        // 短暂延迟让用户看到完成状态
        std::thread::sleep(std::time::Duration::from_millis(800));

        progress_callback_for_install(1.0, downloaded, total_size, "Restarting application...");

        // 重启应用程序
        restart_application();

        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?;

    // 清理临时文件（如果还存在）
    let _ = tokio::fs::remove_file(&temp_file_path).await;

    // 重置下载状态
    {
        let mut s = state.lock().unwrap();
        s.is_downloading = false;
    }

    install_result
}

/// 执行更新（原有方法，保持兼容性）
pub async fn perform_update(_state: &Arc<Mutex<UpdateState>>) -> Result<()> {
    // 在阻塞线程池中执行同步操作
    tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
        let updater = create_updater()?;
        updater.update()?;

        // 更新完成后重启程序
        restart_application();

        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
    .map_err(|e| anyhow::anyhow!("Update error: {}", e))?;

    Ok(())
}

/// 重启应用程序
fn restart_application() {
    println!("[DEBUG] 准备重启应用程序...");

    // 获取当前可执行文件路径
    if let Ok(current_exe) = std::env::current_exe() {
        println!("[DEBUG] 当前可执行文件: {:?}", current_exe);

        // 获取命令行参数（排除第一个参数，即程序名）
        let args: Vec<String> = std::env::args().skip(1).collect();

        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            // Windows: 使用 cmd /c start 启动新进程
            let _ = Command::new("cmd")
                .args(["/c", "start", "", &current_exe.to_string_lossy()])
                .args(&args)
                .spawn();
        }

        #[cfg(target_os = "macos")]
        {
            use std::process::Command;
            // macOS: 使用 open 命令重新打开应用
            // 如果是 .app bundle，使用 open；否则直接运行可执行文件
            let exe_path = current_exe.to_string_lossy();
            if exe_path.contains(".app/") {
                // 找到 .app 目录
                if let Some(app_pos) = exe_path.find(".app/") {
                    let app_path = &exe_path[..app_pos + 4];
                    println!("[DEBUG] 重启 macOS 应用: {}", app_path);
                    let _ = Command::new("open").arg("-n").arg(app_path).spawn();
                }
            } else {
                // 直接运行可执行文件
                println!("[DEBUG] 直接运行可执行文件: {}", exe_path);
                let _ = Command::new(&*exe_path).args(&args).spawn();
            }
        }

        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            // Linux: 直接启动新进程
            let _ = Command::new(&current_exe).args(&args).spawn();
        }
    }

    // 退出当前进程
    println!("[DEBUG] 退出当前进程");
    std::process::exit(0);
}

/// 重置更新状态
pub fn reset_update_state(state: &Arc<Mutex<UpdateState>>) {
    let mut s = state.lock().unwrap();
    s.is_downloading = false;
    s.download_progress = 0.0;
    s.download_status = String::new();
    s.downloaded_bytes = 0;
    s.total_bytes = 0;
    s.error = String::new();
}
