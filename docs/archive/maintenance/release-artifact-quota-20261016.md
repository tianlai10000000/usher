# Release artifact quota / 发布产物配额（2026-10-16）

[Release v0.0.10-alpha.1](https://github.com/tianlai10000000/usher/actions/runs/38872541335)
的只读 job 快照确认：两个 macOS target 编译、保存缓存、bundle、压缩包和 DMG
均成功，产品上传失败。用户日志显示 `Artifact storage quota has been hit`。
查询时 Windows 尚在编译，这不是整轮运行的最终状态。

抽查 v0.0.9-alpha.1–alpha.4 的四次成功发布，共有 12 个未过期 Actions 产品
artifact，占用 867,807,088 bytes（827.61 MiB），到期时间对应约 90 天保留。
对应每个 Release 的 11 个预期下载附件均存在、非空且状态为 uploaded。未下载
逐字节核对，也未查询账号总额度、其它仓库或计费设置。

本地修复为产品上传显式保留 7 天、构建诊断与最终计时保留 3 天。产品上传仍阻断；
保留所有产品文件名及既有 debug 包，不改 Release 优化、缓存、权限、触发器或
发布方式。当前契约见 [Release CI](../../specs/release-ci.md)。

专项契约、行为模型及 metrics 共 96 项通过。必需回归：fmt/check 退出 0；库测试
965 passed / 9 ignored / 0 failed；doctest 1 passed / 1 existing ignored / 0 failed。
构建输入 745 个文件在这些检查中无 hash 漂移。工作流 A/B、负例与 lint 的原始记录
存于本机 `docs/validation/release-artifact-quota-20261016/`，遵循
[本地输出规范](../../validation/README.md)，不作为项目测试输入。

本地修复验证时尚未部署或云端重跑；缩短保留期不会删除旧 artifact。随后用户明确
授权清理上述 12 个旧 Actions artifact，已逐项删除并通过独立 GET 确认四个旧 run
均无剩余 artifact，移除的元数据大小共 827.61 MiB。对应 44 个 Release 附件的
ID、名称、大小、状态及 GitHub digest 与删除前完全一致。

当前 run 在清理期间新增 7 个 artifact，均保留；因此其快照不完全相同，删除前
空清单没有缺失项，不能把并行新增写成误删或虚称快照不变。原始前后记录及
`artifact-cleanup-result.json` 保留在同一本地输出目录。未执行任何 Release、
编译缓存的写入/删除、计费修改或构建重跑；本次仅核对远端结果与更新记录，不重复
运行应用构建或 UI 回归。

最终只读观察还确认 v0.0.10-alpha.1 的第 2 次 attempt 已完成且结论为 success，
对应 Release 已有 11 个 uploaded 附件。这是观察到的并行运行结果；本 agent 没有
触发重跑，不据此推断全账号配额结算状态或本地保留期候选已部署。

GitHub artifact 与 cache 使用不同存储额度，清 cache 无法释放 artifact 空间。
删除后用量刷新可能需要 6–12 小时，不能保证立即恢复，也不抹去已累计计费。
依据 [GitHub storage rules](https://docs.github.com/en/billing/concepts/product-billing/github-actions)。
