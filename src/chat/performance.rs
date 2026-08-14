use std::cell::RefCell;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use slint::Model;

use crate::{MainWindow, Route};

const TRACE_RESIZE_TAIL: Duration = Duration::from_millis(250);
const TRACE_SCROLL_TAIL: Duration = Duration::from_millis(350);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PerformanceVariant {
    Baseline,
    Optimized,
}

impl PerformanceVariant {
    pub(crate) fn is_optimized(self) -> bool {
        matches!(self, Self::Optimized)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Optimized => "optimized",
        }
    }
}

pub(crate) fn variant() -> PerformanceVariant {
    static VARIANT: OnceLock<PerformanceVariant> = OnceLock::new();
    *VARIANT.get_or_init(|| {
        if std::env::var("USHER_CHAT_PERF_VARIANT").as_deref() == Ok("baseline") {
            PerformanceVariant::Baseline
        } else {
            PerformanceVariant::Optimized
        }
    })
}

pub(crate) fn fixture_enabled() -> bool {
    crate::memory_diagnostics::diagnostics_enabled()
        && std::env::var("USHER_MEMORY_SCENARIO").as_deref() == Ok("chat-performance")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TracePhase {
    Idle,
    Resize,
    Scroll,
}

impl TracePhase {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Resize => "resize",
            Self::Scroll => "scroll",
        }
    }
}

#[derive(Debug)]
struct FrameSample {
    elapsed_us: u128,
    phase: TracePhase,
    frame_gap_us: u128,
    render_us: u128,
    mounted_rows: usize,
    scroll_y: f32,
    bottom_distance_px: f32,
    scrollbar_dragging: bool,
    scrollbar_pointer_y: f32,
    scrollbar_thumb_y: f32,
    scrollbar_release_revision: i32,
    scrollbar_release_position: f32,
    anchor_scroll_writes: u64,
    wheel_input_revision: i32,
    wheel_input_delta: f32,
    model_commits: u64,
    virtual_blank_px: f32,
    model_mutation_us: u128,
    mutation_to_render_us: u128,
    inserted_rows: usize,
    removed_rows: usize,
    incremental_mutation: bool,
}

pub(crate) type SharedFrameTrace = Rc<RefCell<FrameTrace>>;

pub(crate) struct FrameTrace {
    path: Option<PathBuf>,
    started: Instant,
    warmup: Duration,
    last_before: Option<Instant>,
    last_scroll_y: Option<f32>,
    resize_active_until: Option<Instant>,
    scroll_active_until: Option<Instant>,
    last_phase: TracePhase,
    pending_sample: Option<usize>,
    pending_render_started: Option<Instant>,
    pending_mutation_completed_at: Option<Instant>,
    last_sampled_mutation_revision: u64,
    samples: Vec<FrameSample>,
}

