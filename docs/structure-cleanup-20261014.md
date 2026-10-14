# 2026-10-14 第 6 批结构治理记录

本文是历史验收记录。当前职责见中英文架构文档及 Project、PR Review、AI Code Review 规格。
基线为 `1890c8ac9eeec812e3eeab1d18cf9546a3bab3b0`，开始时工作区干净；前 1–5 批已经包含在基线中。本轮不改第三方 Material、用户配置或持久化格式，不宣称运行速度或内存收益。

## 结构 A/B

| 原门禁 | 基线 | 候选 | 原阈值 |
| --- | ---: | ---: | ---: |
| AI Review controller 行数 | 3884 | 3697 | 3850 |
| 根 Slint 行数 | 3101 | 2982 | 3000 |
| 根 Slint callbacks | 142 | 133 | 140 |
| 根 Slint properties | 425 | 401 | 410 |
| PR Review actions 行数 | 1150 | 545 | 580 |

原有 22 条 `check_max` 声明逐字保留，没有调高阈值、删除检查或增加豁免。候选结构门禁通过；这仅说明结构约束满足，不代表全部质量检查或性能验收通过。原始输出、预算比较与正反例结果位于 [validation/structure-cleanup-20261014/](validation/structure-cleanup-20261014/)。

新增 7 条预算：AI 输入模块 330 行；PR action presentation 460 行、remote 310 行；Project bridge 70 行/9 callbacks/24 properties；Project dialog host 90 行。提取模块仍被反向 controller 依赖门禁检查，输入/remote 模块另禁止 UI/controller 类型。正好等于阈值的正例必须通过，超过一项的负例必须失败。

## 职责变化与不变量

- `src/ai_review/review_input.rs` 拥有不可变请求、Provider diff 规范化、UTF-8 安全的 48 KiB Ask 上下文、模型校验和 Worker 上的规则快照读取。Controller 保留调度、异步加载/取消、Session 协调与 UI 投影。旧测试没有搬走以减少行数；新增两项输入边界测试。
- `actions.rs` 保留命令校验、独立的 RemoteActionState/RemoteMutation 代际、weak window 完成投递和刷新协调。`actions/remote.rs` 持有 typed 目标快照并执行 Provider → 共享 PR 状态发布 → Hook；`actions/presentation.rs` 负责能力、就绪状态、合并选择及语言投影。13 个旧测试仍在对应职责下。
- Remote mutation 返回 Future 前同步捕获含凭据 scope 的 `PrKey` 和 Hook 事件。不能把这种准备无意改成第一次 poll 才执行；成功后取得新的 mutation ticket 的时点同样保留。陈旧读取发布被拒绝时仍直接丢弃。
- `ProjectBridge` 统一页面、根层弹窗和 Rust 的展示/临时草稿 API；根窗口只保留路由、全窗口几何与遮罩层级。持久状态仍由 `ProjectStateFacade` 管理，没有新建第二个领域 Store。语言切换、异步 weak 回写、投影 model 身份、Hook 选择与诊断全部改向同一 bridge。

## 迁移中发现的既有缺陷

旧 Project 页面内部 `new-project-pr-auto-update` 未与根窗口/弹窗双向连接：卡片编辑赋值发生在页面局部，提交却读取根草稿，可能把原本开启的预取标志保存为关闭。直接消费同一 bridge 后恢复了这条绑定。真实根 UI 用例覆盖 grid/list × en/zh-CN，编辑、已挂载弹窗切语言、取消清草稿、重开保存及新建默认值；原生验收使用纯内存 example，不启动应用服务。基线网格测试实际打开编辑窗后在预取标志断言失败，列表基线则在首次点击后未打开弹窗；两项失败分别保存，不互相替代证据。

指针回归还定位了另一项既有缺陷：列表 `row-focus` 的宽度为行宽减 184px，却未声明 x/y，默认居中后覆盖了尾部 Edit 区域；未聚焦时 FocusScope 接受首次 Press，导致需要第二次点击。修复显式设置 `x: 0px; y: 0px`，使它与 `row-touch` 同边界。保留原始单次点击，未增加等待或双击，也未移除裁剪。早期按裁剪/测试激活排查的推断被事件命中证据推翻，原始失败另行保留。

UI 门禁现按文件局部 import/export 追踪继承链到 canonical 共享 Shell，原直接消费者清单/数量不变。已有 Shell 的继承适配器禁止追加视觉子树；错误来源、同名假冒、循环、歧义、缺失或重复 Shell 均被负例覆盖，没有按新宿主名字增加豁免。它仍是保守源码检查，不解释任意 Slint 视觉实例图。

## 验证结果

