# Release CI fingerprint diagnostics / 生产构建重编归因（2026-10-16）

Status: adopt the locally validated diagnostic tooling; hosted deployment and
execution remain unperformed. This batch changes observability and controlled
cache experiments, not Release optimization parameters or product behavior.

状态：采用已通过本地验证的诊断工具，尚未部署或执行云端候选。本批补充可观测性与
受控缓存实验，不调整 Release 优化参数或产品行为。

The [successful-run audit](release-ci-success-audit-20261016.md) observed exact
macOS cache restoration followed by workspace recompilation. Those jobs did not
collect fingerprint reasons. This follow-up provides bounded diagnostics and a
reproducible local mechanism; it cannot retrospectively identify the cause of all
thirteen historical path-package rebuilds or claim a CI speedup.

[成功运行审计](release-ci-success-audit-20261016.md)观察到 macOS 精确恢复缓存后仍重编
workspace，但当时未采集 fingerprint 原因。本批提供有界诊断与可复现的本地机制，
不能追认历史十三个 path 包的全部重编实因，也不宣称 CI 提速。

## Accepted changes / 采用的改动

- `.github/workflows/release.yml` adds optional manual `cache_diagnostics=false`,
  `cache_probe=normal|cold|warm` (default normal) and empty `probe_id` inputs.
  Default normal mode invokes Cargo directly as before. Explicit diagnostics or
  a cold/warm probe select the fixed fingerprint wrapper.
- `scripts/ci_release_build.py` runs the same Release/target/jobs/timings/locked
  command, adding only the fixed fingerprint logger. Both raw child streams are
  suppressed. Console output consists of fixed status labels, allowlisted package
  names and numeric Cargo exit status. The JSON contains package/reason enums,
  mtime ordering, bounded counts and truncation/collector flags; it never exports
  raw paths, arbitrary log payloads or compiler prose. Limits are 16 KiB per line,
  8 MiB examined trace, 128 records and 1,000,000 per counter.
- `scripts/ci_release_probe.py` validates default-branch cache-only probes, creates
  isolated cohort keys and rejects warm samples unless both registry and compiler
  caches restore successfully with exact hits. It runs neither Cargo nor network
  operations itself.
- `scripts/ci_release_metrics.py` preserves cancelled job/step API timestamps when
  the end precedes the start, with `duration_seconds=null` and
  `timing_anomaly=cancelled_end_precedes_start`. Other timings survive. Reversed
  non-cancelled intervals remain errors; missing timing stays unknown.

- 工作流新增可选手动输入：默认关闭诊断、默认 normal 探针、空 cohort ID；默认
  normal 仍直接调用 Cargo。显式诊断或冷/热探针启用固定 wrapper。
- 构建 wrapper 保持原 Release/target/jobs/timings/locked 命令，仅添加固定 logger。
  两路原始输出不回放；控制台只生成固定状态、白名单包名与退出码，JSON 只导出枚举、
  mtime 关系及有界计数/状态。限额为单行 16 KiB、trace 8 MiB、128 条记录、单计数
  1,000,000；不导出原始路径、任意日志或编译错误正文。
- 探针 helper 仅验证身份、隔离 key 与双缓存精确命中，不自行执行 Cargo/网络。
- 计时 helper 遇到取消记录的 API 反向时间时保留原状态/时间、耗时记 null 并标记
  固定 anomaly，不再丢弃全部计时；非取消的反向时间仍失败，缺失时间保持未知。

Suppressing compiler prose is an explicit opt-in tradeoff. To obtain full compiler
errors, use normal mode with diagnostics disabled. Unknown logger formats may
lose attribution or aggregate as `other`; they never become raw-output fallbacks.
Parser/export failures do not rerun Cargo or mask its failure; the child status
remains blocking, with conventional signal exit mapping. Ordinary diagnostic
planning/export stays best-effort; cold/warm planning and exact-hit verification
are strict acceptance gates.

