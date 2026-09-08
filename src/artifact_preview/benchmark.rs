//! Opt-in real Preview fixture shared by the before/after executables.
use super::*;
use std::time::{Duration, Instant};

pub(super) fn enabled() -> bool {
    std::env::var("USHER_P5_BENCHMARK").as_deref() == Ok("60")
        && crate::app_paths::benchmark_offline_mode()
        && std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_some()
}

pub(super) fn setup(ui: &MainWindow) {
    if !enabled() {
        return;
    }
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        slint::Timer::single_shot(Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            AppState::get(&ui).set_benchmark_preview_created_lines(0);
            let paragraphs =
                std::env::var("USHER_P5_PREVIEW_SCENARIO").as_deref() == Ok("paragraphs");
            let mut markdown = String::from("# P5 Preview\n\n");
            if paragraphs {
                for index in 0..2000 {
                    markdown.push_str(&format!("Paragraph {index}: 中文排版与 English text **strong** `inline` mixed content. Reflow must preserve exact extent.\n\n"));
                }
            } else {
                markdown.push_str("```text\n");
                for index in 0..12000 {
                    markdown.push_str(&format!("line {index:05} 中文 English source text\n"));
                }
                markdown.push_str("```\n\n");
            }
            markdown.push_str("P5 FINAL TAIL\n");
            ui.invoke_chat_open_artifact("p5-preview".into(), "P5 Preview".into(), markdown.into());
            step(ui.as_weak(), 0, Instant::now());
        });
    });
}

fn step(weak: slint::Weak<MainWindow>, index: usize, started: Instant) {
    slint::Timer::single_shot(Duration::from_millis(500), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_artifact_preview_loading()
            || AppState::get(&ui).get_benchmark_preview_extent() <= 0.0
        {
            if started.elapsed() < Duration::from_secs(20) {
                step(weak, index, started);
            }
            return;
        }
        if index < 16 {
            if index == 4 || index == 8 {
                ui.window().set_size(slint::LogicalSize::new(
                    if index == 4 { 1100.0 } else { 900.0 },
                    700.0,
                ));
            }
            AppState::get(&ui).set_benchmark_preview_offset(if index == 15 {
                1.0
            } else {
                (index % 4) as f32 / 4.0
            });
            step(weak, index + 1, started);
        } else {
            let state = AppState::get(&ui);
            let tail = state.get_benchmark_preview_tail();
            log::info!(
                "[P5][60] result tail={tail} extent={} created={} elapsed_ms={}",
                state.get_benchmark_preview_extent(),
                state.get_benchmark_preview_created_lines(),
                started.elapsed().as_millis()
            );
            if tail {
                log::info!("[P5][60] complete");
            }
        }
    });
}

pub(super) fn layout(start: Instant, window: &crate::ArtifactPreviewVirtualWindow) {
    if enabled() {
        log::info!(
            "[P5][60] layout_us={} mounted_blocks={} extent={}",
            start.elapsed().as_micros(),
            window.rows.row_count(),
            window.content_height
        );
    }
}
