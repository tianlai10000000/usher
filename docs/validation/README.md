# Local validation output / 本地验证输出

`docs/validation/` 用于本地验证结果、一次性实验和临时测试代码。除本页外，
新增内容默认由根 `.gitignore` 忽略，不作为提交清单。

真正的项目回归测试放在 `tests/` 或 `scripts/tests/`，所需小型输入放在对应
`fixtures/` 中。例如 Release 策略测试的
[基线工作流](../../scripts/tests/fixtures/release-ci-reliability/baseline-release.yml)
由正式测试直接读取，不依赖本地实验目录。

需要长期维护的行为与结论写入现有架构或 spec；本地原始证据先校验归档，再清理
工作副本。恢复方法见[本地归档](../local-archives.md#validation-experiments-20261016)。
原受 Git 跟踪的 159 个本地历史记录也已校验归档并从仓库移除，见
[历史验证归档](../local-archives.md#validation-tracked-history-20261016)。
正式项目测试及其输入保留在测试目录。

New dated reports, one-off experiments and local test tools stay out of Git.
Project regression tests and their fixture inputs belong in `tests/` or
`scripts/tests/`. Promote stable contracts into architecture/spec documents;
verify and archive local evidence before removing working copies. The 159
previously tracked local records have also been archived and removed; project
regression tests and their inputs remain in the test directories.
