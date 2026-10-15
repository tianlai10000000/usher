# macOS native candidate

本记录区分中间候选与最终候选；最终结果见 [README](README.md)。所有原生操作均使用
CUA app API，在完整生产 MainWindow、MessagePageBridge 和 Messages ComponentFactory
上执行，未直接注入 Slint WindowEvent 或调用业务 callback。截图已在工具会话中逐项
检查；本目录保留应用日志与二进制 SHA，不声称 AX chrome 能反映 Slint 内部焦点。

## 中间候选：核心修复及禁用方向失败

[构建](candidate-build-initial.log)、[隔离夹具/SHA](candidate-fixture.json)、
[应用日志](candidate-initial-native.log)。800 × 600 logical，scale 2，light，1000 条
合成通知、20 条/页、50 页；与 [baseline](native-baseline.md) 使用独立但同构的临时状态。

| 操作 | 实际观察 |
| --- | --- |
| English：bell → Tab | 消息中心显示，侧栏 Home 有蓝色键盘环；未读 1000。 |
| 保留 Home 焦点，滚到 footer，单击 page 4，再 Return、Space | 页面回顶部，首条 0940；仍在 Notifications，未读 1000，Home 环消失。baseline 相同前置的 Return 会跳 Home。 |
| 中性页面焦点 → Tab | Mark all as read 获得可见焦点。后续 Tab 能到分页链接，分页栏自动进入视口。 |
| 分页链接持有可见环，Return | 到 page 50，末行 0001；footer 保持可见，能继续 Tab 到宿主 breadcrumb。 |
| Command+B 两次 | 侧栏依次收起、恢复。此项确认原生命令可用；单次 bridge 转发由 compiled UI 断言。 |
| Settings 中 English → 简体中文，bell 重入 | 同一个已保留 factory 的标题、计数、分类、筛选和方向文字刷新为中文；仍保持 page 50。 |
| 首页键盘环前置，page 1 指针点击，再 Return、Space | 首条 1000，未读仍 1000，停留通知页。 |
| 逐键 Tab，键之间读取原生状态 | 可到“下一页”的蓝环。仅快速发送长串 Tab 不作为逐项焦点证据。 |
| 下一页持焦点，Space | page 2 → 3；footer 和下一页键盘环保持可见。 |
| 同一下一页，46 次 Return，每次读取原生状态 | page 3 → 49；末行 0021，未发生 Home 跳转、已读或删除。 |
| Return 到 50，再 Tab | 新焦点到 breadcrumb 首页，但禁用的下一页仍有第二个蓝环：**失败**。与 [compiled before](disabled-outline-before.log) 的 border width 2 而非 0 相符，不能交付这一候选。 |

已通过原生 Command+Q 退出中间候选；没有把关闭/隐藏动作当作优雅 drain 的验证。

## 原生反向输入工具限制

CUA `pressKey('shift+Tab')` 在消息页和普通宿主侧栏都表现为正向 Tab：Home → Chat。
原生 Raise 到前台、100ms duration 与 `Shift_L+Tab` 都未改变结果。`Shift+Tab`、
`ISO_Left_Tab` 和小写 `tab` 被工具明确拒绝为 keyNotFound；没有把这些调用记为通过。
因此本批不能声称已通过真实反向 Tab 的原生自动化验收。

源代码背景（推断，不是工具侧事件追踪）：固定版本 winit macOS 的 flagsChanged
路径与普通键上的 modifier flags 使用不同事件；Slint 的 modifier state 依赖 modifier
KeyPressed，可能解释此输入现象。产品侧的真实 Shift+Tab 与独立 Backtab 则由完整
compiled UI 的反向跨 factory 断言验证。保留人机原生键盘复核项，不将宿主也出现的
自动化输入现象归咎于新 focus bridge。

## 最终候选

[最终 manifest/SHA](final-fixture.json)、[最终应用日志](final-native.log)、
[重编译](candidate-build-reveal.log)、[进程清理](native-process-check.log)。
最终二进制 SHA256 为 `edd845e81b826bd1e88ca5457a37f6f835b80db3028a0e80376b09d9b956b69c`。
包含禁用描边清理、独立 Backtab 及目标预滚入修复。后者的
[before trace](backtab-trace-before.log) 显示反向卡在同一个 Mark as read，
随后 [4 项 compiled 回归](focused-reveal-after.log) 全部通过。

重用已注册的本任务临时 bundle：确认其精确路径对应进程退出后替换二进制，再启动。
首次复用保留了旧 crash sentinel，产生一条“Previous run ended unexpectedly”，导致
1000 条上限内的合成消息位置偏移一位。这是 [重启对照日志](final-restart-control-native.log)，
没有将它冒充纯夹具对照。之后确认进程退出，保存整个旧临时 state/log，恢复 baseline
的原始 1000 条未读合成消息和隔离 Settings，再执行下表。最终无非合成消息。
这也再次说明本次 Command+Q 退出不等于验证应用 graceful drain。

| 最终原生检查点 | 观察与结果 |
| --- | --- |
| en，800 × 600 logical | bell → Tab 显示 Home 蓝环；保留该环滚 footer，单击 page 4，Return + Space 后首条 0940、未读 1000，仍在 Notifications。通过。 |
| 完整逐键 Tab | 指针到 page 49，中性焦点 → Tab 到 Mark all as read；逐键读取原生状态后遍历 20 行操作，到 Next 蓝环。通过，不以快速长串按键替代。 |
| 末页禁用与宿主退出 | Next 持焦点，Space 到 page 50，末行 0001、footer 可见，禁用 Next 无蓝环；Tab 到宿主 breadcrumb Home，只有一个蓝环。对照中间候选双环，修复后通过。 |
| 原生 zoom / restore | 原生 zoom 的截图源尺寸 5120 × 2692 physical（scale 2），内容居中、footer 完整无溢出；还原到 1600 × 1200 physical。通过。 |
| runtime en → zh-CN | 从 Settings 切简体中文，重入同一 factory，仍在 page 50；标题、计数、分类、筛选、方向文案即时刷新。通过。 |
| zh 指针后按键 | 首页蓝环 → 指针 page 1 → Return + Space：首条 1000、未读 1000，仍在通知。通过。 |
| 空过滤 | 已读 0 显示中文空结果，footer 隐藏；全部恢复 1000 条与分页。通过。 |
| zh 数字键盘激活 | 可见数字 6 焦点环 → Return 到 page 6；后续 Tab + Space 到 page 8，footer 始终可见。稳定页码槽位重投影后焦点槽的数字可改变，本项不声称锁定同一语义页码。通过。 |
| 快捷键与路由重入 | 页内键盘焦点下 Command+B 两次依次收起/恢复侧栏；Home → bell 重入保留 page 8，首条 0860。通过。精确单次 callback 转发另由 compiled 断言。 |
| 单页过滤及恢复 | 仅指针将合成消息 0860 标已读：未读 999，已读 1 只显示这一行，无 footer；全部恢复后指针 page 2 + Return + Space，首条 0980、未读仍 999。通过。 |

最终持久化夹具经读取核对：1000 条，999 未读，0 条非合成消息；唯一已读消息是上表
显式指针操作。退出后 baseline / candidate / final 的精确测试进程均不存在。
原生反向 Shift+Tab 的工具限制仍如上，不声称该项通过；Modal、独立 Backtab、隐藏
控件和强制 640px 分支的完整断言属于 compiled UI 覆盖。
