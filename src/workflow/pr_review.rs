//! Stable façade for the pull-request review workspace.

mod avatar;
mod cache;
mod controller;
mod diff_model;
mod diff_projection;
mod file_tree;
mod find;
mod hunk;
mod selection;
mod timeline;
mod virtual_list;

pub(crate) use controller::{WorkflowPrReviewController, bind_pr_review};
