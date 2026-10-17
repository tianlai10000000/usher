"""Reuse the real i18n build script across isolated checkout-path fixtures.

Run ``cargo check -p usher-i18n --locked`` first. Set
USHER_TEST_I18N_BUILD_SCRIPT to exercise a particular before/after executable.
No fixture workspace is compiled, and no workstation settings are used.
"""

import copy
import json
import re
import tomllib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


def build_script_executable():
    explicit = os.environ.get("USHER_TEST_I18N_BUILD_SCRIPT")
    if explicit:
        executable = Path(explicit).resolve()
        if not executable.is_file():
            raise AssertionError(f"i18n build script does not exist: {executable}")
        return executable

    metadata = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked", "--offline"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    if metadata.returncode:
        raise AssertionError(metadata.stdout + metadata.stderr)
    target = Path(json.loads(metadata.stdout)["target_directory"])
    executable_name = "build-script-build.exe" if os.name == "nt" else "build-script-build"
    candidates = list(target.glob(f"*/build/usher-i18n-*/{executable_name}"))
    candidates.extend(target.glob(f"*/*/build/usher-i18n-*/{executable_name}"))
    candidates = [path for path in candidates if path.is_file()]
    if not candidates:
        raise AssertionError("Run cargo check -p usher-i18n --locked before the i18n path regression")
    return max(candidates, key=lambda path: path.stat().st_mtime_ns).resolve()


