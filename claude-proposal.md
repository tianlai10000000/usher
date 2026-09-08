# Usher 项目缺陷分析与优化提案

> 首轮审查：2026-08-27 ｜ **第二轮复审：2026-08-30（HEAD=73ae042，见 §9）** ｜ 分支：`feature/pr-review`
> 方法：5 路并行深度审查（Git Provider 层 / PR&AI Review 控制器 / Slint UI 与渲染 / 基础设施与平台层 / 全局质量体检），所有结论均附代码位置证据，未经证实的猜测不收录。

> 第三轮性能与架构复审：2026-09-05（HEAD=`2ff3f3b`），2026-09-07 归档，见 §13–§16。第三轮采用代码路径核查、定向最小复现及本地检查；前文数据、方法与完成状态保留为对应批次的历史记录，不代表第三轮重新验收通过。

---

## 0. 执行摘要

Usher 是一个约 **94,268 行手写代码**（Rust 62,745 行 + Slint 31,523 行）的桌面 PR Review / AI 代码评审客户端，工程素养明显高于平均水平：git 解析层（10,907 行）非测试代码零 panic、452 个测试（占代码量 29.4%）、6,046 行 spec 文档且与代码一致、unsafe 95% 收敛在两个平台文件、有实测数据支撑的性能工程。

但存在**一个致命结构性缺口和四类系统性缺陷**：

1. **零 CI 护栏**：452 个测试从不自动运行（`.github/workflows/` 只有 release.yml），直接后果是已完成的结构化重构在一个月内大幅回流（`lib.rs` +68%、`gitee/conversation.rs` +395%）。
2. **安全链路薄弱**：自更新无任何签名/哈希校验即 `self_replace` 自身二进制；四个平台 token + AI api_key 明文存 0644 权限的 TOML；Chat 走硬编码内网 IP 的明文 HTTP。
3. **架构债集中在两处**：三个 Git provider 是三份平行实现（无 trait，helper 重复 8 函数 × 3 份且已产生行为分歧）；PR Review 控制器 4,385 行 god object（23 个 `Arc<Mutex>` + 6 个手写序列号），绕开了项目自建的 `usher-state-store`。
4. **性能问题可测可修**：离线启动最坏阻塞主线程 9 秒；diff 查找每按键全量物化行模型；AI Review 每个进度 tick 全量重建 PR 列表 model；Chat 虚拟化写完了但生产构建从未启用；Gitee 因 `Option::or` 误用每次多发 1–2 个必然浪费的 HTTP 请求。

下文按 缺陷详述（§2–§5）→ 亮点（§6）→ 优先级路线图（§7）组织。

---

## 1. 项目概况

| 维度 | 数据 |
|---|---|
| 手写代码 | Rust 62,745 行（135 文件）+ Slint 31,523 行 |
| Slint 生成代码 | `usher-ui` 生成 **474,296 行 / ~49 MB** Rust；`usher-ui-settings` 另 70,985 行 |
| workspace | 9 个 crate（另有幽灵空目录 `crates/usher-ui-workflow/`，未入 workspace、零文件） |
| 依赖 | Cargo.lock 852 个包，其中 **75 个包多版本共存**（base64 0.22+0.23、toml 0.8+1.1、objc2 全家桶等），且 native-TLS 与 rustls 并存 |
| 测试 | 452 个（inline 17,249 行 + 集成 1,184 行），测试代码占比 29.4% |
| CI | 仅 release.yml（tag 触发构建），**无任何 test/clippy/fmt 流水线** |
| 构建 | 8 个 profile 调优；Slint 增量：08-03 文档基线 66.7s，**08-27 实测 39.1s**（代码生成 7.9s + 编译生成 crate 28.5s + 主 crate 4.2s，几乎完全串行单核受限）；纯 Rust 增量 4.3s（健康）；`target/` 实测 31 GiB，其中 incremental 11 GiB 含 `usher_ui` **4 份重复 identity（~7.4 GiB）** |
| 提交 | 98 次提交（近 30 天 41 次），Conventional Commits 100%，单人高强度冲刺期 |

---

## 2. 安全缺陷

### S1.【高】自更新全链路无完整性校验，可被植入任意二进制
- `src/update/updater.rs:270-340,513`：下载 GitHub Release 资产 → 落盘 → 解包 → `self_replace::self_replace()`，**无 SHA256、无签名验签、无 digest 比对**。TLS 只保证通道；发布账号被盗、CI 产物被换、企业内网 TLS 中间人任一场景下应用会自动把恶意二进制写入自身。
- 加重项 1（资产误配）：`updater.rs:199-240` 匹配失败后兜底"找 Release 里任意 `.tar.gz`/`.zip`"，且 `contains("apple-darwin")` 分支无 `cfg!(target_os)` 约束——可能把错误平台的二进制装进去直接砖化。
- 加重项 2（TOCTOU）：`updater.rs:284-288` 临时文件路径 `temp_dir/usher_update_{version}.tar.gz` 完全可预测，`File::create` 跟随符号链接。
- 加重项 3（替换目标不可控）：`updater.rs:452-496` 递归找"第一个名叫 usher 的文件"作为替换源。
- **修复**：Release 发布 `SHA256SUMS` + minisign/cosign 签名，客户端内置公钥，先验哈希再验签；删除模糊兜底匹配（用 `self_update::get_target()` 严格匹配，匹配不到报错）；临时文件改用 `tempfile`（`O_EXCL|O_NOFOLLOW`）落到应用私有目录；归档内路径按约定精确查找。

### S2.【高】更新后重启必然失败（被自己的单实例锁挡住）
- `src/update/updater.rs:581-632` 先 spawn 新进程再 `request_shutdown`；旧进程仍持 `INSTANCE_LOCK_FILE` 排他锁（`src/single_instance.rs:37-53`），新进程在 `src/lib.rs:890-893` 判定"已有实例"直接退出。**用户观感：更新完应用消失。**
- **修复**：改为先释放锁/优雅退出，再由 detach 的 helper 延迟拉起；新进程侧对锁获取做有限重试（5×300ms）。

### S3.【高】凭据明文存储 + 非原子写 + 解析失败静默覆盖 = token 可被永久抹除
- `src/settings.rs:111-159,312-336`：GitHub/GitLab/Gitee/Gitee 企业 token 与 AI `api_key` 明文序列化进 `settings.toml`，`fs::write` 截断式非原子写，文件权限从未收紧（实际 0644）。
- `src/settings.rs:289-308`：`load_from_file` 反序列化失败仅打日志即**回落 `AppSettings::default()`**，下一次任意 `save()` 用默认值整体覆盖原文件——一次断电写坏配置即导致所有 token 静默丢失。
- 并发放大：`save()` 只持**读锁**（`settings.rs:313-316`），而 `save` 有 UI Timer、多处 `spawn_blocking` 至少四个并发来源（`lib.rs:156`、`settings.rs:570`、`artifact_preview/mod.rs:643,688`），并发 `fs::write` 同一路径可产出混合内容的非法 TOML，正好触发上述覆盖链路。
- **修复**：token 迁系统钥匙串（macOS Keychain / Windows Credential Manager），TOML 只留引用；落盘统一"写同目录 .tmp → fsync → rename"（`crates/usher-messages/src/storage.rs:39-46` 已有现成实现可复用）；解析失败时把损坏文件改名 `settings.toml.corrupt-<ts>` 并报错，绝不静默覆盖；`save()` 加写入互斥。

### S4.【高】Chat 明文 HTTP + 硬编码内网 IP + 用户消息进日志
- `src/constants/chat.rs:2`：`pub const RAG_API_URL: &str = "http://10.135.33.188:83/query"`——发布二进制携带硬编码内网地址，明文无鉴权。
- `src/chat/chat.rs:31`：`log::debug!` 把完整用户输入写进日志文件。
- **修复**：端点改 https 且入 settings 可配置；删除该日志或降级为只记长度。

### S5.【中】关闭清理用子串匹配 `forceTerminate`，可能误杀第三方应用
- `src/shutdown.rs:364-366,406-407`：按 `localizedName.contains("usher")` 强杀进程，任何名字含 "usher" 的无关应用都会中枪。
- **修复**：按 `bundleIdentifier` 精确匹配或维护自己 spawn 的 pid 集合。

---

## 3. 架构缺陷

### A1.【高】Git Provider 层是"三份平行实现"，而非"一个抽象 + 三个适配器"
- 无任何 `trait GitProvider`（全仓 grep 为空）；`src/git/mod.rs:20-466` 12 个入口函数各写一个三分支 `match repo_type`，新增 provider 要改 12 处；相同错误文案复制 4 遍且措辞不一致（`mod.rs:47-49,135-137,186-188,478-480` vs `52/140/484`）。
- 通用 helper 每家各写一份且**已分歧**：`string_field`/`nested_string`/`value_u32`/`value_to_id`/`header_u32`/`fallback_total` 等 8+ 函数 ×2–3 份；`value_u32` 的 GitHub 版缺字符串回退（`github.rs:1321` vs `gitlab.rs:1577`、`gitee/mod.rs:2368`），服务端返回字符串数字时 additions/deletions 静默为 0。
- 直接后果：正确写法约束不了错误写法——GitHub/GitLab 的计数回退用 `match ... None => ...await`（正确），Gitee 用 `Option::or(...await)`（bug，见 P1）；三家分页截断策略互不一致（见 C2）。
- **修复**：引入 `trait GitProvider` + `RepoContext` + `ProviderCapabilities`；新建 `src/git/json.rs` 上提全部共享 helper（统一采用带字符串回退的版本）；预计净删 250–300 行。这是多个高严重度缺陷的共同根因，建议紧急 bug 修完后尽快立项。

### A2.【高】PR Review 控制器是 4,385 行 god object，且绕过了自建的 state-store
- `src/workflow/pr_review/controller.rs`：`bind_pr_review` 单函数 753 行（`:226-979`）注册 42 个回调；结构体 23 个 `Arc<Mutex>`/`Arc<Atomic*>` 字段 + 6 个手写请求序列号；281 处 `ui.set_*`；诊断设施、测试 fixture（约 200 行）内联在生产 `open()` 主流程里；982 行单测同文件。
- 状态管理三轨并行：AI Review / Messages 走 `usher-state-store`（reducer + subscribe，规范）；PR Review 全裸 `Arc<Mutex>` + AtomicU64；Tag 又是另一套 `SharedWorkflowState`。同一类问题（请求作废/一致性/持久化）三处三种解法，每处都要重新证明正确性——下文 C 组多个竞态 bug 正是手写方案的直接产物。
- **修复**：把 PR Review 会话状态建模为 `ReviewState` + reducer 接入 state-store，序列号换 store 的 revision；`refresh_action_state`/`submit_review`/`dismiss_review`/`merge`/`revert` 五个同构方法（`:1800-2125`）合并为泛型 `run_action<T>`，直接减 250 行；fixture/诊断移 `#[cfg]` 隔离模块。

### A3.【高】零测试 CI，重构成果正在回流
- `.github/workflows/` 只有 release.yml；无 `cargo test`/`clippy`/`fmt --check`。
- 实证后果：`docs/architecture-structure-audit.md`（2026-07，声称"P1/P2 已全部完成"）与今日实际对比——`src/lib.rs` 635→1,066 行（+68%）、`gitee/conversation.rs` 194→961（+395%）、`chat/interactive.rs` 871→1,855（+113%）、`gitee/mod.rs` 2,263→3,595（+59%）。拆分完成但无守护，热点文件全数回涨；22 处 `allow(dead_code)` 无人清理；两套锁中毒策略并存无人统一。
- **修复**：一天工作量——`on: pull_request` 跑 `cargo test --workspace --locked` + `cargo clippy -- -D warnings` + `cargo fmt --check` + `cargo check --no-default-features --features renderer-femtovg`（后者同时守护 A5 的 feature 双实现漂移）。**投入产出比全场最高。**

### A4.【中】构建/编译速度：瓶颈是单入口生成的巨型 crate，且开发环境本身有三处"白白变慢"的问题

**2026-08-27 当日实测（M2 Pro，feature/pr-review 分支）**——比 `docs/build-performance.md` 的 08-03 基线（66.7s）已有改善（期间 `1ce3645 perf(ui): isolate Settings page compilation` 落地），但结构性瓶颈未变：

| 场景 | 耗时 |
|---|---:|
| 纯 Rust 修改（touch `src/lib.rs`） | 4.3s（健康） |
| Slint UI 修改（touch `ui/main.slint`） | **39.1s** |
| ├ Slint 代码生成（build script，opt-level 0 运行） | 7.9s |
| ├ 编译生成的 `usher-ui` crate（44.1 MB，较 08-15 基线 40.5 MB **+9%**，本分支 AI review UI 是主要增量） | **28.5s（73%）** |
| └ `usher` lib + bin | 4.2s |

