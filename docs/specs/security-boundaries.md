# Security boundaries / 安全边界

## Scope and ownership / 范围与职责

This contract covers application-owned credential routing, HTTP diagnostics,
update download files and external file/process launching. It does not certify
third-party code, release authenticity or every application execution path.
User-authored Hook scripts retain their explicit opt-in contract in [Hooks](hooks.md).

本文约束应用自有凭据路由、HTTP 诊断、更新下载文件和外部文件/进程启动；不代表
第三方代码、发布包真实性或所有执行路径已通过安全审计。用户编写的 Hook 脚本
继续遵循 [Hooks](hooks.md) 的显式启用契约。

| Owner | Responsibility / 职责 |
| --- | --- |
| `src/git/locator.rs`, `src/git/gitlab.rs` | Actual GitLab API origin validation and one credential attachment entry / 实际 GitLab API origin 校验及单一凭据附加入口 |
| `src/http/mod.rs` | Shared clients, redirect selection after interceptors, bounded diagnostics / 共享 client、拦截器执行后的重定向策略选择及有界诊断 |
| `src/update/download_archive.rs`, `src/update/updater.rs` | Atomic download-file ownership through extraction and direct executable restart / 原子下载文件从创建到解包的所有权及直接重启进程 |
| `src/platform/mod.rs` | Explicit external file opening without shell interpretation / 显式外部打开文件，禁止 shell 解释 |

## Credential destinations / 凭据目标

- Provider identification is not authorization to disclose credentials. GitLab's
  global token may be attached only to `https://gitlab.com` (effective port 443)
  or the exact origin explicitly configured in GitLab API settings. Compare parsed
  scheme, host and effective port; a hostname substring is insufficient.
- Read the configured token and trusted origin from the same settings snapshot.
  Unconfigured self-hosted GitLab detection and anonymous reads remain available;
  configuring the API origin is required before its requests can use the token.
- `src/http/mod.rs` owns both clients with common connection/user-agent settings.
  After request interceptors run, sensitive headers, recognized credential headers,
  URL userinfo and known credential query keys select the client that permits only
  same-origin redirects and retains reqwest's bounded redirect limit. Cross-origin
  redirects fail before contacting the destination, including HTTPS downgrades and
  port changes. Anonymous requests retain normal redirects for CDN downloads.
- New credential mechanisms must extend this classification or mark custom header
  values sensitive. The query-key list is not a universal secret detector.

- Provider 识别不等于允许发送凭据。GitLab 全局 Token 只能发往
  `https://gitlab.com`（有效端口 443）或 GitLab 设置中显式配置的精确 origin；
  必须比较解析后的协议、主机和有效端口，不能依赖域名子串。
- Token 与可信 origin 来自同一个设置快照。未配置自建 GitLab 仍可识别并匿名读取；
  使用全局 Token 前必须配置其 API origin。
- HTTP 模块集中持有两种策略的 client，共用连接和 User-Agent 配置。请求拦截器
  执行后，根据敏感 Header、已知鉴权 Header、URL 用户信息及已知凭据 query key
  选择仅允许同 origin 重定向的 client，并保留 reqwest 的跳转次数上限。跨 origin
  时先失败再发送，不接触目标，协议降级和端口变化同样拒绝；匿名 CDN 下载仍可跳转。
- 新增凭据机制时必须同步识别规则或把自定义 Header value 标为敏感。query key
  列表不保证识别所有任意命名的秘密。

## Diagnostic exits / 诊断出口

HTTP URL diagnostics retain scheme, host, port and path; they omit userinfo, the
whole query and fragments. Non-HTTP(S) URLs become a fixed redacted marker. Strip
the URL from reqwest errors before formatting. JSON decode diagnostics retain
category and line/column without echoing payload values. `HttpError::Debug` emits
metadata and response length, never arbitrary message prose or response excerpts.
The bounded raw response remains available to explicit feature error handling;
callers must not log it. Paths and operation labels must not contain secrets.

