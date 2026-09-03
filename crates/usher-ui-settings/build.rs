use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn prepare_language_picker(out_dir: &Path) -> std::io::Result<PathBuf> {
    let path = out_dir.join("manifest_language_picker.slint");
    let mut source = String::from(
        "import { AppState } from \"@app-state\";\n\
         import { I18n } from \"@i18n\";\n\
         import { GitHubScrollView } from \"@scroll-view\";\n\
         import { GitHubTheme } from \"@github-theme\";\n\
         import { GitHubSettingsNavItem, GitHubSettingsSectionMenuButton } from \"@settings-controls\";\n\n",
    );
    let locales = usher_i18n::available_locales();

    for (component, selection, auto_label, select_callback) in [
        (
            "ManifestInterfaceLanguagePicker",
            "AppState.language-selection.id",
            "I18n.language-system-default",
            "AppState.set-language",
        ),
        (
            "ManifestReplyLanguagePicker",
            "AppState.ai-agent-reply-language-selection.id",
            "I18n.settings-ai-agent-reply-language-auto",
            "AppState.select-ai-agent-reply-language",
        ),
    ] {
        let option_count = locales.len() + 1;
        writeln!(
            source,
            "export component {component} inherits VerticalLayout {{\n    callback changed();\n    private property <bool> open: false;\n    private property <string> menu-selection: \"\";\n    width: 100%;\n    spacing: root.open ? 4px : 0px;\n\n    Rectangle {{\n        width: parent.width;\n        height: 40px;\n        background: transparent;"
        )
        .expect("writing to String cannot fail");
        write_trigger(
            &mut source,
            selection,
            usher_i18n::AUTO_LANGUAGE,
            auto_label,
        );
        for locale in locales {
            write_trigger(
                &mut source,
                selection,
                locale.id,
                &format!("{:?}", locale.native_name),
            );
        }
        writeln!(
            source,
            "    }}\n\n    Rectangle {{\n        width: parent.width;\n        height: root.open ? min(264px, {}px) : 0px;\n        visible: root.open;\n        border-radius: 6px;\n        border-width: 1px;\n        border-color: GitHubTheme.border-default;\n        background: GitHubTheme.canvas-default;\n        clip: true;\n\n        GitHubScrollView {{\n            x: 4px;\n            y: 4px;\n            width: parent.width - 8px;\n            height: parent.height - 8px;\n            show-scrollbar: {};\n\n            Rectangle {{\n                width: parent.width;\n                height: {}px;\n                background: transparent;",
            option_count * 32 + 8,
            option_count > 8,
            option_count * 32,
        )
        .expect("writing to String cannot fail");
        write_option(
            &mut source,
            usher_i18n::AUTO_LANGUAGE,
            auto_label,
            select_callback,
            0,
        );
        for (index, locale) in locales.iter().enumerate() {
            write_option(
                &mut source,
                locale.id,
                &format!("{:?}", locale.native_name),
                select_callback,
                index + 1,
            );
        }
        source.push_str("            }\n        }\n    }\n}\n\n");
    }

    if !fs::read(&path).is_ok_and(|existing| existing == source.as_bytes()) {
        fs::write(&path, source)?;
    }
    Ok(path)
}

fn write_trigger(source: &mut String, selection: &str, id: &str, label: &str) {
    writeln!(
        source,
        "\n        GitHubSettingsSectionMenuButton {{\n            width: parent.width;\n            visible: {selection} == {id:?};\n            text: {label};\n            open: root.open;\n            clicked => {{\n                root.menu-selection = {selection};\n                root.open = !root.open;\n            }}\n        }}"
    )
    .expect("writing to String cannot fail");
}

fn write_option(source: &mut String, id: &str, label: &str, select_callback: &str, index: usize) {
    writeln!(
        source,
        "\n                GitHubSettingsNavItem {{\n                    y: {}px;\n                    width: parent.width;\n                    text: {label};\n                    selected: root.menu-selection == {id:?};\n                    clicked => {{\n                        root.menu-selection = {id:?};\n                        {select_callback}({id:?});\n                        root.changed();\n                        root.open = false;\n                    }}\n                }}",
        index * 32,
    )
    .expect("writing to String cannot fail");
}

fn main() {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ai_review_settings_library = if std::env::var_os("CARGO_FEATURE_AI_CODE_REVIEW").is_some() {
        repository_root.join("ui/features/ai_review_settings/full.slint")
    } else {
        repository_root.join("ui/features/ai_review_settings/disabled.slint")
    };
    let i18n = usher_i18n::build_support::prepare_slint_bundle(
        std::env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"),
        env!("CARGO_PKG_NAME"),
    )
    .expect("failed to prepare Slint translations");
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"));
    let language_picker =
        prepare_language_picker(&out_dir).expect("failed to prepare manifest language picker");
    println!(
        "cargo:rerun-if-changed={}",
        repository_root.join("i18n/manifest.toml").display()
    );
    let emit_debug_info = std::env::var_os("SLINT_EMIT_DEBUG_INFO").is_some()
        || std::env::var("PROFILE").is_ok_and(|profile| profile == "debug");
    let config = slint_build::CompilerConfiguration::new()
        .with_debug_info(emit_debug_info)
        .with_bundled_translations(i18n.translations_path)
        .with_library_paths(HashMap::from([
            (
                "material".to_string(),
                repository_root.join("material-1.7.1-fixed/material.slint"),
            ),
            ("ai-review-settings".to_string(), ai_review_settings_library),
            ("i18n".to_string(), i18n.library_path),
            ("language-picker".to_string(), language_picker),
            (
                "app-state".to_string(),
                repository_root.join("ui/state/app_state.slint"),
            ),
            (
                "scroll-view".to_string(),
                repository_root.join("ui/components/github_scroll_view.slint"),
            ),
            (
                "github-theme".to_string(),
                repository_root.join("ui/theme/github_theme.slint"),
            ),
            (
                "settings-controls".to_string(),
                repository_root.join("ui/components/settings/github_settings_controls.slint"),
            ),
        ]))
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