user time ≈ wall time，链路几乎完全串行、单核受限——`ui/main.slint` 单入口 import 全部路由是根因。profile 调优已到头（A/B 实测对 Rust 侧改善 17%，对 Slint 侧仅 6%）。

**当日顺带发现的三个环境问题（零改动即可修）**：
1. **构建曾完全坏掉**：rustc 1.97.1 增量编译 ICE（`Found unstable fingerprints for evaluate_obligation`），须 `cargo clean -p usher` 修复。若近期体感"特别慢"，很可能是反复撞 ICE 后被迫大范围 clean 丢热缓存。留意复发。
2. **增量缓存重复 identity**：`target/debug/incremental` 11 GiB，`usher_ui` 有 **4 份**独立缓存（~7.4 GiB）——多个 feature/flag 组合共用 `target/debug` 互相踢缓存。
3. **IDE 未走 `ide` profile**：`.vscode/settings.json` 配了 `--profile ide` 但 `target/ide` 不存在——IDE 检查正往 `target/debug` 写缓存，正是第 2 条的成因之一。

**共享组件三倍编译**：`crates/usher-ui`、`usher-ui-settings`、`usher-messages` 三个 build.rs 各以不同根编译 `ui/` 目录，`GitHubScrollView`/`GitHubTheme`/`GitHubButton` 及内嵌资源在三份生成代码里各出现一次。

**修复（按性价比分级，详见路线图 #8/#26/#29）**：
- **零改动**：UI 迭代期用已有快路径 `./scripts/ui-check.sh`（~0.6s 校验）与 `./scripts/ui-live-preview.sh`（热重载，PR Review 页在监听图内）；核对 IDE 走 `ide` profile；受控清理重复 identity 与旧 release 产物。
- **低风险配置实验**：`[profile.dev.build-override] opt-level = 2`——slint-build 目前以 O0 运行，7.9s 代码生成预期压到 2–3s（削 UI 热路径 ~15%），文档从未测过，是现存最便宜的未尝试手段，按文档既有 A/B 协议验收。
- **工具链实验（可选本地路径）**：nightly 并行前端 `-Z threads=8`，唯一能直接作用于 28.5s 前端受限单元的手段（稳定化是 Rust 2026 项目目标）；与增量编译组合有已知问题，实验须隔离 target。
- **结构性**：按 Settings/ComponentContainer 已验证模式拆出 **PR Review 页**（相关 Slint ~1.2 万行，占根生成树 ~30%，拆出后 PR Review 迭代预期降到 10s 内）——注意 Workflow 拆分曾因跨 crate 桥接回归被完整回滚，必须走文档里的 8 条回归清单；公共设计系统抽成唯一 `as_library()` crate 供三方引用；`ui/main.slint`（382 属性 / 138 回调）的 PR Review 约 60 个透传属性收敛为 `global PrReviewState`。
- **不要再碰**（文档已否决/无收益）：repeater 压缩、`experimental-module-builds`、sccache/换链接器（链接仅 2.4s）、加 Cargo jobs（路径串行）；Slint 升级暂无红利（1.17.1 已是最新）。

### A5.【中】feature 双实现缺契约：`ai_review` 的 enabled/disabled 是手抄双份
- `pr_review` 做对了（`ui/features/pr_review/contract.slint` 143 行契约，两实现继承之，漂移即编译错）；但 `ui/features/ai_review/disabled.slint:5-51` 把 4 个 struct + 35 个属性/回调**逐字段手抄**自 `full.slint`。任一次构建只编译其中一份，漂移在默认 feature 的 CI 下完全不可见。
- **修复**：为 `ai_review`/`ai_review_settings` 补 contract.slint；CI 加 `--no-default-features` check（与 A3 合并落地）。

### A6.【低】workspace 残留与工具漂移
- `crates/usher-ui-workflow/` 空幽灵目录（回滚残留），删除。
- `crates/usher-ui/examples/ui_generated_size.rs` 缺 `ai-review` 库路径注册，默认根必然编译失败——而 build-performance.md 把该工具列为优化前提。修复后再启动 A4。

---

## 4. 性能缺陷

### P1.【高】Gitee：`Option::or` 实参立即求值，每次固定多发 1–2 个 HTTP 请求
- `src/git/gitee/mod.rs:317-320,410-417`：`commit_count_from_detail(&detail).or(count_pull_request_commits(...).await)`——`or` 的实参**无条件求值**，探测请求必然发出且串行叠加 RTT；PublicV5 探测还用 `per_page=100`（`:1597,1669`），为拿一个计数下载 100 条完整对象。GitHub/GitLab 同逻辑写法正确（`github.rs:308-313`、`gitlab.rs:403-412`），纯 bug。
- **修复**：改 `match`/`if let`；探测 `per_page` 降为 1 改读分页头；全仓 grep `.or(` + `.await` 审计。0.5 小时工作量。

### P2.【高】离线/弱网启动最坏阻塞主线程 9 秒才显示窗口
- `src/lib.rs:724` → `src/network.rs:13,41`：`setup_network_monitor` 在主线程 `block_on` 串行探测 3 个 URL（各 3s 超时），之后才 `ui.show()`（`lib.rs:956`）。
- **修复**：`network_online` 给乐观初值，首次探测移入后台线程（后续轮询本就在后台）。

### P3.【高】diff 查找：每按键全量物化行模型 + 冲垮 64 行缓存，全部同步在 UI 线程
- `ui/views/workflow/pr_review_workspace.slint:472-476` 无去抖；`src/workflow/pr_review/find.rs:29-60` 逐行 `row_data()` 穿过 `LazyPrDiffLineModel`——每行未命中即重建整行结构（token VecModel + SharedString 分配，`diff_model.rs:100-130`），缓存容量仅 64（`diff_model.rs:21`）必然全量抖动。
- 叠加：`find-matches` 写成每行属性绑定调 Rust 回调且每次 `ModelRc::new(VecModel::from(...))` 新分配（`pr_review_workspace.slint:3289` 等 4 处 + `find.rs:67-91`），查询变化时所有挂载行同时重建重复器。
- **修复**：搜索直接扫控制器已持有的 `Arc<PullRequestDiffFile>` 纯文本，绕开 Slint model；加 150–200ms 去抖；匹配结果一次算好按行查表，去掉绑定内回调。

### P4.【高】Chat 虚拟化只在测试 fixture 下启用，生产全量挂载
- `src/chat/performance.rs:46-49` + `src/chat/interactive.rs:626`：`virtualization_enabled = fixture_enabled()`，即须同时满足诊断开关 + `USHER_MEMORY_SCENARIO=chat-performance`。真实用户路径下整套虚拟列表（`crates/usher-virtual-list` + `VirtualLayoutCache`，535 行）是死代码，"加载更多历史"后每条消息完整实例化组件树。
- 同族问题：`ui/main.slint:342` 的 `chat-performance-optimized` 从未被 Rust 侧赋值（grep `set_chat_performance_optimized` 零命中），A/B 开关半边失效，基线跑分测的是混合状态。
- **修复**：虚拟化默认开启，与 fixture 注入解耦；补赋值或删除 Slint 侧 baseline 分支。一行级改动解锁已写好的整套设施。

### P5.【高】AI Review 每个进度 tick 全量重建 PR 列表与任务两个 model
- `src/ai_review/controller.rs:474-486,2116-2157`：每处理完一个 64 KiB diff chunk 就 dispatch 一次，订阅回调无差分地：重建任务 model（≤128 行全克隆）→ 全量读出 PR 列表逐行 clone → 整体替换 model。model 身份替换导致 ListView 重建、滚动/选中态丢失（项目自己在 `pr_review/controller.rs:3279-3282` 注释里明确写过这个反模式）；`latest_task_for_pr` 还是 O(行数×128) 反向全扫。
- **修复**：用 state-store 已有的 `subscribe_selector` 投影 `(number, status, finding_count)` 靠 `PartialEq` 去重；PR 列表改 `set_row_data` 原地更新；`progress_percent` 单独走轻量路径。

### P6.【中】串行请求与 N+1
- `src/git/github.rs:433-459`：action state 三个独立 GET 串行（同文件 `:614-619` 已有 `try_join!` 正确示范），用户打开 PR 多等 2 个 RTT；`gitlab.rs:501-507`、`gitee/mod.rs:663-685` 同。
- N+1：GitHub 列表页 `join_all` 无上限并发 21 个请求（`github.rs:203-244`），且全仓**无任何**重试/退避/`Retry-After` 处理（`http/mod.rs:147` 仅一句文案）——极易触发 GitHub secondary rate limit，然后被 C3 的静默吞错掩盖；Gitee 补头像 50 commit 的 PR 要 50+100 次请求（`gitee/mod.rs:1422-1557`）。
- GitLab Closed 页翻页 O(P²) 流量（`gitlab.rs:163-252`，每翻一页从第 1 页重拉全部前缀且串行）。
- **修复**：`tokio::join!` 并行化独立请求；`http` 层加指数退避重试 + rate-limit 头感知；`join_all` 换 `buffer_unordered(≤4)`；Gitee identity 补全加进程级 LRU；GitLab closed 列表在控制器层缓存归并结果。

### P7.【中】热路径反复深拷贝整个 AppSettings
- `src/settings.rs:339-341` `get()` 全量 clone；`configured_token`/`gitee_api_profile` 等在单个函数里被调 3 次（`gitee/mod.rs:1379-1409` 等），每个请求构建触发 1–2 次全量 clone + `Url::parse`。20 并发场景下仅构造请求就上百次深拷贝。
- **修复**：`SettingsManager` 提供 `with_git<R>(f: impl FnOnce(&GitSettings) -> R)` 细粒度访问；`RepoLocator` 解析时一次性缓存 api_profile/base_url/token。

### P8.【中】UI 线程同步 IO 与阻塞调用
- Chat 持久化：启动时主线程逐行扫全量 jsonl（`chat/interactive.rs:934` → `persistence.rs:165-181`）；发消息落盘也在 UI 线程（`interactive.rs:1264,1326`）。同模块"加载更多"已正确用 `spawn_blocking`，策略不一致。
- Settings/Project：6 个 `save_*` 走 UI 线程 Timer 回调里的 `fs::write`（`settings.rs:496-559`）；`write_projects` 同步（`project/controller.rs:240,430,454`）。
- Tag 工作流：`refresh_tag_context` 在 UI 线程 + 持 `shared_state` 锁内同步跑 git 子进程（`tag/controller.rs:1116-1135` → `:1915`）；同文件 `:301-349` 已有 spawn_blocking + 序号防抖的正确模板，纯路径遗漏。`inspect_latest_remote_tag` 还把同一条 `git ls-remote` 跑两遍（`:1965-1979`）。
- 日志无缓冲：`logger/debug_logger.rs:91-105` 直写文件无 BufWriter，fern 全局互斥使 UI 线程排队等后台线程的系统调用。
- **修复**：全部收敛到 `spawn_blocking`/专用写线程；日志加 BufWriter + 定期 flush。

### P9.【中】渲染层资源浪费
- 头像：无任何缓存，conversation/commits 两路各自下载解码同一 URL；`HashSet.take(32)` 迭代顺序随机导致每次打开加载到的头像子集不同（`avatar.rs:19-50`）；3s 超时丢弃**全部**已解码结果（`pr_review/controller.rs:1425-1427`）；同一作者 80×80 RGBA 每条时间线记录 `clone_from_slice` 一份（`timeline.rs:312-322`，100 条评论 = 2.56 MB 重复像素 + 30 张重复纹理）。→ 进程级 `BoundedCache<String, slint::Image>`，超时下沉到单请求。
- `visible:` 代替 `if`：855 行的 ArtifactPreviewPanel 无条件常驻（`ui/main.slint:1659`）；**每个可见 diff 代码行实例化一个隐藏 TextInput**（`content.slint:584`）；AI Review 面板用 `opacity:0` + 移出窗口隐藏，FemtoVG 下强制离屏 layer（`main.slint:2369-2387`）。→ 逐一改 `if`。
- Artifact Preview 宽度每变 1px 全文档 parley 重排、无逐块测量缓存、无 `live-resizing` 门控（`artifact_preview/virtual_layout.rs:131-160`）；PR diff 虚拟窗口实时拖拽逐帧全量重算 + `set_vec` 整体替换（`virtual_list.rs:59-84,150`）。→ 加 resize 门控 + `(block_id,width)` extent 缓存。
- 图片解码峰值内存翻倍：32 MB 解码结果再 `clone_from_slice` 一份（`artifact_preview/mod.rs:767-777`）。

