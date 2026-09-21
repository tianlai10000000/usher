//! Exact log extents, measured off-loop; scrolling only projects a bounded window.
use super::log_layout::LogLayout;
use super::log_model::{LogModel, LogSnapshot, ParsedText};
use crate::app_runtime::latest_compute::LatestCompute;
use crate::{JobLogLineItem, JobLogVirtualRow, JobLogWindow, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use usher_virtual_list::{VirtualList, VirtualWindow};

const MAX_CACHED_VIEWPORTS: usize = 16;
const MAX_CACHED_VIEWPORT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
struct Key {
    revision: i32,
    count: usize,
    width: u32,
    line_height: u32,
}

struct Request {
    key: Key,
    lines: LogSnapshot,
    collection: parley::fontique::Collection,
}

struct Measurement {
    key: Key,
    lines: LogSnapshot,
    wrapped: Vec<Vec<Range<usize>>>,
    extents: Vec<u32>,
    text_retained_bytes: usize,
    layout_retained_bytes: usize,
}

fn snapshot_retained_bytes(lines: &LogSnapshot) -> usize {
    lines
        .retained_text_bytes()
        .saturating_add(lines.len().saturating_mul(std::mem::size_of::<usize>()))
}

fn measure(request: Request, cancelled: impl Fn() -> bool) -> Option<Measurement> {
    let mut layout = LogLayout::with_collection(request.collection);
    let mut wrapped = Vec::with_capacity(request.lines.len());
    let mut extents = Vec::with_capacity(request.lines.len());
    for line in request.lines.iter() {
        if cancelled() {
            return None;
        }
        let text = line.display_text();
        let visual = layout.line_ranges(&text, f32::from_bits(request.key.width));
        let height = f32::from_bits(request.key.line_height)
            + visual.len().saturating_sub(1) as f32 * 20.0
            + 18.0;
        extents.push(height.max(32.0).ceil() as u32);
        wrapped.push(visual);
    }
    let text_retained_bytes = snapshot_retained_bytes(&request.lines);
    let wrapped_retained_bytes = wrapped
        .iter()
        .map(|ranges| {
            ranges
                .capacity()
                .saturating_mul(std::mem::size_of::<Range<usize>>())
        })
        .fold(
            wrapped
                .capacity()
                .saturating_mul(std::mem::size_of::<Vec<Range<usize>>>()),
            usize::saturating_add,
        );
    let extent_retained_bytes = extents
        .capacity()
        .saturating_mul(std::mem::size_of::<u32>())
        // VirtualList retains its own immutable extent slice in addition to
        // this measurement vector.
        .saturating_add(extents.len().saturating_mul(std::mem::size_of::<u32>()));
    Some(Measurement {
        key: request.key,
        lines: request.lines,
        wrapped,
        extents,
        text_retained_bytes,
        layout_retained_bytes: wrapped_retained_bytes.saturating_add(extent_retained_bytes),
    })
}

struct Viewport {
    collection: parley::fontique::Collection,
    model: ModelRc<JobLogLineItem>,
    requested: Option<Key>,
    measurement: Option<Measurement>,
    worker: LatestCompute<Request, Measurement>,
    list: VirtualList,
    window: VirtualWindow,
    rows: Rc<VecModel<JobLogVirtualRow>>,
    indices: Vec<usize>,
    pending_lines: Option<LogSnapshot>,
    retained_bytes: usize,
    #[cfg(test)]
    retained_bytes_override: Option<usize>,
}

impl Viewport {
    fn new(model: ModelRc<JobLogLineItem>) -> Self {
        let mut viewport = Self {
            collection: LogLayout::new().collection(),
            model,
            requested: None,
            measurement: None,
            worker: LatestCompute::new(),
            list: VirtualList::new(&[], 2),
            window: VirtualWindow::default(),
            rows: Rc::new(VecModel::default()),
            indices: Vec::new(),
            pending_lines: None,
            retained_bytes: 0,
            #[cfg(test)]
            retained_bytes_override: None,
        };
        viewport.refresh_retained_bytes();
        viewport
    }

    fn install(&mut self, measurement: Measurement) {
        self.list.replace_item_extents(&measurement.extents, 2);
        self.measurement = Some(measurement);
        self.pending_lines = None;
        self.indices.clear();
        self.rows.set_vec(Vec::new());
        self.refresh_retained_bytes();
    }

    fn refresh_retained_bytes(&mut self) {
        #[cfg(test)]
        if let Some(retained_bytes) = self.retained_bytes_override {
            self.retained_bytes = retained_bytes;
            return;
        }
        let log_model = self.model.as_any().downcast_ref::<LogModel>();
        let source_bytes = log_model.map_or(0, |model| {
            model.retained_text_bytes().saturating_add(
                model
                    .row_count()
                    .saturating_mul(std::mem::size_of::<usize>()),
            )
        });
        let pending_bytes = self.pending_lines.as_ref().map_or(0, |lines| {
            if log_model.is_some_and(|model| model.shares_snapshot(lines)) {
                0
            } else {
                snapshot_retained_bytes(lines)
            }
        });
        let measurement_bytes = self.measurement.as_ref().map_or(0, |measurement| {
            let shares_source =
                log_model.is_some_and(|model| model.shares_snapshot(&measurement.lines));
            let text_retained_bytes = if shares_source {
                0
            } else {
                measurement.text_retained_bytes
            };
            text_retained_bytes.saturating_add(measurement.layout_retained_bytes)
        });
        self.retained_bytes = source_bytes
            .saturating_add(pending_bytes)
            .saturating_add(measurement_bytes);
    }

    fn is_mounted(&self) -> bool {
        Rc::strong_count(&self.rows) > 1
    }

    fn prepare_request(&mut self, key: Key) -> Request {
        let lines = if let Some(model) = self.model.as_any().downcast_ref::<LogModel>() {
            model.measurement_lines()
        } else {
            // ModelRc does not expose the backing model's allocation capacity. The
            // concrete fallback snapshot is therefore the first measurable owner;
            // retain and account it while the worker request is pending.
            LogSnapshot::from_parsed(self.model.iter().map(|line| ParsedText {
                text: line.text.to_string(),
                prefix: line.prefix.to_string(),
                kind: line.kind.to_string(),
            }))
        };
        self.requested = Some(key.clone());
        self.pending_lines = Some(lines.clone());
        self.refresh_retained_bytes();
        Request {
            key,
            lines,
            collection: self.collection.clone(),
        }
    }

    fn layout(
        &mut self,
        key: Key,
        offset: f32,
        height: f32,
        weak: &slint::Weak<MainWindow>,
    ) -> JobLogWindow {
        self.worker.acknowledge();
        if let Some(result) = self.worker.take_output() {
            if result.key == key {
                self.install(result);
            }
        }
        if self.requested.as_ref() != Some(&key) {
            let request = self.prepare_request(key.clone());
            #[cfg(test)]
            {
                let _ = weak;
                self.install(measure(request, || false).unwrap());
            }
            #[cfg(not(test))]
            {
                let weak = weak.clone();
                self.worker.submit(
                    request,
                    |request, token| measure(request, || token.cancelled()),
                    move || {
                        let weak = weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                let global = ui.global::<crate::JobLogLayout>();
                                global.set_epoch(global.get_epoch().wrapping_add(1));
                            }
                        });
                    },
                );
            }
        }
        let Some(measurement) = &self.measurement else {
            return JobLogWindow {
                rows: self.rows.clone().into(),
                loading: true,
                ..Default::default()
            };
        };
        self.list
            .set_viewport(offset.max(0.0) as u64, height.max(1.0).ceil() as u32);
        self.list.write_window(&mut self.window);
        let indices: Vec<_> = self.window.items.iter().map(|item| item.index).collect();
        if indices != self.indices {
            let rows: Vec<_> = self
                .window
                .items
                .iter()
                .map(|item| {
                    let line = measurement.lines.line(item.index);
                    JobLogVirtualRow {
                        line: JobLogLineItem {
                            text: line.text.as_str().into(),
                            prefix: line.prefix.as_str().into(),
                            kind: line.kind.as_str().into(),
                        },
                        line_number: (item.index + 1) as i32,
                        visual_lines: ModelRc::new(VecModel::from(
                            measurement.wrapped[item.index]
                                .iter()
                                .map(|range| line.display_range(range.clone()).as_ref().into())
                                .collect::<Vec<_>>(),
                        )),
                        offset: item.start as f32,
                        extent: item.extent as f32,
                    }
                })
                .collect();
            self.rows.set_vec(rows);
            self.indices = indices;
        }
        JobLogWindow {
            rows: self.rows.clone().into(),
            content_height: self.window.content_extent as f32,
            loading: measurement.key != key,
        }
    }
}

