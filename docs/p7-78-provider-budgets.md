# P7 #78 Provider response budgets / Provider 响应预算

Adopted 2026-10-03. Prerequisite: accepted #73; baseline retains accepted
#74–76 and excludes rejected #77. / 候选；前置 #73 已采用，基线保留 #74–76，
不包含已撤回 #77。本批通过独立验收，保留预算代码。

## Preselected contract / 预先契约

Ordinary metadata JSON: 16 MiB per consumed response. PR files/diffs/commit
comparisons: 64 MiB per consumed response. GitHub, GitLab and Gitee public/v8
use the actual shared HTTP pipeline and keep their request operation, auth,
retry policy and normalization. No partial body/diff success. Preserve source
12-MiB JSON / 8-MiB decoded or raw limits and existing 1/16-MiB conflict limits.
These are finite accepted inputs, not universal provider-legal PR support,
pagination/normalized/cache budgets, or an RSS gain claim. Headers-only and
ignored successful bodies remain unconsumed; adding a limit must not add reads
or mutation retries. Large diff explicit load remains a post-download syntax gate.

普通元数据 JSON 每响应 16 MiB；PR 文件/diff/commit 比较每响应 64 MiB。
三平台及 Gitee 两 profile 使用真实统一 HTTP 管线，保留 operation、认证、重试
和归一化；不得返回成功的部分正文/diff。源码与冲突既有预算不变。有限输入边界
不等于全部 Provider 合法 PR 支持、分页/归一化/缓存预算或 RSS 收益；只读 header
及忽略成功正文的路径不增加读取或变更重试。大 diff 显式加载仍在下载后执行。

## Preselected validation / 预先验收

- Register every real RequestOptions construction and consumed-body type,
  including Gitee public/enterprise and dynamic operations. Preserve the eight
  existing protected declarations. Distinguish GitLab's header-only commit count
  and Gitee's seven ignored-success-body mutation constructions.
- Unit/self-check plus controlled Release actual-facade -> actual adapter ->
  shared HTTP component A/B, three independent AB/BA/AB pairs. Same exact input,
  loopback server/gates, immutable source/dependency/profile/binary hashes. A
  receives identical instrumentation only and retains its unbounded old policy.
- Both metadata and diff exact/over inputs use the actual 16/64-MiB production
  limits and chunked no-Content-Length valid JSON. Below/exact succeed completely;
  +1 fails with real ResponseTooLarge; successful-header overflow is not retried.
  Preserve status retry, idle/cancel, optional fallback and mutation single-attempt.
- Legal large Unicode patches, pagination, normalized full file/row/content
  checksums and post-download explicit syntax loading. No silent truncation.
- Full native regression retains P5/P7 gate: isolated/offline nonactive Home,
  two workers, 30s x AB/BA/AB; Peak/Settled RSS delta <= max(8 MiB, 5% A median),
  B growth/growth delta <=8 MiB, complete UI/preference/durable fixture outcomes,
  zero framebuffer errors and clean normal exit <=8s. Component HTTP evidence
  and native Home cost regression are separate; rendered frame latency is not
  inferred from callbacks.
- Existing list/diff error projection inspected; execute affected error/loading
  and explicit-load route behavior in source and packaged non-source locale, or
  state exact unexecuted native/manual rows. A missing observation is not success.
- cargo fmt/check/lib/diff plus #73 quality layers are run by root. Record failed
  or unavailable tooling, exact outcome and adopt/reject; do not advance failed B.

## Execution and decision / 实施与决策

**Adopted; retain the six provider/budget source files. / 采用，保留六个 Provider/预算源文件。**

Production changes are limited to GitHub/GitLab/Gitee adapters, a shared feature
budget declaration and its tests. The inventory has 87 RequestOptions sites:
71 metadata, 8 diff, 8 preserved source/conflict declarations. Header-only and
ignored successful mutation bodies remain unconsumed. Static independent coverage
also compares old/new operations, retry and body-reader policy; no HTTP executor,
UI, Settings or rejected #77 code changed.

