# Terminal Batch D Returned-Home Attribution (2026-09-28)

## Decision

**Attribution-only batch; no Terminal product optimization adopted.** Opening a
real same-project PTY, executing a deterministic command, closing the drawer,
and returning Home added only **3.203 / 4.938 MiB RSS** over a symmetric
Workflow-only control. Both rounds missed the predeclared 5 MiB owner screen,
and graphics dirty differed by only **0.375 / 0.141 MiB**.

中文结论：本批没有把 Terminal 圆角裁剪或会话生命周期当作猜测性优化。相对只进入
Workflow 的对照，真实 Terminal 操作在两轮回到 Home 后只多保留
**3.203 / 4.938 MiB RSS**，未达到两轮都至少 5 MiB 的筛选线；图形脏页差异也仅为
**0.375 / 0.141 MiB**。因此没有修改产品 UI，也没有破坏同项目 PTY、内容与 cwd
保留契约。

## Question and ownership boundary

The all-page run had measured a `+6.625 MiB RSS / +5.375 MiB footprint`
Workflow-plus-Terminal marginal increase, but that sequential result could not
separate the Workflow route from the Terminal drawer and PTY. Batch D used two
fresh processes from one frozen binary:

- `workflow-only`: enter Workflow, wait for the same active interval, return
  Home, and repeat;
- `terminal`: enter Workflow, open a real PTY for the same project, submit a
  marker command through the production callback, observe its output, close
  the drawer, return Home, and repeat.

The treatment deliberately retained the same-project Terminal session after
the drawer closed. Clearing the session, killing the PTY, resetting cwd, or
destroying stable Workflow state would violate the existing lifecycle contract
and was never considered a valid RSS optimization.

## Frozen source build

| Field | Value |
| --- | --- |
| Git HEAD | `181e457b43d944701d434dd2bd86504853de4f21` |
| Build | `cargo build --release --locked --features test-fixtures` |
| Artifact | `baseline-03e1be5` |
| Artifact SHA-256 | `03e1be59b2ce76314c9041d3f6d38ef961c8cfcf2294fade0e62b1c6073c281f` |
| Cargo.lock SHA-256 | `2f87849fe4319e952d1af35509bd232852d724adaa96956e7929cfb7447118a8` |
| Backend | Winit/FemtoVG, two Tokio workers |
| Window | 800x600 logical at scale 2.00 |
| State | Offline and isolated per process |

The executable was copied directly from the repository Release build to a
frozen evidence path. No installed application binary was used.

## Non-activating protocol

The fixture selected a versioned benchmark-only Winit presentation before
constructing `MainWindow`. On macOS it used Accessory activation policy,
disabled activation over other applications, kept the native window at the
bottom, and disabled cursor hit-testing. All navigation and Terminal actions
ran through Slint event-loop callbacks. The driver contains no AX action,
`CGEvent`, mouse movement, keyboard injection, or application activation.

An independent `NSWorkspace` monitor sampled the frontmost PID every 100 ms and
would reject the run if Usher ever became frontmost. The preliminary smoke run
and both measured arms passed; the two measured logs contain 1,563 and 1,562
non-Usher samples respectively. This protocol therefore did not take over the
user's mouse or foreground application.

Each arm settled initial Home for 30 seconds, retained five RSS and footprint
samples plus categorized footprint and `vmmap`, performed the assigned route
operation, returned Home for 30 seconds, and repeated once. Every sampled phase
had zero RSS and footprint spread, zero compression, the expected route and
geometry, and a clean shutdown. The initial RSS difference was 0.141 MiB.

## Pilot result

The attribution value is the treatment's returned-Home residual minus the
control's returned-Home residual.

| Round | Workflow-only RSS residual | Terminal RSS residual | Terminal contrast | Workflow-only footprint residual | Terminal footprint residual | Footprint contrast |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | +18.953 MiB | +22.156 MiB | **+3.203 MiB** | +24.547 MiB | +20.766 MiB | -3.781 MiB |
| 2 | +17.297 MiB | +22.234 MiB | **+4.938 MiB** | +22.109 MiB | +18.719 MiB | -3.391 MiB |

The simpler absolute returned-Home treatment-minus-control values were also
below the screen: `+3.063 / +4.797 MiB RSS`. The Workflow-only arm itself
retained `+18.953 / +17.297 MiB RSS`, so the earlier aggregate signal was not a
clean Terminal owner.

The category deltas do not support a rounded-drawer rendering candidate:

| Category contrast | Round 1 | Round 2 |
| --- | ---: | ---: |
| Graphics dirty | +0.375 MiB | +0.141 MiB |
| Malloc Large dirty | +2.359 MiB | +4.563 MiB |
| Malloc Small dirty | +0.766 MiB | -0.734 MiB |
| Mapped-file clean | +0.281 MiB | +0.281 MiB |

The only sizeable positive category was Malloc Large, and it still did not
produce a two-round 5 MiB RSS owner. Removing the drawer's rounded clip would
target a graphics mechanism that this experiment did not observe.

## Evidence limitations found during audit

The generated comparison is mechanically `valid` and `stable` under the v1
summarizer, but two audit findings narrow what may be concluded:

1. Initial physical footprint differed by 7.203 MiB, almost entirely an
   IOSurface baseline difference. Both arms reached 36.641 MiB IOSurface dirty
   after the route operation. The negative footprint contrasts are therefore
   not evidence that Terminal reduces memory; footprint attribution is
   unbalanced. This does not rescue the failed positive RSS screen.
2. The v1 probe seeded and verified `run-0/settings.toml`, while `app_paths`
   selected `run-PA` and `run-PB`. Both actual settings files were identical
   (SHA-256 `316d7b62...`), so the arms remained symmetric, but the reported
   “settings unchanged” assertion checked the wrong file. The next batch must
   bind its seed path to the run ID, verify the path actually logged by the
   application, and require initial footprint balance of at most 5 MiB.

Because these limitations can only weaken attribution, not turn the measured
sub-threshold result into a passing owner, the run is retained as a conservative
rejection rather than repeated until it passes.

## Resulting boundary

- Do not change Terminal drawer clipping based on this run.
- Do not clear or kill the same-project PTY when the drawer closes.
- Do not reinterpret the Workflow-only high-water as Terminal ownership.
- Keep the non-activating, event-loop-only presentation support as measurement
  infrastructure; it is compiled only by `test-fixtures` and has no production
  behavior when its explicit environment mode is absent.
- Move the next attribution batch to Project, where the earlier independent
  signal was `+14.922 MiB RSS / +7.703 MiB footprint`, and split empty shell,
  project-card trees, and distinct Logo resources before proposing code.

## Evidence

- Protocol: `memory-benchmark-results/terminal-home-batch-d-20260928/protocol.md`
- Raw arms: `memory-benchmark-results/terminal-home-batch-d-20260928/runs/PA/`
  and `runs/PB/`
- Machine-readable comparison:
  `memory-benchmark-results/terminal-home-batch-d-20260928/pilot-attribution.json`
- Foreground-safety smoke:
  `memory-benchmark-results/terminal-home-batch-d-20260928/runs/focus-smoke-v1/`

The evidence directory is intentionally Git-ignored. This tracked report is the
decision record.
