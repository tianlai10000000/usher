//! Project feature composition façade.

mod benchmark;
mod controller;
mod hook_selection;
mod logo;
mod logo_loader;
mod logo_prepare;
mod logo_requests;
mod model;
mod overview;
mod projection;
mod state;
mod storage;

pub(crate) use controller::setup_project;
pub(crate) use hook_selection::refresh_hook_choices;
pub(crate) use logo::resolve_project_logo_image;
pub(crate) use model::ProjectRepositoryMetadata;
pub(crate) use state::hook_selected;
pub(crate) use state::load_repository_metadata;

pub(crate) fn sync_language(ui: &crate::MainWindow) {
    controller::sync_language(ui);
    refresh_hook_choices(ui);
}
