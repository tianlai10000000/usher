# P7 #79 Conflict enrichment owner — adopted

Status: implementation applied; locked check, nine fixture-tool tests, eleven
owner/cache tests, the real Settings SaveErr test and full root lib
(730 passed, zero failed, two ignored) passed. Controlled Release/native
acceptance passed; adopted. Raw evidence is in
`memory-benchmark-results/p7-79-20261003/`, with `decision.json` and immutable
A/B source/instrumentation/build records.
The baseline is the actually adopted #78 source; failed #77 shutdown
owners, deadline/error policy and unrelated P2 batches are excluded.

The candidate changes eight Rust files and the two architecture languages,
`pr-conflicts.md`, its index and this bounded acceptance record. It limits only
list enrichment: one lifetime driver, latest pending round and four shared
logical-job permits spanning provider status reads and Checking backoff. It
preserves existing metadata/totals generations, workspace cancellation, 1 MiB
status caps, HTTP retry policy, 30-second/64-entry cache, platform status meaning,
Silent feedback, existing Slint layout and semantic catalog.

Acceptance gates, fixed before execution:

1. Format, locked check, relevant owner/cache/settings tests, full root lib,
   diff-check, HTTP-client inventory and existing structural gates. Baseline
   failures remain reported separately. Twelve drafted unit tests exercise actual
   Tokio driver/permitted futures and typed cache dispatch; they are not a
   substitute for actual provider HTTP or native UI.
2. Serial same-profile ordinary Release A/B, default features plus
   `test-fixtures`, two runtime workers, same shared fixture, deterministic dummy
   credentials and isolated settings/project corpus. AB/BA/AB per scene. Preserve
   binaries/source hashes, instrumentation-only diff, input hashes, stdout,
   stderr, strict client/server traces and failure results. Do not patch A with
   owner, cancellation, credential listener or new shutdown behavior.
3. For real held-body storm and Checking-backoff inputs, B's instrumented active
   logical read/backoff peak must be <=4; each entered Future must drop before B
   exits. A must actually expose >4 overlapping work for the storm to count as a
   defect comparison. B cancellation has a preselected 2000 ms upper gate for
   observable old whole rounds on replacement and old rounds/read/backoff futures
   on Git-memory invalidation. Identical instrumentation records the start before
   actual replacement admission or settings mutation, and joins it to the actual
   admission/post-mutation observer and actual drops. This conservative interval
   includes admission wait; missing/out-of-order starts or missing drops fail,
   never become zero. Already settled work is excluded. Unpolled rounds, metadata,
   totals and independent workspace jobs are outside this measured time gate.
   Native paging must finish on actual page4/numbers301..310;
   project switching must finish on the second repository/numbers501..510.
   Ordinary storm/page/project/backoff scenes must eventually show ten Clear rows
   after releasing the real gate. Entire actual round entry/settlement is observed
   independently of read/sleep gauges, so an await gap does not count as completion.
4. Real `AppState` Git token/hosting saves encounter a directory at the actual
   settings atomic-write target, after the original TOML has been loaded.
   Confirm actual SettingsManager memory change and actual failed UI feedback.
   Before releasing held responses, B must have zero old read/backoff futures,
   no eligible list repository and ten Unknown rows. A must retain old reads;
   otherwise the baseline defect was not exercised. Restore the file after the
   failed feedback so later cleanup is not a second artificial save fault. Fault
   injection is one-shot independently of fault-active cleanup. With no subsequent
   Refresh or locale-triggered enrichment, the final actual rendered B list must
   remain ten Unknown rows with no eligible repository; A retains ten Checking
   rows and its old list-repository key. The initially held four old-key reads
   must not commit after the change. A's remaining six may capture new credentials
   when first polled and commit legitimate new-key cache entries; those are not
   old-key leaks. Actual whole rounds must settle before either normal exit.
5. Native held-read shutdown invokes the existing request_shutdown. Actual
   legacy-drain completion must observe B owner closed, zero reads/backoffs and
   actual join completed. A should still have accepted unowned reads at that
   barrier. This is a feature-lifecycle comparison; it does not claim the
   rejected #77 total-exit, timeout or failure-sentinel guarantees.
6. Actual Workflow route/PR flow2, startup reveal and selected-list
   AfterRendering snapshots are required. Invoke actual AppState locale callbacks
   en→zh-CN→en on ordinary scenes; SaveErr scenes stop at their actual invalidated
   frame because locale synchronization also starts a new enrichment. Shared status catalog getters must refresh immediately and match
   their semantic source values and a real subsequent frame. Binding/frame
   observations do not prove pixel-level glyph/hover/viewport quality.
7. Run the repository's existing P5 Home Release guard separately with the new
   fixture env absent. Keep its original RSS/effect/normal-exit thresholds, actual
   reveal/nonactivation and framebuffer observations, and raw
   evidence. Route/request counts cannot replace that regression check; do not
   infer an unmeasured performance improvement from cancellation source alone.

