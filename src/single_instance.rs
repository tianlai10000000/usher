use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use fs2::FileExt;
use slint::ComponentHandle;

use crate::MainWindow;

static INSTANCE_LOCK_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

fn instance_lock_file() -> &'static Mutex<Option<File>> {
    INSTANCE_LOCK_FILE.get_or_init(|| Mutex::new(None))
}

fn lock_file_name(namespace: Option<&str>) -> String {
    let namespace = namespace.filter(|value| {
        !value.is_empty()
            && value.len() <= 80
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    });
    namespace.map_or_else(
        || "usher.instance.lock".to_owned(),
        |value| format!("usher.{value}.instance.lock"),
    )
}

fn activation_file_name(namespace: Option<&str>) -> String {
    lock_file_name(namespace).replace(".instance.lock", ".instance.activate")
}

fn lock_file_path() -> PathBuf {
    let namespace = std::env::var("USHER_BENCHMARK_INSTANCE_NAMESPACE").ok();
    let file_name = lock_file_name(namespace.as_deref());
    if let Some(dir) = crate::app_paths::data_local_dir() {
        let _ = std::fs::create_dir_all(&dir);
        dir.join(file_name)
    } else {
        PathBuf::from(file_name)
    }
}

fn activation_file_path() -> PathBuf {
    let namespace = std::env::var("USHER_BENCHMARK_INSTANCE_NAMESPACE").ok();
    let file_name = activation_file_name(namespace.as_deref());
    crate::app_paths::data_local_dir()
        .map(|dir| dir.join(&file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
}

/// Acquire global process lock.
/// Returns false when another instance is already running.
pub fn acquire_instance_lock() -> Result<bool> {
    let mut lock_file = instance_lock_file()
        .lock()
        .map_err(|error| anyhow::anyhow!("Instance lock state is poisoned: {error}"))?;
    if lock_file.is_some() {
        return Ok(true);
    }

    let path = lock_file_path();
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("Failed to open lock file: {}", path.display()))?;

    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = std::fs::remove_file(activation_file_path());
            *lock_file = Some(file);
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

pub fn request_existing_instance_activation() -> Result<()> {
    let path = activation_file_path();
    let nonce = format!(
        "{}:{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    usher_fs::atomic_write_private(&path, nonce.as_bytes())
        .with_context(|| format!("Failed to publish activation request: {}", path.display()))
}

pub struct ActivationListener {
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for ActivationListener {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(join) = self.join.take()
            && let Err(error) = join.join()
        {
            log::warn!("[SingleInstance] Activation listener panicked: {error:?}");
        }
    }
}

pub fn start_activation_listener(window: &MainWindow) -> Result<ActivationListener> {
    let path = activation_file_path();
    let running = Arc::new(AtomicBool::new(true));
    let running_worker = running.clone();
    let weak = window.as_weak();
    let join = std::thread::Builder::new()
        .name("single-instance-activation".to_string())
        .spawn(move || {
            let mut previous = String::new();
            while running_worker.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(100));
                let Ok(request) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if request.is_empty() || request == previous {
                    continue;
                }
                previous = request;
                let weak = weak.clone();
                if slint::invoke_from_event_loop(move || {
                    let Some(window) = weak.upgrade() else {
                        return;
                    };
                    if let Err(error) = window.show() {
                        log::error!("[SingleInstance] Failed to reveal existing window: {error}");
                        return;
                    }
                    crate::platform::activate_main_window(&window);
                    window.window().request_redraw();
                })
                .is_err()
                {
                    break;
                }
            }
        })
        .context("Failed to start single-instance activation listener")?;
    Ok(ActivationListener {
        running,
        join: Some(join),
    })
}

/// Release the process lock before handing an update restart to a new process.
/// Dropping the file remains a fallback if the explicit unlock reports an OS
/// error. Closing the descriptor releases the OS lock, so that failure is
/// logged but does not leave the current process running without a tracked lock.
pub fn release_instance_lock() -> Result<()> {
    let mut lock_file = instance_lock_file()
        .lock()
        .map_err(|error| anyhow::anyhow!("Instance lock state is poisoned: {error}"))?;
    let Some(file) = lock_file.take() else {
        return Ok(());
    };
    if let Err(error) = fs2::FileExt::unlock(&file) {
        log::warn!("Explicit instance unlock failed; closing the lock file as fallback: {error}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{activation_file_name, lock_file_name};

    #[test]
    fn ordinary_launches_keep_the_global_lock_name() {
        assert_eq!(lock_file_name(None), "usher.instance.lock");
        assert_eq!(lock_file_name(Some("")), "usher.instance.lock");
    }

    #[test]
    fn benchmark_namespaces_are_bounded_and_path_safe() {
        assert_eq!(
            lock_file_name(Some("chat-fast-drag-20260814")),
            "usher.chat-fast-drag-20260814.instance.lock"
        );
        assert_eq!(lock_file_name(Some("../escape")), "usher.instance.lock");
        assert_eq!(
            lock_file_name(Some("contains space")),
            "usher.instance.lock"
        );
    }

    #[test]
    fn activation_channel_uses_the_same_safe_namespace() {
        assert_eq!(
            activation_file_name(Some("chat-fast-drag")),
            "usher.chat-fast-drag.instance.activate"
        );
        assert_eq!(
            activation_file_name(Some("../escape")),
            "usher.instance.activate"
        );
    }
}
