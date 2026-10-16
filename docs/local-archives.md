# Local historical archives / 本地历史归档

Historical benchmark evidence, validation output and old release artifacts live
in dated directories under the ignored `.local-archives/`. These archives are
local files and are not included in Git. They are not build or runtime inputs.
The first cleanup below used `.local-archives/20261014/`.

历史基准证据、验证输出及旧 Release 产物保存在已忽略的 `.local-archives/` 日期目录。
归档仅在本机保存，不进入 Git，也不是构建或运行输入。下表为 2026-10-14 首批清理。

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

<a id="validation-experiments-20261016"></a>

## Local validation — 2026-10-16 / 本地验证归档

本次新增的日期验证目录均为本地实验、报告与重放材料，不作为项目提交内容。
原有 1997 个未跟踪文件已校验归档并移除原位置；其中正式 Release 策略回归测试
所需的基线工作流逐字迁至
[`scripts/tests/fixtures/release-ci-reliability/`](../scripts/tests/fixtures/release-ci-reliability/baseline-release.yml)。
此前生成的 176 项提交清单已撤销并作为历史记录归档。
`docs/validation/` 只新增一份[本地输出规范](validation/README.md)，
原已跟踪的 159 个历史文件已在[后续清理](#validation-tracked-history-20261016)中
同样归档移除；三处 Python 缓存的 8 个文件已清理。

New dated validation materials are local experiments, not project test inputs.
All 1997 originally untracked files were archived and removed from their original
locations. The one baseline used by regular Release policy tests moved verbatim
into `scripts/tests/fixtures/`. The earlier submission inventory is historical
and no longer applies. Previously tracked validation history is covered by the
follow-up cleanup below.

| Archive under `.local-archives/20261016/` | Members | Original bytes | Archive bytes |
| --- | ---: | ---: | ---: |
| `validation-experiments.tar.gz` | 312 | 71,620,528 | 6,068,053 |
| `validation-raw-output.tar.gz` | 1511 | 237,962,035 | 47,412,083 |
| `validation-reports.tar.gz` | 176 | 1,768,234 | 492,625 |

第三份归档包含剩余 174 个本地报告/实验材料、已撤销的 `commit-manifest.json`，
以及迁移前 `scripts/tests/test_release_workflow_behavior.py` 的副本。该副本保留旧
`--record-ab` 本地记录器，供独立历史工作副本重放；当前项目测试只保留回归模型及
正反例，不再携带生成本地验收报告的命令。

The third package contains the remaining 174 local materials, the withdrawn
inventory, and a snapshot of the original behavior-test script with its local
recorder. Restore that snapshot only in a separate historical working copy.
The current project suite retains its regression model and positive/negative
cases without the experiment-output writer.

SHA-256:

- `validation-experiments.tar.gz`: `59c77a643bc61acd7cf49d0e34618212ea43c2ba961721265f0c0fbd13183116`
- `validation-raw-output.tar.gz`: `88cbc1facdbe41dd631475b9633b4c60c5640cda56add3c65a335c9e26a5a569`
- `validation-reports.tar.gz`: `a43d44176e3ac1c8189f4ba96c7443f6188df879175d514448886cd1e91c667d`

每份归档旁的同名 `.manifest.json` 列出精确成员路径、内容 SHA-256、大小、权限及
纳秒修改时间，并记录验证和删除状态。校验覆盖成员集合、逐文件内容与元数据，以及
实际恢复后的逐文件内容检查。只有验证通过后才删除原文件。

Each companion manifest records exact member names, content hashes, sizes, modes,
nanosecond timestamps and verification/removal status. Every member and an actual
extraction are checked before the original is removed.

文档中的“归档：…”保留原报告路径或原报告内的相对文件名，可在 manifest 中定位。
保留的正式文档仍记录已采用/否决结论与未测范围；原始 JSON/CSV 内的来源路径保持
原文。归档不构成新增的性能或原生验收通过证据。

Archive-labelled links retain original member paths. Current contracts retain
accepted/rejected decisions and limitations; archived data keeps historical paths
unchanged. Archival adds no new performance or native acceptance claim.

从仓库根目录检查、恢复到独立历史工作副本：

```sh
shasum -a 256 .local-archives/20261016/validation-*.tar.gz
validation_restore_dir=$(mktemp -d "${TMPDIR:-/tmp}/usher-validation.XXXXXX")
tar -xzf .local-archives/20261016/validation-experiments.tar.gz -C "$validation_restore_dir"
tar -xzf .local-archives/20261016/validation-raw-output.tar.gz -C "$validation_restore_dir"
tar -xzf .local-archives/20261016/validation-reports.tar.gz -C "$validation_restore_dir"
```

归档及配套 manifest 只在本机保存，不随 Git 分发。跨机器重放历史实验时需另外复制；
正常构建和项目回归测试不读取这些归档或本地日期验证目录。

Local archives and manifests are not distributed by Git. Copy them separately for
historical replay; normal builds and project regression tests do not read them.

<a id="validation-tracked-history-20261016"></a>

## Previously tracked validation — 2026-10-16 / 原已跟踪验证材料

对原受 Git 跟踪的 `docs/validation/` 文件采用相同标准：159 个文件均为历史
本地验证报告、日志、运行记录或旧算法对照补丁，不是项目测试代码或正式测试输入。
核对源码、测试、脚本及 CI 后未发现读取依赖，故全部校验归档后从仓库移除。
真正的回归测试仍在 `tests/`、`scripts/tests/` 及各 crate 中；此前迁移的
Release 基线夹具也保留在 `scripts/tests/fixtures/`。

The 159 formerly tracked files are historical local reports and output, not
project tests or their fixture inputs. Source, test, script and CI dependency
checks found no readers. They were verified and archived before removal;
actual regression suites and their fixtures remain in their owning test paths.

| Archive under `.local-archives/20261016/` | Members | Original bytes | Archive bytes |
| --- | ---: | ---: | ---: |
| `validation-tracked-history.tar.gz` | 159 | 1,518,688 | 308,140 |

SHA-256: `7cbc7ee81f6b8e1f811f4a1ebf41cd1ecb93a41ff20f8d068aadd2fcc85a8f18`

| Original directory under `docs/validation/` | Files |
| --- | ---: |
| `repository-cleanup-20261014/` | 14 |
| `security-maintenance-20261014/` | 1 |
| `spec-audit-20261014/` | 5 |
| `spec-gap-fixes-20261015/` | 27 |
| `spec-gap-followups-20261015/` | 42 |
| `structure-cleanup-20261014/` | 18 |
| `tray-locale-20261014/` | 4 |
| `ui-components-build-20261015/` | 6 |
| `ui120-macos-focus-20261015/` | 42 |

同名 `validation-tracked-history.manifest.json` 保留全部精确成员路径、内容
SHA-256、大小、权限、纳秒修改时间及校验/删除状态。归档验证覆盖成员集合与
逐文件内容/元数据，并实际解包核对 159 项内容；错误 hash 的负例也被正确拒绝。
`*-fixture.json` 在本批中是原生运行输出，由正式测试在临时目录重新生成，
没有把这些历史输出迁成项目测试输入。

The companion manifest records exact members, content hashes, sizes, modes,
nanosecond timestamps and verification/removal status. Verification includes
every member, metadata, an extraction smoke check and a rejected wrong-hash
control. The archived `*-fixture.json` files are generated native-run records;
project tests create their own temporary fixtures.

文档归档链接保留原成员路径；历史结果、采用决定及未测范围不因文件迁移而改变。
归档仅在本机保存，不随 Git 分发，正常构建及回归测试不依赖它。

Archive-labelled links retain member paths. Historical results, decisions and
untested scope are unchanged. This local package is not distributed by Git and
is not required by ordinary builds or regression tests.

```sh
shasum -a 256 .local-archives/20261016/validation-tracked-history.tar.gz
validation_restore_dir=$(mktemp -d "${TMPDIR:-/tmp}/usher-tracked-validation.XXXXXX")
tar -xzf .local-archives/20261016/validation-tracked-history.tar.gz -C "$validation_restore_dir"
```
