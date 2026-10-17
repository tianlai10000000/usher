// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::path::PathBuf;

fn main() {
    let repository_root = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must provide CARGO_MANIFEST_DIR"),
    )
    .join("../..");
    let ui_components_library = if std::env::var_os("CARGO_FEATURE_UI_COMPONENTS").is_some() {
        if std::env::var_os("CARGO_FEATURE_LIVE_PREVIEW").is_some() {
            repository_root.join("ui/features/ui_components_enabled.slint")
        } else {
            repository_root.join("ui/features/ui_components_factory.slint")
        }
    } else {
        repository_root.join("ui/features/ui_components_disabled.slint")
    };
    let pr_review_page = if std::env::var_os("CARGO_FEATURE_DEV_SHELL").is_some() {
        repository_root.join("ui/features/pr_review/dev_shell.slint")
    } else {
        repository_root.join("ui/features/pr_review/full.slint")
    };
    let pr_review_library = if std::env::var_os("CARGO_FEATURE_LIVE_PREVIEW").is_some() {
        repository_root.join("ui/features/pr_review/integrated.slint")
    } else {
        repository_root.join("ui/features/pr_review/factory_host.slint")
    };
    let ai_review_library = if std::env::var_os("CARGO_FEATURE_AI_CODE_REVIEW").is_some() {
        repository_root.join("ui/features/ai_review/full.slint")
    } else {
        repository_root.join("ui/features/ai_review/disabled.slint")
    };
    let emit_debug_info = std::env::var_os("SLINT_EMIT_DEBUG_INFO").is_some()
        || std::env::var("PROFILE").is_ok_and(|profile| profile == "debug");
    let i18n = usher_i18n_build::prepare_slint_bundle(
        &repository_root,
        std::env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"),
        env!("CARGO_PKG_NAME"),
    )
    .expect("failed to prepare Slint translations");
    let config = usher_ui_build::CompilerConfiguration::new()
        .with_debug_info(emit_debug_info)
        .with_bundled_translations(i18n.translations_path)
        .with_library_paths(HashMap::from([
            (
                "material".to_string(),
                repository_root.join("material-1.18.1/material.slint"),
            ),
            ("ui-components".to_string(), ui_components_library),
            ("pr-review".to_string(), pr_review_library),
            ("pr-review-page".to_string(), pr_review_page),
            ("ai-review".to_string(), ai_review_library),
            ("i18n".to_string(), i18n.library_path),
        ]));

    usher_ui_build::compile_with_config(repository_root.join("ui/main.slint"), config).unwrap();
}
