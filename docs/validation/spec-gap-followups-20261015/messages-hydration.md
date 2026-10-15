# Messages hydration correctness evidence

This change removes redundant startup writes without changing the JSON schema or
moving domain reduction back onto the UI thread. The application command-worker
budget, first-frame control, and producer/final-close ownership remain separate.

## Code boundary

- `MessageCenter::hydrate_from_storage` is one successful initialization. A short
  mutex/condition variable shares that result across concurrent callers; a guard
  clears in-progress ownership on error/unwind. Same-thread subscriber reentry is
  explicitly rejected instead of waiting for itself. An I/O failure permits retry.
- The typed hydration merge runs before persistence registration. Registration is
  followed by a current snapshot read and a comparison with the normalized disk
  baseline. Equal disk-only or empty revision-zero state is already durable and
  is not offered to the writer. A different state is offered once; subsequent
  business commits use the automatic observer. An equal/older explicit offer can
  race an automatic one and is treated as covered rather than a missing commit.
- Invalid JSON/schema keeps the existing empty-history recovery behavior. Without
  a changed business state, hydration preserves the original corrupt bytes. A
  subsequent valid changed state atomically replaces them. Other read errors leave
  accepted memory state intact and durability unavailable until hydration succeeds.
- Successful repeated hydration does not reload stale disk state while a delete's
  writer is blocked, so it cannot resurrect a removed record.

## Regression assertions

`crates/usher-messages/src/hydration_tests.rs` exercises the production facade,
real Store, observer registration, and real atomic JSON adapter with a write
counter or explicit gates:

| Regression | Required outcome |
| --- | --- |
| Disk-only compact JSON load | Zero writer calls; original bytes remain identical. A later read-state commit writes and reloads correctly. |
| Empty revision-zero missing store | No writer offer or file creation on hydration, flush, or checked shutdown. |
| Early publish/read/remove/clear | Final merged state persists only when different from disk; deleting all early additions leaves an already-durable baseline without a write. |
| Subscriber read/delete/clear before observer registration | The post-registration comparison captures the final changes, including an empty result. |
| Concurrent mutations while hydration subscriber is blocked | No write before registration; final accepted read/delete/new publication is durable afterward. |
| Repeated hydration while deletion write is blocked | No new revision or resurrected disk row; one final deletion write. |
| Corrupt history and non-JSON I/O failure | Corrupt bytes survive unchanged until a valid mutation; accepted state survives a failed read and an explicit retry. |
| Concurrent hydration, same-thread reentry, checked shutdown | One successful initialization shared by both callers; reentry rejected; final shutdown waits for admitted operations and performs no disk-only write. |

The prior implementation unconditionally offered the merged snapshot, including
revision zero; the original `deferred_empty_hydration_persists_revision_zero`
regression explicitly expected file creation. The revised no-file and write-counter
assertions are negative cases for that behavior. No runtime performance gain is
claimed; the comparison concerns write/no-write and preserved mutation semantics.

## Checks executed by the implementation agent

- `rustfmt --edition 2024` for the three changed message-package Rust files: passed.
- `git diff --check -- crates/usher-messages/src/center.rs crates/usher-messages/src/center_tests.rs crates/usher-messages/src/hydration_tests.rs docs/specs/messages.md docs/specs/state-store.md`: passed.
- Cargo commands were intentionally left to the root agent's serialized validation
  queue. The root follow-up record owns actual package/full-regression outcomes;
  the assertions above are not a claim that those tests have already run.

The completed root regression is recorded in [batch results](README.md): the
message package passed all 34 tests, and the old algorithm failed the three
no-redundant-write assertions while its two legitimate mutation controls passed.

Native desktop notification appearance and OS permission behavior are unaffected
and were not manually retested here. No workstation settings are used by fixtures.
