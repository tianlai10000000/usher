use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn prepare_language_picker(out_dir: &Path) -> std::io::Result<PathBuf> {
    let path = out_dir.join("manifest_language_picker.slint");
    let mut source = String::from(
        "import { AppState } from \"@app-state\";\n\
         import { I18n } from \"@i18n\";\n\
         import { GitHubSelectState, GitHubSelectPopup, GitHubSelectOption } from \"@select\";\n\
         import { GitHubSettingsSectionMenuButton } from \"@settings-controls\";\n\n",
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
            "export component {component} inherits Rectangle {{\n    callback changed();\n    in property <int> reset-revision: 0;\n    private property <int> commit-revision: GitHubSelectState.commit-revision;\n    width: 100%;\n    height: 40px;\n    changed reset-revision => {{ GitHubSelectState.open = false; }}\n    changed commit-revision => {{\n        if GitHubSelectState.owner == {component:?} {{ root.changed(); }}\n    }}\n"
        )
        .expect("writing to String cannot fail");
        write_trigger(
            &mut source,
            component,
            selection,
            usher_i18n::AUTO_LANGUAGE,
            auto_label,
            0,
        );
        for (index, locale) in locales.iter().enumerate() {
            write_trigger(
                &mut source,
                component,
                selection,
                locale.id,
                &format!("{:?}", locale.native_name),
                index + 1,
            );
        }
        writeln!(
            source,
            "}}\n\nexport component {component}Overlay inherits GitHubSelectPopup {{\n    owner: {component:?};\n    option-count: {option_count};\n    selected(index) => {{ root.select-option(index); }}"
        )
        .expect("writing to String cannot fail");
        write_option(&mut source, usher_i18n::AUTO_LANGUAGE, auto_label, 0);
        for (index, locale) in locales.iter().enumerate() {
            write_option(
                &mut source,
                locale.id,
                &format!("{:?}", locale.native_name),
                index + 1,
            );
        }
        source.push_str("\n    function select-option(index: int) {\n");
        for (index, id) in std::iter::once(usher_i18n::AUTO_LANGUAGE)
            .chain(locales.iter().map(|locale| locale.id))
            .enumerate()
        {
            writeln!(source, "        if index == {index} {{\n            GitHubSelectState.selected-value = {id:?};\n            {select_callback}({id:?});\n        }}")
                .expect("writing to String cannot fail");
        }
        source.push_str("        GitHubSelectState.commit-revision += 1;\n        GitHubSelectState.open = false;\n    }\n}\n\n");
    }

    if !fs::read(&path).is_ok_and(|existing| existing == source.as_bytes()) {
        fs::write(&path, source)?;
    }
    Ok(path)
}

fn write_trigger(
    source: &mut String,
    component: &str,
    selection: &str,
    id: &str,
    label: &str,
    index: usize,
) {
    writeln!(
        source,
        "\n        GitHubSettingsSectionMenuButton {{\n            x: 0px;\n            y: 0px;\n            width: parent.width;\n            visible: {selection} == {id:?};\n            text: {label};\n            open: GitHubSelectState.open && GitHubSelectState.owner == {component:?};\n            restore-focus-on-close: GitHubSelectState.restore-focus;\n            clicked => {{\n                if self.open {{ GitHubSelectState.close(true); }} else {{\n                    GitHubSelectState.show({component:?}, {selection}, {index}, root.absolute-position.x, root.absolute-position.y, root.width, root.height);\n                }}\n            }}\n        }}"
    )
    .expect("writing to String cannot fail");
}

fn write_option(source: &mut String, id: &str, label: &str, index: usize) {
    writeln!(
        source,
        "\n        GitHubSelectOption {{\n            x: 0px;\n            y: {}px;\n            width: root.option-width;\n            text: {label};\n            selected: GitHubSelectState.selected-value == {id:?};\n            active: GitHubSelectState.open && GitHubSelectState.owner == root.owner && GitHubSelectState.active-index == {index};\n            activated => {{ root.select-option({index}); }}\n        }}",
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
                repository_root.join("material-1.18.1/material.slint"),
            ),
            ("ai-review-settings".to_string(), ai_review_settings_library),
            ("i18n".to_string(), i18n.library_path),
            ("language-picker".to_string(), language_picker),
            (
                "select".to_string(),
                repository_root.join("ui/components/input/github_select.slint"),
            ),
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
