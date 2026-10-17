# MainWindow PR Review compilation boundary / 编译边界实验（2026-10-16）

Status: **B3 is rejected as the next accepted baseline: formal default-Release P6
failed both the strict RSS and relative final-footprint gates.** All ten warm
dev/Release `ui-components` compile groups are complete and audited. The local
default-feature Release executables and size are validated. B3 remains only as a
reviewable workspace candidate and local evidence; the original A is preserved.
Scoped native checks completed with the explicit limits recorded below. The
completed UI-edit groups establish compile-time improvements for their fixed
profile/feature graphs; the controls below also record the Rust-only cost. They
establish no runtime, memory, default production-artifact or hosted-CI improvement.

状态：**正式默认 Release P6 的严格 RSS 与相对 final-footprint 门禁均失败，B3 最终拒绝
进入 accepted baseline。** 固定 ui-components 图的 dev/Release 十组编译矩阵全部完成并
审计通过，本机默认 feature Release 可执行文件身份与体积已核对。B3 仅保留为可审阅的
工作区候选和本机证据，原 A 保持完整；已完成
列明范围的原生检查，具体缺测项见下文。已完成 UI 编辑组只证明各自固定 profile/feature
图的编译收益；下文另记录 Rust-only 成本，不代表运行时、内存、默认生产制品或云端 CI 改善。
编译收益和制品身份核对不能覆盖已失败的运行时门禁。

## Problem and responsibility / 问题与职责

The complete PR Review visual graph was generated inside `usher-ui`'s MainWindow.
An edit to either the shell or PR page therefore invalidated that large generated
Rust unit. The successful hosted audit also identified MainWindow and the final
binary as expensive compiler units; it did not establish that splitting them
would improve every build path.

完整 PR Review 视觉图原先随 MainWindow 一起生成，主壳或 PR 页面改动都会使较大的
生成 Rust 单元失效。既有云端审计显示 MainWindow 与最终二进制编译昂贵，但不能
据此预先断言拆分后所有构建路径都会更快。

The candidate moves the cohesive PR Review route—Conversation, Commits, Files
changed, Diff/Find/selection, Conflicts and its dialogs—into
`crates/usher-ui-pr-review`. Rust binds the actual factory globals directly via
`PrReviewUiExt::pr_review()`. It does not mirror PR row models or add property-by-
property MainWindow callback proxies. Controller presentation/review command
owners receive explicit size/dependency gates. Provider mutations, project/file
generations, durable state and request-time Agent identity capture retain their
existing responsibilities.

候选把完整 PR Review 路由与所属弹窗移到独立 UI 包；Rust 取得实际 factory global
直接绑定，不复制行 model，也不逐属性添加主壳代理。控制器的展示与评审命令各有
内聚 owner 和规模/依赖门禁。Provider mutation、项目/文件代次、持久状态与入队前
Agent 身份捕获保持原职责。

The static factory shares MainWindow's actual WindowAdapter and restores the
shell item tree immediately after construction, including error results. The
factory root is retained; its active Surface condition destroys the heavy
document on route exit while retaining the same globals/models for reentry.
Scalar host context covers geometry/chrome/modal summaries. MainWindow continues
to own navigation, native chrome, AI panel, status surfaces and window geometry.
Thirty fixed Tooltip actions have independent literal I18n leaves in one local
portal; runtime paths/timestamps keep the documented generic metadata exception.

静态 factory 共用主壳真实 WindowAdapter，构造后（包括错误）立即恢复 shell item tree。
保留轻量 root，Surface 根据 active 销毁/重建重文档，重入继续使用原 global/model。
小型 host context 只交换几何/chrome/modal 摘要；主壳继续拥有导航、原生 chrome、
AI 面板、状态栏和窗口几何。30 个固定 Tooltip 动作使用独立 literal/I18n 叶子和
单个本地 portal；动态路径/时间继续使用既有 metadata 例外。

Default/static builds compile the independent page once. `live-preview` keeps
the integrated MainWindow page and skips separate static generation; `dev-shell`
selects the protocol-compatible empty page in both owners. Renderer forwarding,
complete language bundles and immediate locale projection remain required.
See the [PR Review UI contract](../../specs/pr-review-ui.md) for the inspected
code paths, ownership and acceptance matrix.

默认/静态模式独立生成一次完整页面；live-preview 继续集成解释图并跳过新包静态
生成，dev-shell 两侧选择相同空页协议。渲染器转发、完整语言包及即时 locale 投影
继续受约束，具体入口与矩阵见 [PR Review UI 规格](../../specs/pr-review-ui.md)。

## Frozen comparison inputs / 冻结对照输入

A is the frozen dirty working source based on GitHub HEAD `18a43c8`, not a clean
copy of that commit. It includes the user's existing macOS change. B3 adds only
the reviewed PR split overlay to that A snapshot. Both arms retain A's frozen
CI/documents; subsequent CI/document edits are excluded. Existing lock packages and Cargo
configuration are unchanged apart from the new owner and feature forwarding.

A 为基于该 HEAD 的冻结脏工作区，不是该 commit 的纯净副本，包含用户已有 macOS
改动。B3 仅叠加本轮审核后的拆分；两侧沿用 A 已冻结的 CI/文档，不混入后续改动。既有 lock 包与
Cargo 配置保持，仅新增 owner/feature 转发。

| Input / 输入 | Frozen identity / 冻结身份 |
| --- | --- |
| A source manifest, 980 files / A 清单 | `257054910137321b9dcfc17600f3b24e3856b48e0fe10219ecfee5339a8e8365` |
| A source aggregate / A 源码聚合 | `f3c0b83129f263fc69923928fcd4a059f90e22069c2b71b06109af82dc27c05d` |
| B3 source manifest, 1,000 files / B3 清单 | `8053d90bd697d0ba960bc0d62d21b759738dbd9eaa5d02f55e91e37fad1ee2b8` |
| B3 source aggregate / B3 源码聚合 | `80ab92adedab2c550f35cdf446c56c7eb9396ab89f4b3153d788d7801093b7ad` |
| Preserved user `src/platform/macos.rs`, both arms / 两侧用户改动 | `ef28417899936c7e877414aea0104cf6b94092f2b0db580206190a776180609b` |

B3 adds 20 files, modifies 40 and removes none: net filesystem source bytes
increase by 199,006. This is neither patch edit-distance nor generated-code,
binary-size or RSS evidence. The new generated UI owner inherits dev opt1/no-debug
and test no-debug policy. **All Release settings remain unchanged:** strip=true,
opt-level=z, full LTO, CGU1, panic=abort, debug=false and incremental=false.
No existing optimization budget is relaxed to admit the candidate.

B3 新增 20、修改 40、删除 0 个文件，源文件字节净增 199,006；不是 patch 编辑量、
生成代码、二进制体积或 RSS 指标。新生成 UI owner 延续 dev opt1/无 debug 和 test
无 debug。**Release 全参数保持不变**，没有为通过候选放宽既有预算。

B1 and B2 were superseded before any Cargo invocation in those frozen source
trees and have no accepted performance result. Their original bytes/manifests
are retained as `source-B-pre-modal`/`source-candidate-v1.json` and
`source-B-pre-route`/`source-candidate-v2.json`. B1 precedes the modal shortcut and
Copied placement fixes; B2 precedes final route Tooltip cleanup and fixture
corrections. Neither is adopted or silently promoted into the baseline.

B1/B2 的冻结树均在执行 Cargo 前被替换，无可采纳性能结果；原字节/清单保留。
B1 缺 modal 快捷键及 Copied 定位修复，B2 缺最终路由 Tooltip 清理/夹具修正，均未
采用，也不作为后续基线。

## Completed dev/page build group / 已完成 dev/页面构建组

The frozen three-pair dev group changed the real PR page's
`file-list-min-width: 220px` to 221/222/223 px, with identical fixture values in
each A/B pair and AB/BA/AB order. It retained `--features ui-components`, locked/
offline inputs, source inventories and normalized environment. Initialization,
settle/fresh confirmations and restore builds were excluded. This semantic edit
substitutes for the Slint comment group in this compilation-boundary batch;
comment-edit speed remains unmeasured.

冻结 dev 组三对实改 PR 页的最小文件栏宽，依次为 221/222/223 px；两侧语义值相同，
顺序 AB/BA/AB。保持 ui-components、locked/offline、完整源码清单和归一环境；排除
初始化、settle/fresh 与恢复构建。本编译边界批次以真实语义编辑替代 Slint 注释组，
没有测量注释编辑提速。

| Pair / 对 | A seconds / 秒 | B3 seconds / 秒 |
| --- | ---: | ---: |
| 1 (AB) | 177.844069 | 65.388647 |
| 2 (BA) | 175.560721 | 59.289826 |
| 3 (AB) | 179.697286 | 64.587991 |
| Median / 中位数 | **177.844069** | **64.587991** |

The relevant wall-time median improves **63.6828%**, passing this group's 10%
compile threshold. A rebuilds `usher-ui`; B3 keeps MainWindow's library and
build-script execution fresh and rebuilds `usher-ui-pr-review`. Its actual rustc
and script-launch evidence agrees with Cargo freshness. Both final restoration
confirmations are fresh, with no runtime rustc or UI build-script execution.
The group audit passes with `adopt=false`; this is no automatic overall adoption.

