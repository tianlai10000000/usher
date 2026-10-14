# Completed GitHub/Primer UI Migration Batches / 已完成的 UI 改造批次

[Current plan / 当前计划](../ui-github-migration-plan.md) · [Stable UI contract / 当前规范](../specs/ui-design.md)

The accepted Batch 1–7 checklists below were moved from the migration plan during
the 2026-10-14 documentation cleanup. Their original acceptance, corrections,
failures, and unexecuted checks remain dated evidence rather than current
component inventories. Batch 8–10 and pending UI-113/follow-up acceptance stay
in the current plan.

以下为第一至七批的原始验收记录，保留当时的修正、失败与未执行项；旧组件名称、
依赖版本和验证基线仅用于历史对照。当前规范及未完成验收仍由当前计划维护。

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
  Do not add a page-number input. See [UI-149](../../bug-history.md).
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

