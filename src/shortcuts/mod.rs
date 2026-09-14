//! Application-wide keyboard shortcut registry and dispatcher.
//!
//! Slint owns raw window key delivery. This module owns normalization,
//! conflict-free matching, scope checks, user-facing labels, and command
//! dispatch so feature modules do not duplicate modifier or platform rules.

use std::fmt;
use std::sync::OnceLock;

use crate::{MainWindow, Route};
use slint::{ComponentHandle, Timer};

/// Stable command identifiers used by buttons, menus, tests, and key bindings.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CommandId {
    ToggleSidebar,
    ToggleTerminal,
    NavigateBack,
    NavigateForward,
    OpenSettings,
}

/// Where a binding is eligible to run.
///
/// Route and feature scopes are intentionally part of the registry contract so
/// future page commands can use the same resolver without adding another key
/// listener. The first registry entries are application-wide shell commands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ShortcutScope {
    Application,
    Route(Route),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Modifiers {
    primary: bool,
    alt: bool,
    shift: bool,
    secondary: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KeyStroke {
    key: String,
    modifiers: Modifiers,
}

impl KeyStroke {
    fn from_slint(text: &str, primary: bool, alt: bool, shift: bool, secondary: bool) -> Self {
        Self {
            key: normalize_key(text),
            modifiers: Modifiers {
                primary,
                alt,
                shift,
                secondary,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ShortcutBinding {
    command: CommandId,
    scope: ShortcutScope,
    key: &'static str,
    modifiers: Modifiers,
}

impl ShortcutBinding {
    const fn primary(command: CommandId, key: &'static str) -> Self {
        Self {
            command,
            scope: ShortcutScope::Application,
            key,
            modifiers: Modifiers {
                primary: true,
                alt: false,
                shift: false,
                secondary: false,
            },
        }
    }

    const fn for_route(mut self, route: Route) -> Self {
        self.scope = ShortcutScope::Route(route);
        self
    }

    fn matches(&self, stroke: &KeyStroke, context: ShortcutContext) -> bool {
        self.key == stroke.key
            && self.modifiers == stroke.modifiers
            && match self.scope {
                ShortcutScope::Application => true,
                ShortcutScope::Route(route) => route == context.route,
            }
    }
}

const DEFAULT_BINDINGS: &[ShortcutBinding] = &[
    ShortcutBinding::primary(CommandId::ToggleSidebar, "b"),
    // Mirrors Codex's application bottom-panel command. Usher's bottom panel
    // is the project-aware terminal drawer and belongs to project routes.
    ShortcutBinding::primary(CommandId::ToggleTerminal, "j").for_route(Route::Workflow),
    ShortcutBinding::primary(CommandId::ToggleTerminal, "j").for_route(Route::Project),
    ShortcutBinding::primary(CommandId::NavigateBack, "["),
    ShortcutBinding::primary(CommandId::NavigateForward, "]"),
    ShortcutBinding::primary(CommandId::OpenSettings, ","),
];

static APPLICATION_REGISTRY: OnceLock<ShortcutRegistry> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
struct ShortcutContext {
    route: Route,
    modal_overlay_visible: bool,
}

#[derive(Debug, PartialEq)]
enum RegistryError {
    DuplicateBinding {
        first: CommandId,
        second: CommandId,
        scope: ShortcutScope,
        key: &'static str,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBinding {
                first,
                second,
                scope,
                key,
            } => write!(
                formatter,
                "shortcut {key:?} is assigned to both {first:?} and {second:?} in {scope:?}"
            ),
        }
    }
}

#[derive(Debug)]
struct ShortcutRegistry {
    bindings: &'static [ShortcutBinding],
}

impl ShortcutRegistry {
    fn application() -> Result<Self, RegistryError> {
        Self::new(DEFAULT_BINDINGS)
    }

    fn new(bindings: &'static [ShortcutBinding]) -> Result<Self, RegistryError> {
        for (index, binding) in bindings.iter().enumerate() {
            if let Some(conflict) = bindings[index + 1..].iter().find(|candidate| {
                scopes_overlap(candidate.scope, binding.scope)
                    && candidate.key == binding.key
                    && candidate.modifiers == binding.modifiers
            }) {
                return Err(RegistryError::DuplicateBinding {
                    first: binding.command,
                    second: conflict.command,
                    scope: binding.scope,
                    key: binding.key,
                });
            }
        }
        Ok(Self { bindings })
    }

    fn resolve(&self, stroke: &KeyStroke, context: ShortcutContext) -> Option<CommandId> {
        if context.modal_overlay_visible {
            return None;
        }

        self.bindings
            .iter()
            .find(|binding| binding.matches(stroke, context))
            .map(|binding| binding.command)
    }

    fn binding(&self, command: CommandId) -> Option<&ShortcutBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.command == command)
    }
}

fn scopes_overlap(first: ShortcutScope, second: ShortcutScope) -> bool {
    match (first, second) {
        (ShortcutScope::Application, _) | (_, ShortcutScope::Application) => true,
        (ShortcutScope::Route(first), ShortcutScope::Route(second)) => first == second,
    }
}

/// Start the visible shortcut initialization pipeline after the window exists.
///
/// Completion means all registry validation, Slint callback binding, and the
/// platform input bridge have succeeded. The status bar remains in a failure
/// state if that contract cannot be satisfied.
pub(crate) fn start_initialization(ui: &MainWindow) {
    ui.set_shortcut_initialization_visible(true);
    ui.set_shortcut_initialization_failed(false);
    ui.set_shortcut_initialization_progress(0.1);
    ui.set_shortcut_initialization_status(crate::i18n::tr("shortcuts.loading").into_owned().into());
    ui.set_shortcut_initialization_detail(
        shortcut_status_detail(
            "shortcuts.status_loading",
            "shortcuts.status_waiting",
            "shortcuts.status_waiting",
        )
        .into(),
    );

    let ui_weak = ui.as_weak();
    Timer::single_shot(std::time::Duration::from_millis(16), move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if let Err(error) = install_registry_and_callbacks(&ui) {
            fail_initialization(&ui, &format!("Shortcuts unavailable: {error}"));
            return;
        }

        ui.set_shortcut_initialization_progress(0.6);
        ui.set_shortcut_initialization_status(
            crate::i18n::tr("shortcuts.loading_input")
                .into_owned()
                .into(),
        );
        ui.set_shortcut_initialization_detail(
            shortcut_status_detail(
                "shortcuts.status_ready",
                "shortcuts.status_loading",
                "shortcuts.status_waiting",
            )
            .into(),
        );
        schedule_platform_bridge(ui.as_weak(), 0);
    });
}