相关 wall time 中位数改善 **63.6828%**，本组通过 10% 编译门槛。A 重编 MainWindow，
B3 保持其库和 build-script 执行 fresh，仅重编 PR UI；实际 rustc/脚本启动与 Cargo
freshness 相符。两侧恢复确认均 fresh，无运行时 rustc/UI 脚本执行。审计保留
`adopt=false`，本组结果不自动代表整体采用。

| Median unit cost / 单元中位耗时（秒） | A | B3 |
| --- | ---: | ---: |
| MainWindow build-script execution / 主壳生成 | 3.22 | 0 (fresh) |
| MainWindow Rust library / 主壳 Rust 库 | 171.25 | 0 (fresh) |
| Independent PR build-script execution / PR 生成 | Absent / 无 | 1.92 |
| Independent PR Rust library / PR Rust 库 | Absent / 无 | 59.85 |
| Root library / 根库 | 5.19 | 4.68 |
| Binary / 二进制 | 2.05 | 1.93 |

MainWindow frontend/codegen medians are 26.05/146.73 seconds in A; independent PR
frontend/codegen medians are 7.43/50.48 seconds in B3. These are independently
rounded medians, and pipelined root work overlaps UI compilation; do not add them
to reconstruct wall time.

A 主壳前端/codegen 中位数为 26.05/146.73 秒；B3 PR 为 7.43/50.48 秒。分项中位数
独立取值且经舍入，根库流水与 UI 编译重叠，不能简单相加重建 wall time。

| Actual generated Rust / 实际生成 Rust | Bytes / 字节 | Lines / 行 |
| --- | ---: | ---: |
| A MainWindow | 38,297,156 | 392,798 |
| B3 MainWindow | 26,527,297 | 275,799 |
| B3 PR factory | 15,033,486 | 153,245 |
| B3 MainWindow + PR | 41,560,783 | 429,044 |
| Total change / 合计变化 | **+3,263,627** | **+36,246** |

MainWindow alone shrinks, while MainWindow plus PR generated code grows. These
are generated-source measurements, not binary/target/RSS savings. Settings,
playground and Messages generated files retain equal path-normalized hashes
between arms. The complete audit is
`/private/tmp/usher-mainwindow-split-20261016/prsplit3-dev-page-complete-audit.json`,
SHA-256 `e8ffd9b4493b67a4d4b0145006374f698ec228c65ca643cb605079ed8f2cad11`;
the frozen recorder/orchestrator did not change during collection.

主壳单独缩小，但加上 PR 的生成代码总量增加；不能据此宣称二进制、target 或 RSS
减少。Settings、演练场和 Messages 两侧生成文件的路径归一 hash 一致。完整审计与
原始日志留在本机私有目录，采集期间冻结记录器/编排器未变。

## Final dev shell and control results / dev 主壳及对照最终结果

The four other dev groups completed their original B3 plan, restoration and fresh
confirmations before their complete audits were frozen. This closes the original
batch after the P6 failure; it does not promote B3 into a new baseline.

Direct `ui/main.slint` semantic edits use the same 544 → 545/546/547 px fixture
and three AB/BA/AB pairs as the separately measured Release shell group:

| Pair/order | Baseline A (s) | Candidate B3 (s) |
| --- | ---: | ---: |
| 1 / AB | 174.408550 | 115.089751 |
| 2 / BA | 168.507501 | 109.497754 |
| 3 / AB | 170.966685 | 113.074052 |
| Median | **170.966685** | **113.074052** |

The dev shell median improves **33.861938%**, passing this related-edit group's
10% compile threshold. MainWindow recompiles in both arms; PR in B, Settings,
Components and Messages remain fresh. MainWindow build-script/Rust medians are
3.08/165.14 seconds in A and 2.53/107.80 seconds in B; frontend/codegen medians
are 23.15/143.78 → 14.29/93.51 seconds. Root-library/binary medians are
4.73/1.85 → 4.49/1.78 seconds. Unit medians overlap and must not be added.

| Pair/order | Rust-only A (s) | Rust-only B3 (s) | Unchanged A (s) | Unchanged B3 (s) |
| --- | ---: | ---: | ---: | ---: |
| 1 / AB | 1.882386 | 1.720633 | 0.462250 | 0.464682 |
| 2 / BA | 1.614211 | 1.581900 | 0.467656 | 0.467830 |
| 3 / AB | 1.738656 | 1.658398 | 0.463772 | 0.459338 |
| Median | **1.738656** | **1.658398** | **0.463772** | **0.464682** |

The Rust-only comment fixture launches only the final binary unit; its median
observation is −0.080258 seconds / −4.616103%, with root library and every UI
unit fresh. It is a control, not a separate optimization acceptance. The unchanged
group launches no actual commands and has zero nonzero-duration timing units;
its median difference is only +0.000910 seconds, with no acceleration claim.
Both controls leave `related_compile_threshold_pass` null. The separate Release
Rust-only **+2.759098%** cost remains recorded below.

The predefined dev shared-input positive control completed one AB pair,
**179.106307 → 124.317071 seconds**, with all four real UI consumers recompiling
in A and all five, including PR, in B. This is an input-invalidation control,
not a three-pair performance acceptance result.

Each three-pair group has 16 valid records: six included samples and ten excluded
settle/confirm/restore controls. The shared control has 12 valid records, two
included samples and ten excluded controls. All 60 records have matching
structured warning/stderr identity multisets against their group reference,
2,701 compiler warning messages with 274 distinct identities, no foreign paths
and normalized environment hash
`69c5239e70b0da5d3c982e0756fabbafe685b903d3d5cef6f48baa17b7f49fc6`.
Actual codegen follows the frozen owner policies. Every source/generated hash
returns to its frozen reference; all eight restore confirmations launch zero
actual commands and record zero timing work:

| Group | A fresh confirmation (s) | B fresh confirmation (s) |
| --- | ---: | ---: |
| Shell | 0.721103 | 0.490465 |
| Rust-only | 0.484384 | 0.473147 |
| Unchanged | 0.461860 | 0.461490 |
| Shared input | 0.965497 | 0.665851 |

Frozen complete audits and their original ledgers:

- [Dev shell audit](/private/tmp/usher-mainwindow-split-20261016/prsplit3-dev-shell_semantic-complete-audit.json),
  SHA-256 `29a3674ee11a929b69b7d5e0855f6ea7b59e1e8ea566112fc2c9e66be2de51a4`;
  [ledger](../../validation/mainwindow-build-split-20261016/prsplit3-dev-shell_semantic-ledger.json),
  SHA-256 `eca69654d30fbb85b2e2b3cc60c119be6f63ad9872da29b31f0034698e238dfc`.
- [Dev Rust-only audit](/private/tmp/usher-mainwindow-split-20261016/prsplit3-dev-rust_comment-complete-audit.json),
  SHA-256 `d0c0b111ff43eab2cfff7f920c7bc320e9ab6e162d3af8fa25157321afcda6f8`;
  [ledger](../../validation/mainwindow-build-split-20261016/prsplit3-dev-rust_comment-ledger.json),
  SHA-256 `2a2fa1019000e288f119bd2e2385906cea4c6963e7490e7a6472d0992e57ba36`.
- [Dev unchanged audit](/private/tmp/usher-mainwindow-split-20261016/prsplit3-dev-unchanged-complete-audit.json),
  SHA-256 `7277d4486a4185de2b349253fb5098dcac193042241a55e454224b488f15eb25`;
  [ledger](../../validation/mainwindow-build-split-20261016/prsplit3-dev-unchanged-ledger.json),
  SHA-256 `9b4ab45da5c43574f7795a00fcd12ca83078c385b688191de297eb2c7fa22155`.
- [Dev shared-input audit](/private/tmp/usher-mainwindow-split-20261016/prsplit3-dev-sharedcontrol-complete-audit.json),
  SHA-256 `198d7d94d97b978eb911679e3412c7ce4fe546bb4851bc63432dc85aeb9a8131`;
  [ledger](../../validation/mainwindow-build-split-20261016/prsplit3-dev-sharedcontrol-ledger.json),
  SHA-256 `13c68586362522f43b3bbaba6ba7a675f858d509c6209bf5c54fd0a2fe401ac6`.

其余四个 dev 组已按同批原计划完成并审计：主壳三对中位数
170.966685 → 113.074052 秒，改善 33.861938%，通过相关编辑编译门槛；PR 包和其他
独立 UI 保持 fresh。Rust-only 及无改动组分别观察到 1.738656 → 1.658398 秒、
0.463772 → 0.464682 秒，仅作控制，不宣称单独提速验收。共享输入一对 AB 为
179.106307 → 124.317071 秒，真实 UI 消费者仍按预期从四个增为五个并全部重编。
60 条记录、警告身份、参数、环境、路径及源码/生成产物恢复核对通过，八次恢复后
fresh 确认均为零工作。十组编译矩阵的完成不改变后文 P6 失败与 B3 最终拒绝结论。

