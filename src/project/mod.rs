//! Project feature composition façade.

mod controller;
mod logo;
mod model;
mod overview;
mod storage;

pub(crate) use controller::setup_project;
pub(crate) use logo::resolve_project_logo_image;
pub(crate) use model::ProjectRepositoryMetadata;
pub(crate) use storage::load_repository_metadata;
