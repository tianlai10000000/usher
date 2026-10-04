#[cfg(any(test, feature = "test-fixtures"))]
mod fixture;
mod projection;
mod state;

use crate::app_runtime::latest_compute::LatestCompute;
use crate::git::conflicts::{self, ConflictError, ConflictSnapshot};
use crate::workflow::pr_conflict_status::{Key, shared};
use crate::{MainWindow, PrConflictFile, PrConflictsBridge};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use state::{Action, SessionFacade};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
type PreparedWork = (u64, u64, i32, projection::Prepared);
type Preparation = (u64, u64, i32, conflicts::ConflictFile);

#[derive(Clone, PartialEq, Eq)]
struct Context {
    alias: String,
    number: String,
    source: String,
    target: String,
}
#[derive(Clone, Default)]
pub(super) struct ConflictController {
    state: SessionFacade,
    context: Arc<Mutex<Option<Context>>>,
    cancellation: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    generation: Arc<AtomicU64>,
    selection: Arc<AtomicU64>,
    preparation: Arc<std::sync::OnceLock<LatestCompute<Preparation, PreparedWork>>>,
    loads: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}
impl ConflictController {
    pub(super) fn bind(&self, ui: &MainWindow) {
        bind_geometry(ui);
        #[cfg(feature = "test-fixtures")]
        fixture::setup(self, ui);
        let cancellation = self.cancellation.clone();
        let loads = self.loads.clone();
        crate::shutdown::register_async_drain("cancel_pr_conflicts", move || {
            Box::pin(async move {
                if let Some(cancel) = cancellation.lock().unwrap().as_ref() {
                    cancel.store(true, Ordering::Relaxed);
                }
                let handles = std::mem::take(&mut *loads.lock().unwrap());
                for handle in handles {
                    handle.abort();
                    let _ = handle.await;
                }
            })
        });
        let controller = self.clone();
        let weak = ui.as_weak();
        ui.global::<PrConflictsBridge>().on_refresh(move || {
            if let Some(ui) = weak.upgrade() {
                controller.load(&ui);
            }
        });
        let controller = self.clone();
        let weak = ui.as_weak();
        ui.global::<PrConflictsBridge>()
            .on_select_file(move |index| {
                if let Some(ui) = weak.upgrade() {
                    controller.select_file(&ui, index);
                }
            });
        let controller = self.clone();
        let weak = ui.as_weak();
        ui.global::<PrConflictsBridge>()
            .on_step_block(move |delta| {
                let Some(ui) = weak.upgrade() else { return };
                let b = ui.global::<PrConflictsBridge>();
                let Some(snapshot) = controller.state.snapshot().snapshot else {
                    return;
                };
                let Some(file) = snapshot.files.get(b.get_selected_file() as usize) else {
                    return;
                };
                if file.blocks.is_empty() {
                    return;
                }
                let next = (b.get_selected_block() + delta).clamp(0, file.blocks.len() as i32 - 1);
                b.set_selected_block(next);
                b.set_reveal_line(file.blocks[next as usize].start as i32);
                b.set_revision(b.get_revision() + 1);
            });
        let controller = self.clone();
        let weak = ui.as_weak();
        ui.global::<PrConflictsBridge>().on_copy(move || {
            let Some(ui) = weak.upgrade() else { return };
            let b = ui.global::<PrConflictsBridge>();
            if !b.get_active() || b.get_loading() || b.get_copy_busy() {
                return;
            }
            let Some(snapshot) = controller.state.snapshot().snapshot else {
                return;
            };
            if let Some(text) = snapshot
                .files
                .get(ui.global::<PrConflictsBridge>().get_selected_file() as usize)
                .and_then(|f| f.content.clone())
            {
                b.set_copy_busy(true);
                let generation = controller.generation.load(Ordering::SeqCst);
                let selection = controller.selection.load(Ordering::SeqCst);
                let controller = controller.clone();
                let weak = ui.as_weak();
                crate::app_runtime::shared_runtime().spawn_blocking(move || {
                    let success = copy_text(text.to_string());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            controller.finish_copy(&ui, generation, selection, success);
                        }
                    });
                });
            }
        });
        let controller = self.clone();
        let weak = ui.as_weak();
        ui.global::<PrConflictsBridge>().on_copy_selection(
            move |start_line, start_col, end_line, end_col| {
                let Some(ui) = weak.upgrade() else { return };
                let Some(snapshot) = controller.state.snapshot().snapshot else {
                    return;
                };
                let Some(content) = snapshot
                    .files
                    .get(ui.global::<PrConflictsBridge>().get_selected_file() as usize)
                    .and_then(|f| f.content.clone())
                else {
                    return;
                };
                crate::app_runtime::shared_runtime().spawn_blocking(move || {
                    let mut parts = Vec::new();
                    for (index, text) in content
                        .lines()
                        .enumerate()
                        .skip(start_line.max(0) as usize)
                        .take((end_line - start_line + 1).max(0) as usize)
                    {
                        let line = index as i32;
                        let text = super::hunk::sanitize_diff_content(text);
                        let start = if line == start_line {
                            start_col.max(0) as usize
                        } else {
                            0
                        };
                        let end = if line == end_line {
                            end_col.max(0) as usize
                        } else {
                            text.chars().count()
                        };
                        parts.push(
                            text.chars()
                                .skip(start)
                                .take(end.saturating_sub(start))
                                .collect::<String>(),
                        );
                    }
                    copy_text(parts.join("\n"));
                });
            },
        );
    }
    fn finish_copy(&self, ui: &MainWindow, generation: u64, selection: u64, success: bool) {
        let b = ui.global::<PrConflictsBridge>();
        if !b.get_active()
            || self.generation.load(Ordering::SeqCst) != generation
            || self.selection.load(Ordering::SeqCst) != selection
        {
            return;
        }
        b.set_copy_busy(false);
        if success {
            b.set_copy_completed(b.get_copy_completed().wrapping_add(1));
        } else {
            crate::workflow::bindings::show_error(ui, &crate::i18n::tr("conflicts.copy_failed"));
        }
    }
    pub(super) fn open(
        &self,
        ui: &MainWindow,
        alias: String,
        number: String,
        source: String,
        target: String,
    ) {
        let context = Context {
            alias,
            number,
            source,
            target,
        };
        let b = ui.global::<PrConflictsBridge>();
        if self.context.lock().unwrap().as_ref() == Some(&context) {
            if b.get_active() {
                return;
            }
            b.set_active(true);
            if b.get_loading() {
                return;
            }
            if let Some(snapshot) = self.state.snapshot().snapshot {
                self.present(ui, &snapshot);
                return;
            }
        }
        *self.context.lock().unwrap() = Some(context);
        b.set_active(true);
        self.load(ui);
    }
    pub(super) fn prefetch(
        &self,
        ui: &MainWindow,
        alias: String,
        number: String,
        source: String,
        target: String,
    ) {
        let b = ui.global::<PrConflictsBridge>();
        let context = Context {
            alias,
            number,
            source,
            target,
        };
        if b.get_active()
            || b.get_file_count() >= 0
            || self.context.lock().unwrap().as_ref() == Some(&context)
        {
            return;
        }
        *self.context.lock().unwrap() = Some(context);
        self.load(ui);
    }
    pub(super) fn close(&self, ui: &MainWindow) {
        self.leave_workspace(ui);
        ui.global::<PrConflictsBridge>().set_file_count(-1);
    }
    pub(super) fn leave_workspace(&self, ui: &MainWindow) {
        self.cancel_loads();
        if let Some(worker) = self.preparation.get() {
            worker.cancel();
        }
        if let Some(cancel) = self.cancellation.lock().unwrap().take() {
            cancel.store(true, Ordering::Relaxed);
        }
        *self.context.lock().unwrap() = None;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let state = self.state.clone();
        crate::app_runtime::shared_runtime()
            .spawn_blocking(move || state.dispatch(Action::Reset(generation)));
        let b = ui.global::<PrConflictsBridge>();
        b.set_active(false);
        b.set_copy_busy(false);
        b.set_copy_completed(0);
        b.set_loading(false);
        b.set_rows(ModelRc::default());
        b.set_files(ModelRc::default());
        b.set_repository("".into());
        b.set_number("".into());
        b.set_status("unknown".into());
    }
    fn load(&self, ui: &MainWindow) {
        self.cancel_loads();
        if let Some(worker) = self.preparation.get() {
            worker.cancel();
        }
        let Some(context) = self.context.lock().unwrap().clone() else {
            return;
        };
        if let Some(cancel) = self.cancellation.lock().unwrap().take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.cancellation.lock().unwrap() = Some(cancel.clone());
        let b = ui.global::<PrConflictsBridge>();
        b.set_loading(true);
        b.set_file_count(-1);
        b.set_copy_busy(false);
        b.set_error_key("".into());
        b.set_rows(ModelRc::default());
        b.set_files(ModelRc::default());
        b.set_selected_file(-1);
        b.set_block_count(0);
        b.set_selected_block(-1);
        b.set_text_available(false);
        b.set_local(false);
        b.set_status("checking".into());
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let controller = self.clone();
        let weak = ui.as_weak();
        let handle = crate::app_runtime::shared_runtime().spawn(async move {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            controller.state.dispatch(Action::Reset(generation));
            let alias = context.alias.clone();
            let metadata = tokio::task::spawn_blocking(move || {
                crate::project::load_repository_metadata(&alias)
            })
            .await
            .ok()
            .and_then(Result::ok);
            #[cfg(feature = "test-fixtures")]
            let fixture = fixture::load_snapshot(&context.alias).await;
            #[cfg(not(feature = "test-fixtures"))]
            let fixture: Option<ConflictSnapshot> = None;
            let result = if let Some(snapshot) = fixture {
                Ok(Arc::new(snapshot))
            } else if let Some(meta) = metadata.as_ref() {
                let identity = Key::new(meta, &context.number);
                let conflict_scope = shared().scope();
                let result = conflicts::load(
                    &meta.repo_type,
                    &meta.repo_project_id,
                    &meta.remote_repo,
                    &context.number,
                    &context.source,
                    &context.target,
                    cancel.clone(),
                )
                .await;
                let result = if identity == Key::new(meta, &context.number) {
                    result
                } else {
                    Err(ConflictError::Stale)
                };
                if let Ok(snapshot) = &result {
                    if !cancel.load(Ordering::Relaxed) && !snapshot.local {
                        shared().observe(conflict_scope, identity, snapshot.status);
                    }
                }
                result.map(Arc::new)
            } else {
                Err(ConflictError::Unavailable)
            };
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            controller
                .state
                .dispatch(Action::Loaded(generation, result.clone()));
            let _ = slint::invoke_from_event_loop(move || {
                if cancel.load(Ordering::Relaxed)
                    || controller.generation.load(Ordering::SeqCst) != generation
                    || controller.state.snapshot().generation != generation
                {
                    return;
                }
                let Some(ui) = weak.upgrade() else { return };
                let b = ui.global::<PrConflictsBridge>();
                b.set_loading(false);
                if let Some(meta) = metadata {
                    b.set_repository(Key::new(&meta, &context.number).repository.into());
                }
                b.set_number(context.number.into());
                controller.finish_load(&ui, generation, result);
            });
        });
        self.loads.lock().unwrap().push(handle);
    }
    fn cancel_loads(&self) {
        for handle in self.loads.lock().unwrap().drain(..) {
            handle.abort();
        }
    }
    fn finish_load(
        &self,
        ui: &MainWindow,
        generation: u64,
        result: Result<Arc<ConflictSnapshot>, ConflictError>,
    ) {
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let b = ui.global::<PrConflictsBridge>();
        b.set_loading(false);
        match result {
            Ok(snapshot) => {
                b.set_file_count(snapshot.files.len() as i32);
                b.set_status(snapshot.status.as_str().into());
                if b.get_active() {
                    self.present(ui, &snapshot);
                }
            }
            Err(error) => {
                b.set_status("unknown".into());
                b.set_error_key(error.key().into());
            }
        }
    }
    fn present(&self, ui: &MainWindow, snapshot: &ConflictSnapshot) {
        let b = ui.global::<PrConflictsBridge>();
        b.set_file_count(snapshot.files.len() as i32);
        b.set_status(snapshot.status.as_str().into());
        b.set_local(snapshot.local);
        b.set_source_branch(snapshot.source_branch.clone().into());
        b.set_target_branch(snapshot.target_branch.clone().into());
        b.set_files(ModelRc::new(VecModel::from(
            snapshot
                .files
                .iter()
                .map(|f| PrConflictFile {
                    name: f.path.rsplit('/').next().unwrap_or(&f.path).into(),
                    path: f.path.clone().into(),
                    target_path: f.target_path.clone().into(),
                    count: f.blocks.len() as i32,
                })
                .collect::<Vec<_>>(),
        )));
        if !snapshot.files.is_empty() {
            self.select_file(ui, 0);
        }
    }
    fn select_file(&self, ui: &MainWindow, index: i32) {
        let session = self.state.snapshot();
        let Some(snapshot) = session.snapshot else {
            return;
        };
        let Some(file) = snapshot.files.get(index as usize).cloned() else {
            return;
        };
        let b = ui.global::<PrConflictsBridge>();
        b.set_selected_file(index);
        b.set_copy_busy(false);
        b.set_rows(ModelRc::default());
        b.set_loading(true);
        b.set_text_available(file.content.is_some());
        b.set_block_count(file.blocks.len() as i32);
        b.set_selected_block(if file.blocks.is_empty() { -1 } else { 0 });
        let state = self.state.clone();
        let weak = ui.as_weak();
        let selection = self.selection.fetch_add(1, Ordering::SeqCst) + 1;
        let selection_guard = self.selection.clone();
        let worker = self.preparation.get_or_init(LatestCompute::new).clone();
        let completion = worker.clone();
        worker.submit(
            (session.generation, selection, index, file),
            move |(generation, selection, index, file), token| {
                if token.cancelled() {
                    return None;
                }
                state.dispatch(Action::Select(generation, selection));
                let prepared = projection::prepare(file);
                (!token.cancelled()).then_some((generation, selection, index, prepared))
            },
            move || {
                let completion = completion.clone();
                let weak = weak.clone();
                let selection_guard = selection_guard.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    completion.acknowledge();
                    let Some((_generation, selection, index, rows)) = completion.take_output()
                    else {
                        return;
                    };
                    if selection_guard.load(Ordering::SeqCst) != selection {
                        return;
                    }
                    let Some(ui) = weak.upgrade() else { return };
                    let b = ui.global::<PrConflictsBridge>();
                    if !b.get_active() || b.get_selected_file() != index {
                        return;
                    }
                    b.set_loading(false);
                    b.set_max_columns(rows.max_columns());
                    let first_conflict = rows.first_conflict();
                    b.set_rows(ModelRc::new(rows.into_model()));
                    b.set_reveal_line(first_conflict);
                    b.set_revision(b.get_revision() + 1);
                });
            },
        );
    }
}

