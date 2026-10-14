# P7 #80 Chat sparse history index — candidate gates

Latest retry, 2026-10-06: rejected at actual native gates; no candidate code
was applied. The visible-desktop retry completed both smoke arms and all six
AB/BA/AB callback processes with normal exits. Candidate B failed the strict
two-ready-frame anchor contract in two formal cells; the separate B smoke also
exceeded the existing50ms commit-gap p95 limit. Details follow in the retry
section below. The earlier locked-desktop decision and all raw failures remain
historical evidence.

Status: rejected for this run and reverted to adopted #79 on 2026-10-04.
Component, OS-error, source/type and unit results below remain evidence for the
archived candidate. Required native, physical, full Preview and independent Home
acceptance were not completed. A fresh actual native display precheck still
reported a locked session and inactive/asleep display. This is an unavailable
acceptance prerequisite, not evidence of a candidate-specific rendering defect.
All five candidate Rust files and the four candidate architecture/spec changes
were restored to their exact adopted79 state; the candidate and failed receipts
remain preserved. Conditional #81 cannot use this candidate as its baseline.

Baseline: adopted #79 source archive SHA
`641b419f355cba5dc265896dcb43a70d13e5bf8fbc8a07f5ac675b1d355fe090`.
Evidence destination: `memory-benchmark-results/p7-80-20261003/`.

Final run decision: `final-decision.json`. The fresh environment receipt is
`native-display-r3/display-context.json`; the helper returned0, but the strict
native prerequisite failed. `rollback-to79-r2/result.json` verifies every restored
or removed file against the accepted79 archive, preserving unrelated changes.
The first rollback preparation was stopped before mutation by a mistyped expected
SHA; the corrected exact-source review and applied result are retained separately.
No native/physical/Home result was changed to a pass, and the component gain is
not an application/RSS gain. Re-evaluation requires a fresh visible-desktop run.

The candidate keeps JSONL metadata/defaults, 20-row startup tail, 10-row pages,
existing blocking I/O and session/route generation guards. A clone-shared
immutable sparse index with stride256 replaces repeated full-file scans only
for strictly increasing IDs. Duplicate/nonmonotonic IDs retain original
last-filtered physical order/hasMore. Cold construction, append/invalidation and
warm queries are measured separately; no durable index/raw-page LRU is added.
The unused by-ID lookup and id_start are test/fixture-gated until #81 actually
adopts a consumer. Census counts known source capacities, not all heap/RSS.

Known contract changes are explicit: adding LF before a new record prevents a
valid/damaged EOF suffix from swallowing it, preserving the old byte prefix;
append now needs read permission to inspect EOF. Actual cold scan I/O errors
reject partial indexes. Confirmed append is not revoked by a subsequent
derived-cache refresh error. Refresh trusts append-only growth after sampled
proof; arbitrary unsampled edits (including edit+growth) are unsupported.

Gates fixed before controlled execution:

1. Existing and twelve new real-store regressions, actual I/O faults, locked
   checks/root lib/format/diff and current full quality. Preserve existing
   baseline reds without waiver; ensure current candidate test names actually
   execute, given the #79 shared-target cache invalidations.
2. Frozen standard Release, same lock/compiler/profile and actual dependency
   artifacts, two workers, identical A/B source instrumentation. Real AI/RAG
   100k mixed long/short records; 10k second scale. Serial AB/BA/AB, each pair
   full output/hasMore/default/status/Unicode oracle and actual File.read/write
   returned bytes/serde calls/source capacity census. Missing/partial/invalid
   receipt, forced stop or nonzero exit is failure, not empty success.
3. Warm limit10 decode <=534 and <=1% baseline100k, >=20% lower material warm
   median in every matched pair/mode; actual read bytes within selected
   checkpoints plus actual stopping block/8192-buffer/proof allowance. Cold
   <=2*baseline scan+100ms; startup tail-local and no index. Append reads actual
   new span/incomplete-tail replay/proofs. Keep fallback/cold outside warm gain.
