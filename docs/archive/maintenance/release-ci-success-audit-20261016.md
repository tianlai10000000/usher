# Release 成功运行审计 / Successful Release run audit — 2026-10-16

结论：缓存复用、避免 Bundle 重复编译、编译成功后提前保存缓存已有实际证据；
主编译耗时仍未解决，不能据此宣称整体构建稳定提速。这是既有云端运行的只读
观察，不是受控性能 A/B，也没有改变 Release 参数或触发新的构建。

## 来源与时间口径

- [成功运行](https://github.com/tianlai10000000/usher/actions/runs/38872541335)：
  `v0.0.10-alpha.1`，SHA `18a43c855a10005730e4d8a43a1199ae8580f72a`，attempt **2**。
- 成功 attempt 的 API 时间范围：北京时间 **16:37:20–16:59:52，22 分 32 秒**。
  首次触发至最终完成为 **43 分 53 秒**，包括失败 attempt 与重试间隔。
- 三个构建 job 并行。关键路径经过 macOS ARM64，然后发布与报告，不能相加
  三个平台的 job 时长。签名与公证本次均跳过。
- 原始证据留在本地忽略目录 `docs/validation/release-ci-success-audit-20261016/`：
  API 快照、各 target 的实际 `cache.json` / Cargo HTML、下载与成员哈希、日志事实
  摘要、独立时间核对。完整日志仅存本地临时目录，不进入文档或报告附件。

## 各阶段与缓存

以下阶段取 GitHub API step 的秒级时间；短于一秒的步骤可能显示零。安装、checkout、
清理及步骤间隙未全部列入分项，因此分项不必等于 job 总时长。

| 阶段 | macOS ARM64 | macOS x64 | Windows x64 |
| --- | ---: | ---: | ---: |
| registry + build 恢复 | 21 秒 | 28 秒 | 2 秒 |
| Build Release | **20 分 07 秒** | **17 分 17 秒** | **18 分 03 秒** |
| registry + build 保存 | 跳过 | 跳过 | 40 秒 |
| Bundle / tar / DMG 或 ZIP | 56 秒 | 60 秒 | 9 秒 |
| 产品 + 诊断上传 | 4 秒 | 6 秒 | 4 秒 |
| job 总时长 | **21 分 51 秒** | **19 分 18 秒** | **19 分 25 秒** |
| registry / build 命中 | **均精确命中** | **均精确命中** | **均未命中** |

发布 job 17 秒，报告 job 7 秒。ARM64 Build Release 占成功 attempt 总时长约 89%；
这是时间占比，不是优化收益。API 步骤标记成功并不等于缓存命中，以上命中结论同时
核对了实际缓存元数据与日志。

macOS 恢复的 build 压缩缓存分别为约 695 / 698 MB，恢复约 17 / 22 秒；Cargo 为
748 / 750 个 fresh 单元、各 24 个 dirty 单元。日志仅重编了 13 个 workspace path
包，没有第三方依赖的 `Compiling` 行。因此依赖复用有效，但项目自身并未完整复用。
包和 Cargo 单元不是同一计数口径。

Windows 是冷构建：Cargo fresh 0 / dirty 780，下载 543 个 crate，出现 466 条
`Compiling`。第一次 attempt 取消且未保存缓存；第二次才成功保存 registry 与 build
缓存，分别耗时 30 / 10 秒。新增 Windows 缓存路径已跑通，恢复后的实际收益未测。

## 主编译瓶颈

Cargo HTML 单元的 duration 是墙钟跨度，可能互相重叠，不能把所有单元相加。

| Cargo 单元 | macOS ARM64 | macOS x64 | Windows x64 |
| --- | ---: | ---: | ---: |
| MainWindow `usher-ui` Rust 库 | **13 分 00.7 秒** | **12 分 18.3 秒** | **7 分 51.7 秒** |
| 最终 `usher` 二进制单元 | **6 分 28.3 秒** | **4 分 23.0 秒** | **5 分 11.7 秒** |
| 两段串行合计 / Cargo 总时长 | **97.1%** | **96.9%** | **73.8%** |

三个 target 都是 MainWindow 库结束后紧接二进制单元。ARM64 库中 frontend 261.28 秒、
codegen 519.42 秒；最终二进制的报告没有细分 frontend / LLVM / LTO / linker，不能
把其 388.25 秒全部称为链接耗时。MainWindow build script（含 Slint 生成）仅
15.41 秒，x64 为 10.71 秒；主要时间花在生成代码后的 Rust 编译和最终二进制阶段。

Settings 库 ARM64 204.83 秒、x64 213.16 秒，但与 MainWindow 并行，不应直接加到
上述关键路径。Windows 在 MainWindow 开始前还有约 4 分 38 秒的冷依赖阶段；
`aws-lc-sys` build script 为 150.57 秒，也存在重叠。

当前生产 profile 仍为 `opt-level="z"`、完整 LTO、单 CGU、关闭增量编译。这些设置
涉及体积与编译成本取舍，但本次没有 CPU 采样或参数对照，不能据此给出 LTO 的
独立耗时或承诺修改后的收益。

## 之前优化的实际效果与边界

1. **Bundle 重复编译已在本次消失。**
   [旧成功运行](https://github.com/tianlai10000000/usher/actions/runs/38341728654)
   `v0.0.9-alpha.4` 的 Bundle 日志重新编译 MainWindow、Settings、messages 与根包，
   ARM64 / x64 步骤为 7 分 41 秒 / 7 分 12 秒。本次相应步骤为 9 / 10 秒，
   `Compiling` 均为 0，Cargo 检查仅 0.93 / 1.09 秒。跨源码、跨镜像的行为对照
   证明重复编译现象未再出现，不作为受控提速比例。
2. **工具缓存与提前保存有效。**第一次 attempt 的 cargo-bundle 安装要编译工具，
   第二次只检查已安装版本。第一次 macOS 产品上传失败前已经保存 build 缓存，
   第二次成功精确恢复，说明后续上传失败没有丢掉编译缓存。
3. **分层 fallback 与跨 tag 预热尚无实测收益。**本次 macOS 命中来自同一 tag、
   同一 SHA 的第一次 attempt；Windows 全 miss。没有实际命中严格或粗层 fallback。
   只读查询可见的 `workflow_dispatch` 运行数量为 0，尚无默认分支预热成功的证据。
   不同 tag 不能互读彼此缓存；后续新 tag 仍可能冷构建，见
   [GitHub 缓存作用域](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache)。
4. **没有整体稳定提速的证据。**旧成功运行 35 分 05 秒、本次 22 分 32 秒，但源码与
   runner 镜像不同。更需注意，同一 SHA 的首次 macOS Build 为 ARM64 9 分 26 秒、
   x64 9 分 30 秒，第二次精确命中缓存却为 20 分 07 秒、17 分 17 秒。公开镜像与
   Rust 版本相同不等于宿主性能和负载相同；日志也没有 fingerprint 原因或 CPU / 内存
   采样。因此既不能把总体缩短归功于缓存，也不能认定缓存导致变慢。
5. **保留期候选没有参与本次运行。**执行的 workflow SHA-256 为
   `88570576ff67103426ff4a67786999a23acccb1ed91660c65db0013bd3544d01`。
   本地 7 天产品 / 3 天诊断保留期改动尚未部署，本次实际上传仍为 90 / 14 天，
   不能把存储治理算作编译性能收益。

## 后续优先级与诊断缺口

1. 先在相同 SHA、相同工具链/配置下收集 Cargo fingerprint 原因，解释精确恢复后
   13 个 workspace 包为何仍 dirty，再重复冷/热对照。checkout mtime 只是待证假设；
   不能直接回填时间戳来绕过 Cargo 正确性检查，还须验证真实源码变化必然重编。
2. 单独验证默认分支预热与新 tag 消费，以及 Windows 后续恢复；当前结果只能证明
   保存与同 tag macOS 恢复，不能代替这些场景。此审计未触发云端构建。
3. 若主编译仍必需，优先研究 MainWindow 编译单元及最终二进制阶段，而不是继续
   优化十几秒的上传、缓存传输或 Slint 生成。Release codegen / LTO 变更必须另做
   受控构建时间、运行性能与体积 A/B，本次保持参数不变。

另有诊断健壮性问题：第一次 attempt 的取消发布 job API 时间结束比开始早 1 秒，
使严格校验的计时收集器退出 2。按实际快照已本地重现；异常时长在审计中保持未知。
第二次报告成功，但取消任务的该边界仍待单独修复，不属于主编译瓶颈。

## 验证与未执行项

- 7 项源数据/哈希/缓存分类/串行关键路径一致性检查全部通过；独立 API 时间核对
  与 macOS/Windows 日志分析相符。旧 Bundle 取证另有 7 项自检全部通过。
- 结论经第二位 agent 只读复核，无实质性矛盾；文档相对链接检查与
  `git diff --check` 通过，工作流及 Cargo manifest 在本次审计中未变。
- 下载并核验 4 个诊断 ZIP，没有下载产品附件；HTML 按数据解析，没有执行脚本。
- 本次只读 API/日志/附件分析与文档记录；没有重新运行 Cargo、修改应用/工作流、
  Release 参数、缓存或远端产物，也没有触发/重试云端运行。
- 不把本次观察称为优化批次验收或受控 A/B；运行时/体积回归、Windows 热恢复、
  默认分支预热与新 tag、分层 fallback、硬件负载归因均未执行。
- 保留审计前已有的工作区改动，包括 artifact 保留期候选及其文档/测试。
