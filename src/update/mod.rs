pub mod check_update;
mod download_archive;
pub mod updater;

// Re-export setup_updater for easy access
pub use updater::setup_updater;

// Re-export auto-update control functions
pub use check_update::{start_auto_update, stop_auto_update};