## Completed regression / 已完成回归

These are actual log outcomes, not predicted results. Check-mode timings are
verification costs, not controlled A/B speed measurements.

以下为实际日志结果；check 耗时只记录验证成本，不作受控性能对照。

| Check / 检查 | Result / 结果 | Local evidence / 本机证据 |
| --- | --- | --- |
| Full root library, loopback allowed / 根库全测 | **966 passed, 0 failed, 9 ignored**, exit 0 | `full-lib-tests-4.log` |
| Seven integration binaries / 七组集成 | **48 passed, 0 failed**, exit 0: AI response 27, Preview resize 2, PR list 1, core journeys 5, factory 8, header 3, status bar 2 | `pr-integration-regression-2.log` |
| Latest actual factory suite / 最新 factory 专项 | **8 passed, 0 failed**, exit 0 | `factory-final-2.log` |
| Default `cargo check --locked --offline` | exit 0; Cargo reports 24.52 s / 通过 | `default-check.log` |
| Root doctests / 根文档测试 | **1 passed, 0 failed, 1 ignored**, exit 0 | `doctests.log` |
| `dev-shell,ui-components` check | exit 0; Cargo reports **51.50 s** | `dev-shell-check.log` |
| `live-preview,ui-components` check | exit 0; Cargo reports **89 s** | `live-preview-check.log` |
| Formatting, architecture and UI policy / 格式与结构/UI 门禁 | Passed / 通过 | Formatting run plus `architecture-gate-final-pre-ab.log`, `ui-policy-gate-final-pre-ab.log` |
| Python gate/build-path fixtures / Python 门禁及构建路径 | Quality **81/81**; architecture **42/42 + 1/1 dependency + 1/1 modal**, independent runs; i18n suites **5/5** / 均通过 | Gate runs in tool output; i18n log retained as `i18n-build-path-tests.log` |
| Final independent Release CI tooling regression / 最终独立 CI 工具回归 | **134 passed**, exit 0, 1.884 s; no runtime-speed inference | [Final CI log](/private/tmp/usher-mainwindow-split-20261016/final-ci-release-tests.log) |
| Final formatting and whitespace / 最终格式及空白 | `cargo fmt --all -- --check` exit 0; `git diff --check` exit 0 | Final root verification; source/provenance record below |

`pre-ab-validation.json` records check commands, exit codes and log hashes. The
integration log is SHA-256
`3ecbb7c2530ad8ea5274ae1687222c3dcac44ab3e7e4ed89faf669a1d5e8f54b`;
the final library log is
`122f719cc25fc500fa1092c4c5679f1a47b2312a0f3a3d7b06f9144a381ccf0b`.
The observed dev/test linker `__eh_frame` compact-unwind size warning remains
recorded. For the completed dev/page group, all six samples have exactly the
same structured warning identity multiset as A's fresh confirmation: 2,701 JSON
warning messages, 274 distinct identities. Dependency warnings include Cargo
cache replay. The existing root-binary `linker_messages` warning is present in
both arms. Two additional identical stderr warnings in each settle phase belong
to excluded initialization. This group has no new warning identity; it is not
warning-free. At this group-audit snapshot, parity could not be extrapolated to
other groups; the final matrix audit below now covers all ten groups separately.

机器记录包含命令、退出码和原始日志 hash。已完成 dev/页面组六样本与 A fresh 确认
的 2,701 条 JSON warning、274 个不同身份多重集完全一致，含依赖缓存重放；双方均有
根二进制 compact-unwind linker 警告，settle 额外两条相同 stderr warning 属排除项。
本组没有新增警告身份，不等于没有警告；该组快照不能推断其他组，现已由最终十组
审计分别核对。

The gate-suite tool returns report quality 81/81 in 31.446 seconds, architecture
42/42 in 298.197 seconds and separate dependency/modal additions 1/1 in
3.200/1.195 seconds. The latter are not one 44-test invocation. Those original
outputs were not saved as separate raw files; no such log path is claimed.
The two added controls are
`ArchitectureGateTests.test_pr_generated_package_dependencies_cannot_grow_runtime_infrastructure`
and `ArchitectureGateTests.test_pr_review_modal_shortcut_regression_owner_remains_bounded`.

门禁完整输出当时仅保留于工具返回：quality 81 项、architecture 42 项，另分别执行
dependency/modal 各 1 项；不是一次运行 44 项。没有另外落盘原始日志，不能伪称
存在日志文件；上方单列已落盘的 i18n 记录。

Factory tests retain actual-adapter/shell identity and a standalone-window
negative control, multi-window isolation, identical ModelRc on reentry, weak-handle
release, real two-way popover exclusivity, actual key/Tab crossing, source/packaged
locale refresh, stale Tooltip token rejection and immediate route-exit revocation.
The testing accessibility descendant query cannot discover the Window root's
metadata. Its failed root-name lookup was replaced by the actual embedded
`PrReviewSurface` geometry: exact full-window origin/size, actual document width
and trailing diffstat alignment, across 20 narrow/wide route cycles. Adapter and
shell item-tree identity assertions remain; the geometry requirement was not
weakened into mere presence checks.

factory 专项保留真实 adapter/shell 身份、独立窗口负例、多窗口隔离、同一 ModelRc
重入、weak 释放、双向真实浮层互斥、按键/Tab 跨边界、源/内置语言刷新及 Tooltip
代次/退出即撤销。测试 AX 子孙查询不能定位 Window root metadata，因此将失败的
根名称查询替换为实际 Surface 的严格满窗原点/尺寸、文档宽度和尾部统计对齐，覆盖
20 次宽窄路由循环；adapter 与 shell 身份断言仍保留，没有降为只检查节点存在。

## Recorded failures and controls / 失败与正反例

- The first two root-library attempts failed compilation because migrated test
  modules lacked `PrReviewUiExt`, then `slint::ComponentHandle` trait imports.
  These were repaired in the owning test modules, not exempted.
- Attempt three ran 940 passed/26 failed/9 ignored. Twenty failures required
  loopback sockets denied with EPERM; four Conflict layout fixtures needed the
  actual page owner and synchronized layout; two AI producer fixtures had a real
  composition mismatch. They were not socket failures: installing the PR factory
  before AI callbacks and making the Ask panel visible matched production startup,
  prevented initial visibility publication from clearing task/mount state, and
  retained accepted-user durable-write/shutdown assertions. Attempt four passes
  all 966 nonignored tests with loopback permission.
- The first final factory rerun was 7 passed/1 failed due to the Window-root AX
  lookup described above; the strict Surface-based replacement passes 8/8.
- The modal fence has an actual before/after test against the native shortcut
  entry without a Slint event-loop turn. Before: exit **101**, named assertion
  `modal kind 0 must fence the actual native shortcut entry`. After: exit **0**,
  five modal kinds reject shortcuts immediately on open and admit them immediately
  on close; another feature's modal remains blocking. This synchronizes only the
  owning modal boolean, not layout or PR models.

- 前两轮根库编译失败分别是测试缺少两个必要 trait import，已在所属测试修复。
- 第三轮 26 失败中，20 个为 loopback EPERM，4 个为 Conflicts 实际 owner/布局同步，
  2 个为 AI fixture 装配顺序与面板可见性，不归咎 socket。修复保持用户 receipt、
  durable write 与 shutdown 断言；允许 loopback 的第四轮全测通过。
- factory 首次 final 重跑因 AX root 查询 7 过 1 败，严格 Surface 几何替换后 8/8。
- modal 原生准入实测 before=101/after=0；五种弹窗同次打开即阻断、关闭即恢复，
  不清除其他功能 modal，只同步布尔摘要，不推进布局或复制 model。

The modal before/after logs are retained with SHA-256
`d5446dec6430ef5906e09a14b69d72db962aea37426ac4c11ea26969a8a45af8`
and `9d5b60d4f23c418fe50421a829545a2e54e59ce5948f0cc75abff96501d12ccf`;
`modal-fence-ab.json` records the command and successful source restoration.
This driver changed only the shared shortcut entry's synchronization line to
demonstrate the named assertion's red/green behavior, then restored that file.
It did not record a whole-workspace invariant file inventory. This is a local
functional regression control, with no runtime-performance inference; the formal
B3 build matrix separately guards complete source manifests and environment.

该 driver 仅切换共享快捷键入口的一行同步调用，证明指定断言红灯/绿灯并恢复文件；
没有记录全 workspace 每文件恒定，不是全项目性能对照。正式 B3 构建矩阵另有完整
源码清单 hash 与环境 guard，不能与这个局部功能回归混为一项证据。

## Tooling-only A/B / 纯工具门禁对照

`structure-gates-ab-v2.json` compares original HEAD scripts and candidate scripts
on the same 13 fixtures. It supersedes the interim report; all expected outcomes
match. New invalid cases previously accepted now fail: missing feature-factory
portal, compiler alias bypass, detached renderer, direct/indirect heavy static
imports, swapped literal Tooltip/action measurement identities, adapter I/O,
new-owner growth and a runtime dependency disguised as Slint. Legitimate portal/
Tooltip controls pass in both; the existing root budget remains blocking in both.

