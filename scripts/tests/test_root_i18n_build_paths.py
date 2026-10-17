"""Reuse the root build script across two checkout paths without invoking Cargo.

Run ``cargo check --features ui-components --locked`` first, or select already
compiled build scripts with USHER_TEST_ROOT_BUILD_SCRIPT and
USHER_TEST_UI_BUILD_SCRIPTS (a JSON package-to-executable map). The fixtures
compile only tiny Slint documents in scratch paths; they never invoke Cargo or rustc.
"""

import hashlib
import json
import re
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


def root_build_script_executable():
    explicit = os.environ.get("USHER_TEST_ROOT_BUILD_SCRIPT")
    if explicit:
        executable = Path(explicit).resolve()
        if not executable.is_file():
            raise AssertionError(f"root build script does not exist: {executable}")
        return executable
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target.is_absolute():
        target = ROOT / target
    executable_name = "build-script-build.exe" if os.name == "nt" else "build-script-build"
    candidates = list(target.glob(f"*/build/usher-*/{executable_name}"))
    candidates.extend(target.glob(f"*/*/build/usher-*/{executable_name}"))
    candidates = [path for path in candidates if path.parent.name.removeprefix("usher-").isalnum()]
    if not candidates:
        raise AssertionError("Build the root package before the root i18n path regression")
    return max(candidates, key=lambda path: path.stat().st_mtime_ns).resolve()


