# Usher

[简体中文](README.zh.md) · [Architecture](docs/architecture.md) · [Specifications](docs/specs/README.md)

Usher is an alpha-stage desktop workspace for managing local Git projects and repetitive delivery workflows. It combines project metadata, pull-request summaries, guided Branch/Tag/PR/CD flows, an embedded terminal, chat assistance, update management, and desktop notifications in a Slint application.

## Highlights

- Manage local repositories, aliases, tags, logos, and remote repository URLs.
- Inspect open PR/MR counts for `fat`, `uat`, and `prod` across GitHub, GitLab, and Gitee-compatible servers.
- Run guided Branch, Tag, PR, and CD workflows with per-project state.
- Open a project-aware terminal drawer without leaving the current page.
- Configure an independent OpenAI-compatible AI Agent provider (such as DeepSeek); the existing RAG chat view remains separate.
- Receive update, toast, notification, tray, and network-status feedback.
- Switch between light, dark, and system theme modes.
- Route all application HTTP traffic through a shared reqwest-based interception pipeline.

> Usher is under active development; several secondary routes are development surfaces rather than finished product features.

## Supported targets

The maintained desktop targets are:

- macOS, with native title-bar, tray, bundling, and DMG support.
- Windows, with release packaging support.

Linux may build with a suitable Slint backend, but it is not currently covered by the release workflow. WebAssembly and Android instructions from the original template have been removed because the current desktop dependencies and platform integrations are not maintained for those targets.

## Requirements

- A current stable Rust toolchain with the 2024 edition available.
- Git in `PATH` for workflow and repository operations.
- Platform build dependencies required by Slint/winit.
- Optional: `cargo-bundle` and `create-dmg` for macOS packaging.

The first build may download a prebuilt Skia package because the project enables both FemtoVG and Skia renderers.

## Quick start

```sh
git clone https://github.com/tianlai10000000/usher.git
cd usher
cargo run
```

Useful development commands:

```sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
```

The full `cargo test --locked` command currently exposes a pre-existing failing doctest in `src/logger/debug_logger.rs`; the library unit tests pass.

To inspect the optional Skia software path:

```sh
SLINT_BACKEND=winit-skia-software cargo run
```

Debug builds write five-second process-memory samples and lifecycle checkpoints to the normal Usher log. To enable the same diagnostics in a release build:

```sh
USHER_MEMORY_DIAGNOSTICS=1 ./usher
```

Filter for `[Memory][Diag]`, `[Memory][UI]`, and `[Workflow][PR Review][Memory]` to correlate RSS/physical-footprint changes with window size and PR model/cache ownership.

## Configuration and local data

Usher uses `directories::ProjectDirs::from("com", "usher", "app")` to select platform-native locations.

| Data | Relative location |
| --- | --- |
| Settings, Git platform tokens/API roots, and AI Agent credentials | config directory: `settings.toml` |
| Project registry | local data directory: `project/projects.json` |
| Chat history | local data directory: `im/chat_history.jsonl` |
| Logs | local data directory: `logs/<date>/usher.log` |
| Cached remote project logos | temporary directory: `usher-logo-cache/` |

GitHub, GitLab, and Gitee tokens can be managed from Settings. The **Code Hosting API Configuration** section stores a per-provider enterprise API URL and enterprise name under `[git.platform.<provider>]`; enter either the service host or the complete REST API root (for example, `https://git.example.com` or `https://git.example.com/api/v4`). The provider version path is added when omitted. These values are local-only and used only when the configured API host matches a repository remote. The **AI Agent** section accepts an OpenAI-compatible Base URL, API Key, and Model Name; it provides an extensible provider selector, editable model name, and a non-billable connection test. Settings are saved under `[ai.agent]`. Do not commit generated settings, logs, histories, or credentials. See [AI Agent configuration](docs/specs/ai-agent.md).

## Architecture at a glance

```text
src/main.rs
  -> src/lib.rs                application composition and Slint bindings
     -> src/app_runtime.rs     shared Tokio runtime
     -> src/http/mod.rs        shared HTTP clients and interceptors
     -> src/project/           project registry, logos, PR summaries
     -> src/workflow/          Branch / Tag / PR / CD state and execution
     -> src/terminal/          PTY session and terminal rendering
     -> src/chat/              chat transport, history, Markdown, UI binding
     -> src/update/            update checks, download, install
     -> src/{toast,notification,dialog}.rs
ui/main.slint
  -> ui/views/                 page-level views
  -> ui/components/            reusable application components
material-1.7.1-fixed/            vendored Slint v1.17.1 Material library
```

Read [docs/architecture.md](docs/architecture.md) for ownership, lifecycle, data flows, and extension points. Coding agents must also follow [AGENTS.md](AGENTS.md).

## HTTP interception

All application-owned HTTP calls use `src/http/mod.rs`, which wraps reqwest instead of replacing it. The module provides:

- one shared asynchronous client and one shared blocking client;
- request, response, and error interceptor hooks;
- normalized `HttpError` categories and status handling;
- per-request error feedback: silent, toast, notification, or both;
- retained access to reqwest request builders, JSON decoding, and streaming responses.

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

## Documentation

- [English architecture](docs/architecture.md)
- [中文架构文档](docs/architecture.zh.md)
- [Coding-agent instructions](AGENTS.md)
- [Specification index](docs/specs/README.md)
- [Pull request workflow specification](docs/specs/pr-review.md)
- [Terminal behavior specification](docs/specs/terminal.md)
- [Historical terminal investigation notes](docs/terminal_viewport_iteration_notes.md)

## License

No root project license file is currently present. The vendored Material component library retains its own license in `material-1.7.1-fixed/LICENSE.md`.
