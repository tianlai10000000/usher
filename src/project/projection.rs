//! Project-only selector, one worker preparation, one replaceable UI delivery.
use super::logo_prepare::{LogoCache, PreparedLogo};
use super::model::ProjectConfig;
use super::state::{ProjectState, ProjectStateFacade};
use crate::{MainWindow, ProjectItem};
use slint::{ComponentHandle, Model, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use usher_state_store::{StoreSnapshot, Subscription};

#[derive(Clone)]
struct Selection {
    projects: Arc<Vec<ProjectConfig>>,
    query: String,
    preview: Option<String>,
    logo_revision: u64,
    refresh: u64,
}
impl Selection {
    fn new(state: &ProjectState) -> Self {
        Self {
            projects: state.projects.clone(),
            query: state.logo_query.clone(),
            preview: state.preview_logo_url.clone(),
            logo_revision: state.logo_revision,
            refresh: state.logo_refresh,
        }
    }
    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.projects, &other.projects)
            && self.query == other.query
            && self.preview == other.preview
            && self.logo_revision == other.logo_revision
            && self.refresh == other.refresh
    }
}
struct PreparedView {
    selection: Selection,
    rows: Vec<(usize, Arc<PreparedLogo>)>,
    preview: Arc<PreparedLogo>,
    preview_failed: bool,
    generation: u64,
}
#[derive(Default)]
struct Mailbox {
    selection: Option<Selection>,
    input: Option<StoreSnapshot<ProjectState>>,
    generation: u64,
    pending: Option<PreparedView>,
    queued: bool,
    stopped: bool,
}
impl Mailbox {
    fn accept(&mut self, snapshot: StoreSnapshot<ProjectState>) -> bool {
        if !snapshot.state().hydrated {
            return false;
        }
        let selected = Selection::new(snapshot.state());
        if self.stopped
            || self
                .selection
                .as_ref()
                .is_some_and(|old| old.same(&selected))
        {
            return false;
        }
        self.selection = Some(selected);
        self.input = Some(snapshot);
        self.generation += 1;
        true
    }
    fn publish(&mut self, view: PreparedView) -> bool {
        if self.stopped || view.generation != self.generation {
            return false;
        }
        self.pending = Some(view);
        !std::mem::replace(&mut self.queued, true)
    }
}

pub(super) struct ProjectProjection {
    mailbox: Arc<Mutex<Mailbox>>,
    task: tokio::task::JoinHandle<()>,
    subscription: Mutex<Option<Subscription>>,
}
impl ProjectProjection {
    pub(super) fn start(ui: &MainWindow, state: ProjectStateFacade) -> Self {
        let model = Rc::new(VecModel::default());
        ui.global::<crate::ProjectBridge>()
            .set_items(model.clone().into());
        VIEW.with(|view| {
            *view.borrow_mut() = Some(UiView {
                model,
                previous: None,
            })
        });
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let notify = Arc::new(Notify::new());
        let updates = mailbox.clone();
        let wake = notify.clone();
        let subscription = state.subscribe(Arc::new(move |snapshot| {
            if updates
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .accept(snapshot)
            {
                wake.notify_one();
            }
        }));
        let (start, ready) = tokio::sync::oneshot::channel();
        crate::startup::run_after_first_frame(move || {
            let _ = start.send(());
        });
        let window = ui.as_weak();
        let output = mailbox.clone();
        let task = crate::app_runtime::shared_runtime().spawn(async move {
            let _ = ready.await;
            let mut cache = None;
            loop {
                notify.notified().await;
                let (snapshot, generation) = {
                    let guard = output.lock().unwrap_or_else(|p| p.into_inner());
                    if guard.stopped { break; }
                    (guard.input.clone().expect("hydrated selector"), guard.generation)
                };
                let tasks = state.prepare_pr_overview(snapshot.state().logo_query.clone());
                super::controller::prefetch_project_pr_overview(state.clone(), tasks);
                let cancellation = output.clone();
                let prepared = tokio::task::spawn_blocking(move || {
                    let mut cache = cache.unwrap_or_else(LogoCache::new);
                    let selection = Selection::new(snapshot.state());
                    let mut sources = HashSet::new();
                    let mut rows = Vec::new();
                    for (index, project) in selection.projects.iter().enumerate() {
                        if cancellation.lock().unwrap_or_else(|p| p.into_inner()).stopped { break; }
                        if !matches_query(project, &selection.query) { continue; }
                        sources.insert(project.logo.clone());
                        let version = snapshot.state().logo_version(&project.logo);
                        rows.push((index, cache.prepare(&project.logo, version)));
                    }
                    let preview_source = selection.preview.as_deref().unwrap_or_default();
                    sources.insert(preview_source.to_owned());
                    let preview = cache.prepare(preview_source, snapshot.state().logo_version(preview_source));
                    let preview_failed = snapshot.state().logo_failed(preview_source);
                    cache.retain(&sources);
                    if crate::terminal::diag_logging_enabled() {
                        let source_bytes: u64 = rows.iter().map(|(_, image)| image.source_bytes).sum();
                        let decoded_bytes: u64 = rows.iter().map(|(_, image)| image.decoded_bytes).sum();
                        log::info!("[Project][Diag] configured_items={} visible_items={} logo_source_bytes={source_bytes} logo_decoded_rgba_bytes_estimate={decoded_bytes} decodes={}", selection.projects.len(), rows.len(), cache.decoded);
                    }
                    (cache, PreparedView { selection, rows, preview, preview_failed, generation })
                }).await;
                let Ok((next_cache, view)) = prepared else { break; };
                cache = Some(next_cache);
                let schedule = output.lock().unwrap_or_else(|p| p.into_inner()).publish(view);
                if schedule {
                    let output = output.clone(); let window = window.clone();
                    if slint::invoke_from_event_loop(move || {
                        let view = { let mut slot = output.lock().unwrap_or_else(|p| p.into_inner());
                            slot.queued = false;
                            slot.pending.take().filter(|view| !slot.stopped && view.generation == slot.generation)
                        };
                        if let (Some(view), Some(ui)) = (view, window.upgrade()) {
                            VIEW.with(|current| { if let Some(current) = current.borrow_mut().as_mut() { current.apply(&ui, view); } });
                        }
                    }).is_err() { break; }
                }
            }
        });
        Self {
            mailbox,
            task,
            subscription: Mutex::new(Some(subscription)),
        }
    }
    pub(super) fn shutdown(&self) {
        self.subscription
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let mut slot = self.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        slot.stopped = true;
        slot.pending.take();
        slot.input.take();
        self.task.abort();
    }
}
impl Drop for ProjectProjection {
    fn drop(&mut self) {
        self.shutdown();
    }
}

