# GitHub Primer Octicons

Files named `octicon-*.svg` are derived from GitHub's open-source Primer Octicons collection:

- Source: <https://github.com/primer/octicons>
- License: MIT
- Copyright: GitHub, Inc.

The SVG geometry remains upstream. Theme- and status-dependent colors are applied
by Slint with GitHub Primer semantic foreground tokens rather than embedded fills.
`octicon-file-directory-16.svg` is the Primer Octicons `file-directory-16`
outline icon used by the Workflow header “Show in Finder/Explorer” control.
`octicon-file-directory-open-fill-16.svg` is the Primer Octicons `file-directory-open-fill-16` icon.
`octicon-copy-16.svg` is the Primer Octicons `copy-16` icon.
`octicon-check-16.svg` is the matching success-state icon used with Copy by
the shared GitHub CopyToClipboardButton.
`octicon-fold-16.svg` is the Primer Octicons `fold-16` icon.
`octicon-fold-down-16.svg` is the Primer Octicons `fold-down-16` icon used by
GitHub's per-hunk “expand following lines” gutter control.
`octicon-fold-up-16.svg` is the Primer Octicons `fold-up-16` icon used by
GitHub's per-hunk “expand preceding lines” gutter control.
`octicon-unfold-16.svg` is the unchanged current Primer Octicons `unfold-16`
icon used by the selected-file “Expand all lines” action.
`octicon-kebab-horizontal-16.svg` is the Primer Octicons `kebab-horizontal-16` icon.
`octicon-sidebar-collapse-16.svg` and `octicon-sidebar-expand-16.svg` are the
matching Primer Octicons used by the PR file-tree visibility toggle.
`octicon-eye-16.svg` is the matching Primer Octicon used by the PR file
overflow menu.
`octicon-comment-discussion-16.svg`, `octicon-git-commit-16.svg`, and
`octicon-file-diff-16.svg` are the upstream 16 px icons used by the PR review
navigation tabs.
`octicon-copilot-16.svg` is the unchanged upstream 16 px GitHub Copilot icon
used by the PR-list AI review action.
`octicon-git-pull-request-16.svg` is the upstream-style 16 px pull-request
status icon used by the PR review summary badge.
`octicon-arrow-left-16.svg` is the unchanged Primer back-navigation icon used
by the PR review header and the breadcrumb IconButton.
`octicon-repo-push-16.svg` is downloaded unchanged from Primer Octicons and is
used by the Conversation timeline's `added N commits` batch record.
`octicon-stack-16.svg` and `octicon-x-16.svg` are the upstream controls used
by the PR commit-range selector.
`octicon-chevron-up-16.svg` is the upstream ChevronUp Octicon used with
ChevronDown by the Files changed in-diff find bar (previous / next match).
`octicon-triangle-down-16.svg` is the upstream TriangleDown Octicon used by
GitHub PR-list filter triggers.
`octicon-alert-16.svg` is the upstream warning mark used by GitHub-style
embedded loading errors.
`octicon-info-16.svg` is the upstream info mark used by Tag status banners.
`octicon-check-circle-16.svg` and `octicon-stop-16.svg` are the upstream
success and critical Banner marks used by the shared Notify component.
`octicon-check-circle-fill-16.svg` is the upstream filled success mark used by
GitHub Actions step rows in both their expanded and collapsed states.
`octicon-x-circle-fill-16.svg` is the matching filled failure mark so success
and failure share the same 16 px circle diameter (not a 20 px painted disc
behind a plain X).
`octicon-terminal-16.svg`, `octicon-pencil-16.svg`, `octicon-trash-16.svg`,
`octicon-sync-16.svg`, `octicon-plus-16.svg`, and `octicon-repo-16.svg` are the
unchanged upstream controls used by the Project list and its actions.
`octicon-link-external-16.svg` is the unchanged upstream LinkExternal Octicon
used by the notification Banner overflow “View all notifications” action.
`octicon-arrow-up-16.svg` is the upstream ArrowUp Octicon used by the page
Back-to-top IconButton.
`octicon-home-16.svg`, `octicon-gear-16.svg`, and
`octicon-broadcast-16.svg` are unchanged upstream icons used by application
navigation, settings actions, and network status. The native TitleBar keeps
its established `settings.svg` and `about.svg` assets as explicit product
exceptions.
`octicon-play-16.svg` is the unchanged upstream Play icon used by the UI
Components registry entry for the application-level Job Run route.
`octicon-list-unordered-16.svg` and `octicon-table-16.svg` are the unchanged
upstream controls used by the Project page's list/grid SegmentedControl.
`octicon-screen-full-16.svg`, `octicon-screen-normal-16.svg`, and
`octicon-grabber-16.svg` are the unchanged upstream controls used by the global
terminal drawer.

## GitHub diffstat blocks

Files named `github-diffstat-*.svg` reproduce GitHub's current five-cell diffstat
states in a Slint-compatible asset form. They are status blocks, not Octicons:

- added: solid success block;
- deleted: danger block with diagonal deletion stripes;
- neutral: unallocated outlined block.

The geometry follows GitHub Primer's 8 px diffstat block and 1 px inter-block
spacing. Light and dark files use the corresponding GitHub Primer color tokens.

- Design-system source: <https://github.com/primer/css>
- Color-token source: <https://github.com/primer/primitives>
- License: MIT
- Copyright: GitHub, Inc.