fn install_registry_and_callbacks(ui: &MainWindow) -> Result<(), RegistryError> {
    ensure_application_registry()?;
    ui.set_shortcut_toggle_sidebar_label(binding_label(CommandId::ToggleSidebar).into());
    ui.set_shortcut_toggle_terminal_label(binding_label(CommandId::ToggleTerminal).into());
    ui.set_shortcut_navigate_back_label(binding_label(CommandId::NavigateBack).into());
    ui.set_shortcut_navigate_forward_label(binding_label(CommandId::NavigateForward).into());
    ui.set_shortcut_open_settings_label(binding_label(CommandId::OpenSettings).into());
    let ui_weak = ui.as_weak();
    ui.on_app_shortcut_key_pressed(move |text, primary, alt, shift, secondary| {
        let Some(ui) = ui_weak.upgrade() else {
            return false;
        };
        handle_key_pressed(&ui, &text, primary, alt, shift, secondary)
    });
    Ok(())
}

fn schedule_platform_bridge(ui_weak: slint::Weak<MainWindow>, attempt: u8) {
    Timer::single_shot(std::time::Duration::from_millis(50), move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if crate::platform::install_shortcut_input_bridge(&ui) {
            ui.set_shortcut_initialization_progress(1.0);
            ui.set_shortcut_initialization_status(
                crate::i18n::tr("status.shortcuts_ready")
                    .into_owned()
                    .into(),
            );
            ui.set_shortcut_initialization_detail(
                shortcut_status_detail(
                    "shortcuts.status_ready",
                    "shortcuts.status_ready",
                    "shortcuts.status_ready",
                )
                .into(),
            );
            log::info!("[Shortcut] Initialization complete");
            return;
        }

        const MAX_ATTEMPTS: u8 = 8;
        if attempt + 1 >= MAX_ATTEMPTS {
            fail_initialization(&ui, "Shortcuts unavailable: input bridge failed");
            return;
        }
        ui.set_shortcut_initialization_progress(0.6 + f32::from(attempt + 1) * 0.04);
        schedule_platform_bridge(ui.as_weak(), attempt + 1);
    });
}

fn fail_initialization(ui: &MainWindow, message: &str) {
    ui.set_shortcut_initialization_failed(true);
    ui.set_shortcut_initialization_status(
        crate::i18n::tr("shortcuts.unavailable").into_owned().into(),
    );
    ui.set_shortcut_initialization_detail(
        shortcut_status_detail(
            "shortcuts.status_failed",
            "shortcuts.status_failed",
            "shortcuts.status_failed",
        )
        .into(),
    );
    log::error!("[Shortcut] {message}");
}