`structure-gates-modal-owner-ab.json` adds three controls for the subsequently
added modal-test owner: at-budget remains 0/0, over-budget changes 0/1, and the old
root over-budget control remains 1/1. These are source-gate results, separate from
the CI fingerprint batch's 13 workflow mutations. The recorder/orchestrator's
29 self-tests and private source-freezer/audit tests (7/10) likewise demonstrate
tooling behavior only, not application/runtime gains.

The final read-only postprocessor is `audit_matrix_v2.py`. The original parser
failed on a complete dependency rustc command whose Cargo metadata contains a
quoted newline; the real two-line fixture remains private. V2 parses the complete
command and rejects truncated/nested/ambiguous commands, missing runtime
invocations, foreign targets and changed codegen settings. Its **17 pure-Python
tests passed** in 0.009 seconds, without Cargo. Structured warning tests also
reject identical totals with changed warning prose or child diagnostics. This
supersedes the initial 10-test parser for final audit validation; those historical
tests and the old parser remain preserved.

最终只读后处理器 v2 修复真实 Cargo metadata 带引号换行的完整命令解析；截断、嵌套、
身份含糊、缺少实际 runtime 调用、跨 target 与 codegen 参数变化均拒绝。**17 项纯
Python 自测通过**，不运行 Cargo；同总数但 warning 正文/child 不同也拒绝。旧解析器
及初始 10 项历史验证保留，不冒充最终解析器覆盖。

`audit-v2-tooling-ab-final.json` records the exact command, raw self-test log/hash,
real fixture hash and frozen input identities; its SHA-256 is
`2643cbf90866ca13788c183d1695bae1fa888a21d814ff266b973537001fa33c`.
The new postprocessor is outside the frozen performance inputs and did not change
the Cargo recorder or orchestrator.

13 组对照使用同一 fixture 分别运行原 HEAD 与候选脚本，新增无效路径从误放行变为
拒绝，正常 portal/Tooltip 对照继续通过，旧根预算继续阻断；补充 modal owner 的
3 对照同样满足预期。它们不同于 CI fingerprint 的 13 个 workflow 破坏变体。
记录器/编排器 29 项、私有冻结器/审计工具 7/10 项也只证明工具行为，不代表运行时
收益。原始报告保留于本轮私有临时目录，测试不依赖日期目录。

