"""Reuse the root build script across two checkout paths without invoking Cargo.

Run a normal root build first, or set USHER_TEST_ROOT_BUILD_SCRIPT to a specific
before/after executable. Fixtures only write generated bundles in scratch paths.
"""

import json
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


if __name__ == "__main__":
    unittest.main()