fn shortcut_status_detail(
    registry_status_key: &str,
    platform_status_key: &str,
    command_status_key: &str,
) -> String {
    let registry_status = crate::i18n::tr(registry_status_key);
    let platform_status = crate::i18n::tr(platform_status_key);
    let command_status = crate::i18n::tr(command_status_key);
    let toggle_sidebar = binding_label(CommandId::ToggleSidebar);
    let toggle_terminal = binding_label(CommandId::ToggleTerminal);
    let back = binding_label(CommandId::NavigateBack);
    let forward = binding_label(CommandId::NavigateForward);
    let settings = binding_label(CommandId::OpenSettings);
    crate::i18n::tr_args(
        "shortcuts.detail",
        &[
            ("registry_status", registry_status.as_ref()),
            ("platform_status", platform_status.as_ref()),
            ("command_status", command_status.as_ref()),
            ("toggle_sidebar_binding", toggle_sidebar.as_str()),
            ("toggle_terminal_binding", toggle_terminal.as_str()),
            ("back_binding", back.as_str()),
            ("forward_binding", forward.as_str()),
            ("settings_binding", settings.as_str()),
        ],
    )
}

/// Resolve one platform-normalized key press through the application registry.
///
/// Slint's public `KeyEvent` currently exposes logical text rather than a
/// physical key. Most platforms can forward that text directly. macOS uses
/// this same entry point after its adapter converts `NSEvent.keyCode`, keeping
/// punctuation shortcuts independent of the active input method.
pub(crate) fn handle_key_pressed(
    ui: &MainWindow,
    text: &str,
    primary: bool,
    alt: bool,
    shift: bool,
    secondary: bool,
) -> bool {
    if ui.get_settings_hook_dialog_open()
        && ui.invoke_settings_hook_dialog_key_pressed(text.into(), primary, alt, shift, secondary)
    {
        return true;
    }
    let stroke = KeyStroke::from_slint(text, primary, alt, shift, secondary);
    let context = ShortcutContext {
        route: ui.get_current_route(),
        modal_overlay_visible: ui.get_modal_overlay_visible(),
    };
    let Some(command) = application_registry().resolve(&stroke, context) else {
        return false;
    };

    dispatch(ui, command)
}

fn dispatch(ui: &MainWindow, command: CommandId) -> bool {
    match command {
        CommandId::ToggleSidebar => ui.invoke_toggle_navigation_drawer(),
        CommandId::ToggleTerminal => ui.invoke_shell_drawer_toggle(),
        CommandId::NavigateBack if ui.get_navigation_can_go_back() => ui.invoke_navigate_back(),
        CommandId::NavigateForward if ui.get_navigation_can_go_forward() => {
            ui.invoke_navigate_forward()
        }
        CommandId::OpenSettings if ui.get_current_route() != Route::Settings => {
            ui.invoke_navigate(Route::Settings)
        }
        CommandId::NavigateBack | CommandId::NavigateForward | CommandId::OpenSettings => {
            return false;
        }
    }
    log::debug!("[Shortcut] Dispatched {command:?}");
    true
}

/// Return the platform-native binding label for tooltips and future menus.
pub(crate) fn display_binding(command: CommandId) -> Option<String> {
    let binding = application_registry().binding(command)?;
    Some(format_binding(binding))
}

fn binding_label(command: CommandId) -> String {
    display_binding(command).unwrap_or_default()
}

fn application_registry() -> &'static ShortcutRegistry {
    ensure_application_registry()
        .unwrap_or_else(|error| panic!("invalid application shortcut registry: {error}"))
}

fn ensure_application_registry() -> Result<&'static ShortcutRegistry, RegistryError> {
    if let Some(registry) = APPLICATION_REGISTRY.get() {
        return Ok(registry);
    }
    let registry = ShortcutRegistry::application()?;
    let _ = APPLICATION_REGISTRY.set(registry);
    Ok(APPLICATION_REGISTRY
        .get()
        .expect("shortcut registry must be set after successful validation"))
}