### P10.【中】AI Review 请求无总超时 + 分块可跑 80 分钟
- `src/ai/agent.rs:169` `.timeout(Duration::ZERO)` 无总时长上限，仅靠 300s 空闲超时（`docs/ai-review-response-timeout-investigation.md` 记录的排障刚把它从 90s 提到 300s——治标）；非流式请求 + 64 KiB 分块串行（`ai_review/controller.rs:1255`），1 MB diff = 16 块 × 最坏 300s；`shutdown()` 只取消 chat 不取消 review（`controller.rs:695-699`），退出时最多 8 个长连接悬挂、最终状态丢失。
- **修复**：改 streaming（文档自己已建议）；加 `chunk_count × 单块预算` 的总超时；shutdown 先 cancel 全部 review 任务。

---

## 5. 正确性 / 实现质量缺陷

### C1.【高】依赖请求失败静默吞掉，可把"不可合并"误判为"可合并"
- `src/git/github.rs:454-469`：reviews 请求失败 `.unwrap_or_default()` → `aggregate_review_state` 返回 `""` → `ChangesRequested` 分支被跳过 → `can_merge` 可能为 `true`。**一次 403/429 就能让被 request-changes 的 PR 出现可点的 Merge 按钮。** 同类静默 `.ok()`/`unwrap_or_default` 还有 `github.rs:246`、`gitee/mod.rs:864-943,1056`、`gitlab.rs:281-287,716-735`。
- **修复**：影响写操作可用性的读取引入 `enum ReviewSnapshot { Known(..), Unavailable }`，Unavailable 时禁用 Merge 并明示"无法确认审查状态"；纯展示性降级保留但必须 `log::warn!`。

### C2.【高】分页静默截断：>100 文件的 PR 只显示 100 个且无任何标记
- `gitee/mod.rs:357-362,421`（diff 文件单页 100、硬编码 `too_many_commits:false`）；`gitlab.rs:700-707,1100-1105`（notes/commits 单页）；`gitlab.rs:396-398`（diff 第 30 页 break 不设截断标志）。Review 结论可能建立在残缺 diff 上。
- **修复**：统一 `load_all_pages(...) -> (Vec, truncated)` helper；`PullRequestDiffDetail` 加独立 `files_truncated`，UI 显示截断横幅。

### C3.【高】Conversation 时间线按字符串字典序排序，跨时区/空时间戳错位
- `src/git/model.rs:430,447`：`created_at` 字符串比较；GitHub 用 `Z` 后缀、Gitee 企业用 `+08:00`（`gitee/mod.rs:2666-2675` 测试数据可证），同一时刻排序错乱；企业 review 摘要 `created_at` 显式置空串（`gitee/mod.rs:1265-1266`）→ **全部 approve/reject 卡片被排到时间线最前**。
- **修复**：排序前 `parse_from_rfc3339` 归一化到 `DateTime<Utc>`，解析失败排末尾。

### C4.【高】聊天用户消息在请求失败/取消时永久丢失
- `src/ai_review/controller.rs:1622-1673`：user 消息先渲染进 UI，但 `complete_chat(...).await?` 失败或任务被 `tokio::select!` 取消时，`persist_chat_session` 永不执行——用户下次打开面板自己的提问连同上下文消失。
- 同族：配置/任务历史 `load_*` 解析失败静默 `unwrap_or_default()`（`config.rs:222-228`、`controller.rs:2775-2779`），随后被下一次写入覆盖，用户全部 review 规则静默清空且不可恢复（与 S3 同模式）。
- **修复**："追加 user 消息 + 立即持久化"与"请求回答"拆两步；load 区分 NotFound 与解析失败，失败时改名保留原文件。

### C5.【中】手写请求序号必然遗漏——两处��发现的竞态
- `pr_review/controller.rs:1160-1219`：`open()` 新 PR 时递增了 4 个序号**唯独漏了 `action_sequence`**，旧 PR 在飞的 `refresh_action_state` 响应会通过校验，把上一个 PR 的 can_merge/merge readiness 写到新 PR 头部。
- 同函数：手动 Reload 复用相同 PR number，`is_new_pull_request == false` 跳过失效逻辑，**评论区和 commit 列表刷新后仍是旧数据**（`:416-432,1377-1385`）。
- **修复**：短期补 `invalidate_all_pending()` 统一失效 + `open(force_refresh)`；根治靠 A2（迁 state-store revision）。

### C6.【中】更新器/关闭路径的锁中毒双标准
- 好模式已存在：32 处 `unwrap_or_else(|p| p.into_inner())`；坏模式并存：36 处 `lock().unwrap()`，集中在 `update/updater.rs`（8 处）、`shutdown.rs:89,208`、`toast.rs:156,174`、`dialog.rs:68`——updater 下载进度锁在 8 个回调里 unwrap，任一 panic 后所有后续更新级联 panic（release 下 `panic=abort` 直接终止）。
- 相邻问题：panic hook 在用户关闭 tracing 后直接 `return`（`tracing_logger.rs:306-310`），panic 完全无输出；任意后台线程 panic 都触发整应用关闭（`:357`）；启动失败经 fallback 以 `exit(0)` 结束（`shutdown.rs:301`），外部无法感知。
- **修复**：统一推广 `into_inner()` 模式（或迁 parking_lot）；panic hook 无条件先落盘、按线程区分处置；退出码按原因非零。

### C7.【中】macOS FFI 层："unsafe + panic + static mut" 三重叠加
- 27 处 `static mut` 裸 ObjC 指针全部在 `platform/macos.rs`（18）与 `tray.rs`（9）——edition 2024 下 `static_mut_refs` 是 deny-by-default lint；`tray.rs:237-478` 9 处 `Class::get(...).unwrap()`/`ClassDecl::new(...).unwrap()`（类名已注册即 panic）。
- observer/monitor 泄漏：`install_titlebar_settings_button` 幂等标志置位太晚（`macos.rs:1678` 注册 9 个观察者，`:1866` 才置标志，中间 5 个提前 return 点，且被 30ms 定时器反复调用约 40 次）——失败一次即观察者翻倍，resize 回调放大 N 倍执行；`remove_tray_macos` 不摘 observer/event handler/monitor；`create_view` alloc/init 只增不减。
- shutdown fallback 在 signal 线程/panic hook 线程直接调 AppKit（`shutdown.rs:283-303` → NSStatusBar/NSWorkspace），非主线程 UB。
- **修复**：迁 `objc2`（`Retained<T>` 自动 ARC + 类型安全）；observer 注册单独 `Once` 保护；token 保存并在 shutdown 成对注销；fallback 路径跳过 AppKit 或 dispatch 到主队列。

### C8.【中】喂给 AI 的 diff 有两处构造缺陷，直接导致 finding 行号错误
- `ai_review/controller.rs:2325-2337`：正文行内容以 `@@` 开头即被当 hunk 头输出（丢 `+`/`-` 前缀），此后行号与增删归属全部错位——而 `reveal_finding` 正按此行号定位。修复：删掉 `|| starts_with("@@")`，只信 `line.kind`。
- `crates/usher-ai-review/src/lib.rs:704-715`：单文件超 64 KiB 从任意行切块，第 2 块起无 `diff --git`/`@@` 头，模型无从知道行号归属。修复：切块补写文件头 + 合成 `@@` 位置提示，或按文件为最小分块单位。

### C9.【中】定时器竞态与 UI 细节
- Toast 单全局 visible + 每 toast 一个无 id 校验的定时器互相踩踏（`toast.rs:154-168`）；通知 hover 结束后双定时器竞争、横幅约 200ms 即消失（`notification.rs:497-530,887-911`）。修复：世代号。
- Tag 功能把字面 "TODO" 渲染给用户：`tag/controller.rs:181,263,414,578,842,1128-1131`——Remote 模式未实现，版本号字段直接显示 "TODO"。至少改成 "Remote mode not available"。
- `clamp_step_log` 超限后每行 O(32KB) 复制，万行输出 ≈ 320 MB 内存拷贝，且截断提示报的总字节数恒错（`tag/controller.rs:29-47,1777-1783`）。改环形缓冲。
- 时区硬编码 UTC+8 且每次调用重建 `FixedOffset`（`timeline.rs:383`），与同屏 tooltip 的本地时区不一致。
- 终端 PTY 只 `kill` 不 `wait` 不杀进程组（`terminal/session.rs:105-107`）：僵尸进程累积，shell 里的 `npm run dev` 等变孤儿继续占端口。

### C10.【低】其余实现债（择要）
- HTTP 错误 `Display` 丢弃已捕获的服务端错误正文，60 处 `.map_err(|e| e.to_string())` 让用户只看到裸 status（`http/mod.rs:57,120-130,162-174`）——Gitee 已被迫手工绕行（`gitee/mod.rs:2203-2216`）。加 `detail_message()`。
- GitHub Search 分页数未 clamp 1000 上限，第 101 页起必 422（`github.rs:110-155`）。
- HTTP 客户端初始化失败即 panic + `panic=abort` = 启动无提示终止（`http/mod.rs:29-31,851`）。
- 拦截器 RwLock 中毒后所有 HTTP 日志永久静默消失（`http/mod.rs:444-449`）。
- Chat 渲染四层缓存 key 全部只看 id/长度不看内容（`renderer.rs:20`、`interactive.rs:695`、`virtual_layout.rs:241`）——当前"整条追加"侥幸正确，任何流式/编辑功能上线即静默失效。加 `content_revision`。
- `commit_sha_references` 会把 `deadbeef` 这类纯十六进制英文词误判为 SHA（`gitlab.rs:962-982`）。
- 双 hunk 行高硬编码 52、gutter 宽度魔数跨 Rust/Slint 重复（`virtual_list.rs:257-263`）。
- 首条通知 id 为 0 与"0=未分配"约定冲突（`notification.rs:25,777`）。
- 2s 关闭看门狗可能截断主线程尾部 trace 写入（`shutdown.rs:312-323` vs `lib.rs:1016-1054`）。

---

## 6. 做得好的（重构时不要破坏）

1. **git 解析层零 panic**：10,907 行非测试代码 0 处 unwrap/expect，93 个真实响应解码测试撑着容错路径。
2. **无跨 await 持锁**：15 处疑似逐一核查全为误报，全部正确 scope；全用 `std::sync::Mutex` 借编译器禁止跨 await。
3. **syntect 高亮范式**（`pr_review/controller.rs:2190-2225`）：spawn_blocking + 序号守卫 + 双重校验 + invoke_from_event_loop 原子提交 + BoundedCache——全仓最规范的异步 UI 更新模板，应推广为标准。
4. **`pr_review/contract.slint`** 的特性开关契约模式、**`MarkdownVirtualLayoutBridge` 的 BufferedIncremental** 虚拟化、**`ChatMessageModelBridge` 的前后缀增量 diff**——都是正确方向，问题只是覆盖不全或未启用。
5. 文档与代码一致性罕见地好（architecture.md 声称的路径逐一验证存在）；性能工程有实测数据（6 个 benchmark 目录、A/B profile 数据、66.7s 关键路径拆解）。

**核心矛盾**：项目不缺工程能力，缺的是把能力固化下来的自动化（CI）和把正确模式推广到全部路径的一致性执行。

---

## 7. 优化路线图（按优先级）

### P0 — 本周内，低成本高回报（合计 ≈ 2–3 人日）

> 状态更新（2026-08-27）：P0 共 8 项，已全部完成并通过相关自动化检查。

| # | 事项 | 依据 | 工作量 | 状态 |
|---|---|---|---|---|
| 1 | 建 PR CI：test + clippy + fmt + `--no-default-features` check | A3/A5 | 1d | 已完成 |
| 2 | 修 Gitee `Option::or` 立即求值（3 处 match 化） | P1 | 0.5h | 已完成 |
| 3 | 修启动阻塞：网络首探移后台 + 乐观初值 | P2 | 1h | 已完成 |
| 4 | 修更新重启死锁（先释放单实例锁再拉起） | S2 | 0.5d | 已完成 |
| 5 | settings 原子写 + 写互斥 + 损坏不覆盖（复用 messages 的实现） | S3 后半 | 0.5d | 已完成 |
| 6 | Chat 端点去硬编码内网 IP、删用户消息日志 | S4 | 1h | 已完成 |
| 7 | 删幽灵目录 `usher-ui-workflow`；Tag UI 的 "TODO" 字符串换文案 | A6/C9 | 0.5h | 已完成 |
| 8 | 构建环境自检：确认 IDE 使用独立 `ide` profile 并生成 `target/ide`；清理 `target/debug/incremental` 重复 identity，确认旧 release 产物不存在；UI 迭代使用 `ui-check.sh` / `ui-live-preview.sh` 快路径；记录并规避 rustc 1.97.1 增量 ICE | A4 | 0.5d | 已完成 |

