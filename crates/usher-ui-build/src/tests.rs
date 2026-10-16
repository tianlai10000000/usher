use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "usher-ui-build-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn file(&self, path: &str) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "fixture").unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn generated_files_are_excluded_but_external_sources_and_assets_remain() {
    let fixture = Fixture::new();
    let source = fixture.file("ui/main.slint");
    let asset = fixture.file("ui/icon.svg");
    let generated = fixture.file("out/catalog/i18n.slint");
    let translation = fixture.file("out/translations/zh/LC_MESSAGES/fixture.po");
    let sibling = fixture.file("out-sibling/external.slint");
    let out = fixture.0.join("out");
    let actual = source_dependencies(
        &[
            source.clone(),
            asset.clone(),
            generated,
            translation,
            sibling.clone(),
            out.clone(),
        ],
        &fixture.0,
        &out,
    )
    .unwrap();
    let expected = [source, asset, sibling]
        .map(|path| path.canonicalize().unwrap())
        .into_iter()
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn relative_dot_segments_are_checked_and_original_source_paths_preserved() {
    let fixture = Fixture::new();
    let source = fixture.file("ui/main.slint");
    fixture.file("out/catalog/i18n.slint");
    let actual = source_dependencies(
        &[
            PathBuf::from("ui/../out/catalog/i18n.slint"),
            PathBuf::from("out/../ui/main.slint"),
            source.clone(),
        ],
        &fixture.0,
        &fixture.0.join("out"),
    )
    .unwrap();
    assert_eq!(
        actual,
        BTreeSet::from([
            fixture.0.join("out/../ui/main.slint"),
            source.canonicalize().unwrap(),
        ])
    );
}

#[test]
fn missing_dependency_fails_instead_of_losing_a_real_input_watch() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("out")).unwrap();
    let result = source_dependencies(
        &[fixture.0.join("missing.slint")],
        &fixture.0,
        &fixture.0.join("out"),
    );
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
}

#[cfg(unix)]
#[test]
fn symlink_aliases_remain_watched_when_retargeted_across_the_output_boundary() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let source = fixture.file("ui/source.slint");
    let generated = fixture.file("out/generated.slint");
    symlink(&source, fixture.0.join("out/source-link.slint")).unwrap();
    symlink(&generated, fixture.0.join("ui/generated-link.slint")).unwrap();
    symlink(fixture.0.join("out"), fixture.0.join("out-alias")).unwrap();
    let actual = source_dependencies(
        &[
            fixture.0.join("out/source-link.slint"),
            fixture.0.join("ui/generated-link.slint"),
            fixture.0.join("out-alias/generated.slint"),
            fixture.0.join("out-alias"),
        ],
        &fixture.0,
        &fixture.0.join("out-alias"),
    )
    .unwrap();
    let link = fixture.0.join("out/source-link.slint");
    assert_eq!(
        actual,
        BTreeSet::from([
            source.canonicalize().unwrap(),
            link.clone(),
            fixture.0.join("ui/generated-link.slint"),
        ])
    );

    // The same alias must still be watched after changing its external target.
    let replacement = fixture.file("ui/replacement.slint");
    fs::remove_file(&link).unwrap();
    symlink(&replacement, &link).unwrap();
    let retargeted = source_dependencies(
        std::slice::from_ref(&link),
        &fixture.0,
        &fixture.0.join("out"),
    )
    .unwrap();
    assert_eq!(
        retargeted,
        BTreeSet::from([replacement.canonicalize().unwrap(), link.clone()])
    );
    fs::remove_file(&link).unwrap();
    assert!(source_dependencies(&[link], &fixture.0, &fixture.0.join("out")).is_err());

    // An external alias of a generated file must survive the opposite retarget.
    let link = fixture.0.join("ui/generated-link.slint");
    fs::remove_file(&link).unwrap();
    symlink(&source, &link).unwrap();
    let retargeted = source_dependencies(
        std::slice::from_ref(&link),
        &fixture.0,
        &fixture.0.join("out"),
    )
    .unwrap();
    assert_eq!(retargeted, BTreeSet::from([source, link.clone()]));
    fs::remove_file(&link).unwrap();
    assert!(source_dependencies(&[link], &fixture.0, &fixture.0.join("out")).is_err());
}