impl FrameTrace {
    pub(crate) fn from_environment() -> SharedFrameTrace {
        let path = std::env::var_os("USHER_CHAT_PERF_TRACE").map(PathBuf::from);
        let warmup = std::env::var("USHER_CHAT_PERF_TRACE_WARMUP_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_secs(3));
        Rc::new(RefCell::new(Self {
            path,
            started: Instant::now(),
            warmup,
            last_before: None,
            last_scroll_y: None,
            resize_active_until: None,
            scroll_active_until: None,
            last_phase: TracePhase::Idle,
            pending_sample: None,
            pending_render_started: None,
            pending_mutation_completed_at: None,
            last_sampled_mutation_revision: 0,
            samples: Vec::new(),
        }))
    }

    pub(crate) fn before_rendering(&mut self, ui: &MainWindow) {
        if self.path.is_none() {
            return;
        }

        let now = Instant::now();
        if self.started.elapsed() < self.warmup || ui.get_current_route() != Route::Chat {
            self.last_before = None;
            self.last_scroll_y = None;
            self.last_phase = TracePhase::Idle;
            return;
        }

        let scroll_y = ui.get_chat_scroll_position();
        if self
            .last_scroll_y
            .is_some_and(|previous| (previous - scroll_y).abs() > 0.25)
        {
            self.scroll_active_until = Some(now + TRACE_SCROLL_TAIL);
        }
        self.last_scroll_y = Some(scroll_y);

        if ui.get_window_live_resizing() {
            self.resize_active_until = Some(now + TRACE_RESIZE_TAIL);
        }
        let resize_active = ui.get_window_live_resizing()
            || self
                .resize_active_until
                .is_some_and(|deadline| now <= deadline);
        let scroll_active = self
            .scroll_active_until
            .is_some_and(|deadline| now <= deadline);
        let phase = if resize_active {
            TracePhase::Resize
        } else if scroll_active {
            TracePhase::Scroll
        } else {
            TracePhase::Idle
        };

        let same_phase = phase == self.last_phase;
        let frame_gap_us = if same_phase {
            self.last_before
                .map_or(0, |previous| now.duration_since(previous).as_micros())
        } else {
            0
        };
        self.last_before = Some(now);
        self.last_phase = phase;

        let (model_commits, _) = super::renderer::diagnostic_counts();
        let mutation = super::renderer::latest_model_mutation()
            .filter(|mutation| mutation.revision > self.last_sampled_mutation_revision);
        if let Some(mutation) = mutation.as_ref() {
            self.last_sampled_mutation_revision = mutation.revision;
        }
        let viewport_height = (ui.get_chat_layout_window_height() - 180.0).clamp(360.0, 1400.0);
        let virtual_blank_px =
            super::interactive::virtual_blank_exposure_px((-scroll_y).max(0.0), viewport_height);
        self.samples.push(FrameSample {
            elapsed_us: self.started.elapsed().as_micros(),
            phase,
            frame_gap_us,
            render_us: 0,
            mounted_rows: ui.get_chat_messages().row_count(),
            scroll_y,
            bottom_distance_px: ui.get_chat_bottom_distance(),
            scrollbar_dragging: ui.get_chat_scrollbar_dragging(),
            scrollbar_pointer_y: ui.get_chat_scrollbar_drag_pointer_y(),
            scrollbar_thumb_y: ui.get_chat_scrollbar_drag_thumb_y(),
            scrollbar_release_revision: ui.get_chat_scrollbar_drag_release_revision(),
            scrollbar_release_position: ui.get_chat_scrollbar_drag_release_position(),
            anchor_scroll_writes: super::interactive::anchor_scroll_write_count(),
            wheel_input_revision: ui.get_chat_wheel_input_revision(),
            wheel_input_delta: ui.get_chat_wheel_input_delta(),
            model_commits,
            virtual_blank_px,
            model_mutation_us: mutation
                .as_ref()
                .map_or(0, |mutation| mutation.duration.as_micros()),
            mutation_to_render_us: 0,
            inserted_rows: mutation
                .as_ref()
                .map_or(0, |mutation| mutation.inserted_rows),
            removed_rows: mutation
                .as_ref()
                .map_or(0, |mutation| mutation.removed_rows),
            incremental_mutation: mutation
                .as_ref()
                .is_some_and(|mutation| mutation.incremental),
        });
        self.pending_sample = Some(self.samples.len() - 1);
        self.pending_render_started = Some(now);
        self.pending_mutation_completed_at = mutation.map(|mutation| mutation.completed_at);
    }

    pub(crate) fn after_rendering(&mut self) {
        let (Some(index), Some(started)) = (
            self.pending_sample.take(),
            self.pending_render_started.take(),
        ) else {
            return;
        };
        if let Some(sample) = self.samples.get_mut(index) {
            sample.render_us = started.elapsed().as_micros();
            sample.mutation_to_render_us = self
                .pending_mutation_completed_at
                .take()
                .map_or(0, |completed_at| completed_at.elapsed().as_micros());
        }
    }

    pub(crate) fn write(&self) -> io::Result<Option<(usize, PathBuf)>> {
        let Some(path) = self.path.as_ref() else {
            return Ok(None);
        };
        write_trace(path, variant(), &self.samples)?;
        Ok(Some((self.samples.len(), path.clone())))
    }
}

fn write_trace(
    path: &Path,
    variant: PerformanceVariant,
    samples: &[FrameSample],
) -> io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    writeln!(
        writer,
        "variant\telapsed_us\tphase\tframe_gap_us\trender_us\tmounted_rows\tscroll_y\tmodel_commits\tvirtual_blank_px\tmodel_mutation_us\tmutation_to_render_us\tinserted_rows\tremoved_rows\tincremental_mutation\tbottom_distance_px\tscrollbar_dragging\tscrollbar_pointer_y\tscrollbar_thumb_y\tanchor_scroll_writes\tscrollbar_release_revision\tscrollbar_release_position\twheel_input_revision\twheel_input_delta"
    )?;
    for sample in samples {
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{}\t{:.2}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{}\t{:.2}\t{:.2}\t{}\t{}\t{:.2}\t{}\t{:.2}",
            variant.label(),
            sample.elapsed_us,
            sample.phase.label(),
            sample.frame_gap_us,
            sample.render_us,
            sample.mounted_rows,
            sample.scroll_y,
            sample.model_commits,
            sample.virtual_blank_px,
            sample.model_mutation_us,
            sample.mutation_to_render_us,
            sample.inserted_rows,
            sample.removed_rows,
            u8::from(sample.incremental_mutation),
            sample.bottom_distance_px,
            u8::from(sample.scrollbar_dragging),
            sample.scrollbar_pointer_y,
            sample.scrollbar_thumb_y,
            sample.anchor_scroll_writes,
            sample.scrollbar_release_revision,
            sample.scrollbar_release_position,
            sample.wheel_input_revision,
            sample.wheel_input_delta,
        )?;
    }
    writer.flush()
}