### P1 — 两周内，正确性与安全（合计 ≈ 5–7 人日）

| # | 事项 | 依据 | 状态 |
|---|---|---|---|
| 9 | 自更新加 SHA256+签名校验，删模糊资产兜底，临时文件用 tempfile | S1 | 暂缓（本批排除） |
| 10 | token 迁系统钥匙串（TOML 只留引用） | S3 | 暂缓（本批排除） |
| 11 | reviews/approvals 失败禁 Merge（`ReviewSnapshot::Unavailable`） | C1 | 已完成 |
| 12 | 分页截断显式化（统一 `load_all_pages` + `files_truncated` 横幅） | C2 | 已完成 |
| 13 | 时间线时间戳归一化排序 | C3 | 已完成 |
| 14 | 聊天消息先落盘再请求；load 失败不覆盖原文件 | C4 | 已完成 |
| 15 | `open()` 补 `action_sequence` 失效 + Reload 强刷（先打补丁，根治见 #28） | C5 | 已完成 |
| 16 | 统一锁中毒策略（36 处 `lock().unwrap()` → `into_inner()`）；panic hook 无条件落盘 | C6 | 已完成 |

### P2 — 一个月内，性能专项（合计 ≈ 6–8 人日）

| # | 事项 | 依据 | 状态 |
|---|---|---|---|
| 17 | diff 查找改扫原始数据 + 去抖 + 匹配结果查表化 | P3 | 已完成 |
| 18 | Chat 虚拟化生产启用；`chat-performance-optimized` 接线或删分支 | P4 | 已完成 |
| 19 | AI Review 订阅改 `subscribe_selector` 投影 + PR 列表 `set_row_data` 原地更新 | P5 | 已完成 |
| 20 | http 层加重试/退避/rate-limit 感知；串行请求 `join!` 化；`join_all` 限并发 | P6 | 已完成 |
| 21 | 头像进程级缓存（存 `slint::Image`）+ 单请求超时 | P9 | 已完成 |
| 22 | `visible:` → `if` 清理（ArtifactPreviewPanel、每行 TextInput、AI 面板 opacity） | P9 | 已完成 |
| 23 | UI 线程同步 IO 收敛 spawn_blocking（settings/chat/project/tag）；日志加 BufWriter | P8 | 已完成 |
| 24 | AI Review：diff 构造两处 bug（`@@` 误判、切块补头）+ 总超时 + shutdown 取消 review | C8/P10 | 已完成 |
| 25 | Settings 细粒度访问替代全量 clone；RepoLocator 缓存 api profile | P7 | 已完成 |
| 26 | 构建配置实验：`[profile.dev.build-override] opt-level = 2`（Slint 代码生成 7.9s → 预期 2–3s，削 UI 热路径 ~15%），按 build-performance.md 既有 A/B 协议验收；可选：nightly `-Z threads=8` 本地实验脚本（隔离 target，不入 stable 交付链） | A4 | 已完成（A/B 回退 40.1%，未采纳） |

> P2 提交复核（2026-08-29）：下列 10 条中断后的补充修复已完成，并已补回归测试。

| # | 复核修复 | 状态 |
|---|---|---|
| P2-R1 | 修复 `ai_review_response_ui` 缺少 Slint debug metadata 导致的 7 条 CI 失败 | 已完成 |
| P2-R2 | Chat 用户消息落盘失败时恢复输入并提示，不再静默丢失草稿 | 已完成 |
| P2-R3 | Chat 消息改为锁内准备、锁外落盘、锁内提交，避免持有 Session 锁执行磁盘 IO | 已完成 |
| P2-R4 | Chat 虚拟滚动边缘判断去重，并统一使用生产虚拟化开关 | 已完成 |
| P2-R5 | Project 快照获取与文件写入统一串行，成功提示延后到实际落盘完成 | 已完成 |
| P2-R6 | Gitee identity avatar 仅缓存成功响应，瞬时失败不再形成永久负缓存 | 已完成 |
| P2-R7 | GET/HEAD 默认有限重试并提供显式 opt-out，消除调用点遗漏 | 已完成 |
| P2-R8 | GitLab Closed/Merged 前缀按 IID 去重、刷新 total，显式第一页刷新绕过旧缓存 | 已完成 |
| P2-R9 | Conversation/Commits 正文先提交 UI，头像异步补载，不再阻塞内容 18 秒 | 已完成 |
| P2-R10 | HTTP 重试共享单一总预算；主限流等待超出预算时终止，不再产生约 92 秒最坏等待 | 已完成 |

### P3 — 一个季度内，结构性重构（需单独立项）

> 状态更新（2026-08-29）：5 项已完成；#29、#31 与 #32 已完成低风险前置或可控范围收敛，完整跨 crate UI 拆分、完整 `objc2` 迁移及剩余传递依赖去重留待各自的独立回归阶段。

| # | 事项 | 依据 | 备注 | 状态 |
|---|---|---|---|---|
| 27 | Git Provider trait 化 + 共享 json.rs（净删 250–300 行） | A1 | 是 C1/C2/P1 类 bug 的根因治理 | 已完成 |
| 28 | PR Review 控制器迁 state-store + `run_action<T>` 收敛 + fixture 隔离 | A2/C5 | 手写序号竞态一并消失 | 已完成 |
| 29 | UI 编译边界：按 Settings/ComponentContainer 已验证模式拆出 PR Review 页（~1.2 万行 Slint，占根生成树 ~30%）；公共组件抽 library crate；main.slint 状态袋拆 global | A4/A6 | 已补 `@ai-review` 契约/package 前置和生成体积工具路径；完整拆分仍须按 8 条 UI 回归清单独立验收。目标：PR Review 迭代 39.1s → 10s 内 | 部分完成 |
| 30 | ai_review feature 补 contract.slint | A5 | 与 #1 的 CI check 配套 | 已完成 |
| 31 | macOS FFI 迁 objc2；observer 成对注销；shutdown fallback 不碰 AppKit | C7 | observer 已成对注销，后台 shutdown fallback 已隔离 AppKit；完整 `objc2` 类型迁移待独立阶段 | 部分完成 |
| 32 | 依赖收敛：tokio/full 裁剪、消除 75 个重复版本、统一 TLS 栈 | §1 | 已裁剪 Tokio feature、把直接 base64 依赖对齐到 0.22，并确认应用 HTTP 为 rustls 单栈；`cargo tree -d` 仍有 55 个传递重复版本条目 | 部分完成 |
| 33 | AI Review 改 streaming 传输；会话文件驱逐时清理孤儿 `.json`（当前含完整 diff 永不删除） | P10/H4 | Review/Ask 使用 SSE 增量读取并保留 JSON fallback；驱逐同步清理会话文件 | 已完成 |
| 34 | 项目文档回写：更新 `architecture-structure-audit.md` 结论并加行数/复杂度门禁；把 2026-08-27 实测（39.1s 拆解、44.1 MB 生成体积、ICE 事件、重复 identity）回写 `build-performance.md` 作为新基线 | A3/A4 | 防止再次回流；旧基线（66.7s/48.98 MB）已失效 | 已完成 |

---

## 8. 结语

按投入产出比排序，**最重要的一件事是 P0-#1（建 CI）**：它成本一天，却是防止本报告中一半问题复发的前提——没有护栏，任何一次修复都会重演"重构完成 → 一个月后回流"的既有轨迹。其次是安全三件套（S1/S2/S3，自更新与凭据），它们与代码质量无关，是发布级软件的底线要求。性能问题（P 组）几乎全部有明确的、已在仓库内其他位置验证过的正确模式可以照搬，属于"把自己的最佳实践推广到全部路径"，风险低、收益可测。真正需要架构决策的只有 P3 的四项（provider trait、state-store 收敛、UI 编译边界、objc2 迁移），建议在 CI 与安全债清完后按季度节奏推进。

---

# 第二轮复审（2026-08-30，HEAD=73ae042）

> 方法：5 路并行只读复审（上轮 10 条 finding 修复验收 / provider 新架构 / AI Review SSE 与请求生命周期 / 残余债务盘点 / 自由挖掘），全部结论附代码位置证据；另实跑全量测试与门禁脚本核验。**本轮只审不改。**

## 9. 复审总评与硬事实

**总评**：P0–P3 四批的执行质量总体扎实——上轮 10 条 finding 验收 **6 条完全修复、4 条部分修复**，无一遗漏；provider trait 迁移完整（`mod.rs` 零 `match repo_type` 残留、外部调用 100% 走门面）；PR Review 的 typed generations 堵上了 C5 的两个竞态；fixture/诊断在 pr_review 侧已 `#[cfg]` 隔离；`lock().unwrap()` 生产代码已清零。但复审也发现：**修复本身引入了 1 个高危新缺陷（R2-1）**，SSE 改造有 3 处语义漏洞，新抽象层"语法收敛、语义欠账"（能力建模不足导致 4 处参数被静默丢弃），且**发布工程（签名/公证/自更新校验）仍是零**——这是 alpha 转 beta 前的硬门槛。

**硬事实**（2026-08-30 实测）：

| 指标 | 首轮（08-27） | 本轮 | 变化 |
|---|---|---|---|
| 全量测试 | 452 个 / 有 7 个失败 | **509 个 / 全绿（0 失败）** | ✅ |
| 手写 Rust | 62,745 行 | 67,287 行 | +4,542（provider.rs 560、json.rs 107 为主） |
| `src/git/mod.rs` | 810 行、12 个三分支 match | 689 行、**0 个 match** | ✅ 门面化完成 |
| `pr_review/controller.rs` | 4,385 行 / 23 个 Arc\<Mutex\> | 4,522 行 / 16 个 Arc\<Mutex\> + facade | 函数数 95 = 门禁预算 95，**余量 0** |
| `ai_review/controller.rs` | 3,164 行 | **3,730 行（+566，无门禁预算）** | ⚠️ 第二大热点失守 |
| 非测试 `lock().unwrap()` | 36 处 | **0 处** | ✅ |
| 非测试 `unwrap()`（严格口径） | — | 16 处（唯一高危：`tray.rs:238`） | |
| `allow(dead_code)` | 22 | 24（+2） | ⚠️ 趋势未被门禁覆盖 |
| `cargo tree -d` 重复包 | 75 | 47 | ✅ 部分收敛 |

### 9.1 上轮 10 条 finding 修复验收

| # | 项 | 验收 | 残留 |
|---|---|---|---|
| 1 | 7 个测试失败（Slint debug metadata） | ✅ 已修复，23/23 绿 | — |
| 2 | chat 落盘失败丢消息 | ✅ 已修复（恢复输入+提示+id 不推进，有单测） | 失败时用户已切模式则草稿不恢复也无提示（`interactive.rs:1476` 的 mode 判断），见 R2-8 |
| 3 | project 旧快照后写竞态 | ✅ 已修复（写锁内取快照，有并发回归测试） | 落盘失败内存不回滚（R2-14） |
| 4 | gitee 头像负缓存 | ✅ 已修复（Err 不写缓存，有单测） | — |
| 5 | gitlab 前缀缓存 | ⚠️ 部分：IID 去重/total 刷新/首页旁路已落实 | TTL 续命未解决；**去重引入新漏行 bug**（R2-6） |
| 6 | 头像捆绑内容上屏 | ✅ 已修复（内容先行 + set_row_data 补丁 + generation 守卫） | patch 对未变化行也无条件 set_row_data（R2-16） |
| 7 | HTTP 重试放大 92s | ⚠️ 预算收敛到 ~30s、403 主限流不再默认重试 | **修复引入高危新缺陷 R2-1** |
| 8 | chat 持锁 IO | ⚠️ 发送路径已"锁内准备/锁外 IO/锁内提交" | `load_more_history` 仍持锁读文件（R2-13） |
| 9 | 重试贴牌漏 4 处 | ✅ 已修复（GET/HEAD 默认重试 + no_retry 逃生口） | `no_retry` 零生产调用（与 R2-1 相关） |
| 10 | 边缘守卫复制+开关不一致 | ⚠️ 抽函数完成、两处统一到 virtualization_enabled | 同文件仍有 5 处 `variant().is_optimized()` gate 与之不一致 |

