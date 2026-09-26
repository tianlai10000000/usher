#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

failed=0

# Application UI may only consume the public @material library mapping. Match
# every versioned vendored tree so a future directory replacement cannot make
# this guard silently stale.
private_material=$(rg -n 'material-[^"/]+/' ui --glob '*.slint' || true)
if [ -n "$private_material" ]; then
    echo "UI policy: private vendored Material imports are forbidden:" >&2
    echo "$private_material" >&2
    failed=1
fi

material_imports=$(rg -n '@material' ui --glob '*.slint' || true)
unexpected_material=$(echo "$material_imports" \
    | rg -v '^ui/main\.slint:[0-9]+:export \{ MaterialWindowAdapter \} from "@material";' \
    | rg -v '^ui/views/workflow/pr_review_workspace\.slint:[0-9]+:import \{ ScrollView \} from "@material";' \
    | rg -v '^ui/components/github_scroll_view\.slint:[0-9]+:import \{ ScrollView as MaterialScrollView \} from "@material";' \
    || true)
if [ -n "$unexpected_material" ]; then
    echo "UI policy: unexpected @material usage:" >&2
    echo "$unexpected_material" >&2
    failed=1
fi

legacy_icons=$(rg -n \
    '@image-url\("[^"]*assets/icons/(?!octicon-|github-|lucide-|provider-)[^"]+"\)' \
    ui --glob '*.slint' --pcre2 || true)
unexpected_icons=$(echo "$legacy_icons" \
    | rg -v '^ui/components/title_bar\.slint:[0-9]+:.*assets/icons/(settings|about)\.svg' \
    || true)
if [ -n "$unexpected_icons" ]; then
    echo "UI policy: unregistered application icon usage is forbidden:" >&2
    echo "$unexpected_icons" >&2
    failed=1
fi

