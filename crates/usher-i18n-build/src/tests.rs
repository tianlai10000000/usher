use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{compile_rust_catalog, prepare_slint_bundle};

const MANIFEST: &str = r#"source_locale = "en"
fallback_locale = "en"
[[locales]]
id = "en"
native_name = "English"
aliases = ["en-US"]
minimum_coverage = 1.0
[[locales]]
id = "zh-CN"
native_name = "简体中文"
aliases = ["zh"]
minimum_coverage = 1.0
"#;
const UI_MESSAGES: &str = r#"[[messages]]
key = "fixture.title"
text = "Hello {name}"
[[messages]]
key = "fixture.note"
text = "Ready"
"#;
const RUST_MESSAGES: &str = r#"[[messages]]
key = "fixture.platform"
text = "Open folder"
slint = false
"#;
const UI_PO: &str = r#"msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"
"Language: zh-CN\n"

msgctxt "fixture.title"
msgid "Hello {name}"
msgstr "你好 {name}"

msgctxt "fixture.note"
msgid "Ready"
msgstr "就绪"
"#;
const RUST_PO: &str = r#"msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"
"Language: zh-CN\n"

msgctxt "fixture.platform"
msgid "Open folder"
msgstr "打开文件夹"
"#;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "usher-i18n-build-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("create isolated fixture");
        let fixture = Self { root };
        fixture.write("manifest.toml", MANIFEST);
        fixture.write("messages.toml", UI_MESSAGES);
        fixture.write("rust-messages.toml", RUST_MESSAGES);
        fixture.write("locales/zh-CN/LC_MESSAGES/usher.po", UI_PO);
        fixture.write("locales/zh-CN/LC_MESSAGES/usher-rust.po", RUST_PO);
        fixture
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join("i18n").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn compile(&self) -> crate::BuildResult<()> {
        compile_rust_catalog(&self.root, &self.root.join("runtime"))
    }

    fn runtime(&self) -> String {
        fs::read_to_string(self.root.join("runtime/catalogs.rs")).unwrap()
    }

    fn prepare(&self, domain: &str) -> crate::PreparedSlintBundle {
        prepare_slint_bundle(&self.root, self.root.join(domain), domain).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_error(result: crate::BuildResult<()>, expected: &str) {
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "expected {expected:?}, found {error:?}"
    );
}

fn bundled_po(bundle: &crate::PreparedSlintBundle, domain: &str) -> PathBuf {
    bundle
        .translations_path
        .join("zh-CN/LC_MESSAGES")
        .join(format!("{domain}.po"))
}

fn snapshot(path: &Path) -> (Vec<u8>, std::time::SystemTime) {
    (
        fs::read(path).unwrap(),
        fs::metadata(path).unwrap().modified().unwrap(),
    )
}

#[test]
fn runtime_merges_scopes_without_embedding_build_inputs() {
    let fixture = Fixture::new();
    fixture.compile().unwrap();
    let generated = fixture.runtime();
    assert!(generated.contains("fixture.title"));
    assert!(generated.contains("fixture.platform"));
    assert!(generated.contains("你好 {name}"));
    assert!(generated.contains("打开文件夹"));
    assert!(!generated.contains("GENERATED_SLINT_CATALOG"));
    assert!(!generated.contains("GENERATED_PO_SOURCES"));
    assert!(!generated.contains("include_str!"));
    assert!(!generated.contains(fixture.root.to_string_lossy().as_ref()));
}

#[test]
fn every_domain_receives_identical_complete_ui_catalog_and_original_po_bytes() {
    let fixture = Fixture::new();
    let first = fixture.prepare("usher-ui");
    let second = fixture.prepare("usher-messages");
    assert_eq!(
        fs::read(&first.library_path).unwrap(),
        fs::read(&second.library_path).unwrap()
    );
    assert_eq!(
        fs::read(bundled_po(&first, "usher-ui")).unwrap(),
        UI_PO.as_bytes()
    );
    assert_eq!(
        fs::read(bundled_po(&second, "usher-messages")).unwrap(),
        UI_PO.as_bytes()
    );
    let slint = fs::read_to_string(first.library_path).unwrap();
    assert!(slint.contains("fixture-title"));
    assert!(slint.contains("fixture-note"));
    assert!(slint.contains("{{name}}"));
    assert!(!slint.contains("fixture-platform"));
    assert_eq!(first.locales[1].id, "zh-CN");
}

#[test]
fn rust_only_edits_and_invalid_rust_files_leave_ui_bundle_bytes_and_mtimes_unchanged() {
    let fixture = Fixture::new();
    let bundle = fixture.prepare("usher-ui");
    let library_before = snapshot(&bundle.library_path);
    let po_path = bundled_po(&bundle, "usher-ui");
    let po_before = snapshot(&po_path);
    fixture.write("rust-messages.toml", "this is not TOML");
    fixture.write("locales/zh-CN/LC_MESSAGES/usher-rust.po", "invalid PO");
    fixture.prepare("usher-ui");
    assert_eq!(snapshot(&bundle.library_path), library_before);
    assert_eq!(snapshot(&po_path), po_before);
    assert!(fixture.compile().is_err());
}