## 10. 第二轮新发现（按严重度）

### 10.1 高严重度（修复引入 / 改造遗漏的正确性问题）

- **R2-1【高·回归】重试改造把所有默认重试 GET 的总耗时（含正文下载）硬砍到 10 秒**——`src/http/mod.rs:1043-1046` 在 retry 生效时 `*request.timeout_mut() = Some(remaining.min(RETRY_ATTEMPT_TIMEOUT=10s))`，而 reqwest 0.13 的 per-request timeout 覆盖响应体读取。与"GET 默认重试"叠加后：`updater.rs:279` 显式配置 600s 的**更新包流式下载超过 10 秒必失败**；provider 的大 diff/文件 GET 从 30s 降到 10s；`read_idle_timeout` 对 GET 成为死配置。修复：仅在"还有后续尝试"时启用 10s 尝试上限、最后一次用完整剩余预算；`updater`/头像等长下载显式 `.no_retry()`。`docs/specs/http.md` 也未提及此上限（文档与行为不符）。
- **R2-2【高】SSE `finish()` 不校验 `[DONE]`/finish_reason**（`src/ai/agent.rs:148-162`）——连接在事件边界被干净关闭时，截断的响应被当完整成功：Review 侧报误导性 "invalid JSON"，聊天侧把半句话当完整答案落盘上屏。伴生问题：流中 `{"error":...}` 事件被 `#[serde(default)]` 静默吞掉（:191-197）；中途断流已收内容全部丢弃（:376-399）。
- **R2-3【高】聊天路径仍无总预算，且防护实际变弱**——`complete_chat` → `complete_review` 的 `.timeout(Duration::ZERO)`（agent.rs:295）关闭总截止；本次又删除了 `BodyProgressPolicy::NonWhitespace`，**任何 chunk（含 SSE 心跳注释行）都重置 300s 空闲计时**（http/mod.rs:824-849）——心跳不断的挂死连接两道防线全部失效，UI 永远 "Thinking..."。Review chunk 有 600s 墙钟，聊天没有。修复：聊天两处调用外包 `tokio::time::timeout`（一行级）；空闲计时改由 SSE 层在收到非空 delta 时刷新。
- **R2-4【高】`run_action` 让 5 类远程动作共用单一 `RemoteAction` 代际互相取消**（`pr_review/controller.rs:1925-2194`）——merge 成功后，diff 加载自动触发的 `refresh_action_state` 会作废 merge 的 completion：**远端已合并，但 UI 无 toast、readiness 不更新、PR 列表不刷新**。修复：读轮询与各类 mutation 拆分代际；只读轮询不应点亮 action loading（:1953）。
- **R2-5【高】`invalidate` 后 loading 永久卡住**——`code_loading=true` 在 spawn 前设置，completion 的 `is_current` 早退排在 loading 复位**之前**（controller.rs:2591/2757/2811）；"View file 在飞中点折叠上下文"即可复现 spinner 永挂。修复：loading 复位提到 `is_current` 检查之前。
- **R2-6【高】gitlab 去重修复引入分页误判**（`gitlab.rs:371-397`）——去重后 `entry.items` 可短于 `page_limit × page_size`，循环上界仍固定 → 未置 `exhausted` 却返回短页，上层把 `page_count` 算成当前页，**分页器误判"已是最后一页"**。循环条件应改按 `items.len() < prefix_len && !exhausted` 续取。
- **R2-7【高】能力未建模导致参数被静默丢弃**（provider.rs:315-319/490 等 4 处）——UI 有 "merge 后删除源分支" 勾选框且无 provider 门控，GitHub 勾了**不删也不提示**；dismiss 的 `review_id` 在 GitLab/Gitee 退化为"撤销自己的 approve"语义不同却无标记；两处 `ignore_whitespace` 同理。`ProviderCapabilities` 需从 2 bool 扩到 ≥6 维，UI 按能力门控。
- **R2-8【高】Windows 完全失守**——CI 只跑 macos-latest，所有 `cfg(windows)` 代码 PR 阶段零编译验证；且 `ai_review` 的 4 处原子写用裸 `fs::rename` 覆盖已存在文件，**Windows 上 AI Review 配置/任务状态保存必失败**（AlreadyExists）。修复：CI 加 windows `cargo check` job；原子写统一到 settings.rs 已硬化的 `atomic_write`（唯一实现含 ReplaceFileW/fsync，全仓其余 6 份都是弱化版）。
- **R2-9【高】发布工程三连缺**——release.yml 无 codesign/notarize（用户下载 DMG 被 Gatekeeper 判"已损坏"）；自更新仍零校验（P1-#9 暂缓项，`updater.rs:406-455` 直接解包替换自身）+ 临时解压目录名可预测（`/tmp/usher_update_extract_{pid}`，TOCTOU/本地提权面，半小时可修成 `tempfile::TempDir`）；`tray.rs:238` `CString::new(s).unwrap()` 在 FFI 边界 panic 属 UB（一行兜底可修）。
- **R2-10【高】终端 PTY 终止只 `kill()` 不 `wait()` 不杀进程组**（`terminal/session.rs:105-107`）——切项目路径反复触发：僵尸进程线性累积；shell 里的 `npm run dev` 等变孤儿继续占端口。
- **R2-11【高】网络监视器每 3 秒向 Google/Apple/Microsoft 发明文 HTTP 探测**（`network.rs:10-68`）——~20 次/分钟持续整个生命周期（隐私/企业内网硬伤）、离线时每轮 9s 无退避、无退出条件。应改系统可达性 API 事件驱动 + 指数退避兜底。

### 10.2 中严重度（一致性 / UX / 性能残留）

- **R2-12 错误呈现三通路未收敛（最大横切债）**：`user_message()` 分类良好但全仓仅 4 个调用点；git 层 62 处 `.map_err(|e| e.to_string())` 抹平结构化错误；8 处原始开发者串直接显示到 UI（含 `set_workflow_pr_merge_block_reason`）；`check_update.rs:108-120` 还有第三套 substring 匹配分类。建议 `Result<T, GitError>` 贯通 + UI 边界统一转 `user_message()` + `error_kind` 枚举驱动"去设置/重试"按钮。
- **R2-13 chat `load_more_history` 仍持 session 锁读文件**（interactive.rs:1319-1329）——慢盘大历史下 UI 线程同锁卡顿，R8 只治了发送路径。
- **R2-14 project 落盘失败不回滚内存/UI**；`persist` 在 guard 未 drop 前被调用空转等锁（controller.rs:430/455/473）。
- **R2-15 通知/Toast 定时器体系**：hover 结束 ~200ms 横幅即消失 + 定时器链泄漏（notification.rs:497-530/887-910）；Toast 单例无 token 互相踩踏（toast.rs:127-173）；首条通知 id=0 与哨兵冲突（:25，一行修）；队列每次变更整体重建 ModelRc（350ms 魔法延迟因此存在）。
- **R2-16 效率残留六件套**：selector 每 tick 双份全表 String 克隆（~1792 分配/tick，`TaskRowProjection` 换 SharedString 可顺带删掉 4 遍字段手写）；`PrefixCache` get/insert 双重深拷贝（Arc 化 ~20 行）；头像像素/Image 双层各 128 条 ~3MiB 纯冗余（像素层降 16 即可）；`reduce_tasks` 全量 clone + 新增全量深比较；头像 patch 对未变化行也 set_row_data；banner 订阅每次克隆最多 1000 个 id。
- **R2-17 SSE/生命周期次级项**：JSON fallback 无字节上限（流式有 16MiB，非流式无限）；会话文件驱逐是差集式非对账式（corrupt 恢复后旧文件永不清理）+ 三处 `.tmp` 失败残留不清理 + `session_paths` 字符过滤可致 id 碰撞；shutdown 先 drop 订阅再等取消，取消终态不落盘（下次启动误报 "interrupted"）；取消可留孤儿 user turn；`merge_conversations` O(n·m) 未修。
- **R2-18 门禁与 CI 缺口**：结构门禁 callback 正则漏 `pure callback`（报 128 实为 138，预算 140，真实余量 2）；`ai_review/controller.rs`（3,730 行）与 `content.slint`/`shell.slint`、`git/mod.rs`/`provider.rs` 均无预算；pr_review 函数数预算 95 卡死在现值会**惩罚拆分**；`check-ui-policy.sh` 未接入 CI；`rg` 零命中时脚本会崩；doctest 被排除且有已知失败在腐烂；无 cargo audit；push 直推不触发 CI。
- **R2-19 provider 层遗留**（上轮点名未修的 6 项确认仍在）：GitHub Search 1000 上限 page_count 未 clamp（第 101 页起 422）；`commit_sha_references` 连纯数字（工单号/时间戳）都会误判为 SHA；User-Agent 实为 5 套且 GitHub 4 处不带版本；`X-GitHub-Api-Version` 硬编码且仅 1 个请求带；gitee 公开版 merged/closed 过滤仍是每页 O(30×100) 全扫描（GitLab 的 PrefixCache 未共享给它）；`overview.rs:26` 环境计数循环无最大页保护。加上新债：trait 默认 `revert` 里残留 `match repo_type` + 永不可达的第三份文案；"PR 号为空"校验文案复制 8 份；`RepoContext` 承载 discover/saved 两种状态无类型区分；capabilities() 每次穿透配置读取未缓存；provider.rs 新测试读真实用户 settings.toml 非 hermetic；`RepoLocator` 的 `Debug` 可能泄漏 URL 内嵌凭据。
- **R2-20 启动路径无度量**：`ui()` 首帧前串行 ≥6 次磁盘 IO+图片解码（settings/messages/projects+logo 解码/chat/ai_review×2），11 个内存检查点但**零时间检查点**；窗口揭示固定 4×30ms 拍 + 最坏 1200ms 死等；`main()` 里 `ui().unwrap()` 启动失败零提示（messages.json 损坏即静默闪退，应降级内存态 + NSAlert）。
- **R2-21 单实例无唤起**：关闭=隐藏到托盘 + 第二实例只 eprintln 退出 → Windows/Linux 用户再次启动"什么都没发生"。

### 10.3 低严重度 / 长期

- **R2-22 i18n 为零，且无语言设置**：无 `@tr()`/gettext，全部 UI 文案硬编码英文，而文档/注释是中英双语；`AppSettings` 没有 language 字段，Settings 页没有语言选择器——用户完全无法切换语言。Slint 1.17 原生支持 `@tr()` + 打包翻译（可运行时切换）。**已确认立项，方案见 P4-D 专项。**
- **R2-23 无障碍覆盖 ~1/3**：`accessible-*` 106 处 vs `clicked` 292 处 + TouchArea 117 处；建议做成 ui-policy 软规则，先覆盖 PR Review 工作区与导航。
- **R2-24 测试结构**：82% 是 src 单测，核心旅程（打开 PR→diff→评论→merge）零端到端；`updater.rs`/`terminal/mod.rs`/`hunk.rs`/`git/model.rs` 等大文件零单测（后两者纯函数密集、补测成本最低）；7 个 benchmark 脚本+对比工具无 CI 化（前两轮性能成果无回归防线），6 个 results 目录散落仓库根。
- **R2-25 可运维性**：日志无轮转/保留策略（单文件无限增长）、无"打开日志目录/导出诊断包"入口、无崩溃哨兵（"上次异常退出"检测）；info/debug 级别错配散见。
- **R2-26 模式债**：39 处 `Timer::single_shot` 时序 hack（1/10/16/50/100/350/500ms 躲 RefCell 重入），应收敛为 `defer_to_next_loop_turn` 辅助；`settings` 的 `schedule_settings_save` 是伪防抖（N 次调用 N 个定时器 N 次 fsync），且 preview 两个 save 在 UI 线程同步 fsync；state-store 仅 3 域采用，project（4 个 pending 集合）收益最大应优先迁入，"store+持久化 worker+observe_commits" 三处重复应抽 `PersistentStore<T>`；路由生命周期钩子只有 workflow 一个事件，Chat/PR Review/Terminal 的离场释放无统一落点；`usher-state-store` observer 持非重入锁回调无重入防护；BoundedCache 4 份实现 3 种驱逐语义待泛化收敛；terminal `ENABLE_TERMINAL_SPAN_RENDER` 恒真常量留 7 处死分支；bundle 元数据模板化、updater 无 alpha/stable channel 概念。

## 11. 第四批（P4）路线图

### P4-A — 本周：修复引入的回归与一行级高危（≈2 人日）

