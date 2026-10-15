# AI Review producer shutdown boundary

`src/ai_review/producers.rs` now owns admission and the lifetime of complete
controller operations. Synchronous registration precedes future spawning or
Session user admission. Closing rejects new Review, Ask, open/read and task-cancel
operations. Already accepted Review/Ask futures remain owned through their real
Session receipts, terminal Task Actions, Message Center publication and awaited
Hook dispatch. The finalizer runs on a blocking worker only after all producer
permits have returned; concurrent or cancelled shutdown waiters share that one
owned finalizer and result.

`AiReviewController::shutdown_async` closes admission, cancels running requests,
retires queued requests without starting them, and waits for the final barrier
before stopping task/config persistence. Its registered pre-shutdown compatibility
method only observes the completed result and performs no reducer work or joins
on the UI thread. Root composition registers this async drain before Hook closure
and final Message Center closure. The root record owns wiring validation.

Startup recovery reserves a producer permit when the controller is constructed.
First-frame activation and shutdown take the same recovery obligation exactly
once. Shutdown before the first frame starts recovery itself and waits for the
actual blocking read, typed hydration and persistence offer. It cannot drop old
interrupted tasks or wait forever for an unrendered first-frame callback.

The barrier does not consult historical `SessionTransactions::drain` failure
counts. A failed Ask followed by a successful explicit retry does not poison
shutdown with the earlier error. Session errors retain their existing per-request
UI/log behavior; this change does not alter the Session transaction protocol.

## Regression assertions

- `controller_producer_tests::shutdown_waits_for_review_terminal_continuation_and_cancels_queued_start` blocks the production Complete subscriber after the real Review Session write but before terminal Message Center/Hook continuation, proves close still waits, cancels queued work, rejects another start, then checks the final task JSON and message.
- `controller_producer_tests::accepted_ask_waits_its_blocking_user_receipt_even_after_shutdown_waiter_is_cancelled` invokes the compiled Ask callback, blocks the real atomic Session writer, cancels one shutdown waiter, and proves another waiter still owns the barrier and the accepted user row persists exactly once.
- `controller_producer_tests::shutdown_before_first_frame_completes_reserved_task_recovery_once` restores an on-disk running task as interrupted/failed during shutdown and checks a later first-frame activation cannot mutate it again.
- `controller_producer_tests::successful_explicit_same_text_ask_after_io_failure_does_not_poison_shutdown` injects a real invalid Session parent path, retries the same text after repairing it, and checks successful persistence/close despite the test-only historical failure diagnostic.
- `producers::tests::before_first_poll_permit_and_cancelled_waiter_keep_one_final_barrier` covers registration before polling, rejection after close and once-only finalization.

All fixtures use temporary directories and an isolated Config facade, never
workstation settings or credentials. The controller-local fixture flag reuses the
existing fixture transport path without mutating process-wide environment values.

## Checks run by the implementation agent

`rustfmt --edition 2024` and `git diff --check -- src/ai_review` passed. Cargo
execution remains serialized by the root agent; the regression assertions above
are not a claim that compilation or tests have already passed. No native UI or
external Provider acceptance was performed here, and no performance gain is claimed.

The root's completed [batch regression](README.md) includes all four controller
producer fixtures and the before-first-poll/cancelled-waiter finalizer test;
each passed in the complete library run.