4. Full existing raw prefix and accepted message after missing-LF faults: retain
   actual A loss and require B exact original valid records plus complete new
   source. This deliberate repair is not scored as all-output-equal. Actual
   read/write fault results and recovery are separate from malformed JSON.
   Installed Rust1.98.1 macOS `std::sys::fs::unix::File::flush` is `Ok(())` at
   source line1667; it performs no failing syscall here. Preserve source hash
   and actual flush-call evidence, without fabricating a flush-failure result or
   implying fsync. Other platforms are not covered by this observed boundary.
5. Native source tails must be real100k IDs99981..100000 for AI and RAG, each
   with hasMore. True paging callbacks, mode/re-entry/locale/hidden delayed I/O,
   full >=64KiB Preview input and complete normalized plain-text copy (separate
   actual provider-input/prepared/paint observations), actual AfterRendering
   receipts and strict23column TSV. Markdown copy is document.plain_text; raw
   Markdown and normalized copy must not be compared as identical bytes.
   Callback-only fixtures do not satisfy physical wheel/reversal, two24step
   thumb drags, width/height resize or 800ms idle/anchor/extent acceptance.
   Execute the real physical stage with supported hit testing separately from
   the ordinary Home nonactivation guard; apply existing <=32 rows/50ms/1px
   gates, without freezing ranges or inserting estimated geometry.
6. Normal independent P6/Home/RSS/heartbeat/clean-exit guards with history-fixture
   env absent, original thresholds and actual complete effects. Any failed
   candidate is reverted to adopted79 and cannot be #81's baseline.

## Executed source and OS evidence

The immutable original index-only #80 source snapshot has SHA256
`d03cae639a2b780f8104ab5300d53e6d63d7421900ba0fd36e482af9f1ba1adb`.
Both source-built components use standard Release opt-z/full LTO/one codegen
unit/panic-abort and the same pinned serde/serde_json Release artifacts, traced
to the accepted79 application dependency fingerprint. Source and dependency
hashes/commands are in `A-component-v1/build.json` and `B-component-v1/build.json`.

Fresh `component-smoke-v2` passed eight source cells. Formal
`component-formal-v2` passed 168 cells: two scales, three AB/BA/AB pairs, fourteen
cells per arm. All six independent pair comparisons passed for both AI and RAG.
At100k the A warm median is188.36–191.51ms versus B0.696–0.732ms; at10k
A19.09–19.59ms versus B0.763–0.824ms. These are controlled component operations,
not end-to-end UI speed or RSS gains. Exact source/body/page/default/Unicode
oracles, warm decode/read budgets, cold/startup/append budgets and declared
baseline missing-LF defects passed separately. All raw observations remain in
the individual source cell directories.

The first `component-smoke-v1` failed verification because the reader incorrectly
required a page iteration identity on the legitimate `has_older` operation.
Only the Python verifier changed; strict ordering/count/type requirements remain.
Actual eight-trace positive tests and missing/duplicate/bool/end negative tests
passed before fresh v2 execution. Original failed raw results and the before/after
verifier/provenance are retained; they are not relabeled successful.

`os-formal-v2` passed24 actual OS-error/recovery cells in three AB/BA/AB pairs.
EACCES is from real chmod permissions, EBADF from a safely owned descriptor of
the same history opened write-only and passed to the real std read, and EFBIG
from an isolated child soft RLIMIT_FSIZE with SIGXFSZ ignored. No fake error is
returned by the interposer. Candidate cold read rejects a partial index; an
accepted append followed by a derived-refresh read error returns its accepted
result, drops the index and rebuilds correctly on retry. Real failed writes drop
the index and repaired retry preserves all old bytes plus the complete accepted
new message. Baseline write-only append and baseline retry LF loss are recorded
explicitly rather than counted as candidate correctness failures.

The first OS verifier assumed a128-byte returned partial write in both arms.
Actual macOS A returned4196 bytes then failed the LF write; B returned128 bytes
then EFBIG. The real OS-error and full-source recovery gates were unchanged;
the corrected reader requires positive actual returned bytes within the actual
serialized body instead of that unsupported exact-byte assumption. All12 B cells
already passed; three A verifier failures are preserved in `os-formal-v1`, and
fresh unchanged-binary v2 passed all24. `os-verifier-correction.json` records the
source hashes and boundary. Actual flush calls and the installed std source hash
are retained in the OS summary; no flush error or fsync guarantee is fabricated.

