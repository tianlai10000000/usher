"""Behavior tests for quality gates using isolated repository fixtures."""

import os
from pathlib import Path
import re
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
            "ui/features/project/dialog.slint",
            "ui/components/settings/github_hook_settings.slint",
            "ui/components/settings/github_ai_agent_editor.slint",
            "ui/components/settings/github_ai_agent_delete.slint",
            "ui/components/update/update.slint",
            "ui/components/github_action_menu.slint",
            "ui/components/button/github_icon_button.slint",
            "ui/components/notify/actions_notify.slint",
            "ui/components/toast/toast.slint",
            "ui/components/pr_review/content.slint",
            "ui/components/navigation/github_breadcrumb.slint",
            "ui/components/navigation/github_window_navigation.slint",
            "ui/views/workflow/pr_review_workspace.slint",
            "ui/features/pr_review/factory_host.slint",
            "crates/usher-ui/build.rs",
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

    def test_showcase_fixed_portal_accepts_one_host(self):
        self.copy("ui/components/showcase/github_showcase_tooltip.slint")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_agent_connection_action_keeps_seven_fixed_surfaces(self):
        self.copy("ui/components/settings/github_ai_agent_tooltip.slint")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_agent_connection_gate_rejects_peer_identity_and_surface_aliases(self):
        path = "ui/components/settings/github_ai_agent_tooltip.slint"
        source = (ROOT / path).read_text()
        gate = (self.root / "scripts/check-ui-policy.sh").read_text()
        start = gate.index("# Agent factory actions use fixed semantic leaves")
        end = gate.index("if elevation_path.exists():", start)
        baseline = gate[:start] + gate[end:]
        cases = [
            ("AgentTooltipState.show(AgentTooltipKind.TestConnection,",
             "AgentTooltipState.show(AgentTooltipKind.Edit,", "literal leaf"),
            ("tooltip-text: I18n.common-test-connection;",
             "tooltip-text: I18n.settings-ai-agent-edit;", "literal leaf"),
            ("            text: I18n.common-test-connection;",
             "            text: I18n.settings-ai-agent-edit;", "seven fixed literal surfaces"),
            ("surface-test-connection := GitHubTooltipSurface {",
             "surface-test-connection := Rectangle {", "seven fixed literal surfaces"),
        ]
        for previous, changed, message in cases:
            with self.subTest(change=changed):
                candidate = source.replace(previous, changed, 1)
                self.assertNotEqual(source, candidate)
                self.write(path, candidate)
                # The old general portal rules accepted these ownership bugs.
                self.write("scripts/check-ui-policy.sh", baseline)
                before = self.check()
                self.assertEqual(before.returncode, 0, before.stdout + before.stderr)
                self.write("scripts/check-ui-policy.sh", gate)
                after = self.check()
                self.assertEqual(after.returncode, 1, after.stdout + after.stderr)
                self.assertIn(message, after.stderr)

    def test_showcase_fixed_portal_rejects_extra_host(self):
        path = "ui/components/showcase/github_showcase_tooltip.slint"
        self.write(path, (ROOT / path).read_text() + "\nGitHubTransientFloatingOverlayHost { active: true; }\n")
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
        self.copy_elevation_fixture()
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

    def write_inherited_dialog(self, imports, base="BaseSurface", body=""):
        self.copy_elevation_fixture()
        self.write(
            "ui/fixtures/inherited.slint",
            f"{imports}\nexport component InheritedDialog inherits {base} {{ {body} }}\n",
        )

    def assert_inheritance_rejected(self, detail):
        result = self.check()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(
            "component InheritedDialog must own exactly one GitHubDialog instance",
            result.stderr,
        )
        self.assertIn(detail, result.stderr)

    def test_named_dialog_accepts_imported_inheritance_and_reexport_alias(self):
        self.write_inherited_dialog('import { Surface as BaseSurface } from "./barrel.slint";')
        self.write(
            "ui/fixtures/barrel.slint",
            'export { GitHubProjectDialog as Surface } from "../components/project/github_project_dialog.slint";\n',
        )
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_named_dialog_accepts_same_file_inheritance(self):
        self.write_inherited_dialog(
            'import { GitHubProjectDialog } from "../components/project/github_project_dialog.slint";\n'
            'component BaseSurface inherits GitHubProjectDialog {}'
        )
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_named_dialog_rejects_zero_shell_through_inheritance(self):
        self.write_inherited_dialog("component BaseSurface inherits Rectangle {}")
        self.assert_inheritance_rejected("found 0 across its inheritance chain")

    def test_named_dialog_rejects_inherited_plus_direct_aliased_shell(self):
        self.write_inherited_dialog(
            'import { GitHubProjectDialog as BaseSurface } from "../components/project/github_project_dialog.slint";\n'
            'import { GitHubDialog as ExtraShell } from "../components/dialog/github_dialog.slint";',
            body="ExtraShell {}",
        )
        self.assert_inheritance_rejected("found 2 across its inheritance chain")

    def test_inherited_dialog_rejects_extra_wrapper_subtree(self):
        self.write_inherited_dialog(
            'import { GitHubProjectDialog } from "../components/project/github_project_dialog.slint";',
            base="GitHubProjectDialog",
            body="GitHubProjectDialog {}",
        )
        self.assert_inheritance_rejected("inherited Dialog adapter must not add component instances")

    def test_inherited_dialog_rejects_extra_wrapper_alias_subtree(self):
        self.write_inherited_dialog(
            'import { GitHubProjectDialog as BaseSurface } from "../components/project/github_project_dialog.slint";',
            body="BaseSurface {}",
        )
        self.assert_inheritance_rejected("inherited Dialog adapter must not add component instances")

    def test_inherited_dialog_rejects_an_unproven_visual_subtree(self):
        self.write_inherited_dialog(
            'import { GitHubProjectDialog as BaseSurface } from "../components/project/github_project_dialog.slint";',
            body="Rectangle {}",
        )
        self.assert_inheritance_rejected("inherited Dialog adapter must not add component instances")

    def test_named_dialog_rejects_local_inheritance_cycle(self):
        self.write_inherited_dialog("component BaseSurface inherits InheritedDialog {}")
        self.assert_inheritance_rejected("cyclic Dialog inheritance")

    def test_named_dialog_rejects_reexport_binding_cycle(self):
        self.write_inherited_dialog('import { BaseSurface } from "./first.slint";')
        self.write("ui/fixtures/first.slint", 'export { BaseSurface } from "./second.slint";\n')
        self.write("ui/fixtures/second.slint", 'export { BaseSurface } from "./first.slint";\n')
        self.assert_inheritance_rejected("cyclic component binding")

    def test_named_dialog_rejects_ambiguous_ancestor_import(self):
        self.write_inherited_dialog(
            'import { GitHubProjectDialog as BaseSurface } from "../components/project/github_project_dialog.slint";\n'
            'import { GitHubProjectDialog as BaseSurface } from "../components/project/github_project_dialog.slint";'
        )
        self.assert_inheritance_rejected("ambiguous component ui/fixtures/inherited.slint:BaseSurface")

    def test_named_dialog_rejects_duplicate_ancestor_definition(self):
        self.write_inherited_dialog(
            'import { GitHubProjectDialog } from "../components/project/github_project_dialog.slint";\n'
            'component BaseSurface inherits GitHubProjectDialog {}\n'
            'component BaseSurface inherits GitHubProjectDialog {}'
        )
        self.assert_inheritance_rejected("ambiguous component ui/fixtures/inherited.slint:BaseSurface")

    def test_named_dialog_does_not_resolve_same_name_from_another_file(self):
        self.write_inherited_dialog(
            'import { GitHubProjectDialog as BaseSurface } from "./unrelated.slint";'
        )
        self.write(
            "ui/fixtures/unrelated.slint",
            "export component GitHubProjectDialog inherits Rectangle {}\n",
        )
        self.assert_inheritance_rejected("found 0 across its inheritance chain")

    def test_named_dialog_does_not_credit_a_fake_shared_shell(self):
        self.write(
            "ui/fixtures/dialog.slint",
            'import { GitHubDialog } from "./fake.slint";\n'
            'export component InheritedDialog { GitHubDialog {} }\n',
        )
        self.write(
            "ui/fixtures/fake.slint",
            "export component GitHubDialog inherits Rectangle {}\n",
        )
        self.assert_inheritance_rejected("found 0 across its inheritance chain")

    def test_named_dialog_rejects_missing_or_private_imported_ancestor(self):
        self.write_inherited_dialog('import { BaseSurface } from "./private.slint";')
        self.assert_inheritance_rejected("missing component source")
        self.write("ui/fixtures/private.slint", "component BaseSurface inherits Rectangle {}\n")
        self.assert_inheritance_rejected("unresolved or ambiguous component")

    def test_named_dialog_rejects_unresolved_library_ancestor(self):
        self.write_inherited_dialog('import { BaseSurface } from "@unknown";')
        self.assert_inheritance_rejected("missing component source")

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

    def test_ai_agent_dialogs_cannot_replace_the_shared_shell(self):
        self.copy_elevation_fixture()
        for path in ("ui/components/settings/github_ai_agent_editor.slint",
                     "ui/components/settings/github_ai_agent_delete.slint"):
            with self.subTest(path=path):
                source = (ROOT / path).read_text()
                self.assertIn("frame := GitHubDialog {", source)
                self.write(path, source.replace("frame := GitHubDialog {", "frame := Rectangle {", 1))
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(path, result.stderr)
                self.assertIn("must own exactly one GitHubDialog", result.stderr)
                self.write(path, source)

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

    def test_public_factory_helper_checks_feature_roots_and_real_portal_instances(self):
        self.write("crates/fixture/build.rs", '''fn main() {
    usher_ui_build::compile_library_with_config(root.join("ui/features/fixture.slint"), config, "fixture");
}
''')
        path = "ui/features/fixture.slint"
        self.write(path, "GitHubTooltipOverlayHost {}\n")
        self.assertEqual(self.check().returncode, 0)
        self.write(path, 'import { GitHubTooltipOverlayHost } from "../components/github_tooltip_overlay.slint";\n// GitHubTooltipOverlayHost {}\nRectangle {}\n')
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("independent factory root lacks a tooltip portal", result.stderr)

    def test_factory_helper_alias_cannot_silently_skip_the_root_check(self):
        self.write("crates/fixture/build.rs", '''use usher_ui_build::compile_library_with_config as factory;
fn main() { factory(root.join("ui/features/fixture.slint"), config, "fixture"); }
''')
        self.write("ui/features/fixture.slint", "Rectangle {}\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("cannot locate the factory root view through a compiler alias", result.stderr)

    def pr_static_fixture(self):
        self.write("ui/main.slint", 'import { PrReviewHost } from "@pr-review";\nGitHubArtifactPreviewPanel {}\n')
        self.copy("crates/usher-ui/build.rs")
        self.copy("ui/features/pr_review/factory_host.slint")

    def test_static_mainwindow_closure_allows_independent_and_integrated_graphs(self):
        self.pr_static_fixture()
        # The live interpreter is allowed its heavy UI graph; it is excluded
        # from the actual static library selection/import closure.
        self.write("ui/features/pr_review/integrated.slint", 'import { Page } from "../../views/workflow/pr_review_workspace.slint";\n')
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_static_mainwindow_closure_rejects_indirect_heavy_imports_and_reexports(self):
        self.pr_static_fixture()
        for owner in ("views/workflow/pr_review_workspace.slint", "views/workflow/pr_review_conflicts.slint", "components/pr_review/content.slint"):
            with self.subTest(owner=owner):
                self.write("ui/features/pr_review/factory_host.slint", 'export { Proxy } from "relay.slint";\n')
                self.write("ui/features/pr_review/relay.slint", f'export {{ Page as Proxy }} from "../../{owner}";\n')
                result = self.check()
                self.assertEqual(result.returncode, 1)
                self.assertIn("static PR import closure reaches heavy owner", result.stderr)
                self.assertIn("relay.slint", result.stderr)

    def test_static_mainwindow_closure_does_not_require_the_pr_library_marker(self):
        self.write("ui/main.slint", 'import { Page } from "views/workflow/pr_review_workspace.slint";\nGitHubArtifactPreviewPanel {}\n')
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("static PR import closure reaches heavy owner", result.stderr)

    def test_static_build_mapping_cannot_switch_back_to_integrated_or_use_comment_decoy(self):
        self.pr_static_fixture()
        path = self.root / "crates/usher-ui/build.rs"
        source = path.read_text()
        changed = source.replace('join("ui/features/pr_review/factory_host.slint")', 'join("ui/features/pr_review/integrated.slint")')
        self.assertNotEqual(changed, source)
        path.write_text(changed + "\n/* " + source + " */\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("static PR mapping must isolate the factory host", result.stderr)

    def test_pr_factory_window_adapter_accepts_restoration_and_variable_renaming(self):
        path = "src/workflow/pr_review/ui.rs"
        self.copy(path)
        target = self.root / path
        source = target.read_text()
        for original, replacement in (("host", "host_adapter"), ("shell", "shell_root"), ("result", "created")):
            source = re.sub(r"\b" + original + r"\b", replacement, source)
        target.write_text(source)
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pr_factory_window_adapter_rejects_detached_or_late_restoration(self):
        path = "src/workflow/pr_review/ui.rs"
        source = (ROOT / path).read_text()
        variants = [
            source.replace("new_with_existing_window(window.window())", "new()", 1),
            source.replace("new_with_existing_window(window.window())", "new_with_existing_window(other.window())", 1),
            source.replace("    host.set_component(&shell);\n", "", 1) + "\n// host.set_component(&shell);\n",
            source.replace("    host.set_component(&shell);\n", "", 1).replace('    let page = result.expect("failed to create PR Review UI");', '    let page = result.expect("failed to create PR Review UI");\n    host.set_component(&shell);', 1),
        ]
        for changed in variants:
            with self.subTest(variant=variants.index(changed)):
                self.assertNotEqual(changed, source)
                self.write(path, changed)
                result = self.check()
                self.assertEqual(result.returncode, 1)
                self.assertIn("restore its shell before result handling or binding", result.stderr)

    def pr_tooltip_fixture(self):
        for suffix in ("state", "anchors", "buttons", "overlay"):
            self.copy(f"ui/components/pr_review/tooltip_{suffix}.slint")

    def test_pr_fixed_tooltip_literal_owner_graph_is_accepted(self):
        self.pr_tooltip_fixture()
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pr_fixed_tooltip_rejects_swapped_identity_dynamic_kind_and_generic_double_publisher(self):
        self.pr_tooltip_fixture()
        cases = [
            ("anchors", "PrReviewTooltipKind.CopySourceBranch,", "PrReviewTooltipKind.CopyFullSha,"),
            ("anchors", "export component PrCopySourceBranchTooltipAnchor inherits PrReviewTooltipAnchor {", "export component PrCopySourceBranchTooltipAnchor inherits PrReviewTooltipAnchor {\n    in property <PrReviewTooltipKind> kind;"),
            ("buttons", "show-tooltip: !PrReviewTooltipState.enabled;", "show-tooltip: true;"),
            ("state", "enabled: false;", "enabled: true;"),
        ]
        for suffix, original, replacement in cases:
            with self.subTest(owner=suffix, replacement=replacement):
                path = self.root / f"ui/components/pr_review/tooltip_{suffix}.slint"
                source = path.read_text()
                self.assertIn(original, source)
                path.write_text(source.replace(original, replacement, 1))
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn(f"ui/components/pr_review/tooltip_{suffix}.slint", result.stderr)
                path.write_text(source)

    def test_pr_fixed_tooltip_rejects_swapped_labels_dimensions_and_conditional_measurement(self):
        self.pr_tooltip_fixture()
        path = self.root / "ui/components/pr_review/tooltip_overlay.slint"
        source = path.read_text()
        variants = [
            source.replace("text: I18n.pr-review-copy-source-branch;", "text: I18n.pr-review-copy-full-sha;", 1),
            source.replace("? surface-copysourcebranch.width :", "? surface-copyfullsha.width :", 1),
            source.replace("surface-copysourcebranch := GitHubTooltipSurface {", "if root.active: surface-copysourcebranch := GitHubTooltipSurface {", 1),
            source.replace("text: I18n.pr-review-copy-source-branch;", "text: root.dynamic-text; // text: I18n.pr-review-copy-source-branch;", 1),
        ]
        for changed in variants:
            with self.subTest(variant=variants.index(changed)):
                self.assertNotEqual(changed, source)
                path.write_text(changed)
                result = self.check()
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("thirty fixed literal surfaces", result.stderr)

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
        baseline = os.environ.get("USHER_TEST_QUALITY_BASE_REF")
        if baseline:
            source = subprocess.check_output(
                ["git", "show", f"{baseline}:scripts/check-quality.sh"],
                cwd=ROOT,
                text=True,
            )
            (self.root / "scripts/check-quality.sh").write_text(source)
        else:
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
        self.assertEqual(len(calls), 9)
        self.assertEqual(calls[0], "cargo fmt --all -- --check")
        self.assertIn("cargo check --locked", calls)
        self.assertIn("cargo test --lib --locked", calls)
        self.assertIn("cargo test --doc --locked", calls)
        self.assertEqual(calls[-1], "git diff --check")
        self.assertIn("9 checks passed", result.stdout)
        self.assertEqual(self.source_file.read_text(), "fn main( ) { }\n")

    def test_explicit_quick_keeps_root_scope_and_caller_environment(self):
        self.env["CARGO_INCREMENTAL"] = "caller-setting"
        self.env["SLINT_EMIT_DEBUG_INFO"] = "caller-debug"
        result = self.run_checks("--quick")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = self.calls.read_text().splitlines()
        self.assertEqual(len(calls), 9)
        self.assertIn("cargo check --locked", calls)
        self.assertIn("cargo test --lib --locked", calls)
        self.assertIn("cargo test --doc --locked", calls)
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
            "cargo test --doc --locked",
            "git diff --check",
        ])
        environment = self.environment_calls.read_text().splitlines()
        self.assertIn("cargo test --workspace --tests --locked|0|1", environment)
        self.assertTrue(all(line.endswith("|0|caller-debug") for line in environment
                            if not line.startswith("cargo test --workspace ")))
        self.assertIn("11 checks passed", result.stdout)
        self.assertEqual(self.source_file.read_text(), "fn main( ) { }\n")

    def test_full_retains_multiple_failures_and_runs_every_later_contract(self):
        self.env.update(USHER_TEST_FAIL="architecture-structure ", USHER_TEST_FAIL_CODE="4",
                        USHER_TEST_FAIL_SECOND="cargo clippy --workspace --all-targets --locked -- -D warnings",
                        USHER_TEST_FAIL_SECOND_CODE="17")
        result = self.run_checks("--full")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("architecture(4)", result.stderr)
        self.assertIn("workspace-lint(17)", result.stderr)
        self.assertIn("9 checks passed", result.stdout)
        calls = self.calls.read_text().splitlines()
        self.assertEqual(len(calls), 11)
        self.assertIn("cargo test --workspace --tests --locked", calls)
        self.assertIn("cargo check -p usher --all-targets --no-default-features --features renderer-femtovg --locked", calls)
        self.assertEqual(calls[-1], "git diff --check")

    def test_architecture_failure_is_preserved_and_later_tests_run(self):
        self.env["USHER_TEST_FAIL"] = "architecture-structure "
        result = self.run_checks()
        self.assertEqual(result.returncode, 1)
        self.assertIn("architecture(1)", result.stderr)
        self.assertIn("8 checks passed", result.stdout)
        calls = self.calls.read_text().splitlines()
        self.assertIn("cargo test --lib --locked", calls)
        self.assertEqual(calls[-1], "git diff --check")

    def test_doctest_failure_remains_blocking_in_both_modes(self):
        self.env.update(USHER_TEST_FAIL="cargo test --doc --locked", USHER_TEST_FAIL_CODE="101")
        for mode, passed in [("--quick", 8), ("--full", 10)]:
            with self.subTest(mode=mode):
                if self.calls.exists():
                    self.calls.unlink()
                result = self.run_checks(mode)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("doctests(101)", result.stderr)
                self.assertIn(f"{passed} checks passed", result.stdout)
                calls = self.calls.read_text().splitlines()
                self.assertEqual(calls.count("cargo test --doc --locked"), 1)
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
