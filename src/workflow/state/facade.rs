//! Bounded ordered Workflow admission and automatic revisioned UI projection.
use super::reducer::{WorkflowAction, WorkflowEffect, WorkflowOutput};
use super::{WorkflowProjectContext, WorkflowStore, initialize_ui};
use slint::ComponentHandle;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use usher_state_store::{LatestDelivery, StateStore, StoreSnapshot, Subscription};

const CAPACITY: usize = 64;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowIdentity {
    pub(crate) project: String,
    pub(crate) generation: u64,
}
enum Command {
    Action(
        WorkflowIdentity,
        WorkflowAction,
        Arc<tokio::sync::OwnedSemaphorePermit>,
    ),
    Barrier(tokio::sync::oneshot::Sender<()>),
}
struct Admission {
    sender: Option<tokio::sync::mpsc::Sender<Command>>,
    identity: WorkflowIdentity,
    draft: Option<(String, String, String)>,
}
struct Inner {
    store: Arc<dyn StateStore<WorkflowStore, WorkflowAction, WorkflowOutput>>,
    admission: Mutex<Admission>,
    closing: AtomicBool,
    owners: Arc<tokio::sync::Semaphore>,
    projection: Arc<LatestDelivery<StoreSnapshot<WorkflowStore>>>,
    subscription: Mutex<Option<Subscription>>,
    window: Mutex<Option<slint::Weak<crate::MainWindow>>>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    pub(super) cancellation: Arc<super::super::tag::activity::TagActivity>,
    stopped: tokio::sync::Notify,
    finished: AtomicBool,
    shutdown: tokio::sync::Mutex<()>,
}
#[derive(Clone)]
pub(crate) struct SharedWorkflowState {
    inner: Arc<Inner>,
    owner: Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
}
impl SharedWorkflowState {
    pub(crate) fn new() -> Self {
        let store = super::reducer::create_store();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(CAPACITY);
        let facade = Self {
            inner: Arc::new(Inner {
                store: store.clone(),
                admission: Mutex::new(Admission {
                    sender: Some(sender),
                    identity: WorkflowIdentity {
                        project: "default".into(),
                        generation: 0,
                    },
                    draft: None,
                }),
                closing: AtomicBool::new(false),
                owners: Arc::new(tokio::sync::Semaphore::new(CAPACITY)),
                projection: Arc::new(LatestDelivery::default()),
                subscription: Mutex::new(None),
                window: Mutex::new(None),
                tasks: Mutex::new(Vec::new()),
                cancellation: Arc::default(),
                stopped: tokio::sync::Notify::new(),
                finished: AtomicBool::new(false),
                shutdown: tokio::sync::Mutex::new(()),
            }),
            owner: None,
        };
        let weak = Arc::downgrade(&facade.inner);
        crate::app_runtime::shared_runtime().spawn(async move {
            while let Some(command) = receiver.recv().await {
                match command {
                    Command::Barrier(done) => {
                        let _ = done.send(());
                    }
                    Command::Action(identity, action, owner) => {
                        let Some(inner) = weak.upgrade() else {
                            break;
                        };
                        let state = store.snapshot();
                        if !matches!(&action, WorkflowAction::SelectProject(_))
                            && state.state().identity != identity
                        {
                            continue;
                        }
                        let draft = draft_input(&action);
                        let reducer_store = store.clone();
                        let reduced = tokio::task::spawn_blocking(move || {
                            let action =
                                super::super::tag::state::prepare_action(state.state(), action);
                            reducer_store.dispatch(action)
                        })
                        .await;
                        let facade = Self {
                            inner,
                            owner: Some(owner),
                        };
                        if let Some(draft) = draft {
                            let mut pending = facade.inner.admission.lock().unwrap();
                            if pending.identity == identity
                                && pending.draft.as_ref() == Some(&draft)
                            {
                                pending.draft = None;
                            }
                        }
                        match reduced {
                            Ok(commit) => {
                                for effect in commit.output.effects {
                                    facade.execute(identity.clone(), effect);
                                }
                            }
                            Err(error) => log::error!("[Workflow] Reducer task failed: {error}"),
                        }
                    }
                }
            }
            if let Some(inner) = weak.upgrade() {
                inner.finished.store(true, Ordering::Release);
                inner.stopped.notify_waiters();
            }
        });
        facade
    }
    pub(crate) fn snapshot(&self) -> StoreSnapshot<WorkflowStore> {
        self.inner.store.snapshot()
    }
    pub(crate) fn identity(&self) -> WorkflowIdentity {
        self.inner.admission.lock().unwrap().identity.clone()
    }
    pub(crate) fn submit(&self, action: WorkflowAction) -> Result<(), &'static str> {
        let mut admission = self.inner.admission.lock().unwrap();
        if self.inner.closing.load(Ordering::Acquire) {
            return Err("Workflow is closing");
        }
        let draft = draft_input(&action);
        let owner = Arc::new(
            self.inner
                .owners
                .clone()
                .try_acquire_owned()
                .map_err(|_| "Workflow operation owners are full")?,
        );
        admission
            .sender
            .as_ref()
            .ok_or("Workflow is closed")?
            .try_send(Command::Action(admission.identity.clone(), action, owner))
            .map_err(|_| "Workflow queue is full or closed")?;
        if draft.is_some() {
            admission.draft = draft;
        }
        Ok(())
    }
    pub(crate) fn select_project(
        &self,
        context: WorkflowProjectContext,
    ) -> Result<(), &'static str> {
        let mut admission = self.inner.admission.lock().unwrap();
        if self.inner.closing.load(Ordering::Acquire) {
            return Err("Workflow is closing");
        }
        let owner = Arc::new(
            self.inner
                .owners
                .clone()
                .try_acquire_owned()
                .map_err(|_| "Workflow operation owners are full")?,
        );
        let identity = WorkflowIdentity {
            project: context.id.clone(),
            generation: admission.identity.generation.wrapping_add(1),
        };
        admission
            .sender
            .as_ref()
            .ok_or("Workflow is closed")?
            .try_send(Command::Action(
                identity.clone(),
                WorkflowAction::SelectProject((context, identity.clone())),
                owner,
            ))
            .map_err(|_| "Workflow queue is full or closed")?;
        admission.identity = identity;
        admission.draft = None;
        Ok(())
    }
    pub(crate) async fn publish(&self, identity: WorkflowIdentity, action: WorkflowAction) -> bool {
        let sender = self.inner.admission.lock().unwrap().sender.clone();
        let owner = match self.owner.clone() {
            Some(owner) => owner,
            None => match self.inner.owners.clone().acquire_owned().await {
                Ok(owner) => Arc::new(owner),
                Err(_) => return false,
            },
        };
        match sender {
            Some(sender) => sender
                .send(Command::Action(identity, action, owner))
                .await
                .is_ok(),
            None => false,
        }
    }
    pub(crate) fn install(&self, window: &crate::MainWindow) {
        *self.inner.window.lock().unwrap() = Some(window.as_weak());
        let mut subscription = self.inner.subscription.lock().unwrap();
        if subscription.is_some() {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        *subscription = Some(self.inner.store.subscribe(Arc::new(move |snapshot| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let facade = Self { inner, owner: None };
            if !facade.inner.projection.offer(snapshot.revision(), snapshot) {
                return;
            }
            let weak = Arc::downgrade(&facade.inner);
            let window = facade.inner.window.lock().unwrap().clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let facade = Self { inner, owner: None };
                if let Some(snapshot) = facade.inner.projection.take()
                    && snapshot.state().identity == facade.identity()
                    && let Some(window) = window.and_then(|window| window.upgrade())
                {
                    facade.project_ui(&window, &snapshot);
                }
            });
        })));
    }
    pub(crate) fn project_ui(
        &self,
        window: &crate::MainWindow,
        snapshot: &StoreSnapshot<WorkflowStore>,
    ) {
        let draft = self.inner.admission.lock().unwrap().draft.clone();
        initialize_ui(
            window,
            &snapshot.state().current_project,
            snapshot.state().current_state_ref(),
        );
        if let Some((next, target, description)) = draft {
            window.set_workflow_tag_next_version(next.into());
            window.set_workflow_tag_remove_target(target.into());
            window.set_workflow_tag_description(description.into());
        }
    }
    pub(crate) fn post(
        &self,
        identity: WorkflowIdentity,
        effect: impl FnOnce(&crate::MainWindow) + Send + 'static,
    ) {
        let owner = self.owner.clone();
        let weak = Arc::downgrade(&self.inner);
        let Some(window) = self.inner.window.lock().unwrap().clone() else {
            return;
        };
        let _ = slint::invoke_from_event_loop(move || {
            let _owner = owner;
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if (Self { inner, owner: None }).identity() == identity
                && let Some(window) = window.upgrade()
            {
                effect(&window);
            }
        });
    }
    pub(crate) fn spawn(&self, task: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut tasks = self.inner.tasks.lock().unwrap();
        tasks.retain(|task| !task.is_finished());
        tasks.push(crate::app_runtime::shared_runtime().spawn(task));
    }
    pub(crate) fn activity(&self) -> Arc<super::super::tag::activity::TagActivity> {
        self.inner.cancellation.clone()
    }
    pub(crate) fn is_closing(&self) -> bool {
        self.inner.closing.load(Ordering::Acquire)
    }
    fn execute(&self, identity: WorkflowIdentity, effect: WorkflowEffect) {
        match effect {
            WorkflowEffect::OpenProject => {
                let facade = self.clone();
                self.post(identity, move |window| {
                    if facade.is_closing() {
                        return;
                    }
                    super::initialize_current_ui(window, &facade);
                    window.invoke_navigate(crate::Route::Workflow);
                });
            }
            WorkflowEffect::CdCompleted { title, summary } => self.post(identity, move |window| {
                super::super::cd::show_completion(window, &title.resolve(), &summary.resolve())
            }),
            WorkflowEffect::Tag(effect) => {
                super::super::tag::controller::execute(self.clone(), identity, effect)
            }
            WorkflowEffect::Cd(request) => {
                super::super::cd::execute(self.clone(), identity, request)
            }
            WorkflowEffect::Toast(success, text) => self.post(identity, move |window| {
                let message = text.resolve();
                if success {
                    super::super::bindings::show_success(window, &message);
                } else {
                    super::super::bindings::show_error(window, &message);
                }
            }),
        }
    }
    pub(crate) async fn shutdown_async(&self) {
        let _shutdown = self.inner.shutdown.lock().await;
        if self.inner.finished.load(Ordering::Acquire) {
            return;
        }
        self.inner.closing.store(true, Ordering::Release);
        self.inner.owners.close();
        let sender = self.inner.admission.lock().unwrap().sender.clone();
        if let Some(sender) = sender {
            let (done, wait) = tokio::sync::oneshot::channel();
            let _ = sender.send(Command::Barrier(done)).await;
            let _ = wait.await;
        }
        self.activity().cancel_all();
        loop {
            let tasks = std::mem::take(&mut *self.inner.tasks.lock().unwrap());
            for task in tasks {
                let _ = task.await;
            }
            let sender = self.inner.admission.lock().unwrap().sender.clone();
            if let Some(sender) = sender {
                let (done, wait) = tokio::sync::oneshot::channel();
                let _ = sender.send(Command::Barrier(done)).await;
                let _ = wait.await;
            }
            if self.inner.tasks.lock().unwrap().is_empty() {
                break;
            }
            self.activity().cancel_all();
        }
        self.inner.admission.lock().unwrap().sender.take();
        while !self.inner.finished.load(Ordering::Acquire) {
            let notified = self.inner.stopped.notified();
            if self.inner.finished.load(Ordering::Acquire) {
                break;
            }
            notified.await;
        }
        self.inner.projection.close();
        self.inner.subscription.lock().unwrap().take();
    }
}

fn draft_input(action: &WorkflowAction) -> Option<(String, String, String)> {
    match action {
        WorkflowAction::Tag(super::super::tag::state::TagAction::Draft {
            next,
            target,
            description,
        }) => Some((next.clone(), target.clone(), description.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod ui_tests;
