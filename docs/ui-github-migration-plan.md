# GitHub/Primer UI Migration Plan / GitHub/Primer UI 改造清单

## Rules / 执行规则

- The stable contract is [`docs/specs/ui-design.md`](specs/ui-design.md).
- Every application-owned GitHub/Primer visual component anywhere under
  `ui/` must use a `GitHub`-prefixed component name. Audits are repository-wide,
  not limited to `ui/components/`.
- Components that do not implement a GitHub/Primer visual pattern keep their
  domain names; consuming a GitHub token alone does not trigger a rename.
- Visual controls, rows, cards, menus, tabs, panels, dialogs, fields, and
  feedback components belong in `ui/components/<feature>/`. Files under
  `ui/views/` may declare only a `GitHub*Page` route composition.
- Existing unprefixed GitHub-style components must be renamed together with
  every import and usage; aliases are not the final state.
- Every batch includes light/dark, minimum/wide layout, interaction-state, and
  regression checks appropriate to its scope.
- Complete one batch, hand it off, and wait for explicit user acceptance
  before starting another batch, unless the user explicitly authorizes a
  consecutive batch run. A consecutive run must stop after its last authorized
  batch for acceptance.
- Every discovered defect is recorded in [`bug-history.md`](../bug-history.md),
  including deferred defects.

## Batches / 分批清单

- [x] UI-113: move Settings language Select options out of form layout into the
  shared popup foundation; add the same Select and adjacent-field regression
  example to UI Components.
- [ ] UI-113 visual acceptance: floating placement, outside/Escape dismissal,
  keyboard selection, and unchanged following-field positions.
- [x] UI-114/UI-115 follow-up: correct Select child-slot geometry, reuse Tooltip
  placement, and observe window-wide outside clicks without intercepting controls.
- [ ] Verify the follow-up in Settings and UI Components, including window-edge
  placement and single-click actions in the page/sidebar/title/status areas.

| Batch | Scope | Status |
| --- | --- | --- |
| 1 | Establish `GitHubTheme`/`GitHubMetrics`; migrate active UI away from Material palette/style globals and duplicated functional colors; remove direct vendored Material imports; rewrite UI governance docs and create migration/bug ledgers. | Accepted / 已验收 |
| 2 | Inventory and rename all existing GitHub-style shared components to `GitHub*`; update imports/usages; align shared Button, IconButton, Link, Tooltip, Copy, Spinner, and form-control interaction states. | Accepted / 已验收 |
| 3 | Align root shell: title bar, navigation rail, status bar, page header, breadcrumbs, responsive content frame, scrollbar, and global focus behavior. | Accepted / 已验收 |
| 4 | Align overlays and feedback: `GitHubDialog`, ActionMenu/AnchoredOverlay, Toast, notification/banner, update surfaces, and root-layer input ownership. | Accepted / 已验收 |
| 5 | Align project surfaces: home/project/search/detail pages, repository cards/lists, empty/loading/error states, and project dialogs. | Accepted after UI-020 fix / 修复 UI-020 后已验收 |
| 6 | Align workflow foundation and Branch/Tag/CD views: tabs, steps, fields, Jobs/Steps, menus, status/state presentation, and responsive layouts. | Accepted / 已验收 |
| 7 | Align PR list and PR review workspace: file tree, filters, diff controls, code rows, conversations, commits, review/merge dialogs, and narrow/wide behavior. | Accepted / 已验收 |
| 8 | Align active Settings, Chat, Terminal drawer, and About surfaces; preserve terminal-specific typography and behavior contracts. | Awaiting acceptance / 待验收 |
| 9 | Consolidate component demos into the UI Components playground; remove obsolete demo routes; enforce property-matrix registration; final naming/token/state audit. | Awaiting acceptance / 待验收 |
| 10 | Final repository-wide consolidation: precise prefix boundary, Octicon system, shared-component deduplication, keyboard/accessibility, executable Material boundary, responsive regression, and documentation/ledger closeout. | Awaiting acceptance / 待验收 |

## Completed batch records / 已完成批次记录

The accepted Batch 1–7 details are preserved in the [historical record](archive/ui-github-migration.md).
第一至七批已验收；详细清单、修正与未执行检查已归档，以下保留原有章节锚点。

<a id="batch-1-acceptance-checklist--第一批验收清单"></a>

