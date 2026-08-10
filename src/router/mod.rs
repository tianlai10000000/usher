use crate::{MainWindow, Route};
use slint::Weak;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const BREADCRUMB_HIDDEN_ROUTES: &[Route] = &[Route::Chat, Route::PrReview];
pub(crate) const MAX_ROUTE_HISTORY: usize = 64;
// Order must match `GitHubNavigationRail` items in `ui/main.slint`.
const NAVIGATION_ROUTES: &[Route] = &[
    Route::Home,
    Route::Chat,
    Route::Project,
    #[cfg(feature = "ui-components")]
    Route::UiComponents,
];

#[derive(Debug, Clone, Copy)]
pub struct RouteUiState {
    pub show_breadcrumb: bool,
    pub show_navigation_drawer: bool,
    pub navigation_current_index: i32,
    pub chat_route_visible: bool,
    pub show_page_scroll_view: bool,
}

pub fn route_ui_state(route: Route) -> RouteUiState {
    let chat_route_visible = route == Route::Chat;
    RouteUiState {
        show_breadcrumb: !BREADCRUMB_HIDDEN_ROUTES.contains(&route),
        // PR review remains in the Workflow shell and must preserve the
        // user's existing docked-sidebar state. The dedicated page still owns
        // its content scrolling, but it no longer forces navigation closed.
        show_navigation_drawer: true,
        navigation_current_index: navigation_index_for_route(route),
        chat_route_visible,
        show_page_scroll_view: !chat_route_visible && route != Route::PrReview,
    }
}

fn navigation_index_for_route(route: Route) -> i32 {
    NAVIGATION_ROUTES
        .iter()
        .position(|&nav_route| nav_route == route)
        .map_or(-1, |index| index as i32)
}

fn available_route(route: Route) -> Route {
    #[cfg(not(feature = "ui-components"))]
    if route == Route::UiComponents {
        return Route::Home;
    }

    route
}

pub fn route_for_navigation_index(index: i32) -> Option<Route> {
    usize::try_from(index)
        .ok()
        .and_then(|idx| NAVIGATION_ROUTES.get(idx).copied())
}

#[derive(Clone)]
pub struct Router {
    window: Weak<MainWindow>,
    history: Arc<Mutex<NavigationHistory>>,
    workflow_reachable_ab: Option<Arc<Mutex<bool>>>,
    lifecycle_hook: Rc<dyn Fn(&MainWindow, RouteLifecycleEvent)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RouteLifecycleEvent {
    pub(crate) previous: Route,
    pub(crate) current: Route,
    pub(crate) workflow_became_unreachable: bool,
}

#[derive(Debug, Default)]
struct NavigationHistory {
    back_entries: Vec<Route>,
    forward_entries: Vec<Route>,
}

impl NavigationHistory {
    fn push_bounded(entries: &mut Vec<Route>, route: Route) {
        if entries.last().copied() == Some(route) {
            return;
        }
        if entries.len() == MAX_ROUTE_HISTORY {
            entries.remove(0);
        }
        entries.push(route);
    }

    fn take_distinct_target(entries: &mut Vec<Route>, current: Route) -> Option<Route> {
        while entries.last().copied() == Some(current) {
            entries.pop();
        }
        entries.pop()
    }

    fn record_navigation(&mut self, current: Route, target: Route) {
        if current == target {
            return;
        }
        Self::push_bounded(&mut self.back_entries, current);
        self.forward_entries.clear();
    }

    fn take_back_target(&mut self, current: Route) -> Option<Route> {
        let target = Self::take_distinct_target(&mut self.back_entries, current)?;
        Self::push_bounded(&mut self.forward_entries, current);
        Some(target)
    }

    fn take_forward_target(&mut self, current: Route) -> Option<Route> {
        let target = Self::take_distinct_target(&mut self.forward_entries, current)?;
        Self::push_bounded(&mut self.back_entries, current);
        Some(target)
    }

    fn previous_or(&self, fallback: Route) -> Route {
        self.back_entries.last().copied().unwrap_or(fallback)
    }

    fn can_go_back(&self) -> bool {
        !self.back_entries.is_empty()
    }

    fn can_go_forward(&self) -> bool {
        !self.forward_entries.is_empty()
    }

    fn contains_workflow_route(&self) -> bool {
        self.back_entries
            .iter()
            .chain(self.forward_entries.iter())
            .copied()
            .any(is_workflow_route)
    }

    fn workflow_reachable_from(&self, current: Route) -> bool {
        is_workflow_route(current) || self.contains_workflow_route()
    }
}

impl Router {
    pub fn new(
        window: Weak<MainWindow>,
        lifecycle_hook: Rc<dyn Fn(&MainWindow, RouteLifecycleEvent)>,
    ) -> Self {
        Self {
            window,
            history: Arc::new(Mutex::new(NavigationHistory::default())),
            workflow_reachable_ab: std::env::var("USHER_MEMORY_WORKFLOW_LIFECYCLE")
                .ok()
                .filter(|value| matches!(value.as_str(), "retain" | "clear"))
                .map(|_| Arc::new(Mutex::new(false))),
            lifecycle_hook,
        }
    }