# FemtoVG rasterizes a live box shadow at the target item's physical size and
# retains the result in renderer and item caches. Application UI therefore uses
# only fixed-source nine-slice elevations or direct paint such as a gradient.
# Keep this rule absolute: an exception list cannot prove a caller's geometry.
shadow_policy_violations=$(python3 - "$repo_root" <<'PY'
import pathlib
import re
import sys

repo_root = pathlib.Path(sys.argv[1])
token_pattern = re.compile(
    r'//[^\n]*|/\*.*?\*/|"(?:\\.|[^"\\])*"|'
    r'<=>|:=|&&|\|\||==|!=|<=|>=|'
    r'[A-Za-z_][A-Za-z0-9_-]*|\d+(?:\.\d+)?(?:px)?|'
    r'[{}():;,.!+\-*/@<>]',
    re.DOTALL,
)


def tokenize(source):
    tokens = []
    for match in token_pattern.finditer(source):
        value = match.group(0)
        if value.startswith("//") or value.startswith("/*"):
            continue
        tokens.append((value, match.start(), value.startswith('"')))
    return tokens


def values(tokens):
    return [token[0] for token in tokens]


def code_compact(tokens):
    return "".join(value for value, _, is_string in tokens if not is_string)


def sequence_positions(tokens, expected):
    token_values = values(tokens)
    width = len(expected)
    return [
        index
        for index in range(len(token_values) - width + 1)
        if token_values[index : index + width] == expected
    ]


def closing_brace(tokens, open_index):
    depth = 1
    for index in range(open_index + 1, len(tokens)):
        if tokens[index][0] == "{":
            depth += 1
        elif tokens[index][0] == "}":
            depth -= 1
            if depth == 0:
                return index
    return None


def component_body(tokens, name):
    header = ["export", "component", name, "inherits", "Image", "{"]
    matches = sequence_positions(tokens, header)
    if len(matches) != 1:
        return None, len(matches)
    open_index = matches[0] + len(header) - 1
    close_index = closing_brace(tokens, open_index)
    if close_index is None:
        return None, 1
    return tokens[open_index + 1 : close_index], 1


def owner_body(tokens, marker):
    matches = sequence_positions(tokens, marker)
    if len(matches) != 1:
        return None, len(matches)
    marker_end = matches[0] + len(marker)
    if marker[-1] == "{":
        open_index = marker_end - 1
    else:
        open_index = next(
            (
                index
                for index in range(marker_end, len(tokens))
                if tokens[index][0] in {"{", ";"}
            ),
            None,
        )
        if open_index is None or tokens[open_index][0] != "{":
            return None, 1
    close_index = closing_brace(tokens, open_index)
    if close_index is None:
        return None, 1
    return tokens[open_index + 1 : close_index], 1


def component_definitions(tokens):
    definitions = []
    for index, (value, _, is_string) in enumerate(tokens):
        if is_string or value != "component" or index + 1 >= len(tokens):
            continue
        name, _, name_is_string = tokens[index + 1]
        if name_is_string:
            continue
        open_index = next(
            (
                cursor
                for cursor in range(index + 2, len(tokens))
                if tokens[cursor][0] in {"{", ";"}
            ),
            None,
        )
        if open_index is None or tokens[open_index][0] != "{":
            continue
        close_index = closing_brace(tokens, open_index)
        if close_index is not None:
            definitions.append((name, open_index, close_index))
    return definitions


def direct_bindings(tokens):
    bindings = {}
    depth = 0
    index = 0
    while index < len(tokens):
        value, _, is_string = tokens[index]
        if value == "{":
            depth += 1
        elif value == "}":
            depth -= 1
        elif (
            depth == 0
            and not is_string
            and index + 1 < len(tokens)
            and tokens[index + 1][0] == ":"
        ):
            end = index + 2
            while end < len(tokens) and tokens[end][0] != ";":
                if tokens[end][0] == "{":
                    break
                end += 1
            if end < len(tokens) and tokens[end][0] == ";":
                bindings.setdefault(value, []).append(values(tokens[index : end + 1]))
                index = end
        index += 1
    return bindings


sources = {}
for path in sorted((repo_root / "ui").rglob("*.slint")):
    source = path.read_text(encoding="utf-8")
    tokens = tokenize(source)
    relative = path.relative_to(repo_root)
    sources[relative] = (source, tokens)
    for value, position, is_string in tokens:
        if not is_string and value.startswith("drop-shadow-"):
            line = source.count("\n", 0, position) + 1
            print(f"{relative}:{line}: raw live box shadow is forbidden ({value})")

elevation_path = repo_root / "ui/components/github_elevation.slint"
dialog_path = repo_root / "ui/components/dialog/github_dialog.slint"
if elevation_path.exists() or dialog_path.exists():
    if not elevation_path.exists():
        print("ui/components/github_elevation.slint: shared Dialog elevation is missing")
    if not dialog_path.exists():
        print("ui/components/dialog/github_dialog.slint: shared Dialog owner is missing")

# Dialog is an application boundary, not only a visual convention. Named
# Dialog/Modal wrappers must structurally own the shared shell, and every direct
# shared-shell site is registered so replacing one with custom modal chrome is a
# reviewed architecture change. The global raw-shadow rule above remains the
# final protection even for a deliberately disguised surface.
dialog_component_exemptions = {"GitHubDialog", "GitHubDialogElevation"}
for relative, (_, tokens) in sources.items():
    for name, open_index, close_index in component_definitions(tokens):
        if name in dialog_component_exemptions:
            continue
        if "Dialog" not in name and "Modal" not in name:
            continue
        body = tokens[open_index + 1 : close_index]
        count = len(sequence_positions(body, ["GitHubDialog", "{"]))
        if count != 1:
            print(
                f"{relative}: component {name} must own exactly one "
                f"GitHubDialog instance (found {count})"
            )

dialog_consumer_requirements = {
    pathlib.Path("ui/components/dialog/dialog_enhance.slint"): 1,
    pathlib.Path("ui/components/project/github_project_dialog.slint"): 1,
    pathlib.Path("ui/components/settings/github_hook_settings.slint"): 1,
    pathlib.Path("ui/components/update/update.slint"): 1,
    pathlib.Path("ui/views/workflow/pr_review_workspace.slint"): 5,
}
if dialog_path.exists():
    observed_dialog_consumers = {
        relative: len(sequence_positions(tokens, ["GitHubDialog", "{"]))
        for relative, (_, tokens) in sources.items()
    }
    for relative, expected in dialog_consumer_requirements.items():
        actual = observed_dialog_consumers.get(relative, 0)
        if actual != expected:
            print(
                f"{relative}: expected {expected} registered GitHubDialog "
                f"instance(s), found {actual}"
            )
    for relative, actual in observed_dialog_consumers.items():
        if actual and relative not in dialog_consumer_requirements:
            print(
                f"{relative}: found {actual} unregistered GitHubDialog "
                "instance(s)"
            )

    backdrop_requirements = {
        pathlib.Path("ui/components/dialog/github_dialog.slint"): 1,
        pathlib.Path("ui/main.slint"): 2,
    }
    observed_backdrops = {
        relative: len(sequence_positions(tokens, ["GitHubTheme", ".", "backdrop"]))
        for relative, (_, tokens) in sources.items()
    }
    for relative, expected in backdrop_requirements.items():
        actual = observed_backdrops.get(relative, 0)
        if actual != expected:
            print(
                f"{relative}: expected {expected} registered modal backdrop "
                f"paint(s), found {actual}"
            )
    for relative, actual in observed_backdrops.items():
        if actual and relative not in backdrop_requirements:
            print(
                f"{relative}: modal backdrop paint must be owned by "
                "GitHubDialog or the reviewed root continuation"
            )

if elevation_path.exists():
    relative = pathlib.Path("ui/components/github_elevation.slint")
    source, tokens = sources[relative]
    forbidden = {
        "ClippedImage",
        "Layer",
        "cache-rendering-hint",
        "clip",
        "horizontal-tiling",
        "opacity",
        "vertical-tiling",
    }
    for value, _, is_string in tokens:
        if not is_string and value in forbidden:
            print(f"{relative}: fixed-cost elevation forbids {value}")

    profiles = {
        "GitHubDialogElevation": {
            "visible": ["visible", ":", "root", ".", "target-width", ">=", "96px", "&&", "root", ".", "target-height", ">=", "96px", ";"],
            "x": ["x", ":", "root", ".", "target-x", "-", "24px", ";"],
            "y": ["y", ":", "root", ".", "target-y", "-", "16px", ";"],
            "width": ["width", ":", "root", ".", "target-width", "+", "48px", ";"],
            "height": ["height", ":", "root", ".", "target-height", "+", "48px", ";"],
            "source": ["source", ":", "@", "image-url", "(", '"../assets/elevation/dialog-shadow-mask.png"', ",", "nine-slice", "(", "72", "72", "72", "72", ")", ")", ";"],
            "image-fit": ["image-fit", ":", "fill", ";"],
            "colorize": ["colorize", ":", "GitHubTheme", ".", "shadow-floating-dialog", ";"],
        },
        "GitHubFloatingElevation": {
            "visible": ["visible", ":", "root", ".", "target-width", ">=", "40px", "&&", "root", ".", "target-height", ">=", "40px", ";"],
            "x": ["x", ":", "root", ".", "target-x", "-", "18px", ";"],
            "y": ["y", ":", "root", ".", "target-y", "-", "12px", ";"],
            "width": ["width", ":", "root", ".", "target-width", "+", "36px", ";"],
            "height": ["height", ":", "root", ".", "target-height", "+", "36px", ";"],
            "source": ["source", ":", "@", "image-url", "(", '"../assets/elevation/floating-shadow-mask.png"', ",", "nine-slice", "(", "37", "37", "37", "37", ")", ")", ";"],
            "image-fit": ["image-fit", ":", "fill", ";"],
            "colorize": ["colorize", ":", "root", ".", "shadow-color", ";"],
        },
        "GitHubCompactElevation": {
            "visible": ["visible", ":", "root", ".", "target-width", ">=", "32px", "&&", "root", ".", "target-height", ">=", "32px", ";"],
            "x": ["x", ":", "root", ".", "target-x", "-", "12px", ";"],
            "y": ["y", ":", "root", ".", "target-y", "-", "8px", ";"],
            "width": ["width", ":", "root", ".", "target-width", "+", "24px", ";"],
            "height": ["height", ":", "root", ".", "target-height", "+", "24px", ";"],
            "source": ["source", ":", "@", "image-url", "(", '"../assets/elevation/compact-shadow-mask.png"', ",", "nine-slice", "(", "27", "27", "27", "27", ")", ")", ";"],
            "image-fit": ["image-fit", ":", "fill", ";"],
            "colorize": ["colorize", ":", "root", ".", "shadow-color", ";"],
        },
    }
    for name, required_bindings in profiles.items():
        body, definition_count = component_body(tokens, name)
        if definition_count != 1 or body is None:
            print(f"{relative}: expected one exported {name} Image component")
            continue
        bindings = direct_bindings(body)
        for property_name, expected in required_bindings.items():
            if bindings.get(property_name) != [expected]:
                print(f"{relative}: {name} has an invalid direct {property_name} contract")

consumer_requirements = {
    pathlib.Path("ui/components/dialog/github_dialog.slint"): ("GitHubDialogElevation", 1, ["export", "component", "GitHubDialog"]),
    pathlib.Path("ui/components/github_action_menu.slint"): ("GitHubFloatingElevation", 1, ["export", "component", "GitHubActionMenu"]),
    pathlib.Path("ui/components/button/github_icon_button.slint"): ("GitHubCompactElevation", 1, ["export", "component", "GitHubIconButton"]),
    pathlib.Path("ui/components/notify/actions_notify.slint"): ("GitHubCompactElevation", 1, ["export", "component", "GitHubNotificationBanner"]),
    pathlib.Path("ui/components/toast/toast.slint"): ("GitHubCompactElevation", 1, ["export", "component", "GitHubToast"]),
    pathlib.Path("ui/components/pr_review/content.slint"): ("GitHubCompactElevation", 1, ["export", "component", "GitHubCodeRenderingIndicator"]),
    pathlib.Path("ui/main.slint"): ("GitHubCompactElevation", 1, ["main-content-host", ":=", "Rectangle", "{"]),
}
if elevation_path.exists():
    for relative, (component, expected, marker) in consumer_requirements.items():
        if relative not in sources:
            print(f"{relative}: required elevation consumer file is missing")
            continue
        _, tokens = sources[relative]
        body, owner_count = owner_body(tokens, marker)
        if owner_count != 1 or body is None:
            print(f"{relative}: expected elevation consumer owner is missing")
            continue
        count = len(sequence_positions(body, [component, "{"]))
        if count != expected:
                print(f"{relative}: expected {expected} {component} instance(s), found {count}")

if elevation_path.exists() and dialog_path.exists():
    dialog_elevation_instances = {
        relative: len(sequence_positions(tokens, ["GitHubDialogElevation", "{"]))
        for relative, (_, tokens) in sources.items()
    }
    total_dialog_elevations = sum(dialog_elevation_instances.values())
    owner_elevations = dialog_elevation_instances.get(
        pathlib.Path("ui/components/dialog/github_dialog.slint"), 0
    )
    if total_dialog_elevations != 1 or owner_elevations != 1:
        print(
            "ui/components/dialog/github_dialog.slint: GitHubDialogElevation "
            "must have exactly one application instance owned by GitHubDialog"
        )

if dialog_path.exists():
    _, tokens = sources[pathlib.Path("ui/components/dialog/github_dialog.slint")]
    body, _ = owner_body(tokens, ["export", "component", "GitHubDialog"])
    compact = "" if body is None else code_compact(body)
    if "if!root.narrow-fullscreen:GitHubDialogElevation{" not in compact:
        print("ui/components/dialog/github_dialog.slint: elevation must be disabled for narrow fullscreen")

breadcrumb_path = repo_root / "ui/components/navigation/github_breadcrumb.slint"
if elevation_path.exists():
    relative = pathlib.Path("ui/components/navigation/github_breadcrumb.slint")
    if not breadcrumb_path.exists():
        print(f"{relative}: direct-painted page-header edge owner is missing")
    else:
        _, tokens = sources[relative]
        body, owner_count = owner_body(tokens, ["export", "component", "GitHubBreadcrumb"])
        compact = "" if owner_count != 1 or body is None else code_compact(body)
        if "if!root.titlebar-mode:Rectangle{" not in compact or "background:@linear-gradient(" not in compact:
            print(f"{relative}: page-header edge must use direct gradient paint")
elif breadcrumb_path.exists():
    _, tokens = sources[pathlib.Path("ui/components/navigation/github_breadcrumb.slint")]
    body, owner_count = owner_body(tokens, ["export", "component", "GitHubBreadcrumb"])
    compact = "" if owner_count != 1 or body is None else code_compact(body)
    if "if!root.titlebar-mode:Rectangle{" not in compact or "background:@linear-gradient(" not in compact:
        print("ui/components/navigation/github_breadcrumb.slint: page-header edge must use direct gradient paint")
PY
)
if [ -n "$shadow_policy_violations" ]; then
    echo "UI policy: application surfaces must use fixed-source elevation or direct paint:" >&2
    echo "$shadow_policy_violations" >&2
    failed=1
