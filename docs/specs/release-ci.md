# Release CI / 生产构建与缓存

## Scope and owners / 范围与职责

This contract covers the existing GitHub Release workflow's production failure
boundary, manual cache prewarming, cache restoration and diagnostic exports.
It does not change Release optimization parameters, product features, target
support or signing policy, and does not promise a hosted build time.

本文约束现有 GitHub Release 工作流的生产失败边界、手动缓存预热、恢复及诊断导出。
不改变 Release 优化参数、产品 feature、支持 target 或签名策略，不承诺云端构建耗时。

| Owner / 所有者 | Responsibility / 职责 |
| --- | --- |
| `.github/workflows/release.yml` | Triggers, typed input, exact ref guard, matrix/build/cache/package/sign/publish and failure propagation / 触发、类型化输入、精确 ref 校验、矩阵/构建/缓存/打包/签名/发布与失败传播 |
| `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`, `rust-toolchain.toml` | Compiler/profile/feature and dependency inputs; Release remains opt-z, full LTO, CGU1 and incremental off / 编译配置和依赖输入，保持现有 Release 策略 |
| `scripts/ci_release_metrics.py` | Eight-field cache metadata allowlist and current-attempt job/step projection; no network execution / 八字段缓存元数据白名单与当前 attempt 时间投影，不自行联网 |
| `scripts/ci_release_probe.py` | Validated manual probe identity, isolated cache keys and strict warm-hit verification; no Cargo/network execution / 手动探针身份、隔离缓存 key 与严格热命中校验，不执行 Cargo/网络 |
| `scripts/ci_release_build.py` | Opt-in fixed Release invocation and bounded fingerprint projection; preserves Cargo failure without exporting raw child output / 显式诊断模式的固定 Release 调用与有界 fingerprint 投影，保留 Cargo 失败且不导出原始子进程输出 |
| `scripts/tests/test_release_workflow.py`, `test_release_workflow_behavior.py`, `test_ci_release_metrics.py`, `test_ci_release_probe.py` | Workflow contracts, finite local policy, real subprocess privacy/exit controls and export fixtures / 工作流契约、有限本地策略、真实子进程隐私/退出码与导出夹具 |