#[test]
fn rust_only_source_and_translation_edits_refresh_runtime_without_changing_ui() {
    let fixture = Fixture::new();
    fixture.compile().unwrap();
    let bundle = fixture.prepare("usher-ui");
    let library = snapshot(&bundle.library_path);
    let po_path = bundled_po(&bundle, "usher-ui");
    let po = snapshot(&po_path);
    fixture.write(
        "rust-messages.toml",
        &RUST_MESSAGES.replace("Open folder", "Show folder"),
    );
    fixture.write(
        "locales/zh-CN/LC_MESSAGES/usher-rust.po",
        &RUST_PO
            .replace("Open folder", "Show folder")
            .replace("打开文件夹", "显示文件夹"),
    );
    fixture.compile().unwrap();
    fixture.prepare("usher-ui");
    assert!(fixture.runtime().contains("Show folder"));
    assert!(fixture.runtime().contains("显示文件夹"));
    assert_eq!(snapshot(&bundle.library_path), library);
    assert_eq!(snapshot(&po_path), po);
}

#[test]
fn supplied_checkout_paths_are_resolved_independently_on_every_call() {
    let first = Fixture::new();
    let second = Fixture::new();
    second.write("messages.toml", &UI_MESSAGES.replace("Ready", "Changed"));
    second.write(
        "locales/zh-CN/LC_MESSAGES/usher.po",
        &UI_PO.replace("Ready", "Changed"),
    );
    let a = first.prepare("usher-ui");
    let b = second.prepare("usher-ui");
    assert!(
        fs::read_to_string(a.library_path)
            .unwrap()
            .contains("Ready")
    );
    assert!(
        fs::read_to_string(b.library_path)
            .unwrap()
            .contains("Changed")
    );
    first.compile().unwrap();
    second.compile().unwrap();
    assert!(first.runtime().contains("Ready"));
    assert!(second.runtime().contains("Changed"));
}

#[test]
fn ui_source_and_translation_edits_update_their_respective_outputs() {
    let fixture = Fixture::new();
    let before = fixture.prepare("usher-ui");
    let library = fs::read(&before.library_path).unwrap();
    fixture.write("messages.toml", &UI_MESSAGES.replace("Ready", "Changed"));
    fixture.write(
        "locales/zh-CN/LC_MESSAGES/usher.po",
        &UI_PO.replace("Ready", "Changed"),
    );
    fixture.prepare("usher-ui");
    assert_ne!(fs::read(&before.library_path).unwrap(), library);
    let library = fs::read(&before.library_path).unwrap();
    fixture.write(
        "locales/zh-CN/LC_MESSAGES/usher.po",
        &UI_PO.replace("Ready", "Changed").replace("就绪", "已更改"),
    );
    fixture.prepare("usher-ui");
    assert_eq!(fs::read(&before.library_path).unwrap(), library);
    assert!(
        fs::read_to_string(bundled_po(&before, "usher-ui"))
            .unwrap()
            .contains("已更改")
    );
    fixture.compile().unwrap();
    assert!(fixture.runtime().contains("已更改"));
}

#[test]
fn duplicate_keys_and_property_collisions_are_rejected_across_scopes() {
    let fixture = Fixture::new();
    fixture.write(
        "rust-messages.toml",
        &RUST_MESSAGES.replace("fixture.platform", "fixture.title"),
    );
    assert_error(fixture.compile(), "duplicate message key");
    fixture.write(
        "rust-messages.toml",
        &RUST_MESSAGES.replace("fixture.platform", "fixture_title"),
    );
    assert_error(fixture.compile(), "collide as Slint property");
}

#[test]
fn malformed_source_placeholders_and_unknown_registry_fields_fail() {
    for invalid in [
        UI_MESSAGES.replace("Hello {name}", "Hello {name"),
        UI_MESSAGES.replace("Hello {name}", "Hello {}"),
        UI_MESSAGES.replace("Hello {name}", "Hello }{name}"),
        format!("{UI_MESSAGES}unknown_field = 1\n"),
        UI_MESSAGES.replace("fixture.title", "fixture title"),
    ] {
        let fixture = Fixture::new();
        fixture.write("messages.toml", &invalid);
        assert!(
            fixture.compile().is_err(),
            "accepted invalid messages {invalid:?}"
        );
        assert!(prepare_slint_bundle(&fixture.root, fixture.root.join("ui"), "usher-ui").is_err());
    }
}

