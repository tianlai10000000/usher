# Usher AI/Coding Agent 文档入口

这份文件保留用于兼容旧对话和旧链接，不再承载完整架构或问题排查记录。

请按顺序阅读：

1. 根目录 [AGENTS.md](../AGENTS.md)：coding agent 的强制规则、验证命令和交付要求。
2. [architecture.zh.md](architecture.zh.md) 或 [architecture.md](architecture.md)：当前架构、职责和数据流。
3. [specs/README.md](specs/README.md)：稳定行为规格索引。
4. 对应 feature 的源码和测试。

历史终端排障过程位于 [terminal_viewport_iteration_notes.md](terminal_viewport_iteration_notes.md)，它不是当前行为的 source of truth。当前 Terminal 契约以 [specs/terminal.md](specs/terminal.md) 为准。

## 本次整理修正的旧文档偏差

- 旧根 README 仍把项目描述为 Slint Material Template，并包含当前未维护的 Web/Android 启动说明。
- 旧架构文档将 Terminal 的“当前问题、尝试方案、已修复结论”混为一体，并存在章节编号顺序错误。
- 旧文档没有记录统一 HTTP 层；此前 Chat、Network、Updater、Project Logo 和 PR Stats 各自直接使用 reqwest。
- 仓库操作位于 `src/git/`，接口 URL 常量位于 `src/codereview/api/`；两者禁止反向耦合。
- `Route` 枚举包含多个开发页面，但主导航当前只开放 Home、Chat 和 Project。

后续不要继续向本文件追加架构细节；应同步更新中英文 architecture，或在 `docs/specs/` 中维护稳定契约。
