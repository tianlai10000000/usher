//! Exact log extents, measured off-loop; scrolling only projects a bounded window.
use super::log_layout::LogLayout;
use super::log_model::{LogModel, LogSnapshot, ParsedText};
use crate::app_runtime::latest_compute::LatestCompute;
use crate::{JobLogLineItem, JobLogVirtualRow, JobLogWindow, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use usher_virtual_list::{VirtualList, VirtualWindow};

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
    Some(Measurement {
        key: request.key,
        lines: request.lines,
        wrapped,
        extents,
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
}

impl Viewport {
    fn new(model: ModelRc<JobLogLineItem>) -> Self {
        Self {
            collection: LogLayout::new().collection(),
            model,
            requested: None,
            measurement: None,
            worker: LatestCompute::new(),
            list: VirtualList::new(&[], 2),
            window: VirtualWindow::default(),
            rows: Rc::new(VecModel::default()),
            indices: Vec::new(),
        }
    }

    fn install(&mut self, measurement: Measurement) {
        self.list.replace_item_extents(&measurement.extents, 2);
        self.measurement = Some(measurement);
        self.indices.clear();
        self.rows.set_vec(Vec::new());
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
            let lines = if let Some(model) = self.model.as_any().downcast_ref::<LogModel>() {
                model.measurement_lines()
            } else {
                LogSnapshot::from_parsed(self.model.iter().map(|line| ParsedText {
                    text: line.text.to_string(),
                    prefix: line.prefix.to_string(),
                    kind: line.kind.to_string(),
                }))
            };
            self.requested = Some(key.clone());
            let request = Request {
                key: key.clone(),
                lines,
                collection: self.collection.clone(),
            };
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

pub(super) fn setup(ui: &MainWindow) {
    let viewports = RefCell::new(Vec::<Viewport>::new());
    let weak = ui.as_weak();
    ui.global::<crate::JobLogLayout>().on_layout_window(
        move |model, revision, width, offset, height, line_height, _epoch| {
            let mut viewports = viewports.borrow_mut();
            let index = if let Some(index) = viewports.iter().position(|view| view.model == model) {
                index
            } else {
                // Bounded cache across route/source switches, including independent disclosures.
                if viewports.len() == 16 {
                    viewports.remove(0);
                }
                viewports.push(Viewport::new(model.clone()));
                viewports.len() - 1
            };
            let window = viewports[index].layout(
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
            if super::retention_benchmark::enabled() {
                let mut bytes = super::retention_benchmark::Retained::default();
                for view in viewports.iter() {
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
            window
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