| # | 事项 | 依据 | 状态 |
|---|---|---|---|
| 35 | 修 R2-1：10s 尝试上限只作用于非末次尝试；updater/头像长下载显式 no_retry；回写 docs/specs/http.md | R2-1 | 已完成 |
| 36 | 修 R2-6：gitlab 去重后按 `items.len() < prefix_len` 续取；顺带修 TTL 续命（扩展不刷新 updated_at） | R2-6 | 已完成 |
| 37 | 修 R2-4/R2-5：RemoteAction 拆读轮询与 mutation 代际；loading 复位提到 is_current 之前 | R2-4/5 | 已完成 |
| 38 | 聊天路径包总超时（两处一行级）；SSE finish 校验 [DONE]/finish_reason + error 事件透出 | R2-2/3 | 已完成 |
| 39 | 一行级三连：tray CString 兜底、通知 id 从 1 起、updater 临时目录换 tempfile | R2-9/15 | 已完成 |
| 40 | CI 加 windows-latest `cargo check` job + 接入 check-ui-policy.sh + 修门禁 pure callback 正则 + 补 ai_review/controller 预算 | R2-8/18 | 已完成 |

状态更新（2026-08-30）：P4-A #35–#40 已全部实现，并通过定向回归与全量验证。

### P4-B — 两周内：一致性与守护（≈5–6 人日）

| # | 事项 | 依据 | 状态 |
|---|---|---|---|
| 41 | 抽 `usher-fs` 原子写（settings 版为唯一实现），7 处调用点统一；settings 文件 chmod 0600（#10 止血） | R2-8/B12 | 已完成 |
| 42 | 错误贯通：`GitError` 替代 `Result<T, String>`，UI 边界统一 user_message + error_kind 按钮 | R2-12 | 已完成 |
| 43 | ProviderCapabilities 扩 6 维 + UI 门控（merge 删分支勾选框）+ RepoContext/ProjectContext 拆分 + 删 trait 默认 revert 的 match | R2-7/19 | 已完成 |
| 44 | 通知/Toast 世代号 + 队列增量更新（删 350ms 魔法延迟）；network.rs 改事件驱动/退避；PTY wait+killpg | R2-10/11/15 | 已完成 |
| 45 | ai_review fixture ~470 行照 pr_review 模式 `#[cfg]` 隔离；驱逐改对账式 + 清 .tmp | R2-17 | 已完成 |
| 46 | 效率六件套（SharedString 投影/PrefixCache Arc/头像像素层降容/reduce_tasks 免克隆/patch 跳过未变行/banner 门禁） | R2-16 | 已完成 |
| 47 | chat load_more_history 锁外读；project 失败回滚；mode 切换时的草稿保留 | R2-13/14/8 | 已完成 |

状态更新（2026-08-30）：P4-B #41–#47 已全部实现，并补充定向回归覆盖；最终全量验证见本批次交付记录。

### P4-C — 一个月+：产品化（beta 门槛）与结构

| # | 事项 | 依据 | 状态 |
|---|---|---|---|
| 48 | **发布工程**：codesign+notarize、SHA256SUMS+minisign、自更新强制校验、alpha/stable channel、崩溃哨兵、日志轮转+诊断包导出 | R2-9/25 | 部分完成（签名/公证逻辑已保留但默认关闭，设置 Actions variable `MACOS_SIGNING_NOTARIZATION_ENABLED=true` 后恢复；channel、哨兵、轮转与诊断包已完成；SHA256/minisign/自更新强制校验按用户要求暂缓） |
| 49 | 启动时间轴埋点 → 数据驱动延后初始化（logo 解码/task_state/messages 首帧后补）；main() 失败原生告警；单实例唤起通道 | R2-20/21 | 已完成 |
| 50 | 核心旅程 3 条集成测试 + hunk/render/model 纯函数单测 + benchmark nightly 化（阈值断言） | R2-24 | 已完成 |
| 51 | state-store 推广：project 迁入 + `PersistentStore<T>` 门面 + pr_list 裸 AtomicU64 迁 generations + 路由生命周期钩子泛化 | R2-26 | 已完成 |
| 52 | 无障碍 ui-policy 软规则；`defer_to_next_loop_turn` 收敛 39 处定时器 hack；BoundedCache 泛化收敛（i18n 已拆出为 P4-D 专项） | R2-23/26 | 部分完成（新增统一 next-turn helper；调用点收敛与其余事项待处理） |
| 53 | provider 遗留清尾：Search 1000 clamp、SHA 误判、UA/API 版本集中、gitee 复用 PrefixCache、overview 页数上限、测试 hermetic 化、Debug 脱敏 | R2-19 | 已完成 |

### P4-D — i18n 与语言设置专项（已确认立项，合计 ≈ 4–6 人日 + 持续翻译维护）

> 立项时现状：零 i18n 基建（全仓无 `@tr()`/gettext 命中）；`AppSettings` 无 language 字段；Settings 页无语言选择器；用户文案分散在两处——Slint 文件（约 32k 行 UI）与 Rust 侧动态构造的字符串（toast/notification/错误提示/状态文案）。
>
> **技术选型（建议）**：Slint 侧用 `@tr()` + **打包翻译（bundled translations）**，而非运行时 gettext——打包方案不引入 gettext 运行时依赖、跨三平台一致，且 `slint::select_bundled_translation("zh-CN")` 支持**运行时切换语言**（bindings 自动重估，无需重启）。Rust 侧用轻量 catalog（同一套 `.po` 文件驱动），保证两侧共用一份翻译资产。

| # | 步骤 | 内容 | 工作量 | 依据/风险 |
|---|---|---|---|---|
| 54-1 | **基建接线** | 三个编译 Slint 的 crate 统一消费由 `usher-i18n` 从根级 `i18n/` 资产生成的类型化词典和索引一致 bundle；UI check 同步生成 `@i18n` library | 0.5d | **已完成**；修正了原方案“各 crate 独立提取会产生全局索引冲突”的风险 |
| 54-2 | **语言设置** | `AppSettings.language` 持久化 `auto` 或 manifest locale id；设置页使用 manifest 驱动选择器，系统 locale/别名/回退统一解析，运行时即时切换 | 1d | **已完成**；新增语言无需修改 UI/Rust 分支 |
| 54-3 | **Slint 文案批量迁移** | 所有文案改为生成的语义键属性，分页面迁移 Settings、主 Shell、PR Review、Chat、通知等 | 1.5–2d | **已完成**：生产 Slint 文案与 Tooltip 已接入生成 catalog；已人工验收 Home、Settings、Projects、Messages、Chat、Workflow、Tag 与 Terminal 的中英文即时切换，并修复 Chat 时间仍固定中文的问题 |
| 54-4 | **Rust 侧文案收敛** | Rust 用户文案在 UI 边界通过同一 catalog 查询，日志与协议文本不翻译 | 1d | **已完成**：动态状态保留语义 key/`LocalizedText` 并在展示边界解析；Provider/PTY/文件系统技术错误只进入日志 |
| 54-5 | **守护与验收** | 构建校验键/别名/陈旧 PO/source drift/覆盖率；CI 禁止手写 `@tr()`；跨 MainWindow/Settings/Messages bundle 索引回归测试 | 0.5d | **已完成（外部数据受限）**：catalog、命名占位符、共享 bundle 与生产 Slint 硬编码门禁已落地，可访问路由已完成视觉验收；企业 Provider 当前不可达，PR Review 真实数据页仍需在网络恢复后补一次人工数据验收 |

> 里程碑切分建议：54-1 + 54-2 + 54-3（仅 Settings 页与通知/Toast 先行翻译）可作为第一个可见交付——"语言设置可用、设置页全中文"；其余页面文案随 54-3/54-4 分页面推进，避免一次 32k 行 Slint 的大爆炸式改动。

## 12. 第二轮结语

两轮下来，项目已经从"工程能力强但无护栏"走到"护栏建立、核心债清偿过半"。本轮最重要的三个信号：**其一**，修复也会引入回归（R2-1 是 P2 复核修复自己造成的，且规格文档未同步）——这印证了 P4-A 把"修复的修复"放在第一位的必要性，也说明 spec 与实现的同步应成为 review checklist 项。**其二**，抽象层的价值在语义不在语法：provider trait 迁移干净利落，但真正的 provider 差异（删分支、dismiss 语义、能力矩阵）还躺在 `_` 参数里——下一步的重心是把 capabilities 做实。**其三**，纵向功能债已收敛，剩下的大头是横切面（错误呈现、i18n、无障碍、可运维性）与发布工程——这些决定 alpha 能否变成可分发的 beta，建议 P4-C 的 #48 作为下一个里程碑的锚点。

---

# 第三轮性能与架构复审（2026-09-05，HEAD=2ff3f3b）

## 13. 范围、结论与证据边界

本轮对照前两轮提案、架构文档与相关 spec，核查 Project、Job/Tag、Artifact Preview、Diff Find、State Store、AI Review 持久化和 CI 路径。记录于 2026-09-07；本次归档不代表实施或重新运行性能基准。

下一轮应优先处理 **后台任务的有界性、Store 到 UI 的增量更新，以及大内容的单次计算预算**。现有 HTTP、虚拟化和 State Store 基础可以继续复用，但新增功能未完整继承这些约束。迁入 Store、限制最终日志长度、减少可见 block 数，都不能单独保证整条链路的性能上限。

- 以下 R3-1 的关闭信号消费问题已通过相同通道合并逻辑的独立最小程序复现；这不是完整应用退出测试。
- 其余发现基于列出的代码路径。复杂度、积压和卡顿风险由实现推导，尚无本轮 Release 延迟、峰值内存或加速比例实测。
- 代码位置以 `2ff3f3b` 为基线，后续实施应按符号重新定位；不把旧批次已完成项全部重新标记为未完成，也不声称相关 spec 已全部重新验收。

## 14. 第三轮发现

### R3-1【高·新发现】AI Review 持久化 Worker 可能吞掉关闭信号

**证据**：`src/ai_review/controller.rs:3483` 的 `TaskPersistenceWorker` 使用 `while let Ok(Some(newer)) = receiver.try_recv()` 合并快照；同文件 `shutdown()` 发送 `None` 后等待 Worker `join()`。

**触发与影响**：当合并循环读到 `None` 时，关闭信号被消费，但只退出内层循环。外层随后再次等待 `recv()`；关闭调用仍持有 sender 并等待 `join()`，可能使退出流程无法完成。独立最小复现按顺序发送两个快照与关闭信号，确认合并后接收端进入等待，而不是保留关闭命令。

**建议**：优先修复控制消息处理，使用显式 `Persist / Shutdown` 命令，保证合并快照时不丢失控制消息。评估复用通用 `PersistentStore` 的 Worker 协议，并保留 AI Review 自有的会话对账职责，避免继续维护另一套有差异的关闭逻辑。

**验收**：覆盖空闲关闭、存在待合并快照时关闭、正在写盘时关闭；验证最后一个约定持久化 revision 落盘且 Worker 在测试期限内退出。

### R3-2【高·新发现】Project Logo 下载失败形成订阅驱动的自动重试循环

**证据**：`src/project/controller.rs:95` 订阅完整 Project State 并调度刷新；`:433` 的 `refresh_project_items()` 同时投影 UI 和启动预取；`:681` 的预取失败分支调用 `finish_remote_logo()`。`src/project/logo.rs:279` 的单次下载虽设置 `.no_retry()`，但没有跨请求的失败冷却。

**触发与影响**：下载失败 → 清除 pending 并提交 Store → 订阅刷新列表 → 缓存仍缺失 → 再次下载同一 URL。快速返回 404 的地址可能持续触发请求、状态复制和 UI 刷新；单请求的 no-retry 不能阻止该循环。

**建议**：资源状态明确区分 Pending、Ready、Failed；失败状态带有限退避、重试预算和手动重试入口，避免永久负缓存。整个 Logo 预取器设置并发上限，并把请求启动从 UI 投影函数中移出。

**验收**：用本地可控的 404、超时、恢复成功响应验证请求次数有上限、冷却生效且恢复后可重试；批量项目加载时验证并发上限和页面响应。

### R3-3【高·旧批次残留】Project Store 更新仍触发全表重建及 UI 同步 I/O

**证据**：`src/project/controller.rs:95` 订阅完整 State，`:433` 每次刷新替换整个项目 `VecModel`；`src/project/state.rs:263` 的 reducer 克隆完整 State，保存、Logo pending 和 PR overview 标记都进入同一状态。`src/project/logo.rs:76` 的图片缓存未命中路径同步读取/解码；`src/project/controller.rs:450` 的诊断统计在诊断开启时读取图片文件。

