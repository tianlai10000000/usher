"""Boundary fixtures for the lexical architecture gate, not a Rust parser."""

import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ArchitectureGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="usher architecture ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        baseline = os.environ.get("USHER_TEST_ARCHITECTURE_BASE_REF")
        if baseline:
            source = subprocess.check_output(
                ["git", "show", f"{baseline}:scripts/check-architecture-structure.sh"],
                cwd=ROOT,
                text=True,
            )
        else:
            source = (ROOT / "scripts/check-architecture-structure.sh").read_text()
        self.write("scripts/check-architecture-structure.sh", source)
        for path in set(re.findall(r"(?:line_count|rust_function_count|slint_declaration_count) ([\w/.-]+)", source)):
            self.write(path, "pub fn fixture() {}\n" if path.endswith(".rs") else
                       "callback fixture();\nin property <bool> fixture;\n")
        self.write("src/http/mod.rs", "pub fn fixture() {}\n")
        self.write("crates/fixture/src/lib.rs", "pub fn fixture() {}\n")

    def write(self, path, source):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(source)

    def check(self):
        return subprocess.run(
            ["sh", str(self.root / "scripts/check-architecture-structure.sh")],
            cwd="/tmp",
            capture_output=True,
            text=True,
            timeout=30,
        )

    def assert_accepted(self):
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_shared_http_owner_may_construct_clients(self):
        self.write("src/http/mod.rs", """use reqwest::Client as Transport;
fn fixture() {
    let _ = reqwest::Client::builder();
    let _ = Transport::new();
    let _ = reqwest::Client::default();
}
""")
        self.assert_accepted()

    def test_request_types_and_shared_builders_remain_allowed(self):
        self.write("src/feature.rs", """use reqwest::{Method, StatusCode, Url};
use reqwest::header::{HeaderMap, HeaderValue};
fn request() { let _ = crate::http::get("https://example.test"); }
""")
        self.assert_accepted()

    def test_unrelated_client_names_do_not_grant_http_ownership(self):
        self.write("src/feature.rs", """struct Client;
fn fixture() { let _ = another_crate::Client::new(); }
""")
        self.assert_accepted()

    def test_direct_http_shortcuts_and_constructors_are_rejected(self):
        cases = [
            "reqwest::get(ENDPOINT)",
            "reqwest::blocking::get(ENDPOINT)",
            "reqwest::Client::new()",
            "reqwest::Client::default()",
            "reqwest::Client::builder()",
            "reqwest::ClientBuilder::new()",
            "reqwest :: Client :: default()",
            "reqwest\n    ::Client\n    ::new()",
        ]
        for call in cases:
            with self.subTest(call=call):
                self.write("src/feature.rs", f"fn fixture() {{ let _ = {call}; }}\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("HTTP", result.stderr)
                self.assertIn("src/feature.rs", result.stderr)

    def test_imported_constructors_shortcuts_and_namespace_aliases_are_rejected(self):
        cases = [
            "use reqwest::Client;\nfn fixture() { let _ = Client::new(); }",
            "use reqwest::Client as Transport;\nfn fixture() { let _ = Transport::default(); }",
            "use reqwest::{\n    Method,\n    Client as Transport,\n};",
            "use reqwest::{Method, get as fetch};",
            "use reqwest::blocking::{Client as Transport};",
            "use reqwest::*;",
            "use reqwest::{self as transport, Method};",
            "use reqwest as transport;",
            "use {reqwest as transport};",
            "extern crate reqwest as transport;",
        ]
        for source in cases:
            with self.subTest(source=source):
                self.write("crates/fixture/src/lib.rs", source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("HTTP", result.stderr)
                self.assertIn("crates/fixture/src/lib.rs", result.stderr)

    def test_existing_structure_budget_remains_blocking(self):
        self.write("src/lib.rs", "pub fn fixture() {}\n" + "// fixture\n" * 1100)
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("src/lib.rs lines is 1101 (budget 1100)", result.stderr)

    def test_batch_six_original_limits_accept_boundary_and_reject_growth(self):
        # Keep these literal: deriving expected limits from the gate would let
        # a budget increase silently update both the implementation and test.
        cases = [
            ("src/ai_review/controller.rs", "AI Review controller lines", 3850, "// fixture\n"),
            ("ui/main.slint", "root Slint lines", 3000, "// fixture\n"),
            ("ui/main.slint", "root Slint callbacks", 140, "callback fixture();\n"),
            ("ui/main.slint", "root Slint properties", 410, "in property <bool> fixture;\n"),
            ("src/workflow/pr_review/controller/actions.rs", "PR Review remote actions lines", 580, "// fixture\n"),
        ]
        for path, label, limit, declaration in cases:
            with self.subTest(label=label):
                previous = (self.root / path).read_text()
                try:
                    self.write(path, declaration * limit)
                    self.assert_accepted()
                    self.write(path, declaration * (limit + 1))
                    result = self.check()
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn(f"{label} is {limit + 1} (budget {limit})", result.stderr)
                finally:
                    self.write(path, previous)

    def test_existing_runtime_boundary_remains_blocking(self):
        self.write("src/feature.rs", "fn fixture() { let _ = Runtime::new(); }\n")
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("Tokio runtime creation escaped", result.stderr)

    def test_extracted_owners_cannot_hide_structural_growth(self):
        cases = [
            ("src/ai_review/review_input.rs", "AI Review input lines", 330, "// fixture\n"),
            ("src/workflow/pr_review/controller/actions/presentation.rs", "PR Review action presentation lines", 460, "// fixture\n"),
            ("src/workflow/pr_review/controller/actions/remote.rs", "PR Review action remote lines", 310, "// fixture\n"),
            ("ui/features/project/bridge.slint", "Project bridge lines", 70, "// fixture\n"),
            ("ui/features/project/bridge.slint", "Project bridge callbacks", 9, "callback fixture();\n"),
            ("ui/features/project/bridge.slint", "Project bridge properties", 24, "in property <bool> fixture;\n"),
            ("ui/features/project/dialog.slint", "Project dialog host lines", 90, "// fixture\n"),
        ]
        for path, label, limit, declaration in cases:
            with self.subTest(label=label):
                target = self.root / path
                previous = target.read_text() if target.exists() else ""
                try:
                    self.write(path, declaration * limit)
                    self.assert_accepted()
                    self.write(path, declaration * (limit + 1))
                    result = self.check()
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn(f"{label} is {limit + 1} (budget {limit})", result.stderr)
                finally:
                    self.write(path, previous)

    def test_extracted_leaves_cannot_import_controller_or_ui(self):
        cases = [
            ("src/ai_review/review_input.rs", "use super::controller::ControllerInner;"),
            ("src/workflow/pr_review/controller/actions/presentation.rs", "use super::*;"),
            ("src/workflow/pr_review/controller/actions/remote.rs", "use crate::MainWindow;"),
            ("src/ai_review/review_input.rs", "use crate::MainWindow;"),
        ]
        for path, source in cases:
            with self.subTest(path=path, source=source):
                target = self.root / path
                previous = target.read_text() if target.exists() else ""
                try:
                    self.write(path, source + "\n")
                    result = self.check()
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn("Architecture structure:", result.stderr)
                    self.assertIn(path, result.stderr)
                finally:
                    self.write(path, previous)


if __name__ == "__main__":
    unittest.main()
