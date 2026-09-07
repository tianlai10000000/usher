//! Coalesce state snapshots before enqueueing a Slint event-loop delivery.
use crate::MainWindow;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use usher_state_store::LatestDelivery;

/// For replaceable view state only. Ordered events, deltas and user actions
/// must keep their individual delivery semantics. `apply` runs on the UI loop.
pub(crate) fn latest_ui<T: Send + 'static>(
    window: slint::Weak<MainWindow>,
    apply: impl Fn(&MainWindow, T) + Send + Sync + 'static,
) -> impl Fn(T) + Send + Sync {
    let pending = Arc::new(LatestDelivery::default());
    let revision = AtomicU64::new(0);
    let apply = Arc::new(apply);
    move |value| {
        if !pending.offer(revision.fetch_add(1, Ordering::AcqRel), value) {
            return;
        }
        let latest = pending.clone();
        let weak = window.clone();
        let apply = apply.clone();
        if slint::invoke_from_event_loop(move || {
            let Some(value) = latest.take() else {
                return;
            };
            if let Some(ui) = weak.upgrade() {
                apply(&ui, value);
            } else {
                latest.close();
            }
        })
        .is_err()
        {
            pending.close();
        }
    }
}