fi

if [ -f ui/components/github_elevation.slint ]; then
    if ! python3 scripts/generate-dialog-elevation.py --check >/dev/null; then
        echo "UI policy: elevation assets do not match their generator." >&2
        failed=1
    fi
fi

# Slint globals do not cross ComponentFactory/library-module boundaries. Every
# independently generated route must therefore mount a factory-local tooltip
# portal at its root, even when its current controls happen not to show tips.
factory_build_scripts=$(rg -l '\.as_library\(' crates --glob 'build.rs' || true)
for factory_build_script in $factory_build_scripts; do
    factory_views=$(rg -o 'ui/views/[^" ]+\.slint' "$factory_build_script" || true)
    if [ -z "$factory_views" ]; then
        echo "UI policy: cannot locate the factory root view in $factory_build_script" >&2
        failed=1
        continue
    fi

    for factory_view in $factory_views; do
        if ! rg -q 'GitHub(Message)?TooltipOverlayHost' "$factory_view"; then
            echo "UI policy: independent factory root lacks a tooltip portal: $factory_view" >&2
            failed=1
        fi
    done
done

# Tooltip surfaces may differ in factory-local state/labels, but all placement
# must flow through the shared lifecycle facade. Its geometry implementation is
# file-private, and the former public raw host must never reappear.
legacy_floating_geometry=$(rg -n '\bGitHubFloatingOverlayHost\b' \
    ui --glob '*.slint' || true)
