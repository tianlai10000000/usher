use std::{collections::HashMap, path::PathBuf};

fn main() {
    if std::env::var_os("CARGO_FEATURE_INTEGRATED_PREVIEW").is_some() {
        // Live preview imports the gallery into its interpreter root instead.
        println!("cargo:rerun-if-changed=build.rs");
        return;
    }
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let i18n = usher_i18n_build::prepare_slint_bundle(
        &root,
        std::env::var_os("OUT_DIR").unwrap(),
        env!("CARGO_PKG_NAME"),
    )
    .expect("failed to prepare playground translations");
    let ai_review = if std::env::var_os("CARGO_FEATURE_AI_CODE_REVIEW").is_some() {
        "ui/features/ai_review/full.slint"
    } else {
        "ui/features/ai_review/disabled.slint"
    };
    let config = usher_ui_build::CompilerConfiguration::new()
        .with_debug_info(
            std::env::var_os("SLINT_EMIT_DEBUG_INFO").is_some()
                || std::env::var("PROFILE").is_ok_and(|profile| profile == "debug"),
        )
        .with_bundled_translations(i18n.translations_path)
        .with_library_paths(HashMap::from([
            (
                "material".into(),
                root.join("material-1.18.1/material.slint"),
            ),
            ("i18n".into(), i18n.library_path),
            ("ai-review".into(), root.join(ai_review)),
        ]));
    // A factory needs the static handle even if the host interpreter is enabled
    // through the environment instead of Cargo's integrated-preview feature.
    let preview = std::env::var_os("SLINT_LIVE_PREVIEW");
    if preview.is_some() {
        // SAFETY: this Cargo build script is single-threaded.
        unsafe { std::env::remove_var("SLINT_LIVE_PREVIEW") };
    }
    let result = usher_ui_build::compile_library_with_config(
        root.join("ui/views/ui_components_factory.slint"),
        config,
        "usher_ui_components",
    );
    if let Some(value) = preview {
        // SAFETY: same single-threaded build-script invariant.
        unsafe { std::env::set_var("SLINT_LIVE_PREVIEW", value) };
    }
    result.expect("failed to compile playground");
}
