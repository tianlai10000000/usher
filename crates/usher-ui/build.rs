// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::path::Path;

fn main() {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ui_components_library = if std::env::var_os("CARGO_FEATURE_UI_COMPONENTS").is_some() {
        repository_root.join("ui/features/ui_components_enabled.slint")
    } else {
        repository_root.join("ui/features/ui_components_disabled.slint")
    };
    let pr_review_library = if std::env::var_os("CARGO_FEATURE_DEV_SHELL").is_some() {
        repository_root.join("ui/features/pr_review/dev_shell.slint")
    } else {
        repository_root.join("ui/features/pr_review/full.slint")
    };
    let ai_review_library = if std::env::var_os("CARGO_FEATURE_AI_CODE_REVIEW").is_some() {
        repository_root.join("ui/features/ai_review/full.slint")
    } else {
        repository_root.join("ui/features/ai_review/disabled.slint")
    };
    let config = slint_build::CompilerConfiguration::new().with_library_paths(HashMap::from([
        (
            "material".to_string(),
            repository_root.join("material-1.7.1-fixed/material.slint"),
        ),
        ("ui-components".to_string(), ui_components_library),
        ("pr-review".to_string(), pr_review_library),
        ("ai-review".to_string(), ai_review_library),
    ]));

    slint_build::compile_with_config(repository_root.join("ui/main.slint"), config).unwrap();
}
