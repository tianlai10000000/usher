use std::collections::HashMap;
use std::path::PathBuf;

fn main() {
    let repository_root = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must provide CARGO_MANIFEST_DIR"),
    )
    .join("../..");
    let i18n = usher_i18n_build::prepare_slint_bundle(
        &repository_root,
        std::env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"),
        env!("CARGO_PKG_NAME"),
    )
    .expect("failed to prepare Slint translations");
    let config = usher_ui_build::CompilerConfiguration::new()
        .with_bundled_translations(i18n.translations_path)
        .with_library_paths(HashMap::from([
            (
                "material".to_string(),
                repository_root.join("material-1.18.1/material.slint"),
            ),
            ("i18n".to_string(), i18n.library_path),
        ]));

    // Keep the independently mounted page static for the same reason as
    // Settings: ComponentFactory requires a statically generated component.
    let live_preview = std::env::var_os("SLINT_LIVE_PREVIEW");
    if live_preview.is_some() {
        // SAFETY: Cargo build scripts are single-threaded processes.
        unsafe { std::env::remove_var("SLINT_LIVE_PREVIEW") };
    }
    let result = usher_ui_build::compile_library_with_config(
        repository_root.join("ui/views/messages.slint"),
        config,
        "usher_messages",
    );
    if let Some(value) = live_preview {
        // SAFETY: same single-threaded build-script invariant as above.
        unsafe { std::env::set_var("SLINT_LIVE_PREVIEW", value) };
    }
    result.unwrap();
}
