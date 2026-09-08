//! Tag workflow composition façade.

mod benchmark;
mod command_output;
mod commands;
mod controller;
mod version;

pub(crate) use commands::default_create_tag_steps;
pub(crate) use controller::{bind_tag, refresh_tag_context};
pub(crate) use version::VersionBump;
