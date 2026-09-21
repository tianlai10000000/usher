# Exact Virtual Scrolling / 精确虚拟滚动

## Scope / 范围

Reusable contract for variable-height native Slint documents: wrapped logs,
Markdown blocks and PR Diff rows. This is the default solution when a thumb
changes size while scrolling an unchanged document. Fixed-height ListView
usage does not need migration merely because it uses ListView.

本文是可变行高原生 Slint 文档的通用契约，适用于换行日志、Markdown block 和 PR
Diff 行。未变化文档在滚动中滑块尺寸变化时，默认采用本方案；固定行高 ListView
不需要仅因使用 ListView 而迁移。

## Cause / 原因

Slint's virtual ListView repeater estimates unseen row geometry from mounted
rows. With mixed wrapped/unwrapped rows, a different visible window can change
the estimated document height. A scrollbar derived from that height changes
size and scroll range during ordinary scrolling. Freezing only the thumb,
debouncing measurements, using source-line count, or retaining the largest
observed estimate does not provide correct document geometry.

Slint 虚拟 ListView repeater 通过挂载行估算未见行的几何。长短混排/换行时，可见
窗口改变会改变估算的文档高度；依赖它的滑块尺寸与滚动范围因此变化。仅冻结滑块、
debounce 测量、按源行数估算、取已见高度最大值，都不能保证正确文档几何。

## Shared Infrastructure / 共享基础设施

- `crates/usher-virtual-list/`: exact extents, prefix-sum offsets, bounded
  visible/overscan windows and clamped reveal through `VirtualList`.
- `src/app_runtime/latest_compute.rs`: cancellable latest-only background
  computation, stale-result exclusion and registered shutdown cancellation.
- `ui/components/github_scroll_view.slint`: application-owned viewport and
  scrollbar; `GitHubScrollView` can host a fixed-size virtual document, or a
  native Flickable can pair with `GitHubVirtualListScrollbar`.
- Feature-owned adapters retain domain parsing, font/layout metrics and typed
  row projections. UI code never depends directly on the virtualizer engine.

共享模块分别负责精确 extent/offset/有界窗口、可取消后台计算，以及应用自有滚动
视口/滑块。领域解析、字体排版和强类型行投影仍归 Feature，禁止 UI 依赖底层引擎。

## Integration / 接入步骤

1. Define a layout key from source identity/revision, final text width, font,
   line-height policy and paint metrics. Offset, hover and scroll direction must not be in the key.
   Same-count row edits must advance the revision; model identity alone is not
   a content revision.
2. Prepare immutable Send-safe source data. Measure expensive wrapping on
   `LatestCompute`, passing the event-loop-selected font collection. Match
   painting's font, `line-height-factor` or documented fallback pitch, wrap mode,
   padding, glyph safety and line-box rounding exactly. Follow
   [Multiline text layout](text-layout.md). Cancel obsolete requests and reject
   results whose key no longer matches.
3. Install the complete positive row extents into `VirtualList` once per key.
   Use its `content_extent` as the only document-height source. Keep the extent
   stable until an actual source or layout change is committed.
4. Give Flickable/ScrollView that exact document height. Render only the bounded
   virtual window, with each row at its exact start and extent. Reuse a stable
   rows ModelRc; scrolling must not reparse, reshape or convert the whole source.
5. Feed the scrollbar the same content/visible extents and viewport offset.
   Never mix exact thumb geometry with ListView's estimated scroll range.
   Freeze pointer grab/mapping during a direct drag, not document truth during
   all scrolling. Wheel scrolling of an unchanged document needs no timer hack.
6. Refresh the window imperatively on offset/size/source/worker-epoch changes,
   with a reentrancy guard. Row model mutations must not run inside a pure
   property binding that depends on the same rows. Worker completion touches
   Slint only through weak window + `invoke_from_event_loop`.
7. Define reflow/content-update policy explicitly: clamp invalid offsets,
   retain or reset the intended source position, and expose initial loading.
   While measurement is pending, show one centered loading state and hide any
   stale row projection. Do not install new wrapping at an old revision or
   continue rendering rows from another source. Independent nested output owns
   its gestures; do not put an arbitrating page Flickable above it.
8. Treat entry count and accounted retained bytes as soft eviction targets for
   unmounted source/layout caches, not as hard heap or RSS limits. Pin the current
   callback and every rows model still mounted by the UI; their combined working
   set may stay over target until those leases are released. Retry only while over
   target, then evict the oldest unmounted entry and request non-blocking worker
   cancellation. Cache snapshot byte counts when snapshots are produced so the
   event loop reads O(entries) counters instead of rescanning document rows.

1. Layout key 包含源身份/revision、最终文本宽度、字体、行高策略和绘制指标，不包含
   滚动位置、hover 或方向。同数量的行修改也要递增 revision，model 身份不能替代内容版本。
2. 构造不可变 Send-safe 源数据，昂贵换行交给 `LatestCompute`，传入 event-loop
   选定的字体 collection；字体、`line-height-factor` 或已记录的 fallback pitch、wrap
   mode、padding、安全余量及 line-box rounding 必须与绘制一致，并遵循
   [多行文本排版规范](text-layout.md)。取消旧请求，拒绝 key 不匹配结果。
