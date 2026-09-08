//! Opt-in scroll/reveal diagnostics, separate from review orchestration.
#[cfg(feature = "test-fixtures")]
use std::fs::{File, OpenOptions};
#[cfg(feature = "test-fixtures")]
use std::io::{BufWriter, Write};
#[cfg(feature = "test-fixtures")]
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};
#[cfg(feature = "test-fixtures")]
use std::time::Instant;

#[cfg(feature = "test-fixtures")]
const PR_SCROLL_DIAGNOSTICS_PATH: &str = "/tmp/usher-pr-scroll-diagnostics.log";

#[cfg(feature = "test-fixtures")]
pub(super) struct PrScrollDiagnostics {
    started_at: Instant,
    sequence: AtomicU64,
    writer: Mutex<BufWriter<File>>,
}

#[cfg(not(feature = "test-fixtures"))]
pub(super) struct PrScrollDiagnostics;

#[cfg(feature = "test-fixtures")]
static PR_SCROLL_DIAGNOSTICS: OnceLock<Option<PrScrollDiagnostics>> = OnceLock::new();

#[cfg(feature = "test-fixtures")]
pub(super) fn pr_scroll_diagnostics() -> Option<&'static PrScrollDiagnostics> {
    PR_SCROLL_DIAGNOSTICS
        .get_or_init(|| {
            let enabled = std::env::var("USHER_PR_SCROLL_DIAGNOSTICS")
                .map(|value| !matches!(value.trim(), "" | "0" | "false" | "off"))
                .unwrap_or(false);
            if !enabled {
                return None;
            }

            match OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(PR_SCROLL_DIAGNOSTICS_PATH)
            {
                Ok(file) => Some(PrScrollDiagnostics {
                    started_at: Instant::now(),
                    sequence: AtomicU64::new(0),
                    writer: Mutex::new(BufWriter::new(file)),
                }),
                Err(error) => {
                    eprintln!(
                        "[PRScrollDiag] failed to open {PR_SCROLL_DIAGNOSTICS_PATH}: {error}"
                    );
                    None
                }
            }
        })
        .as_ref()
}

#[cfg(not(feature = "test-fixtures"))]
pub(super) fn pr_scroll_diagnostics() -> Option<&'static PrScrollDiagnostics> {
    None
}

#[cfg(feature = "test-fixtures")]
#[allow(clippy::too_many_arguments)]
pub(super) fn write_pr_scroll_diagnostic(
    kind: &str,
    viewport_y: f32,
    requested_y: f32,
    visible_height: f32,
    viewport_height: f32,
    find_open: bool,
    find_focused: bool,
    hunk_pending: bool,
    find_revision: i32,
    line_revision: i32,
    hunk_attempts: i32,
) {
    let Some(diagnostics) = pr_scroll_diagnostics() else {
        return;
    };
    let sequence = diagnostics.sequence.fetch_add(1, Ordering::Relaxed) + 1;
    let elapsed_us = diagnostics.started_at.elapsed().as_micros();
    let maximum = (viewport_height - visible_height).max(0.0);
    let Ok(mut writer) = diagnostics.writer.lock() else {
        return;
    };
    let _ = writeln!(
        writer,
        "seq={sequence} elapsed_us={elapsed_us} kind={kind} viewport_y={viewport_y:.3} requested_y={requested_y:.3} visible_height={visible_height:.3} viewport_height={viewport_height:.3} maximum={maximum:.3} find_open={find_open} find_focused={find_focused} hunk_pending={hunk_pending} find_revision={find_revision} line_revision={line_revision} hunk_attempts={hunk_attempts}"
    );
    if sequence % 16 == 0 || kind != "viewport" {
        let _ = writer.flush();
    }
}

#[cfg(not(feature = "test-fixtures"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn write_pr_scroll_diagnostic(
    _kind: &str,
    _viewport_y: f32,
    _requested_y: f32,
    _visible_height: f32,
    _viewport_height: f32,
    _find_open: bool,
    _find_focused: bool,
    _hunk_pending: bool,
    _find_revision: i32,
    _line_revision: i32,
    _hunk_attempts: i32,
) {
}

#[cfg(feature = "test-fixtures")]
#[allow(clippy::too_many_arguments)]
pub(super) fn write_pr_find_reveal_diagnostic(
    line: i32,
    start_char: i32,
    row_count: usize,
    code_width: f32,
    cell_width: f32,
    side: &str,
    current_viewport_y: f32,
    target_viewport_y: f32,
    visible_height: f32,
    content_height: f32,
) {
    let Some(diagnostics) = pr_scroll_diagnostics() else {
        return;
    };
    let sequence = diagnostics.sequence.fetch_add(1, Ordering::Relaxed) + 1;
    let elapsed_us = diagnostics.started_at.elapsed().as_micros();
    let average_item_height = if row_count == 0 {
        0.0
    } else {
        content_height / row_count as f32
    };
    let Ok(mut writer) = diagnostics.writer.lock() else {
        return;
    };
    let _ = writeln!(
        writer,
        "seq={sequence} elapsed_us={elapsed_us} kind=find-resolve line={line} start_char={start_char} side={side} row_count={row_count} code_width={code_width:.3} cell_width={cell_width:.3} average_item_height={average_item_height:.3} viewport_y={current_viewport_y:.3} target_viewport_y={target_viewport_y:.3} visible_height={visible_height:.3} viewport_height={content_height:.3}"
    );
    let _ = writer.flush();
}

#[cfg(not(feature = "test-fixtures"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn write_pr_find_reveal_diagnostic(
    _line: i32,
    _start_char: i32,
    _row_count: usize,
    _code_width: f32,
    _cell_width: f32,
    _side: &str,
    _current_viewport_y: f32,
    _target_viewport_y: f32,
    _visible_height: f32,
    _content_height: f32,
) {
}
