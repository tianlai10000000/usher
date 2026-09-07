//! Tag workflow composition façade.

mod benchmark;
mod command_output;
mod commands;
mod controller;
mod version;

pub(crate) use controller::{bind_tag, refresh_tag_context};
