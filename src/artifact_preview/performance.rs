use std::cell::RefCell;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::Model;

use crate::{ArtifactPreviewWindow, MainWindow};

const TRACE_RESIZE_TAIL: Duration = Duration::from_millis(300);
const TRACE_SCROLL_TAIL: Duration = Duration::from_millis(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraceHost {
    DockedDivider,
    DetachedWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TracePhase {
    Resize,
    Scroll,
}

impl TracePhase {
    fn label(self) -> &'static str {
        match self {
            Self::Resize => "resize",
            Self::Scroll => "scroll",
        }
    }
}

impl TraceHost {
    fn label(self) -> &'static str {
        match self {
            Self::DockedDivider => "docked-divider",
            Self::DetachedWindow => "detached-window",
        }
    }
}

#[derive(Debug)]
struct FrameSample {
    elapsed_us: u128,
    host: TraceHost,
    phase: TracePhase,
    frame_gap_us: u128,
    render_us: u128,
    block_count: usize,
    mounted_rows: usize,
    projection_commits: u64,
}

struct FrameTrace {
    path: Option<PathBuf>,
    started: Instant,
    warmup: Duration,
    last_before: Option<Instant>,
    last_host: Option<TraceHost>,
    active_until: Option<Instant>,
    scroll_active_until: Option<Instant>,
    scroll_projection_commits: u64,
    mounted_rows: usize,
    pending_sample: Option<usize>,
    pending_render_started: Option<Instant>,
    pending_host: Option<TraceHost>,
    samples: Vec<FrameSample>,
}

type SharedFrameTrace = Rc<RefCell<FrameTrace>>;

thread_local! {
    static FRAME_TRACE: RefCell<Option<SharedFrameTrace>> = const { RefCell::new(None) };
}

