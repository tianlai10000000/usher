//! Optional, independently compiled static UI Components playground.
//! MainWindow mounts this product through a full-window ComponentFactory.

#[cfg(not(feature = "integrated-preview"))]
slint::include_modules!();
