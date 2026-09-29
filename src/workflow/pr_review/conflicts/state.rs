use crate::git::conflicts::{ConflictError, ConflictSnapshot};
use std::sync::Arc;
use usher_state_store::{LocalStateStore, Reduction, StateStore};

#[derive(Clone, Default)]
pub(super) struct Session {
    pub(super) generation: u64,
    pub(super) selection: u64,
    pub(super) snapshot: Option<Arc<ConflictSnapshot>>,
    pub(super) error: Option<ConflictError>,
}
pub(super) enum Action {
    Reset(u64),
    Loaded(u64, Result<Arc<ConflictSnapshot>, ConflictError>),
    Select(u64, u64),
}
#[derive(Clone)]
pub(super) struct SessionFacade(Arc<dyn StateStore<Session, Action, u64>>);
impl Default for SessionFacade {
    fn default() -> Self {
        Self(Arc::new(LocalStateStore::new(
            Session::default(),
            |s: &Session, a| {
                let mut next = s.clone();
                let result = match a {
                    Action::Reset(g) => {
                        if g <= s.generation {
                            return Reduction::unchanged(next, s.generation);
                        }
                        next.generation = g;
                        next.selection = 0;
                        next.snapshot = None;
                        next.error = None;
                        next.generation
                    }
                    Action::Loaded(g, result) => {
                        if g != s.generation {
                            return Reduction::unchanged(next, 0);
                        }
                        match result {
                            Ok(snapshot) => next.snapshot = Some(snapshot),
                            Err(error) => next.error = Some(error),
                        }
                        next.generation
                    }
                    Action::Select(g, selection) => {
                        if g != s.generation || selection <= s.selection {
                            return Reduction::unchanged(next, s.selection);
                        }
                        next.selection = selection;
                        selection
                    }
                };
                Reduction::changed(next, result)
            },
        )))
    }
}
impl SessionFacade {
    pub(super) fn dispatch(&self, a: Action) -> u64 {
        self.0.dispatch(a).output
    }
    pub(super) fn snapshot(&self) -> Session {
        self.0.snapshot().state().clone()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_failure_cannot_replace_new_session() {
        let s = SessionFacade::default();
        let old = s.dispatch(Action::Reset(1));
        let new = s.dispatch(Action::Reset(2));
        s.dispatch(Action::Loaded(old, Err(ConflictError::Unavailable)));
        assert_eq!(s.snapshot().generation, new);
        assert!(s.snapshot().error.is_none());
    }
}
