//! Opt-in P5-B fixture. Uses the real Store actions, callbacks and Slint model.
use super::state::ProjectStateFacade;
use crate::{MainWindow, Route};
use slint::{ComponentHandle, Model};
use std::time::{Duration, Instant};

pub(super) fn setup(ui: &MainWindow, state: &ProjectStateFacade) {
    if std::env::var("USHER_P5_BENCHMARK").as_deref() != Ok("57")
        || std::env::var("USHER_BENCHMARK_OFFLINE").as_deref() != Ok("1")
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let window = ui.as_weak();
    let state = state.clone();
    crate::startup::run_after_first_frame(move || {
        crate::app_runtime::shared_runtime().spawn(async move {
            // Wait for real startup hydration before exercising projection.
            while state.snapshot().state().projects.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let weak = window.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_current_route(Route::Project);
                }
            });
            tokio::time::sleep(Duration::from_secs(2)).await;
            for round in 0..24u32 {
                let began = Instant::now();
                for index in 0..64 {
                    state.complete_pr_overview(format!("p5-{index}"), Some((round, index, 1)));
                }
                let weak = window.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        let query = if round % 2 == 0 { "project 1" } else { "" };
                        ui.set_project_search_query(query.into());
                        ui.invoke_project_search_projects(query.into());
                        log::info!(
                            "[P5][57] round={round} dispatch_to_ui_us={}",
                            began.elapsed().as_micros()
                        );
                    }
                });
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = window.upgrade() {
                    let model = ui.get_project_items();
                    let updated = (0..model.row_count())
                        .filter(|&i| {
                            model
                                .row_data(i)
                                .is_some_and(|row| row.pr_fat_count_text == "23")
                        })
                        .count();
                    log::info!(
                        "[P5][57] complete rows={} updated={updated}",
                        model.row_count()
                    );
                }
            });
        });
    });
}
