use std::{collections::HashMap, path::PathBuf};

fn main() {
    if std::env::var_os("CARGO_FEATURE_INTEGRATED_PREVIEW").is_some() {
        println!("cargo:rerun-if-changed=build.rs");
        return;
    }
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let i18n = usher_i18n_build::prepare_slint_bundle(
        &root,
        std::env::var_os("OUT_DIR").unwrap(),
        env!("CARGO_PKG_NAME"),
    )
    .expect("failed to prepare PR Review translations");
    let page = if std::env::var_os("CARGO_FEATURE_DEV_SHELL").is_some() {
        "ui/features/pr_review/dev_shell.slint"
    } else {
        "ui/features/pr_review/full.slint"
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
            ("pr-review-page".into(), root.join(page)),
        ]));
    let preview = std::env::var_os("SLINT_LIVE_PREVIEW");
    if preview.is_some() {
        // SAFETY: Cargo build scripts execute this code on one thread.
        unsafe { std::env::remove_var("SLINT_LIVE_PREVIEW") };
    }
    let result = usher_ui_build::compile_library_with_config(
        root.join("ui/features/pr_review/factory.slint"),
        config,
        "usher_ui_pr_review",
    );
    if let Some(value) = preview {
        // SAFETY: same single-threaded build-script boundary.
        unsafe { std::env::set_var("SLINT_LIVE_PREVIEW", value) };
    }
    result.expect("failed to compile PR Review");
}
