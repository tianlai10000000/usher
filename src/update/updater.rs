use anyhow::{Context, Result};
use self_update::cargo_crate_version;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct UpdateState {
    pub available: bool,
    pub version: String,
    pub progress: f32,
    pub error: String,
}

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
            e
        })?;
        let current_version = cargo_crate_version!();
        println!(
            "[DEBUG] 当前版本: {}, 最新版本: {}",
            current_version, latest_release.version
        );

        // 比较版本，返回是否是最新版本
        if latest_release.version == current_version {
            println!("[DEBUG] 已是最新版本");
            return Ok((true, String::new()));
        }

        println!("[DEBUG] 发现新版本: {}", latest_release.version);
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

pub async fn perform_update(_state: &Arc<Mutex<UpdateState>>) -> Result<()> {
    // 在阻塞线程池中执行同步操作
    tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
        let updater = create_updater()?;
        updater.update()?;

        // 更新完成后退出程序
        std::process::exit(0);
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
    .map_err(|e| anyhow::anyhow!("Update error: {}", e))?;

    Ok(())
}
