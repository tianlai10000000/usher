# 2026-10-14 安全与可维护性清理记录

本次仅审计并实施用户批准的批次 1–5，没有执行第 6 批架构治理。基线为 `ad6e701071a6695370f9228eb5626218b7f63cff`，初始工作区干净，未涉及无关用户改动或 vendored Material。主机为 macOS，Rust 1.98.1。结论限于已证实的漏洞和维护缺陷；未建立恶意植入证据，不代表整个仓库不存在其它问题。本次不宣称性能收益，也没有执行 Release 性能 A/B。

后续第 6 批结构治理见 [结构清理记录](structure-cleanup-20261014.md)，本文保留批次 1–5 当时的验证结果。

本文是历史记录；当前约束见 [安全边界](../../specs/security-boundaries.md)、[HTTP 管线](../../specs/http.md) 和 [AGENTS.md](../../../AGENTS.md)。小型原始证据统一保存于 [raw-evidence.txt（归档：docs/validation/security-maintenance-20261014/raw-evidence.txt）](../../local-archives.md#validation-tracked-history-20261016)，每段保留来源文件名，excerpt 是原文摘录。没有保存真实配置、凭据或完整库测试日志。

## 批次 1：凭据目标与重定向

代码：`src/git/locator.rs`、`src/git/gitlab.rs`、`src/http/mod.rs`、`src/http/redirect_policy_tests.rs`。

源码核对发现，包含 `gitlab` 的任意主机可被识别为 GitLab，其远程地址决定 API 目标，但请求仍附加全局 Token。修复改为在同一设置快照中读取 Token 与可信地址，只授权 `https://gitlab.com` 或显式配置的精确 origin（协议、主机、有效端口），并收敛到一个 GitLab 鉴权入口。未配置自建服务仍可匿名读取。首次请求风险的基线是源码证据，没有运行向外部站点泄露真实 Token 的实验；候选 3/3 测试通过，其中新增 origin 测试 2 例，已有 URL Debug 测试 1 例。

锁定 reqwest 默认跳转不移除 `PRIVATE-TOKEN`。HTTP owner 现在集中持有匿名和凭据策略两种 client，在 interceptor 之后选择；凭据请求仅允许同 origin，保留跳转上限，匿名 CDN 仍可跨源。真实双 loopback 基线 2/3 通过，失败证明请求触达第二个 origin；候选扩展为 5/5 通过，覆盖拒绝跨源、同源鉴权、匿名跳转、循环和分类。最早的 sandbox 监听拒绝不算产品失败证据。采用修复；任意新 query 凭据仍须扩展识别，自定义鉴权 Header 应标记 sensitive。

## 批次 2：统一日志隐私出口

代码：`src/http/mod.rs`、`src/http/privacy_tests.rs`。

旧脱敏只识别 5 个 query key；userinfo、fragment、其它 query 值以及 reqwest error/派生 Debug/Serde data error 仍可能泄露秘密。修复移除诊断 URL 的全部 query、userinfo、fragment，非 HTTP(S) 使用固定标记；reqwest 格式化前 `without_url()`；JSON 错误仅保留类别和行列；Debug 仅输出元数据和响应长度。受限响应摘要仍可供显式业务处理，但不得写日志。

同组 7 例基线 1 通过、6 失败，候选 7/7 通过，后来增强的两条断言亦被最终库测试覆盖。采用修复；保留的 endpoint path 和 operation 自身不得承载秘密，不能声称能识别任意路径中的秘密。

## 批次 3：更新下载的文件所有权

代码：`src/update/download_archive.rs`、`src/update/mod.rs`、`src/update/updater.rs`。

旧版本号临时路径配合 `File::create` 可跟随预置软链接截断无关文件，安装时重开路径还会丢失文件身份。现在由 `NamedTempFile` 原子创建随机文件，owner 持有至安装结束；writer/归档 reader 使用已打开句柄，flush/关闭 writer 后读取，reader 释放后才 rewind。取消、错误与成功统一析构清理。删除无调用方的旧 `perform_update` 兼容入口，避免另一条更新路径绕过现有流程。

真实 Tokio `File::create` 的 macOS/Unix 基线夹具使合成 victim 从 38 字节变为 0；仅注入独立临时父目录，没有 HTTP 或安装。候选 4 项文件测试通过，涵盖重复读取、提前错误、无效归档、旧路径及下载后新路径被软链接替换。采用修复，但已有 Windows ZIP 资产与 tar.gz 安装器错配本次未修，不能据此宣称更新器全面安全。

## 批次 4：文件和参数不经过 shell

代码：`src/platform/mod.rs`、`src/update/updater.rs`。

Windows 原先通过 `cmd /C start` 打开文件和重启。现在文件以单一绝对路径参数传给 `explorer.exe`，重启直接指定 executable 并保留 `OsString` 参数；单实例锁释放/失败恢复顺序及 macOS bundle 的 `open -n` 分支保留。

保存了旧/新 command program/args 的源码核对和 macOS 对象检查；这不是 Windows 原生执行。文件打开参数测试及两项重启测试通过，其中 Unix 真实子进程验证空格、`&`、`$(...)`、`%PATH%` 和引号保持字面值。采用构造修复，Windows 文件关联、编码及实际重启仍待验收。

## 批次 5：门禁、文档示例与测试隔离

代码：`scripts/check-architecture-structure.sh`、`scripts/check-quality.sh`、`scripts/tests/test_architecture_checks.py`、`scripts/tests/test_quality_checks.py`、`src/logger/debug_logger.rs`、`src/settings.rs`。

HTTP 门禁补查 shortcut、默认构造、ClientBuilder、导入别名和通配符。独立夹具覆盖 18 种违规形式，旧版漏掉 15 种，候选全部拦截；7 个顶层测试通过，合法 owner 和既有预算正反例保留。没有提高阈值。它仍是词法检查：注释/字符串可能命中，宏、依赖重命名及间接 re-export 仍须审查。

logger 示例改为真实可编译的公共 `log` 宏用法，quick/full 均加入独立 root doctest，分别为 9/11 项。runner 基线出现 7 个失败断言，包括两模式漏跑 doctest；候选 10/10 通过，故意失败的 doctest 保留 exit 101 并阻断最终结果。

首次库测试 826 通过、1 失败、2 ignored，logo 超时用例单独复跑仍失败：它假定 30 秒默认值，全局设置却读取了机器上的 60 秒配置。修复仅在 `cfg(test)` 下从生命周期受保护的私有临时目录加载默认设置，新增只读隔离断言，并禁止测试调用生产 `SettingsManager::new`。没有改真实配置、放宽超时或跳过测试。最终库测试 **828 通过、0 失败、2 个既有 ignored**；首次失败原文仍保留。采用修复。

## 最终验证与边界

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check`、`git diff --check` | 均 exit 0 |
| `cargo check --locked`、最终 `cargo test --lib --locked --no-run` | 均通过，无 warning；后者确认测试构造器隔离的最终编译 |
| `cargo test --lib --locked` | 828 通过、0 失败、2 ignored，34.40 秒；ignored 为真实 AI Provider 和内存 A/B 专项 |
| `cargo test --doc --locked` | 1 通过、0 失败、1 个既有 ignored；logger 示例通过 |
| Python 脚本套件 | 85/85 通过；输出中的模拟 runner exit 7 属于负例 |
| UI policy / i18n catalog | 均通过；不等于人工 UI 验收 |
| 实仓结构门禁 | 仍被下述 5 项既有超限阻断，没有豁免 |

既有超限：AI Review controller 3884/3850 行；root Slint 3101/3000 行、callbacks 142/140、properties 425/410；PR Review remote actions 1150/580 行。没有完成第 6 批结构治理，不能写成整个 `check-quality.sh --full` 已通过。

未执行 Windows 原生构建/文件关联/重启、线上 Provider 请求、真实更新安装、签名验证或 Release 性能 A/B；Windows ZIP/tar 错配仍未修复。源码核对、夹具执行、命令对象检查与目标平台验收是不同层次的证据。

## 可重复命令

从仓库根运行；loopback 夹具需本地监听权限，不需要线上服务或真实 Token。以下 baseline ref 失败是预期证据，不能计作候选通过。

```sh
cargo test --lib --locked http::privacy_tests
cargo test --lib --locked http::redirect_policy_tests
cargo test --lib --locked git::locator::tests
cargo test --lib --locked update::download_archive::tests
cargo test --lib --locked restart
cargo test --lib --locked windows_file_open
USHER_TEST_ARCHITECTURE_BASE_REF=ad6e701071a6695370f9228eb5626218b7f63cff python3 -m unittest scripts.tests.test_architecture_checks
python3 -m unittest scripts.tests.test_architecture_checks
USHER_TEST_QUALITY_BASE_REF=ad6e701071a6695370f9228eb5626218b7f63cff python3 -m unittest scripts.tests.test_quality_checks.QualityRunnerTests
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cargo test --lib --locked
cargo test --doc --locked
sh scripts/check-architecture-structure.sh
```

隐私基线使用同组 7 例；redirect 历史基线仅最初 3 例，候选扩展为 5 例，不应写成同组总数。再做 HTTP 旧实现对照时应在独立临时 checkout 接入夹具，不回退当前树。原始证据末尾保存了 `baseline-updater-symlink.rs` 与 `windows-command-shapes.rs` 合成程序；提取到独立临时目录，按对应日志的 rustc argv 编译。前者执行参数必须是新建专用夹具目录，Tokio rlib/绝对路径需对应本机；后者只检查 command 对象，未启动 Windows 进程。

## 后续 Agent 的经验

1. Provider 识别不等于凭据授权；核对首次请求、redirect 和同一配置快照，不能只匹配主机子串。
2. 脱敏覆盖所有格式化出口；截断不等于脱敏，派生 Debug 和底层错误也须用合成秘密验证。
3. 文件身份依靠句柄和 owner；随机命名后重开路径仍可能丢失身份，清理必须覆盖全部退出路径。
4. 路径/参数保持 `Path`/`OsString`，不拼入 shell；对象单测不能替代 Windows 原生验收。
5. 搜索调用方后删除危险备用入口，不以“兼容”为由保留绕过当前策略的重复流程。
6. 测试从私有夹具读取默认状态，不能改用户配置、扩大等待或忽略失败来制造全绿。
7. 门禁需正反例和基线对照；声明词法局限，保留失败原文，不能提高阈值或永久豁免历史失败。
