//! Generated Slint types for the Usher desktop UI.
//!
//! UI source remains under the repository-level `ui/` directory. This crate owns
//! compiling that source and exposes the generated window, globals, models, and
//! enums to the application composition crate.

slint::include_modules!();

mod floating_overlay;
pub use floating_overlay::install_floating_overlay_positioning;
