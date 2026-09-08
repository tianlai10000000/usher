//! Event-loop ownership of the bounded background Find mailbox and UI models.
use super::find::collect_diff_find_row_matches;
use super::find_worker::{FindModel, FindResults, FindSource, scan, side_key};
use crate::app_runtime::latest_compute::LatestCompute;
use crate::{MainWindow, WorkflowPrDiffFindMatch, WorkflowPrDiffLine};
use slint::{ComponentHandle, ModelRc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_BRIDGE: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static BRIDGE: RefCell<Weak<FindBridge>> = RefCell::default();
}
struct Request {
    source: FindSource,
    query: String,
    split: bool,
}
struct FindBridge {
    id: u64,
    worker: LatestCompute<Request, FindResults>,
    model: Rc<FindModel>,
    key: RefCell<Option<(ModelRc<WorkflowPrDiffLine>, String, bool)>>,
    rows: RefCell<HashMap<(i32, u8), ModelRc<WorkflowPrDiffFindMatch>>>,
}
impl FindBridge {
    fn cancel(&self) {
        self.worker.cancel();
        self.key.borrow_mut().take();
        self.rows.borrow_mut().clear();
        self.model.replace(None);
    }
    fn matches(
        &self,
        ui: &MainWindow,
        query: &str,
        split: bool,
    ) -> ModelRc<WorkflowPrDiffFindMatch> {
        let query = query.trim();
        let lines = ui.get_workflow_pr_review_lines();
        if self
            .key
            .borrow()
            .as_ref()
            .is_some_and(|(old, q, s)| old == &lines && q == query && *s == split)
        {
            return self.model.clone().into();
        }
        self.cancel();
        if query.is_empty() {
            return self.model.clone().into();
        }
        let Some(source) = FindSource::capture(&lines) else {
            return self.model.clone().into();
        };
        *self.key.borrow_mut() = Some((lines, query.into(), split));
        let weak = ui.as_weak();
        let worker = self.worker.clone();
        let id = self.id;
        self.worker.submit(
            Request {
                source,
                query: query.into(),
                split,
            },
            |request, token| {
                scan(request.source, request.query, request.split, || {
                    token.cancelled()
                })
            },
            move || {
                let weak = weak.clone();
                let worker = worker.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    worker.acknowledge();
                    let Some(result) = worker.take_output() else {
                        return;
                    };
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    let bridge = BRIDGE.with(|slot| slot.borrow().upgrade());
                    let Some(bridge) = bridge else {
                        return;
                    };
                    if bridge.id != id {
                        return;
                    }
                    let current =
                        bridge
                            .key
                            .borrow()
                            .as_ref()
                            .is_some_and(|(lines, query, split)| {
                                lines == &ui.get_workflow_pr_review_lines()
                                    && query == &result.query
                                    && *split == (ui.get_workflow_pr_review_diff_view() == "split")
                            });
                    if current {
                        bridge.rows.borrow_mut().clear();
                        bridge.model.replace(Some(Arc::new(result)));
                        ui.set_workflow_pr_review_find_result_revision(
                            ui.get_workflow_pr_review_find_result_revision()
                                .wrapping_add(1),
                        );
                    } else {
                        bridge.cancel();
                    }
                });
            },
        );
        self.model.clone().into()
    }
    fn row(
        &self,
        ui: &MainWindow,
        line: i32,
        side: &str,
        query: &str,
    ) -> ModelRc<WorkflowPrDiffFindMatch> {
        let lines = ui.get_workflow_pr_review_lines();
        let current = self
            .key
            .borrow()
            .as_ref()
            .is_some_and(|(old, q, _)| old == &lines && q == query.trim());
        if current {
            if let Some(result) = self.model.result.borrow().as_ref() {
                let key = (line, side_key(side));
                let mut rows = self.rows.borrow_mut();
                if let Some(model) = rows.get(&key) {
                    return model.clone();
                }
                if rows.len() >= 128 {
                    rows.clear();
                }
                let model = result.row_model(line, side);
                rows.insert(key, model.clone());
                return model;
            }
        }
        collect_diff_find_row_matches(&lines, line, side, query)
    }
}
impl Drop for FindBridge {
    fn drop(&mut self) {
        self.worker.cancel();
    }
}
pub(super) fn cancel() {
    let bridge = BRIDGE.with(|slot| slot.borrow().upgrade());
    if let Some(bridge) = bridge {
        bridge.cancel();
    }
}
pub(super) fn bind(ui: &MainWindow) {
    let bridge = Rc::new(FindBridge {
        id: NEXT_BRIDGE.fetch_add(1, Ordering::Relaxed),
        worker: LatestCompute::new(),
        model: Rc::new(FindModel::new()),
        key: RefCell::new(None),
        rows: RefCell::new(HashMap::new()),
    });
    BRIDGE.with(|slot| *slot.borrow_mut() = Rc::downgrade(&bridge));
    let weak = ui.as_weak();
    let full = bridge.clone();
    ui.global::<crate::PrDiffBridge>()
        .on_workflow_pr_review_find_diff_matches(move |query, split| {
            weak.upgrade().map_or_else(ModelRc::default, |ui| {
                full.matches(&ui, query.as_str(), split)
            })
        });
    let weak = ui.as_weak();
    ui.global::<crate::PrDiffBridge>()
        .on_workflow_pr_review_find_diff_row_matches(move |line, side, query| {
            weak.upgrade().map_or_else(ModelRc::default, |ui| {
                bridge.row(&ui, line, side.as_str(), query.as_str())
            })
        });
}
