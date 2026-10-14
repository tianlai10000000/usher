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

## Batch 1 acceptance checklist / 第一批验收清单

- [x] Add application-owned `GitHubTheme` and `GitHubMetrics`.
- [x] Remove the legacy `ColorsTheme`.
- [x] Replace Material palette/style/typography token usage in active UI.
- [x] Remove direct Slint imports from the then-current `material-1.7.1-fixed/`; use
  `@material` public imports where a low-level control is still needed.
- [x] Centralize active GitHub/Primer functional colors, including PR diff and
  overlay roles.
- [x] Rewrite the stable UI design specification.
- [x] Record the nine-batch migration and acceptance gate.
- [x] Create/update the bug history.
- [x] User acceptance.

The legacy/demo surfaces identified here were removed in Batch 9. Their
Notification, Toast, and Dialog behavior now uses the real shared managers in
the UI Components playground.

这里登记的旧版/演示页面已在第九批删除；其中 Notification、Toast、Dialog 行为
现统一通过 UI Components 演练场调用真实共享 Manager。

## Batch 2 acceptance checklist / 第二批验收清单

- [x] Inventory every visual component declared under `ui/components/`.
- [x] Rename every exported and local visual component symbol to a
  `GitHub*` name and update all Slint imports/usages.
- [x] Add shared `GitHubIconButton`, `GitHubLink`, and `GitHubSpinner`
  foundations under `ui/components/`.
- [x] Route icon-only terminal, project-card, floating, and copy actions
  through `GitHubIconButton`.
- [x] Route all indeterminate spinner rendering through `GitHubSpinner`.
- [x] Preserve control dimensions while entering loading state and prevent
  duplicate activation.
- [x] Add keyboard activation and visible focus treatment to shared Button,
  IconButton, Link, Settings navigation, ToggleSwitch, and SegmentedControl
  items.
- [x] Extend `GitHubTextField` with required, disabled, loading, caption, and
  descriptive validation-message states.
- [x] Keep all then-current `material-1.7.1-fixed/` files unchanged and continue using only
  public `@material` imports.
- [x] Update related architecture/spec documents and bug history.
- [x] User acceptance, including verification of the breadcrumb alignment and
  pointer-vs-keyboard focus-ring fixes tracked as UI-011 and UI-012.
- [x] Correct the original directory-limited audit: migrate active page-local
  visual components into `ui/components/<feature>/`, rename them to
  `GitHub*`, and rename active route entries to `GitHub*Page`.

Representative symbol migrations:

| Previous | Current |
| --- | --- |
| `IconActionButton` | `GitHubIconButton` |
| `FloatingIconButton` | `GitHubFloatingIconButton` |
| `PageScrollActions` | `GitHubPageScrollActions` |
| `CustomNavDrawer` | `GitHubNavigationRail` |
| `PageView` | `GitHubPageView` |
| `StatusBar` / `TitleBar` | `GitHubStatusBar` / `GitHubTitleBar` |
| `TerminalDrawer` | `GitHubTerminalDrawer` |
| `Job` | `GitHubActionsJob` |
| `Toast` / `NotificationItem` | `GitHubToast` / `GitHubNotificationItem` |
| PR-review and Tag visual components | Corresponding `GitHub*` symbols |

File names that group a feature may remain feature-oriented; the naming rule
applies to Slint visual component symbols. Business structs, enums, globals,
routes, and callbacks remain domain-oriented.

## Batch 3 acceptance checklist / 第三批验收清单

- [x] Replace root-shell Material layout, text, icon, and scrolling wrappers
  with application-owned native/GitHub foundations.
- [x] Add `GitHubScrollView` with a stable mounted viewport and semantic
  scrollbar states.
- [x] Add reusable `GitHubPageHeader` title, description, leading visual,
  border, and action slots.
- [x] Rename the breadcrumb family to `GitHubBreadcrumb*` and preserve aligned
  icon/text baselines.
- [x] Route breadcrumb back, title-bar settings, and title-bar about actions
  through `GitHubIconButton`.
- [x] Align navigation-rail selected, hover, pressed, disabled, and
  keyboard-focus-visible states; selected routes use the semantic selected
  surface without an additional blue accent bar.
- [x] Align the title bar and status bar with semantic GitHub tokens and shared
  progress/action-menu foundations.
- [x] Keep the public `MaterialWindowAdapter` as the only Material boundary in
  the root shell; do not depend on private vendored implementation files.
- [x] Keep all vendored Slint Material source files unchanged.

Batch 3 is complete as part of the user-authorized consecutive Batch 3→4 run.
Its final acceptance is grouped with Batch 4 below.

## Batch 4 acceptance checklist / 第四批验收清单

- [x] Make `GitHubDialog` responsive, modal, backdrop-dismissible when
  configured, Escape-dismissible, and keyboard-focus-visible.
- [x] Route dialog body overflow through `GitHubScrollView`.
- [x] Add Escape dismissal and focus ownership to `GitHubActionMenu`, and add
  reusable `GitHubActionListItem` rows.
- [x] Wire active status, workflow, PR filter, PR file, and readiness menus to
  the common dismissal contract.
- [x] Replace the retired rail-style toast anatomy with a compact semantic
  Primer Banner-style `GitHubToast`.
- [x] Align notification actions and dismiss controls with shared GitHub
  buttons and semantic Banner variants.
