# Usher

[简体中文](README.zh.md) · [Architecture](docs/architecture.md) · [Specifications](docs/specs/README.md)

Usher is an alpha-stage desktop workspace for managing local Git projects and repetitive delivery workflows. It combines project metadata, pull-request summaries, guided Branch/Tag/PR/CD flows, an embedded terminal, chat assistance, update management, and desktop notifications in a Slint application.

## Highlights

- Manage local repositories, aliases, tags, logos, and remote repository URLs.
- Inspect open PR/MR counts for `fat`, `uat`, and `prod` across GitHub, GitLab, and Gitee-compatible servers.
- Run project-scoped Branch validation, Tag automation, staged CD, and pull-request workflows.
- Open a project-aware terminal drawer without leaving the current page.
- Use one Chat page for OpenAI-compatible AI conversation or isolated RAG conversation, with separate histories.
- Run up to eight configurable concurrent AI code reviews; inspect findings beside the PR diff and chat with the configured AI Agent using the current pull request as context.
- Receive durable in-app messages plus native OS notifications, alongside toast, tray, update, and network-status feedback.
- Switch between light, dark, and system theme modes.
- Switch the UI language at runtime using the system locale or any packaged catalog.
- Route all application HTTP traffic through a shared reqwest-based interception pipeline.

> Usher is under active development; several secondary routes are development surfaces rather than finished product features.

## Supported targets

The maintained desktop targets are:

- macOS, with native title-bar, tray, bundling, and DMG support.
- Windows, with release packaging support.

Linux may build with a suitable Slint backend, but it is not currently covered by the release workflow. WebAssembly and Android instructions from the original template have been removed because the current desktop dependencies and platform integrations are not maintained for those targets.

## Requirements

- Rust 1.98.1, pinned by `rust-toolchain.toml`, with the 2024 edition available.
- Git in `PATH` for workflow and repository operations.
- Platform build dependencies required by Slint/winit.
- Optional: `cargo-bundle` v0.12.0 and `create-dmg` v1.3.0 for macOS packaging. The
  release scripts install the pinned `cargo-bundle` version automatically.

Production builds enable only FemtoVG, which keeps renderer selection deterministic and avoids linking Skia or Slint live-preview support into the normal application. Skia and live preview remain explicit development options.

## Quick start

```sh
git clone https://github.com/tianlai10000000/usher.git
cd usher
cargo run
```

Useful development commands:

```sh
./scripts/check-ui-policy.sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
git diff --check
```

Run local quality checks without rewriting source:

```sh
./scripts/check-quality.sh          # Default quick layer; --quick is equivalent
./scripts/check-quality.sh --full   # Workspace and feature contracts
```

Both layers run formatting, architecture/UI/i18n gates, script tests and whitespace
checks. Quick adds the default root check and root library tests. Full instead runs
workspace all-targets check, workspace unit/integration tests, workspace all-targets
Clippy with warnings denied, and an all-targets check of the `usher` application
without its optional AI core and UI/Settings bridges. Full sets `CARGO_INCREMENTAL=0`
and enables Slint debug metadata for workspace tests, matching manual CI. Workspace
tests deliberately exclude the separately tracked logger doctest.

Every ordinary failure is reported with its original exit code; later checks still
run and the aggregate exit status remains nonzero. Interrupts stop the sequence.
The commands require `cargo`, `rg`, `python3`, and `git`; HTTP fixtures need localhost
listening permission. Known structure and lint failures remain blocking. The manual
macOS CI job uses the full entry; Windows retains its separate default workspace
check. This does not enable hosted schedules or replace targeted UI/Release A/B
checks. The original structure ownership plan is in [batch 64](docs/p6-64-quality.md).

AI Code Review is a detachable Cargo feature enabled by the default product
build. For a constrained build without its core package or UI/settings bridge:

```sh
cargo check --no-default-features --features renderer-femtovg --locked
```

Everyday `dev` builds optimize third-party dependencies and generated Slint UI
crates at level 1 while keeping application-owned Rust at opt0. `dev` and
`test` retain source line tables and incremental compilation, but avoid macOS's
default unpacked debug objects. This keeps application panic backtraces and
source breakpoints useful while reducing ordinary `cargo run` code residency.
When full LLDB variable and type inspection is required, use the isolated
heavyweight profile:

