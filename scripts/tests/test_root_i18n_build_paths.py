"""Reuse the root build script across two checkout paths without invoking Cargo.

Run a normal root build first, or set USHER_TEST_ROOT_BUILD_SCRIPT to a specific
before/after executable. Fixtures only write generated bundles in scratch paths.
"""

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
    def test_reused_root_executable_publishes_bundle_in_current_checkout(self):
        executable = root_build_script_executable()
        with tempfile.TemporaryDirectory(prefix="usher root i18n paths ") as directory:
            scratch = Path(directory)
            bundles = []
            for name in ["checkout alpha", "checkout beta"]:
                repository = scratch / name
                repository.mkdir()
                result = subprocess.run(
                    [str(executable)],
                    cwd=scratch,
                    env=dict(os.environ, CARGO_MANIFEST_DIR=str(repository)),
                    capture_output=True,
                    text=True,
                    timeout=30,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                generated = repository / "target/slint-macro-tests/usher-i18n"
                expected = f"cargo:rustc-env=SLINT_BUNDLE_TRANSLATIONS={generated / 'translations'}"
                self.assertIn(expected, result.stdout.splitlines())
                self.assertTrue((generated / "i18n.slint").is_file())
                translations = list((generated / "translations").glob("*/LC_MESSAGES/usher.po"))
                self.assertTrue(translations, "the macro path must contain actual locale catalogs")
                bundles.append(generated)
            self.assertNotEqual(bundles[0], bundles[1])
            self.assertEqual(
                (bundles[0] / "i18n.slint").read_bytes(),
                (bundles[1] / "i18n.slint").read_bytes(),
            )


if __name__ == "__main__":
    unittest.main()
