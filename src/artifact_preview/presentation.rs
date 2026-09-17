//! Shared preview placement preference with ordered actions and worker persistence.

use std::sync::{Arc, Mutex};

use usher_state_store::{PersistentStore, Reduction, StoreListener, Subscription};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PresentationMode {
    Docked,
    Detached,
    Floating,
}

impl PresentationMode {
    pub(crate) fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Detached,
            2 => Self::Floating,
            _ => Self::Docked,
        }
    }

    pub(crate) fn from_setting(value: &str) -> Self {
        match value {
            "detached" => Self::Detached,
            "floating" => Self::Floating,
            _ => Self::Docked,
        }
    }

    pub(crate) fn index(self) -> i32 {
        match self {
            Self::Docked => 0,
            Self::Detached => 1,
            Self::Floating => 2,
        }
    }

    pub(crate) fn setting_value(self) -> &'static str {
        match self {
            Self::Docked => "docked",
            Self::Detached => "detached",
            Self::Floating => "floating",
        }
    }
}

enum PresentationAction {
    Select(PresentationMode),
}

fn reduce(
    current: &PresentationMode,
    action: PresentationAction,
) -> Reduction<PresentationMode, ()> {
    let PresentationAction::Select(next) = action;
    if next == *current {
        Reduction::unchanged(next, ())
    } else {
        Reduction::changed(next, ())
    }
}

#[derive(Clone)]
pub(super) struct PresentationFacade {
    store: PersistentStore<PresentationMode, PresentationAction, ()>,
    commands: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<PresentationAction>>>>,
}

impl PresentationFacade {
    pub(super) fn open(initial: PresentationMode) -> std::io::Result<Self> {
        let store = PersistentStore::new(
            initial,
            reduce,
            "preview-presentation",
            |mode| crate::settings::save_preview_presentation_mode(mode.setting_value()),
            |_, error| log::error!("[ArtifactPreview] Placement persistence failed: {error}"),
        )?;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let commands = Arc::new(Mutex::new(Some(sender)));
        let worker_store = store.clone();
        let worker = crate::app_runtime::shared_runtime().spawn(async move {
            while let Some(action) = receiver.recv().await {
                worker_store.dispatch(action);
            }
        });
        let shutdown_commands = commands.clone();
        let shutdown_store = store.clone();
        crate::shutdown::register_async_drain("preview_presentation", move || {
            Box::pin(async move {
                shutdown_commands.lock().unwrap().take();
                let _ = worker.await;
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(error) = shutdown_store.flush() {
                        log::error!("[ArtifactPreview] Placement flush failed: {error}");
                    }
                    shutdown_store.shutdown();
                })
                .await;
            })
        });
        Ok(Self { store, commands })
    }

    pub(super) fn select(&self, mode: PresentationMode) {
        if let Some(sender) = self.commands.lock().unwrap().as_ref() {
            let _ = sender.send(PresentationAction::Select(mode));
        }
    }

    pub(super) fn subscribe(&self, listener: StoreListener<PresentationMode>) -> Subscription {
        self.store.subscribe(listener)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_values_round_trip_and_unknown_values_keep_split_layout() {
        for mode in [
            PresentationMode::Docked,
            PresentationMode::Detached,
            PresentationMode::Floating,
        ] {
            assert_eq!(PresentationMode::from_setting(mode.setting_value()), mode);
            assert_eq!(PresentationMode::from_index(mode.index()), mode);
        }
        assert_eq!(
            PresentationMode::from_setting("unknown"),
            PresentationMode::Docked
        );
        assert_eq!(PresentationMode::from_index(9), PresentationMode::Docked);
    }

    #[test]
    fn placement_commits_notify_and_persist_only_changed_preferences() {
        let persisted = Arc::new(Mutex::new(Vec::new()));
        let writes = persisted.clone();
        let store = PersistentStore::new(
            PresentationMode::Docked,
            reduce,
            "preview-placement-test",
            move |mode| {
                writes.lock().unwrap().push(*mode);
                Ok(())
            },
            |_, error| panic!("{error}"),
        )
        .unwrap();
        let revisions = Arc::new(Mutex::new(Vec::new()));
        let observed = revisions.clone();
        let _subscription = store.subscribe(Arc::new(move |snapshot| {
            observed
                .lock()
                .unwrap()
                .push((snapshot.revision(), *snapshot.state()));
        }));
        for mode in [
            PresentationMode::Floating,
            PresentationMode::Detached,
            PresentationMode::Docked,
        ] {
            store.dispatch(PresentationAction::Select(mode));
            store.dispatch(PresentationAction::Select(mode));
            store.flush().unwrap();
        }
        store.shutdown();
        assert_eq!(
            *persisted.lock().unwrap(),
            [
                PresentationMode::Floating,
                PresentationMode::Detached,
                PresentationMode::Docked
            ]
        );
        assert_eq!(revisions.lock().unwrap().len(), 4);
    }
}