if [ -n "$legacy_floating_geometry" ]; then
    echo "UI policy: feature portals must use GitHubTransientFloatingOverlayHost:" >&2
    echo "$legacy_floating_geometry" >&2
    failed=1
fi

private_floating_geometry=$(rg -l '\bFloatingOverlayGeometryHost\b' \
    ui --glob '*.slint' || true)
unexpected_private_geometry=$(echo "$private_floating_geometry" \
    | rg -v '^ui/components/github_floating_overlay\.slint$' \
    || true)
if [ -n "$unexpected_private_geometry" ]; then
    echo "UI policy: private Floating Overlay geometry escaped its owner:" >&2
    echo "$unexpected_private_geometry" >&2
    failed=1
fi

# A fixed-action portal selects among persistent child surfaces inside one
# lifecycle host. The generic text portal is the sole two-slot exception: its
# mutually exclusive conditional slots recreate dynamic measurement state.
transient_host_consumers=$(rg -l 'GitHubTransientFloatingOverlayHost[[:space:]]*\{' \
    ui --glob '*.slint' || true)
for transient_host_consumer in $transient_host_consumers; do
    transient_host_count=$(rg -o 'GitHubTransientFloatingOverlayHost[[:space:]]*\{' \
        "$transient_host_consumer" | wc -l | tr -d ' ')
    if [ "$transient_host_consumer" = "ui/components/github_tooltip_overlay.slint" ]; then
        if [ "$transient_host_count" -ne 2 ]; then
            echo "UI policy: generic Tooltip portal must keep exactly two mutually exclusive transient hosts: $transient_host_consumer" >&2
            failed=1
        fi
        continue
    fi
    if [ "$transient_host_count" -gt 1 ]; then
        echo "UI policy: fixed-action portals must share one transient Floating Overlay host: $transient_host_consumer" >&2
        failed=1
    fi