丢弃编译错误正文是显式诊断模式的取舍；需要完整错误时关闭诊断、运行 normal。
未知 logger 格式可失去归因或归入 `other`，不得回退输出原文。解析/导出失败不能
重跑 Cargo 或吞失败，保留子进程状态及常规信号退出映射。正常诊断规划/导出尽力
执行，冷/热探针规划与双缓存 exact 命中则严格验收。

Release opt-z/full LTO/CGU1/incremental-off, default features, the three-target
matrix, normal strict/coarse cache policy, product/signing/publication failures,
permissions and automatic triggers are unchanged. Product artifacts retain their
names/files and 7-day retention; diagnostics retain 3 days. No remote artifact,
cache, Release or billing state was modified.

Release 参数、默认 feature、三 target 矩阵、正常严格/粗层缓存、产品/签名/发布阻断、
权限和自动触发均未改变；保留产品 7 天、诊断 3 天与现有产物兼容。本批未修改远端
artifact、cache、Release 或计费。

## Local baseline/candidate evidence / 本地基线与候选证据

The frozen baseline passes its original contract but lacks the new probe and
privacy acceptance requirements. The candidate passes the expanded contract;
normal tag, branch-dispatch, default-branch prewarm and invalid-prewarm policy
controls agree between baseline and candidate. Normal keys/fallbacks/cache paths,
build command, runner/Rust/matrix and product step bodies remain unchanged.

冻结基线通过旧契约，但缺少新探针和隐私验收；候选通过扩展契约。正常 tag、分支
手动运行、默认分支预热及非法预热的前后策略一致，正常缓存身份/路径、构建命令、
runner/Rust/矩阵与产品步骤保持一致。

| Check / 检查 | Observed result / 实际结果 |
| --- | --- |
| `python3 -B -m unittest discover -s scripts/tests -p 'test_*release*.py'` | 134 passed, 0 failed, exit 0; metrics 43, probe 26, workflow contract 44, finite behavior 21 / 134 项通过 |
| Actual-workflow destructive mutations / 真实工作流破坏变体 | All 13 rejected: cold restore enabled, normal namespace leakage, missing/tolerated warm guard, tolerated probe planning, blocking normal planning, missing source identity, raw-log upload, global tracing, missing locked, swallowed build failure, excessive product/diagnostic retention / 全部被拒绝 |
| Privacy and exit controls / 隐私与退出码 | Unsafe passthrough control exposes a synthetic marker; final wrapper exposes none, including both child streams, unknown prologues after progress, fake Cargo-header continuations and compiler prose containing logger words; original failure 101 retained / 旧透传负例泄漏，最终候选不泄漏且保留失败 |
| Cancellation control / 取消对照 | Baseline rejects the entire -1-second report; candidate keeps cancellation with null/anomaly; success/failure/skipped/null-conclusion reversed intervals remain rejected / 基线丢整份报告，候选保留未知耗时与异常 |
| Actual workflow static validation / 工作流静态检查 | actionlint 1.7.12 exit 0 with ShellCheck explicitly disabled; Ruby safe YAML parse exit 0; relevant Bash `-n` checks exit 0; scoped `git diff --check` exit 0 / 均通过，未执行 ShellCheck |

Tests also cover shell-looking ref/default-branch values as data, Main/main and
same-name-tag rejection, wrong cohort/source/configuration/Rust/target identities,
cache misses/fallbacks/failed or skipped restores, subprocess exits and export
failure, truncation and format variation. Local policy fixtures are finite models,
not a hosted Actions runner, cache service or Cargo freshness implementation.

测试另覆盖 shell-looking 输入仅作数据、大小写与同名 tag、错误 cohort/SHA/配置/
Rust/target、未命中/fallback/失败/跳过恢复、真实子进程退出与导出失败、截断和格式
变异；有限模型不替代 Actions runner、缓存服务或 Cargo freshness 引擎。

Frozen source SHA-256 values:

| Source / 源码 | SHA-256 |
| --- | --- |
| `.github/workflows/release.yml` | `366b975602ac69099716ed64991f249cd51e4414165abe6034b166e4b13a481a` |
| `scripts/ci_release_build.py` | `51e649360fc73af596e8c7d9ac807aaf202318a258cdebbddb968bb0ca06169a` |
| `scripts/ci_release_probe.py` | `05f089e46bbf72db23af1043ea06225890e5348e5040ff29b9d4d5034cfb4b00` |
| `scripts/ci_release_metrics.py` | `bd47b516c0521f1600103bde92237183d38774d0af8a6b93e812b0c4153ef50a` |