impl Drop for Viewport {
    fn drop(&mut self) {
        self.worker.cancel();
    }
}

struct ViewportCache {
    viewports: Vec<Viewport>,
    max_entries: usize,
    max_retained_bytes: usize,
}

impl ViewportCache {
    fn new() -> Self {
        Self {
            viewports: Vec::new(),
            max_entries: MAX_CACHED_VIEWPORTS,
            max_retained_bytes: MAX_CACHED_VIEWPORT_BYTES,
        }
    }

    #[cfg(test)]
    fn with_limits(max_entries: usize, max_retained_bytes: usize) -> Self {
        Self {
            viewports: Vec::new(),
            max_entries,
            max_retained_bytes,
        }
    }

    fn activate(&mut self, model: &ModelRc<JobLogLineItem>) -> usize {
        let active =
            if let Some(index) = self.viewports.iter().position(|view| view.model == *model) {
                index
            } else {
                self.viewports.push(Viewport::new(model.clone()));
                self.viewports.len() - 1
            };
        self.enforce_limits(active)
    }

    fn enforce_limits(&mut self, active: usize) -> usize {
        self.prune(Some(active))
            .expect("the protected current viewport cannot be pruned")
    }

    fn prune(&mut self, mut active: Option<usize>) -> Option<usize> {
        // Inactive LogModels may continue receiving live output while their
        // disclosures are collapsed. Snapshot byte counters make this refresh
        // O(viewports), without rescanning any source rows.
        for viewport in &mut self.viewports {
            viewport.refresh_retained_bytes();
        }
        loop {
            if !self.over_limits() {
                break;
            }
            let Some(oldest_inactive) = (0..self.viewports.len())
                .find(|index| Some(*index) != active && !self.viewports[*index].is_mounted())
            else {
                // The current viewport and all rows models still mounted by Slint are
                // pinned. Their combined working set may temporarily exceed either
                // limit until a disclosure releases its JobLogWindow.
                break;
            };
            self.viewports.remove(oldest_inactive);
            if active.is_some_and(|active| oldest_inactive < active) {
                active = active.map(|active| active - 1);
            }
        }
        active
    }