Prepared native tools are not execution evidence. Full physical/native and Home
results, failure causes, unexecuted checks and adopt/reject decision remain pending.

## Current full quality

`cargo fmt --all`, `cargo check --locked`, all15 Chat persistence tests and the
root `cargo test --lib --locked` (742 passed, two ignored) passed. The full
`./scripts/check-quality.sh --full` executed905 workspace tests, zero failures,
four ignored in30 groups; it ran the current742 root tests, rather than the old
cached #78/#79 binaries. Eight checks passed and the aggregate remains exit1.
Existing structure failures are root3084/3000 lines,422/410 properties and PR
actions717/580 lines. Strict workspace Clippy still stops at the existing
`crates/usher-i18n/src/lib.rs:89` unnecessary lazy evaluation. Later Clippy
diagnostics are not established by this stop. The known logger doctest is not
part of the workspace `--tests` gate and has not been rerun for this candidate.
No existing red was waived, no budget was increased, and this result does not
claim the whole repository is green.

## Native fixture correction, no acceptance credit

Both ordinary native A/B source builds passed with the standard Release policy;
the nine common overlay files are byte-identical, and the only business source
differences are persistence.rs plus the two new index files. Build/source/profile
and dependency inventories remain in `A-native-release-v1` and
`B-native-release-v1`.

The first `native-formal-v1` reached the real Preview provider/worker path but
the common fixture waited for `copy_content == raw Markdown`. This contradicts
the inspected application adapter at `src/artifact_preview/mod.rs:870`, which
sets Markdown copy content to the complete parsed document.plain_text. A and B
both timed out; a third B cell was normally terminated once the shared verifier
error was identified, and the remaining three cells were not credited. The
runner was stopped with exit130; existing failed result/trace/log files and
`root-stop.json`/`root-adjudication.json` are retained. All three completed test
processes exited normally with no forced kill, and no child remained running.

No production change or runtime regression has been established by this failed
fixture. The corrected same-source A/B instrumentation must prove the complete
raw provider input and the separately normalized full copy, then rebuild and
rerun a fresh entire callback and physical matrix. The old failed evidence is
not an adoption pass. Native/physical/Home acceptance remains pending.

The corrected v2 tools passed19 evidence-reader and15 physical-reader tests.
Fresh A/B trees were restored from their separate immutable business archives;
all11 common overlay files are byte-identical. The real Preview worker now
records original model/source, prepared plain text, adapted copy and mounted
copy separately. An independent event/block-flattening reference shares only
the CommonMark tokenizer; unsupported fixture grammar fails explicitly. Full
body hashing/parsing stays off the event loop, with one SharedString capture
and a small polled receipt. Tool sources, tests and source manifests are frozen
under `executed-preview-v2-tools` and `native-common-overlay-v2-proof.json`.
These tool tests are not native adoption evidence; rebuilt v2 execution remains
required, and all v1 failed files are preserved.

## v2 baseline native smoke, failed

The new A Release build passed in411.35s; the three actual probe Rust tests also
passed. The actual A process completed112 command/settlement events with13
errors: twelve stale Rust-projected Preview titles after en→zh-CN, and one
request-time anchor observation moving from-151 to2944px. This is a failed
smoke, not a passed native matrix; B and physical/Home have not been run.

The locale failure is grounded in the unchanged baseline:
`interactive::sync_language` clears caches and produces new translated rows,
but `ChatMessageModelBridge::replace_rows` returns on unchanged(mode,id) keys
without updating fields, and its overlap branch likewise retains old fields.
A bounded retained-row reconciliation was prepared as a necessary UI
correctness addition to a new candidate; its source and seven model/renderer
tests are now applied and passed, with749 root tests passing. Native validation
of this addition remains pending.
No renderer source-edit/cache-revision or anchor behavior change is inferred.

The native fixture writes its large JSON after the event loop returns. Actual
legacy platform termination interrupted that write, leaving3,707,605 bytes of
invalid JSON. The complete error ledger and112-event array were recovered from
the intact prefix for diagnosis; neither makes the whole trace valid or earns
acceptance credit. SIGTERM and process exit0 are retained, but the application
logged its platform-termination/force-exit watchdog during the blocked trace
write; this does not prove a clean normal shutdown. The original
log/result/partial trace remain in `native-smoke-v2-A`, with separate root adjudications.
The common observer needs an actual off-UI pre-quit completed-write receipt.

