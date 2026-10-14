# PR Conflict Browsing / PR 冲突查看

## Ownership / 职责

`src/git/conflicts/` owns normalized status, snapshots, marker parsing and isolated
Git analysis. Provider adapters own authenticated HTTP and profile decoding.
`workflow/pr_conflict_status.rs` owns the bounded ephemeral state-store facade
shared by PR list and detail. `workflow/pr_review/conflicts/` owns typed session
generations, cancellation and selected-file preparation. Its Slint bridge holds
projection data and commands, separate from ordinary Diff selection and models.

Conflicts 保留 PR 标题，显示平铺文件列表和只读源码。返回保留来源 Tab；列表入口
返回列表。离开 PR 路由取消冲突任务，普通 Review 缓存遵守主规格。没有在线编辑、
Accept、标记已解决或提交操作；后续范围见 [迭代计划](../pr-conflicts-iterations.md)。
Opening an already active session is a no-op, including while loading. Only the
explicit refresh action reloads that session; re-entry preserves selection and scroll.

## Status / 状态

- Open rows distinguish Checking, Conflicting, Clear and Unknown. Closed/Merged
  rows carry NotApplicable and omit the open-PR badge.
- Checking uses the shared animated `GitHubSpinner` in the same 14px icon slot
  in both the list and conflict workspace. Other states unmount the spinner and
  restore their static status icon; the localized status label stays visible.
- GitHub uses explicit `mergeable`; null means Checking. GitLab uses explicit
  conflict/mergeable/checking states. False `has_conflicts` alongside an approval
  blocker does not prove Clear.
- Enterprise Gitee v8 reverses `conflict_result`: true is Clear. Public v5 false
  `mergeable` remains Unknown. Overall merge eligibility is not conflict evidence.
- Fresh list detail observations are reused. A lifetime owner accepts only the
  newest pending list enrichment round; the driver drops a replaced round's
  actual HTTP/backoff futures and RAII permits before starting its successor.
  Four shared permits bound logical enrichment jobs, including backoff. This
  does not bound list metadata/totals, workspace reads or all physical TCP
  connections. Checking retains at most two delayed rereads and list rows never
  download Git objects. The 64-entry ephemeral cache expires after 30 seconds;
  replacement can reuse valid entries, while refresh/project invalidation clears
  eligibility. Effective hosting/credential memory changes invalidate before
  disk save, including SaveErr; setters do not notify when normalized current stored fields remain equal.
  Legacy loaded fields with whitespace/trailing-slash differences may invalidate
  conservatively on their first normalization.
- Typed scope epochs are captured before contributing reads. A private short
  gate serializes invalidation/close with typed cache commits; a stale epoch
  cannot repopulate a cleared cache or update a newer UI scope. Repository, PR
  and hosting/credential fingerprints remain part of identity. Projection
  subscriptions only enqueue event-loop work and must not reenter that gate.
- Workflow binding installs the driver and its existing async-drain registration
  eagerly. Close rejects new rounds/observations, clears pending work and drops
  active reads/backoffs; drain retains the real JoinHandle until completion and
  caches its actual join receipt. Dropping a drain waiter does not abort/drop the
  owned driver. The current void legacy shutdown adapter logs join failures;
  this feature does not introduce a global exit deadline or nonzero-exit policy.

四许可仅约束列表 enrichment 的逻辑任务；共享 Runtime、Provider HTTP、Workspace
独立取消和 Page/Totals 代际保持原职责。最新轮次替换与设置失效会取消实际读取和
退避 Future，服务器可能稍后才观测连接关闭。保存失败不撤销已生效的内存凭据，
因此仍须推进 scope 并拒绝旧观测。缓存、UI 排队回写和关闭准入共用同一短 gate。
- Local analysis is labelled and does not overwrite server list observations.

跨平台状态只依据精确信号，不能把审批、流水线等阻塞解释成代码冲突。列表与详情
服务器观测通过共享 facade 同步；不持久化翻译文案、凭据或冲突源码。

## Snapshots / 快照

Enterprise v8 reads `current_state` and `conflicts`, preserving source
`their_path` and target `our_path`, independent of write permissions. Commitish
references are resolved before and after loading; moving revisions fail as Stale.
UTF-8/base64 text is decoded. Unsupported content remains a named file without
a text preview. Unsupported native endpoints may fall back to Git;
authentication failures do not trigger that fallback.

Other profiles use a temporary bare repository and validated PR head refs
(`refs/pull/<iid>/head` or `refs/merge-requests/<iid>/head`). Fetched revisions are
pinned and checked against available API SHAs. Modern
`merge-tree --write-tree -z --messages head target` handles ancestry without
checkout, index merge, commit or push. Stage entries and typed message paths
preserve structural conflicts. Binary/submodule/symlink contents have no text
preview; structural conflicts may expose text without inline markers.

Git uses the user's Git credentials, independently of API Tokens. Background
prompts, hooks, external protocols, recursive submodules, redirects and global
merge drivers are disabled at the relevant subprocess boundaries. Shared
process-tree ownership cancels descendants; temporary objects are dropped on
completion/cancellation. Commands time out after 60 seconds and local analysis
after 120 seconds. A 256 MiB directory budget is polled on a worker every 500 ms:
this is a cancellation threshold, not a filesystem quota. Preview limits are
8 MiB, 1,000 files, 100,000 lines per file and 64 KiB per line. Exceeded limits
produce explicit errors rather than silent truncation.