## Independent Cargo mechanism / 独立 Cargo 机制夹具

A dependency-free two-package fixture used an isolated temporary target and the
same Release optimization policy. It performed a real local Git checkout of the
same commit into the same source path, then restored the cache with its original
file mtimes. The source SHA and bytes match before/after checkout. No source mtime
backdating or project build was used.

无外部依赖的双包夹具使用独立临时 target 与相同 Release 优化策略，在同一路径真实
checkout 同一 commit，再恢复保留原 mtime 的缓存；前后 SHA 与源码字节一致，没有
倒改源码时间，也没有运行主项目构建。

| Phase / 阶段 | Fresh / Dirty artifacts | Evidence / 证据 |
| --- | --- | --- |
| Cold build / 冷构建 | 0 / 2 | No prior fingerprints / 无旧 fingerprint |
| In-place warm / 原地热构建 | 2 / 0 | No dirty reason / 无 dirty 原因 |
| Same SHA/bytes checkout + restored cache / 同 SHA/字节新 checkout + 恢复缓存 | 0 / 2 | `changed_file`, `stale_newer`, then dependency fingerprint / 文件较新及依赖传播 |
| Next in-place warm / 随后原地热构建 | 2 / 0 | Fresh control / fresh 对照 |
| Actual owned-library edit / 真实库源码修改 | 0 / 2 | Changed-file/dependency control / 源码/依赖变化对照 |
| Edited-source warm / 修改后原地热构建 | 2 / 0 | Fresh control / fresh 对照 |

All six Cargo commands exited 0. This proves one cache-invalidating mechanism on
this machine; the historical hosted jobs lack the necessary trace to identify
their precise trigger. It supplies no production build-time percentage.

六次 Cargo 均退出 0。仅证明本机可复现的一种失效机制，历史云端缺少对应 trace，
尚不能确定具体触发链，也不提供生产提速百分比。

## Frozen workspace replay / 冻结 workspace 回放

The subsequent complete local replay used frozen dirty baseline A from the
MainWindow experiment: exactly 980 declared source files at the same source path,
the same external synthetic Git commit before/after checkout, and the same target
path. This is not a checkout of the historical GitHub `18a43c8` source. It retained
the accepted A command/environment: Release, `ui-components`, offline/locked,
incremental off and fingerprint logging. No application source change or Release
parameter change was adopted from this diagnostic sequence.

随后完成的本地完整项目回放使用 MainWindow 实验的冻结脏基线 A：精确 980 个声明
文件、相同源码/target 路径、checkout 前后相同的外置合成 Git commit。它不是历史
GitHub `18a43c8` 源码。命令/环境与已接受的 A 一致，使用 Release、ui-components、
offline/locked、关闭增量及 fingerprint logger；本诊断未采用任何应用源码或生产
参数变更。

The harness first required an entirely fresh hot build. It then cloned the cache
with APFS `cp -cRp`, created all 980 source files afresh using `checkout-index`
without changing their bytes, and restored the cloned cache to the original target
path. All 980 source mtimes changed naturally. The original/snapshot/restored
cache inventories matched content hashes, file modes and mtimes across 6,905
entries containing 2,973,584,937 file bytes. Source registration, checkout, APFS
cloning and full inventory hashing are excluded from the Cargo durations below;
these local clone costs do not measure GitHub cache transfer/extraction.

先要求原地热构建全部 fresh，再以 APFS clone 保存缓存，通过 checkout-index 实际
重建全部 980 个源码文件，保持字节不变；最后把缓存恢复到原 target 路径。980 个
源码 mtime 均自然改变。原缓存/快照/恢复的 6,905 项（文件共 2,973,584,937 B）
content hash、mode、mtime 清单相同。源码登记/checkout、APFS clone 和全量清单
hash 均不计入下表 Cargo 耗时，也不代表 GitHub 缓存传输/解包成本。