The anchor observer also needs the real production capture made after I/O and
before presentation, rather than assuming the request-time geometry is that
capture. The1px gate remains. A complete-event Preview verification separately
exposed a missing Python hashlib import in the non-sample generator branch;
this actual failure is retained and needs its own branch test. All corrections
require fresh strict evidence; no failed native file is relabeled successful.

## New bounded locale candidate, before native execution

The new candidate adds only `message_model.rs` and its test child to the
unchanged three index sources. It compares the complete generated nine-field
row, refreshes only different retained fields, and preserves ModelRc, structural
keys, prefix/suffix mutation order and the existing selected window. All seven
actual peer-notification/fresh-renderer tests passed, including AI/RAG
en→zh-CN→en,32 rows, shifted overlap and no-op notifications. This does not
repair earlier source-signature/cache invalidation for arbitrary same-ID edits
or claim an anchor, CPU or RSS gain.

Locked check and749 root tests passed, with two existing ignored. The new full
quality gate passed912 tests in30 groups, zero failures, four ignored; its eight
successful checks include full UI policy, i18n, workspace check/tests, feature-off
check and diff. It still exits1 on the same3084/3000 root lines,422/410 root
properties,717/580 action lines and the unchanged i18n Clippy line89. Later
Clippy diagnostics and the separate logger doctest remain unestablished.

Before new native runs, A's observed unchanged-key locale defect is an explicit
functional negative baseline. Its actual locale errors must be retained and
its native/physical product-contract result stays false. Only comparison-evidence
validity may accept that precise baseline negative; every other A failure
remains a failure. B must have zero stale rows and pass every original full-body,
actual-capture1px,32-row, frame/input, physical and independent Home gate.
Top-level evidence validity must never be reported as both product contracts
passing. No old truncated trace becomes valid, and no failed index-only native
matrix is credited to this new candidate.

Preview corpus comparison also follows the inspected existing renderer adapter:
only Normal Assistant records strip a legacy status marker and at most one
ASCII space. Non-Normal records preserve their original body. Disk-original
bytes/hash and complete adapted provider-input bytes/hash are separate facts;
normalization cannot excuse arbitrary truncation. Large-body acceptance retains
the64KiB actual provider-input threshold. This is existing behavior, not a new
production normalization change.

## v3 frozen source and completed tool checks

The new five-Rust-file business source archive contains806 repository files,
SHA256 `4c14157d563a10647b7ab13107f60ca69f58ddb85470568ab15c1063625074d4`.
Clean A and B trees were restored from adopted79 and this unadopted candidate,
then received the same13-file combined observer overlay. Root checked every
installed file hash and equal lock hashes, recorded in
`native-common-overlay-v3-proof.json`. No previous instrumented tree was reused.

Root actually executed27 evidence-reader and17 physical-reader tests, all44
passing, plus all three standalone Rust writer tests against the actual common
writer source. The latter prove full atomic closed trace before receipt even
with a nonzero error ledger, real ENOTDIR open failure and real rename failure
with raw output retained. Commands, lock/source/log hashes and exit0 are in
`writer-probe-tests-v3.json`; frozen executed tools are under `executed-v3-tools`.
These are tooling checks, not product IO, UI, shutdown or performance passes.

The writer now finishes off the event loop before normal quit, retaining the
closed trace and separate actual byte/count receipt. The observer captures the
real production anchor after history commit and immediately before presentation.
Fresh native execution must still prove that row settles within1px; changing
the observer is not a presumption that the earlier drift was only a fixture bug.
The common callback program now requires60 actual commands, at least120 actual
frames, five complete Preview receipts and an en→zh-CN→en sequence on the same
mounted Chat model. B must have zero stale titles. Actual native, physical and
Home adoption gates remain unexecuted for v3 at this point.

## v3.1 two-frame observer gate, before Release execution

