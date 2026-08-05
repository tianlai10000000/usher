# Lucide sidebar icons

Files named `lucide-*.svg` are unchanged path geometry from Lucide's open-source
icon collection and are used by the Codex-style application shell, including
the sidebar, compact title-bar actions, and the persistent shortcut status
indicator.

- Source: <https://github.com/lucide-icons/lucide>
- License: ISC
- Copyright: Lucide Contributors

Slint applies a semantic foreground color and displays the 24 px source
geometry at 16 px in the application sidebar. The even size keeps Image x/y on
whole logical pixels so FemtoVG does not bilinear-blur the pre-rasterized SVG;
Lucide's round 2/24 stroke proportions remain intact at that scale.
