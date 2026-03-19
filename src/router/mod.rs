use crate::{MainWindow, Route};
use slint::{ModelRc, VecModel, Weak};

const BREADCRUMB_HIDDEN_ROUTES: &[Route] = &[Route::Chat, Route::Login];
const NAVIGATION_ROUTES: &[Route] = &[
    // Route::Home,
    // Route::Search,
    // Route::Notification,
    // Route::Toast,
    // Route::Dialog,
    // Route::Tools,
    Route::Chat,
    Route::Project,
    // Route::Workflow,
    // Route::List,
    // Route::Detail,
    // Route::Settings,
    // Route::About,
];

#[derive(Debug, Clone, Copy)]
pub struct RouteUiState {
    pub show_breadcrumb: bool,
    pub show_navigation_drawer: bool,
    pub navigation_current_index: i32,
    pub chat_route_visible: bool,
    pub dialog_route_visible: bool,
    pub show_page_scroll_view: bool,
}

pub fn route_ui_state(route: Route) -> RouteUiState {
    let chat_route_visible = route == Route::Chat;
    let dialog_route_visible = route == Route::Dialog;
    RouteUiState {
        show_breadcrumb: !BREADCRUMB_HIDDEN_ROUTES.contains(&route),
        show_navigation_drawer: route != Route::Login,
        navigation_current_index: navigation_index_for_route(route),
        chat_route_visible,
        dialog_route_visible,
        show_page_scroll_view: !chat_route_visible && !dialog_route_visible,
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
}

impl Router {
    pub fn new(window: Weak<MainWindow>) -> Self {
        Self { window }
    }

    pub fn apply_route_ui_state(window: &MainWindow, route: Route) -> RouteUiState {
        let state = route_ui_state(route);
        window.set_show_breadcrumb(state.show_breadcrumb);
        window.set_show_navigation_drawer(state.show_navigation_drawer);
        window.set_navigation_current_index(state.navigation_current_index);
        window.set_show_chat_page(state.chat_route_visible);
        window.set_show_dialog_page(state.dialog_route_visible);
        window.set_show_page_scroll_view(state.show_page_scroll_view);
        state
    }

    pub fn navigate(&self, route: Route) -> Option<RouteUiState> {
        self.window.upgrade().map(|win| {
            let previous = win.get_current_route();
            win.set_current_route(route);
            let state = Self::apply_route_ui_state(&win, route);

            // Proactively drop large workflow logs/details when leaving the workflow page.
            // This reduces resident memory after visiting Tag/CD tabs (git command logs can be large).
            if previous == Route::Workflow && route != Route::Workflow {
                win.set_workflow_tag_result("".into());
                win.set_workflow_tag_steps(ModelRc::new(VecModel::from(Vec::new())));
                win.set_workflow_cd_summary("".into());
                win.set_workflow_cd_build_detail("".into());
                win.set_workflow_cd_test_detail("".into());
                win.set_workflow_cd_deploy_detail("".into());
                win.set_workflow_cd_verify_detail("".into());
            }
            // Can add other global side effects here, such as:
            // - Record route history
            // - Send analytics events
            // - Lazy load certain resources, etc.
            log::debug!("→ Navigate to: {:?}", route);
            state
        })
    }
}
