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
| `scripts/tests/test_release_workflow.py`, `test_release_workflow_behavior.py`, `test_ci_release_metrics.py` | Workflow contracts, finite local policy cases and export fixtures / 工作流契约、有限本地策略模型及导出夹具 |

The operational entry is [Production CI in the build guide](../build-performance.md#production-ci-cache-and-timings--生产-ci-缓存与计时).
Current local evidence and unexecuted checks belong to the
[validation record（归档：docs/validation/release-ci-reliability-20261016/README.md）](../local-archives.md#validation-experiments-20261016).

操作入口见[构建指南](../build-performance.md#production-ci-cache-and-timings--生产-ci-缓存与计时)，
本地证据和未执行项见[验证记录（归档：docs/validation/release-ci-reliability-20261016/README.md）](../local-archives.md#validation-experiments-20261016)。

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

| Failure / 失败 | Required behavior / 必须行为 |
| --- | --- |
| Diagnostic Python setup, cache metadata or diagnostic upload / 诊断 Python、元数据或诊断上传 | Best-effort; visible failure must not block otherwise successful production / 尽力执行，错误可见且不阻断正常生产 |
| Final timing-report job / 最终计时 job | Job-level best-effort; failed collection stops summary generation, never fabricates success / job 级容错，采集失败不伪造成功报告 |
| Build, packaging, enabled signing/notarization, product upload or publication / 构建、打包、已启用签名/公证、产品上传或发布 | Remain blocking; diagnostics tolerance must not make them green / 保持阻断，不能被诊断容错改成成功 |
| Invalid manual prewarm ref / 非法预热 ref | Fail before checkout/restore/build/save; reporting cannot erase that failure / checkout/恢复/构建/保存前失败，报告不能抹掉失败 |
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
default branch, manually select that branch and `cache_only=true` to seed reusable
build caches. Expiration, eviction, checkout mtimes and transfer/save overhead
remain possible costs. See [GitHub cache rules](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).

缓存不可变且按 ref 隔离；不同 tag 不互读，tag 可读取默认分支缓存。改动进入默认
分支后，可选择该分支与 `cache_only=true` 手动预热。过期、淘汰、checkout mtime 及
传输/保存仍有成本，不能从本机 dev 数据推断云端收益。

## Diagnostic data / 诊断数据

Per-target diagnostics contain only allowlisted cache fields and Cargo HTML.
The read-only report job requests all jobs for the current run attempt, preserves
failure/skip/cancel states and unknown/zero-date timing, excludes itself and emits
the summary plus a separate timing artifact. Do not sum parallel job durations as
workflow elapsed time. Do not export full environment, commands, logs or secrets.
Product downloads select only product artifact names, excluding diagnostics.

各 target 诊断只保存白名单缓存字段与 Cargo HTML；只读报告读取当前 attempt 的全部
job，保留失败/跳过/取消及未知/零日期，不计自身，并导出 summary 与独立时间 artifact。
并行 job 耗时不能相加当总耗时；禁止导出全量环境、命令、日志或秘密。产品下载
只选择产品 artifact 名称，诊断不得混入发布资产。

## Iteration and acceptance / 迭代与验收

Inspect the workflow, metrics helper and all three focused test files together.
Run local workflow/behavior/metrics fixtures with:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_*release*.py'
```

Also lint the actual workflow with actionlint and compare baseline/candidate
positive/negative policy cases. Cover normal tag/branch dispatch, default-branch
prewarm, same-name tag/case-variant rejection, diagnostic failures, legitimate
production failures, strict/coarse cache priority and explicit namespace misses.
Real Bash guard fixtures must treat shell-looking ref/default-branch values as
data. Record actual failures and unexecuted checks without replacing historical
evidence.

修改时配套核对 workflow、helper 和三个专项测试文件，运行上述夹具并对真实 workflow
执行 actionlint；比较基线/候选的正常/负例策略。覆盖正常 tag/分支手动运行、默认
分支预热、同名 tag/大小写变体拒绝、诊断失败和正常生产失败、严格/粗层优先级及
显式 namespace 不匹配。真实 Bash 夹具必须验证 shell-looking ref/分支值只作数据。
保留真实失败与未执行项，不覆盖历史记录。

These local checks model a finite policy and test fixed shell/data boundaries;
they are not an Actions runner, cache service or Cargo freshness engine. Current
local fixes are validated in the linked record; hosted Windows/macOS execution
and cold/warm A/B remain unexecuted. Future hosted measurements must record
toolchain/target/profile/source, cache match and transfer/save/build/packaging
costs before making a speed claim.

本地检查是有限策略模型与固定 shell/数据边界验证，不能充当 Actions runner、缓存
服务或 Cargo freshness 引擎。当前修复已有链接记录中的本地证据；Windows/macOS
云端执行及冷/暖 A/B 仍未执行。后续云端测速须记录工具链/target/profile/源码、缓存
匹配与传输/保存/编译/打包成本，再作速度结论。
