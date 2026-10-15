"""Native fixture preparation/launch isolation, without launching any GUI."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PREPARE = ROOT / "scripts/prepare-messages-focus-native.py"


class MessageFocusFixtureTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="usher focus fixture ")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.binary = self.root / "fake Usher & $(not-a-command)"
        self.binary.write_text(
            f"#!{sys.executable}\nimport json, os, sys\n"
            "print(json.dumps({'argv':sys.argv,'environment':dict(os.environ)}), flush=True)\n"
        )
        self.binary.chmod(0o755)

    def prepare(self, output, *extra):
        return subprocess.run(
            [sys.executable, str(PREPARE), "--binary", str(self.binary),
             "--output", str(output), "--label", "test", *extra],
            capture_output=True, text=True, timeout=30,
        )

    def test_launcher_passes_paths_as_data_and_installs_isolation_before_exec(self):
        output = self.root / "bundle 中文 & $()"
        result = self.prepare(output, "--locale", "zh-CN", "--count", "41")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        manifest = json.loads(result.stdout)
        bundle = Path(manifest["bundle"])
        environment = dict(os.environ, USHER_MEMORY_BENCHMARK_RUN="old",
                           USHER_BENCHMARK_STATE_ROOT="/must-not-use",
                           USHER_TRAY_LOCALE_FIXTURE="1", SLINT_BACKEND="wrong")
        completed = subprocess.run(
            [str(bundle / "Contents/MacOS/launch")],
            env=environment, capture_output=True, timeout=30,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        observed = json.loads((output / "application.log").read_text())
        self.assertEqual(observed["argv"], [str(bundle / "Contents/MacOS/Usher")])
        expected = manifest["environment"]
        self.assertEqual(
            {key: value for key, value in observed["environment"].items() if key.startswith("USHER_")},
            {key: value for key, value in expected.items() if key.startswith("USHER_")},
        )
        self.assertEqual(observed["environment"]["SLINT_BACKEND"], "winit-femtovg")
        state = Path(manifest["state"])
        messages = json.loads((state / "messages/messages.json").read_text())
        self.assertEqual(len(messages), 41)
        self.assertEqual(len({message["id"] for message in messages}), 41)
        self.assertTrue(all(message["target"] is None for message in messages))
        self.assertIn('language = "zh-CN"', (state / "settings.toml").read_text())

    def test_invalid_budget_and_reused_output_are_rejected_without_overwriting(self):
        for count in ["0", "1001"]:
            output = self.root / f"invalid-{count}"
            self.assertNotEqual(self.prepare(output, "--count", count).returncode, 0)
            self.assertFalse(output.exists())
        output = self.root / "existing user fixture"
        output.mkdir()
        marker = output / "keep.txt"
        marker.write_text("keep")
        self.assertNotEqual(self.prepare(output).returncode, 0)
        self.assertEqual(marker.read_text(), "keep")
        self.assertEqual(list(output.iterdir()), [marker])


if __name__ == "__main__":
    unittest.main()