企业部署、私有 fork refs、Git 凭据和服务器合并策略仍须线上验收。字段 fixture
和本地 Git 测试不能证明所有平台的线上权限；无法读取时显示不可用，不解释为无冲突。

## UI And Locale / UI 与多语言

The code viewport reuses syntax rendering and shaped-token selection. Fixed
28px rows use the shared virtual-list facade and exact extents. `LatestCompute`
prepares selected files on a worker; only the newest result installs a lazy Slint
model on the event loop. Clipboard selection projection also runs on a worker.
The document extent binding uses only the source row count, independently of
the offset-dependent visible window. Inertia must never read an extent that
depends on its own animated offset. File copying uses `GitHubCopyButton` and
confirms only a successful worker write for the current session/file selection;
errors resolve a shared semantic message on the event loop. Changing files
clears feedback. The Conflicts entry uses the same 14px foreground text, 16px icon
and 48px row as the other review tabs. The shared model-driven navigation moves
whole trailing entries into More when they do not fit. Inline and menu counts
display 99+ above 99, with complete counts retained in accessible labels.
The Conflicts badge counts files in a successfully loaded snapshot, including
nontext files, rather than marker blocks. Unknown counts omit the badge; a
confirmed empty snapshot displays zero. The derived review summary survives
returning from Conflicts after the heavy models are released. Refresh, leaving
the review route, a new PR/project and provider invalidation clear it.
Entering ordinary Review starts one cancellable background snapshot read, so
the badge appears without opening Conflicts first. Route-history reentry also
starts the read after a route exit cleared it. The read uses the same provider
adapter and bounded Git fallback as explicit entry, with silent HTTP feedback.
It publishes only count/status until the workspace is opened; file and code
models stay unmounted. Opening Conflicts joins the in-flight read or reuses its
completed snapshot. Session generations reject late completions after reset.
The flat file list uses fixed 56px rows: basename first, muted full path second,
and a trailing red conflict icon in a reserved column. Renames retain both paths
in the secondary line, accessible label and root-hosted hover tooltip. Pointer,
Enter and Space select the same file; activation dismisses the tooltip.
All snapshot entries remain unresolved, including nontext/structural conflicts
with zero marker blocks. Marker counts belong to the code toolbar, not file rows;
resolved indicators are deferred with the resolution workflow.
Narrow workspaces initially collapse it; its toggle remains available.
Its 9px divider hit area supports drag resizing with the ordinary review file
tree's pointer-delta and cancel behavior. The local width preference survives
hide/show and is clamped to 220px through min(520px, 45% of workspace width),
without replacing source models or refreshing content.

Brackets have a 1px red left stem with ticks pointing right. The narrow bracket
lane is yellow from opening marker through closing marker inclusive. Number
cells after the opening marker through the closing marker are yellow; the
opening number cell is neutral, digits muted, and code cells retain the canvas.

所有文案使用共享 I18n catalog；领域错误只保存语义 key。运行时切换语言无需
重新抓取源码。起始 `<<<<<<<` 行的窄括号槽必须有黄色背景，不能与行号单元格混淆。
第一批包括文件/冲突导航、选择/复制、滚动及加载/空/错误/非文本状态。

## Native Fixture / 原生验收

```sh
cargo build --locked --features test-fixtures
USHER_BENCHMARK_OFFLINE=1 \
USHER_BENCHMARK_STATE_ROOT=/tmp/usher-conflicts-qa \
USHER_PR_CONFLICTS_FIXTURE=1 \
USHER_PR_CONFLICTS_LOCALE=en \
./target/debug/usher
```

Add `USHER_PR_CONFLICTS_FILES=long-paths` for shared-prefix paths, duplicate
basenames, a root file, Unicode and a renamed nontext conflict. The same option
works with `USHER_PR_CONFLICTS_SCENE=review` before opening Conflicts.
Set `USHER_PR_CONFLICTS_DELAY_MS=30000` with the Review scene to inspect loading
animation before the mock snapshot arrives (default 300ms, maximum 60000ms).

Only this isolated offline fixture opens Conflicts automatically. Verify wide
and narrow layouts, first/last marker geometry, both navigation directions,
file switching, nontext content, selection/copy, scrolling and runtime locale
changes with native screenshots. Run library/PR journey tests and i18n/UI policy
gates. Build success alone does not establish layout correctness.
Set `USHER_PR_CONFLICTS_SCENE=review` to start on ordinary Review and exercise
the asynchronous count read without first opening the conflict workspace.

Native screenshots, commands and explicitly unexecuted scenarios are recorded
in [the 2026-09-28 acceptance record](../archive/pr-conflicts/validation.zh.md#offline-validation-20260928) and
[the interaction-fix acceptance record](../archive/pr-conflicts/validation.zh.md#interaction-fixes-20260929).
The current single-row navigation and resizable pane are verified in
[the navigation acceptance record](../archive/pr-conflicts/validation.zh.md#navigation-and-counts-20260929).