| Phase / 阶段 | Cargo seconds / 秒 | Fresh / Dirty units | MainWindow seconds | Root library seconds | Binary seconds |
| --- | ---: | ---: | ---: | ---: | ---: |
| `hot-before` | 0.612159 | 775 / 0 | 0 | 0 | 0 |
| `new-checkout-restored-cache` | 475.784704 | 748 / 27 | 255.16 | 44.14 | 204.92 |
| `hot-after` | 0.925765 | 775 / 0 | 0 | 0 | 0 |
| `real-source-comment-control` | 211.638399 | 774 / 1 | 0 | 0 | 211.20 |
| `source-restore-excluded-build` | 209.931539 | 774 / 1 | 0 | 0 | 209.12 |
| `hot-final` | 0.867313 | 775 / 0 | 0 | 0 | 0 |

All six phases exited 0, with no collector error or truncation. The same-byte
checkout rebuilt 27 units belonging to 14 workspace packages. Fingerprint facts
include direct `changed_file`/`stale_newer` evidence and downstream
`dependency_fingerprint` propagation. The three in-place hot controls rebuilt
nothing. A real `src/main.rs` comment edit rebuilt only the binary; restoring its
original bytes rebuilt that unit once more, explicitly excluded from comparison.
Final source hashes match the starting inventory.

六阶段均退出 0，采集无错误/截断。同字节新 checkout 导致 14 个 workspace 包的
27 个单元重编，fingerprint 包含直接 changed_file/stale_newer 及下游依赖传播。
三次原地热对照均无重编；真实 main.rs 注释修改只重编二进制，恢复原字节后再重编
一次，该恢复样本明确排除比较；最终源码 hash 与起始清单一致。

Cargo HTML attributes MainWindow's 255.16 seconds to 59.21 seconds frontend and
195.95 seconds codegen. The root library takes 44.14 seconds (13.70 frontend,
30.44 codegen) and overlaps MainWindow through pipelining; the binary starts after
MainWindow at 270.76 seconds and takes 204.92 seconds. Do not sum the overlapping
library work as additional wall time. This sequence identifies expensive rebuild
owners locally; it is a single diagnostic sequence, not a production speedup A/B.

HTML 显示 MainWindow 为前端 59.21 秒、codegen 195.95 秒；根库为前端 13.70 秒、
codegen 30.44 秒，利用流水与 MainWindow 重叠。二进制在第 270.76 秒、MainWindow
完成后开始，耗时 204.92 秒；重叠的根库时间不能再加作 wall time。这识别本机重编
的主要 owner，但单次诊断序列不构成生产提速 A/B。

| Frozen evidence / 冻结证据 | Identity / 身份 |
| --- | --- |
| External synthetic commit / 外置合成 commit | `c81c8183c9754030ed92d4a4eaf10081037d75d3` |
| Source manifest SHA-256 / 源码清单 | `257054910137321b9dcfc17600f3b24e3856b48e0fe10219ecfee5339a8e8365` |
| Starting/final source aggregate SHA-256 / 起止源码聚合 | `f3c0b83129f263fc69923928fcd4a059f90e22069c2b71b06109af82dc27c05d` |
| Accepted A build record SHA-256 / A 构建记录 | `c6648e30e6ba344a29f3f739abfb0438eef795cd814138197d6f9825cb31c7c1` |
| Cache inventory digest / 缓存清单摘要 | `7454ba449556ced61a29ee3009451273209c842f597d60d9e794d12e5aad4cf7` |
| Completed `replay.json` SHA-256 / 完整回放记录 | `ce3c51c5278590188a12124ebe8ace71e9a5a6b61c492c25e45e855c00b33ca2` |
| Extracted analysis SHA-256 / 提取分析 | `d22f3fd11fc7e73dc4d6abf298152cb7f5f70605b03afc8a5e171411d8953ac9` |