done

# Opacity on a full-window positioning tree can retain a renderer layer after a
# route is hidden. Raw hosts therefore never accept opacity, and callers of the
# lifecycle facade may select it only through `active`; overriding visible or
# opacity would bypass the invariant. Parse component bodies by brace depth so
# opacity on a small nested surface remains valid.
overlay_lifecycle_violations=$(python3 - "$repo_root" <<'PY'
import pathlib
import re
import sys

repo_root = pathlib.Path(sys.argv[1])
targets = {
    "FloatingOverlayGeometryHost": ({"opacity"}, None),
    "GitHubTransientFloatingOverlayHost": ({"opacity", "visible"}, "active"),
}
token_pattern = re.compile(
    r'//[^\n]*|/\*.*?\*/|"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z0-9_-]*|[{}:.;]',
    re.DOTALL,
)


def closing_brace(tokens, open_index):
    depth = 1
    cursor = open_index + 1
    while cursor < len(tokens):
        current = tokens[cursor][0]
        if current == "{":
            depth += 1
        elif current == "}":
            depth -= 1
            if depth == 0:
                return cursor
        cursor += 1
    return None


def direct_property_names(tokens, open_index):
    close_index = closing_brace(tokens, open_index)
    if close_index is None:
        return set()
    depth = 1
    properties = set()
    for cursor in range(open_index + 1, close_index):
        current = tokens[cursor][0]
        if current == "{":
            depth += 1
        elif current == "}":
            depth -= 1
        elif depth == 1 and cursor + 1 < close_index and tokens[cursor + 1][0] == ":":
            properties.add(current)
    return properties


def direct_value(source, tokens, open_index, property_name):
    close_index = closing_brace(tokens, open_index)
    if close_index is None:
        return None
    depth = 1
    cursor = open_index + 1
    while cursor < close_index:
        current = tokens[cursor][0]
        if current == "{":
            depth += 1
        elif current == "}":
            depth -= 1
        elif (
            depth == 1
            and current == property_name
            and cursor + 1 < close_index
            and tokens[cursor + 1][0] == ":"
        ):
            value_start = tokens[cursor + 1][1] + 1
            value_end = cursor + 2
            while value_end < close_index and tokens[value_end][0] != ";":
                value_end += 1
            if value_end < close_index:
                return re.sub(r"\s+", "", source[value_start:tokens[value_end][1]])
            return None
        cursor += 1
    return None

for path in sorted((repo_root / "ui").rglob("*.slint")):
    source = path.read_text(encoding="utf-8")
    relative = path.relative_to(repo_root)
    tokens = []
    for match in token_pattern.finditer(source):
        token = match.group(0)
        if token.startswith("//") or token.startswith("/*") or token.startswith('"'):
            continue
        tokens.append((token, match.start()))

    for index, (token, _) in enumerate(tokens[:-1]):
        target = targets.get(token)
        if target is None or tokens[index + 1][0] != "{":
            continue

        forbidden, required = target
        depth = 1
        cursor = index + 2
        direct_properties = set()
        while cursor < len(tokens) and depth > 0:
            current, position = tokens[cursor]
            if current == "{":
                depth += 1
            elif current == "}":
                depth -= 1
            elif depth == 1 and cursor + 1 < len(tokens) and tokens[cursor + 1][0] == ":":
                direct_properties.add(current)
                if current in forbidden:
                    line = source.count("\n", 0, position) + 1
                    print(f"{relative}:{line}: {token} must not set {current} directly")
            cursor += 1

        if required is not None and required not in direct_properties:
            position = tokens[index][1]
            line = source.count("\n", 0, position) + 1
            print(f"{relative}:{line}: {token} must set {required} explicitly")

    if relative == pathlib.Path("ui/components/github_floating_overlay.slint"):
        facade_definitions = []
        private_definitions = []
        for index, (token, _) in enumerate(tokens):
            if index == 0 or tokens[index - 1][0] != "component":
                continue
            if token == "GitHubTransientFloatingOverlayHost":
                facade_definitions.append(index)
            elif token == "FloatingOverlayGeometryHost":
                private_definitions.append(index)

        if len(facade_definitions) != 1:
            print(f"{relative}: lifecycle facade must have exactly one component definition")
        else:
            facade_index = facade_definitions[0]
            facade_open = next(
                (index for index in range(facade_index + 1, len(tokens)) if tokens[index][0] == "{"),
                None,
            )
            if facade_open is None:
                print(f"{relative}: lifecycle facade definition has no body")
            else:
                forbidden = direct_property_names(tokens, facade_open) & {"opacity", "visible"}
                for property_name in sorted(forbidden):
                    print(f"{relative}: lifecycle facade root must not set {property_name}")

                facade_close = closing_brace(tokens, facade_open)
                raw_instances = []
                if facade_close is not None:
                    for index in range(facade_open + 1, facade_close):
                        if (
                            tokens[index][0] == "FloatingOverlayGeometryHost"
                            and index + 1 < facade_close
                            and tokens[index + 1][0] == "{"
                        ):
                            raw_instances.append(index + 1)
                if len(raw_instances) != 1:
                    print(f"{relative}: lifecycle facade must own exactly one private geometry instance")
                elif direct_value(source, tokens, raw_instances[0], "visible") != "root.active":
                    print(f"{relative}: private geometry visibility must be exactly root.active")

        if len(private_definitions) != 1:
            print(f"{relative}: must declare exactly one private FloatingOverlayGeometryHost")
        elif private_definitions[0] >= 2 and tokens[private_definitions[0] - 2][0] == "export":
            print(f"{relative}: FloatingOverlayGeometryHost must remain file-private")

    if relative == pathlib.Path("ui/components/github_tooltip_overlay.slint"):
        expected_slots = (
            r'if\s+TooltipOverlayState\.open\s*&&\s*TooltipOverlayState\.text\s*!=\s*""\s*'
            r'&&\s*root\.even-claim\s*:\s*even-floating-host\s*:=\s*'
            r'GitHubTransientFloatingOverlayHost\s*\{',
            r'if\s+TooltipOverlayState\.open\s*&&\s*TooltipOverlayState\.text\s*!=\s*""\s*'
            r'&&\s*!root\.even-claim\s*:\s*odd-floating-host\s*:=\s*'
            r'GitHubTransientFloatingOverlayHost\s*\{',
        )
        host_count = len(re.findall(r'\bGitHubTransientFloatingOverlayHost\s*\{', source))
        if host_count != 2 or any(len(re.findall(pattern, source)) != 1 for pattern in expected_slots):
            print(f"{relative}: generic Tooltip portal must keep exactly two mutually exclusive claim slots")
PY
)
if [ -n "$overlay_lifecycle_violations" ]; then
    echo "UI policy: full-window overlay lifecycle must use active visibility:" >&2
    echo "$overlay_lifecycle_violations" >&2
    failed=1
