//! Typed request-generation authority for the PR Review session.

use std::sync::Arc;

use usher_state_store::{LocalStateStore, Reduction, StateStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReviewRequestKind {
    Diff,
    Highlight,
    Expand,
    Reveal,
    Conversation,
    Commits,
    RemoteActionState,
    RemoteMutation,
}

#[derive(Clone, Default)]
struct ReviewState {
    presentation_suspended: bool,
    diff_generation: u64,
    highlight_generation: u64,
    expand_generation: u64,
    reveal_generation: u64,
    conversation_generation: u64,
    commits_generation: u64,
    remote_action_state_generation: u64,
    remote_mutation_generation: u64,
}

impl ReviewState {
    fn generation(&self, kind: ReviewRequestKind) -> u64 {
        match kind {
            ReviewRequestKind::Diff => self.diff_generation,
            ReviewRequestKind::Highlight => self.highlight_generation,
            ReviewRequestKind::Expand => self.expand_generation,
            ReviewRequestKind::Reveal => self.reveal_generation,
            ReviewRequestKind::Conversation => self.conversation_generation,
            ReviewRequestKind::Commits => self.commits_generation,
            ReviewRequestKind::RemoteActionState => self.remote_action_state_generation,
            ReviewRequestKind::RemoteMutation => self.remote_mutation_generation,
        }
    }

    fn advance(&mut self, kind: ReviewRequestKind) -> u64 {
        let generation = match kind {
            ReviewRequestKind::Diff => &mut self.diff_generation,
            ReviewRequestKind::Highlight => &mut self.highlight_generation,
            ReviewRequestKind::Expand => &mut self.expand_generation,
            ReviewRequestKind::Reveal => &mut self.reveal_generation,
            ReviewRequestKind::Conversation => &mut self.conversation_generation,
            ReviewRequestKind::Commits => &mut self.commits_generation,
            ReviewRequestKind::RemoteActionState => &mut self.remote_action_state_generation,
            ReviewRequestKind::RemoteMutation => &mut self.remote_mutation_generation,
        };
        *generation = generation.saturating_add(1);
        *generation
    }
}

enum ReviewAction {
    Advance(ReviewRequestKind),
    InvalidateAll,
    SetPresentationSuspended(bool),
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
                        ReviewAction::SetPresentationSuspended(suspended) => {
                            if current.presentation_suspended == suspended {
                                return Reduction::unchanged(current.clone(), 0);
                            }
                            next.presentation_suspended = suspended;
                            if suspended {
                                for kind in [
                                    ReviewRequestKind::Highlight,
                                    ReviewRequestKind::Expand,
                                    ReviewRequestKind::Reveal,
                                ] {
                                    next.advance(kind);
                                }
                            }
                            0
                        }
                        ReviewAction::Advance(kind) => next.advance(kind),
                        ReviewAction::InvalidateAll => {
                            for kind in [
                                ReviewRequestKind::Diff,
                                ReviewRequestKind::Highlight,
                                ReviewRequestKind::Expand,
                                ReviewRequestKind::Reveal,
                                ReviewRequestKind::Conversation,
                                ReviewRequestKind::Commits,
                                ReviewRequestKind::RemoteActionState,
                                ReviewRequestKind::RemoteMutation,
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

    pub(super) fn set_presentation_suspended(&self, suspended: bool) {
        self.store
            .dispatch(ReviewAction::SetPresentationSuspended(suspended));
    }

    pub(super) fn presentation_suspended(&self) -> bool {
        self.store
            .snapshot()
            .select(|state| state.presentation_suspended)
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
    fn suspension_invalidates_derived_work_but_preserves_remote_mutations_and_reads() {
        let state = ReviewStateFacade::new();
        let kinds = [
            ReviewRequestKind::Diff,
            ReviewRequestKind::Highlight,
            ReviewRequestKind::Expand,
            ReviewRequestKind::Reveal,
            ReviewRequestKind::Conversation,
            ReviewRequestKind::Commits,
            ReviewRequestKind::RemoteActionState,
            ReviewRequestKind::RemoteMutation,
        ];
        let generations = kinds.map(|kind| state.begin(kind));
        state.set_presentation_suspended(true);
        assert!(state.presentation_suspended());
        for (kind, generation) in kinds.into_iter().zip(generations) {
            let derived = matches!(
                kind,
                ReviewRequestKind::Highlight
                    | ReviewRequestKind::Expand
                    | ReviewRequestKind::Reveal
            );
            assert_eq!(state.is_current(kind, generation), !derived);
        }
        state.set_presentation_suspended(false);
        assert!(!state.presentation_suspended());
        assert!(!state.is_current(ReviewRequestKind::Highlight, generations[1]));
    }

    #[test]
    fn request_generations_are_independent_and_invalidate_older_completions() {
        let state = ReviewStateFacade::new();
        let diff = state.begin(ReviewRequestKind::Diff);
        let conversation = state.begin(ReviewRequestKind::Conversation);
        let reveal = state.begin(ReviewRequestKind::Reveal);
        let action_state = state.begin(ReviewRequestKind::RemoteActionState);
        let mutation = state.begin(ReviewRequestKind::RemoteMutation);
        assert!(state.is_current(ReviewRequestKind::Diff, diff));
        assert!(state.is_current(ReviewRequestKind::Conversation, conversation));
        assert!(state.is_current(ReviewRequestKind::Reveal, reveal));
        assert!(state.is_current(ReviewRequestKind::RemoteActionState, action_state));
        assert!(state.is_current(ReviewRequestKind::RemoteMutation, mutation));

        let next_action_state = state.begin(ReviewRequestKind::RemoteActionState);
        assert!(!state.is_current(ReviewRequestKind::RemoteActionState, action_state));
        assert!(state.is_current(ReviewRequestKind::RemoteActionState, next_action_state));
        assert!(state.is_current(ReviewRequestKind::RemoteMutation, mutation));

        let next_diff = state.begin(ReviewRequestKind::Diff);
        assert!(!state.is_current(ReviewRequestKind::Diff, diff));
        assert!(state.is_current(ReviewRequestKind::Diff, next_diff));
        assert!(state.is_current(ReviewRequestKind::Conversation, conversation));

        let next_reveal = state.begin(ReviewRequestKind::Reveal);
        assert!(!state.is_current(ReviewRequestKind::Reveal, reveal));
        assert!(state.is_current(ReviewRequestKind::Reveal, next_reveal));

        state.invalidate_all();
        assert!(!state.is_current(ReviewRequestKind::Diff, next_diff));
        assert!(!state.is_current(ReviewRequestKind::Conversation, conversation));
        assert!(!state.is_current(ReviewRequestKind::Reveal, next_reveal));
        assert!(!state.is_current(ReviewRequestKind::RemoteActionState, next_action_state));
        assert!(!state.is_current(ReviewRequestKind::RemoteMutation, mutation));
    }
}