The operational entry is [Production CI in the build guide](../build-performance.md#production-ci-cache-and-timings--生产-ci-缓存与计时).
Current local evidence and unexecuted checks belong to the
[validation record（归档：docs/validation/release-ci-reliability-20261016/README.md）](../local-archives.md#validation-experiments-20261016).

操作入口见[构建指南](../build-performance.md#production-ci-cache-and-timings--生产-ci-缓存与计时)，
本地证据和未执行项见[验证记录（归档：docs/validation/release-ci-reliability-20261016/README.md）](../local-archives.md#validation-experiments-20261016)。
The [fingerprint follow-up](../archive/maintenance/release-ci-fingerprint-20261016.md)
records the current tooling evidence and unexecuted hosted probe plan.
当前诊断工具证据与未执行的云端探针计划见
[fingerprint 后续记录](../archive/maintenance/release-ci-fingerprint-20261016.md)。

## Modes and ref boundary / 模式与 ref 边界

- Pushing a `v*` tag remains the only automatic Release trigger. Manual
  `workflow_dispatch` remains available; no branch-push/schedule prewarm is added.
- `cache_only` is a boolean with default `false`. False or absent retains normal
  production behavior: tag refs may publish after successful jobs; ordinary
  branch dispatch builds/packages without publishing a GitHub Release.
- True uses the same macOS arm64, macOS x86_64 and Windows x86_64 build matrix,
  Rust setup, production build and cache identities. It skips packaging-tool
  installation, packaging, signing/notarization, product upload and release.
- Each build job checks a true manual prewarm before checkout/cache operations.
  Pass the repository default branch as environment data, then compare quoted
  `GITHUB_REF` to `refs/heads/$RELEASE_DEFAULT_BRANCH` in Bash. Comparison must be
  case-sensitive. Other branches, case variants and same-name tags fail explicitly;
  skipping the job with a successful workflow is insufficient. Never interpolate
  ref/default-branch data into shell source or diagnostic text.

- push `v*` tag 仍是唯一自动发布触发器；保留手动入口，不新增分支 push/schedule 预热。
- `cache_only` 是默认 `false` 的布尔值；关闭或缺失时保留正常行为：成功的 tag 运行
  可以发布，普通分支手动运行构建/打包但不创建 GitHub Release。
- 开启后复用 macOS arm64、macOS x86_64、Windows x86_64 三个 target 的矩阵、Rust
  准备、生产 build 与缓存身份，跳过打包工具安装、打包、签名/公证、产品上传及发布。
- 两个 build job 都必须在 checkout/缓存操作前校验手动预热。默认分支名按环境数据
  传递，Bash 引用操作数、区分大小写比较完整 `refs/heads/` ref；非默认分支、大小写
  变体和同名 tag 明确失败，不能仅跳过 job 后假成功。ref/分支数据禁止拼入 shell
  源码或错误文案。

### Controlled cache probes / 受控缓存探针

`cache_diagnostics` is an optional boolean, default `false`. `cache_probe` is an
optional `normal`/`cold`/`warm` choice, default `normal`; `probe_id` defaults to an
empty string. Ordinary tag runs and default manual runs retain direct Cargo output
and the normal cache namespace. Explicit diagnostics enable the bounded projection
below; cold/warm probes enable it automatically without changing Cargo's build
graph, Release flags, default features or `CARGO_INCREMENTAL=0`.

`cache_diagnostics` 为默认 `false` 的可选布尔值；`cache_probe` 默认 `normal`，可选
`cold`/`warm`，`probe_id` 默认空。普通 tag 和默认手动运行仍使用直接 Cargo 输出及
正常缓存 namespace。显式诊断启用下述有界投影，冷/热探针自动启用；不改变 Cargo
构建图、Release 参数、默认 feature 或 `CARGO_INCREMENTAL=0`。

- Cold/warm require a manual default-branch `cache_only=true` run. Invalid modes,
  an ID in normal/cold mode, or malformed warm IDs fail before checkout. The
  exact Bash ref guard still runs first.
- Cold assigns a unique `<run-id>-<attempt>` cohort, skips both registry and build
  restoration, and saves successful builds under `release-probe-v1-<cohort>` keys.
  These keys are separate from normal production/prewarm keys.
- Warm names the prior cold cohort and accepts no fallback. Before Cargo, both
  registry and build restores must report `cache-hit=true` and `outcome=success`.
  Miss, partial restore, wrong source/configuration/Rust/target identity or eviction
  cannot silently turn a warm probe into a cold build. Planning/verification is
  blocking for probes; optional normal-mode planning remains best-effort.
- Keep the same source SHA, configuration, installed Rust and runner class across
  the pair, and inspect all three targets. Cargo always runs after a warm restore;
  exact cache identity does not promise fresh units or improved elapsed time.

- 冷/热探针只允许默认分支手动 `cache_only=true`；非法模式、normal/cold 带 ID、
  非法 warm ID 在 checkout 前失败，精确 Bash ref 校验仍是首步。
- cold 使用唯一 `<run-id>-<attempt>` cohort，跳过下载与编译缓存恢复，成功后保存到
  `release-probe-v1-<cohort>`；与正常生产/预热缓存隔离。
- warm 指定先前 cold cohort，不使用 fallback；Cargo 前下载和编译两项都必须
  `cache-hit=true` 且 `outcome=success`。未命中、部分恢复、源码/配置/Rust/target
  身份不符或淘汰不能悄悄降为冷构建。探针规划/校验阻断；普通模式可选规划尽力执行。
- 一对样本保持相同 SHA、配置、Rust 与 runner 类别，并核对全部三个 target。
  warm 恢复后仍执行 Cargo；exact 身份不保证编译单元 fresh 或耗时降低。

Deploy the reviewed workflow/helpers to the repository default branch before
using these inputs. Probe caches do not seed normal production caches. Test
default-branch prewarm and Windows normal warm restoration separately with
`cache_probe=normal`; a rerun of the same tag does not establish cross-tag reuse.
The current candidate has not been dispatched on hosted runners.

这些输入必须先随审核后的 workflow/helper 部署到默认分支。隔离探针不预热正常
生产缓存；默认分支预热和 Windows 正常热恢复另用 `cache_probe=normal` 验证。
同一 tag 重跑不证明跨 tag 复用；当前候选尚未在托管 runner 调度执行。

## Production and diagnostic failures / 生产与诊断失败

Builds use `cargo build --release --locked` with the existing target/jobs/timing
flags and default product features. Do not replace it with a cache-hit skip or
`release-fast`. Keep the existing bundle command; do not assume cargo-bundle
supports arbitrary Cargo flags. Successful compilation may save unsigned caches
before packaging; failed builds must not save them. Exact cache hits need no new
immutable snapshot.

生产 build 使用 `--release --locked`，保留现有 target/jobs/timing 参数和默认产品
feature；缓存命中不能跳过 Cargo，也不能切成 `release-fast`。bundle 命令保持现状，
不得假定它支持任意 Cargo flag。成功编译可以先保存未签名缓存再打包；构建失败
不得保存，exact hit 不重复创建同 key 的不可变快照。

Product Actions uploads explicitly retain artifacts for **7 days**; per-target
diagnostics and the final timing report retain artifacts for **3 days**. Keep
existing product names and files for updater/download compatibility. Published
Release assets are independent of these temporary handoff artifacts.

产品 Actions 上传显式保留 **7 天**，各 target 诊断与最终计时报告显式保留 **3 天**。
保留现有产品名称和文件，维持更新/下载兼容；已发布的 Release 附件独立于这些
临时中转 artifact。

An artifact quota rejection can occur after successful compilation and packaging.
Keep product upload blocking; swallowing the failure leaves the publication job
without its inputs. Actions cache storage has a separate allowance, so clearing
compiler caches does not free artifact storage. Shorter retention affects only
new uploads. Before deleting old artifacts, enumerate IDs/sizes/expiry and check
the corresponding published assets; obtain authorization for the exact deletion
scope. Do not delete Releases, current-run products or caches as a quota workaround.
Usage refresh may take 6–12 hours; cleanup does not guarantee immediate recovery.

artifact 配额拒绝可发生在编译和打包成功之后。产品上传仍须阻断，不能吞掉失败，
否则发布缺少输入。Actions cache 另有额度，清编译缓存不会释放 artifact 存储。
缩短保留期仅影响新上传。删除旧产物前列明 ID、大小和到期时间，核对对应发布附件，
并取得针对该清单的授权；不得通过删除 Release、当前运行产物或 cache 绕过配额。
用量刷新可能需要 6–12 小时，清理不保证即时恢复。

| Failure / 失败 | Required behavior / 必须行为 |
| --- | --- |
| Diagnostic Python setup, cache metadata or diagnostic upload / 诊断 Python、元数据或诊断上传 | Best-effort; visible failure must not block otherwise successful production / 尽力执行，错误可见且不阻断正常生产 |
| Final timing-report job / 最终计时 job | Job-level best-effort; failed collection stops summary generation, never fabricates success / job 级容错，采集失败不伪造成功报告 |
| Build, packaging, enabled signing/notarization, product upload or publication / 构建、打包、已启用签名/公证、产品上传或发布 | Remain blocking; diagnostics tolerance must not make them green / 保持阻断，不能被诊断容错改成成功 |
| Invalid manual prewarm ref / 非法预热 ref | Fail before checkout/restore/build/save; reporting cannot erase that failure / checkout/恢复/构建/保存前失败，报告不能抹掉失败 |
| Invalid probe plan or non-exact warm restore / 非法探针或 warm 非精确恢复 | Probe must fail before Cargo; normal production diagnostics remain best-effort / 探针在 Cargo 前失败，正常生产诊断仍尽力执行 |
| Cancellation or unavailable API / 取消或 API 不可用 | Preserve actual status; report availability is not guaranteed / 保留实际状态，不保证仍可生成报告 |

## Layered caches / 分层缓存

Keep separate Cargo download and compiler-output caches. Compiler paths include
host `target/release/`, the selected target's Release directory and
`target/.rustc_info.json`, excluding development profiles and signing credentials.
The macOS tool-binary cache is retained; cache-only prewarm does not install
cargo-bundle/create-dmg and therefore does not guarantee those tools are warm.

下载缓存与编译缓存分开；编译路径包含 host/当前 target 的 Release 目录和
`.rustc_info.json`，排除开发 profile 与签名凭据。保留 macOS 工具二进制缓存；仅预热
模式不安装 cargo-bundle/create-dmg，不能承诺这些工具已有缓存。

Restore in this order:

1. Strict primary key: epoch, OS, host architecture, target, installed Rust cache
   key, `release-default`, configuration hashes and source SHA.
2. Strict fallback: the same fields without source SHA.
3. Coarse fallback: epoch, OS, host architecture, target, Rust and `release-default`.

恢复依次为：含配置 hash/源码 SHA 的严格主 key、只去源码 SHA 的严格 fallback、再到
只保留 epoch/OS/宿主架构/target/Rust/`release-default` 的粗层 fallback。

The coarse layer can cross manifest, lock, Cargo config and workflow changes,
including actual Release profile changes whose namespace label remains unchanged.
The key does not certify complete profile/feature or native-environment identity.
Cargo must run after every restore and validate its tracked profiles, features,
source inputs and fingerprints. SDK/native tools outside that tracking require a
deliberate epoch change across primary and fallback keys or a cold cache. A cache
match proves neither fresh compiler units nor net wall-time savings.

粗层允许跨 manifest、lock、Cargo config、workflow，实际 Release profile 改变而标签
未变时也可恢复。key 不证明完整 profile/feature 或 native 环境一致，恢复后必须由
Cargo 校验已跟踪的 profile、feature、源码输入和 fingerprint。未被跟踪的 SDK/native
工具变化需同步提升主 key 与 fallback 的 epoch，或使用冷缓存。匹配缓存不等于
编译单元 fresh，也不等于总耗时降低。

Caches are immutable and ref-scoped. Different tags cannot consume each other's
caches; tags may consume default-branch caches. After the change reaches the
default branch, manually select that branch with `cache_only=true` and
`cache_probe=normal` to seed reusable production build caches. The isolated
cold/warm probe namespace does not seed these production caches. Expiration,
eviction, checkout mtimes and transfer/save overhead
remain possible costs. See [GitHub cache rules](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).

缓存不可变且按 ref 隔离；不同 tag 不互读，tag 可读取默认分支缓存。改动进入默认
分支后，可选择该分支、`cache_only=true` 与 `cache_probe=normal` 手动预热生产缓存；
隔离 cold/warm 探针的命名空间不用于预热普通生产缓存。过期、淘汰、checkout mtime 及
传输/保存仍有成本，不能从本机 dev 数据推断云端收益。

## Diagnostic data / 诊断数据

Per-target diagnostics contain eight-field allowlisted cache metadata, the
validated probe plan when enabled, bounded fingerprint facts when enabled, and
Cargo HTML. Raw Cargo stdout/stderr are never diagnostic artifacts.
The read-only report job requests all jobs for the current run attempt, preserves
failure/skip/cancel states and unknown/zero-date timing, excludes itself and emits
the summary plus a separate timing artifact. Do not sum parallel job durations as
workflow elapsed time. Do not export full environment, commands, logs or secrets.
Product downloads select only product artifact names, excluding diagnostics.

各 target 诊断保存八字段白名单缓存元数据、启用时的探针计划与有界 fingerprint
事实，以及 Cargo HTML，不保存原始 Cargo stdout/stderr。只读报告读取当前 attempt 的全部
job，保留失败/跳过/取消及未知/零日期，不计自身，并导出 summary 与独立时间 artifact。
并行 job 耗时不能相加当总耗时；禁止导出全量环境、命令、日志或秘密。产品下载
只选择产品 artifact 名称，诊断不得混入发布资产。

Opt-in fingerprint mode adds only the fixed
`CARGO_LOG=cargo::core::compiler::fingerprint=info` setting to the existing build
environment. Console output is constructed from fixed status labels, allowlisted
workspace package names and Cargo's numeric exit status. Both raw output streams,
including compiler error prose, paths, arbitrary logger text and continuation
payloads, are suppressed rather than truncated and replayed. This is an explicit
diagnostic-mode tradeoff: use a normal run with diagnostics disabled for full
compiler errors. Normal/default runs keep their existing direct Cargo channel.

显式 fingerprint 模式仅向原构建环境添加固定 `CARGO_LOG`。控制台只生成固定状态、
白名单 workspace 包名与 Cargo 数字退出码；两路原始输出，包括编译错误正文、路径、
任意 logger 文案和续行载荷，均不回放。此为诊断模式的明确取舍；需要完整编译错误时
使用关闭诊断的 normal 运行，默认路径仍直接输出 Cargo。

The JSON projection contains fixed package/reason enums, mtime ordering, bounded
counts and truncation/collector indicators. Limit each line to 16 KiB, examined
trace to 8 MiB, records to 128 and individual counts to 1,000,000. Unknown package
names/formats aggregate or remain unattributed; no raw fallback is permitted.
Parser/export failures must never rerun Cargo or convert a failing build to
success; preserve the child status, with conventional signal-to-exit mapping.

JSON 只含固定包名/原因枚举、mtime 大小关系、有界计数和截断/采集状态。单行最多
16 KiB、检查 trace 最多 8 MiB、最多 128 条记录、单项计数最多 1,000,000。
未知包名/格式聚合或无法归因，不得回退导出原文。解析/导出失败不能重跑 Cargo 或
吞掉构建失败；保留子进程状态，信号按常规退出码映射。

If a cancelled job/step has an API completion timestamp earlier than its start,
retain both timestamps and its cancelled status, set `duration_seconds=null`,
and emit `timing_anomaly=cancelled_end_precedes_start`. Do not invent a negative
duration or discard every other timing. A reversed interval for a non-cancelled
record remains an error; unavailable timing stays unknown.

取消的 job/step 若 API 完成时间早于开始，保留原时间与取消状态，耗时记 `null`，
并标记 `cancelled_end_precedes_start`；不能生成负耗时或丢弃其他记录。非取消记录的
反向时间仍报错，缺失耗时保持未知。

## Iteration and acceptance / 迭代与验收

Inspect the workflow, all three helpers and their focused test files together.
Run local workflow/behavior/metrics fixtures with:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_*release*.py'
```

Also lint the actual workflow with actionlint and compare baseline/candidate
positive/negative policy cases. Cover normal tag/branch dispatch, default-branch
prewarm, same-name tag/case-variant rejection, diagnostic failures, legitimate
production failures, strict/coarse cache priority and explicit namespace misses.
Real Bash guard fixtures must treat shell-looking ref/default-branch values as
data. Cover wrong probe cohort/source/configuration/Rust/target, either cache
missing or failed, cold restore isolation, normal-mode failure tolerance, raw
secret markers in both child streams, logger format changes, truncation, compiler
failure/export failure and cancelled reverse timestamps. Record actual failures
and unexecuted checks without replacing historical evidence.

修改时配套核对 workflow、三个 helper 及专项测试，运行上述夹具并对真实 workflow
执行 actionlint；比较基线/候选的正常/负例策略。覆盖正常 tag/分支手动运行、默认
分支预热、同名 tag/大小写变体拒绝、诊断失败和正常生产失败、严格/粗层优先级及
显式 namespace 不匹配。真实 Bash 夹具必须验证 shell-looking ref/分支值只作数据。
还须覆盖错误 cohort/SHA/配置/Rust/target、任一缓存缺失/失败、cold 恢复隔离、
normal 容错、两路子进程合成秘密、logger 格式变化、截断、编译/导出失败及取消反向
时间。保留真实失败与未执行项，不覆盖历史记录。

These local checks model a finite policy and test fixed shell/data boundaries;
they are not an Actions runner, cache service or Cargo freshness engine. The
[quota incident](../archive/maintenance/release-artifact-quota-20261016.md) confirms
that the observed macOS jobs compiled and packaged before product uploads failed.
The retention change is locally validated; its hosted rerun and cold/warm A/B
remain unexecuted. Future hosted measurements must record
toolchain/target/profile/source, cache match and transfer/save/build/packaging
costs before making a speed claim.

本地检查是有限策略模型与固定 shell/数据边界验证，不能充当 Actions runner、缓存
服务或 Cargo freshness 引擎。[配额事件](../archive/maintenance/release-artifact-quota-20261016.md)
已确认所观察的 macOS job 编译、打包成功后上传失败；保留期修复只有本地验证，
该候选云端重跑及冷/暖 A/B 未执行。后续云端测速须记录工具链/target/profile/源码、缓存
匹配与传输/保存/编译/打包成本，再作速度结论。

The [fingerprint follow-up](../archive/maintenance/release-ci-fingerprint-20261016.md)
records 134 passing local tests, 13 rejected workflow mutations and a six-stage
independent Cargo fixture. That fixture reproduces same-SHA/same-byte checkout
mtime invalidation after restoring preserved cache files; it does not establish
the historical hosted run's thirteen-package cause or a production speedup.

[fingerprint 后续记录](../archive/maintenance/release-ci-fingerprint-20261016.md)
保存 134 项本地通过、13 个破坏变体被拒绝及独立 Cargo 六阶段夹具。夹具复现同
SHA/同字节新 checkout 恢复缓存后的 mtime 失效机制，不能据此断言历史云端十三个包
的实际重编原因或生产提速。