Independent source review found the v3 readiness count did not prove both idle
frames were within1px: only the final frame's anchor position was checked.
The callback reader now checks the existing actual F-1/F frame snapshots.
The physical observer retains exactly one bounded first-ready load snapshot,
clears it on a readiness reset and emits first/current real snapshots together.
The physical reader checks each against the same actual captured AnchorSettle,
page token, committed identity, mounted mode/locale and idle/pendingNone state.
The negative first-frame2px/final-frame0px fixture must fail. This is a strict
observer correction; it changes no product anchor or scroll behavior.

Fresh clean A/B sources received the same13-file overlay, with all installed
hashes checked. The only Rust observer difference from v3 is
`src/chat/physical_native.rs`, SHA256
`17569dbd636cc210671f72ea81ec423389315e1b29dfd7de736b396f838856d7`.
The three Preview Rust tests actually passed on v3's full-UI test build in
219.99s; their source and seams are byte-identical in v3.1. This filtered A
fixture run is not the separate749-test candidate library result.

Root actually passed33 callback-reader and23 physical-reader tests. The first
frozen-package physical run returned exit1 with three FileNotFound errors
because its required source-only assembly input was omitted. Root performed a
fresh actual dry source assembly, added those unchanged test inputs and reran
the unchanged23 tests, all passing; the failed log and precise correction remain
in `physical-tool-tests-v31.log` and
`v31-tool-fixture-package-correction.json`. No failure was hidden or waived.
Tool hashes, transitive comparator inputs and source fixtures are frozen under
`executed-v31-tools`; installed common hashes are in
`native-common-overlay-v31-proof.json`. Fresh Release/native/physical/Home are
still pending. No v3 or v3.1 tool result alone is product adoption evidence.

## v3.1 actual Release and failed native prerequisites

The actual A standard Release build passed in395.461538s, exit0; its binary
SHA256 is `d81c0e0f806ea522fed006c3575623c783b7261d43877bdfaa899cb792ffe0cb`.
The build refreshed476 source inputs and retained its compiler, source, lock,
profile and Cargo fingerprint graph in `A-native-release-v31/`. The actual B
standard Release build also passed, exit0 in387.983269s, refreshing479 inputs.
Its binary SHA256 is
`7b0e905f8e43270ff4ac76539b34a0f8edacf7415fdc41f532f17b2e9b4ba22e`.
`native-release-v31-provenance-proof.json` verifies both retained binary hashes,
equal compiler/profile/features/Cargo/lock, the exact five intended Rust source
differences, all other owned inputs identical and48 identical current direct
external dependency fingerprints. The1544 equal cached external fingerprint
records include earlier feature configurations; they are not falsely reported
as1544 dependencies rebuilt by this run.

The actual A callback smoke exceeded its180s external deadline while still on
Home. Locale(en) was invoked, but the60-command contract did not finish. Its
normal exit was0 without a forced stop; the completion trace and closed-writer
receipt were absent. This is FAILED evidence, retained in
`native-smoke-v31-A/`. A header-only Chat frame trace does not establish that
Home rendered no frames: the inspected performance observer records Chat only.

The separate actual physical smoke failed before the first preparation command:
the real Quartz helper timed out waiting for1280×860 and reported1152×775.
The process exited0 normally in6.258529s, with no forced stop; the actual111-byte
event writer closed, flushed and synced with zero lost events. There was no
completed physical contract. Both failure and writer receipt remain in
`physical-smoke-v31-A/`; writer success is not UI acceptance.

An isolated same-binary diagnostic retained a real1s main-thread sample and a
real SIGUSR1 resize callback. The sample showed ordinary AppKit/CFRunLoop waiting
(618 of626 main-thread samples); the event-loop callback logged a resize to
800×600. The real on-screen window-list helper returned exit0 with no owned
window. These observations alone do not diagnose a product deadlock or establish
the reason for the earlier size mismatch. See `native-v31-diagnostic-r2/`.

Root then executed a separate read-only native display/session diagnostic.
`display-context-v31-native.json` reports `CGSSessionScreenIsLocked=true` and
display1 `active=0`, `asleep=1`, with an1800×1169 screen at scale2. This proves
the visible-desktop prerequisite was unavailable at that diagnostic time; it
does not retrospectively assign every earlier failure to one cause. The same
diagnostic inside the filesystem sandbox returned an empty screen list; that
separate `display-context-v31.json` is not native display-state evidence.
The diagnostic only reads state: it does not unlock or wake the machine.