- [x] Render one notification Banner at a time while preserving the queued
  notifications behind it.
- [x] Replace Material progress indicators with `GitHubProgressBar`.
- [x] Preserve full-window overlay ownership at the root layer.
- [x] Keep all vendored Slint Material source files unchanged.
- [x] User acceptance of Batch 3 and Batch 4, confirmed by authorization to
  continue with the next batch.

## Batch 5 acceptance checklist / 第五批验收清单

- [x] Add the reusable `GitHubBlankslate` foundation for default, loading,
  error, bordered, narrow, and spacious empty-state presentations.
- [x] Replace the Home, Search, and Detail placeholder surfaces with
  `GitHubPageHeader` plus actionable `GitHubBlankslate` compositions.
- [x] Align the Project page header, search toolbar, view switcher, reload
  action, repository grid/list cards, truncation, and responsive spacing.
- [x] Route project icon-only actions and the view switcher through the shared
  `GitHubIconButton` interaction contract.
- [x] Provide distinct loading, load-error/retry, no-project, and
  filtered-empty states without replacing the project model while scrolling.
- [x] Run user-triggered project-registry reloads off the Slint event loop,
  return state on the event loop, and prevent duplicate reload activation.
- [x] Align add/edit project forms with `GitHubDialog`, `GitHubScrollView`,
  required labels, descriptive validation state, and stable footer actions.
- [x] Preserve destructive project deletion confirmation and danger semantics.
- [x] Fix the initial Home Blankslate spacing, truncation, and unnecessary
  scrollbar regression recorded as UI-018.
- [x] Keep all vendored Slint Material source files unchanged.
- [x] User acceptance of Batch 5 after the shared Blankslate alignment fix
  recorded as UI-020, confirmed by authorization to continue with Batch 6.

## Batch 6 acceptance checklist / 第六批验收清单

- [x] Add shared, keyboard-accessible `GitHubUnderlineNavItem` and route both
  Workflow-level and Tag Add/Remove tabs through it.
- [x] Add shared, keyboard-accessible `GitHubSegmentedControl*` foundations and
  route Tag scope and semantic-version choices through them.
- [x] Remove the duplicated Workflow/Tag tab and Tag-only segmented-control
  implementations.
- [x] Replace the disabled Branch/CD placeholders with their existing
  project-scoped functional flows.
- [x] Replace Branch and CD Material text, button, switch, field, panel, and
  status presentation with application-owned GitHub components and tokens.
- [x] Make Branch fields and CD deployment/rollback controls stack at compact
  widths while keeping actions and result/status surfaces readable.
- [x] Preserve Branch validation, CD duplicate-start protection, Tag
  cancellation/state retention, and the reusable `GitHubActionsJob` presenter.
- [x] Keep PR-list/review internals reserved for Batch 7.
- [x] Keep all vendored Slint Material source files unchanged.
- [x] User acceptance of Batch 6, confirmed by authorization to continue with
  Batch 7.

## Batch 7 acceptance checklist / 第七批验收清单

- [x] Replace PR-list Material typography with native text plus
  `GitHubTheme` semantic roles.
- [x] Route PR pagination and compact overflow controls through shared
  `GitHubButton` and `GitHubIconButton` foundations.
- [x] 2026-10-13 pagination correction: use shared `GitHubPagination` and bounded
  Primer page-link projection for PR and Inbox. Confirmed page counts expose page
  links/ellipses; unknown page counts and compact widths retain directional links.
  Do not add a page-number input. See [UI-149](../bug-history.md).
- [x] Complete the correction's root compilation, page/state/locale regressions
  and native wide/narrow English/zh-CN PR-list acceptance: 779 library tests,
  five shared-algorithm tests and one production pagination UI regression passed;
  1280x900/light/en, 800x600/dark/zh-CN and 1000x760/dark/en inspected.
  Live provider APIs and the pre-existing logger doctest were not run.
- [x] Route provider-list illustrated empty/error panels through the shared
  `GitHubBlankslate` contract and retry action while preserving the established
  teamwork/connection visuals.
- [x] Add keyboard-visible focus and Enter/Space activation to PR state,
  filter, menu-option, primary review-tab, and commit-range controls.
- [x] Replace Review-workspace Material typography with application-owned
  GitHub typography while retaining only public `@material` virtual
  `ListView`/`ScrollView` behavior where large models require it.
- [x] Route Files-changed toolbar and tree-toggle icon actions through
  `GitHubIconButton`.
- [x] Make diff token and diffstat dark-mode choices depend on
  `GitHubTheme.dark-mode`, not application state.
- [x] Reuse `GitHubBlankslate` for recoverable Review loading errors.
- [x] Preserve lazy Conversation/Commits loading, virtualized file/diff rows,
  provider capability rules, review/merge/revert dialogs, and responsive
  file-tree behavior.
- [x] Keep every visual component under `ui/components/` with a `GitHub`
  component-symbol prefix; keep the route-only Review composition under
  `ui/views/`.
- [x] Keep all vendored Slint Material source files unchanged.
- [x] Resolve Batch 7 acceptance feedback for centered Blankslate prose,
  pointer-only focus rings, stable UnderlineNav widths, compact repository
  menu spacing, toolbar tooltip layering, and rendering-indicator alignment.
- [x] User acceptance of Batch 7, confirmed by authorization to continue with
  Batch 8.

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