pub(crate) fn initialize() {
    let path = std::env::var_os("USHER_PREVIEW_PERF_TRACE").map(PathBuf::from);
    let warmup = std::env::var("USHER_PREVIEW_PERF_TRACE_WARMUP_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(3));
    FRAME_TRACE.with(|slot| {
        if path.is_none() {
            *slot.borrow_mut() = None;
            return;
        }
        *slot.borrow_mut() = Some(Rc::new(RefCell::new(FrameTrace {
            path,
            started: Instant::now(),
            warmup,
            last_before: None,
            last_host: None,
            active_until: None,
            scroll_active_until: None,
            scroll_projection_commits: 0,
            mounted_rows: 0,
            pending_sample: None,
            pending_render_started: None,
            pending_host: None,
            samples: Vec::new(),
        })));
    });
}

fn shared_trace() -> Option<SharedFrameTrace> {
    FRAME_TRACE.with(|slot| slot.borrow().clone())
}

fn before_rendering(trace: &SharedFrameTrace, host: TraceHost, active: bool, block_count: usize) {
    let mut trace = trace.borrow_mut();
    if trace.path.is_none() {
        return;
    }
    let now = Instant::now();
    if trace.started.elapsed() < trace.warmup {
        trace.last_before = None;
        trace.last_host = None;
        return;
    }
    let was_tracing = trace.active_until.is_some_and(|deadline| now <= deadline)
        || trace
            .scroll_active_until
            .is_some_and(|deadline| now <= deadline);
    if active {
        trace.active_until = Some(now + TRACE_RESIZE_TAIL);
    }
    let tracing_resize = active || trace.active_until.is_some_and(|deadline| now <= deadline);
    let tracing_scroll = trace
        .scroll_active_until
        .is_some_and(|deadline| now <= deadline);
    let phase = if tracing_resize {
        TracePhase::Resize
    } else if tracing_scroll {
        TracePhase::Scroll
    } else {
        trace.last_before = None;
        trace.last_host = None;
        return;
    };

    let frame_gap_us = if was_tracing && trace.last_host == Some(host) {
        trace
            .last_before
            .map_or(0, |previous| now.duration_since(previous).as_micros())
    } else {
        0
    };
    trace.last_before = Some(now);
    trace.last_host = Some(host);
    let elapsed_us = trace.started.elapsed().as_micros();
    let mounted_rows = trace.mounted_rows;
    let projection_commits = trace.scroll_projection_commits;
    trace.samples.push(FrameSample {
        elapsed_us,
        host,
        phase,
        frame_gap_us,
        render_us: 0,
        block_count,
        mounted_rows,
        projection_commits,
    });
    trace.pending_sample = Some(trace.samples.len() - 1);
    trace.pending_render_started = Some(now);
    trace.pending_host = Some(host);
}

pub(super) fn mark_scroll(host: &'static str, projected: bool, mounted_rows: usize) {
    let Some(trace) = shared_trace() else {
        return;
    };
    let mut trace = trace.borrow_mut();
    if trace.path.is_none() {
        return;
    }
    let expected_host = match host {
        "docked-divider" => TraceHost::DockedDivider,
        "detached-window" => TraceHost::DetachedWindow,
        _ => return,
    };
    let now = Instant::now();
    let was_scrolling = trace
        .scroll_active_until
        .is_some_and(|deadline| now <= deadline);
    if !was_scrolling {
        trace.last_before = None;
        trace.last_host = None;
    }
    trace.last_host = Some(expected_host);
    trace.scroll_active_until = Some(now + TRACE_SCROLL_TAIL);
    trace.mounted_rows = mounted_rows;
    if projected {
        trace.scroll_projection_commits = trace.scroll_projection_commits.saturating_add(1);
    }
}

fn after_rendering(trace: &SharedFrameTrace, host: TraceHost) {
    let mut trace = trace.borrow_mut();
    if trace.pending_host != Some(host) {
        return;
    }
    let (Some(index), Some(started)) = (
        trace.pending_sample.take(),
        trace.pending_render_started.take(),
    ) else {
        return;
    };
    trace.pending_host = None;
    if let Some(sample) = trace.samples.get_mut(index) {
        sample.render_us = started.elapsed().as_micros();
    }
}

pub(crate) fn before_main_rendering(ui: &MainWindow) {
    if !ui.get_artifact_preview_visible() && !ui.get_artifact_preview_panel_transitioning() {
        return;
    }
    let Some(trace) = shared_trace() else {
        return;
    };
    let block_count = ui.get_artifact_preview_blocks().row_count();
    before_rendering(
        &trace,
        TraceHost::DockedDivider,
        ui.get_artifact_preview_divider_resizing() || ui.get_artifact_preview_panel_transitioning(),
        block_count,
    );
}

pub(crate) fn after_main_rendering() {
    if let Some(trace) = shared_trace() {
        after_rendering(&trace, TraceHost::DockedDivider);
    }
}

pub(crate) fn before_detached_rendering(window: &ArtifactPreviewWindow) {
    let Some(trace) = shared_trace() else {
        return;
    };
    if trace.borrow().path.is_none() {
        return;
    }
    before_rendering(
        &trace,
        TraceHost::DetachedWindow,
        window.get_layout_resizing(),
        window.get_markdown_blocks().row_count(),
    );
}

pub(crate) fn after_detached_rendering() {
    if let Some(trace) = shared_trace()
        && trace.borrow().path.is_some()
    {
        after_rendering(&trace, TraceHost::DetachedWindow);
    }
}

pub(crate) fn write() -> io::Result<Option<(usize, PathBuf)>> {
    let Some(trace) = shared_trace() else {
        return Ok(None);
    };
    let trace = trace.borrow();
    let Some(path) = trace.path.as_ref() else {
        return Ok(None);
    };
    write_trace(path, &trace.samples)?;
    Ok(Some((trace.samples.len(), path.clone())))
}

fn write_trace(path: &Path, samples: &[FrameSample]) -> io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    writeln!(
        writer,
        "variant\telapsed_us\thost\tphase\tframe_gap_us\trender_us\tblock_count\tmounted_rows\tprojection_commits"
    )?;
    let variant = if std::env::var("USHER_PREVIEW_SCROLL_PERF_VARIANT").as_deref() == Ok("baseline")
        || (std::env::var_os("USHER_PREVIEW_SCROLL_PERF_VARIANT").is_none()
            && std::env::var("USHER_PREVIEW_PERF_VARIANT").as_deref() == Ok("baseline"))
    {
        "baseline"
    } else {
        "optimized"
    };
    for sample in samples {
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            variant,
            sample.elapsed_us,
            sample.host.label(),
            sample.phase.label(),
            sample.frame_gap_us,
            sample.render_us,
            sample.block_count,
            sample.mounted_rows,
            sample.projection_commits,
        )?;
    }
    writer.flush()
}
