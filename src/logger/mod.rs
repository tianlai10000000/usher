pub mod debug_logger;
pub mod error_handing;
pub mod tracing_logger;

use crate::logger::debug_logger::{DebugLogConfig, setup_debug_logger as init_debug_logger};
use crate::logger::tracing_logger::TraceLogConfig;
use crate::{DebugLogger, MainWindow};
use anyhow::{Context, Result};
use slint::Global;

/// Initialize tracing logger module
/// Set up tracing logger callbacks and behavior
pub fn setup_tracing_logger(_app: &MainWindow, active: bool) -> Result<()> {
    // 1. Initialize logging first (all subsequent logs depend on it)
    tracing_logger::init_tracing_logger(TraceLogConfig {
        active,
        level: log::LevelFilter::Info,
        log_file_path: None, // Use default path
        log_per_day: true,   // Organize logs by date (YYYY-MM-DD/usher.log)
    })
    .context("Logging system failed to start, the app cannot continue")?;

    // 2. Set global panic hook (capture all unhandled crashes)
    tracing_logger::setup_global_panic_hook();

    Ok(())
}

pub fn setup_debug_logger(app: &MainWindow, console_output: bool, file_output: bool) -> Result<()> {
    init_debug_logger(DebugLogConfig {
        level: log::LevelFilter::Debug,
        console_output,
        file_output,
        log_file_path: None,
        log_per_day: true,
    })
    .map_err(|e| anyhow::Error::msg(e))?;

    // Bind Slint log callbacks
    DebugLogger::get(app).on_log_error(|msg| {
        debug_logger::slint_log_error(&msg);
    });

    DebugLogger::get(app).on_log_warn(|msg| {
        debug_logger::slint_log_warn(&msg);
    });

    DebugLogger::get(app).on_log_info(|msg| {
        debug_logger::slint_log_info(&msg);
    });

    DebugLogger::get(app).on_log_debug(|msg| {
        debug_logger::slint_log_debug(&msg);
    });

    DebugLogger::get(app).on_log_trace(|msg| {
        debug_logger::slint_log_trace(&msg);
    });

    // Configuration callbacks (optional implementation)
    DebugLogger::get(app).on_set_level(|level| {
        log::info!("[Logger] Level change requested to: {}", level);
        // Note: fern cannot dynamically change level after initialization
        // This only logs the request, actual change requires app restart
    });

    DebugLogger::get(app).on_set_write_to_file(|enabled| {
        log::info!("[Logger] Write to file change requested: {}", enabled);
        // Same as above, requires app restart to take effect
    });

    log::info!("[Logger] Slint callbacks bound");
    Ok(())
}