fn format_binding(binding: &ShortcutBinding) -> String {
    #[cfg(target_os = "macos")]
    {
        let mut label = String::new();
        if binding.modifiers.secondary {
            label.push('⌃');
        }
        if binding.modifiers.alt {
            label.push('⌥');
        }
        if binding.modifiers.shift {
            label.push('⇧');
        }
        if binding.modifiers.primary {
            label.push('⌘');
        }
        label.push_str(&binding.key.to_uppercase());
        label
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mut parts = Vec::new();
        if binding.modifiers.primary {
            parts.push("Ctrl".to_string());
        }
        if binding.modifiers.secondary {
            parts.push("Meta".to_string());
        }
        if binding.modifiers.alt {
            parts.push("Alt".to_string());
        }
        if binding.modifiers.shift {
            parts.push("Shift".to_string());
        }
        parts.push(binding.key.to_uppercase());
        parts.join("+")
    }
}

fn normalize_key(text: &str) -> String {
    text.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Modifiers = Modifiers {
        primary: true,
        alt: false,
        shift: false,
        secondary: false,
    };

    fn context(route: Route) -> ShortcutContext {
        ShortcutContext {
            route,
            modal_overlay_visible: false,
        }
    }

    #[test]
    fn resolves_primary_shortcuts_case_insensitively() {
        let registry = ShortcutRegistry::application().unwrap();
        let stroke = KeyStroke::from_slint("B", true, false, false, false);

        assert_eq!(
            registry.resolve(&stroke, context(Route::Home)),
            Some(CommandId::ToggleSidebar)
        );
    }

    #[test]
    fn rejects_extra_modifiers_and_unmodified_keys() {
        let registry = ShortcutRegistry::application().unwrap();

        assert_eq!(
            registry.resolve(
                &KeyStroke::from_slint("b", true, false, true, false),
                context(Route::Home)
            ),
            None
        );
        assert_eq!(
            registry.resolve(
                &KeyStroke::from_slint("b", false, false, false, false),
                context(Route::Home)
            ),
            None
        );
    }

    #[test]
    fn blocks_application_shortcuts_while_a_modal_overlay_owns_input() {
        let registry = ShortcutRegistry::application().unwrap();
        let stroke = KeyStroke::from_slint("b", true, false, false, false);

        assert_eq!(
            registry.resolve(
                &stroke,
                ShortcutContext {
                    route: Route::Home,
                    modal_overlay_visible: true,
                }
            ),
            None
        );
    }

    #[test]
    fn route_scopes_do_not_leak_to_other_pages() {
        static ROUTE_BINDINGS: &[ShortcutBinding] =
            &[ShortcutBinding::primary(CommandId::OpenSettings, "k").for_route(Route::Project)];
        let registry = ShortcutRegistry::new(ROUTE_BINDINGS).unwrap();
        let stroke = KeyStroke {
            key: "k".to_string(),
            modifiers: PRIMARY,
        };

        assert_eq!(
            registry.resolve(&stroke, context(Route::Project)),
            Some(CommandId::OpenSettings)
        );
        assert_eq!(registry.resolve(&stroke, context(Route::Home)), None);
    }

    #[test]
    fn terminal_binding_matches_both_project_routes_only() {
        let registry = ShortcutRegistry::application().unwrap();
        let stroke = KeyStroke::from_slint("j", true, false, false, false);

        assert_eq!(
            registry.resolve(&stroke, context(Route::Workflow)),
            Some(CommandId::ToggleTerminal)
        );
        assert_eq!(
            registry.resolve(&stroke, context(Route::Project)),
            Some(CommandId::ToggleTerminal)
        );
        assert_eq!(registry.resolve(&stroke, context(Route::Home)), None);
    }

    #[test]
    fn duplicate_bindings_are_rejected_at_startup() {
        static DUPLICATES: &[ShortcutBinding] = &[
            ShortcutBinding::primary(CommandId::ToggleSidebar, "b"),
            ShortcutBinding::primary(CommandId::OpenSettings, "b"),
        ];

        assert!(matches!(
            ShortcutRegistry::new(DUPLICATES),
            Err(RegistryError::DuplicateBinding { .. })
        ));
    }

    #[test]
    fn application_bindings_conflict_with_the_same_route_binding() {
        static DUPLICATES: &[ShortcutBinding] = &[
            ShortcutBinding::primary(CommandId::ToggleSidebar, "b"),
            ShortcutBinding::primary(CommandId::OpenSettings, "b").for_route(Route::Project),
        ];

        assert!(matches!(
            ShortcutRegistry::new(DUPLICATES),
            Err(RegistryError::DuplicateBinding { .. })
        ));
    }

    #[test]
    fn binding_labels_are_platform_native() {
        let registry = ShortcutRegistry::application().unwrap();
        let binding = registry.binding(CommandId::ToggleSidebar).unwrap();

        #[cfg(target_os = "macos")]
        assert_eq!(format_binding(binding), "⌘B");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(format_binding(binding), "Ctrl+B");
    }
}