| 检查 | 实际结果 |
| --- | --- |
| `./scripts/check-quality.sh --quick` | exit 0，9/9 项通过：格式、结构、UI、i18n、脚本测试、默认编译、库测试、doctest、diff |
| 最终库测试 | 832 通过、0 失败、2 个既有 ignored，34.42 秒 |
| 根包 doctest | 1 通过、0 失败、1 个既有 ignored；logger 示例通过 |
| Python 脚本套件 | 103/103 通过，38.662 秒；输出中的模拟 runner exit 7 属于负例 |
| Project/PR/Preview 根 UI 集成 | 默认 feature 图 11/11 通过；Project 单用例覆盖 en/zh-CN × grid/list 和已挂载弹窗切语言 |
| 禁用 AI 的 root all-targets check | 最终复验 exit 0，24.08 秒；保留 `settings_page.rs` 两项既有未使用导入警告，源码与基线逐字相同 |
| 原生离线 Project 夹具 | macOS、light、1200×900；英文列表/网格/保存/取消已检查，最终修复版本中文列表在未聚焦行上单次 Edit 即打开，保存值 On、新建默认 Off 正确 |

基线结构门禁失败 5 项，候选全部通过。所有原阈值字面量边界正反例通过；扩展后的 10 个结构顶层用例在基线有 11 个失败子例（缺少新 owner/依赖保护），候选全部通过。Dialog 的 3 个相同 A/B 用例从基线 3 失败变为候选 3 通过；完整 quality 脚本用例 65/65 通过。16 个抽取函数的规范化 body SHA256 相同；13 个 PR 旧测试全部保留，12 个 body 相同，1 个增加断言。文本等价不替代运行时验证。

初次沙箱库测试 816 通过、14 例因本机监听权限失败；获准后重跑，并行托盘改动加入前的工作区为 830 通过、0 失败、2 ignored。最初新增 UI 用例误用未绑定的 accessibility default action，改为真实单次指针后又发现上述焦点重叠；最终没有放宽断言、延长点击或绕过 UI。首次编译的多余测试 import 已删除。一次重复 baseline 编译主动取消（exit 130），有效的基线失败记录已经保存，取消不计作通过证据。

执行期间工作区出现独立的托盘/i18n 并行改动（`src/lib.rs`、`src/tray.rs`、`docs/specs/tray.md`、`i18n/messages.toml`、中文 PO）。这些改动均保留，没有纳入本批职责拆分；最终 832 个通过项包含它们新增的两个托盘测试。最终默认质量入口、11 项界面集成与禁用 AI 复验均在合并的工作区上运行。没有提交或推送。

未执行 `check-quality.sh --full` 的 workspace/Clippy 组合、Windows 原生验收、线上 Provider/真实安装流程或 Release 性能 A/B。原生夹具不覆盖生产 AppKit accessory titlebar；当前不宣称性能收益。详见验证目录中的原始输出、失败历史和原生验收范围。

可重复的候选命令：

```sh
./scripts/check-quality.sh --quick
cargo test --locked --test project_bridge_ui --test pr_review_core_journeys --test pr_review_header_ui --test artifact_preview_resize_ui
cargo check -p usher --all-targets --no-default-features --features renderer-femtovg --locked
cargo run --locked --features test-fixtures --example project_bridge_fixture -- --locale zh-CN --view list
```

结构基线对照使用 `USHER_TEST_ARCHITECTURE_BASE_REF=1890c8ac9eeec812e3eeab1d18cf9546a3bab3b0`；UI policy 使用 `USHER_TEST_UI_POLICY_BASE_REF` 同一值。基线缺失保护导致失败是预期证据，不能计为候选通过。Project 旧版对照在独立 archive 中仅把 Bridge getter/setter/callback 映射回旧的 `project_*` Root API；grid-first 与 list-first 分别揭示两个既有缺陷，未回退当前树。

## 经验与防复发

1. 门禁超限是职责审查信号。抽取完整输入、传输、展示边界，不能靠压缩空行或只搬测试制造通过。
2. 新 owner 同时获得规模和依赖检查，既有阈值另以字面量边界正反例锁定。检查应遍历实际新路径，避免膨胀转移到无检查文件。
3. 异步重构核对两个顺序：调用/完成顺序，以及身份、凭据域、事件时间的捕获时点。`async fn` 的首次执行时点不是普通函数调用时点。
4. Slint 根 API 拆分必须迁移实际消费端，不保留成套代理或复制状态。逐项核对双向草稿，编译不能证明表单回写完整。
5. 结构检查需要理解合法复用的所有权，不能改名绕过或按路径豁免；同时用没有共享 shell、重复 shell、错误继承和循环反例防止漏检。
6. 未声明坐标不等于零坐标。焦点节点和 TouchArea 必须按实际几何核对，尤其是较窄节点的默认居中；单次点击测试不能先预热焦点。
7. 将结构 A/B、真实行为回归和原生视觉检查分开记录。纯重构不能宣称性能提升；有性能主张时另做受控 Release A/B。