    pub fn apply_route_ui_state(window: &MainWindow, route: Route) -> RouteUiState {
        let state = route_ui_state(route);
        window.set_show_breadcrumb(state.show_breadcrumb);
        window.set_show_navigation_drawer(state.show_navigation_drawer);
        window.set_navigation_current_index(state.navigation_current_index);
        window.set_show_chat_page(state.chat_route_visible);
        window.set_show_page_scroll_view(state.show_page_scroll_view);
        state
    }

    pub fn navigate(&self, route: Route) -> Option<RouteUiState> {
        self.window.upgrade().map(|win| {
            let route = available_route(route);
            let previous = win.get_current_route();
            if previous != route {
                if let Ok(mut history) = self.history.lock() {
                    history.record_navigation(previous, route);
                }
            }
            self.apply_transition(&win, previous, route)
        })
    }

    pub fn back(&self) -> Option<RouteUiState> {
        self.window.upgrade().and_then(|win| {
            let current = win.get_current_route();
            let target = self
                .history
                .lock()
                .ok()
                .and_then(|mut history| history.take_back_target(current))?;
            Some(self.apply_transition(&win, current, target))
        })
    }

    pub fn forward(&self) -> Option<RouteUiState> {
        self.window.upgrade().and_then(|win| {
            let current = win.get_current_route();
            let target = self
                .history
                .lock()
                .ok()
                .and_then(|mut history| history.take_forward_target(current))?;
            Some(self.apply_transition(&win, current, target))
        })
    }

    fn apply_transition(&self, win: &MainWindow, previous: Route, route: Route) -> RouteUiState {
        win.set_current_route(route);
        let (history_top, can_go_back, can_go_forward) = self
            .history
            .lock()
            .ok()
            .map(|history| {
                (
                    history.previous_or(route),
                    history.can_go_back(),
                    history.can_go_forward(),
                )
            })
            .unwrap_or((route, false, false));
        win.set_previous_route(history_top);
        win.set_navigation_can_go_back(can_go_back);
        win.set_navigation_can_go_forward(can_go_forward);
        let state = Self::apply_route_ui_state(win, route);

        // Keep Workflow/PR state while a route remains reachable through Back.
        // Once navigation leaves the workflow domain with no workflow entry in
        // history, its large logs and models can be released.
        let workflow_reachable = self
            .history
            .lock()
            .map(|history| history.workflow_reachable_from(route))
            .unwrap_or_else(|_| is_workflow_route(route));
        let workflow_became_unreachable = self.workflow_reachable_ab.as_ref().map_or_else(
            || is_workflow_route(previous) && !workflow_reachable,
            |reachable| {
                reachable
                    .lock()
                    .map(|mut previous_reachable| {
                        let became_unreachable = *previous_reachable && !workflow_reachable;
                        *previous_reachable = workflow_reachable;
                        became_unreachable
                    })
                    .unwrap_or(false)
            },
        );
        (self.lifecycle_hook)(
            win,
            RouteLifecycleEvent {
                previous,
                current: route,
                workflow_became_unreachable,
            },
        );
        crate::platform::update_titlebar_navigation_controls(win);
        log::debug!("→ Navigate to: {:?}", route);
        state
    }
}

fn is_workflow_route(route: Route) -> bool {
    matches!(route, Route::Workflow | Route::PrReview)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_ROUTE_HISTORY, NavigationHistory, available_route, is_workflow_route,
        route_for_navigation_index, route_ui_state,
    };
    use crate::Route;

    #[test]
    fn full_window_review_stays_in_workflow_domain_but_job_run_is_global() {
        let state = route_ui_state(Route::PrReview);
        assert!(!state.show_breadcrumb);
        assert!(state.show_navigation_drawer);
        assert!(!state.show_page_scroll_view);
        let job_run = route_ui_state(Route::JobRun);
        assert!(job_run.show_breadcrumb);
        assert!(job_run.show_navigation_drawer);
        assert!(job_run.show_page_scroll_view);
        assert!(is_workflow_route(Route::Workflow));
        assert!(!is_workflow_route(Route::JobRun));
        assert!(is_workflow_route(Route::PrReview));
        assert!(!is_workflow_route(Route::Home));
    }

    #[test]
    #[cfg(feature = "ui-components")]
    fn ui_components_is_a_primary_navigation_route_when_enabled() {
        assert_eq!(available_route(Route::UiComponents), Route::UiComponents);
        let state = route_ui_state(Route::UiComponents);
        assert!(state.show_breadcrumb);
        assert!(state.show_navigation_drawer);
        assert!(state.show_page_scroll_view);
        assert_eq!(state.navigation_current_index, 3);
        assert_eq!(route_for_navigation_index(3), Some(Route::UiComponents));
    }

