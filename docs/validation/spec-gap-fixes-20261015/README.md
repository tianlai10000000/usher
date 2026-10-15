# Spec gap fixes / 已确认实现缺口修复（2026-10-15）

本记录保留第一批交付时状态；下方当时剩余的 hydration、最终关闭、Diagnostics、
Status bar、Workflow/Tag 项的后续修复见
[第二批记录](../spec-gap-followups-20261015/README.md)。

本批接续 [spec 源码核对](../spec-audit-20261014/README.md)，修复两个优先缺口：
通用 Preview File 读取预算，以及 Inbox/Banner/AI Agent Save 在 UI 线程同步执行
reducer。保留上一批文档整理和用户已暂存的三项 `.DS_Store` 删除；没有提交 Git。

This batch fixes generic Preview file growth and UI-originated synchronous
reduction. It is a correctness/resource-boundary change, with no runtime speed,
RSS or footprint improvement claim. The before/after comparisons below test
behavior and gate coverage, not a controlled Release performance A/B.

## Implementation and inspected paths / 实现与核对路径

| Boundary / 边界 | Change and regression / 修复与回归 |
| --- | --- |
| Preview file source | `crates/usher-preview/src/lib.rs`, `file_source.rs` and `file_source/tests.rs`: one regular-file handle, bounded metadata hint, 8 KiB chunks, payload/requested Vec capacity ≤16 MiB, one stack overflow-probe byte. Growth after metadata, exact budget, ordinary content, path replacement, regular symlink, Unix FIFO, interruption and cancellation controls. |
| AI Agent Save | `src/ai/settings_state.rs`, `settings_state_tests.rs`: complete five-field FIFO capture, eight active-inclusive saves, adjacent deduplication preserving A→B→A, worker reduction and checked per-request flush receipt, explicit same-value retry without a new business revision, checked close/drain. |
| AI completion | `src/ai/settings_ui.rs`, `settings_ui_tests.rs`, `src/settings_page.rs`: independent eight-owner budget until UI completion/drop, weak event-loop delivery, host request order, factory/route/raw-draft guards and latest-attempt localized feedback. Tests cover en/zh-CN, failure/recovery, inverse completion, newer edits/visit and failed UI delivery. Root callback arguments are no longer ignored. |
| Messages | `src/message_center.rs`, `src/message_center/commands.rs`, `commands_tests.rs`, `paging_tests.rs`: one ordered worker, 256 business commands / 1 MiB owned-capacity accounting including active commands; one reserved hydration slot; no accepted action coalescing. Pre-first-frame commands stay bounded and run after hydration; shutdown before first frame follows the same order. Real compiled Inbox callbacks return while a real Store observer blocks the worker. |
| Notification ingress | `src/notification.rs`, `src/notification/ingress.rs`, `ingress_tests.rs`: independent 256-entry / 1 MiB UI payload accounting, owned action snapshot, admission-time focus decision, unique completion tickets/latest Banner ID, live-row check after delete/clear and bounded overload fallback. Tests use a real notifier adapter and compiled production notification callbacks. |
| Lifecycle and ownership | `src/lib.rs` registers worker drains before later producers' async drains; message command drain flushes without closing background publish admission. Final center close is registered early at the existing HIGH pre-shutdown priority, so later same-priority publishers stop first under LIFO. New cohesive owners have size/dependency gates; original budgets are unchanged. |

Rust callback bindings and their generated Slint declarations were inspected in
`ui/state/app_state.slint`, `ui/views/settings.slint` and the independently
compiled Messages page. No Slint layout or catalog text was changed. Specs and
both architecture language versions describe the resulting contracts.

## Before/after controls / 修复前后对照

- Preview negative control temporarily replaces only the new reader helper with
  the former unbounded `read_to_end` algorithm; the exact patch is in
  [preview-legacy-algorithm.patch](preview-legacy-algorithm.patch). The growth
  test fails because it consumes 67,108,864 bytes instead of at most 16,777,217;
  ordinary-file and exact-limit controls still pass. See
  [legacy regression](preview-legacy-regression.log). This isolates the old
  algorithm; it is not an old full-binary benchmark. The candidate was restored
  before subsequent checks; all 12 source-reader cases pass.
- UI baseline source paths directly invoked `replace/dispatch` or message
  mutation methods on the callback thread. Candidate regressions use actual
  reducers/subscribers and captured thread IDs, block accepted work, then assert
  nonblocking admission, FIFO identity and drain behavior. Completion tests
  additionally withhold UI consumption to prove worker completion cannot escape
  UI-owner budgets. These are correctness assertions, not frame-latency results.
- Architecture A/B uses `USHER_TEST_ARCHITECTURE_BASE_REF=HEAD` to load the old
  gate against the new owner fixtures. It produces ten expected failures: the
  five new size and five dependency violations are accepted by the old gate.
  The candidate rejects them while legitimate fixtures pass. See
  [baseline negative fixtures](architecture-baseline-negative.log) and
  [candidate fixtures](architecture-ui-fixtures-final.log). Adopt candidate;
  no existing budget was raised.

