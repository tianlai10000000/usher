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

    def copy(self, path):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / path, target)

    def copy_elevation_fixture(self):
        for path in (
            "scripts/generate-dialog-elevation.py",
            "ui/assets/elevation/dialog-shadow-mask.png",
            "ui/assets/elevation/floating-shadow-mask.png",
            "ui/assets/elevation/compact-shadow-mask.png",
            "ui/components/github_elevation.slint",
            "ui/components/dialog/github_dialog.slint",
            "ui/components/dialog/dialog_enhance.slint",
            "ui/components/project/github_project_dialog.slint",
            "ui/components/settings/github_hook_settings.slint",
            "ui/components/update/update.slint",
            "ui/components/github_action_menu.slint",
            "ui/components/button/github_icon_button.slint",
            "ui/components/notify/actions_notify.slint",
            "ui/components/toast/toast.slint",
            "ui/components/pr_review/content.slint",
            "ui/components/navigation/github_breadcrumb.slint",
            "ui/views/workflow/pr_review_workspace.slint",
            "ui/main.slint",
        ):
            self.copy(path)

    def check(self):
        return subprocess.run(
            ["sh", str(self.root / "scripts/check-ui-policy.sh")],
            cwd=self.temp.name,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_shared_floating_lifecycle_wrapper_is_accepted(self):
        path = "ui/components/github_floating_overlay.slint"
        self.write(path, (ROOT / path).read_text())
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_lifecycle_wrapper_requires_private_geometry_to_track_active(self):
        path = "ui/components/github_floating_overlay.slint"
        source = (ROOT / path).read_text()
        changed = source.replace("visible: root.active;", "visible: true;", 1)
        self.assertNotEqual(source, changed)
        self.write(path, changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("private geometry visibility must be exactly root.active", result.stderr)

    def test_lifecycle_wrapper_rejects_root_opacity(self):
        path = "ui/components/github_floating_overlay.slint"
        source = (ROOT / path).read_text()
        declaration = (
            "export component GitHubTransientFloatingOverlayHost inherits Rectangle {\n"
        )
        changed = source.replace(declaration, declaration + "    opacity: 0;\n", 1)
        self.assertNotEqual(source, changed)
        self.write(path, changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("lifecycle facade root must not set opacity", result.stderr)

    def test_lifecycle_wrapper_requires_one_private_geometry_instance(self):
        path = "ui/components/github_floating_overlay.slint"
        source = (ROOT / path).read_text()
        marker = "    positioning-host := FloatingOverlayGeometryHost {\n"
        duplicate = (
            "    duplicate-host := FloatingOverlayGeometryHost {\n"
            "        visible: root.active;\n"
            "    }\n\n"
        )
        changed = source.replace(marker, duplicate + marker, 1)
        self.assertNotEqual(source, changed)
        self.write(path, changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("must own exactly one private geometry instance", result.stderr)

    def test_transient_host_allows_nested_surface_opacity(self):
        self.write(
            "ui/components/settings/github_hook_tooltip.slint",
            """GitHubTransientFloatingOverlayHost {
    active: true;
    Rectangle {
        opacity: 0;
    }
}
""",
        )
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_unregistered_sibling_portal_is_rejected(self):
        self.write(
            "ui/components/settings/github_hook_tooltip_copy.slint",
            "GitHubFloatingOverlayHost {}\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("feature portals must use GitHubTransientFloatingOverlayHost", result.stderr)
        self.assertIn("github_hook_tooltip_copy.slint", result.stderr)

    def test_raw_host_direct_opacity_is_rejected_in_infrastructure(self):
        self.write(
            "ui/components/github_floating_overlay.slint",
            """FloatingOverlayGeometryHost {
    opacity: 0;
}
""",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("full-window overlay lifecycle", result.stderr)
        self.assertIn("must not set opacity directly", result.stderr)

    def test_transient_host_direct_opacity_is_rejected(self):
        self.write(
            "ui/components/settings/github_hook_tooltip.slint",
            """GitHubTransientFloatingOverlayHost {
    active: false;
    opacity: 0;
}
""",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("must not set opacity directly", result.stderr)

    def test_transient_host_direct_visible_is_rejected(self):
        self.write(
            "ui/components/messages/github_message_tooltip.slint",
            """GitHubTransientFloatingOverlayHost {
    active: false;
    visible: false;
}
""",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("must not set visible directly", result.stderr)

    def test_transient_host_requires_explicit_active_lifecycle(self):
        self.write(
            "ui/components/messages/github_message_tooltip.slint",
            "GitHubTransientFloatingOverlayHost {}\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("must set active explicitly", result.stderr)

    def test_fixed_action_portal_rejects_multiple_transient_hosts(self):
        self.write(
            "ui/components/messages/github_message_tooltip.slint",
            "GitHubTransientFloatingOverlayHost { active: true; }\n" * 2,
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("must share one transient Floating Overlay host", result.stderr)

    def test_generic_dynamic_portal_allows_two_conditional_slots(self):
        path = "ui/components/github_tooltip_overlay.slint"
        self.write(path, (ROOT / path).read_text())
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_generic_dynamic_portal_rejects_third_transient_host(self):
        path = "ui/components/github_tooltip_overlay.slint"
        source = (ROOT / path).read_text()
        source += "\nGitHubTransientFloatingOverlayHost { active: true; }\n"
        self.write(path, source)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("exactly two mutually exclusive", result.stderr)

    def test_generic_dynamic_portal_rejects_nonexclusive_slots(self):
        path = "ui/components/github_tooltip_overlay.slint"
        source = (ROOT / path).read_text()
        changed = source.replace("&& !root.even-claim:", "&& root.even-claim:", 1)
        self.assertNotEqual(source, changed)
        self.write(path, changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("exactly two mutually exclusive claim slots", result.stderr)

    def test_allowlisted_hook_path_still_rejects_builtin_tooltip(self):
        self.write("ui/components/settings/github_hook_tooltip.slint", "Tooltip {}\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("built-in/local Tooltip", result.stderr)

    def test_allowlisted_hook_path_still_rejects_private_material(self):
        self.write(
            "ui/components/settings/github_hook_tooltip.slint",
            'import { Tooltip } from "material-1.18.0/private.slint";\n',
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("private vendored Material", result.stderr)

    def test_hook_code_editor_current_lifecycle_owner_is_accepted(self):
        path = "ui/components/settings/github_hook_settings.slint"
        self.write(path, (ROOT / path).read_text())
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_hook_code_editor_lifecycle_requires_route_activity(self):
        path = "ui/components/settings/github_hook_settings.slint"
        source = (ROOT / path).read_text()
        changed = source.replace("root.route-active\n        && ", "", 1)
        self.assertNotEqual(source, changed)
        self.write(path, changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "HookCodeEditor requires a conditional owner gated by route-active and Add/Edit session lifecycle",
            result.stderr,
        )

    def test_hook_code_editor_lifecycle_rejects_visible_instead_of_conditional_owner(self):
        path = "ui/components/settings/github_hook_settings.slint"
        source = (ROOT / path).read_text()
        conditional = "if root.editor-active: editor-frame := Rectangle {"
        changed = source.replace(
            conditional,
            "editor-frame := Rectangle {\n                visible: root.editor-active;",
            1,
        )
        self.assertNotEqual(source, changed)
        self.write(path, changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "HookCodeEditor requires a conditional owner gated by route-active and Add/Edit session lifecycle",
            result.stderr,
        )

    def test_hook_code_editor_rejects_rounded_document_clip(self):
        path = "ui/components/settings/github_hook_settings.slint"
        source = (ROOT / path).read_text()
        marker = "                background: GitHubTheme.control-input-bg;\n"
        changed = source.replace(marker, marker + "                clip: true;\n", 1)
        self.assertNotEqual(source, changed)
        self.write(path, changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("must not round-clip the document subtree", result.stderr)

    def test_dialog_fixed_cost_elevation_is_accepted(self):
        self.copy_elevation_fixture()
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_named_dialog_requires_the_shared_shell(self):
        self.write(
            "ui/components/rogue_dialog.slint",
            "export component RogueDialog inherits Rectangle { Rectangle {} }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "component RogueDialog must own exactly one GitHubDialog instance",
            result.stderr,
        )

    def test_named_modal_comments_and_strings_do_not_satisfy_the_shared_shell(self):
        self.write(
            "ui/components/rogue_modal.slint",
            """export component RogueModal inherits Rectangle {
    // GitHubDialog {}
    Text { text: "GitHubDialog {"; }
}
""",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "component RogueModal must own exactly one GitHubDialog instance",
            result.stderr,
        )

    def test_unregistered_shared_dialog_consumer_is_rejected(self):
        self.copy_elevation_fixture()
        self.write(
            "ui/components/rogue_surface.slint",
            "Rectangle { GitHubDialog {} }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("unregistered GitHubDialog instance", result.stderr)

    def test_dialog_elevation_cannot_bypass_the_shared_shell(self):
        self.copy_elevation_fixture()
        self.write(
            "ui/components/rogue_surface.slint",
            "Rectangle { GitHubDialogElevation {} }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "must have exactly one application instance owned by GitHubDialog",
            result.stderr,
        )

    def test_modal_backdrop_paint_is_registered(self):
        self.copy_elevation_fixture()
        self.write(
            "ui/components/rogue_surface.slint",
            "Rectangle { background: GitHubTheme.backdrop; }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("modal backdrop paint must be owned", result.stderr)

    def test_dialog_live_shadow_is_rejected(self):
        path = "ui/components/dialog/github_dialog.slint"
        source = (ROOT / path).read_text()
        changed = source.replace(
            "        background: GitHubTheme.canvas-default;\n",
            "        background: GitHubTheme.canvas-default;\n"
            "        drop-shadow-blur: 24px;\n",
            1,
        )
        self.assertNotEqual(source, changed)
        self.write(path, changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("live box shadow", result.stderr)

    def test_unregistered_live_shadow_is_rejected(self):
        self.write(
            "ui/components/large_surface.slint",
            "Rectangle { drop-shadow-blur: 24px; }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("raw live box shadow is forbidden", result.stderr)

    def test_live_shadow_spread_is_rejected(self):
        self.write(
            "ui/components/compact_surface.slint",
            "Rectangle { drop-shadow-spread: 2px; }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("drop-shadow-spread", result.stderr)

    def test_live_shadow_alias_is_rejected(self):
        self.write(
            "ui/components/compact_surface.slint",
            "Rectangle { property <length> shadow <=> root.drop-shadow-blur; }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("drop-shadow-blur", result.stderr)

    def test_shadow_words_in_comments_and_strings_are_ignored(self):
        self.write(
            "ui/components/compact_surface.slint",
            "// drop-shadow-spread: 2px;\nText { text: \"drop-shadow-blur: 4px\"; }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_dialog_elevation_rejects_renderer_layer(self):
        self.write(
            "ui/components/github_elevation.slint",
            "export component GitHubDialogElevation inherits Image { Layer{} }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("fixed-cost elevation forbids Layer", result.stderr)

    def test_elevation_rejects_whitespace_obfuscated_clip(self):
        self.write(
            "ui/components/github_elevation.slint",
            "export component GitHubDialogElevation inherits Image { clip : true; }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("fixed-cost elevation forbids clip", result.stderr)

    def test_elevation_rejects_whitespace_obfuscated_opacity(self):
        self.write(
            "ui/components/github_elevation.slint",
            "export component GitHubDialogElevation inherits Image { opacity : 0; }\n",
        )
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("fixed-cost elevation forbids opacity", result.stderr)

    def test_elevation_rejects_caps_that_overlap_small_targets(self):
        self.copy_elevation_fixture()
        path = self.root / "ui/components/github_elevation.slint"
        source = path.read_text()
        changed = source.replace(
            "nine-slice(27 27 27 27)",
            "nine-slice(38 38 38 38)",
            1,
        )
        self.assertNotEqual(source, changed)
        path.write_text(changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("invalid direct source contract", result.stderr)

    def test_elevation_contract_must_be_on_component_root(self):
        self.copy_elevation_fixture()
        path = self.root / "ui/components/github_elevation.slint"
        source = path.read_text()
        direct = "    x: root.target-x - 24px;\n"
        changed = source.replace(
            direct,
            "    x: 0px;\n    Rectangle { x: root.target-x - 24px; }\n",
            1,
        )
        self.assertNotEqual(source, changed)
        path.write_text(changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("invalid direct x contract", result.stderr)

    def test_elevation_comments_do_not_trigger_or_satisfy_contracts(self):
        self.copy_elevation_fixture()
        elevation = self.root / "ui/components/github_elevation.slint"
        elevation.write_text(
            elevation.read_text()
            + "\n// Layer { opacity: 0; drop-shadow-spread: 2px; }\n"
        )
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

        menu = self.root / "ui/components/github_action_menu.slint"
        source = menu.read_text()
        changed = source.replace(
            "GitHubFloatingElevation {",
            "Rectangle { // GitHubFloatingElevation {",
            1,
        )
        self.assertNotEqual(source, changed)
        menu.write_text(changed)
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("expected 1 GitHubFloatingElevation instance(s), found 0", result.stderr)

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
        source = Path(os.environ.get("USHER_TEST_QUALITY_BASE_SCRIPT", ROOT / "scripts/check-quality.sh"))
        shutil.copy(source, self.root / "scripts/check-quality.sh")
        self.calls = self.root / "calls.txt"
        self.environment_calls = self.root / "environment.txt"
        self.source_file = self.root / "unchanged.rs"
        self.source_file.write_text("fn main( ) { }\n")
        self.env = dict(os.environ, PATH=f"{self.root / 'bin'}:{os.environ['PATH']}",
                        USHER_TEST_CALLS=str(self.calls),
                        USHER_TEST_ENV_CALLS=str(self.environment_calls))
        for name in ["cargo", "python3", "git", "rg"]:
            self.stub(self.root / "bin" / name, name)
        for name in ["architecture-structure", "ui-policy", "i18n"]:
            self.stub(self.root / "scripts" / f"check-{name}.sh", name)

    def stub(self, path, label):
        path.write_text(
            "#!/bin/sh\n"
            f'call="{label} $*"\n'
            'printf "%s\\n" "$call" >> "$USHER_TEST_CALLS"\n'
            'printf "%s|%s|%s\\n" "$call" "${CARGO_INCREMENTAL:-unset}" "${SLINT_EMIT_DEBUG_INFO:-unset}" >> "$USHER_TEST_ENV_CALLS"\n'
            'if [ "$call" = "${USHER_TEST_FAIL:-}" ]; then exit "${USHER_TEST_FAIL_CODE:-1}"; fi\n'
            'if [ "$call" = "${USHER_TEST_FAIL_SECOND:-}" ]; then exit "${USHER_TEST_FAIL_SECOND_CODE:-1}"; fi\n'
            'if [ "$call" = "${USHER_TEST_INTERRUPT:-}" ]; then exit "${USHER_TEST_INTERRUPT_CODE:-130}"; fi\n'
            'exit 0\n'
        )
        path.chmod(0o755)

    def run_checks(self, *arguments):
        return subprocess.run(
            ["/bin/sh", str(self.root / "scripts/check-quality.sh"), *arguments],
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
        self.assertEqual(self.source_file.read_text(), "fn main( ) { }\n")

    def test_explicit_quick_keeps_root_scope_and_caller_environment(self):
        self.env["CARGO_INCREMENTAL"] = "caller-setting"
        self.env["SLINT_EMIT_DEBUG_INFO"] = "caller-debug"
        result = self.run_checks("--quick")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = self.calls.read_text().splitlines()
        self.assertEqual(len(calls), 8)
        self.assertIn("cargo check --locked", calls)
        self.assertIn("cargo test --lib --locked", calls)
        self.assertFalse(any("--workspace" in call for call in calls))
        self.assertTrue(all(line.endswith("|caller-setting|caller-debug")
                            for line in self.environment_calls.read_text().splitlines()))

    def test_full_runs_workspace_and_no_ai_application_contracts(self):
        self.env["CARGO_INCREMENTAL"] = "caller-setting"
        self.env["SLINT_EMIT_DEBUG_INFO"] = "caller-debug"
        result = self.run_checks("--full")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = self.calls.read_text().splitlines()
        self.assertEqual(calls, [
            "cargo fmt --all -- --check", "architecture-structure ", "ui-policy ",
            "i18n ", "python3 -m unittest discover -s scripts/tests -p test_*.py",
            "cargo check --workspace --all-targets --locked",
            "cargo test --workspace --tests --locked",
            "cargo clippy --workspace --all-targets --locked -- -D warnings",
            "cargo check -p usher --all-targets --no-default-features --features renderer-femtovg --locked",
            "git diff --check",
        ])
        environment = self.environment_calls.read_text().splitlines()
        self.assertIn("cargo test --workspace --tests --locked|0|1", environment)
        self.assertTrue(all(line.endswith("|0|caller-debug") for line in environment
                            if not line.startswith("cargo test --workspace ")))
        self.assertIn("10 checks passed", result.stdout)
        self.assertEqual(self.source_file.read_text(), "fn main( ) { }\n")

    def test_full_retains_multiple_failures_and_runs_every_later_contract(self):
        self.env.update(USHER_TEST_FAIL="architecture-structure ", USHER_TEST_FAIL_CODE="4",
                        USHER_TEST_FAIL_SECOND="cargo clippy --workspace --all-targets --locked -- -D warnings",
                        USHER_TEST_FAIL_SECOND_CODE="17")
        result = self.run_checks("--full")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("architecture(4)", result.stderr)
        self.assertIn("workspace-lint(17)", result.stderr)
        self.assertIn("8 checks passed", result.stdout)
        calls = self.calls.read_text().splitlines()
        self.assertEqual(len(calls), 10)
        self.assertIn("cargo test --workspace --tests --locked", calls)
        self.assertIn("cargo check -p usher --all-targets --no-default-features --features renderer-femtovg --locked", calls)
        self.assertEqual(calls[-1], "git diff --check")

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

    def test_full_termination_does_not_start_more_checks(self):
        self.env.update(USHER_TEST_INTERRUPT="cargo fmt --all -- --check",
                        USHER_TEST_INTERRUPT_CODE="143")
        result = self.run_checks("--full")
        self.assertEqual(result.returncode, 143)
        self.assertEqual(self.calls.read_text().splitlines(), ["cargo fmt --all -- --check"])

    def test_invalid_arguments_never_start_checks(self):
        for arguments in [("--unknown",), ("--quick", "--full"), ("--help", "--full")]:
            with self.subTest(arguments=arguments):
                result = self.run_checks(*arguments)
                self.assertEqual(result.returncode, 2)
                self.assertFalse(self.calls.exists())

    def test_missing_dependency_is_setup_failure_without_partial_checks(self):
        self.env["PATH"] = str(self.root / "bin")
        (self.root / "bin/rg").unlink()
        dirname = self.root / "bin/dirname"
        dirname.write_text('#!/bin/sh\nif [ "$1" = -- ]; then shift; fi\nprintf "%s\\n" "${1%/*}"\n')
        dirname.chmod(0o755)
        result = self.run_checks()
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("require rg on PATH", result.stderr)
        self.assertFalse(self.calls.exists())


class QualityWorkflowTests(unittest.TestCase):
    def test_manual_ci_reuses_full_entry_and_keeps_windows_scope(self):
        source = (ROOT / ".github/workflows/ci.yml").read_text()
        trigger = source.split("\non:\n", 1)[1].split("\npermissions:", 1)[0].strip()
        self.assertEqual(trigger, "workflow_dispatch:")
        quality, windows = source.split("\n  quality:\n", 1)[1].split("\n  windows-check:\n", 1)
        self.assertEqual(quality.count("run: ./scripts/check-quality.sh --full"), 1)
        self.assertNotIn("run: cargo", quality)
        self.assertEqual(windows.count("run: cargo check --workspace --all-targets --locked"), 1)
        self.assertNotIn("check-quality.sh", windows)
        nightly = (ROOT / ".github/workflows/nightly-performance.yml").read_text()
        active = "\n".join(line for line in nightly.splitlines() if not line.lstrip().startswith("#"))
        self.assertNotIn("schedule:", active)
        self.assertIn("workflow_dispatch:", active)


if __name__ == "__main__":
    unittest.main()
