use std::backtrace::{Backtrace, BacktraceStatus};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::panic;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Local;
use log::LevelFilter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tracing::dispatcher::Dispatch;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::{Interest, Subscriber};
use tracing::{Event, Metadata};
use tracing::{error, warn};
use tracing_subscriber::fmt::writer::MakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

/// Logger configuration
#[derive(Clone, Debug)]
pub struct TraceLogConfig {
    /// Whether to enable logging
    pub active: bool,
    /// Log level
    pub level: LevelFilter,
    /// Log file path (if write_to_file is true)
    pub log_file_path: Option<PathBuf>,
    /// Whether to organize logs by date (YYYY-MM-DD/usher.log)
    pub log_per_day: bool,
}

impl Default for TraceLogConfig {
    fn default() -> Self {
        Self {
            active: true,
            level: LevelFilter::Info,
            log_file_path: None,
            log_per_day: false,
        }
    }
}

/// Global logger initialization state
static TRACING_LOGGER_INITIALIZED: OnceLock<bool> = OnceLock::new();
static TRACING_THREAD_DISPATCH: OnceLock<tracing::Dispatch> = OnceLock::new();
static TRACING_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static TRACING_ACTIVE: AtomicBool = AtomicBool::new(true);

fn is_expected_dual_stack_init_error(err: &impl std::fmt::Display) -> bool {
    let err = err.to_string();
    err.contains("attempted to set a logger after the logging system was already initialized")
}

fn is_tracing_active() -> bool {
    TRACING_ACTIVE.load(Ordering::Relaxed)
}

fn env_filter_from_config(level: LevelFilter) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level_str = level.to_string().to_lowercase();
        EnvFilter::new(level_str)
    })
}

pub fn install_thread_dispatch() {
    if !is_tracing_active() {
        return;
    }
    if let Some(dispatch) = TRACING_THREAD_DISPATCH.get() {
        let guard = tracing::dispatcher::set_default(dispatch);
        std::mem::forget(guard);
    }
}

struct ActiveFileWriter {
    file: Arc<Mutex<std::fs::File>>,
}

impl Write for ActiveFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !is_tracing_active() {
            return Ok(buf.len());
        }
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "failed to lock log file"))?;
        file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        if !is_tracing_active() {
            return Ok(());
        }
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "failed to lock log file"))?;
        file.flush()
    }
}

struct ActiveFileWriterFactory {
    file: Arc<Mutex<std::fs::File>>,
}

impl<'a> MakeWriter<'a> for ActiveFileWriterFactory {
    type Writer = ActiveFileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ActiveFileWriter {
            file: self.file.clone(),
        }
    }
}

struct TeeSubscriber {
    inner: Dispatch,
    file: Dispatch,
}

impl Subscriber for TeeSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.inner.enabled(metadata) || self.file.enabled(metadata)
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        // Prefer inner span ids to preserve original behavior.
        self.inner.new_span(attrs)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        self.inner.record(span, values);
        self.file.record(span, values);
    }

    fn record_follows_from(&self, span: &Id, follows: &Id) {
        self.inner.record_follows_from(span, follows);
        self.file.record_follows_from(span, follows);
    }

    fn event(&self, event: &Event<'_>) {
        self.inner.event(event);
        self.file.event(event);
    }

    fn enter(&self, span: &Id) {
        self.inner.enter(span);
        self.file.enter(span);
    }

    fn exit(&self, span: &Id) {
        self.inner.exit(span);
        self.file.exit(span);
    }

    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        // Always accept to avoid dropping events
        Interest::always()
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        None
    }
}