## Executed checks / 已执行检查

The final check results are recorded here with raw logs. Initial failures are
retained separately and are not counted as passing validation.

| Check | Result / 结果 |
| --- | --- |
| `cargo fmt --all` | Exit 0; [log](cargo-fmt.log) (empty output). |
| `cargo fmt --all --check` | Exit 0 on the final source/test tree; [log](cargo-fmt-check-final.log). |
| `cargo check --locked` | Exit 0; [final log](cargo-check-final.log). |
| `cargo test -p usher-preview --locked` | 39 passed, 2 existing ignored; 0 doctests; [log](preview-package.log). |
| `cargo test -p usher-preview --locked file_source::` | 12 passed; [log](preview-source-tests.log). |
| `cargo check -p usher-preview --no-default-features --locked` | Exit 0; [log](preview-no-default.log). |
| `cargo test -p usher-messages --locked` | 27 passed; 0 doctests; [log](messages-package.log). |
| `cargo test --lib --locked` | Final tree: **847 passed, 0 failed, 2 existing ignored**; [log](cargo-test-lib-final-tree.log). Previous successful run before the last first-frame fixture: 846 passed, 2 ignored; [log](cargo-test-lib-final.log). Both allow only the local listeners required by HTTP test fixtures. |
| `cargo test --doc --locked` | 1 passed, 1 existing ignored; [log](cargo-test-doc.log). |
| `cargo test --lib --locked message_center::` | Initial candidate: 12 passed; [log](messages-bridge-tests.log). Final supplemental result below covers the last first-frame control fixture. |
| `cargo test --lib --locked notification::` | 7 passed; [log](notification-tests.log). Also covered by the final root library run. |
| `cargo test --lib --locked ai::settings_ui::` | 2 passed with final permit assertions; [log](ai-save-ui-final.log). |
| `cargo test --lib --locked deferred_first_frame_hydration` | 1 passed; real first-frame clear/mark-all cases; [log](messages-first-frame-final.log). |
| `./scripts/check-architecture-structure.sh` | Exit 0; [log](architecture-gate.log). |
| `./scripts/check-ui-policy.sh` | Exit 0; [log](ui-policy.log). |
| Python architecture + UI-policy fixtures | 75 passed; [log](architecture-ui-fixtures-final.log). |
| Navigation / source references | 32 documents, 343 local links/anchors, 1,059 source references, all 25 specs indexed; [log](navigation-check.txt). Rechecked after the final evidence links were added. |
| `git diff --check` | Exit 0; [log](diff-check.log). |

Initial issues corrected during iteration:

- The Unix FIFO fixture initially used a rustix API unavailable on macOS; it
  now uses dev-only `libc::mkfifo`. [Initial compile output](preview-source-tests-initial.log).
- New single-file lexical gate diagnostics initially omitted filenames; `rg -H`
  restores actionable diagnostics and the negative fixtures pass.
  [Initial fixture failures](architecture-ui-fixtures.log).
- The first full library run had 831 passed, 15 failed, 2 ignored. Fourteen
  failures came from sandbox-denied local TCP listeners; the new AI feedback
  test also selected a bundled locale before constructing a component. That
  initialization order was fixed, then the complete library suite was rerun
  with local listeners allowed. [Initial log](cargo-test-lib.log).

## Limits and remaining work / 验证边界与未修项

- macOS automated tests use compiled Slint components/headless callback checks;
  they do not certify native pointer/focus, visual paint, operating-system
  delivery or Windows/Linux acceptance. No new layout or Tooltip was introduced.
  The headless backend cannot run queued event-loop callbacks; completion tests
  invoke the same production completion functions explicitly where needed.
- The Preview bound covers source payload/requested Vec capacity, not allocator
  slack, decoder memory or total process RSS. Cancellation cannot interrupt an
  in-flight OS read. Generic symlinks remain legitimate; repository Markdown
  containment/nofollow behavior is separate and retained. Windows device/pipe
  opening is untested.
- A matching Save repeated during a later Settings visit can deduplicate to the
  existing operation; its old visit context prevents a Saved badge on the new
  visit. Persistence still completes; newer edits remain protected.
- Message hydration still redundantly rewrites disk-only/revision-zero state.
  Final MessageCenter close remains a synchronous pre-shutdown barrier; this
  batch moves UI command reduction/drain off-thread, not every lifecycle barrier.
  Native pagination focus, diagnostic archive pruning, open status popup refresh
  and the previously recorded Workflow/Tag gaps remain separate work.
- No production workstation settings or credentials were used in tests. No
  controlled Release A/B was run; no runtime performance adoption is claimed.
