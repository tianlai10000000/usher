//! A review declares interest before async reads; idle confirmation is deferred.
use super::highlight_session::HighlightLease;
use std::sync::{Arc, Mutex, OnceLock};

struct State<T> {
    current: HighlightLease<T>,
    intent: u64,
    leased: bool,
    idle_requested: bool,
    idle_confirmed: bool,
    check_queued: bool,
    releasing: bool,
}

/// One current builtin graph, including its mutable lazy grammar/regex caches.
/// A graph count is not a byte bound. Retired workers keep independent leases.
pub(super) struct HighlightResourceCache<T> {
    state: Mutex<State<T>>,
}

impl<T> Default for HighlightResourceCache<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                current: Arc::new(OnceLock::new()),
                intent: 0,
                leased: false,
                idle_requested: false,
                idle_confirmed: false,
                check_queued: false,
                releasing: false,
            }),
        }
    }
}

impl<T> HighlightResourceCache<T> {
    fn activate_locked(state: &mut State<T>) -> u64 {
        // Saturation conservatively disables error-driven release, never lets
        // an old error acquire authority over a new review's interest.
        state.intent = state.intent.saturating_add(1);
        state.idle_requested = false;
        state.idle_confirmed = false;
        state.intent
    }

    pub(super) fn activate(&self) -> u64 {
        Self::activate_locked(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()))
    }

    pub(super) fn lease(&self) -> HighlightLease<T> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        Self::activate_locked(&mut state);
        state.leased = true;
        state.current.clone()
    }

    pub(super) fn is_loaded(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .current
            .get()
            .is_some()
    }

    /// The caller queues one weak UI check only when this returns true.
    /// A failed read may release only its still-unclaimed review intent.
    pub(super) fn request_idle_check(&self, expected_intent: Option<u64>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(expected) = expected_intent
            && (expected == u64::MAX || state.intent != expected)
        {
            return false;
        }
        state.idle_requested = true;
        state.idle_confirmed = false;
        !std::mem::replace(&mut state.check_queued, true)
    }

    pub(super) fn check_failed(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.check_queued = false;
        state.idle_confirmed = false;
    }

    fn take_slot(state: &mut State<T>) -> Option<HighlightLease<T>> {
        // Even an uninitialized leased slot must be retired: an admitted old
        // factory can still warm it after cancellation.
        if !state.leased {
            return None;
        }
        state.leased = false;
        Some(std::mem::replace(
            &mut state.current,
            Arc::new(OnceLock::new()),
        ))
    }

    /// Read the latest intent, not the epoch of the first coalesced reset.
    pub(super) fn confirm_idle(&self) -> Option<HighlightLease<T>> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !std::mem::take(&mut state.check_queued) {
            return None;
        }
        state.idle_confirmed = state.idle_requested;
        if !state.idle_confirmed || state.releasing {
            return None;
        }
        let slot = Self::take_slot(&mut state)?;
        state.releasing = true;
        Some(slot)
    }

    /// Only the existing release worker calls this after dropping its slot.
    pub(super) fn next_release(&self) -> Option<HighlightLease<T>> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.idle_confirmed
            && let Some(slot) = Self::take_slot(&mut state)
        {
            return Some(slot);
        }
        state.releasing = false;
        None
    }
}

#[cfg(test)]
#[path = "highlight_cache_tests.rs"]
mod tests;