HTTP URL 诊断保留协议、主机、端口和路径，去除用户信息、完整 query 和 fragment；
非 HTTP(S) URL 使用固定脱敏标记。reqwest 错误格式化前移除 URL；JSON 解码诊断
只保留类别和行列，不复述 payload。`HttpError::Debug` 只输出元数据及响应长度，
不输出任意 message 或响应摘要。受限原始响应仍供 feature 显式处理，但不得记录；
路径及 operation 标签也不得承载秘密。

## Files and processes / 文件与进程

- `DownloadArchive` atomically creates a random temporary file, retains its owner
  through installation and lends its opened handle to the writer/readers. Flush
  and close the writer before extraction; drop each reader before rewinding.
  Never reopen the pathname. Dropping the owner cleans partial, failed and
  completed downloads. Extraction uses its own temporary-directory owner.
- Restart the executable directly with `OsString` arguments. Preserve release of
  the single-instance lock before spawn, recovery on spawn failure and shutdown
  after success. macOS app-bundle launching retains its existing `open -n` path.
- Windows external file opening passes one absolute path to `explorer.exe`.
  Do not use `cmd /C start`, expand environment variables or interpolate paths
  into a shell. Existing explicit user-action and file-existence checks remain.

- `DownloadArchive` 原子创建随机临时文件，持续持有到安装结束；写入和读取都使用
  已打开句柄。解包前 flush 并关闭 writer；各 reader 释放后才 rewind，禁止按路径
  重新打开。owner 析构清理未完成、失败和成功下载；解包目录由独立临时目录 owner 管理。
- 重启直接启动 executable，并用 `OsString` 保留参数。维持启动前释放单实例锁、
  启动失败恢复锁、成功后关闭旧进程的顺序；macOS bundle 沿用已有 `open -n` 路径。
- Windows 外部打开文件把一个绝对路径传给 `explorer.exe`，禁止 `cmd /C start`、
  环境变量展开和路径拼接 shell。保留显式用户动作与文件存在检查。

## Failure behavior and regression / 失败与回归

- Reject credential destinations with fake provider hostnames, changed ports or
  schemes; allow public and explicitly configured enterprise controls.
- Use two real loopback servers to prove a rejected redirect never contacts the
  second origin; also exercise same-origin authentication, anonymous CDN-style
  redirects and redirect loops.
- Exercise synthetic secrets through request URL, reqwest error, `Display`,
  ordinary/alternate `Debug` and JSON data errors. Keep a harmless endpoint control.
- Create a legacy-path symlink to a harmless fixture file; it must remain intact.
  Replace the new pathname after download and confirm extraction still reads the
  original handle. Check successful, partial and invalid-archive cleanup.
- Exercise spaces, CJK, `&`, `%`, quotes and shell-looking process arguments.
  Argument-construction tests do not substitute for Windows native file association
  and restart acceptance.
- Quality checks include root doctests. Lexical architecture guards require
  positive and negative fixtures, and do not replace type-aware review.
- Unit-test global settings use a retained temporary directory and defaults;
  timeout or credential tests must not depend on real workstation configuration.

上述正反例均属于交付回归：真实双监听端口证明拒绝跳转未触达目标，真实文件与
软链接证明原文件未被覆盖，进程参数必须保持字面值。参数构造测试不能替代 Windows
原生文件关联与重启验收。质量入口必须执行根包 doctest；词法架构门禁需要正反例，
不能替代类型感知审查。

Release signature verification and the existing Windows ZIP-versus-tar installer
gap are outside this contract's implementation. Track them explicitly rather than
describing temporary-file hardening as a fully verified updater.

发布签名验证及既有 Windows ZIP 资产与 tar 安装器不匹配问题尚未在本契约实现中
解决；必须明确追踪，不能把临时文件加固描述为更新器已完成全面验证。