Native real HTTP fixtures use exact operation names and valid provider JSON.
Unregistered requests remain explicit failures. GitHub/GitLab define Checking in
this corpus and cover actual sleeps; public/enterprise Gitee lack that selected
Checking signal, so their backoff scenes are explicitly skipped. Read/HTTP
cancellation is local Future ownership; peer EOF is recorded separately, with
no assertion that server-side work stops at the same instant. Full online
permissions/hosting compatibility and non-macOS native execution remain external
acceptance, not local fixture proof.

Preparation review fixed unexecuted instrumentation errors before building:
repeated artificial SaveFault injection, hidden locale-triggered enrichment in
SaveErr finals, a pre-release Unknown body wrongly expected to become Clear,
untyped or missing samples becoming zero, and failure paths losing analysis.
All original tools are preserved as invalid-before-execution, not product failures.
Whole-round observations and pre-action cancellation starts now supplement actual
read/sleep guards. Test holds have bounded waits and RAII release, including
negative paths, so a failed assertion cannot leave a permanent blocking actor.

Actual iteration evidence also retains the first unit-test failure: immediate
subscription hydration blocked the test hold before a contributing actor could
start. The test now observes changed commits, matching its stated race, and all
eleven owner/cache tests passed. No production ownership change was made for that
test correction. A Release built successfully. The first B build reused the A
binaries through the shared Cargo target; its successful command is explicitly
invalid as B evidence and was never measured. Timestamp-only refresh of all
owned B inputs forces rebuilding; binary hashes must differ and actual source
provenance must pass before the controlled runs.

## Executed controlled comparison and decision

- Ordinary Release A and B built successfully with the same lock, profile,
  default features plus test-fixtures, two runtime workers and byte-identical
  shared Rust fixture. Frozen executable hashes differ, and actual A dependency
  content was audited. The incorrect freeze target name, incomplete copy and
  invalid first B cache hit are preserved, without measurement credit.
- Component formal matrix: 60/60 passed, four provider profiles, AB/BA/AB.
  Six independent smoke runs passed. Actual baseline held-body/backoff storm
  peaks reached 68 logical jobs; candidate peak stayed four. B actual entered
  futures and whole rounds settled before exit, including held shutdown.
- Native formal matrix initially passed 155/156: all 78 candidate runs passed;
  one GitHub storm A timed out awaiting a rendered-frame/result receipt, although
  its actual whole rounds settled by 859261 us. Its last frame was at 437396 us;
  the rendering cause remains undetermined. The entire GitHub storm AB/BA/AB
  scene was repeated with unchanged binaries/tools/gates and passed 6/6. The
  complete validated matrix uses 150 unaffected originals plus that complete
  six-run repeat; original failed raw/report remains intact. This is not a claim
  that the first matrix passed. See `native-adjudication.json`.
- Native smoke initially passed 13/14: the switch-project verifier incorrectly
  demanded replacement cancellation even though prior scope invalidation had
  already canceled the old round. Python-only v2 instead requires four actual
  initial-repository held bodies, exact old identities, canceled completion and
  every old read/round settled before second-project round entry, without old
  cache commit. Its conservative whole-round lifetime uses the unchanged 2000 ms
  gate; it is not callback-to-cancel timing or server completion. Eleven tool
  tests include negative missing/wrong/late receipts; fresh project smoke passed
  2/2. Shared Rust child and executables are unchanged.
- Native B logical peak maximum was four; measured conservative pre-action
  replacement/settings cancellation maximum was 6.923 ms. Initial old-project
  complete round lifetime maximum was 52.089 ms. Final page/project identities,
  real SaveErr memory invalidation, locale en/zh-CN/en and actual legacy drain
  receipts passed their strict scene gates.
- Home six 30-second AB/BA/AB runs plus extended guard passed. Peak median A/B
  115.28125/115.09375 MiB, Settled 114.78125/114.609375 MiB, growth
  -0.40625/-0.359375 MiB. All fixtures and persisted revisions completed, five
  Settings writes each, revealed nonactive windows, zero framebuffer errors and
  clean exits in 78.236–148.363 ms. No runtime RSS improvement is claimed.
- Full quality initially reused the older 718-test executable; that result is
  invalid for this candidate and is preserved. Timestamp-only refresh of all
  application-owned inputs forced current source recompilation. The actual
  current full run passed eight checks and 893 tests, zero failed, four ignored,
  30 groups (root 730 passed/two ignored). Exit remains one for the same existing
  structure budgets (3084/3000 lines, 422/410 properties, 717/580 action lines)
  and i18n Clippy at `crates/usher-i18n/src/lib.rs:89`. Later lint diagnostics
  remain unverified because Clippy stops there. Format, full root UI policies,
  i18n, script tests, workspace all-targets check/tests, no-AI check and diff
  checks passed. Direct reqwest client creation remains centralized.

Decision: adopt eight Rust changes and synchronized stable ownership docs.
Rejected #77 remains excluded. Online permissions, other OS native behavior,
pixel/hover checks and the known full logger doctest are not claimed as executed.
Unrelated existing working-tree changes are preserved. The next batch must use
the frozen adopted #79 source, never an invalid cached/test fixture candidate.
