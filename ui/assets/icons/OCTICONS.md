# GitHub Primer Octicons

Files named `octicon-*.svg` are derived from GitHub's open-source Primer Octicons collection:

- Source: <https://github.com/primer/octicons>
- License: MIT
- Copyright: GitHub, Inc.

The SVG geometry remains upstream. Theme- and status-dependent colors are applied
by Slint with GitHub Primer semantic foreground tokens rather than embedded fills.
`octicon-file-directory-open-fill-16.svg` is the Primer Octicons `file-directory-open-fill-16` icon.
`octicon-copy-16.svg` is the Primer Octicons `copy-16` icon.
`octicon-fold-16.svg` is the Primer Octicons `fold-16` icon.
`octicon-kebab-horizontal-16.svg` is the Primer Octicons `kebab-horizontal-16` icon.
`octicon-sidebar-collapse-16.svg` and `octicon-sidebar-expand-16.svg` are the
matching Primer Octicons used by the PR file-tree visibility toggle.

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