#[test]
fn scope_flags_cannot_silently_hide_or_expose_messages() {
    let fixture = Fixture::new();
    fixture.write(
        "rust-messages.toml",
        &RUST_MESSAGES.replace("slint = false", "slint = true"),
    );
    assert_error(fixture.compile(), "misplaced message");
    fixture.write("rust-messages.toml", RUST_MESSAGES);
    fixture.write("messages.toml", &format!("{UI_MESSAGES}slint = false\n"));
    assert_error(fixture.compile(), "misplaced message");
    assert!(prepare_slint_bundle(&fixture.root, fixture.root.join("ui"), "usher-ui").is_err());
}

#[test]
fn invalid_manifest_metadata_aliases_fields_and_syntax_fail() {
    for invalid in [
        MANIFEST.replace("id = \"zh-CN\"", "id = \"../zh\""),
        MANIFEST.replace("aliases = [\"zh\"]", "aliases = [\"EN_us\"]"),
        MANIFEST.replace("fallback_locale = \"en\"", "fallback_locale = \"missing\""),
        MANIFEST.replace("minimum_coverage = 1.0", "minimum_coverage = 1.1"),
        format!("{MANIFEST}segmented_count_suffix_offset = 3\n"),
        format!("{MANIFEST}pr_list_compact_text_offset_y = -3\n"),
        format!("{MANIFEST}unknown_field = true\n"),
        "[broken".to_owned(),
    ] {
        let fixture = Fixture::new();
        fixture.write("manifest.toml", &invalid);
        assert!(
            fixture.compile().is_err(),
            "accepted invalid manifest {invalid:?}"
        );
    }
}

#[test]
fn stale_duplicate_contexts_source_drift_and_placeholder_drift_fail_in_both_paths() {
    for invalid in [
        UI_PO.replace("fixture.note", "unknown.context"),
        format!("{UI_PO}\nmsgctxt \"fixture.note\"\nmsgid \"Ready\"\nmsgstr \"重复\"\n"),
        UI_PO.replace("msgid \"Ready\"", "msgid \"Stale\""),
        UI_PO.replace("你好 {name}", "你好 {other}"),
        UI_PO.replace("你好 {name}", "你好 {name"),
        "invalid PO".to_owned(),
    ] {
        let fixture = Fixture::new();
        fixture.write("locales/zh-CN/LC_MESSAGES/usher.po", &invalid);
        assert!(
            fixture.compile().is_err(),
            "accepted invalid PO {invalid:?}"
        );
        assert!(prepare_slint_bundle(&fixture.root, fixture.root.join("ui"), "usher-ui").is_err());
    }
}

#[test]
fn coverage_remains_aggregate_and_missing_optional_translations_fall_back_to_source() {
    let fixture = Fixture::new();
    fixture.write(
        "manifest.toml",
        &MANIFEST.replace("minimum_coverage = 1.0", "minimum_coverage = 0.6"),
    );
    fixture.write(
        "locales/zh-CN/LC_MESSAGES/usher-rust.po",
        &RUST_PO.replace("打开文件夹", ""),
    );
    fixture.compile().unwrap(); // 2 / 3 total, despite 0 / 1 in the Rust scope.
    assert!(fixture.runtime().contains("value: \"Open folder\""));
    fixture.write("manifest.toml", MANIFEST);
    assert_error(fixture.compile(), "translation coverage");
    fixture.prepare("usher-ui"); // UI generation leaves the aggregate runtime gate authoritative.
}

#[test]
fn fuzzy_translations_fall_back_and_count_as_missing_in_the_aggregate_gate() {
    let fixture = Fixture::new();
    fixture.write(
        "manifest.toml",
        &MANIFEST.replace("minimum_coverage = 1.0", "minimum_coverage = 0.6"),
    );
    fixture.write(
        "locales/zh-CN/LC_MESSAGES/usher-rust.po",
        &RUST_PO.replace(
            "msgctxt \"fixture.platform\"",
            "#, fuzzy\nmsgctxt \"fixture.platform\"",
        ),
    );
    fixture.compile().unwrap();
    assert!(fixture.runtime().contains("value: \"Open folder\""));
    assert!(!fixture.runtime().contains("打开文件夹"));
    fixture.write("manifest.toml", MANIFEST);
    assert_error(fixture.compile(), "translation coverage");
}

#[test]
fn unsafe_domains_are_rejected_and_repeated_generation_preserves_mtime() {
    let fixture = Fixture::new();
    for domain in ["", "../escape", "with space", "/absolute", "a\\b"] {
        assert!(prepare_slint_bundle(&fixture.root, fixture.root.join("ui"), domain).is_err());
    }
    fixture.compile().unwrap();
    let path = fixture.root.join("runtime/catalogs.rs");
    let before = snapshot(&path);
    fixture.compile().unwrap();
    assert_eq!(snapshot(&path), before);
}