The same-byte full-workspace cache-restore replay is recorded in the
[fingerprint follow-up](release-ci-fingerprint-20261016.md#frozen-workspace-replay--冻结-workspace-回放).
Its 980-file synthetic A checkout rebuilt 27 units after restored cache mtimes,
while three in-place hot controls were fresh. It explains a reproducible local
invalidation mechanism, not a B3 speed result or the historical hosted thirteen-
package cause. No mtime backfilling workaround was introduced.

同字节全项目缓存回放见上述独立记录；它不是 B3 性能样本，也不追认历史云端原因，
没有引入 mtime 回填绕过真实失效。

## Native regression / 原生回归

The B3 debug payload (`86d5f3d0…131ddb1b`) was exercised with the real macOS
FemtoVG backend and isolated offline settings. Temporary app wrappers preserve
that policy even if the UI tool relaunches an app. Screenshots/AX observations
are in the task transcript; `native-progress.json`, private launch metadata and
application logs record the scenario/payload. These are functional checks,
not startup or runtime-performance samples. Several initial UI-tool bindings
waited hundreds of seconds; none enter the timing results.

B3 使用真实 macOS/FemtoVG、隔离离线配置完成以下检查。临时 wrapper 在自动重启时
也保持隔离；截图与 AX 在任务记录中，启动元数据、日志和 `native-progress.json`
保留实际范围。这是功能回归，不计入启动/运行性能；工具绑定窗口的长等待全部排除。

| Scenario / 场景 | Observed result / 实际结果 |
| --- | --- |
| PR → Conflicts → Back; seven long-path files, duplicate basenames, Unicode, binary rename / 路由及长路径文件 | Single-pointer entry/selection, text return, binary empty state and disabled navigation worked / 单击进入和选文件、文本返回、二进制空态及禁用导航正常 |
| Conflicts layout / 冲突布局 | Splitter drag, scroll lines 61→1, selection and copy success; native zoom 1200→1800→1200 and separate 800-wide Chinese scene / 拖拽、滚动、选区、复制及宽窄窗口正常 |
| Full MainWindow commit-range dialog / 完整主窗口弹窗 | Whole-window scrim; CmdB/CmdComma blocked while open; Cancel followed by CmdB immediately worked / 全窗口遮罩与打开阻断、关闭立即恢复正常 |
| Runtime locale / 运行时语言 | Isolated Settings zh-CN→English refreshed native chrome and Rust PR projections on reentry / 设置切换后原生 chrome 与 PR Rust 投影及时刷新 |
| Fixed Tooltip / 固定提示 | NextConflict↔CopyContent, three successful loops across text files in English and Chinese; short Copied text and right-edge collision inspected / 中英相邻动作各三轮、跨文件、复制成功短文案和右边界避让已核对 |
| Full highlighted Diff / 完整高亮 Diff | Existing `large-ranges-highlighted-v1`: real token paint at wide viewport, multiline selection, scroll to lines 29–31 and native zoom / 现有完整夹具的高亮、跨行选区、滚动及原生缩放正常 |
| AI and route lifecycle / AI 与路由生命周期 | Open/close, no-provider error, sidebar collapse/expand, wide splitter drag changing width by 302 physical pixels; Home hides panel; PR reentry restores content with panel closed / 面板、无 Provider 错误、侧栏、分隔条、退出隐藏及重入恢复正常 |
| Reentry / 重入 | 20 native Home/PR cycles, AX observation at each transition, final highlighted content visibly restored / 20 轮原生往返逐步观察，最终高亮内容恢复 |
| Approval example, English + zh-CN / 中英审批夹具 | Mock Review Cancel/reopen/Approve/Submit: #43 remained open and review-required; list returned with updated review status. #42 single-pointer entry, Merge Cancel/reopen/Squash/Confirm produced merged state / 模拟审批不自动合并，列表刷新；可合并 PR 的取消、重开、Squash 和确认正常 |

Limits and controls remain explicit:

- CUA-injected CmdF entered ordinary `f` in the file filter in **both A and B**.
  Physical-keyboard checking was deferred by the user. Find has no pointer
  opener, so native Find opening/match navigation is **unverified**; existing
  search/focus/Unicode testing-backend regressions passed separately. Do not
  describe the injected chord as a successful native shortcut test.
- The old P5 Find fixture supplies empty highlighted token arrays in both arms;
  its unwrapped body is blank, while wrapped plain-text fallback renders. The
  production rendering check therefore used the existing complete highlighted
  fixture. No production change was made to hide this fixture limitation.
- PR gear→Version pointer exclusivity passed. In the reverse direction the
  existing status-bar outside-click shield consumes the first pointer click;
  the second opens PR settings. A/B status-bar source and z order match. The
  automated inverse exclusivity test uses keyboard activation and does not
  prove reverse first-pointer activation.
- Not all 30 fixed Tooltip categories were hovered individually. The measured
  leaf/identity/source gates cover them structurally; native evidence is limited
  to the categories, locale changes, edge checks and route cycles above. The
  approval example uses mock callbacks and has no production AppKit shortcut
  monitor; its dialogs do not replace the full-main modal check.
- An initial narrow AI splitter drag was clamped; the wide retry changed width.
  A screenshot during sidebar animation was transient; the settled layout was
  checked. An early post-Quit AX query auto-relaunched a task app; that exact
  process was stopped and excluded. Later Quit checks used process inspection
  without querying the app again.

限制：实体键盘 CmdF 按用户选择暂缓；A/B 自动注入同样输入普通 f，原生 Find 搜索栏和
匹配导航不能标通过。旧 P5 夹具的空 token 问题两臂相同，完整绘制改用现有高亮夹具，
未修改生产实现掩盖夹具缺陷。Version→PR 的既有首击盾行为与 Tooltip 类别未逐一 hover
均保留为已知范围限制；结构/测试后端通过不替代原生绘制。窄分隔条、动画中间帧和
工具自动重启的无效尝试均保留记录，不计成功样本。

## Release page warm-edit result / Release 页面热编辑结果

The original frozen runner completed three AB/BA/AB pairs, source restoration
and fresh confirmations after the diagnostic-cache recovery below. Release
parameters were unchanged; the command still selects `ui-components`, so this
is a warm compile-iteration result, not a default-production or hosted-CI result.

| Pair/order | Baseline A (s) | Candidate B3 (s) |
| --- | ---: | ---: |
| 1 / AB | 474.296879 | 288.993820 |
| 2 / BA | 472.074913 | 283.329417 |
| 3 / AB | 466.788085 | 283.425704 |
| Median | **472.074913** | **283.425704** |

The median improved by **39.961710%**, passing this group's 10% compile-time
threshold. B regenerated/recompiled only `usher-ui-pr-review`; MainWindow,
Settings, Components and Messages stayed fresh. Root library/binary work remains.
All 16 records in the recovered cohort replay 2,962 compiler warning messages
with an identical multiset of 273 distinct structured identities, matching stderr
identities, no foreign paths, the same normalized environment, and unchanged
opt-z/CGU-1/panic-abort/full-bin-LTO flags.
Restore confirmations took 0.919748/0.688499 seconds, with zero actual launches
and zero timing work; source hashes returned to the frozen manifests. Six earlier
excluded controls and one interrupted attempt remain separately preserved.

| Unit median (s); overlapping pipelines | A | B3 |
| --- | ---: | ---: |
| Relevant UI build script | 7.53 | 4.40 |
| Relevant generated UI Rust | 255.73 | 68.91 |
| UI frontend / codegen | 54.91 / 200.82 | 19.52 / 49.39 |
| Root library | 43.85 | 43.36 |
| Final binary | 203.85 | 210.26 |

These unit medians must not be added. The final binary became slightly slower
in this page group and is its main remaining compiler stage. The later direct-bin
controls are recorded below. Restored Release generation is A Main 37,753,490 bytes /
383,163 lines, versus B Main 26,153,480 / 269,318 plus PR 14,824,062 / 149,502.
The combined B total is **40,977,542 bytes / 418,820 lines**, an increase of
3,224,052 bytes / 35,657 lines. This is generated source, not binary size or RSS.

Complete group audit: private `prsplit3-release-page-complete-audit.json`, SHA-256
`132c530a183959f8703f7e9b1bd03eb9af04744380b2be0ab9b71cf29e820cf6`.
The first-pair audit was retained separately. At this group-audit snapshot, eight
other groups were still unmeasured; neither this audit nor its threshold pass
adopts B3. Current completion is recorded in the decision table below.

诊断缓存对齐后，原冻结 runner 完成三对 AB/BA/AB、源码恢复和全命中确认。
页面组中位数 472.074913 → 283.425704 秒，改善 39.961710%，通过本组 10% 门槛；
候选只使独立 PR UI 失效，主壳及其他 UI 均 fresh。恢复 cohort 的 16 条记录诊断身份、
实际参数、环境和路径一致，历史 6 条排除控制及 1 条中断完整保留。最终二进制仍约
210 秒，不能把所有单元时耗相加；生成源码合计增加不等于生产体积或 RSS 代价。
这是固定 ui-components 图的热编辑结果；当时其余组及生产验收尚未完成，当前完整
编译矩阵与 P6 失败/拒绝结论另见后文。

## Release MainWindow warm-edit result / Release 主壳热编辑结果

Direct `ui/main.slint` edits were measured separately with the predefined
544 → 545/546/547 px titlebar-action-width fixture. Three AB/BA/AB pairs and
restoration/fresh controls passed, with the same frozen Release command and flags.

| Pair/order | Baseline A (s) | Candidate B3 (s) |
| --- | ---: | ---: |
| 1 / AB | 475.918270 | 380.329418 |
| 2 / BA | 471.080670 | 379.313023 |
| 3 / AB | 475.570333 | 378.247415 |
| Median | **475.570333** | **379.313023** |

The median improved by **20.240394%**, passing this group's 10% threshold.
Both arms recompiled MainWindow; B's PR package, Settings, Components and
Messages stayed fresh. All 16 records have matching warning identity multisets,
environment and actual Release flags, with no foreign paths. Both restored
source/generated hashes returned to their original references; fresh controls
took 1.067863/0.702398 seconds with no actual launches or nonzero timing units.

| Unit median (s); overlapping pipelines | A | B3 |
| --- | ---: | ---: |
| MainWindow build script | 7.07 | 5.48 |
| MainWindow generated Rust | 260.38 | 162.44 |
| Root library | 43.95 | 43.99 |
| Final binary | 207.23 | 209.44 |

Unit timings must not be added. These results prove the measured direct-shell
edit path, separately from the page result; they do not prove dev-profile shell timings,
comment-edit timings, cold builds, default-production runtime or hosted gains.
Private complete audit: `prsplit3-release-shell_semantic-complete-audit.json`,
SHA-256 `5d7c9b5bc549d7e65c4d7be39392f344f4bb267acbae482013b2361f48cf0f29`.
At this audit snapshot, seven groups plus production/runtime gates remained
pending; passing this group alone does not adopt B3.

主壳直接修改单独完成三对 AB/BA/AB、恢复及全命中确认，中位数
475.570333 → 379.313023 秒，改善 20.240394%，通过本组门槛。候选的 PR 包及其他
独立 UI 均 fresh，16 条记录的诊断、实际参数、环境和路径核对通过。Root library
基本不变，最终二进制仍约 209 秒；单元时耗存在重叠，不能相加。此组不代表 dev 主壳、
注释编辑、冷构建、默认生产运行时或云端收益，整体采用仍待其他验收。

## Release Rust-only and unchanged controls / Release Rust-only 与无改动对照

Both controls completed three AB/BA/AB pairs and restoration/fresh checks with
the same frozen Release `ui-components` command. Each group contains six included
performance samples and ten excluded settle/confirm/restore records, all valid.

| Pair/order | Rust-only A (s) | Rust-only B3 (s) | Unchanged A (s) | Unchanged B3 (s) |
| --- | ---: | ---: | ---: | ---: |
| 1 / AB | 206.839619 | 212.632929 | 0.480175 | 0.511976 |
| 2 / BA | 206.923702 | 214.336143 | 0.495645 | 0.502155 |
| 3 / AB | 208.070654 | 212.565998 | 0.494497 | 0.491321 |
| Median | **206.923702** | **212.632929** | **0.494497** | **0.502155** |

The Rust-only fixture changes a comment in `src/main.rs`. It launches only the
final binary compiler unit; root library and all generated UI units remain
fresh. Its observed median cost is **+5.709227 seconds / +2.759098%** for B3,
not an improvement. Binary-unit medians are 206.34 → 211.71 seconds, with the
same opt-z/CGU-1/panic-abort/full-bin-LTO flags. This result limits the page/shell
benefit to those edit paths; it does not justify a general Release speedup claim.

Unchanged builds launch no actual compiler/build-script commands and have zero
nonzero-duration timing units in all six samples. The median difference is
**+0.007657 seconds / +1.548542%**. This is a fresh-build control at about half a
second per invocation, with no claimed acceleration. Neither control has an
applicable 10% related-UI improvement pass; the audits leave that field null.

Each group's 16 records replay the same 2,962 compiler warning messages with
273 distinct structured identities and matching stderr identity multisets, no
foreign paths and the same normalized environment. Source and generated hashes
return to their original frozen references. Rust-only restore confirmations took
1.316703/0.681017 seconds; unchanged confirmations took 0.499957/0.578350 seconds.
All four confirmations launched no actual commands and recorded zero timing work.

The original ledgers match the SHA-256 recorded by their complete group audits:

- [Rust-only ledger](../../validation/mainwindow-build-split-20261016/prsplit3-release-rust_comment-ledger.json):
  `ce115ab069a82934cd4c67b1e89226ca8fa63a0394dbc2089ea7a3eb80581084`;
  [private complete audit](/private/tmp/usher-mainwindow-split-20261016/prsplit3-release-rust_comment-complete-audit.json):
  `f954240e8b56c46acd7c6212f1116703e1228937303858def08bd8cb45585ca1`.
- [Unchanged ledger](../../validation/mainwindow-build-split-20261016/prsplit3-release-unchanged-ledger.json):
  `cc0ade620931625c424a6e8c76dd705c241218032876f74a2df0c6d283890a23`;
  [private complete audit](/private/tmp/usher-mainwindow-split-20261016/prsplit3-release-unchanged-complete-audit.json):
  `a0e7403aab164e05074d64c5735380d8f85254d3df6d7793c8765d8eb1d0f196`.

两个对照各完成三对 AB/BA/AB、恢复及全命中检查，各 16 条有效记录中只计入 6 条性能
样本，另 10 条排除。仅改 `src/main.rs` 注释时只重编最终二进制，候选中位数增加
5.709227 秒（**+2.759098%**），不能称为优化；所有 UI 及 root library 均 fresh。
无改动对照没有实际编译启动或非零计时单元，中位数 0.494497 → 0.502155 秒，差约
7.66 毫秒，不宣称加速。诊断、环境、参数和源码/生成产物恢复审计通过，不等于整体
采用，也不代表默认生产制品、运行时或云端收益。

### Shared-control result / 共享组件对照结果

The predefined single AB positive-control pair completed restoration/fresh checks
and its group audit: **478.788814 → 384.878899 seconds**. The shared input really
recompiled all four generated UI consumers in A (MainWindow, Settings, Components
and Messages), and all five in B, including the extracted PR package. The new
compilation boundary therefore retains the shared-input invalidation path.

The cohort contains 12 valid records: two included AB samples and ten excluded
settle/confirm/restore controls. All have matching warning/stderr identity
multisets, the same normalized environment and actual Release flags, and no
foreign paths. Source and generated hashes returned to their frozen references.
Restore confirmations took **1.072785/0.706206 seconds** for A/B; both launched
zero actual commands and recorded zero timing work. The complete group audit has
no issues. This one-pair positive control is not a three-pair performance median
or a related-UI threshold pass; it does not establish a general speedup or adoption.

The [final ledger](../../validation/mainwindow-build-split-20261016/prsplit3-release-sharedcontrol-ledger.json)
has SHA-256 `a45505b20f300505a8f2b9eb6cf2dd09b1e448c361ff8d775a64c1400384a497`;
the [private complete audit](/private/tmp/usher-mainwindow-split-20261016/prsplit3-release-sharedcontrol-complete-audit.json)
has SHA-256 `378f9172c1117dba282232fa6f55c0a4ae0e4b572ee8c8143b7196eafb09affa`.
All five fixed-graph Release groups are complete; default-feature executable
validation is recorded below. The other four dev groups later completed the same
original evidence plan, but cannot override the formal P6 failure.

共享组件正向对照已完成计划内一对 AB、恢复/全命中确认及组审计：
478.788814 → 384.878899 秒。A 的四个 UI 消费方均实际重编，B 新增独立 PR 包后五个
消费方均实际重编，证明共享输入仍能触发全部真实消费者。12 条有效记录中计入 2 条
AB 样本、排除 10 条控制；诊断身份、环境、实际参数及路径核对通过，源码与生成产物
hash 已恢复。A/B 全命中确认分别为 1.072785/0.706206 秒，实际命令和计时工作均为零。
这是单对正向控制，不是三对性能中位数或相关 UI 提速门槛通过。Release 五组已完成，
默认 feature 可执行文件核对与后续正式 P6 失败见下文；另外四个 dev 组已收齐本批证据。

## Default-feature Release executable audit / 默认 feature Release 可执行文件审计

A separate local macOS supplement built each frozen arm with the exact default
root features: `ai-code-review`, `default` and `renderer-femtovg`. There is no
`--features` argument, no `ui-components`/`test-fixtures`, and no playground
artifact. Both initial feature-transition builds and all four subsequent fresh
confirmations passed: **six valid records**, with four confirmations launching no
actual compiler/build-script commands and recording zero timing work. Actual
build commands preserve opt-z, CGU-1, panic-abort, full binary LTO and strip;
incremental compilation remains disabled (`CARGO_INCREMENTAL=0`).

| Local default-feature executable | Bytes | SHA-256 |
| --- | ---: | --- |
| Baseline A | 39,212,832 | `1ee4f31b9290763eb5f699258cf817f841eee27fe38a34764c398075b42cfa4b` |
| Candidate B3 | 39,547,536 | `f41f06948961be193ab0ef327eba2c3d9886022e5ad49f946829087412434770` |

The measured executable cost is **+334,704 bytes = +0.319199 MiB / +0.853557%**.
This measures the local unbundled macOS executable, not an installer, signed
package, Windows/Linux artifact, RSS or hosted-CI result. Structured compiler
warning identity multisets match across all six records; stderr identity
multisets match within the two build records and within the four fresh controls,
with no ignored identities. Frozen source hashes and the normalized environment
remain consistent, with no foreign paths. The artifact audit has no issues and
marks these executable identities eligible for formal P6 use; **runtime/P6
acceptance is a separate gate and the subsequent formal P6 failed, as recorded
below**.

All six supplement records are excluded from compilation A/B. Switching from
the existing `ui-components` graph to the default graph is artifact preparation,
not a controlled default-production build-speed comparison. The transition times
do not support a compile-time improvement or cold-build claim.

The [private summary](/private/tmp/usher-mainwindow-split-20261016/default-release-supplement/summary.json)
has SHA-256 `1f48e113266c0929a8703c7247701c2f00ada98ba9ff6040f4590ca8972d34aa`;
the [private complete audit](/private/tmp/usher-mainwindow-split-20261016/prsplit3-default-release-artifact-audit.json)
has SHA-256 `11685d77efef57ccf141c29d7fc8781e3da365c282922f931e93da8dd556eb78`.

A separate read-only Finder CUA interface call took **3,960.7 seconds** of tool
wall time while the default Release supplement completed successfully in the
background. That interval measures the CUA interface call, not application
startup or compilation, and is excluded from all performance comparisons.

独立本机 macOS 补充构建使用精确默认 feature，不含 ui-components/test-fixtures 或演练场
产物，实际 opt-z/CGU-1/panic-abort/full-LTO/strip 及关闭增量的参数保持不变。两次
feature 图切换构建及四次全命中确认均通过，后四次实际命令和计时工作为零。默认可执行
文件从 39,212,832 增至 39,547,536 字节，代价 **+334,704 字节（+0.319199 MiB，
+0.853557%）**。六条记录及源码、参数、环境、路径、同阶段诊断核对通过，可作为正式
P6 的制品身份；后续正式 P6 已失败，见下文。六次补充记录全部排除于编译性能
对照，不能把 feature 切换构建时间用于默认生产提速或冷构建结论，也不代表安装包或
其他平台体积。另一次只读 Finder CUA 接口调用耗时 3960.7 秒，期间默认 Release 后台
构建成功；该接口等待不代表程序启动或编译耗时，已从所有性能比较中排除。

## Formal default-Release P6 result: failed / 正式默认 Release P6 结果：失败

**B3 is not accepted and must not be promoted to the next accepted baseline.**
The established batch-66 regression ran three AB/BA/AB pairs with the audited
default-feature Release executables. The driver exited **1**: its summary is
`valid=true`, `passed=false`. Every individual run completed all three ordered
cycles, exited cleanly, was uninterrupted and reported zero framebuffer errors;
these functional results do not override the failed memory gates.

The strict checker also exited **1**. All six runs violate the existing
**RSS strictly below 300 MiB** requirement in peak, lifetime peak, settled RSS
and their raw sampled RSS traces. This requirement already exists in the
[PR Review memory contract](../../specs/pr-review.md) (English line 173, Chinese
line 183); it is not a newly added optional gate. The generic 512 MiB checker
ceiling does not replace it, and even the established relative comparison fails.

| Round/order | A peak RSS (MiB) | B peak RSS (MiB) | A final footprint (MiB; raw result) | B final footprint (MiB; raw result) |
| --- | ---: | ---: | ---: | ---: |
| 1 / AB | 436.453125 | 434.562500 | [134.24](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-default-release/A-1/result.json) | [145.74](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-default-release/B-1/result.json) |
| 2 / BA | 435.375000 | 434.750000 | [128.74](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-default-release/A-2/result.json) | [141.45](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-default-release/B-2/result.json) |
| 3 / AB | 436.734375 | 435.437500 | [144.02](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-default-release/A-3/result.json) | [146.19](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-default-release/B-3/result.json) |

The existing relative gates compare per-arm medians. Their complete results are:

| Median metric | A | B3 | B minus A | Allowed increase | Relative gate |
| --- | ---: | ---: | ---: | ---: | --- |
| Peak RSS (MiB) | 436.453125 | 434.750000 | −1.703125 | 21.822656 | Pass |
| Lifetime peak RSS (MiB) | 436.72 | 434.86 | −1.86 | 21.836 | Pass |
| Settled RSS (MiB) | 433.937500 | 432.640625 | −1.296875 | 21.696875 | Pass |
| Peak footprint (MiB) | 405.06 | 414.47 | +9.41 | 20.253 | Pass |
| Final footprint (MiB) | **134.24** | **145.74** | **+11.50** | **8.00** | **Fail** |
| RSS growth (MiB) | 0 | 0 | 0 | B growth and delta each ≤8 | Pass |
| Heartbeat p95 (ms) | 52.111 | 51.187 | −0.924 | 8.000 | Pass |
| Cycle duration (ms) | 2910 | 2914 | +4 | 582 | Pass |

Final footprint increases by 11.50 MiB, exceeding `max(8 MiB, 5% × 134.24 MiB)`
= **8 MiB**. Both arms failing the absolute RSS ceiling does not excuse B's
relative-footprint failure. No allocator purge, raised budget or smaller fixture
is used to turn this result green.

The protocol requested 90 seconds per process and sampled almost the whole
interval, but **this batch-66 regression is not the full P6 suite or a 15-minute
soak**. The fixture emits heartbeats for about
26 seconds, runs three cycles, clears owned review state and returns to Project;
the remaining interval is post-cycle sampling. The final footprint is the last
emitted diagnostic after that cleanup, not an active-review steady-state result.
The actual window first renders at 800×600; two seconds after the first frame,
the fixture changes it to 1100×700. Metadata's 1100×700 label therefore does not
describe a fixed-size first frame or a 90-second heartbeat window.

The clean launcher removed `MallocNanoZone` and `RUST_LOG`, fixed the locale and
kept the same cleaned environment for both arms. Older RSS measurements made
with different environment settings cannot be directly compared with this
cohort. The present result records a failed gate; it does not establish the
cause of either arm's absolute RSS or authorize changing the memory contract.

The identity join exited **0** and associated both P6 metadata identities with
the prior default-feature artifact summary. It compares declared JSON paths,
hashes and features; it neither rereads executable bytes nor proves runtime
acceptance. Launch intent, successful identity association and valid workload
completion must not be reported as a passing P6 result.

- [P6 summary](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-default-release/summary.json),
  SHA-256 `7f36cec9d4e60a2dbf85fe43337128661f47cbe996d61e2ea0484d8a97bd2fed`.
- [Strict result](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-strict-result.json),
  SHA-256 `853dbabd868b080097f6e369280357b02b7478da7d8095156bf6fbc8fc520e36`.
- [Default-release/P6 identity join](/private/tmp/usher-mainwindow-split-20261016/default-release-p6-66-join.json),
  SHA-256 `62ef0a41267df9e5367d3a9b4f44eb7ac01cce72d83309cc69ab6e4a57b85209`.
- [Clean launch record](/private/tmp/usher-mainwindow-split-20261016/p6-66-prsplit3-default-release-launch.json),
  SHA-256 `6b313dff1372f3d5e58bc180aa223f9fe48f8cd9064f9f5587d71ae88e8200c5`.

正式默认 Release batch-66 的三对 AB/BA/AB 是有效失败：driver exit 1，summary
`valid=true/ passed=false`；六轮业务均完成、干净退出、无中断且 framebuffer 错误为零。
strict checker exit 1，两臂全部轮次的 peak/lifetime/settled RSS 及原始采样均未满足
既有 **RSS 严格小于 300 MiB** 契约，不能改用通用 512 MiB 上限。B 的 final footprint
中位数 134.24 → 145.74 MiB，增加 11.50 MiB，另超过既有 8 MiB 相对容差；其余相对
项结果逐项保留于上表。identity join exit 0 只证明声明的制品身份关联，不证明运行时通过。
90 秒 batch-66 回归不是完整 P6 套件或 15 分钟 soak：heartbeat 约 26 秒，三周期后清
审核状态回到 Project，之后继续采样；
窗口先以 800×600 绘制首帧，首帧后两秒再改为 1100×700。干净环境移除了
MallocNanoZone/RUST_LOG，历史不同环境的 RSS 不能直接比较。**B3 不得升级为 accepted
baseline**；随后四个 dev 组仅收齐本批原计划证据，现已完成，不能覆盖失败，也不能用强制
purge、修改预算或缩小 fixture 制造通过。

## Release diagnostic-cache precondition / Release 诊断缓存前提

The first Release attempt was stopped before completing any pair. Its A/B
`confirm2` controls were fully fresh, but A replayed zero compiler warnings and
B replayed 2,962 from third-party dependencies. The unchanged audit v2 correctly
rejected the warning-count/identity asymmetry. The incomplete A sample was
cancelled (runner exit 130), fixture bytes were restored, and all evidence was
retained. These initialization/confirmation/interrupted records provide **no
Release speed comparison**. See private `release-warning-mismatch-stop.json`;
diagnostic-cache provenance and a minimal reproduction were required before
resuming; the recovery below records their completion. No warning exemption or
Release parameter change was introduced.

首轮 Release 在完整 pair 数为 0 时停止：两臂全命中确认的第三方警告重放为
0 对 2,962，原 audit v2 正确阻断。进行中的 A 样本已取消（runner 130），fixture
原字节已恢复，证据全部保留；这些记录不能计算 Release 收益。当时继续前须查明诊断
缓存来源并完成最小复现，下文记录了恢复结果，没有放宽警告门禁或修改 Release 参数。

The isolated `unicode-vo 0.1.0` reproduction passed four offline builds with a
private Cargo home/vendor/targets. Ordinary initialization actually passed
`--cap-lints allow` and emitted zero warnings; a subsequent `-vv` build was fully
fresh and still emitted zero. Initialization with `-vv` actually passed
`--cap-lints warn` and emitted 150 warnings; its fully fresh repeat replayed the
same 150 identities. All four dependency fingerprint JSON hashes were identical.
This demonstrates the diagnostic-cache mechanism, not production performance.
The official [Cargo verbosity documentation](https://doc.rust-lang.org/cargo/commands/cargo.html#display-options)
also distinguishes dependency-warning visibility under very verbose output.

For the full workspace, 96 dependency units across the 75 warning-producing
packages had byte-identical A/B fingerprints. B's saved diagnostic caches exactly
reproduced the identity multiset of its 2,962 warning messages. A's earlier
initialization command had no `-vv` and no diagnostic caches for those units;
its original dependency rustc commands were not logged. Therefore A's historical `--cap-lints allow` is
an inference supported by the same-Cargo source and the reproduction, not an
observed historical command. Raw reproduction evidence is in
`/private/tmp/usher-warning-cache-repro-20261017-osq7usgx/summary.json`.

隔离的单依赖四步离线复现通过：普通初始化实测 allow/0，改为 -vv 后全命中仍为 0；
从 -vv 初始化实测 warn/150，后续全命中精确重放同 150 个身份；四步 fingerprint
JSON hash 完全一致。全项目 96 个依赖单元 fingerprint 两臂一致，B 的诊断缓存精确
解释 2,962 条重放。A 历史命令没有 -vv，且当时未记录依赖 rustc argv，因此历史
allow 只记为有源码及复现支持的推断，不能冒充当时实测。此实验不证明生产编译收益。

Recovery preserved both old Release directories by same-filesystem rename under
`release-cache-history-20261016T161709Z-9acdeb9d/`, then initialized empty caches
at the original target paths. Sources and debug caches were preserved. A/B init
took 536.622/438.182 seconds, both excluded, with 2,962 warning messages and
identical structured identity multisets, and unchanged actual Release flags.
A child-agent bootstrap preflight rejected a different environment before
invoking Cargo; the root execution reproduced the original `ffb899d5…b2a52b`
normalized environment.

The first bootstrap validator also incorrectly required initialization and fresh
stderr to match. Its only difference was two Cargo notices about ignoring
`rustix`'s unstable profile hint during initialization, present equally in both
arms and absent when fresh. The failed helper/evidence remain preserved. The
corrected private v2 compares stderr strictly within each stage, and structured
diagnostics strictly across all stages; it has no package/warning exemption and
does not change the frozen runner or audit v2. Fifteen pure Python controls
passed, including the actual before/after failure and new-warning negative cases.

Four root-run fresh confirmations passed at 0.507/0.678/0.481/0.478 seconds with
no actual commands or nonzero timing units, and identical identity multisets for all
2,962 warning messages.
`release-warning-bootstrap-v2/summary.json` has SHA-256
`c4a13ff0ad698758166899c4ed3f3c6dc3d477f1af2a840e237e3eb9751ce89d`.
All bootstrap work is excluded from performance. The original matrix was resumed
with the same source, environment, profile and tooling, remeasuring the incomplete
pair; this recovery is not an optimization or a cold-build speed result.

恢复时完整保留两臂旧 Release 目录，在原路径按同一命令初始化；A/B 分别
536.622/438.182 秒，均排除。子代理环境不符的尝试在 Cargo 前停止，root 使用原
环境继续。初版 bootstrap 将 init 与 fresh 的 Cargo 摘要也要求完全相同而误阻断；
两次 rustix hint 提示是唯一差异，真实诊断始终一致。原失败完整保留，v2 按同阶段
比较摘要、跨阶段比较真实诊断，15 个正反例通过且没有警告特例；四次 fresh 都为
零工作。冻结 runner/audit 及 Release 参数不改，原不完整 pair 重新测量，不宣称
恢复工具或初始化产生性能收益。

## Final compile matrix and cache inventory / 最终编译矩阵与缓存库存

The [final matrix summary](/private/tmp/usher-mainwindow-split-20261016/prsplit3-final-matrix-summary.json)
has SHA-256 `104a2683cc599f882973a74b4baf45668b1398541bc2fea75f0256b613029259`.
All **ten groups are `complete_audited`**, with no provisional medians, source
issues or compile-audit issues. The unchanged v2 auditor reports
`compile_audit_passed=true`, `adopt=false`. Across the original ledger history,
159 attempts comprise **158 valid completed records: 52 included measurements
and 106 excluded controls**, plus one historical interrupted attempt. There are
no pending attempts or completed-invalid records. The earlier rejected warning
cohort and interrupted Release attempt remain excluded; B1/B2 were never built.

This is a completed warm compile matrix, not runtime acceptance. The generic
compile-summary `runtime_acceptance=not_evaluated` and `remaining_acceptance`
fields describe that projection's scope; they do not turn the separately
completed, failed P6 back into a pending check. The four related page/shell
groups pass their compile thresholds, while Rust-only/unchanged/shared-input
groups remain controls. There is no true-cold or hosted-CI speed claim.

The [cache inventory](/private/tmp/usher-mainwindow-split-20261016/target-cache-inventory.json),
SHA-256 `def2e42d537884dfd82cbc4ec24b99937ce79951208a27dabd473ad4cf910e77`,
reports `state=complete`, `passed=true` after all ten ledgers completed. These
are **mixed warm cache costs**, combining profile/feature transitions, prior
history and examples. In particular, B includes extra native approval examples.
This is not a steady-state cache-size comparison, cold-build cost, exclusive
physical-disk accounting or evidence for the alternative 10% size threshold.

| Cache bucket; path-logical bytes | A | B3 |
| --- | ---: | ---: |
| Whole target | 16,721,805,191 | 17,325,300,004 |
| Debug | 12,920,076,684 | 13,439,345,160 |
| Debug incremental subset | 9,090,205,446 | 9,375,143,435 |
| Debug examples subset | 0 | 155,307,973 |
| Release | 3,767,629,858 | 3,855,358,318 |
| Release incremental subset | 0 | 0 |

Whole-target regular-file counts are 14,176/14,536. Deduplicating hard links by
inode gives 15,821,931,449/16,396,474,249 logical bytes and
15,963,873,280/16,541,544,448 `st_blocks` bytes. APFS shared/cloned extents mean
the latter are not exclusive allocated storage. Buckets overlap and must not be
added; directory metadata and non-regular entries are excluded. Collection is
not an atomic filesystem snapshot. Its passing collection result is not an
optimization acceptance, and B's extra example artifacts must not be attributed
to the production UI split.

最终矩阵十组全部审计完成，编译审计通过且 `adopt=false`。159 次尝试中有 158 条有效
完成记录（52 条计入测量、106 条排除控制），另有 1 次历史中断，没有运行中或完成后
无效记录。历史排除与未构建的 B1/B2 保留。编译汇总不评价 runtime，不能把已执行且
失败的 P6 改回待测。库存只记录混合 warm 成本，B 另含 155,307,973 字节原生示例；
不是 steady/cold 收益、独占物理盘占用或 10% 体积门槛通过。全部编译证据完成，
仍不改变 B3 拒绝进入 accepted baseline 的决定。

## Final provenance and cleanup / 最终来源及清理

The [final machine handoff](/private/tmp/usher-mainwindow-split-20261016/final-handoff.json),
SHA-256 `eec38b83db6b7e5b2d613fa3c6893fece05d82b483264ea4a068074e81cac26e`,
joins compile completion, failed runtime/strict gates, successful identity/source
proofs and `adopt=false`. It records the applicability of pre-freeze checks and
the remaining native/platform/hosted limits; separately validated local CI
tooling does not imply a hosted speed improvement.

Git HEAD remains `18a43c8`; the dirty workspace is not a newly accepted commit.
The [source proof v2](/private/tmp/usher-mainwindow-split-20261016/final-root-source-proof-v2.json)
records 840 non-documentation files exactly matching frozen B, one fully verified
helper-comment-only replacement (`four` → `five` application build scripts),
and all eight independent CI-tooling file hashes matching their own A/B
provenance. Five existing CI files are accounted for under that separate scope,
not claimed byte-identical to the compile snapshot. Documentation is excluded;
the user macOS hash remains
`ef28417899936c7e877414aea0104cf6b94092f2b0db580206190a776180609b`.
V1's overbroad scope assumption and incorrect comment literal failed and remain
preserved; v2 corrected the provenance scopes without source changes or relaxed
benchmark gates. V2 SHA-256 is
`713dcd10cf4b1785c7a1a1b7db8acf8a3139d7601ea051be9a5e26386618f7ba`.

Fresh independent CI tests passed 134/134 in 1.884 seconds; final formatting and
diff checks exited 0. The
[owned-process cleanup](/private/tmp/usher-mainwindow-split-20261016/final-owned-process-cleanup.json)
records an already expired temporary awake assertion and an exact-birth/argv
matched task-owned assertion stopped, with no remaining Cargo/rustc/Usher
processes and no OS-setting changes. These closeout checks do not replace the
failed runtime gate or the explicitly deferred native/hosted checks.

最终来源核对区分冻结编译候选与独立 CI 工具：840 个非文档文件逐字节匹配 B，helper
只发生完整验证的注释替换，独立 CI 八文件 hash 全匹配，用户 macOS hash 保持。v1
错误范围假设的失败保留，v2 没有改源或放宽门禁。最终 CI 134 项、格式及 diff 检查
通过，临时 awake 进程按所有权清理，无残留编译/应用进程，也未修改 OS 设置；不改变
P6 失败及列明的原生延期/云端未测范围。

## Final validation and decision / 最终验证与决定

| Required evidence / 必需证据 | Current status / 当前状态 |
| --- | --- |
| Frozen B3 dev page semantic edits, three A/B pairs / 页面语义编辑三对 | Complete; median 177.844069 → 64.587991 s, −63.6828%; this group only / 本组完成 |
| Direct MainWindow shell edits reported separately / 主壳直接修改分开报告 | Both audited: dev 170.966685 → 113.074052 s, −33.861938%; Release 475.570333 → 379.313023 s, −20.240394% / 两 profile 完成 |
| Release page semantic edits, three A/B pairs / Release 页面语义编辑三对 | Complete; median 472.074913 → 283.425704 s, −39.961710%; this group only / 本组完成 |
| Complete warm compile matrix / 完整热编辑编译矩阵 | Ten dev/Release groups complete and audited, compile_audit_passed=true/adopt=false; 52 included measurements / 十组完成，编译通过不等于采用 |
| Genuine cold samples / 真正冷样本 | Unmeasured; required for a cold-build claim, not every warm-iteration batch; interrupted initialization excluded / 未测，不宣称冷收益 |
| Rust-comment samples / Rust 注释对照 | Both audited: dev 1.738656 → 1.658398 s (control); Release 206.923702 → 212.632929 s, +2.759098% cost / 两 profile 完成，仅作对照 |
| Unchanged samples / 无改动对照 | Both audited/fresh: dev 0.463772 → 0.464682 s; Release 0.494497 → 0.502155 s; no speedup claim / 两 profile 全命中，不宣称加速 |
| Shared-component positive control / 共享组件正向对照 | Both audited n=1 AB: dev 179.106307 → 124.317071 s; Release 478.788814 → 384.878899 s; all 4→5 consumers rebuilt and restored fresh / 两 profile 正向及恢复对照通过 |
| Cache inventory / 缓存库存 | Recorded mixed warm costs, including B's extra native examples; no steady-state/cold/size-threshold claim / 仅记录混合成本 |
| Default-feature Release executable identity/size / 默认 feature Release 可执行文件身份与体积 | Local macOS audit passed: six records, four fresh controls; +334,704 B / +0.319199 MiB / +0.853557%; no compile-speed claim / 本机审计完成，体积成本已记录 |
| Default-production runtime/P6 gates / 默认生产运行时门禁 | **Failed:** valid workload, driver/strict exits 1; six strict RSS <300 MiB failures and final-footprint median +11.50 MiB >8 MiB / 有效失败，不能以身份核对或 512 MiB 替代 |
| macOS native PR/Conflicts/dialog/scroll/resize/representative Tooltip/AI route reentry, English + Chinese / 原生多语言交互 | Completed within the scope recorded above; physical CmdF/Find remains deferred by the user, Tooltip kinds were not all individually hovered, and the existing Version→PR first-click shield limitation remains / 已完成上文列明范围，保留具体未测项与既有限制 |
| Hosted default-branch prewarming and Windows warm restore / 云端默认分支预热与 Windows 热恢复 | Not executed; no cloud workflow was triggered / 未执行，未触发云端构建 |
| Final adopt/reject decision / 最终采用或拒绝 | **Reject B3 as the next accepted baseline. Candidate remains only in the workspace/evidence; original A is preserved / 最终拒绝 B3 进入 accepted baseline，候选仅留工作区与证据，原 A 保留** |

Warm-edit plans separate page and direct-shell edits, use three paired samples
with AB/BA/AB order, and exclude initialization, relocation, settle/fresh confirms,
restore builds and incomplete pairs. Compile claims require the relevant median
to improve by at least 10%; the existing alternative size threshold and all
relevant runtime/native gates remain binding. A failed candidate cannot advance
into the next baseline. Release builds with `ui-components` are not the default
production artifact, and no hosted CI gain follows from these local results.

页面与主壳分开测量，三对 AB/BA/AB，排除初始化/迁移/settle/fresh/恢复/不完整对。
编译收益须满足相关中位数至少 10% 改善，既有体积替代门槛及运行时/原生验收仍
阻断；失败候选不得进入下批基线。带 ui-components 的 Release 不等于默认生产
制品，本机结果也不能推出云端 CI 收益。

Raw manifests, commands, test logs and timing HTML remain local and ignored under
`docs/validation/mainwindow-build-split-20261016/`; frozen source/cache trees and
gate reports are under `/private/tmp/usher-mainwindow-split-20261016/`. Tracked
tests do not depend on these dated evidence directories. This record closes the
original ten-group compile batch and preserves earlier failures and the failed
formal P6. B3 remains a reviewable workspace/evidence candidate, not an accepted
baseline; the original frozen A source/cache/manifest is preserved. Preserving A
does not retroactively certify it against the failed strict RSS gate.
User macOS changes and unrelated working-tree changes remain preserved.

机器证据保留于本机忽略目录和私有临时目录，受跟踪测试不依赖它们。本记录完成原计划
十组编译批次收口，保留历史失败及正式 P6 失败；B3 仅留为工作区候选与证据，最终拒绝
进入 accepted baseline，原 A 源码/缓存/manifest 保留，也不据此倒推 A 通过严格 RSS。
用户 macOS 与其他无关工作区改动保持完整。