fn copy_text(text: String) -> bool {
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
        Ok(()) => true,
        Err(error) => {
            log::warn!("Conflict clipboard operation failed: {error}");
            false
        }
    }
}

fn bind_geometry(ui: &MainWindow) {
    use std::cell::RefCell;
    use usher_virtual_list::{VirtualList, VirtualWindow};
    let layout = RefCell::new((0usize, VirtualList::new(&[], 4), VirtualWindow::default()));
    ui.global::<PrConflictsBridge>()
        .on_layout(move |count, offset, height| {
            let count = count.max(0) as usize;
            let mut state = layout.borrow_mut();
            let (old, list, window) = &mut *state;
            if *old != count {
                list.replace_item_extents(&vec![28; count], 4);
                *old = count;
            }
            list.set_viewport(offset.max(0.0) as u64, height.max(1.0) as u32);
            list.write_window(window);
            crate::PrConflictWindow {
                start: window.items.first().map_or(0, |item| item.index as i32),
                count: window.items.len() as i32,
                total: window.content_extent as f32,
            }
        });
    let weak = ui.as_weak();
    ui.global::<PrConflictsBridge>()
        .on_resolve_hit(move |start, x, y, family, size| {
            let Some(ui) = weak.upgrade() else {
                return crate::WorkflowPrDiffSelectionHit::default();
            };
            let rows = ui.global::<PrConflictsBridge>().get_rows();
            let index = (start + (y / 28.0).floor() as i32)
                .clamp(0, rows.row_count().saturating_sub(1) as i32);
            let column = rows.row_data(index as usize).map_or(0, |row| {
                super::text_geometry::hit_test_runs(
                    (0..row.tokens.row_count()).filter_map(|i| {
                        row.tokens
                            .row_data(i)
                            .map(|t| (t.text, if t.bold { 600 } else { 400 }, t.italic))
                    }),
                    x,
                    family.as_str(),
                    size,
                    ui.window().scale_factor(),
                )
            });
            crate::WorkflowPrDiffSelectionHit {
                line: index,
                column,
            }
        });
}
