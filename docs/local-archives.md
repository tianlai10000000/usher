# Local historical archives / 本地历史归档

The 2026-10-14 cleanup preserves historical benchmark evidence and old release
artifacts in the ignored `.local-archives/20261014/` directory. These archives
are local files and are not included in Git. They are not build or runtime inputs.

2026-10-14 清理将历史基准证据及旧 Release 产物保存在已忽略的
`.local-archives/20261014/`。归档仅在本机保存，不进入 Git，也不是构建或运行输入。

| Archive | Original paths / 原路径 |
| --- | --- |
| `benchmark-history.tar.gz` | `memory-benchmark-results/`, `sidebar-motion-benchmark-results/`, `chat-interaction-benchmark-results/`, `artifact-preview-interaction-benchmark-results/`, `artifact-preview-transition-benchmark-results/`, `artifact-preview-scroll-benchmark-results/` |
| `old-release-artifacts.tar.gz` | `dist/`, `workflow-dist/`, `dist-workspace.toml` |

Each archive has a matching `.manifest.json` recording the archive SHA-256 and
every retained file's SHA-256, size, timestamp and mode. Creation hashes the
source bytes; verification reads every archived file and compares that digest.
Only verified archives permit removal of the original directories. Finder
`.DS_Store` metadata is excluded. Original relative paths remain unchanged inside
the archive, including historical source snapshots, A/B inputs, raw results and
release files. Any old documentation path into a removed output directory refers
to that same path inside the archive.

每份归档都有对应 `.manifest.json`，记录归档 SHA-256 及每个保留文件的 SHA-256、
大小、时间与权限。创建时计算源内容 hash，验证时逐文件读取归档并对照；只有
验证成功才移除原目录。Finder `.DS_Store` 元数据不进入归档。内部保留原相对路径，
包括历史源码快照、A/B 输入、原始结果及发布文件。历史文档中已移除产物目录的路径，
对应归档内部的相同路径。

Inspect or restore from the repository root into a separate directory:

```sh
tar -tzf .local-archives/20261014/benchmark-history.tar.gz
archive_restore_dir=$(mktemp -d "${TMPDIR:-/tmp}/usher-history.XXXXXX")
tar -xzf .local-archives/20261014/benchmark-history.tar.gz -C "$archive_restore_dir"
```

The current benchmark and release scripts recreate their ordinary output
directories when run. Cleaning Cargo `target/` removes programs and generated
files; the next build regenerates them. Before running the Python script suite
alone after cleanup, run `cargo check -p usher-i18n --locked` to prepare the build
script used by the catalog regression. `scripts/check-quality.sh` already orders
this prerequisite before script tests.

当前基准及发布脚本运行时会重新创建各自的输出目录。清除 Cargo `target/` 会
移除程序与生成文件，下一次构建会重建。清理后若单独运行 Python 脚本测试，应先
执行 `cargo check -p usher-i18n --locked`，准备 catalog 回归使用的构建脚本；
`scripts/check-quality.sh` 已按此顺序执行。

The three obsolete conflict design PNGs are tracked under
`docs/archive/pr-conflicts-20261014/`; the design README links to those files.
`optimizing-proposal.md`, `code-quality-proposal.md` and
`memory-rss-diagnostics.md` remain at the repository root. The first was renamed
from `claude-proposal.md` with identical contents. `ai-review-test-rule.md` remains
because the local AI Review configuration currently selects it.

三张旧冲突设计图保存在已跟踪的 `docs/archive/pr-conflicts-20261014/`，设计 README
已修复链接。`optimizing-proposal.md`、`code-quality-proposal.md` 和
`memory-rss-diagnostics.md` 仍在根目录；首份文件由 `claude-proposal.md` 原内容改名。
`ai-review-test-rule.md` 正被本机 AI Review 配置选用，因此保留。