**触发与影响**：逐项到达的 Logo/PR 结果会反复扫描和转换全部项目，多个项目同时加载时总处理量可能接近二次增长。搜索 callback 同样同步刷新列表；图片冷缓存、慢盘或开启诊断时，文件读取与解码会进一步占用 UI 线程。

**建议**：沿用 #19 的 selector 与增量行更新方式，按稳定项目 ID 更新变化行，合并同一轮 UI 通知；pending 等调度状态不应无条件触发完整展示投影。后台准备 Logo 与诊断元数据，UI 只接收准备好的结果；评估 State 的结构共享，避免每次 bookkeeping Action 深拷贝整个项目集合。

**与旧批次关系**：#51 已完成 Project Store 迁移，但迁移后的投影成本仍需治理；同步 I/O 是 #23 的残留，不能仅凭部分调用改为后台执行就视为全路径完成。

**验收**：批量项目、逐项 overview 完成、冷/热 Logo 缓存和连续搜索；检查未变化行的 model 身份、单次事件刷新数，以及 UI 线程是否仍执行文件读取/图片解码。

### R3-4【高·新发现】Job 日志长度有界，但读取队列和 UI 行数没有完整上限

**证据**：`src/workflow/tag/controller.rs:1819` 使用 `read_until('\n')` 累积一整行，`:1960` 使用 `unbounded_channel`；最终日志通过 `MAX_STEP_LOG_BYTES` 限制为约 32 KiB。约每 50ms 的进度回调复制日志和 Step 快照，`src/jobs/model.rs` 的 `JobStepData::to_slint()` 对已请求日志重新解析所有行；`src/jobs/mod.rs:103` 重建全部 Step model，`ui/components/jobs/job.slint:361` 普通 repeater 挂载全部日志行。

**触发与影响**：超长无换行输出可在截断前占用大量内存；生产速度超过消费速度时，无界队列仍可积压。约 32 KiB 的大量短行也能对应上万行控件，展开后持续输出会反复解析和重建这些行。现有节流限制了更新频率，没有限制每次更新成本。

**建议**：采用有界分块读取、有限队列和明确的超限策略；后台增量解析日志，UI 只追加/淘汰变化行，接入现有虚拟列表。调整队列时同时检查子进程退出后的排空顺序，避免等待 reader 完成却不再消费有限队列。

**验收**：大量短行、超长单行、stdout/stderr 同时突发、输出期间展开/折叠和 Stop；测量队列峰值、挂载行数、更新耗时，并验证尾部日志与退出行为。

### R3-5【高·新发现】Preview block 虚拟化未覆盖大代码块和全篇排版成本

**证据**：`src/artifact_preview/mod.rs:563` 在 UI callback 调用布局桥；`src/artifact_preview/virtual_layout.rs:131` 在文档/宽度等布局键变化时遍历全部 block，并对正文执行 shaping。`ui/components/artifact_preview/github_artifact_preview.slint:252` 在一个代码 block 内全量创建行和 token 控件。

**触发与影响**：拖动期间冻结布局已有价值，但松手后的首次排版仍可能在 UI 线程执行大量工作。外层只挂载少量 block，并不能限制一个包含数万行代码的单独 block；源文件字节上限也不能代替 UI 节点预算。

**建议**：优先增加代码块的行级虚拟化；用可传输文档快照将正文测量移到 Worker，以文档 revision、宽度和字体标识拒绝过期结果。保留现有精确 extent 与滚动几何契约，不通过退回粗略高度估算掩盖排版成本。

**验收**：单个大代码块、大量小段落、中英文混排、首次打开和 resize 松手；分别测量准备耗时、UI 提交耗时、挂载节点数与滚动位置稳定性。

### R3-6【中·旧优化的后续阶段】Diff Find 仍在 UI 线程全量扫描与建立索引

**证据**：`src/workflow/pr_review/controller.rs:976` 的查找 callback 同步调用 `collect_diff_find_matches()` 并重建 `DiffFindRowTable`；`src/workflow/pr_review/find.rs` 已改为扫描 Lazy model 的原始数据，避免物化可见行缓存，但仍扫描整个所选文件并收集全部匹配。

**触发与影响**：#17 的原始数据扫描和去抖修复有效；去抖只减少调用次数，没有限制单次计算。大文件或单字符高命中率查询仍可能造成一次较重的扫描、分配和索引构建。

**建议**：后台扫描和建立索引，用查询代际取消旧任务；UI 提交当前查询结果并只投影可见行高亮。保持现有搜索 spec 的行为：输入更新匹配但不滚动，只有显式导航才定位。

**验收**：大文件、高命中率、Unicode、快速连续输入和切换文件；记录输入延迟和旧任务取消行为，同时验证计数、顺序、高亮和导航不回归。

### R3-7【中·新发现与旧残留】持久化和 UI 通知缺少端到端积压上限

**证据**：`crates/usher-state-store/src/lib.rs:419` 的通用持久化 Worker 入口使用无界 channel，读取时才合并快照；写盘阻塞期间历史 `Arc<State>` 仍可能积累。Store listener 内的合并不等于合并已提交到 Slint event loop 的闭包，例如 Project 每个 commit 都可排队一次刷新。`src/settings.rs:708` 的 `schedule_settings_save()` 仍为每次调用创建定时器与写盘任务。

**建议**：对只关心最终状态的路径提供最新快照槽，以 revision 定义 Flush/Shutdown 屏障，保证资源上限且不破坏持久化顺序；UI bridge 同样只保留一个待执行刷新并消费最新快照。需要逐条保留的事件不能使用这种丢弃中间状态的策略。Settings 改为真正可合并的保存调度，并纳入关闭 flush。

**与旧批次关系**：通用 Worker 的积压边界是本轮补充；Settings 伪防抖是 R2-26/#52 的未完成残留。

**验收**：模拟慢持久化与高频 commit，断言待处理快照数量有界、最终 revision 正确落盘、flush 不提前完成；模拟 UI 暂时繁忙，确认恢复后刷新次数受控且状态为最新值。

### R3-8【中·治理回流】结构门禁失败，性能 CI 仍只覆盖 Home 静态内存

**证据**：本轮实际运行 `scripts/check-architecture-structure.sh`，以下 8 项超过现有预算：

| 检查项 | 实际值 | 预算 |
|---|---:|---:|
| `src/lib.rs` 行数 | 1113 | 1100 |
| PR Review controller 行数 | 5351 | 4600 |
| PR Review controller 函数数 | 107 | 95 |
| Chat controller 行数 | 2650 | 2200 |
| Chat controller 函数数 | 76 | 70 |
| AI Review controller 行数 | 4019 | 3850 |
| AI Review controller 函数数 | 124 | 120 |
| 根 Slint callback 数 | 146 | 140 |

`.github/workflows/ci.yml` 仅由 PR 和手动触发；`.github/workflows/nightly-performance.yml` 已建立 nightly，但实际基准只覆盖 Home 静态内存。因此 #50 的基础设施成果不能视为 Job、Preview、Project 与 Find 热路径已有性能回归保护。

**建议**：PR Review 按加载、远程动作、查找展示拆分；AI Review 按调度、会话、持久化拆分；Chat 按请求、历史分页、视口控制拆分。验收依赖方向和职责归属，行数作为提醒，不以机械切文件或单纯抬预算替代治理。CI 增加主分支 push 检查，分阶段纳入上述热路径基准，记录 UI 延迟、队列峰值和挂载控件数。

**验收**：结构门禁通过；职责和依赖边界经复核；新增基准有固定 fixture、原始数据、可重复阈值和失败证据。正式阈值须由 Release 基线测量确定。

## 15. 第五批（P5）建议路线图

以下沿用旧任务编号从 #55 继续；#55–#61 已完成实现及逐项 A/B，见第 17–19 节；#61 的键盘导航手工验收缺口见第 19 节。#62 已通过职责拆分与两场景 A/B；#63 持续门禁已接入，五场景 Release 对照通过，完整 CI 的已有测试问题见第 20 节。

| 阶段 | # | 事项 | 依据 | 状态 |
|---|---|---|---|---|
| P5-A：退出与请求循环 | 55 | 修复 AI Review Worker 关闭信号消费；补排队/写盘中关闭回归，收敛控制消息协议 | R3-1 | 已完成；三组 Release A/B 通过 |
| P5-A：退出与请求循环 | 56 | Logo 失败退避、重试预算、并发上限；分离 UI 投影和预取启动 | R3-2 | 已完成；三组 Release A/B 通过 |
| P5-B：数据与 UI 更新链路 | 57 | Project selector、稳定行身份、通知合并及后台 Logo/诊断准备 | R3-3；#23/#51 后续 | 已完成；常规/诊断各三组 Release A/B 通过 |
| P5-B：数据与 UI 更新链路 | 58 | Job 有界流读取、队列与增量解析；日志行虚拟化 | R3-4 | 已完成；长行/短行各三组 Release A/B 通过 |
| P5-B：数据与 UI 更新链路 | 59 | 通用最新快照调度及 revision 屏障；Settings 真正合并保存 | R3-7；R2-26/#52 残留 | 已完成；三组 Release A/B 通过 |
| P5-C：大内容计算预算 | 60 | Preview 代码行虚拟化、后台精确测量与过期结果取消 | R3-5 | 已完成；大代码块/混排段落各三组 Release A/B 通过 |
| P5-C：大内容计算预算 | 61 | Diff Find 后台扫描/索引与查询代际取消 | R3-6；#17 后续 | 已实现；三组 80 秒 Release A/B 通过，快捷键/导航手工验收待补 |
| P5-D：持续架构治理 | 62 | 按职责收敛三个 controller，恢复结构门禁 | R3-8；#28/#34 后续 | 已实现；结构门禁及 Find/任务恢复各三组 Release A/B 通过 |
| P5-D：持续架构治理 | 63 | 主分支 push CI；扩展 Project/Job/Preview/Find 性能基准 | R3-8；#50 后续 | 已接入；五场景各三组 Release 重复性对照通过，CI 已有测试问题另列 |

原建议顺序为 P5-A、Project/Job 与公共调度、Preview/Find，最后由 P5-D 持续接入和收敛门禁。实施记录见第 17–20 节；职责变化已同步中英文架构与相关 spec。

旧方案中的 #29 PR Review 编译边界拆分、#31 objc2 迁移和 #52 缓存/时序辅助收敛继续保留原状态。没有新的 A/B 数据支持重启 #26 已否决的构建配置实验；本轮也不重新启动用户明确暂缓的发布校验/凭据迁移事项。

## 16. 第三轮检查记录与未验证范围

以下结果来自 2026-09-05 的 `2ff3f3b` 审计，不是 2026-09-07 文档归档时重新运行的结果：

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过；只检查格式，未改写源码 |
| `cargo check --locked` | 通过 |
| `cargo test --lib --locked` | 494 通过、0 失败、2 忽略；48.29s |
| `git diff --check` | 通过 |
| `scripts/check-architecture-structure.sh` | 失败；8 项超预算，见 R3-8 |
| R3-1 独立通道最小复现 | 确认合并循环消费关闭信号，sender 保留时下一次接收等待 |

库测试伴随一条链接器警告：`__eh_frame` 超过 compact unwind 编码范围；未出现本轮库测试失败。没有运行完整 workspace 测试、完整 doctest、桌面交互验收或新的 Release 性能基准，不能由本轮结果推断这些检查通过。原文中的 logger doctest 已知问题未在本轮重新验证。

审计时工作区干净且未修改仓库文件；2026-09-07 归档仅更新本提案文档，所有 P5 项仍为待实施。


## 17. P5-A 实施与逐项 A/B（2026-09-07）

本次只实施 #55、#56。沿用 `2ff3f3b` 为源码基线；先完成 #55 并通过 A/B，
再以该版本为 A 实施和验收 #56。保留此前提案内容，未推进 #57–#63。

- **#55 已完成**：任务持久化移入 Feature 自有 `src/ai_review/task_persistence.rs`，
  使用显式 `Persist / Shutdown` 命令；合并快照时保留关闭屏障，处理屏障前最新快照
  后退出。禁写、写入错误和重复关闭均不再导致屏障丢失。保留会话对账职责，
  不在本项扩展通用 Store 队列协议（#59）。4 个确定性测试覆盖空闲、排队、写盘中
  关闭，以及最终快照落盘/错误处理。
