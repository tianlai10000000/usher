use crate::{MainWindow, Route};
use slint::Weak;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const BREADCRUMB_HIDDEN_ROUTES: &[Route] = &[Route::Chat, Route::PrReview];
const MAX_ROUTE_HISTORY: usize = 64;
// Order must match `GitHubNavigationRail` items in `ui/main.slint`.
const NAVIGATION_ROUTES: &[Route] = &[
    Route::Home,
    Route::Chat,
    Route::Project,
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
        show_navigation_drawer: route != Route::PrReview,
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

pub fn route_for_navigation_index(index: i32) -> Option<Route> {
    usize::try_from(index)
        .ok()
        .and_then(|idx| NAVIGATION_ROUTES.get(idx).copied())
}

#[derive(Clone)]
pub struct Router {
    window: Weak<MainWindow>,
    history: Arc<Mutex<NavigationHistory>>,
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
    entries: Vec<Route>,
}

impl NavigationHistory {
    fn record_forward(&mut self, current: Route, target: Route) {
        if current != target && self.entries.last().copied() != Some(current) {
            if self.entries.len() == MAX_ROUTE_HISTORY {
                self.entries.remove(0);
            }
            self.entries.push(current);
        }
    }

    fn take_back_target(&mut self, current: Route) -> Option<Route> {
        while self.entries.last().copied() == Some(current) {
            self.entries.pop();
        }
        self.entries.pop()
    }

    fn previous_or(&self, fallback: Route) -> Route {
        self.entries.last().copied().unwrap_or(fallback)
    }

    fn contains_workflow_route(&self) -> bool {
        self.entries.iter().copied().any(is_workflow_route)
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
            let previous = win.get_current_route();
            if previous != route {
                if let Ok(mut history) = self.history.lock() {
                    history.record_forward(previous, route);
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

    fn apply_transition(&self, win: &MainWindow, previous: Route, route: Route) -> RouteUiState {
        win.set_current_route(route);
        let history_top = self
            .history
            .lock()
            .ok()
            .map(|history| history.previous_or(route))
            .unwrap_or(route);
        win.set_previous_route(history_top);
        let state = Self::apply_route_ui_state(win, route);

        // Keep Workflow/PR state while a route remains reachable through Back.
        // Once navigation leaves the workflow domain with no workflow entry in
        // history, its large logs and models can be released.
        let workflow_reachable = is_workflow_route(route)
            || self
                .history
                .lock()
                .map(|history| history.contains_workflow_route())
                .unwrap_or(false);
        (self.lifecycle_hook)(
            win,
            RouteLifecycleEvent {
                previous,
                current: route,
                workflow_became_unreachable: is_workflow_route(previous) && !workflow_reachable,
            },
        );
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
        MAX_ROUTE_HISTORY, NavigationHistory, is_workflow_route, route_for_navigation_index,
        route_ui_state,
    };
    use crate::Route;

    #[test]
    fn full_window_review_stays_in_workflow_domain_but_job_run_is_global() {
        let state = route_ui_state(Route::PrReview);
        assert!(!state.show_breadcrumb);
        assert!(!state.show_navigation_drawer);
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
    fn ui_components_is_a_primary_navigation_route() {
        let state = route_ui_state(Route::UiComponents);
        assert!(state.show_breadcrumb);
        assert!(state.show_navigation_drawer);
        assert!(state.show_page_scroll_view);
        assert_eq!(state.navigation_current_index, 3);
        assert_eq!(route_for_navigation_index(3), Some(Route::UiComponents));
    }

    #[test]
    fn route_history_returns_from_utility_pages_without_creating_a_bounce() {
        let mut history = NavigationHistory::default();
        history.record_forward(Route::Home, Route::Project);
        history.record_forward(Route::Project, Route::Workflow);
        history.record_forward(Route::Workflow, Route::PrReview);
        history.record_forward(Route::PrReview, Route::Settings);

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
        history.record_forward(Route::Home, Route::Home);
        assert_eq!(history.take_back_target(Route::Home), None);
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
            history.record_forward(current, target);
        }
        assert_eq!(history.entries.len(), MAX_ROUTE_HISTORY);
    }
}