```sh
cargo build --profile dev-full
```

For routine optimized local builds, use the isolated fast-release profile. It
keeps the release strip, size optimization, and abort policy, but uses Thin LTO,
eight codegen units, and incremental compilation for shorter rebuilds:

```sh
cargo build --profile release-fast --locked
cargo bundle --profile release-fast --format osx
```

This profile writes to `target/release-fast` and is not used by release
automation. Final distributable artifacts still use `--release` through the
existing release scripts.

IDE background diagnostics use a compact, non-incremental profile so repeated
workspace-wide checks do not duplicate the normal dev cache. VS Code/Cursor
uses it automatically; run the equivalent check manually with:

```sh
cargo ide-check
```

After changing from the old profile policy, run `cargo clean` once if the
existing `target/` still contains historical unpacked objects. See
[build performance and artifact policy](docs/build-performance.md) for the
measured trade-offs and cleanup guidance.

For UI/Rust iteration that does not touch the PR Review detail page, use the
explicit development shell. It keeps a separate target directory and replaces
the complete PR Review feature package with a protocol-compatible empty
implementation:

```sh
./scripts/dev-shell-check.sh
./scripts/dev-shell-run.sh
```

PowerShell equivalents are `scripts/dev-shell-check.ps1` and
`scripts/dev-shell-run.ps1`. The mode intentionally redirects attempts to open
PR Review back to Workflow. Default builds and every existing release command
still compile the complete page; use normal `cargo check --locked` and
`cargo run --locked` before delivering PR Review or final application changes.

In-process Slint live preview is development-only and excluded from normal and
release builds.

The fast UI check requires the matching stable viewer:

```sh
cargo install slint-viewer --version 1.18.1 --locked
```

Use the project scripts for fast UI validation and visual iteration:

```sh
./scripts/ui-check.sh
./scripts/ui-live-preview.sh
```

PowerShell equivalents are `scripts/ui-check.ps1` and
`scripts/ui-live-preview.ps1`. The check validates the complete root UI without
starting the application. Live preview keeps application callbacks and state,
but UI edits are interpreted at runtime; use a normal `cargo run --locked` for
the final static-build and route/layout check. Repeated reloads can retain
substantial diagnostic state, so restart the preview process during long UI
sessions. On macOS the opt-in preview path
uses the documented local Slint watcher patch under
`vendor/i-slint-live-preview`; default and release builds do not select it.

The UI Components playground is also development-only. Normal `cargo run` and
all existing release scripts omit its menu entry and compile graph. The live
preview script enables it automatically; the equivalent direct command is:

```sh
SLINT_LIVE_PREVIEW=1 cargo run --features live-preview,ui-components
```

The playground's **Artifact preview fixtures** section opens Markdown, PNG,
plain-text, JSON, and unsupported-binary samples through the production Preview
registry and host. `ui-components` enables the fixture bridge automatically;
normal and release builds do not compile the samples or expose their controls.

To inspect PR-list layout, review and conflict statuses, use the existing
fixture command below. It opens the mock list automatically in offline mode
with a fresh temporary state directory; no configured project is needed.
The 36 records cover every review/conflict state, Chinese authors, long titles
and branches, missing branches, and pagination. Normal runs never load them.

```sh
./tests/run-pr-list-review-status-fixture.sh
```

Optional environment variables: `USHER_PR_LIST_WIDTH=800`,
`USHER_PR_LIST_HEIGHT=600`, `USHER_PR_LIST_LOCALE=zh-CN` (or `en`), and
`USHER_PR_LIST_PROVIDER=gitee` (or `github`/`gitlab`). These select fixture
presentation only; they do not validate live provider APIs.

Use the interactive offline approval/merge fixture to inspect detail actions and
list reentry:

```sh
./tests/run-pr-approval-state-fixture.sh --list --locale zh-CN
./tests/run-pr-approval-state-fixture.sh --detail --locale en
```

Open includes required, approved and changes-requested rows; Closed includes
merged and unmerged closed rows. Approve updates mock data immediately while the
hidden list retains its model, then returns to the list after two seconds. It
never merges; a merge-ready row requires explicit confirmation. This tool uses
the production Slint UI with mock callbacks, reads no normal configuration and
contacts no provider. Library regressions separately verify shared data and
worker ordering.