- **#56 已完成**：列表与表单预览共享单个 Logo 后台驱动器，调度状态通过 Project
  Store 的强类型 Action 更新。最大并发 4，持续选中的每个 URL 总共最多尝试 3 次，
  失败后分别冷却 2/4 秒。显式刷新重试失败项，保留正在执行的请求身份；移除来源
  后释放调度元数据，关闭时取消请求。UI 投影不再触发 Logo 下载。6 个专项测试覆盖
  预算、冷却、去重、过期完成、元数据释放，以及本地 HTTP 404、真实超时、恢复和取消。

逐项使用真实 Release/FemtoVG 程序、隔离数据和本地 HTTP 夹具，每项三组 AB/BA/AB，
每进程采样 24 秒。表中均为三次中位数，单位 MiB；峰值包含启动过程：

| 任务 | 峰值 RSS A → B | 稳定 RSS A → B | 阶段增长 A → B | 结果 |
|---|---:|---:|---:|---|
| #55 | 114.86 → 113.75 | 114.69 → 113.34 | +0.06 → -0.36 | 通过 |
| #56 | 128.02 → 125.45 | 127.56 → 125.08 | +1.16 → 0.00 | 通过 |

#55 的六个进程均正常退出，128 条中断任务摘要均正确落盘。#56 在 32 个失败 URL
夹具中，请求数中位数由 6,620 降为 96，每个 URL 恰好尝试 3 次，最大并发由 32
降为 4；A 三次退出均超时，B 三次均在 74–130ms 正常退出。两项均通过预先设定的
RSS 门槛：峰值和稳定值增量不超过 `max(8 MiB, A 的 5%)`，阶段增长及其增量均不超过
8 MiB。此结果覆盖本次固定夹具，不代表长时间压力、全部项目页面交互或全局内存上限。

完整协议、二进制 SHA256、每次结果及复现命令见
[`docs/p5-a-performance.md`](docs/p5-a-performance.md)；自动化脚本为
[`scripts/benchmark-p5-a.py`](scripts/benchmark-p5-a.py)。中英文架构和相关 spec 已同步。

验证补充：`cargo fmt --all`、`cargo check --locked`、`git diff --check` 通过，新增
10 个回归测试均通过。沙箱外完整库测试为 503 通过、1 失败、2 忽略；失败为既有 Chat
滚动测试在 offset 9984 的视口断言。使用未修改 HEAD 构建的测试程序确认：沙箱内基线
494 项通过，沙箱外基线单独运行同一 Chat 测试也复现相同失败，不能宣称完整库测试
全绿。结构门禁仍有 7 项历史超预算（原 8 项），保持 #62 待实施。完整环境对照与
检查命令见性能记录；未运行完整 workspace/doctest 或全路由手工视觉验收。

最终分环境验证：沙箱内排除两个需本机监听的 HTTP 驱动测试，502 通过、0 失败、
2 忽略；这两个 HTTP 测试已在沙箱外通过。全部非忽略测试均有通过记录，但不将
分环境通过替代上述完整测试命令的失败结果。

## 18. P5-B 实施与逐项 A/B（2026-09-07）

本轮从干净的 `a42ce0e` 开始，逐项实施 #57 → #58 → #59，每项通过 A/B 后
才推进下一项。完整测量及失败记录见 [`docs/p5-b-performance.md`](docs/p5-b-performance.md)。

- **#57 已完成**：Project 注册表采用 Arc 结构共享；selector 排除 pending 记账，
  单个 Worker 准备最新列表与预览，最多一个排队 UI 回调。稳定 VecModel 只更新
  变化行；图片读取、解码及诊断元数据准备移出 event loop，图片像素与 UI 共享。
  首轮系统字体扫描造成 RSS 约 916 MiB，按门槛否决；改为按需共享应用字体后，
  常规三组峰值 119.56 → 120.08 MiB、稳定值 119.19 → 119.64 MiB，通过。
  诊断场景另三组也通过；全部完成值与退出正常。13 个 Project 单测及 2 个真实
  HTTP 驱动测试通过。架构、Project spec 与索引已同步。

- **#58 已完成**：流式执行移入 `tag/command_output.rs`；4 KiB 分块、16 块队列，
  含截断标记的日志最多 32 KiB。退出期间持续排空，取消时 abort/join Reader，
  继承管道排空限时 2 秒。后台增量解析复用未变化行；Step/log model 保持身份，
  Slint 行缓存最多 128，展开日志复用 ListView 与 GitHub 滚动条，视口最高 420px。
  长行场景三组峰值 253.02 → 143.61 MiB，短行场景另三组也通过 RSS 门槛；
  全部尾部、队列、取消与退出检查通过。7 个 Jobs、12 个 Tag 测试和 UI 策略检查
  通过；已检查真实窗口尾部滚动、折叠重开及窄/宽布局。详见性能记录。

- **#59 已完成**：State Store 与 AI Review 共用最新快照持久化调度，最多保留
  一个执行中和一个待写快照；flush/shutdown 使用 revision 屏障并报告写盘失败。
  AI Review 保留加载完成后同一版本重新落盘的语义。Inbox 和 AI Review 状态投影
  合并 UI 通知；Settings 使用单 Worker、100ms 防抖，退出时排空。
  三组峰值 187.91 → 122.84 MiB、稳定值 184.14 → 122.41 MiB，通过 RSS 门槛。
  Settings 每进程保存 405 → 5 次，Inbox UI 更新中位数 201 → 73 次；四轮落盘
  版本均为 501/1002/1503/2004，最终偏好与未读数正确，六个进程均正常退出。
  State Store 12 个测试、AI Review 持久化 4 个测试、Settings 合并保存测试通过。

最终 `cargo fmt --all`、`cargo check --locked`、`git diff --check` 和 UI 策略检查通过。
沙箱外 `cargo test --lib --locked` 为 **514 通过、1 失败、2 忽略**；失败仍为既有
Chat 滚动测试 offset 9984 的视口断言。相同测试程序在沙箱内排除两个需本机监听的
Logo HTTP 测试后，513 通过、0 失败、2 忽略；两个 HTTP 测试在沙箱外通过。
分环境结果不替代完整命令的失败记录。结构门禁仍有 7 项已有超预算，未提高预算；
#62 保持待实施。中英文架构及相关 spec 已同步；未运行根包完整 doctest、长时压力
或全路由视觉回归。工作区起始干净，没有覆盖其他用户改动。

## 19. P5-C 实施与逐项 A/B（2026-09-07，09-08 收尾）

从干净的 `6183769` 开始，先验收 #60，再实施 #61。完整协议、原始数据位置、
编译异常和视觉预检修复记录见 [`docs/p5-c-performance.md`](docs/p5-c-performance.md)。

- **#60 已完成**：Provider Worker 准备不可变测量快照，共享 blocking pool 执行
  精确 Parley 测量；按文档版本、稳定宽度、字体拒绝旧结果。保留一个执行中计算、
  一个最新待处理请求和一个结果；关闭取消，退出排空。UI 保留上一份精确布局，
  完成通知不重置滚动。大代码围栏只挂载可见行及少量 overscan，转换缓存有界；
  修正超大圆角裁剪产生的 framebuffer 错误，复制仍使用完整源码。
  12,000 行代码场景三组峰值 361.89 → 183.38 MiB，稳定值 314.30 → 158.58 MiB，
  累计行创建 36,000 → 710；2,000 段混排场景三组峰值 121.09 → 120.14 MiB，
  UI 布局回调 P95 200.02 → 0.58ms。两个场景的尾部、渲染错误、退出和 RSS 门槛
  均通过。29 个 Preview 测试、2 个后台调度测试通过；真实窗口已检查尾部、滚动、
  分隔线 resize 与关闭。未用开发构建内存数据代替正式 Release A/B。

- **#61 已实现，RSS A/B 通过**：共享不可变 Diff 源码和行投影，在共享 blocking pool
  扫描；输入、源码替换、关闭和审核重置取消过期代际。紧凑命中范围与行/侧索引替代
  全量 Slint 命中物化，命中转换与行 model 缓存各限 128 项。独立结果版本确保相同
  命中数的结果替换也刷新当前项；完成通知只更新展示，不重放滚动命令。
  三组 80 秒 Release A/B：峰值 RSS 313.88 → 139.34 MiB，稳定值
  309.05 → 136.75 MiB，阶段增长 +0.03 → 0.00 MiB；连续七次查询的 UI 回调
  P95 1396.99 → 1.82ms。全部 72 轮最终查询的计数、首尾内容正确，六个进程正常退出。
  首次 40 秒测试因工作阶段/延后内存释放造成增长差值未通过，原始失败记录保留；
  延长到 80 秒后按原三组中位数及原 RSS 门槛通过，没有放宽阈值。
  11 项 Find 测试通过。Release 手工确认窄 Diff 中文源码换行；自动化的修饰键组合
  被当作普通字符输入，未完成 Find 输入栏、同数量结果替换及首尾导航的视觉验收。
  查询未完成时的 Enter 只刷新去抖，不排队重放，后续显式命令才导航已完成结果。

最终 `cargo fmt --all`、`cargo check --locked`、UI policy、`git diff --check` 通过。
`cargo test --lib --locked` 为 **523 通过、1 个既有 Chat 分页布局失败、2 个忽略**；
失败仍在 `src/chat/pagination_tests.rs:174`，offset 9984，相关源码未改。
结构门禁仍有 7 类既有超限，未抬预算；PR controller 5351 → 5315 行。
不把短时 RSS 测量视为长期 soak 保证；未覆盖完整 detached/theme/Find 视觉矩阵。
起始工作区干净，未覆盖无关用户改动，未创建提交。

## 20. P5-D 职责收敛与持续门禁（2026-09-08）

从干净的 `8b29a8f` 开始。协议、原始证据位置和验收限制见
[`docs/p5-d-performance.md`](docs/p5-d-performance.md)。

- **#62 已实现，A/B 通过**：PR Review 提取远端动作、Diff 绑定和诊断；Chat
  提取视口/锚点策略和稳定增量消息 model；AI Review 提取会话 I/O 与中断恢复。
  设置保存反馈离开根装配层，14 个 Diff callback 归 Feature 的无状态
  `PrDiffBridge`。不搬测试、不提高原预算；PR controller 5315 → 4287 行、
  Chat 2650 → 2165、AI Review 3933 → 3846、根 Rust 1117 → 1034，根 Slint
  callback 149 → 135。结构门禁通过，并补提取模块规模及反向依赖守护。
  Find 三组 80 秒 A/B 峰值 RSS 150.55 → 150.63 MiB、稳定值
  150.08 → 147.48 MiB；任务恢复三组 30 秒 A/B 峰值 113.48 → 114.48 MiB、
  稳定值 113.38 → 113.95 MiB。两场景均通过原 RSS 门槛，12 个进程正常退出。

- **#63 已接入，实跑通过**：CI 加入 `main` push；Nightly 一次构建，串行
  运行 Home、Project、Job、Preview 代码/段落与 Find。新增门禁复用原有真实
  应用夹具，检查绝对 RSS/阶段增长、callback P95、队列/尾部/取消和创建行数。
  每场景三组 AB/BA/AB；本批无应用二进制变化，相同哈希只用于重复性验证，
  不宣称运行时性能提升。原始证据缺失、指标无效、夹具未完成和子进程失败均
  失败关闭，失败后继续采集其它场景并上传证据。12 个门禁策略测试通过。
  五场景共 30 次实跑全部通过，最大单次峰值 RSS 187.22 MiB、最大阶段增长
  0.375 MiB，分别低于 256/8 MiB 门槛；Job 最大取消耗时 267ms，队列最多
  16 块，累计创建最多 278 行。代码 Preview 每进程创建 710 行。代码 Preview
  稳定 RSS 相对差值 8.016 MiB 接近原预算 8.028 MiB，完整波动记录已保留，
  未调整阈值或把相同二进制对照写成性能提升。

完整单元测试在沙箱外为 523 通过、1 个已有 Chat 分页布局失败、2 忽略；失败
仍是 offset 9984 的视口断言。fmt、默认 check、结构/UI/i18n 门禁通过；关闭
默认功能的全目标 check 通过，保留两处已有 Settings 条件导入警告。开发功能
的应用 check 通过，但全目标 check 暴露已有 `ai_review_response_ui` 内联
Slint 夹具缺少 `@i18n` library 映射；未宣称完整 CI 全绿，也未触发远端 CI。
PR 集成测试改用新 global，并补入 #61 已有的即时空查询取消预期，4 项全部通过。
人工检查完成窄 Diff 中文换行/滚动及 Chat 双向滚动、模式切换和重入；CUA 修饰键
仍被当作普通字符输入，Find 键盘/导航验收缺口保留。中英文架构及相关 spec 已同步，
起始工作区干净，没有覆盖无关用户改动，未创建提交。
