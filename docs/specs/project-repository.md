# Project repository metadata / 项目仓库元数据

## Scope / 范围

This specification covers provider metadata resolved on the Project page and the provider-neutral pull-request list displayed in the existing Workflow PR tab. It does not change RAG Chat behavior.

本文约束 Project 页面解析的 Provider 元数据，以及现有 Workflow PR Tab 中的 Provider-neutral PR 列表；不改变 RAG Chat 行为。

## Ownership / 职责

- `src/project/mod.rs` validates form data, starts the asynchronous save flow, and persists `projects.json`.
- `src/codereview/api/` parses remotes and owns provider-specific request paths, authentication headers, and response decoding.
- `src/http/mod.rs` owns request execution, interception, timeout/status handling, and safe logging.
- `src/workflow/pr_list.rs` owns selected-project list state, request sequencing, status filters, and pagination.

- `src/project/mod.rs` 校验表单、启动异步保存流程并持久化 `projects.json`。
- `src/codereview/api/` 解析 remote，负责 Provider 请求路径、鉴权 header 和响应解码。
- `src/http/mod.rs` 负责请求执行、拦截、超时/状态处理及安全日志。
- `src/workflow/pr_list.rs` 负责当前项目的 PR 列表状态、请求序号、状态筛选和分页。

## Invariants / 不变量

1. `remote_repo` determines the provider. Supported values are `gitee`, `gitlab`, and `github`.
2. A supported URL may use HTTPS or SSH Git syntax and must include an owner/group plus repository path.
3. Configured Gitee-compatible enterprise project search uses the final `remote_repo` path segment before `.git` as `project_name`; it never uses the editable project name or alias.
4. Provider project details must be loaded through the shared HTTP module. Tokens and enterprise API configuration come from the matching Git platform settings and must not be logged.
5. Enterprise API URLs are supplied only by `[git.platform.<provider>].api_base_url`; source code must not contain environment-specific endpoints or enterprise identifiers. A configured URL is used only if its host matches `remote_repo`. The service host or full versioned API root is accepted; the provider version path is added when omitted.
6. A successful save writes non-empty `repo_project_id` and lower-case `repo_type` to `project/projects.json`.
7. If lookup, parsing, or local persistence fails, no new partial project record is written and an existing record remains unchanged.
8. A duplicate save for the same project while a lookup is running is rejected. Network work must not block the Slint event loop.
9. Opening Workflow loads `repo_project_id` and `repo_type` from `projects.json`, then requests open PRs by default. Open, closed, merged, and all are explicit caller-selected filters; provider APIs do not inject a default state query.
10. The PR tab displays 10 records per page and bottom-centered pagination. It must lazily request only the page selected by the user and use the documented response `meta.total_count` and `meta.total_pages` for the exact total and total pages. Stale responses must not overwrite the current project/filter/page.

1. `remote_repo` 决定 Provider，支持 `gitee`、`gitlab`、`github`。
2. 支持 HTTPS 或 SSH Git URL，且必须包含 owner/group 与仓库路径。
3. 已配置的 Gitee 兼容企业仓库搜索使用 `remote_repo` 中 `.git` 前的最后一段作为 `project_name`；绝不使用可编辑的项目名称或别名。
4. Provider 详情必须经由统一 HTTP 模块请求；Token 和企业版 API 配置来自对应 Git 平台设置，且不得写入日志。
5. 企业版 API 地址只能由 `[git.platform.<provider>].api_base_url` 提供；源码不得保留环境专属地址或企业标识。只有 API 地址主机与 `remote_repo` 匹配时才使用该配置；既支持服务主机，也支持带版本路径的完整 API 根地址，缺少版本路径时由 Provider 自动补齐。
6. 保存成功后，`project/projects.json` 中写入非空 `repo_project_id` 和小写 `repo_type`。
7. 查询、解析或本地持久化失败时，不写入新的不完整项目记录，已有记录也保持不变。
8. 同一项目在查询未完成时重复保存必须被拒绝；网络工作不得阻塞 Slint event loop。
9. 进入 Workflow 时从 `projects.json` 读取 `repo_project_id`、`repo_type`，默认查询 open PR。open、closed、merged、all 均由调用场景显式选择，Provider API 不自行注入默认状态 query。
10. PR Tab 每页显示 10 条以及底部水平居中的分页，并且必须只懒加载用户选中的页。使用接口响应中定义的 `meta.total_count`、`meta.total_pages` 显示准确总条数和总页数；旧项目、旧筛选或旧页码响应不得覆盖当前视图。

## Provider requests / Provider 请求

| Provider | Detail request | Authentication |
| --- | --- | --- |
| Configured Gitee-compatible enterprise | `GET <api_base_url>/projects?search=<repo name>` | configured `enterprise` (when present), `OAUTH: enabled`, optional `Private-Token` |
| Gitee | `GET /api/v5/repos/<namespace>/<repo>` | optional Gitee token |
| GitLab | `GET <configured api_base_url or remote-derived /api/v4>/projects/<URL-encoded namespace/repo>` | optional `PRIVATE-TOKEN` |
| GitHub | `GET <configured api_base_url or https://api.github.com>/repos/<owner>/<repo>` | optional Bearer token |

The enterprise Gitee search result is selected by exact namespace/path where available; a single-result response is accepted as a fallback. GitLab and GitHub use their configured API root only for a remote with the same host.

企业版 Gitee 搜索结果优先按精确 namespace/path 选择；仅当响应只有一个项目时才接受单结果回退。GitLab 和 GitHub 也只有在 remote 主机匹配时才使用其配置的 API 根地址。

## Regression checklist / 回归清单

- Save a GitHub, GitLab, and Gitee project and verify `repo_project_id` plus `repo_type` in `projects.json`.
- For `git@gitee.example.com:group/repo.git`, verify the search query is `search=repo`.
- Return multiple same-name Gitee results and verify the exact remote namespace is selected.
- Simulate an unsupported remote, 401/403, or timeout and verify no partial project record is written.
- Save twice before the first lookup finishes and verify only one request is sent.
- Confirm the UI remains responsive during the lookup and no token appears in logs.
- Enter Workflow and verify the PR tab shows the saved provider/id and the first 10 open requests.
- Switch open/closed/merged/all and paginate rapidly; verify only the latest response updates the list.