class I18nBuildPathTests(unittest.TestCase):
    def test_reused_executable_reads_current_checkout_and_watches_its_inputs(self):
        executable = build_script_executable()
        with tempfile.TemporaryDirectory(prefix="usher i18n paths ") as directory:
            scratch = Path(directory)
            outputs = []
            for name, source, translation, visible, visible_translation in [
                ("checkout alpha", "Quit alpha", "退出甲", "Visible alpha", "界面甲"),
                ("checkout beta", "Quit beta", "退出乙", "Visible beta", "界面乙"),
            ]:
                repository = scratch / name
                manifest_dir = repository / "crates/usher-i18n"
                manifest_dir.mkdir(parents=True)
                i18n = repository / "i18n"
                po_dir = i18n / "locales/zh-CN/LC_MESSAGES"
                po_dir.mkdir(parents=True)
                (i18n / "manifest.toml").write_text(
                    'source_locale = "en"\nfallback_locale = "en"\n'
                    '\n[[locales]]\nid = "en"\nnative_name = "English"\n'
                    'minimum_coverage = 1.0\n'
                    '\n[[locales]]\nid = "zh-CN"\nnative_name = "简体中文"\n'
                    'minimum_coverage = 1.0\n',
                    encoding="utf-8",
                )
                (i18n / "messages.toml").write_text(
                    f'[[messages]]\nkey = "fixture.visible"\ntext = {json.dumps(visible)}\n',
                    encoding="utf-8",
                )
                (i18n / "rust-messages.toml").write_text(
                    f'[[messages]]\nkey = "tray.quit"\ntext = {json.dumps(source)}\nslint = false\n',
                    encoding="utf-8",
                )
                (po_dir / "usher.po").write_text(
                    'msgid ""\nmsgstr ""\n'
                    '"Language: zh-CN\\n"\n'
                    '"Content-Type: text/plain; charset=UTF-8\\n"\n'
                    '\nmsgctxt "fixture.visible"\n'
                    f'msgid {json.dumps(visible)}\n'
                    f'msgstr {json.dumps(visible_translation, ensure_ascii=False)}\n',
                    encoding="utf-8",
                )
                (po_dir / "usher-rust.po").write_text(
                    'msgid ""\nmsgstr ""\n'
                    '"Language: zh-CN\\n"\n'
                    '"Content-Type: text/plain; charset=UTF-8\\n"\n'
                    '\nmsgctxt "tray.quit"\n'
                    f'msgid {json.dumps(source)}\n'
                    f'msgstr {json.dumps(translation, ensure_ascii=False)}\n',
                    encoding="utf-8",
                )
                out_dir = scratch / f"out {name}"
                out_dir.mkdir()
                result = subprocess.run(
                    [str(executable)],
                    cwd=scratch,
                    env=dict(os.environ, CARGO_MANIFEST_DIR=str(manifest_dir), OUT_DIR=str(out_dir)),
                    capture_output=True,
                    text=True,
                    timeout=30,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                catalog = (out_dir / "catalogs.rs").read_text(encoding="utf-8")
                for key, expected in [
                    ("tray.quit", source),
                    ("tray.quit", translation),
                    ("fixture.visible", visible),
                    ("fixture.visible", visible_translation),
                ]:
                    expected_entry = f'CatalogEntry {{ key: "{key}", value: "{expected}" }}'
                    self.assertTrue(
                        expected_entry in catalog,
                        f"{executable} did not read {expected!r} from {manifest_dir}",
                    )
                # Watch the current inputs so later changes rebuild this catalog.
                watched = [
                    Path(line.split("=", 1)[1]).resolve()
                    for line in result.stdout.splitlines()
                    if line.startswith("cargo:rerun-if-changed=")
                ]
                self.assertEqual(
                    set(watched),
                    {
                        (i18n / "manifest.toml").resolve(),
                        (i18n / "messages.toml").resolve(),
                        (i18n / "rust-messages.toml").resolve(),
                        (po_dir / "usher.po").resolve(),
                        (po_dir / "usher-rust.po").resolve(),
                    },
                )
                outputs.append(catalog)
            self.assertNotEqual(outputs[0], outputs[1])


PR_BOUNDARY_SOURCES = [
    "Cargo.toml", "crates/usher-ui-pr-review/Cargo.toml", "crates/usher-ui-pr-review/src/lib.rs",
    "src/lib.rs", "src/workflow/pr_review/ui.rs", "ui/features/pr_review/factory.slint",
    "ui/features/pr_review/factory_host.slint", "ui/features/pr_review/integrated.slint",
]


def validate_pr_review_feature_ownership(sources):
    """Static ownership contract; full feature compilation remains a separate check."""
    root = tomllib.loads(sources["Cargo.toml"])
    feature = tomllib.loads(sources["crates/usher-ui-pr-review/Cargo.toml"])
    for name in ["renderer-femtovg", "renderer-femtovg-wgpu", "renderer-skia"]:
        assert f"usher-ui-pr-review/{name}" in root["features"][name], "renderer forwarding"
    assert "usher-ui-pr-review/integrated-preview" in root["features"]["live-preview"], "preview forwarding"
    assert {"usher-ui/dev-shell", "usher-ui-pr-review/dev-shell"}.issubset(root["features"]["dev-shell"]), "dev-shell forwarding"
    assert feature["features"]["integrated-preview"] == [], "preview must not build a second static tree"
    assert "usher-i18n-build" in feature["build-dependencies"], "catalog-free build helper"
    assert "usher-i18n" not in feature["build-dependencies"], "Rust-only catalog is not a Slint build dependency"
    assert re.search(r'#\[cfg\(not\(feature = "integrated-preview"\)\)\]\s*slint::include_modules!\(\);',
                     sources["crates/usher-ui-pr-review/src/lib.rs"]), "static generated-code guard"
    access = sources["src/workflow/pr_review/ui.rs"]
    assert re.search(r'#\[cfg\(not\(feature = "live-preview"\)\)\]\s*pub type PrReviewUiRoot = usher_ui_pr_review::PrReviewFactory;', access), "static root type"
    assert re.search(r'#\[cfg\(feature = "live-preview"\)\]\s*pub type PrReviewUiRoot = crate::MainWindow;', access), "integrated root type"
    canonical = set()
    for group in re.findall(r'export\s*\{([^}]+)\}', sources["ui/features/pr_review/factory.slint"]):
        canonical.update(name.strip() for name in group.split(",") if name.strip().startswith(("WorkflowPr", "PrConflict")) and not name.strip().endswith("Bridge"))
    assert "WorkflowPrDiffVirtualRow" in canonical, "nested model row identity"
    rust_export = re.search(r'#\[cfg\(not\(feature = "live-preview"\)\)\]\s*pub use usher_ui_pr_review::\{([^}]+)\};', sources["src/lib.rs"])
    assert rust_export, "static canonical Rust exports"
    assert canonical.issubset({name.strip() for name in rust_export[1].split(",")}), "static DTO identity"
    for path in ["ui/features/pr_review/integrated.slint", "ui/features/pr_review/factory_host.slint"]:
        exported = set()
        for group in re.findall(r'export\s*\{([^}]+)\}', sources[path]):
            exported.update(name.strip() for name in group.split(","))
        assert canonical.issubset(exported), "feature entry DTO contract"
    integrated = sources["ui/features/pr_review/integrated.slint"]
    assert 'export { PrReviewBridge } from "bridge.slint";' in integrated, "integrated actual state owner"
    assert "PrReviewSurface" in integrated, "integrated page owner"
    static = sources["ui/features/pr_review/factory_host.slint"]
    assert "PrReviewSurface" not in static and "pr_review_workspace.slint" not in static, "static shell excludes the body"


class PrReviewI18nFeatureOwnershipTests(unittest.TestCase):
    def setUp(self):
        self.sources = {path: (ROOT / path).read_text(encoding="utf-8") for path in PR_BOUNDARY_SOURCES}

    def test_static_preview_and_dev_shell_keep_one_canonical_model_owner(self):
        validate_pr_review_feature_ownership(self.sources)

    def test_feature_and_nested_dto_drift_are_rejected(self):
        mutations = [
            ("Cargo.toml", '"usher-ui-pr-review/integrated-preview", ', "", "preview forwarding"),
            ("Cargo.toml", '"usher-ui-pr-review/dev-shell"', '"usher-ui/dev-shell"', "dev-shell forwarding"),
            ("src/workflow/pr_review/ui.rs", 'pub type PrReviewUiRoot = crate::MainWindow;',
             'pub type PrReviewUiRoot = usher_ui_pr_review::PrReviewFactory;', "integrated root type"),
            ("crates/usher-ui-pr-review/src/lib.rs", '#[cfg(not(feature = "integrated-preview"))]',
             '#[cfg(feature = "integrated-preview")]', "static generated-code guard"),
            ("ui/features/pr_review/integrated.slint", 'WorkflowPrDiffVirtualRow, ', "", "feature entry DTO contract"),
            ("src/lib.rs", 'WorkflowPrDiffVirtualRow, ', "", "static DTO identity"),
        ]
        for path, before, after, reason in mutations:
            with self.subTest(path=path, reason=reason):
                candidate = copy.copy(self.sources)
                self.assertIn(before, candidate[path])
                candidate[path] = candidate[path].replace(before, after, 1)
                with self.assertRaisesRegex(AssertionError, reason):
                    validate_pr_review_feature_ownership(candidate)


if __name__ == "__main__":
    unittest.main()
