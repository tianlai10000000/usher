//! Tag workflow composition façade.

pub(crate) mod activity;
mod benchmark;
mod command_output;
mod commands;
pub(crate) mod controller;
mod execution;
mod initialization;
mod initialization_error;
mod progress;
pub(crate) mod state;
mod version;

pub(crate) use commands::default_create_tag_steps;
pub(crate) use controller::bind_tag;
pub(crate) use version::VersionBump;
