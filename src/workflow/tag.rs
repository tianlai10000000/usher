//! Tag workflow composition façade.

mod commands;
mod controller;
mod version;

pub(crate) use controller::{bind_tag, refresh_tag_context};