/// Initialize local logging (file-only, no console output)
/// Supports log level override via RUST_LOG (default info)
pub fn init_tracing_logger(config: TraceLogConfig) -> Result<()> {
    // Check if already initialized
    if TRACING_LOGGER_INITIALIZED.get().is_some() {
        return Ok(());
    }

    TRACING_ACTIVE.store(config.active, Ordering::Relaxed);
    if !config.active {
        return Ok(());
    }

    let log_per_day = config.log_per_day;
    let log_path = config
        .log_file_path
        .clone()
        .unwrap_or_else(|| get_default_log_path(log_per_day));

    TRACING_LOG_PATH.set(log_path.clone()).ok();

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .context("Unable to create or open log file")?;
    let file = Arc::new(Mutex::new(file));

    let file_layer = fmt::layer()
        .json() // JSON format for easier parsing/ingestion
        .with_writer(ActiveFileWriterFactory { file: file.clone() })
        .with_ansi(false); // Disable ANSI in log files

    if let Err(err) = tracing_subscriber::registry()
        .with(env_filter_from_config(config.level)) // 优先用 RUST_LOG，缺省按配置级别
        .with(file_layer)
        .try_init()
    {
        // In dual-stack mode, the `log` logger may already be initialized by the debug logger.
        // This fallback is expected in that case, so keep it out of the tracing file to avoid
        // making the local tracing log look broken on every startup.
        if is_expected_dual_stack_init_error(&err) {
            log::debug!(
                "Tracing logger detected pre-initialized global logger; enabling fallback file dispatch: {}",
                err
            );
        } else {
            // If a global subscriber is already set (e.g. by another framework),
            // add a file-only layer on the current thread without changing existing behavior.
            warn!(
                "Tracing subscriber initialization fell back to thread-local file dispatch: {}",
                err
            );
        }
        let current = tracing::dispatcher::get_default(|d| d.clone());
        let file_for_fallback = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .context("Unable to create or open log file (fallback path)")?;
        let file_for_fallback = Arc::new(Mutex::new(file_for_fallback));
        let file_subscriber = tracing_subscriber::fmt::Subscriber::builder()
            .with_env_filter(env_filter_from_config(config.level))
            .with_writer(ActiveFileWriterFactory {
                file: file_for_fallback,
            })
            .with_ansi(false)
            .finish();
        let tee = TeeSubscriber {
            inner: current,
            file: Dispatch::new(file_subscriber),
        };
        let dispatch = Dispatch::new(tee);
        TRACING_THREAD_DISPATCH.set(dispatch).ok();
        install_thread_dispatch();
    }

    // Mark as initialized
    TRACING_LOGGER_INITIALIZED.set(true).ok();

    log::debug!(
        "Tracing Logger initialized successfully! - Active: {:?}, Level: {:?}, PerDay: {:?}, Log file path: {:?}",
        config.active,
        config.level,
        log_per_day,
        log_path.display().to_string()
    );
    tracing::info!(
        event = "tracing_logger_initialized",
        source = "tracing_logger",
        tracing_active = config.active,
        level = %config.level,
        log_per_day = log_per_day,
        log_path = %log_path.display(),
        "Tracing logger file sink is ready"
    );

    Ok(())
}

/// Get the default log file path
/// Uses the directories crate to get the system standard log directory
///
/// # Arguments
/// * `log_per_day` - If true, organizes logs by date (YYYY-MM-DD/usher.log)
pub fn get_default_log_path(log_per_day: bool) -> PathBuf {
    if let Some(proj_dirs) = directories::ProjectDirs::from("com", "usher", "app") {
        let base_log_dir = proj_dirs.data_local_dir().join("logs");

        let log_dir = if log_per_day {
            // Create date-based subdirectory: logs/YYYY-MM-DD/
            let date_str = Local::now().format("%Y-%m-%d").to_string();
            base_log_dir.join(date_str)
        } else {
            base_log_dir
        };

        // Ensure the directory exists
        std::fs::create_dir_all(&log_dir).ok();
        log_dir.join("usher_tracing.log")
    } else {
        // Fallback to current directory
        if log_per_day {
            let date_str = Local::now().format("%Y-%m-%d").to_string();
            PathBuf::from(date_str).join("usher_tracing.log")
        } else {
            PathBuf::from("usher_tracing.log")
        }
    }
}

/// Set global panic hook
///
/// Any unhandled panic in the program will trigger this hook,
/// writing the panic info (location, payload, full backtrace) to the log file.
///
/// Note: call this after the logging system is initialized.
pub fn setup_global_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        if !is_tracing_active() {
            return;
        }
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let payload_str = panic_info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(|s| s.as_str())
            })
            .unwrap_or("(unparseable payload)");

        let backtrace = Backtrace::capture();
        let backtrace_status = match backtrace.status() {
            BacktraceStatus::Captured => "captured",
            BacktraceStatus::Disabled => "disabled",
            BacktraceStatus::Unsupported => "unsupported",
            _ => "unknown",
        };
        let note = "Unhandled panic captured by global panic hook; this indicates an upstream runtime failure, not a tracing logger write failure.";

        error!(
            source          = "global_panic_hook",
            panic_location = %location,
            panic_payload  = %payload_str,
            backtrace_status = backtrace_status,
            backtrace      = ?backtrace,
            note = note,
            "Unhandled panic captured"
        );

        if let Some(path) = TRACING_LOG_PATH.get() {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(
                    file,
                    "{{\"level\":\"ERROR\",\"event\":\"panic\",\"source\":\"global_panic_hook\",\"location\":\"{}\",\"payload\":\"{}\",\"backtrace_status\":\"{}\",\"note\":\"{}\",\"backtrace\":\"{:?}\"}}",
                    location, payload_str, backtrace_status, note, backtrace
                );
            }
        }

        crate::shutdown::request_shutdown("panic-hook");
    }));
}

/// Toggle tracing log output at runtime.
/// When disabled, existing log layers remain but file writes are suppressed.
pub fn set_tracing_active(active: bool) -> Result<()> {
    TRACING_ACTIVE.store(active, Ordering::Relaxed);
    if !active {
        return Ok(());
    }
    if TRACING_LOGGER_INITIALIZED.get().is_none() {
        init_tracing_logger(TraceLogConfig {
            active: true,
            level: LevelFilter::Info,
            log_file_path: None,
            log_per_day: true,
        })?;
    }
    install_thread_dispatch();
    Ok(())
}
