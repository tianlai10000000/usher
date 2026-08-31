//! Typed request generations for PR list pages and state totals.

use std::sync::Arc;

use usher_state_store::{LocalStateStore, Reduction, StateStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PrListRequestKind {
    Page,
    Totals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PrListGeneration(u64);

#[derive(Clone, Default)]
struct PrListRequestState {
    page_generation: u64,
    totals_generation: u64,
}

impl PrListRequestState {
    fn generation(&self, kind: PrListRequestKind) -> u64 {
        match kind {
            PrListRequestKind::Page => self.page_generation,
            PrListRequestKind::Totals => self.totals_generation,
        }
    }

    fn advance(&mut self, kind: PrListRequestKind) -> PrListGeneration {
        let generation = match kind {
            PrListRequestKind::Page => &mut self.page_generation,
            PrListRequestKind::Totals => &mut self.totals_generation,
        };
        *generation = generation.saturating_add(1);
        PrListGeneration(*generation)
    }
}

enum PrListRequestAction {
    Advance(PrListRequestKind),
    InvalidateAll,
}

type PrListRequestStore = dyn StateStore<PrListRequestState, PrListRequestAction, PrListGeneration>;

#[derive(Clone)]
pub(super) struct PrListGenerationFacade {
    store: Arc<PrListRequestStore>,
}

impl PrListGenerationFacade {
    pub(super) fn new() -> Self {
        Self {
            store: Arc::new(LocalStateStore::new(
                PrListRequestState::default(),
                |current: &PrListRequestState, action: PrListRequestAction| {
                    let mut next = current.clone();
                    let generation = match action {
                        PrListRequestAction::Advance(kind) => next.advance(kind),
                        PrListRequestAction::InvalidateAll => {
                            next.advance(PrListRequestKind::Page);
                            next.advance(PrListRequestKind::Totals)
                        }
                    };
                    Reduction::changed(next, generation)
                },
            )),
        }
    }

    pub(super) fn begin(&self, kind: PrListRequestKind) -> PrListGeneration {
        self.store
            .dispatch(PrListRequestAction::Advance(kind))
            .output
    }

    pub(super) fn invalidate(&self, kind: PrListRequestKind) {
        self.store.dispatch(PrListRequestAction::Advance(kind));
    }

    pub(super) fn invalidate_all(&self) {
        self.store.dispatch(PrListRequestAction::InvalidateAll);
    }

    pub(super) fn is_current(&self, kind: PrListRequestKind, generation: PrListGeneration) -> bool {
        self.store
            .snapshot()
            .select(|state| state.generation(kind) == generation.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{PrListGenerationFacade, PrListRequestKind};

    #[test]
    fn page_and_total_generations_are_independent() {
        let generations = PrListGenerationFacade::new();
        let page = generations.begin(PrListRequestKind::Page);
        let totals = generations.begin(PrListRequestKind::Totals);
        assert!(generations.is_current(PrListRequestKind::Page, page));
        assert!(generations.is_current(PrListRequestKind::Totals, totals));

        let next_page = generations.begin(PrListRequestKind::Page);
        assert!(!generations.is_current(PrListRequestKind::Page, page));
        assert!(generations.is_current(PrListRequestKind::Page, next_page));
        assert!(generations.is_current(PrListRequestKind::Totals, totals));

        generations.invalidate_all();
        assert!(!generations.is_current(PrListRequestKind::Page, next_page));
        assert!(!generations.is_current(PrListRequestKind::Totals, totals));
    }
}