3. 每个 key 只安装一次完整正数行 extent；唯一总高度为 `VirtualList.content_extent`，
   只有真实内容或布局提交才能改变它。
4. 视口使用精确文档高度，仅挂载有界虚拟窗口，行使用精确 start/extent。复用稳定
   ModelRc；滚动不得解析、排版或转换整个源 model。
5. 滑块使用同一套 content/visible extent 和 offset，禁止混用精确滑块与估算范围。
   直接拖动仅冻结抓取映射，普通滚轮不需要定时冻结总高度。
6. offset/尺寸/源/worker epoch 变化通过带重入保护的命令刷新窗口，禁止在依赖自身
   行的 pure property binding 中修改 model；Worker 回写使用 weak window 与
   `invoke_from_event_loop`。
7. 明确重排/更新时的位置策略、非法 offset clamp 和初次 loading；不安装旧版本排版，
   等待测量时显示一个居中 loading 状态并隐藏旧行投影，不显示另一来源的行。独立输出区不得有
   抢占其手势的祖先页面 Flickable。
8. 条目数与已记账 retained bytes 是未挂载源/布局缓存的软淘汰目标，不是堆内存或
   RSS 硬上限。当前 callback 与 UI 仍挂载 rows model 的所有视口都必须 pin，其组合
   工作集可持续超标直至 lease 释放。仅在超限时重试，随后淘汰最旧未挂载项并发出
   非阻塞 Worker 取消请求。快照生成时缓存字节计数，使 event loop 只以 O(entries)
   读取计数而不重新扫描文档行。

## Acceptance / 验收

| Case / 场景 | Required result / 必须满足 |
| --- | --- |
| Mixed short, long JSON/path, CJK and blank rows / 长短、JSON/路径、中文与空行混排 | Every wheel/animation frame retains identical content and thumb height at fixed source/width / 源和宽度不变时逐帧总高度、滑块高度不变 |
| Thumb travel 25/50/75/100% / 滑块行程 | Same normalized exact offset; final row/continuations reachable at 100% / 归一化精确 offset 一致，100% 可达最后行/续行 |
| Down then up, drag release / 下滚、上滚、拖动释放 | No gap, overlap, source-number duplication or estimated-range correction / 无空洞、重叠、重复源行号或估算范围回跳 |
| Resize narrow/wide and normal/tall / 窄宽、普通高窗口 resize | Reflow once per layout key; painting and installed extents agree / 按 key 重排，绘制与 extent 一致 |
| Line-height factor or fallback pitch change / 行高 factor 或 fallback pitch 变化 | New layout key; worker measurement, painting and all extents change together / 生成新 layout key，Worker 测量、绘制与全部 extent 同步变化 |
| Append, tail eviction, same-count edit / 追加、尾部截断、同数量修改 | Correct revision/rows/extent; no stale worker result / 版本、行和高度正确，无旧 Worker 结果 |
| Collapse/reopen, route leave and source switch / 折叠重开、离页与切换来源 | Mounted rows stay valid; released leases permit oldest-unmounted reclamation, with no cross-source content or orphan projection / 挂载 rows 保持有效；lease 释放后允许回收最旧未挂载项，无串源或遗留投影 |
| Many large source switches / 多个大日志来源切换 | After leases release, entry and accounted-byte totals return to their soft targets; exact-target and one-byte-over boundaries behave deterministically / lease 释放后条目数与已记账字节回到软目标；恰好等于目标与超 1 byte 的边界行为确定 |
| Multiple expanded large logs / 同时展开多个大日志 | All mounted rows remain complete with exact extents while pinned, even over target; collapsing or leaving the route later reclaims the oldest unmounted owners and stops retrying at target / 即使超标，所有已 pin 的挂载 rows 仍完整且 extent 精确；折叠或离页后回收最旧未挂载 owner，回到目标后停止重试 |
| English + packaged non-source locale / 英文与内置非源语言 | Identical geometry rules, current semantic loading/error copy / 几何规则一致，loading/error 文案即时正确 |
| Production worker path + UI route / 生产 Worker 与真实路由 | Not build-only or synchronous-test-only acceptance / 不得仅编译或仅同步测试交付 |

## Reference Implementations / 参考实现

- Markdown: `src/artifact_preview/virtual_layout.rs` and
  `ui/components/artifact_preview/github_artifact_preview.slint`.
- PR Diff: `src/workflow/pr_review/virtual_list.rs` and
  `ui/features/pr_review/` (lightweight exact display-column metrics).
- Job logs: `src/jobs/log_viewport.rs`, `src/jobs/log_layout.rs` and
  `ui/components/jobs/job.slint` (background wrapping, source-numbered rows,
  pinned mounted rows, and 16-entry / 16 MiB soft eviction targets for the
  accounted unmounted cache).

Read the feature contract for its content-update/reflow policy before copying
an adapter. Share the infrastructure and invariants, not feature-specific state.

复用前先读对应 Feature 的内容更新/重排策略；共享基础设施与不变量，不复制领域状态。