fi

# The Hook source editor owns native editable text, rich glyph runs and a
# scrolling viewport. Settings keeps its factory product mounted after the
# first visit, so the editor needs a real route-and-session conditional owner.
# Keep this deliberately narrow: it protects the reviewed ownership contract
# and fails clearly if that contract is refactored.
hook_editor_lifecycle_violations=$(python3 - "$repo_root" <<'PY'
import pathlib
import re
import sys

repo_root = pathlib.Path(sys.argv[1])
path = repo_root / "ui/components/settings/github_hook_settings.slint"
if not path.exists():
    raise SystemExit(0)

source = path.read_text(encoding="utf-8")
relative = path.relative_to(repo_root)
message = (
    "HookCodeEditor requires a conditional owner gated by route-active "
    "and Add/Edit session lifecycle"
)

definition = re.search(
    r"\bprivate\s+property\s*<\s*bool\s*>\s*editor-active\s*:\s*(.*?)\s*;",
    source,
    re.DOTALL,
)
allowed_expressions = {
    'root.route-active&&(HookSettingsState.dialog-mode=="add"'
    '||HookSettingsState.dialog-mode=="edit")',
    'root.route-active&&(HookSettingsState.dialog-mode=="edit"'
    '||HookSettingsState.dialog-mode=="add")',
}
if (
    definition is None
    or re.sub(r"\s+", "", definition.group(1)) not in allowed_expressions
):
    line = 1 if definition is None else source.count("\n", 0, definition.start()) + 1
    print(f"{relative}:{line}: {message}")

# Match braces while ignoring comments and strings so the instance must be
# structurally inside the reviewed conditional block, rather than merely after
# matching text or hidden by a visible/opacity property.
structural_tokens = re.compile(
    r'//[^\n]*|/\*.*?\*/|"(?:\\.|[^"\\])*"|[{}]',
    re.DOTALL,
)
pairs = {}
stack = []
for match in structural_tokens.finditer(source):
    token = match.group(0)
    if token.startswith("//") or token.startswith("/*") or token.startswith('"'):
        continue
    if token == "{":
        stack.append(match.start())
    elif stack:
        pairs[stack.pop()] = match.start()

owner_pattern = re.compile(
    r"\bif\s+root\.editor-active\s*:\s*"
    r"(?:[A-Za-z_][A-Za-z0-9_-]*\s*:=\s*)?"
    r"[A-Za-z_][A-Za-z0-9_-]*\s*\{"
)
owner_ranges = []
for match in owner_pattern.finditer(source):
    open_position = source.rfind("{", match.start(), match.end())
    close_position = pairs.get(open_position)
    if close_position is not None:
        owner_ranges.append((open_position, close_position))

instance_pattern = re.compile(r"\bHookCodeEditor\s*\{")
for match in instance_pattern.finditer(source):
    open_position = source.rfind("{", match.start(), match.end())
    close_position = pairs.get(open_position)
    line = source.count("\n", 0, match.start()) + 1
    if close_position is None:
        print(f"{relative}:{line}: {message}")
        continue
    if re.search(r"\b(?:visible|opacity)\s*:", source[open_position:close_position]):
        print(f"{relative}:{line}: {message}; visible/opacity is not lifecycle")
        continue
    if not any(start < open_position < end for start, end in owner_ranges):
        print(f"{relative}:{line}: {message}")

frame_match = re.search(
    r"\bif\s+root\.editor-active\s*:\s*editor-frame\s*:=\s*Rectangle\s*\{",
    source,
)
if frame_match is None:
    print(f"{relative}: Hook editor frame is missing")
else:
    frame_open = source.rfind("{", frame_match.start(), frame_match.end())
    frame_close = pairs.get(frame_open)
    if frame_close is None:
        print(f"{relative}: Hook editor frame has no closing brace")
    else:
        frame_body = source[frame_open:frame_close]
        editor_instance = re.search(r"\bHookCodeEditor\s*\{", frame_body)
        direct_prefix = frame_body if editor_instance is None else frame_body[:editor_instance.start()]
        if re.search(r"\bclip\s*:\s*(?!false\b)", direct_prefix):
            line = source.count("\n", 0, frame_open) + 1
            print(
                f"{relative}:{line}: Hook editor frame must not round-clip "
                "the document subtree"
            )
PY
)
if [ -n "$hook_editor_lifecycle_violations" ]; then
    echo "UI policy: heavy Hook editor lifecycle must be conditionally owned:" >&2
    echo "$hook_editor_lifecycle_violations" >&2
    failed=1
fi
builtin_tooltips=$(rg -n '(^|[^A-Za-z0-9_-])Tooltip[[:space:]]*\{' \
    ui --glob '*.slint' || true)
if [ -n "$builtin_tooltips" ]; then
    echo "UI policy: built-in/local Tooltip surfaces are forbidden:" >&2
    echo "$builtin_tooltips" >&2
    failed=1
fi

# The application Preview host changes geometry for PR-side context. Mounting a
# second panel would duplicate the heavy Markdown delegate tree and layout bridge.
preview_panel_instances=$(rg -c 'GitHubArtifactPreviewPanel[[:space:]]*\{' ui/main.slint || true)
if [ "$preview_panel_instances" -ne 1 ]; then
    echo "UI policy: ui/main.slint must mount exactly one Artifact Preview panel (found $preview_panel_instances)" >&2
    failed=1
fi

if [ "$failed" -ne 0 ]; then
    exit 1
fi

echo "UI policy checks passed."
