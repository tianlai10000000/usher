use std::collections::HashMap;
use std::path::Path;

fn main() {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(HashMap::from([(
            "material".to_string(),
            repository_root.join("material-1.7.1-fixed/material.slint"),
        )]))
        .as_library("usher_ui_settings");

    // Slint 1.17's live-preview generator produces a dynamic component handle,
    // while ComponentFactory currently requires a statically generated VWeak.
    // Keep the independently mounted factory product static; the main window
    // remains live-reloadable and still owns the UI Components preview route.
    let live_preview = std::env::var_os("SLINT_LIVE_PREVIEW");
    if live_preview.is_some() {
        // SAFETY: Cargo runs this build script as a single-threaded process and
        // no thread is concurrently reading or mutating its environment.
        unsafe { std::env::remove_var("SLINT_LIVE_PREVIEW") };
    }
    let result =
        slint_build::compile_with_config(repository_root.join("ui/views/settings.slint"), config);
    if let Some(value) = live_preview {
        // SAFETY: same single-threaded build-script invariant as above.
        unsafe { std::env::set_var("SLINT_LIVE_PREVIEW", value) };
    }
    result.unwrap();
}