    fn over_limits(&self) -> bool {
        self.viewports.len() > self.max_entries
            || self
                .viewports
                .iter()
                .map(|view| view.retained_bytes)
                .fold(0usize, usize::saturating_add)
                > self.max_retained_bytes
    }
}

fn start_prune_timer(prune_timer: &Rc<Timer>, viewports: Rc<RefCell<ViewportCache>>) {
    let timer_handle = Rc::downgrade(prune_timer);
    prune_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(250),
        move || {
            let retry = {
                let mut viewports = viewports.borrow_mut();
                viewports.prune(None);
                viewports.over_limits()
            };
            if !retry {
                if let Some(timer) = timer_handle.upgrade() {
                    timer.stop();
                }
            }
        },
    );
}

pub(super) fn setup(ui: &MainWindow) {
    let viewports = Rc::new(RefCell::new(ViewportCache::new()));
    let prune_timer = Rc::new(Timer::default());
    let callback_viewports = viewports.clone();
    let callback_prune_timer = prune_timer.clone();
    let weak = ui.as_weak();
    ui.global::<crate::JobLogLayout>().on_layout_window(
        move |model, revision, width, offset, height, line_height, _epoch| {
            let mut viewports = callback_viewports.borrow_mut();
            let index = viewports.activate(&model);
            let window = viewports.viewports[index].layout(
                Key {
                    revision,
                    count: model.row_count(),
                    width: width.max(1.0).to_bits(),
                    line_height: line_height.max(1.0).to_bits(),
                },
                offset,
                height,
                &weak,
            );
            viewports.enforce_limits(index);
            if super::retention_benchmark::enabled() {
                let mut bytes = super::retention_benchmark::Retained::default();
                for view in viewports.viewports.iter() {
                    if let Some(model) = view.model.as_any().downcast_ref::<LogModel>() {
                        bytes.source += model.retained_text_bytes();
                    }
                    if let Some(measurement) = &view.measurement {
                        if !view
                            .model
                            .as_any()
                            .downcast_ref::<LogModel>()
                            .is_some_and(|model| model.shares_snapshot(&measurement.lines))
                        {
                            bytes.measurement += measurement.lines.retained_text_bytes();
                        }
                        bytes.wrapped += measurement
                            .wrapped
                            .iter()
                            .map(|ranges| ranges.capacity() * std::mem::size_of::<Range<usize>>())
                            .sum::<usize>();
                    }
                }
                super::retention_benchmark::observe(&model, &window, bytes);
            }
            let retry_prune = viewports.over_limits();
            drop(viewports);
            if retry_prune && !callback_prune_timer.running() {
                start_prune_timer(&callback_prune_timer, callback_viewports.clone());
            } else if !retry_prune && callback_prune_timer.running() {
                callback_prune_timer.stop();
            }
            window
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_model() -> ModelRc<JobLogLineItem> {
        Rc::new(VecModel::default()).into()
    }

    fn insert_with_retained_bytes(
        cache: &mut ViewportCache,
        model: &ModelRc<JobLogLineItem>,
        retained_bytes: usize,
    ) -> usize {
        let active = cache.activate(model);
        cache.viewports[active].retained_bytes_override = Some(retained_bytes);
        cache.viewports[active].retained_bytes = retained_bytes;
        cache.enforce_limits(active)
    }

    fn set_retained_bytes(cache: &mut ViewportCache, index: usize, retained_bytes: usize) {
        cache.viewports[index].retained_bytes_override = Some(retained_bytes);
        cache.viewports[index].retained_bytes = retained_bytes;
    }

    fn cache_contains(cache: &ViewportCache, model: &ModelRc<JobLogLineItem>) -> bool {
        cache.viewports.iter().any(|view| view.model == *model)
    }

    #[test]
    fn viewport_cache_keeps_at_most_sixteen_entries() {
        assert_eq!(MAX_CACHED_VIEWPORTS, 16);
        let mut cache = ViewportCache::with_limits(MAX_CACHED_VIEWPORTS, usize::MAX);
        let models: Vec<_> = (0..=MAX_CACHED_VIEWPORTS).map(|_| cache_model()).collect();

        for model in &models {
            insert_with_retained_bytes(&mut cache, model, 1);
        }

        assert_eq!(cache.viewports.len(), MAX_CACHED_VIEWPORTS);
        assert!(!cache_contains(&cache, &models[0]));
        assert!(
            models[1..]
                .iter()
                .all(|model| cache_contains(&cache, model))
        );
    }

    #[test]
    fn viewport_cache_evicts_oldest_entries_to_meet_byte_budget() {
        assert_eq!(MAX_CACHED_VIEWPORT_BYTES, 16 * 1024 * 1024);
        let mut cache = ViewportCache::with_limits(MAX_CACHED_VIEWPORTS, 100);
        let models: Vec<_> = (0..3).map(|_| cache_model()).collect();

        for model in &models {
            insert_with_retained_bytes(&mut cache, model, 40);
        }

        assert_eq!(cache.viewports.len(), 2);
        assert!(!cache_contains(&cache, &models[0]));
        assert!(cache_contains(&cache, &models[1]));
        assert!(cache_contains(&cache, &models[2]));
    }

    #[test]
    fn viewport_cache_keeps_exact_budget_and_evicts_at_one_byte_over() {
        let mut cache = ViewportCache::with_limits(MAX_CACHED_VIEWPORTS, 100);
        let oldest = cache_model();
        let active = cache_model();

        insert_with_retained_bytes(&mut cache, &oldest, 50);
        let active_index = insert_with_retained_bytes(&mut cache, &active, 50);
        assert_eq!(cache.viewports.len(), 2, "exact budget must be retained");

        set_retained_bytes(&mut cache, active_index, 51);
        cache.enforce_limits(active_index);
        assert_eq!(cache.viewports.len(), 1, "one byte over must evict");
        assert!(!cache_contains(&cache, &oldest));
        assert!(cache_contains(&cache, &active));
    }

    #[test]
    fn viewport_cache_keeps_one_oversized_active_entry() {
        let mut cache = ViewportCache::with_limits(MAX_CACHED_VIEWPORTS, 100);
        let old = cache_model();
        let active = cache_model();
        insert_with_retained_bytes(&mut cache, &old, 40);

        insert_with_retained_bytes(&mut cache, &active, 150);

        assert_eq!(cache.viewports.len(), 1);
        assert!(!cache_contains(&cache, &old));
        assert!(cache_contains(&cache, &active));
        assert_eq!(cache.viewports[0].retained_bytes, 150);
    }

    #[test]
    fn viewport_cache_protects_an_old_active_entry_and_evicts_next_oldest() {
        let mut cache = ViewportCache::with_limits(MAX_CACHED_VIEWPORTS, 120);
        let models: Vec<_> = (0..3).map(|_| cache_model()).collect();
        for model in &models {
            insert_with_retained_bytes(&mut cache, model, 40);
        }

        let active = cache.activate(&models[0]);
        assert_eq!(active, 0, "the active viewport is the oldest entry");
        set_retained_bytes(&mut cache, active, 80);
        cache.enforce_limits(active);

        assert_eq!(cache.viewports.len(), 2);
        assert!(cache_contains(&cache, &models[0]));
        assert!(!cache_contains(&cache, &models[1]));
        assert!(cache_contains(&cache, &models[2]));
    }

    #[test]
    fn viewport_cache_pins_all_mounted_rows_until_their_window_is_released() {
        let mut cache = ViewportCache::with_limits(MAX_CACHED_VIEWPORTS, 100);
        let first = cache_model();
        let second = cache_model();

        let first_index = insert_with_retained_bytes(&mut cache, &first, 80);
        let first_lease = cache.viewports[first_index].rows.clone();
        let second_index = insert_with_retained_bytes(&mut cache, &second, 80);
        let second_lease = cache.viewports[second_index].rows.clone();

        cache.prune(None);
        assert_eq!(cache.viewports.len(), 2);
        assert!(cache.over_limits());
        assert!(cache_contains(&cache, &first));
        assert!(cache_contains(&cache, &second));

        drop(first_lease);
        cache.prune(None);
        assert_eq!(cache.viewports.len(), 1);
        assert!(!cache.over_limits());
        assert!(!cache_contains(&cache, &first));
        assert!(cache_contains(&cache, &second));
        drop(second_lease);
    }

    #[test]
    fn production_layout_window_pins_rows_until_the_window_is_dropped() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let model = Rc::new(LogModel::new(
            LogSnapshot::default().update("first\nsecond"),
        ));
        let mut viewport = Viewport::new(model.clone().into());
        assert!(!viewport.is_mounted());

        let window = viewport.layout(
            Key {
                revision: 0,
                count: model.row_count(),
                width: 320.0f32.to_bits(),
                line_height: 14.0f32.to_bits(),
            },
            0.0,
            200.0,
            &ui.as_weak(),
        );
        assert!(!window.loading);
        assert!(viewport.is_mounted());

        drop(window);
        assert!(!viewport.is_mounted());
    }

    #[test]
    fn collapsed_live_log_growth_is_refreshed_before_eviction() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let initial = LogSnapshot::default().update("initial");
        let growing_model = Rc::new(LogModel::new(initial));
        let growing: ModelRc<JobLogLineItem> = growing_model.clone().into();
        let current = cache_model();
        let mut cache = ViewportCache::with_limits(MAX_CACHED_VIEWPORTS, usize::MAX);

        let growing_index = cache.activate(&growing);
        let window = cache.viewports[growing_index].layout(
            Key {
                revision: 0,
                count: 1,
                width: 320.0f32.to_bits(),
                line_height: 14.0f32.to_bits(),
            },
            0.0,
            200.0,
            &ui.as_weak(),
        );
        drop(window);
        cache.viewports[growing_index].refresh_retained_bytes();
        let initial_retained = cache.viewports[growing_index].retained_bytes;
        cache.max_retained_bytes = initial_retained + 1;

        let current_index = cache.activate(&current);
        growing_model.update(LogSnapshot::default().update(&"grown-value".repeat(4096)));
        cache.enforce_limits(current_index);

        assert!(!cache_contains(&cache, &growing));
        assert!(cache_contains(&cache, &current));
    }

    #[test]
    fn generic_fallback_snapshot_is_accounted_while_measurement_is_pending() {
        let model = Rc::new(VecModel::from(vec![JobLogLineItem {
            text: "generic fallback retained text".repeat(32).into(),
            ..Default::default()
        }]));
        let mut viewport = Viewport::new(model.into());
        assert_eq!(viewport.retained_bytes, 0, "opaque source is not estimable");

        let request = viewport.prepare_request(Key {
            revision: 0,
            count: 1,
            width: 320.0f32.to_bits(),
            line_height: 14.0f32.to_bits(),
        });
        assert!(viewport.measurement.is_none());
        assert!(viewport.pending_lines.is_some());
        assert!(viewport.retained_bytes > 0);
        assert_eq!(
            viewport.retained_bytes,
            snapshot_retained_bytes(viewport.pending_lines.as_ref().unwrap())
        );

        viewport.install(measure(request, || false).unwrap());
        assert!(viewport.pending_lines.is_none());
        assert!(viewport.measurement.is_some());
        assert!(viewport.retained_bytes > 0);
    }

    #[test]
    fn stopped_prune_timer_does_not_retain_its_own_handle_or_cache() {
        i_slint_backend_testing::init_no_event_loop();
        let timer = Rc::new(Timer::default());
        let timer_weak = Rc::downgrade(&timer);
        let cache = Rc::new(RefCell::new(ViewportCache::new()));
        let cache_weak = Rc::downgrade(&cache);

        start_prune_timer(&timer, cache.clone());
        assert!(
            timer.running(),
            "the regression must exercise a registered timer"
        );
        assert!(
            Rc::strong_count(&cache) > 1,
            "the registered callback must own the cache before teardown"
        );
        timer.stop();
        drop(cache);
        drop(timer);

        assert!(timer_weak.upgrade().is_none());
        assert!(cache_weak.upgrade().is_none());
    }

    #[test]
    fn production_log_model_layout_retains_shared_rows_and_exact_visible_text() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let snapshot =
            LogSnapshot::default().update("::warning::\u{6d88}\u{606f}%0A%0Anext\n\nplain");
        let model = Rc::new(LogModel::new(snapshot));
        let mut viewport = Viewport::new(model.clone().into());
        for width in [1.0f32, 160.0, 800.0] {
            let key = Key {
                revision: 0,
                count: 3,
                width: width.to_bits(),
                line_height: 14.0f32.to_bits(),
            };
            let window = viewport.layout(key, 0.0, 10000.0, &ui.as_weak());
            assert!(!window.loading);
            let measured = viewport.measurement.as_ref().unwrap();
            assert!(model.shares_snapshot(&measured.lines));
            assert!(measured.text_retained_bytes > 0);
            assert!(measured.layout_retained_bytes > 0);
            let source_retained_bytes = model.retained_text_bytes().saturating_add(
                model
                    .row_count()
                    .saturating_mul(std::mem::size_of::<usize>()),
            );
            assert_eq!(
                viewport.retained_bytes,
                source_retained_bytes.saturating_add(measured.layout_retained_bytes),
                "shared snapshot text must only be accounted as LogModel source"
            );
            assert!(viewport.retained_bytes > 0);
            let mut reference = LogLayout::new();
            for (index, row) in window.rows.iter().enumerate() {
                let line = measured.lines.line(index);
                let expected = reference.wrap(&line.display_text(), width);
                assert_eq!(row.visual_lines.iter().collect::<Vec<_>>(), expected);
                assert_eq!(row.line_number, (index + 1) as i32);
            }
        }
    }

    #[test]
    fn scrolling_reuses_measurement_and_same_count_revision_replaces_it() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let model = Rc::new(VecModel::from(vec![
            JobLogLineItem {
                text: "short".into(),
                ..Default::default()
            };
            1000
        ]));
        let mut viewport = Viewport::new(model.clone().into());
        let mut key = Key {
            revision: 0,
            count: 1000,
            width: 200.0f32.to_bits(),
            line_height: 14.0f32.to_bits(),
        };
        let first = viewport.layout(key.clone(), 0.0, 100.0, &ui.as_weak());
        let measured = viewport.measurement.as_ref().unwrap().wrapped.as_ptr();
        for offset in [100.0, 1000.0, 5000.0, 20000.0, 0.0] {
            let next = viewport.layout(key.clone(), offset, 100.0, &ui.as_weak());
            assert_eq!(next.content_height, first.content_height);
            assert_eq!(
                viewport.measurement.as_ref().unwrap().wrapped.as_ptr(),
                measured
            );
            assert!(next.rows == first.rows);
            assert!(next.rows.row_count() < 12, "projection must stay bounded");
        }
        model.set_row_data(
            50,
            JobLogLineItem {
                text: "long 消息 JSON/path ".repeat(100).into(),
                ..Default::default()
            },
        );
        key.revision += 1;
        let changed = viewport.layout(key.clone(), 0.0, 100.0, &ui.as_weak());
        assert!(changed.content_height > first.content_height);
        key.width = 800.0f32.to_bits();
        let wide = viewport.layout(key, 0.0, 100.0, &ui.as_weak());
        assert!(wide.content_height < changed.content_height);
    }

    #[test]
    fn measurement_keeps_blank_rows_and_cancels_obsolete_work() {
        let request = || Request {
            key: Key {
                revision: 0,
                count: 2,
                width: 120.0f32.to_bits(),
                line_height: 14.0f32.to_bits(),
            },
            lines: LogSnapshot::from_parsed(
                vec![
                    ParsedText {
                        text: "".into(),
                        prefix: "".into(),
                        kind: "plain".into(),
                    },
                    ParsedText {
                        text: "消息\n\nnext".into(),
                        prefix: "Warning: ".into(),
                        kind: "warning".into(),
                    },
                ]
                .into_iter(),
            ),
            collection: LogLayout::new().collection(),
        };
        assert!(measure(request(), || true).is_none());
        let measured = measure(request(), || false).unwrap();
        assert_eq!(
            measured.wrapped[0],
            std::iter::once(0..0).collect::<Vec<_>>()
        );
        assert_eq!(measured.extents[0], 32);
        assert!(measured.extents[1] > 32);
    }
}
