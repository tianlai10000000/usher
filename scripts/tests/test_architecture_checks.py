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

    def test_existing_runtime_boundary_remains_blocking(self):
        self.write("src/feature.rs", "fn fixture() { let _ = Runtime::new(); }\n")
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("Tokio runtime creation escaped", result.stderr)


if __name__ == "__main__":
    unittest.main()
