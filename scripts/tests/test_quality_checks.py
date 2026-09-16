"""Behavior tests for quality gates using isolated repository fixtures."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="usher-policy-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        (self.root / "crates").mkdir()
        (self.root / "ui").mkdir()
        self.write("ui/main.slint", "GitHubArtifactPreviewPanel {}\n")
        baseline = os.environ.get("USHER_TEST_UI_POLICY_BASE_REF")
        if baseline:
            source = subprocess.check_output(
                ["git", "show", f"{baseline}:scripts/check-ui-policy.sh"],
                cwd=ROOT,
                text=True,
            )
        else:
            source = (ROOT / "scripts/check-ui-policy.sh").read_text()
        self.write("scripts/check-ui-policy.sh", source)

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def check(self):
        return subprocess.run(
            ["sh", str(self.root / "scripts/check-ui-policy.sh")],
            cwd=self.temp.name,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_current_hook_factory_portal_is_accepted(self):
        path = "ui/components/settings/github_hook_tooltip.slint"
        self.write(path, (ROOT / path).read_text())
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_unregistered_sibling_portal_is_rejected(self):
        self.write(
            "ui/components/settings/github_hook_tooltip_copy.slint",
            "GitHubFloatingOverlayHost {}\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("github_hook_tooltip_copy.slint", result.stderr)

    def test_allowlisted_hook_path_still_rejects_builtin_tooltip(self):
        self.write("ui/components/settings/github_hook_tooltip.slint", "Tooltip {}\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("built-in/local Tooltip", result.stderr)

    def test_allowlisted_hook_path_still_rejects_private_material(self):
        self.write(
            "ui/components/settings/github_hook_tooltip.slint",
            'import { Tooltip } from "material-1.7.1-fixed/private.slint";\n',
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("private vendored Material", result.stderr)

    def test_factory_without_portal_is_rejected(self):
        self.write("crates/fixture/build.rs", '.as_library("ui/views/fixture.slint")\n')
        self.write("ui/views/fixture.slint", "Rectangle {}\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("independent factory root lacks a tooltip portal", result.stderr)

    def test_duplicate_preview_is_rejected(self):
        self.write("ui/main.slint", "GitHubArtifactPreviewPanel {}\n" * 2)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("exactly one Artifact Preview panel", result.stderr)


class QualityRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="usher quality ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        (self.root / "bin").mkdir()
        shutil.copy(ROOT / "scripts/check-quality.sh", self.root / "scripts")
        self.calls = self.root / "calls.txt"
        self.env = dict(os.environ, PATH=f"{self.root / 'bin'}:{os.environ['PATH']}",
                        USHER_TEST_CALLS=str(self.calls))
        for name in ["cargo", "python3", "git", "rg"]:
            self.stub(self.root / "bin" / name, name)
        for name in ["architecture-structure", "ui-policy", "i18n"]:
            self.stub(self.root / "scripts" / f"check-{name}.sh", name)

    def stub(self, path, label):
        path.write_text(
            "#!/bin/sh\n"
            f'call="{label} $*"\n'
            'printf "%s\\n" "$call" >> "$USHER_TEST_CALLS"\n'
            'if [ "$call" = "${USHER_TEST_FAIL:-}" ]; then exit 1; fi\n'
            'if [ "$call" = "${USHER_TEST_INTERRUPT:-}" ]; then exit 130; fi\n'
            'exit 0\n'
        )
        path.chmod(0o755)

    def run_checks(self):
        return subprocess.run(
            ["sh", str(self.root / "scripts/check-quality.sh")],
            cwd="/tmp",
            env=self.env,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_success_runs_all_checks_without_reformatting(self):
        result = self.run_checks()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = self.calls.read_text().splitlines()
        self.assertEqual(len(calls), 8)
        self.assertEqual(calls[0], "cargo fmt --all -- --check")
        self.assertIn("cargo check --locked", calls)
        self.assertIn("cargo test --lib --locked", calls)
        self.assertEqual(calls[-1], "git diff --check")
        self.assertIn("8 checks passed", result.stdout)

    def test_architecture_failure_is_preserved_and_later_tests_run(self):
        self.env["USHER_TEST_FAIL"] = "architecture-structure "
        result = self.run_checks()
        self.assertEqual(result.returncode, 1)
        self.assertIn("architecture(1)", result.stderr)
        self.assertIn("7 checks passed", result.stdout)
        calls = self.calls.read_text().splitlines()
        self.assertIn("cargo test --lib --locked", calls)
        self.assertEqual(calls[-1], "git diff --check")

    def test_interruption_does_not_start_more_checks(self):
        self.env["USHER_TEST_INTERRUPT"] = "cargo fmt --all -- --check"
        result = self.run_checks()
        self.assertEqual(result.returncode, 130)
        self.assertEqual(len(self.calls.read_text().splitlines()), 1)


if __name__ == "__main__":
    unittest.main()