The full `cargo test --locked` command currently exposes a pre-existing failing doctest in `src/logger/debug_logger.rs`; the library unit tests pass.

To inspect the optional Skia path:

```sh
SLINT_BACKEND=winit-skia cargo run --no-default-features --features renderer-skia
```

To compare the experimental FemtoVG/WGPU path (not the production default):

```sh
SLINT_BACKEND=winit-femtovg-wgpu cargo run --no-default-features --features renderer-femtovg-wgpu
```

Debug builds enable five-second process-memory samples and lifecycle checkpoints by default; they can be disabled under **Settings → Diagnostics → Memory diagnostics**. To force the same diagnostics in a release benchmark:

```sh
USHER_MEMORY_DIAGNOSTICS=1 ./usher
```

Filter for `[Memory][Diag]`, `[Memory][UI]`, and `[Workflow][PR Review][Memory]` to correlate RSS/physical-footprint changes with window size and PR model/cache ownership.

For interactive troubleshooting, open **Settings → Diagnostics** and manually enable only the output destination and detail source needed for the reproduction. `Terminal verbose diagnostics` and `Memory diagnostics` add records but still require `Console output` and/or `Application file logs` to make those records visible. Use `Enable all` only for a short cross-cutting capture, then restore the previous rows or select `Disable all`. Record the reproduction time and avoid entering or sharing secrets while Terminal verbose capture is active. See the [diagnostic logging specification](docs/specs/diagnostics.md) for the switch matrix, file destinations, and collection checklist.

On macOS, run the repeatable two-cycle release benchmark with:

```sh
scripts/benchmark-memory.sh --runs 3 --scenarios home --fixture home-v1
```

The command builds the selected release renderer, starts a fresh process per run, exercises fixed window/fullscreen phases, and writes raw TSV samples, summaries, and application logs under the ignored `memory-benchmark-results/` directory. Window automation requires Accessibility permission. Use `--prepare-hook` for fixed Workflow, PR Review, Chat, or Terminal fixtures; the executable receives `PID RUN SCENARIO`. See `--help` for renderer and sampling options.

The [P6 regression protocol](docs/p6-regression.md) documents the on-demand
Terminal/Hooks/Job suite, paired Release runs and real 15-minute workloads.

The hosted Nightly performance schedule is temporarily paused to avoid
automatic macOS runner usage. The complete workflow remains available through
manual `workflow_dispatch`; restore its commented `schedule` block when daily
performance gates should resume.

## Configuration and local data

Usher uses `directories::ProjectDirs::from("com", "usher", "app")` to select platform-native locations.

| Data | Relative location |
| --- | --- |
| Settings, Git platform tokens/API roots, and AI Agent credentials | config directory: `settings.toml` |
| Project registry | local data directory: `project/projects.json` |
| RAG / AI Chat histories | local data directory: `im/chat_history.jsonl` / `im/ai_chat_history.jsonl` |
| AI Review rules, default, project bindings, concurrency | config directory: `ai-review.json` |
| AI Review task summaries and completed sessions | local data directory: `ai-review/` |
| Durable notification inbox and read state | local data directory: `messages/messages.json` |
| Logs | local data directory: `logs/<date>/usher.log` and `logs/<date>/usher_tracing.log` |
| Cached remote project logos | temporary directory: `usher-logo-cache/` |

GitHub, GitLab, and Gitee tokens can be managed from Settings. Gitee public and enterprise credentials are separate; the repository remote host selects public v5 or the configured enterprise v8 API without sharing tokens. The **Code Hosting API Configuration** section stores a per-provider enterprise API URL and enterprise name under `[git.platform.<provider>]`; enter either the service host or the complete REST API root (for example, `https://git.example.com` or `https://git.example.com/api/v4`). The provider version path is added when omitted. These values are local-only and used only when the configured API host matches a repository remote. The **AI Agent** section accepts an OpenAI-compatible Base URL, API Key, Model Name, and reply language; that language applies to AI Chat, AI Code Review, and Review Ask, while `auto` follows the active interface language and RAG remains unchanged. **AI Review** configures reusable local Markdown rules, the default rule, and the concurrent task limit. Settings are saved under `[ai.agent]` and `ai-review.json`. All built-in Agent instructions live in the source-controlled `src/ai/prompts.toml` catalog. Do not commit generated settings, logs, histories, review sessions, or credentials. See [AI Agent configuration](docs/specs/ai-agent.md) and [AI Code Review](docs/specs/ai-code-review.md).