class RootI18nBuildPathTests(unittest.TestCase):
    def write_fixture(self, repository, locale_id, native_name, source, translation, rust_source):
        i18n = repository / "i18n"
        po_dir = i18n / "locales" / locale_id / "LC_MESSAGES"
        po_dir.mkdir(parents=True, exist_ok=True)
        (i18n / "manifest.toml").write_text(
            'source_locale = "en"\nfallback_locale = "en"\n'
            '\n[[locales]]\nid = "en"\nnative_name = "English"\n'
            'minimum_coverage = 1.0\n'
            f'\n[[locales]]\nid = "{locale_id}"\nnative_name = "{native_name}"\n'
            'minimum_coverage = 1.0\n',
            encoding="utf-8",
        )
        for registry, domain, key, value, translated, slint in [
            ("messages.toml", "usher.po", "fixture.visible", source, translation, True),
            ("rust-messages.toml", "usher-rust.po", "tray.quit", rust_source, rust_source, False),
        ]:
            (i18n / registry).write_text(
                f'[[messages]]\nkey = "{key}"\ntext = {json.dumps(value)}\n'
                + ("" if slint else "slint = false\n"),
                encoding="utf-8",
            )
            (po_dir / domain).write_text(
                'msgid ""\nmsgstr ""\n'
                f'"Language: {locale_id}\\n"\n'
                '"Content-Type: text/plain; charset=UTF-8\\n"\n'
                f'\nmsgctxt "{key}"\n'
                f'msgid {json.dumps(value)}\n'
                f'msgstr {json.dumps(translated, ensure_ascii=False)}\n',
                encoding="utf-8",
            )

    def run_script(self, executable, scratch, repository, locale_id):
        result = subprocess.run(
            [str(executable)],
            cwd=scratch,
            env=dict(os.environ, CARGO_MANIFEST_DIR=str(repository)),
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        watched = set()
        for line in result.stdout.splitlines():
            if line.startswith("cargo:rerun-if-changed="):
                path = Path(line.split("=", 1)[1])
                watched.add((path if path.is_absolute() else repository / path).resolve())
        i18n = repository / "i18n"
        self.assertEqual(
            watched - {(repository / "build.rs").resolve()},
            {
                (i18n / "manifest.toml").resolve(),
                (i18n / "messages.toml").resolve(),
                (i18n / "locales" / locale_id / "LC_MESSAGES/usher.po").resolve(),
            },
            "inline Slint bundles must watch UI catalogs without Rust-only files or parent directories",
        )
        generated = repository / "target/slint-macro-tests/usher-i18n"
        expected = f"cargo:rustc-env=SLINT_BUNDLE_TRANSLATIONS={generated / 'translations'}"
        self.assertIn(expected, result.stdout.splitlines())
        self.assertTrue((generated / "i18n.slint").is_file())
        self.assertTrue(
            list((generated / "translations").glob("*/LC_MESSAGES/usher.po")),
            "the macro path must contain actual locale catalogs",
        )
        return {
            path.relative_to(generated): path.read_bytes()
            for path in generated.rglob("*")
            if path.is_file()
        }

    def test_reused_root_executable_publishes_bundle_in_current_checkout(self):
        executable = root_build_script_executable()
        with tempfile.TemporaryDirectory(prefix="usher root i18n paths ") as directory:
            scratch = Path(directory)
            bundles = []
            for name, locale_id, native_name, source, translation in [
                ("checkout alpha", "zh-CN", "简体中文", "Visible alpha", "界面甲"),
                ("checkout beta", "fr-FR", "Français", "Visible beta", "Interface bêta"),
            ]:
                repository = scratch / name
                self.write_fixture(
                    repository, locale_id, native_name, source, translation, "Quit original"
                )
                bundle = self.run_script(executable, scratch, repository, locale_id)
                self.assertIn(source, bundle[Path("i18n.slint")].decode("utf-8"))
                po = bundle[Path("translations") / locale_id / "LC_MESSAGES/usher.po"].decode("utf-8")
                self.assertIn(translation, po)
                self.assertNotIn("tray.quit", po)

                # A forced re-execution also preserves bundle bytes when only
                # the separate Rust registry and PO catalog change.
                self.write_fixture(
                    repository, locale_id, native_name, source, translation, "Quit changed"
                )
                self.assertEqual(
                    self.run_script(executable, scratch, repository, locale_id), bundle
                )
                bundles.append(bundle)
            self.assertNotEqual(bundles[0], bundles[1])


STATIC_UI_ROOTS = {
    "usher-ui": ("ui/main.slint", "MainWindow"),
    "usher-ui-settings": ("ui/views/settings.slint", "GitHubSettingsPage"),
    "usher-messages": ("ui/views/messages.slint", "GitHubMessagesPage"),
    "usher-ui-components": ("ui/views/ui_components_factory.slint", "GitHubUiComponentsWindow"),
    "usher-ui-pr-review": ("ui/features/pr_review/factory.slint", "PrReviewFactory"),
}


def static_ui_build_script_executable(package):
    explicit = json.loads(os.environ.get("USHER_TEST_UI_BUILD_SCRIPTS", "{}"))
    if package in explicit:
        executable = Path(explicit[package]).resolve()
        if not executable.is_file():
            raise AssertionError(f"UI build script does not exist: {executable}")
        return executable
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target.is_absolute():
        target = ROOT / target
    filename = "build-script-build.exe" if os.name == "nt" else "build-script-build"
    candidates = list(target.glob(f"*/build/{package}-*/{filename}"))
    candidates.extend(target.glob(f"*/*/build/{package}-*/{filename}"))
    candidates = [path for path in candidates if path.parent.name.removeprefix(f"{package}-").isalnum()]
    if not candidates:
        raise AssertionError(f"Build {package} before the static-root i18n regression")
    return max(candidates, key=lambda path: path.stat().st_mtime_ns).resolve()


class StaticRootI18nBuildPathTests(unittest.TestCase):
    def write_fixture(self, repository, source="Visible source"):
        i18n = repository / "i18n"
        i18n.mkdir(parents=True, exist_ok=True)
        # Creation order and manifest order differ from lexical directory order.
        # Every independently compiled root must still produce identical indexes.
        locales = [("zh-CN", "简体中文", "界面译文"), ("fr-FR", "Français", "Interface traduite")]
        manifest = 'source_locale = "en"\nfallback_locale = "en"\n'
        for locale, native in [("en", "English"), *[(a, b) for a, b, _ in locales]]:
            manifest += f'\n[[locales]]\nid = "{locale}"\nnative_name = "{native}"\nminimum_coverage = 1.0\n'
        (i18n / "manifest.toml").write_text(manifest, encoding="utf-8")
        # Two visible entries ensure each root retains the complete catalog,
        # including copy that the tiny fixture never paints.
        (i18n / "messages.toml").write_text(
            f'[[messages]]\nkey = "fixture.visible"\ntext = {json.dumps(source)}\n'
            '\n[[messages]]\nkey = "fixture.unpainted"\ntext = "Unpainted source"\n', encoding="utf-8",
        )
        (i18n / "rust-messages.toml").write_text(
            '[[messages]]\nkey = "fixture.rust"\ntext = "Rust original"\nslint = false\n', encoding="utf-8",
        )
        for locale, _, translated in locales:
            directory = i18n / "locales" / locale / "LC_MESSAGES"
            directory.mkdir(parents=True, exist_ok=True)
            header = f'msgid ""\nmsgstr ""\n"Language: {locale}\\n"\n"Content-Type: text/plain; charset=UTF-8\\n"\n'
            (directory / "usher.po").write_text(
                header + '\nmsgctxt "fixture.visible"\n' + f'msgid {json.dumps(source)}\n'
                + f'msgstr {json.dumps(translated, ensure_ascii=False)}\n'
                + '\nmsgctxt "fixture.unpainted"\nmsgid "Unpainted source"\n'
                + f'msgstr {json.dumps(translated + " unused", ensure_ascii=False)}\n', encoding="utf-8",
            )
            (directory / "usher-rust.po").write_text(
                header + '\nmsgctxt "fixture.rust"\nmsgid "Rust original"\nmsgstr "Rust translated"\n', encoding="utf-8",
            )
        material = repository / "material-1.18.1/material.slint"
        material.parent.mkdir(parents=True, exist_ok=True)
        material.write_text("export component FixtureMaterial inherits Rectangle {}\n", encoding="utf-8")
        for package, (entry, component) in STATIC_UI_ROOTS.items():
            (repository / "crates" / package).mkdir(parents=True, exist_ok=True)
            path = repository / entry
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                'import { I18n } from "@i18n";\n'
                'export global AppState { out property <string> i18n-catalog-sentinel: I18n.fixture-visible; }\n'
                f'export component {component} inherits Window {{ Text {{ text: I18n.fixture-visible; }} }}\n',
                encoding="utf-8",
            )
        return locales

    def run_script(self, package, executable, repository, out_dir):
        out_dir.mkdir(parents=True, exist_ok=True)
        environment = {key: value for key, value in os.environ.items() if not key.startswith("CARGO_FEATURE_")}
        environment.pop("SLINT_LIVE_PREVIEW", None)
        environment.update(CARGO_MANIFEST_DIR=str(repository / "crates" / package),
                           CARGO_PKG_NAME=package, OUT_DIR=str(out_dir), PROFILE="release")
        result = subprocess.run([str(executable)], cwd=repository.parent, env=environment,
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        watched = {Path(line.split("=", 1)[1]).resolve() for line in result.stdout.splitlines()
                   if line.startswith("cargo:rerun-if-changed=")}
        i18n = repository / "i18n"
        required = {i18n / "manifest.toml", i18n / "messages.toml"}
        required.update(i18n.glob("locales/*/LC_MESSAGES/usher.po"))
        self.assertTrue({path.resolve() for path in required}.issubset(watched))
        self.assertTrue((repository / STATIC_UI_ROOTS[package][0]).resolve() in watched)
        self.assertFalse(any(path.name in {"rust-messages.toml", "usher-rust.po"} for path in watched))
        self.assertFalse(i18n.resolve() in watched, "a parent-directory watch includes Rust-only catalogs")
        self.assertFalse(any(path.is_relative_to(out_dir.resolve()) for path in watched),
                         "generated bundles must not become build-script inputs")
        generated = out_dir / "usher-i18n"
        source = (generated / "i18n.slint").read_bytes()
        self.assertIn(b"fixture-unpainted", source)
        catalogs = {path.relative_to(generated / "translations").parts[0]: path.read_bytes()
                    for path in (generated / "translations").glob(f"*/LC_MESSAGES/{package}.po")}
        self.assertEqual(set(catalogs), {"zh-CN", "fr-FR"})
        for catalog in catalogs.values():
            self.assertIn(b"fixture.unpainted", catalog)
            self.assertNotIn(b"fixture.rust", catalog)
        include = next(line.split("=", 2)[2] for line in result.stdout.splitlines()
                       if line.startswith("cargo:rustc-env=SLINT_INCLUDE_GENERATED="))
        rust = Path(include).read_bytes()
        languages = re.findall(r'language\s*:\s*"([^"\n]*)"', rust.decode("utf-8"))
        self.assertEqual(set(languages), {"", "zh-CN", "fr-FR"})
        return source, catalogs, languages, hashlib.sha256(rust).hexdigest()

    def test_all_five_static_roots_share_complete_bundle_and_language_order(self):
        executables = {package: static_ui_build_script_executable(package) for package in STATIC_UI_ROOTS}
        with tempfile.TemporaryDirectory(prefix="usher static i18n roots ") as directory:
            scratch = Path(directory)
            repository = scratch / "checkout with spaces"
            self.write_fixture(repository)
            before = {package: self.run_script(package, executable, repository, scratch / "out" / package)
                      for package, executable in executables.items()}
            expected = next(iter(before.values()))[:3]
            for package, result in before.items():
                self.assertEqual(result[:3], expected, f"{package} changed catalog content or language indexes")

            # Corrupt Rust-only inputs, not merely their values. Static roots
            # must neither read them nor include them in Cargo's watched closure.
            (repository / "i18n/rust-messages.toml").write_text("invalid = [", encoding="utf-8")
            for path in (repository / "i18n/locales").glob("*/LC_MESSAGES/usher-rust.po"):
                path.write_text('msgstr "unterminated', encoding="utf-8")
            for package, executable in executables.items():
                self.assertEqual(self.run_script(package, executable, repository, scratch / "out" / package),
                                 before[package], f"{package} consumed Rust-only catalog inputs")

            # Positive control: a Slint-visible source edit remains an input and
            # changes both the shared global and each root's generated program.
            self.write_fixture(repository, "Visible changed")
            for package, executable in executables.items():
                after = self.run_script(package, executable, repository, scratch / "out" / package)
                self.assertNotEqual(after[0], before[package][0])
                self.assertNotEqual(after[3], before[package][3])


if __name__ == "__main__":
    unittest.main()