    #[test]
    #[cfg(not(feature = "ui-components"))]
    fn ui_components_is_not_a_primary_navigation_route_when_disabled() {
        assert_eq!(available_route(Route::UiComponents), Route::Home);
        let state = route_ui_state(Route::UiComponents);
        assert_eq!(state.navigation_current_index, -1);
        assert_eq!(route_for_navigation_index(3), None);
    }

    #[test]
    fn route_history_returns_from_utility_pages_without_creating_a_bounce() {
        let mut history = NavigationHistory::default();
        history.record_navigation(Route::Home, Route::Project);
        history.record_navigation(Route::Project, Route::Workflow);
        history.record_navigation(Route::Workflow, Route::PrReview);
        history.record_navigation(Route::PrReview, Route::Settings);

        assert_eq!(
            history.take_back_target(Route::Settings),
            Some(Route::PrReview)
        );
        assert_eq!(
            history.take_back_target(Route::PrReview),
            Some(Route::Workflow)
        );
        assert_eq!(
            history.take_back_target(Route::Workflow),
            Some(Route::Project)
        );
    }

    #[test]
    fn repeated_navigation_to_the_current_route_does_not_add_history() {
        let mut history = NavigationHistory::default();
        history.record_navigation(Route::Home, Route::Home);
        assert_eq!(history.take_back_target(Route::Home), None);
    }

    #[test]
    fn back_and_forward_move_between_independent_stacks() {
        let mut history = NavigationHistory::default();
        history.record_navigation(Route::Home, Route::Chat);
        history.record_navigation(Route::Chat, Route::Project);

        assert_eq!(history.take_back_target(Route::Project), Some(Route::Chat));
        assert!(history.can_go_back());
        assert!(history.can_go_forward());
        assert_eq!(
            history.take_forward_target(Route::Chat),
            Some(Route::Project)
        );
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());
    }

    #[test]
    fn direct_navigation_after_back_clears_forward_history() {
        let mut history = NavigationHistory::default();
        history.record_navigation(Route::Home, Route::Chat);
        history.record_navigation(Route::Chat, Route::Project);
        assert_eq!(history.take_back_target(Route::Project), Some(Route::Chat));

        history.record_navigation(Route::Chat, Route::Settings);
        assert!(!history.can_go_forward());
        assert_eq!(history.take_back_target(Route::Settings), Some(Route::Chat));
    }

    #[test]
    fn route_history_is_bounded() {
        let mut history = NavigationHistory::default();
        for index in 0..(MAX_ROUTE_HISTORY * 2) {
            let (current, target) = if index % 2 == 0 {
                (Route::Home, Route::Settings)
            } else {
                (Route::Settings, Route::Home)
            };
            history.record_navigation(current, target);
        }
        assert_eq!(history.back_entries.len(), MAX_ROUTE_HISTORY);
    }

    #[test]
    fn workflow_remains_reachable_while_back_or_forward_can_restore_it() {
        let mut history = NavigationHistory::default();
        history.record_navigation(Route::Home, Route::Workflow);
        history.record_navigation(Route::Workflow, Route::Settings);
        assert!(history.workflow_reachable_from(Route::Settings));

        assert_eq!(
            history.take_back_target(Route::Settings),
            Some(Route::Workflow)
        );
        assert!(history.workflow_reachable_from(Route::Workflow));
        assert_eq!(
            history.take_forward_target(Route::Workflow),
            Some(Route::Settings)
        );
        assert!(history.workflow_reachable_from(Route::Settings));
    }

    #[test]
    fn direct_navigation_after_back_can_make_workflow_unreachable() {
        let mut history = NavigationHistory::default();
        history.record_navigation(Route::Home, Route::Workflow);
        history.record_navigation(Route::Workflow, Route::Settings);
        assert_eq!(
            history.take_back_target(Route::Settings),
            Some(Route::Workflow)
        );
        assert_eq!(history.take_back_target(Route::Workflow), Some(Route::Home));
        assert!(history.workflow_reachable_from(Route::Home));

        history.record_navigation(Route::Home, Route::About);
        assert!(!history.workflow_reachable_from(Route::About));
    }

    #[test]
    fn bounded_history_eviction_can_make_workflow_unreachable() {
        let mut history = NavigationHistory::default();
        history.record_navigation(Route::Home, Route::PrReview);
        history.record_navigation(Route::PrReview, Route::About);
        assert!(history.workflow_reachable_from(Route::About));

        let mut current = Route::About;
        for index in 0..MAX_ROUTE_HISTORY {
            let target = if index % 2 == 0 {
                Route::Home
            } else {
                Route::About
            };
            history.record_navigation(current, target);
            current = target;
        }

        assert!(!history.workflow_reachable_from(current));
    }
}
