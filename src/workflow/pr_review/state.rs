//! Typed request-generation authority for the PR Review session.

use std::sync::Arc;

use usher_state_store::{LocalStateStore, Reduction, StateStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReviewRequestKind {
    Diff,
    Highlight,
    Expand,
    Conversation,
    Commits,
    RemoteAction,
}

#[derive(Clone, Default)]
struct ReviewState {
    diff_generation: u64,
    highlight_generation: u64,
    expand_generation: u64,
    conversation_generation: u64,
    commits_generation: u64,
    remote_action_generation: u64,
}

impl ReviewState {
    fn generation(&self, kind: ReviewRequestKind) -> u64 {
        match kind {
            ReviewRequestKind::Diff => self.diff_generation,
            ReviewRequestKind::Highlight => self.highlight_generation,
            ReviewRequestKind::Expand => self.expand_generation,
            ReviewRequestKind::Conversation => self.conversation_generation,
            ReviewRequestKind::Commits => self.commits_generation,
            ReviewRequestKind::RemoteAction => self.remote_action_generation,
        }
    }

    fn advance(&mut self, kind: ReviewRequestKind) -> u64 {
        let generation = match kind {
            ReviewRequestKind::Diff => &mut self.diff_generation,
            ReviewRequestKind::Highlight => &mut self.highlight_generation,
            ReviewRequestKind::Expand => &mut self.expand_generation,
            ReviewRequestKind::Conversation => &mut self.conversation_generation,
            ReviewRequestKind::Commits => &mut self.commits_generation,
            ReviewRequestKind::RemoteAction => &mut self.remote_action_generation,
        };
        *generation = generation.saturating_add(1);
        *generation
    }
}

enum ReviewAction {
    Advance(ReviewRequestKind),
    InvalidateAll,
}

type ReviewStore = dyn StateStore<ReviewState, ReviewAction, u64>;

#[derive(Clone)]
pub(super) struct ReviewStateFacade {
    store: Arc<ReviewStore>,
}

impl ReviewStateFacade {
    pub(super) fn new() -> Self {
        Self {
            store: Arc::new(LocalStateStore::new(
                ReviewState::default(),
                |current: &ReviewState, action: ReviewAction| {
                    let mut next = current.clone();
                    let output = match action {
                        ReviewAction::Advance(kind) => next.advance(kind),
                        ReviewAction::InvalidateAll => {
                            for kind in [
                                ReviewRequestKind::Diff,
                                ReviewRequestKind::Highlight,
                                ReviewRequestKind::Expand,
                                ReviewRequestKind::Conversation,
                                ReviewRequestKind::Commits,
                                ReviewRequestKind::RemoteAction,
                            ] {
                                next.advance(kind);
                            }
                            0
                        }
                    };
                    Reduction::changed(next, output)
                },
            )),
        }
    }

    pub(super) fn begin(&self, kind: ReviewRequestKind) -> u64 {
        self.store.dispatch(ReviewAction::Advance(kind)).output
    }

    pub(super) fn invalidate(&self, kind: ReviewRequestKind) {
        self.store.dispatch(ReviewAction::Advance(kind));
    }

    pub(super) fn invalidate_all(&self) {
        self.store.dispatch(ReviewAction::InvalidateAll);
    }

    pub(super) fn is_current(&self, kind: ReviewRequestKind, generation: u64) -> bool {
        self.store
            .snapshot()
            .select(|state| state.generation(kind) == generation)
    }
}

#[cfg(test)]
mod tests {
    use super::{ReviewRequestKind, ReviewStateFacade};

    #[test]
    fn request_generations_are_independent_and_invalidate_older_completions() {
        let state = ReviewStateFacade::new();
        let diff = state.begin(ReviewRequestKind::Diff);
        let conversation = state.begin(ReviewRequestKind::Conversation);
        assert!(state.is_current(ReviewRequestKind::Diff, diff));
        assert!(state.is_current(ReviewRequestKind::Conversation, conversation));

        let next_diff = state.begin(ReviewRequestKind::Diff);
        assert!(!state.is_current(ReviewRequestKind::Diff, diff));
        assert!(state.is_current(ReviewRequestKind::Diff, next_diff));
        assert!(state.is_current(ReviewRequestKind::Conversation, conversation));

        state.invalidate_all();
        assert!(!state.is_current(ReviewRequestKind::Diff, next_diff));
        assert!(!state.is_current(ReviewRequestKind::Conversation, conversation));
    }
}