The completed replay and per-phase fingerprint/HTML originals remain under
`/private/tmp/usher-mainwindow-split-20261016/cache-mtime-replay-1/`; the local
`full-project-replay-analysis.json` below records all per-phase hashes and owner
timing extraction. This proves same-source checkout mtime invalidation in the
frozen full workspace, not the historical hosted thirteen-package cause or
Windows/hosted cache behavior. No mtime backfilling or freshness-bypass workaround
was introduced; genuine source edits must continue to invalidate Cargo caches.

完整回放及逐阶段 fingerprint/HTML 原件保留在私有临时目录，下述本地分析保存各阶段
hash 与 owner 时间提取。结论仅为冻结完整 workspace 可复现的同源 checkout mtime
失效，不追溯证明历史云端十三包的原因或 Windows/托管行为。没有引入 mtime 回填或
绕过 freshness 的修补，真实源码修改仍必须使 Cargo 缓存失效。

## Unexecuted hosted plan / 尚未执行的云端计划

The read-only API observation at 2026-10-16 11:52:43 UTC found default branch
`main` at `18a43c855a10005730e4d8a43a1199ae8580f72a`; the currently visible
`workflow_dispatch` run query returned zero entries. That remote source does not
contain these new inputs/helpers. This is an observation of visible API results,
not a claim about all historical manual activity. No push, dispatch or rerun was
performed for this candidate.

2026-10-16 11:52:43 UTC 的只读 API 观察：默认分支 main 为上述 SHA，当前可见的
workflow_dispatch 查询为零条；远端尚无新输入/helper。这只描述可见 API，不推断
所有历史手动活动。本候选没有 push、调度或重跑。

After review and separately authorized deployment to the default branch:

1. Run a cache-only `cold` probe on that branch with an empty ID. Both restore
   steps must be skipped, with successful builds saved to a new
   `release-probe-v1-<run-id>-<attempt>` cohort and no product actions.
2. Keep SHA/configuration/Rust/runner class fixed, run cache-only `warm` using the
   cold cohort ID. Each of the three targets must have both registry/build
   `cache-hit=true` and `outcome=success` before Cargo; no fallback or silent cold
   substitution is valid. Changed identity or cache eviction invalidates the pair.
3. Collect cache/probe/fingerprint projections, Cargo HTML and job/step timing for
   all targets. Report actual exact/fallback/miss, dirty units and transfer/save/
   compile costs; diagnostic availability is best-effort.
4. Separately run default-branch `cache_only=true, cache_probe=normal`, then repeat
   at the same SHA to measure normal Windows warm restoration. Isolated probe
   caches do not seed the normal namespace.
5. Inspect the next separately authorized legitimate `v*` release to test
   default-branch cache visibility from a different tag. Do not create a diagnostic
   release tag; a same-tag rerun does not prove cross-tag reuse.

审核并另行授权部署到默认分支后：先 cold，保持身份再 warm，逐 target 验证双缓存
精确成功；保存白名单投影、HTML 与阶段时间。正常预热/Windows 热恢复另走 normal
namespace；跨 tag 只观察后续获授权的真实发布，不为诊断创建 tag。同 tag 重跑不能
证明默认分支预热。云端收益、Windows 正常热恢复与完整历史归因仍未验证。

## Evidence location / 证据位置

The machine-readable records are local, ignored validation artifacts under
`docs/validation/release-ci-fingerprint-20261016/`: `tooling-ab.json`,
`workflow-validation.json`, `mtime-fixtures.json`, `remote-state.json`,
`cloud-plan.json`, `full-project-replay-analysis.json` and frozen baseline files.
They contain source hashes, commands,
exit codes and bounded facts; raw fixture streams remain private under
`/private/tmp/usher-ci-fingerprint-20261016/`. Tracked tests use their own temporary
or checked-in fixtures and do not depend on this dated directory.

机器记录保存在本机被忽略的 validation 目录，含源码 hash、命令、退出码与有界事实；
原始夹具流留在私有临时目录。受跟踪测试不依赖本次日期目录。稳定契约见
[Release CI](../../specs/release-ci.md)，操作入口见
[生产 CI 构建指南](../../build-performance.md#production-ci-cache-and-timings--生产-ci-缓存与计时)。
