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
        for owner in ("usher-ui", "usher-ui-settings", "usher-messages", "usher-ui-components", "usher-ui-pr-review", "usher-i18n-build", "usher-ui-build"):
            self.write(f"crates/{owner}/Cargo.toml", '[package]\nname = "fixture"\n')
        for owner in ("usher-ui", "usher-ui-settings", "usher-messages", "usher-ui-components", "usher-ui-pr-review"):
            self.write(f"crates/{owner}/build.rs", "fn main() {}\n")

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

    def test_pr_factory_extracted_owners_keep_independent_size_limits(self):
        owners = {
            "src/workflow/pr_review/ui.rs": 120,
            "src/workflow/pr_review/ui/context.rs": 170,
            "src/workflow/pr_review/ui/input.rs": 120,
            "src/workflow/pr_review/controller/presentation.rs": 550,
            "src/workflow/pr_review/controller/actions/reviews.rs": 160,
            "src/workflow/pr_review/controller/highlighting.rs": 260,
            "src/workflow/pr_review/controller/highlighting_tests.rs": 280,
            "src/workflow/pr_review/controller/highlight_session.rs": 260,
            "src/workflow/pr_review/controller/highlight_cache.rs": 260,
            "src/workflow/pr_review/controller/highlight_cache_tests.rs": 280,
            "ui/features/pr_review/bridge.slint": 190,
            "ui/features/pr_review/host_bridge.slint": 110,
            "ui/features/pr_review/surface.slint": 280,
            "ui/features/pr_review/factory.slint": 80,
            "ui/features/pr_review/factory_host.slint": 40,
            "ui/features/pr_review/integrated.slint": 35,
            "crates/usher-ui-pr-review/build.rs": 80,
            "crates/usher-ui-pr-review/src/lib.rs": 30,
            "ui/components/pr_review/tooltip_state.slint": 80,
            "ui/components/pr_review/tooltip_anchors.slint": 230,
            "ui/components/pr_review/tooltip_buttons.slint": 160,
            "ui/components/pr_review/tooltip_overlay.slint": 330,
        }
        for path, limit in owners.items():
            self.write(path, "// bounded owner\n" * limit)
        self.assert_accepted()
        for path, limit in owners.items():
            with self.subTest(owner=path):
                self.write(path, "// bounded owner\n" * (limit + 1))
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(f"is {limit + 1} (budget {limit})", result.stderr)
                self.write(path, "// bounded owner\n" * limit)

    def test_pr_async_idle_ui_test_owner_has_an_independent_size_limit(self):
        path = "src/workflow/pr_review/controller/async_ui_tests.rs"
        self.write(path, "// async UI control\n" * 220)
        self.assert_accepted()
        self.write(path, "// async UI control\n" * 221)
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("async idle UI test owner lines is 221 (budget 220)", result.stderr)

    def test_pr_highlighting_accepts_only_bounded_compute_ownership(self):
        path = "src/workflow/pr_review/controller/highlighting.rs"
        self.write(path, "use crate::app_runtime::latest_compute::LatestCompute;\n")
        self.assert_accepted()
        for source in ("use tokio as runtime;", "spawn_blocking(work);", "use std::thread;",
                       "use reqwest::Client;", "use usher_state_store::LocalStateStore;"):
            with self.subTest(source=source):
                self.write(path, source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("highlighting bypasses bounded compute ownership", result.stderr)

    def test_pr_grammar_owner_has_no_io_runtime_or_store_dependencies(self):
        for path in ("src/workflow/pr_review/controller/highlight_session.rs",
                     "src/workflow/pr_review/controller/highlight_cache.rs"):
            self.write(path, "use std::sync::{Arc, Mutex, OnceLock};\n")
            self.assert_accepted()
            for source in ("use std::fs;", "use std::thread;", "use tokio as runtime;",
                           "spawn_blocking(work);", "use usher_state_store::LocalStateStore;"):
                with self.subTest(owner=path, source=source):
                    self.write(path, source + "\n")
                    result = self.check()
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn("grammar ownership contains I/O, runtime or Store work", result.stderr)
            self.write(path, "use std::sync::{Arc, Mutex, OnceLock};\n")

    def test_pr_review_modal_shortcut_regression_owner_remains_bounded(self):
        path = "src/shortcuts/pr_review_modal_tests.rs"
        self.write(path, "// modal shortcut fixture\n" * 90)
        self.assert_accepted()
        self.write(path, "// modal shortcut fixture\n" * 91)
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("PR Review modal shortcut regression lines is 91 (budget 90)", result.stderr)

    def test_pr_ui_adapters_accept_event_loop_models_and_captured_action_facade(self):
        self.write("src/workflow/pr_review/ui/context.rs", """use slint::{ComponentHandle, ModelRc};
fn sync() { page.global::<crate::PrReviewBridge>().set_workflow_pr_review_files(rows); }
""")
        self.write("src/workflow/pr_review/controller/actions/reviews.rs", """use super::*;
fn submit() { self.run_action(ui, context.submit_review(action, body), completed); }
""")
        self.write("ui/features/pr_review/host_bridge.slint", """export global PrReviewHostBridge {
    in property <string> review-identity;
    in property <component-factory> factory;
    callback open-review(string);
}
""")
        self.assert_accepted()

    def test_pr_event_loop_owners_reject_io_scheduling_clients_and_stores(self):
        for owner in ("src/workflow/pr_review/ui.rs", "src/workflow/pr_review/ui/context.rs",
                      "src/workflow/pr_review/ui/input.rs", "src/workflow/pr_review/controller/presentation.rs"):
            for source in ("use std :: fs;", "use std::{sync::Arc, process as worker};",
                           "use tokio as runtime;", "use crate::app_runtime;",
                           "use usher_state_store::PersistentStore;", "use crate::settings::SettingsManager;"):
                with self.subTest(owner=owner, source=source):
                    self.write(owner, source + "\n")
                    result = self.check()
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn("PR Review factory UI owns I/O", result.stderr)
                    self.assertIn(owner, result.stderr)
            self.write(owner, "// event-loop owner\n")

    def test_pr_shell_does_not_duplicate_models_or_reverse_import_the_root(self):
        cases = {
            "ui/features/pr_review/host_bridge.slint": "in property <[WorkflowPrDiffLine]> lines;\n",
            "ui/features/pr_review/bridge.slint": 'import { MainWindow } from "../../main.slint";\n',
            "crates/usher-ui-pr-review/src/lib.rs": "use std::process::Command;\n",
            "src/workflow/pr_review/controller/actions/reviews.rs": "use reqwest::Url;\n",
        }
        for owner, source in cases.items():
            with self.subTest(owner=owner):
                previous = (self.root / owner).read_text()
                self.write(owner, source)
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("PR Review factory UI owns I/O", result.stderr)
                self.assertIn(owner, result.stderr)
                self.write(owner, previous)

    def test_pr_package_uses_the_shared_catalog_free_public_build_helper(self):
        self.write("crates/usher-ui-pr-review/build.rs", "fn main() { slint_build::compile(root); }\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("public compiler/source-input boundary", result.stderr)
        self.write("crates/usher-ui-pr-review/build.rs", "fn main() { usher_ui_build::compile(root); }\n")
        self.write("crates/usher-ui-pr-review/Cargo.toml", '[dependencies]\nusher-i18n = { path = "../usher-i18n" }\n')
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("embedded/runtime catalogs", result.stderr)

    def test_pr_generated_package_dependencies_cannot_grow_runtime_infrastructure(self):
        path = "crates/usher-ui-pr-review/Cargo.toml"
        self.write(path, (ROOT / path).read_text())
        self.assert_accepted()
        for addition in ('\n[dev-dependencies]\nreqwest = "1"\n',
                         '\n[target.unix.dependencies]\nworkers = { package = "tokio", version = "1" }\n',
                         '\n[dependencies]\ntransport = { package = "reqwest", version = "1" }\n',
                         '\n[dependencies]\nslint = { package = "reqwest", version = "1" }\n'):
            with self.subTest(addition=addition):
                self.write(path, (ROOT / path).read_text() + addition)
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("PR Review factory UI owns I/O", result.stderr)

    def test_window_navigation_accepts_shared_presentation_dependencies(self):
        path = "ui/components/navigation/github_window_navigation.slint"
        for source in ((ROOT / path).read_text(), "// No declarations is a valid count of zero.\n"):
            with self.subTest(source=source[:60]):
                self.write(path, source)
                result = self.check()
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(result.stderr, "")

    def test_window_navigation_rejects_feature_and_shell_dependencies(self):
        path = "ui/components/navigation/github_window_navigation.slint"
        for dependency in ("../../main.slint", "../../features/project/bridge.slint"):
            with self.subTest(dependency=dependency):
                self.write(path, f'import {{ Internal }} from "{dependency}";\n')
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("window navigation depends on shell or feature internals", result.stderr)
                self.assertNotIn("integer expression expected", result.stderr)

    def test_window_navigation_budget_remains_blocking(self):
        self.write("ui/components/navigation/github_window_navigation.slint", "// fixture\n" * 181)
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("Window navigation UI lines is 181 (budget 180)", result.stderr)
        self.assertNotIn("integer expression expected", result.stderr)

    def test_i18n_build_boundary_accepts_catalog_free_helper(self):
        self.write("crates/usher-ui/Cargo.toml", '[build-dependencies]\nusher-i18n-build = { path = "../usher-i18n-build" }\n')
        self.assert_accepted()

    def test_i18n_ui_runtime_dependency_and_embedded_catalog_are_rejected(self):
        cases = [
            ("crates/usher-ui/Cargo.toml", '[build-dependencies]\nusher-i18n = { path = "../usher-i18n" }\n'),
            ("crates/usher-ui-settings/Cargo.toml", '[build-dependencies]\ncatalog = { package = "usher-i18n", path = "../usher-i18n" }\n'),
            ("crates/usher-messages/Cargo.toml", '[build-dependencies]\nusher-i18n = "1"\n'),
            ("crates/usher-i18n-build/src/lib.rs", 'include_str!("catalogs.rs");\n'),
            ("crates/usher-i18n-build/build.rs", 'fn main() {}\n'),
            ("crates/usher-i18n-build/Cargo.toml", '[package]\nbuild = "generated.rs"\n'),
            ("crates/usher-i18n-build/Cargo.toml", '[dependencies]\nslint = "1"\n'),
        ]
        for path, source in cases:
            with self.subTest(path=path):
                target = self.root / path
                previous = target.read_text() if target.exists() else None
                self.write(path, source)
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("i18n UI build support", result.stderr)
                if previous is None:
                    target.unlink()
                else:
                    target.write_text(previous)

    def test_i18n_owner_budget_remains_blocking(self):
        self.write("crates/usher-i18n-build/src/lib.rs", "// fixture\n" * 131)
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("i18n build facade lines is 131 (budget 130)", result.stderr)

    def test_slint_build_integration_accepts_only_public_build_support(self):
        self.write("crates/usher-ui-build/Cargo.toml", '[dependencies]\nslint-build = { version = "=1.18.1" }\n')
        self.write("crates/usher-ui-build/src/lib.rs", 'slint_build::compile_with_output_path(input, output, config);\n')
        self.write("crates/usher-ui/build.rs", 'usher_ui_build::compile_with_config(input, config);\n')
        self.assert_accepted()

    def test_slint_build_integration_rejects_catalog_runtime_and_private_compiler_dependencies(self):
        cases = [
            ("crates/usher-ui-build/Cargo.toml", '[dependencies]\nusher-i18n-build = "1"\n'),
            ("crates/usher-ui-build/Cargo.toml", '[dependencies.catalog]\npackage = "usher-i18n"\n'),
            ("crates/usher-ui-build/Cargo.toml", '[dependencies]\nslint-build = { package = "i-slint-compiler", version = "1" }\n'),
            ("crates/usher-ui-build/Cargo.toml", '[target.unix.dependencies]\nslint = "1"\n'),
            ("crates/usher-ui-build/src/lib.rs", 'use i_slint_compiler::CompilerConfiguration;\n'),
            ("crates/usher-ui-build/src/lib.rs", 'use i_slint_compiler::{CompilerConfiguration};\n'),
            ("crates/usher-ui-build/src/lib.rs", 'use usher_i18n_build::LocaleSpec;\n'),
            ("crates/usher-ui-build/src/lib.rs", 'use slint :: { ComponentHandle };\n'),
            ("crates/usher-ui-build/build.rs", 'fn main() {}\n'),
            ("crates/usher-ui/build.rs", 'slint_build::compile_with_config(input, config);\n'),
            ("crates/usher-ui/build.rs", 'use slint_build::{compile_with_config as compile};\n'),
        ]
        for path, source in cases:
            with self.subTest(path=path, source=source):
                target = self.root / path
                previous = target.read_text() if target.exists() else None
                try:
                    self.write(path, source)
                    result = self.check()
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn("shared Slint build integration", result.stderr)
                finally:
                    if previous is None:
                        target.unlink()
                    else:
                        self.write(path, previous)

    def test_slint_build_integration_budgets_remain_blocking(self):
        for path, label, maximum in [
            ("crates/usher-ui-build/src/lib.rs", "Slint build integration lines", 150),
            ("crates/usher-ui-build/src/tests.rs", "Slint build integration tests lines", 160),
        ]:
            with self.subTest(path=path):
                previous = (self.root / path).read_text()
                try:
                    self.write(path, "// fixture\n" * maximum)
                    self.assert_accepted()
                    self.write(path, "// fixture\n" * (maximum + 1))
                    result = self.check()
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn(f"{label} is {maximum + 1} (budget {maximum})", result.stderr)
                finally:
                    self.write(path, previous)

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
            ("crates/usher-preview/src/file_source.rs", "Preview source reader lines", 145, "// fixture\n"),
            ("src/ai/settings_state.rs", "AI Settings state lines", 320, "// fixture\n"),
            ("src/ai/settings_ui.rs", "AI Settings UI lines", 220, "// fixture\n"),
            ("src/settings_page/ai_agent_drafts.rs", "AI provider draft selection lines", 180, "// fixture\n"),
            ("src/settings/ai_agents.rs", "AI Agent profile configuration lines", 380, "// fixture\n"),
            ("src/settings_page/ai_agent_projection.rs", "AI Agent Settings projection lines", 160, "// fixture\n"),
            ("src/ai/settings_ui/admission.rs", "AI Agent save admission lines", 110, "// fixture\n"),
            ("src/ai/identity.rs", "AI Agent identity lines", 130, "// fixture\n"),
            ("src/ai/request_headers.rs", "AI Agent request headers lines", 120, "// fixture\n"),
            ("src/settings_page/ai_agent_header_object.rs", "AI Agent Header object parser lines", 220, "// fixture\n"),
            ("src/settings_page/ai_agent_header_drafts.rs", "AI Agent Header draft modes lines", 220, "// fixture\n"),
            ("ui/components/settings/github_ai_agent_headers.slint", "AI Agent Header editor UI lines", 180, "// fixture\n"),
            ("src/ai/settings_state/actions.rs", "AI Agent registry actions lines", 140, "// fixture\n"),
            ("src/settings_page/ai_agent_list.rs", "AI Agent list projection lines", 120, "// fixture\n"),
            ("src/ai/settings_ui/delete_confirmation.rs", "AI Agent delete confirmation lines", 100, "// fixture\n"),
            ("src/ai/settings_ui/feedback.rs", "AI Agent save feedback lines", 70, "// fixture\n"),
            ("src/ai/settings_ui/connection_test.rs", "AI Agent connection probe lines", 100, "// fixture\n"),
            ("ui/components/navigation/github_fixed_tabs.slint", "Fixed Tabs UI lines", 240, "// fixture\n"),
            ("ui/components/settings/github_ai_agent_editor.slint", "AI Agent editor UI lines", 260, "// fixture\n"),
            ("ui/components/settings/github_ai_agent_list.slint", "AI Agent list UI lines", 160, "// fixture\n"),
            ("ui/components/settings/github_ai_agent_list_row.slint", "AI Agent list row UI lines", 140, "// fixture\n"),
            ("ui/components/settings/github_ai_agent_tooltip.slint", "AI Agent fixed Tooltip UI lines", 300, "// fixture\n"),
            ("ui/components/settings/github_ai_agent_header_tooltip.slint", "AI Agent Header Tooltip UI lines", 160, "// fixture\n"),
            ("ui/components/settings/github_ai_agent_delete.slint", "AI Agent delete UI lines", 60, "// fixture\n"),
            ("src/chat/request.rs", "Chat request capture lines", 130, "// fixture\n"),
            ("src/ai_review/request_capture.rs", "AI Review request capture lines", 100, "// fixture\n"),
            ("src/ai_review/agent_identity.rs", "AI Review Agent identity lines", 160, "// fixture\n"),
            ("src/message_center/commands.rs", "Message command worker lines", 260, "// fixture\n"),
            ("src/message_center/focus.rs", "Message factory focus bridge lines", 200, "// fixture\n"),
            ("src/ui_components/mod.rs", "Playground factory bridge lines", 200, "// fixture\n"),
            ("src/ui_components/feedback.rs", "Playground feedback adapter lines", 220, "// fixture\n"),
            ("src/ui_components/popover.rs", "Playground popover bridge lines", 100, "// fixture\n"),
            ("ui/views/ui_components_factory.slint", "Playground factory UI lines", 150, "// fixture\n"),
            ("src/focus_navigation.rs", "Shared factory tab navigation lines", 200, "// fixture\n"),
            ("src/ui_components/keyboard.rs", "Playground keyboard bridge lines", 80, "// fixture\n"),
            ("src/notification/ingress.rs", "Notification ingress lines", 370, "// fixture\n"),
            ("src/shutdown/pipeline.rs", "Shutdown pipeline lines", 250, "// fixture\n"),
            ("src/shutdown/pipeline/phase.rs", "Shutdown phase lines", 200, "// fixture\n"),
            ("src/diagnostics/archive.rs", "Diagnostics archive lines", 240, "// fixture\n"),
            ("src/update/download_status.rs", "Download status projection lines", 120, "// fixture\n"),
            ("src/workflow/state/facade.rs", "Workflow facade lines", 400, "// fixture\n"),
            ("src/workflow/state/reducer.rs", "Workflow reducer lines", 220, "// fixture\n"),
            ("src/workflow/state/equality.rs", "Workflow equality lines", 80, "// fixture\n"),
            ("src/workflow/tag/controller.rs", "Tag controller lines", 240, "// fixture\n"),
            ("src/workflow/tag/execution.rs", "Tag execution lines", 620, "// fixture\n"),
            ("src/workflow/tag/state.rs", "Tag state lines", 450, "// fixture\n"),
            ("src/workflow/tag/initialization.rs", "Tag initialization lines", 200, "// fixture\n"),
            ("src/workflow/tag/initialization_error.rs", "Tag initialization diagnostics lines", 160, "// fixture\n"),
            ("src/workflow/tag/progress.rs", "Tag progress lines", 130, "// fixture\n"),
            ("src/workflow/tag/activity.rs", "Tag activity lines", 90, "// fixture\n"),
            ("src/ai_review/producers.rs", "AI Review producers lines", 200, "// fixture\n"),
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

    def test_provider_draft_owner_accepts_event_loop_local_selection(self):
        self.write("src/settings_page/ai_agent_drafts.rs", """use std::{cell::RefCell, rc::Rc};
use slint::{ComponentHandle, Global};
use crate::settings::AiAgentProfile;
fn open_editor(saved: &AiAgentProfile) -> AiAgentProfile {
    saved.clone()
}
""")
        self.assert_accepted()

    def test_provider_draft_owner_rejects_io_workers_and_concrete_stores(self):
        cases = [
            "use std::fs;",
            "use std::process::Command;",
            "use std::thread;",
            "use std::{cell::RefCell, fs};",
            "use std :: process :: Command;",
            "use tokio::task;",
            "fn fixture() { tokio::spawn(async {}); }",
            "use reqwest::Url;",
            'fn fixture() { let _ = crate::http::get("https://example.test"); }',
            "use crate::{http as transport};",
            "use usher_state_store::LocalStateStore;",
            "use usher_state_store::PersistentStore;",
        ]
        path = "src/settings_page/ai_agent_drafts.rs"
        for source in cases:
            with self.subTest(source=source):
                self.write(path, source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("AI provider draft selection owns I/O, workers or concrete Stores", result.stderr)
                self.assertIn(path, result.stderr)

    def test_agent_profile_owners_allow_semantic_data_and_ui_capture(self):
        self.write("src/settings/ai_agents.rs", """use serde::{Deserialize, Serialize};
use crate::settings::AiAgentSettings;
fn normalize(settings: AiAgentSettings) -> AiAgentSettings { settings }
""")
        self.write("src/ai/identity.rs", """fn label() -> String {
    usher_i18n::translate("common.custom").into_owned()
}
""")
        self.write("src/ai/settings_ui/admission.rs", "use std::sync::{Arc, Mutex};\n")
        self.write("src/ai/settings_ui/feedback.rs", 'fn label() -> String { crate::i18n::tr("settings.feedback.ai_agent_saved").into_owned() }\n')
        for owner in ("src/settings_page/ai_agent_projection.rs",
                      "src/ai_review/request_capture.rs", "src/ai_review/agent_identity.rs"):
            self.write(owner, """use slint::{ComponentHandle, Global};
use crate::MainWindow;
fn capture() { let _ = crate::ai::settings_state::current_profile(); }
""")
        self.assert_accepted()

    def test_agent_profile_owners_reject_io_workers_and_concrete_stores(self):
        owners = [
            "src/settings/ai_agents.rs", "src/settings_page/ai_agent_projection.rs", "src/settings_page/ai_agent_list.rs",
            "src/ai/settings_ui/admission.rs", "src/ai/identity.rs",
            "src/ai/settings_state/actions.rs", "src/ai/settings_ui/delete_confirmation.rs", "src/ai/settings_ui/feedback.rs",
            "src/ai_review/request_capture.rs", "src/ai_review/agent_identity.rs",
        ]
        forbidden = [
            "use std::fs;", "use std::io;", "use std::net::TcpStream;",
            "use std::{sync::Arc, process};", "use std :: thread;",
            "use tokio::task;", "fn fixture() { tokio::spawn(async {}); }",
            "use reqwest::Url;", "use usher_state_store::LocalStateStore;",
            "use usher_state_store::PersistentStore;",
            'fn fixture() { let _ = crate::http::get("https://example.test"); }',
            "use crate::{http as transport};",
        ]
        for owner in owners:
            previous = (self.root / owner).read_text()
            try:
                for source in forbidden:
                    with self.subTest(owner=owner, source=source):
                        self.write(owner, source + "\n")
                        result = self.check()
                        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                        self.assertIn("AI Agent configuration, identity or capture owns I/O, workers or concrete Stores", result.stderr)
                        self.assertIn(owner, result.stderr)
            finally:
                self.write(owner, previous)

    def test_agent_probe_owners_preserve_size_budgets(self):
        for path, label, maximum in [
            ("src/ai/settings_ui/connection_probe.rs", "AI Agent shared probe lifecycle lines", 150),
            ("src/ai/settings_ui/list_connection_test.rs", "AI Agent list connection probe lines", 180),
        ]:
            with self.subTest(path=path):
                previous = (self.root / path).read_text()
                try:
                    self.write(path, "// fixture\n" * maximum)
                    self.assert_accepted()
                    self.write(path, "// fixture\n" * (maximum + 1))
                    result = self.check()
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn(f"{label} is {maximum + 1} (budget {maximum})", result.stderr)
                finally:
                    self.write(path, previous)

    def test_agent_probe_owners_keep_lifecycle_and_transport_boundaries(self):
        lifecycle = "src/ai/settings_ui/connection_probe.rs"
        listing = "src/ai/settings_ui/list_connection_test.rs"
        self.write(lifecycle, "use tokio::task::AbortHandle;\nuse crate::ai::agent::ConnectionTestResult;\n")
        self.write(listing, """use slint::{ComponentHandle, Global};
use usher_ui_settings::GitHubSettingsPage;
async fn request() { crate::ai::agent::test_connection(&saved.connection).await; }
fn run() { crate::app_runtime::shared_runtime().spawn(async {}); }
""")
        self.assert_accepted()
        for owner, message, forbidden in [
            (lifecycle, "shared probe lifecycle owns UI, transport, scheduling or I/O", [
                "use slint::Global;", "use usher_ui_settings::GitHubSettingsPage;",
                "fn run() { crate::app_runtime::shared_runtime(); }",
                "fn run() { tokio::spawn(async {}); }",
                "async fn run() { crate::ai::agent::test_connection(&settings).await; }",
            ]),
            (listing, "connection probe bypasses its transport or settings facade", [
                "use crate::MainWindow;", "use crate::settings_page::SettingsPageBridge;",
                "use std::net::TcpStream;", "use reqwest::Url;",
                "fn run() { crate::http::get(url); }",
                "use usher_state_store::PersistentStore;",
            ]),
        ]:
            previous = (self.root / owner).read_text()
            try:
                for source in forbidden:
                    with self.subTest(owner=owner, source=source):
                        self.write(owner, source + "\n")
                        result = self.check()
                        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                        self.assertIn(message, result.stderr)
                        self.assertIn(owner, result.stderr)
            finally:
                self.write(owner, previous)

    def test_chat_request_owner_allows_async_feature_transports_and_http_errors(self):
        self.write("src/chat/request.rs", """use crate::http::{HttpError, HttpErrorKind};
async fn request() -> Result<String, HttpError> {
    crate::ai::agent::complete(&settings, &messages, "AI Chat").await
}
async fn rag() { crate::chat::chat::send_message_to_server("question").await; }
""")
        self.assert_accepted()

    def test_header_preparation_and_registry_reducers_allow_only_semantic_work(self):
        self.write("src/ai/request_headers.rs", """use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use crate::http::HttpError;
fn prepare(value: &mut HeaderValue) { value.set_sensitive(true); }
""")
        self.write("src/ai/settings_state/actions.rs", """use usher_state_store::Reduction;
use crate::settings::AiAgentConfiguration;
fn reduce(state: &AiAgentConfiguration) { let _ = state.clone(); }
""")
        self.assert_accepted()

    def test_header_preparation_rejects_ui_network_scheduling_and_concrete_stores(self):
        owner = "src/ai/request_headers.rs"
        for source in ["use std::fs;", "use std::{sync::Arc, net};",
                       "use tokio::task;", "use crate::MainWindow;",
                       "use slint::ComponentHandle;", "use super::controller::ControllerInner;",
                       "use usher_state_store::PersistentStore;",
                       'fn fixture() { crate::http::get("https://example.test"); }',
                       'async fn fixture() { crate::ai::agent::complete(&a, &b, "fixture").await; }']:
            with self.subTest(source=source):
                self.write(owner, source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("AI Agent request headers owns UI, I/O, scheduling or concrete Stores", result.stderr)
                self.assertIn(owner, result.stderr)

    def test_header_tooltip_owner_has_an_independent_size_budget(self):
        owner = "ui/components/settings/github_ai_agent_header_tooltip.slint"
        self.write(owner, "// fixture\n" * 160)
        self.assert_accepted()
        self.write(owner, "// fixture\n" * 161)
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("AI Agent Header Tooltip UI lines is 161 (budget 160)", result.stderr)

    def test_header_object_and_draft_modes_accept_data_only_conversion(self):
        self.write("src/settings_page/ai_agent_header_object.rs", """use crate::settings::AiAgentHeader;
fn parse(source: &str) { let _: Result<String, _> = serde_json::from_str(source); }
""")
        self.write("src/settings_page/ai_agent_header_drafts.rs", """use slint::{Global, ModelRc};
fn normalize(draft: &crate::settings::AiAgentSettings) {
    let _ = crate::ai::request_headers::validate(draft);
}
""")
        self.assert_accepted()

    def test_header_object_parser_rejects_ui_io_scheduling_and_store_dependencies(self):
        owner = "src/settings_page/ai_agent_header_object.rs"
        for source in ["use std::fs;", "use std::{sync::Arc, net};", "use tokio::task;",
                       "use reqwest::header::HeaderMap;", "use slint::Model;",
                       "use crate::MainWindow;", "use crate::settings::SettingsManager;",
                       "use usher_state_store::PersistentStore;", "use crate::app_runtime;"]:
            with self.subTest(source=source):
                self.write(owner, source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("AI Agent Header object parser owns UI, I/O, scheduling or Stores", result.stderr)
                self.assertIn(owner, result.stderr)

    def test_header_draft_modes_reject_direct_io_workers_and_concrete_stores(self):
        owner = "src/settings_page/ai_agent_header_drafts.rs"
        for source in ["use std::process;", "use std::{sync::Arc, fs};", "use tokio::task;",
                       "use reqwest::header::HeaderMap;", "use usher_state_store::LocalStateStore;",
                       "use crate::settings::SettingsManager;", "use crate::app_runtime;"]:
            with self.subTest(source=source):
                self.write(owner, source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("AI Agent Header draft modes bypass their parser/validation facade", result.stderr)
                self.assertIn(owner, result.stderr)

    def test_registry_reducer_rejects_ui_and_controller_dependencies(self):
        owner = "src/ai/settings_state/actions.rs"
        for source in ["use crate::MainWindow;", "use slint::Global;",
                       "use super::controller::ControllerInner;"]:
            with self.subTest(source=source):
                self.write(owner, source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("AI Agent registry actions depend on UI/controller types", result.stderr)
                self.assertIn(owner, result.stderr)

    def test_connection_probe_accepts_shared_runtime_and_event_loop_delivery(self):
        self.write("src/ai/settings_ui/connection_test.rs", """use slint::{ComponentHandle, Global};
fn probe() {
    crate::app_runtime::shared_runtime().spawn(async move {
        let _ = crate::ai::agent::test_connection(&draft.connection).await;
        let _ = slint::invoke_from_event_loop(move || {});
    });
}
""")
        self.assert_accepted()

    def test_connection_probe_rejects_direct_io_http_and_concrete_stores(self):
        owner = "src/ai/settings_ui/connection_test.rs"
        for source in ["use std::fs;", "use std::{sync::Arc, process};",
                       "use std::thread;", "use reqwest::Url;",
                       "use usher_state_store::LocalStateStore;",
                       "use usher_state_store::PersistentStore;",
                       'fn fixture() { crate::http::get("https://example.test"); }']:
            with self.subTest(source=source):
                self.write(owner, source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("AI Agent connection probe bypasses its transport or settings facade", result.stderr)
                self.assertIn(owner, result.stderr)

    def test_chat_request_owner_rejects_raw_io_clients_and_concrete_stores(self):
        owner = "src/chat/request.rs"
        for source in ["use std::fs;", "use std::io;", "use std::net::TcpStream;",
                       "use std::{sync::Arc, process};", "use std :: thread;",
                       "use reqwest::Url;", "use reqwest::Client as Transport;",
                       "use usher_state_store::LocalStateStore;",
                       "use usher_state_store::PersistentStore;"]:
            with self.subTest(source=source):
                self.write(owner, source + "\n")
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("Chat request capture owns raw I/O, clients or concrete Stores", result.stderr)
                self.assertIn(owner, result.stderr)

    def test_shared_focus_owner_accepts_only_ui_navigation_dependencies(self):
        self.write("src/focus_navigation.rs", "use crate::MainWindow;\nuse i_slint_core::item_tree::ItemRc;\nuse slint::ComponentHandle;\n")
        self.assert_accepted()

    def test_extracted_leaves_cannot_import_controller_or_ui(self):
        cases = [
            ("crates/usher-preview/src/file_source.rs", "use crate::MainWindow;"),
            ("src/ai/settings_state.rs", "use crate::SettingsPageBridge;"),
            ("src/message_center/commands.rs", "use slint::ComponentHandle;"),
            ("src/message_center/focus.rs", "use std::fs;"),
            ("src/ui_components/mod.rs", "use std::fs;"),
            ("src/ui_components/feedback.rs", "use usher_state_store::LocalStateStore;"),
            ("src/ui_components/popover.rs", "use usher_state_store::PersistentStore;"),
            ("src/message_center/focus.rs", "use usher_state_store::LocalStateStore;"),
            ("src/focus_navigation.rs", "use std::process::Command;"),
            ("src/ui_components/keyboard.rs", "use std::fs;"),
            ("src/focus_navigation.rs", "use crate::message_center;"),
            ("src/focus_navigation.rs", "use crate::{MainWindow, ui_components};"),
            ("src/ai/settings_ui.rs", "use usher_state_store::PersistentStore;"),
            ("src/notification/ingress.rs", "use usher_state_store::LocalStateStore;"),
            ("src/shutdown/pipeline.rs", "use crate::MainWindow;"),
            ("src/shutdown/pipeline/phase.rs", "use slint::ComponentHandle;"),
            ("src/diagnostics/archive.rs", "use slint::ComponentHandle;"),
            ("src/update/download_status.rs", "use std::fs;"),
            ("src/workflow/state/reducer.rs", "use crate::MainWindow;"),
            ("src/workflow/state/equality.rs", "use slint::ComponentHandle;"),
            ("src/workflow/tag/state.rs", "use std::process::Command;"),
            ("src/workflow/tag/activity.rs", "use super::controller::Controller;"),
            ("src/ai_review/producers.rs", "use crate::MainWindow;"),
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