thread_local! { static VIEW: RefCell<Option<UiView>> = const { RefCell::new(None) }; }
struct UiView {
    model: Rc<VecModel<ProjectItem>>,
    previous: Option<PreparedView>,
}
impl UiView {
    fn apply(&mut self, ui: &MainWindow, next: PreparedView) {
        // The only Image operation here wraps already decoded shared pixels.
        let old: HashMap<&str, (&ProjectConfig, &Arc<PreparedLogo>)> = self
            .previous
            .as_ref()
            .map(|v| {
                v.rows
                    .iter()
                    .map(|(index, logo)| {
                        let project = &v.selection.projects[*index];
                        (project.project_alias.as_str(), (project, logo))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let wanted: HashSet<&str> = next
            .rows
            .iter()
            .map(|(i, _)| next.selection.projects[*i].project_alias.as_str())
            .collect();
        for index in (0..self.model.row_count()).rev() {
            if !wanted.contains(self.model.row_data(index).unwrap().project_alias.as_str()) {
                self.model.remove(index);
            }
        }
        let mut changed = 0;
        for (row, (index, logo)) in next.rows.iter().enumerate() {
            let project = &next.selection.projects[*index];
            if self
                .model
                .row_data(row)
                .is_some_and(|item| item.project_alias == project.project_alias)
            {
                if old
                    .get(project.project_alias.as_str())
                    .is_some_and(|(p, image)| *p == project && Arc::ptr_eq(image, logo))
                {
                    continue;
                }
                self.model.set_row_data(row, to_ui(project, logo));
            } else {
                if let Some(found) = (row..self.model.row_count()).find(|&i| {
                    self.model.row_data(i).unwrap().project_alias == project.project_alias
                }) {
                    let item = self.model.row_data(found).unwrap();
                    self.model.remove(found);
                    self.model.insert(row, item);
                    if !old
                        .get(project.project_alias.as_str())
                        .is_some_and(|(p, image)| *p == project && Arc::ptr_eq(image, logo))
                    {
                        self.model.set_row_data(row, to_ui(project, logo));
                    }
                } else {
                    self.model.insert(row, to_ui(project, logo));
                }
            }
            changed += 1;
        }
        ui.global::<crate::ProjectBridge>()
            .set_logo_preview_image(slint::Image::from_rgba8(next.preview.pixels.clone()));
        ui.global::<crate::ProjectBridge>().set_logo_preview_status(
            crate::i18n::tr(if next.preview_failed {
                "projects.invalid_logo_default"
            } else {
                next.preview.status
            })
            .into_owned()
            .into(),
        );
        if self
            .previous
            .as_ref()
            .is_none_or(|old| old.selection.refresh != next.selection.refresh)
        {
            ui.global::<crate::ProjectBridge>().set_list_loading(false);
        }
        if std::env::var_os("USHER_P5_BENCHMARK").is_some() {
            log::info!(
                "[P5][57] ui_commit rows={} changed={changed}",
                next.rows.len()
            );
        }
        self.previous = Some(next);
    }
}
fn to_ui(project: &ProjectConfig, logo: &PreparedLogo) -> ProjectItem {
    ProjectItem {
        project_name: project.project_name.clone().into(),
        project_alias: project.project_alias.clone().into(),
        local_repo: project.local_repo.clone().into(),
        remote_repo: project.remote_repo.clone().into(),
        repo_type: project.repo_type.clone().into(),
        logo: project.logo.clone().into(),
        logo_image: slint::Image::from_rgba8(logo.pixels.clone()),
        is_pr_auto_update: project.is_pr_auto_update,
        pr_count_loading: project.pr_overview.loading,
        pr_count_failed: project.pr_overview.failed,
        pr_fat_count_text: count(&project.pr_overview.fat_display, project.pr_overview.failed)
            .into(),
        pr_uat_count_text: count(&project.pr_overview.uat_display, project.pr_overview.failed)
            .into(),
        pr_prod_count_text: count(
            &project.pr_overview.prod_display,
            project.pr_overview.failed,
        )
        .into(),
        project_tags_text: project.project_tags.join(", ").into(),
        project_tags: Rc::new(VecModel::from(
            project
                .project_tags
                .iter()
                .cloned()
                .map(Into::into)
                .collect::<Vec<_>>(),
        ))
        .into(),
    }
}
fn count(value: &str, failed: bool) -> &str {
    if failed {
        "unknown"
    } else if value.is_empty() {
        "-"
    } else {
        value
    }
}
fn matches_query(project: &ProjectConfig, query: &str) -> bool {
    query.is_empty()
        || project.project_name.to_lowercase().contains(query)
        || project.project_alias.to_lowercase().contains(query)
}
pub(super) fn sync_language(ui: &MainWindow) {
    VIEW.with(|current| {
        if let Some(view) = current
            .borrow()
            .as_ref()
            .and_then(|view| view.previous.as_ref())
        {
            ui.global::<crate::ProjectBridge>().set_logo_preview_status(
                crate::i18n::tr(if view.preview_failed {
                    "projects.invalid_logo_default"
                } else {
                    view.preview.status
                })
                .into_owned()
                .into(),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn projects() -> Vec<ProjectConfig> {
        serde_json::from_value(serde_json::json!([
            {"project_name":"Alpha","project_alias":"a","local_repo":"","remote_repo":"","project_tags":["Web"]},
            {"project_name":"Beta","project_alias":"b","local_repo":"","remote_repo":""}
        ])).unwrap()
    }
    fn prepared(state: &ProjectState, generation: u64) -> PreparedView {
        let mut cache = LogoCache::new();
        let logo = cache.prepare("", 0);
        PreparedView {
            selection: Selection::new(state),
            rows: (0..state.projects.len())
                .map(|i| (i, logo.clone()))
                .collect(),
            preview: logo,
            preview_failed: false,
            generation,
        }
    }
    #[test]
    fn bookkeeping_keeps_registry_identity_and_does_not_schedule_projection() {
        let state = ProjectStateFacade::memory(projects());
        let before = state.snapshot();
        let mut slot = Mailbox::default();
        assert!(slot.accept(before.clone()));
        for i in 0..200 {
            state.begin_save(i.to_string());
            state.finish_save(i.to_string());
            assert!(!slot.accept(state.snapshot()));
            assert!(Arc::ptr_eq(
                &before.state().projects,
                &state.snapshot().state().projects
            ));
        }
        assert!(slot.publish(prepared(before.state(), 1)));
        assert!(!slot.publish(prepared(before.state(), 1))); // replaces a single delivery
        state.update_logo_inputs(Some("beta".into()), None);
        assert!(slot.accept(state.snapshot()));
        assert!(!slot.publish(prepared(before.state(), 1))); // obsolete search result
        assert_eq!(slot.generation, 2);
        state.shutdown();
    }
    #[test]
    fn overview_change_preserves_model_and_unmodified_row_models() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let state = ProjectStateFacade::memory(projects());
        let model = Rc::new(VecModel::default());
        let identity: slint::ModelRc<ProjectItem> = model.clone().into();
        ui.global::<crate::ProjectBridge>()
            .set_items(identity.clone());
        let mut view = UiView {
            model,
            previous: None,
        };
        let first = prepared(state.snapshot().state(), 1);
        let logo = first.rows[0].1.clone();
        view.apply(&ui, first);
        let unchanged = view.model.row_data(0).unwrap();
        state.complete_pr_overview("b".into(), Some((7, 8, 9)));
        let mut next = prepared(state.snapshot().state(), 2);
        for (_, image) in &mut next.rows {
            *image = logo.clone();
        }
        view.apply(&ui, next);
        assert_eq!(ui.global::<crate::ProjectBridge>().get_items(), identity);
        assert_eq!(
            view.model.row_data(0).unwrap().project_tags,
            unchanged.project_tags
        );
        assert_eq!(
            view.model.row_data(0).unwrap().logo_image,
            unchanged.logo_image
        );
        assert_eq!(view.model.row_data(1).unwrap().pr_fat_count_text, "7");
        // Filtering uses insert/remove on the existing outer model.
        let mut next = prepared(state.snapshot().state(), 3);
        next.rows.remove(0);
        view.apply(&ui, next);
        assert_eq!(ui.global::<crate::ProjectBridge>().get_items(), identity);
        assert_eq!(view.model.row_count(), 1);
        assert_eq!(view.model.row_data(0).unwrap().project_alias, "b");
        state.shutdown();
    }
}