- [1: Theme, metrics, tokens and governance / Theme、Metrics、Token 与治理](archive/ui-github-migration.md#batch-1-acceptance-checklist--第一批验收清单) — Accepted / 已验收。

<a id="batch-2-acceptance-checklist--第二批验收清单"></a>

- [2: Shared component naming and states / 共享组件命名与状态](archive/ui-github-migration.md#batch-2-acceptance-checklist--第二批验收清单) — Accepted / 已验收。

<a id="batch-3-acceptance-checklist--第三批验收清单"></a>

- [3: Root shell and layout / 根窗口 Shell 与布局](archive/ui-github-migration.md#batch-3-acceptance-checklist--第三批验收清单) — Accepted / 已验收。

<a id="batch-4-acceptance-checklist--第四批验收清单"></a>

- [4: Overlays and feedback / 浮层与反馈](archive/ui-github-migration.md#batch-4-acceptance-checklist--第四批验收清单) — Accepted / 已验收。

<a id="batch-5-acceptance-checklist--第五批验收清单"></a>

- [5: Project surfaces, including UI-020 / Project 页面及 UI-020](archive/ui-github-migration.md#batch-5-acceptance-checklist--第五批验收清单) — Accepted / 已验收。

<a id="batch-6-acceptance-checklist--第六批验收清单"></a>

- [6: Workflow foundations / Workflow 基础](archive/ui-github-migration.md#batch-6-acceptance-checklist--第六批验收清单) — Accepted / 已验收。

<a id="batch-7-acceptance-checklist--第七批验收清单"></a>

- [7: PR list and review, including UI-149 / PR 列表、Review 与 UI-149](archive/ui-github-migration.md#batch-7-acceptance-checklist--第七批验收清单) — Accepted / 已验收。

## Batch 8 acceptance checklist / 第八批验收清单

- [x] Remove Material text, card, layout, spinner, scroll, and button visuals
  from the active Chat route.
- [x] Route Chat history through `GitHubScrollView`, loading through
  `GitHubSpinner`, copy through `GitHubCopyButton`, and send through
  `GitHubIconButton`.
- [x] Replace fixed-width/pill Chat composition with a responsive bordered
  editor, compact history action, six-pixel message surfaces, and
  keyboard-accessible suggestion buttons.
- [x] Preserve Chat's bounded Rust virtual window, history paging callback,
  scroll sentinel, Markdown model, inline error state, and route lifecycle.
- [x] Route Terminal header icon actions and Select mode through shared GitHub
  controls while preserving monospace rendering, PTY viewport, fullscreen,
  selection, scrollback, and project-session behavior.
- [x] Add a keyboard-accessible compact Settings section menu and retain
  existing Settings persistence/async boundaries.
- [x] Keep About on application-owned GitHub typography, detail rows, semantic
  borders, and responsive content width.
- [x] Confirm dormant Login and disabled Tools/List have no navigation or
  product entry point; leave all three legacy routes for the repository-wide
  Batch 9 cleanup.
- [x] Keep every visual component under `ui/components/` with a `GitHub`
  component-symbol prefix and keep all vendored Slint Material source files
  unchanged.
- [x] Resolve Batch 8 acceptance feedback for the send tooltip layer,
  restored composer resizing/handle, and compact Terminal Select typography.
- [ ] User acceptance of Batch 8.

## Batch 9 acceptance checklist / 第九批验收清单

- [x] Add `Route.UiComponents` to the primary navigation rail as the permanent
  interactive GitHub/Primer component playground.
- [x] Add property-comparison matrices for shared buttons, icon actions,
  links/copy, fields, Toggle, SegmentedControl, UnderlineNav, SettingsNav,
  ActionList, Toast, Notification, Dialog, Spinner, ProgressBar, Blankslate,
  semantic tokens, and status presentation.
- [x] Exercise the real root-layer Toast, Notification, and Dialog managers
  from the playground instead of maintaining duplicate demo implementations.
- [x] Register model-dependent Project, Workflow, Chat, Settings, shell, and
  PR Review component families with links to their live routes.
- [x] Remove the obsolete Login, Tools, List, Detail, Notification, Toast, and
  Dialog demo routes, root branches, breadcrumbs, and view files.
- [x] Remove the Dialog route-only visibility state and route manager actions
  directly through `DialogManager`.
- [x] Make playground registration plus applicable public-property comparison
  a mandatory rule for every future component addition or material change.
- [x] Keep all visual components in `ui/components/`, all GitHub/Primer visual
  symbols `GitHub`-prefixed, and all vendored Material sources unchanged.
- [ ] User acceptance of Batch 9.

## Batch 10 acceptance checklist / 第十批验收清单

- [x] Confirm that every component implementing a GitHub/Primer visual pattern
  uses a `GitHub*` symbol, while domain/non-GitHub components retain their
  names; keep every visual component under `ui/components/`.
- [x] Consolidate application command/status icons on official Octicons and
  remove unused legacy assets; preserve the TitleBar `settings.svg` and
  `about.svg` exceptions unchanged. This records the original batch acceptance;
  the unused TitleBar and those exceptions were removed in the 2026-10-14 cleanup.
- [x] Remove pass-through FAB, terminal-action, loading-button, pagination,
  and spinner duplicates; reuse `GitHubIconButton`, `GitHubButton`, and
  `GitHubSpinner` directly.
- [x] Add accessibility roles, enabled/checked/value labels, Enter/Space
  activation, Escape dismissal, and keyboard-visible focus to shared controls
  and the Jobs disclosure rows.
- [x] Limit Material to the public root adapter and PR Review virtualization,
  keep the then-current `material-1.7.1-fixed/` tree unchanged, and enforce both Material and icon
  boundaries with `scripts/check-ui-policy.sh`.
- [x] Verify the live UI Components route at `640x600`, `836x755`, and
  `1280x900`; verify Tab focus and confirm no horizontal overflow, incoherent
  overlap, or restored navigation-rail blue selection bar.
- [x] Update the stable UI specification, architecture references, Octicon
  registry, migration checklist, and bug ledger.
- [ ] User acceptance of Batch 10.
