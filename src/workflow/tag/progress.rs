//! Replaceable progress ownership with a terminal drain barrier.
use super::execution::TagProgressReporter;
use super::state::TagAction;
use crate::i18n::LocalizedText;
use crate::jobs::JobStepData;
use crate::workflow::state::{SharedWorkflowState, WorkflowAction, WorkflowIdentity};
use std::sync::{Arc, Mutex};
struct Mailbox {
    latest: Option<(Vec<JobStepData>, LocalizedText)>,
    closed: bool,
}
pub(super) struct Progress {
    mailbox: Arc<Mutex<Mailbox>>,
    wake: Arc<tokio::sync::Notify>,
    drained: tokio::task::JoinHandle<()>,
}
impl Progress {
    pub(super) fn new(facade: SharedWorkflowState, identity: WorkflowIdentity, run: u64) -> Self {
        let mailbox = Arc::new(Mutex::new(Mailbox {
            latest: None,
            closed: false,
        }));
        let wake = Arc::new(tokio::sync::Notify::new());
        let pending = mailbox.clone();
        let notification = wake.clone();
        let drained = crate::app_runtime::shared_runtime().spawn(async move {
            loop {
                let notified = notification.notified();
                let (latest, closed) = {
                    let mut pending = pending.lock().unwrap();
                    (pending.latest.take(), pending.closed)
                };
                if let Some((steps, summary)) = latest {
                    if !facade
                        .publish(
                            identity.clone(),
                            WorkflowAction::Tag(TagAction::Progress {
                                run,
                                steps,
                                summary,
                            }),
                        )
                        .await
                    {
                        break;
                    }
                } else if closed {
                    break;
                } else {
                    notified.await;
                }
            }
        });
        Self {
            mailbox,
            wake,
            drained,
        }
    }
    pub(super) fn reporter(&self) -> TagProgressReporter {
        let mailbox = self.mailbox.clone();
        let wake = self.wake.clone();
        Arc::new(move |steps, summary| {
            let mut pending = mailbox.lock().unwrap();
            if !pending.closed {
                pending.latest = Some((steps, summary));
                wake.notify_one();
            }
        })
    }
    pub(super) async fn finish(self) {
        self.mailbox.lock().unwrap().closed = true;
        self.wake.notify_one();
        let _ = self.drained.await;
    }
}