| Executed check / 实际检查 | Result / 结果 |
| --- | --- |
| Default `cargo check --locked` | Passed |
| Existing shared HTTP budget tests | 5 passed |
| New provider declaration/policy tests | 3 passed |
| Root `cargo test --lib --locked` | 718 passed, 0 failed, 2 ignored |
| `cargo fmt --all`, `git diff --check` | Passed before Release assembly |
| Architecture structure | Same three old red items: root Slint 3084/3000, properties 422/410, PR actions 717/580 |
| v2 actual Release component boundaries: four profiles × nine scenes × AB/BA/AB | 216 processes passed; complete 24-file/14,424-row Unicode results match; real +1 overflow and no replay |
| v2 actual retry / overdeclared truncated body / held-body cancellation | 72 processes passed |
| Shared Python supplement: actual POST comment success / HTTP500 | 48 processes passed; one attempt, Gitee successful bodies not newly read |
| Shared Python supplement: 100+1 file pages (GitHub/GitLab/Gitee public) | 18 processes passed; 101 unique files, 202 normalized rows; both physical pages observed |
| Shared Python supplement: GitLab detail missing `commits_count` | 6 processes passed; real header-only fallback observed and full diff matches |
| Earlier GitHub component smoke | 18 processes passed; separate from the 360 formal runs |
| Native v2 smoke | Invalid fixture: 2 pass, 4 fail/timeout; not a product adopt/reject result |
| Native v3 formal routes | 120 processes: 117 passed, 3 GitLab List B runs rejected by an incorrect fixture error-kind assertion; raw failures preserved |
| Driver-only v4 GitLab List rerun | 6 passed with identical v3 binaries/inputs and exact existing `invalid_response` classification; other 114 v3 runs plus these 6 form the accepted 120-run route matrix |
| Native Home: 30s × AB/BA/AB | 6 passed; Peak median A/B 115.703/116.047 MiB, Settled 115.203/115.563 MiB, growth -0.406/-0.391 MiB; all original RSS gates pass |
| Home effects and exit | All revisions 501/1002/1503/2004; 5 settings writes/run; no framebuffer errors; revealed nonactive windows; normal exit 38–123ms |
| Final #73 full quality rerun | 8 checks passed, 2 existing failures; exit 1. Workspace tests 881 passed/0 failed/4 ignored across 30 groups; default and no-AI all-targets checks pass |

All component arms use immutable standard Release binaries with the same
default + test-fixtures features, opt-z/LTO/codegen-units=1/panic-abort policy,
lock hash and shared instrumentation. Each formal cell has three independent
AB/BA/AB pairs with matching semantic input hashes and raw client/server traces.
The supplement changes only common Python input/observation code, not binaries.
The evidence root is `memory-benchmark-results/p7-78-20261003/`, including both
build manifests, source hashes, Cargo fingerprint graphs, exact instrumentation
patches, every process result, normalized output and byte-limit observations.
No runtime/RSS gain is inferred from bounded input or component callback timing.

The uncompiled v1 preparation is preserved as INVALID. The v2 native smoke
uncovered three preparation errors: an unregistered real GitHub timeline read;
passing initial_tab=0 (Conversation) while waiting for Diff paint; and comparing
hidden List diagnostic fields against a mounted Review frame. Its large-row
assertion was also weaker than the exact 601-row expected first file. Native
v3 uses the real callback's initial_tab=2, records/validates the actual tab and
inactive conflict workspace, compares only the selected surface's render key,
and requires 24 files / selected file 0 / exact path / 601 rows after explicit
load. Both v2 full source archives, tools, binaries and failed raw observations
remain preserved. These changes are shared fixture corrections only.

The formal v3 matrix exposed one further driver error: it required `unknown`
for every overflow. The existing GitLab List adapter wraps response.json failure
with `decode failed`, and the unchanged GitError classifier correctly projects
`invalid_response`; the other seven profile/surface combinations project
`unknown`. V4 changes only this exact Python assertion table. Its six reruns
reuse the immutable v3 binaries; no Rust, server, payload or dependency changes
were made. `native-adjudication.json` selects the unaffected 114 v3 processes
and the six corrected v4 processes; v3's original failure summary is untouched.
126 formal processes were physically executed, plus six earlier v3 smoke runs.

Language observation uses the actual Slint Global fields referenced by the
mounted error components plus matching AfterRendering snapshots through the
real AppState locale callback. It is not a pixel/glyph or physical mouse check.
Every optional endpoint is not natively invoked merely because all 87 declarations
are registered. The logger doctest, remote CI and Windows are unexecuted. This
input-budget change claims complete accepted fixture outputs and bounded reads,
with a passing Home cost regression; it does not claim an RSS or latency gain.

The full-quality failures are unchanged from accepted #73: the three structure
budget violations above and strict Clippy stopping at the existing
`crates/usher-i18n/src/lib.rs:89` `unnecessary_lazy_evaluations`. They remain
nonzero failures; later crates' strict lint has not been established. No new
failure was found in this candidate's checks or native gates. `decision.json`
records the adopt decision, failure comparison and unexecuted rows. The final
source snapshot is the next batch's baseline; rejected #77 remains excluded.
Earlier accepted changes and unrelated ignored working-tree files are preserved.