## Architecture at a glance

```text
src/main.rs
  -> src/lib.rs                application composition and callback bindings
     -> crates/usher-ui/       generated Slint API boundary
     -> crates/usher-i18n/     locale registry and shared Rust/Slint catalogs
        -> ui/main.slint       root window and exported UI contract
           -> ui/views/        page-level views
           -> ui/components/   reusable application components
     -> src/app_runtime.rs     shared Tokio runtime
     -> src/http/mod.rs        shared HTTP clients and interceptors
     -> src/platform/          native appearance, title bar, window interaction, fonts
     -> src/project/           project registry, logos, PR summaries
     -> src/jobs/              reusable Job model and application run route projection
     -> src/workflow/          Branch / Tag / PR / CD state and execution
     -> src/terminal/          PTY session and terminal rendering
     -> src/chat/              shared AI/RAG chat, isolated histories, UI binding
     -> crates/usher-ai-review/ optional headless review domain and scheduler
     -> src/ai_review/         optional review controller and persistence
     -> crates/usher-preview/ provider registry and Markdown/image/text preparation
     -> src/artifact_preview/ shared docked/detached preview hosts
     -> crates/usher-messages/ durable message center, OS delivery, isolated inbox UI
     -> crates/usher-search/   shared text matching, result navigation, viewport reveal
     -> src/update/            update checks, download, install
     -> src/{toast,notification,message_center,dialog}.rs
material-1.18.1/            exact Material source from the Slint v1.18.1 tag
```

Read [docs/architecture.md](docs/architecture.md) for ownership, lifecycle, data flows, and extension points. Coding agents must also follow [AGENTS.md](AGENTS.md).

## HTTP interception

All application-owned HTTP calls use `src/http/mod.rs`, which wraps reqwest instead of replacing it. The module provides:

- one shared request client for every application-owned HTTP call;
- request, response, and error interceptor hooks;
- normalized `HttpError` categories and status handling;
- independent per-request total and response-body idle timeouts;
- per-request error feedback: silent, toast, notification, or both;
- retained access to reqwest request builders plus centralized JSON, text,
  bytes, and streaming body reads.

Background polling must remain silent. User-initiated requests may opt into UI feedback. See [docs/specs/http.md](docs/specs/http.md).

## Release packaging

Build and validate local release artifacts with:

```sh
./scripts/verify-release.sh
```

Pass a target explicitly when it cannot be inferred:

```sh
./scripts/verify-release.sh x86_64-pc-windows-gnu
```

The GitHub release workflow produces macOS archives/DMGs and Windows ZIP bundles.
A pushed `v*` tag is the only automatic build trigger and starts the Release
workflow. Direct `main` pushes and pull requests do not start a second build;
CI remains available through manual `workflow_dispatch` when needed.

## Documentation

- [English architecture](docs/architecture.md)
- [中文架构文档](docs/architecture.zh.md)
- [Project structure audit and refactoring roadmap (中文)](docs/architecture-structure-audit.md)
- [Coding-agent instructions](AGENTS.md)
- [Specification index](docs/specs/README.md)
- [GitHub/Primer UI design specification](docs/specs/ui-design.md)
- [GitHub/Primer UI migration plan](docs/ui-github-migration-plan.md)
- [UI bug history](bug-history.md)
- [Pull request workflow specification](docs/specs/pr-review.md)
- [Terminal behavior specification](docs/specs/terminal.md)
- [Build performance and artifact policy](docs/build-performance.md)
- [Memory optimization backlog](docs/memory_optimization_backlog.md)
- [内存优化待办](docs/memory_optimization_backlog.zh.md)
- [Historical terminal investigation notes](docs/terminal_viewport_iteration_notes.md)

## License

Usher is **proprietary, non-open-source** software. See [`LICENSE.txt`](LICENSE.txt): all rights reserved; use requires explicit permission from the copyright holder.

Third-party components keep their own licenses (for example Material in `material-1.18.1/LICENSE.md`).