The user has been asked to restore a visible, unlocked desktop. Actual native,
physical and independent Home A/B adoption gates remain pending. No failed run
is credited, no acceptance threshold is relaxed, and #80 remains unadopted.

A fresh driver-only v3.2 adds a read-only native display prerequisite without
rebuilding or modifying either v3.1 binary. Its first12-test run failed one
expectation: macOS's `/var` temporary path was compared with the actual command's
canonical `/private/var` path. A fresh copy corrects only the test expectation to
the inspected `helper.resolve()` behavior. All12 tests then passed; original
tools, failed log and exact test hashes remain in
`display-guard-tests-v32.log`, `display-guard-tests-v32-r2.log` and
`display-guard-v32-test-correction.json`. Driver and guard code were unchanged.
These positive/negative reader fixtures validate environment-tool behavior only.
They do not prove a visible desktop, rendered frame or product acceptance.

## 2026-10-06 visible-desktop retry — actual native rejection

The user reported an unlocked, awake desktop. Current clean HEAD is
`30e7862e5da168a38e8662a68a87f9af696bf2f2`; all468 production source/build
inputs still match the previously adopted79 inventory. The current807-file
Git archive is frozen separately with SHA256
`9b3be9b61055cb723f48fb21e9512bc7711ee4c0b81c0b48329f1a41ea0b96a0`.
The retained v31 A/B binary hashes and source/profile/compiler provenance were
rechecked before execution. No candidate source was restored to production.

The fresh real display helper returned console/login true, display1 active1
and asleep0 at1800×1169/scale2, with a null lock field. The original guard
rejected null because it demanded explicit false. Its failed receipt is retained.
A driver-only correction allows a readiness attempt for this exact null schema
only with all other strict prerequisites; it records `lock_state:not_reported`
and never claims null proves unlocked. Missing/wrong-type fields, explicit lock,
empty/sandboxed screens and sleeping/inactive displays still fail. Sixteen
reader/receipt tests passed. Neither Swift helper nor callback/physical readers,
Rust instrumentation, binaries or acceptance thresholds changed. The correction
is only environment readiness, not product acceptance.

Actual execution and independent raw review:

- Smoke A:60 commands completed,1202 strict TSV rows and a closed/synced writer;
  normal exit0. It failed on an actual captured anchor moving from−151px to
  2944px in both ready frames, plus16 existing locale-title negatives. The
  partially visible original row was547px high, so it was a legitimate visible
  anchor. This is an observed baseline failure, not a proved candidate-only cause.
- Smoke B:60 commands,32 two-frame anchor receipts, zero semantic/locale errors,
  five complete Preview proofs and closed/synced atomic trace passed. Of the
  five complete source inputs, three were65536 bytes and two256 bytes. Its
  320-row TSV has64 commit samples/62 positive gaps: p95=52.9641ms, max90.864ms,
  over the unchanged50ms p95 gate. Mutation-to-render max19.392ms and mounted
  max30 do not replace the failed gap metric. Native321 AfterRendering samples
  and320 TSV rows are distinct observer ranges; no unit/sample mismatch was found.
- Fresh formal AB/BA/AB: all six processes completed and exited0, no timeout or
  forced stop. The three A comparison receipts passed while preserving their
  known locale product negative. B passed one cell and failed two. Pair1-B
  token9 RAG/en20→30 and pair2-B token26 AI/zh-CN110→120 both had the actual
  captured anchor row100000 absent from the first idle/ready frame, despite
  loading/restoring=false and pending-anchor=None. The next frame reinserted
  it at the correct−151px. The mandatory F−1/F reader correctly rejects this
  one-frame removal; final-frame-only Rust `errors=[]` does not override it.

Decision: reject this retry. Physical, controlled delayed I/O, independent
Home/P6 and adoption checks were not advanced after the mandatory native gate
failed. No index-specific root cause, complete adoption or application/RSS gain
is claimed. No failed source was applied or becomes #82's baseline, and #81
remains conditional. Raw commands, all eight actual processes, original guard
failure, guard source/test freeze and analysis are retained in
`memory-benchmark-results/p7-80-20261006/`; earlier results are unchanged.
