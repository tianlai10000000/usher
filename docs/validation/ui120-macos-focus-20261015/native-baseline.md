# macOS native baseline

Build: `cargo build --locked --features test-fixtures`, exit 0, 3m 51s.
[Build log](baseline-build.log), [binary/state manifest](baseline-fixture.json),
[native application log](baseline-native.log), [environment](native-environment.txt).
The copied binary is the pre-UI-120-fix working tree, including earlier spec-gap
fixes. It is not the repository's older HEAD binary.

The disposable bundle ran the complete application, including the production
MainWindow, Rust MessagePageBridge, ComponentFactory page and AppKit title bar.
The launcher installs offline mode and its absolute temporary state root before
exec; it clears inherited `USHER_*` fixture flags. The store contained 1,000
synthetic messages, 20 per page / 50 pages. No workstation Settings/messages or
real Provider data were used. Runtime logs confirm all settings/history paths
under `/private/tmp/usher-ui120-baseline-20261015/state/default/run-0`.

All input below was sent by the CUA native app API. No WindowEvent, direct
callback invocation, AppleScript or direct CGEvent synthesis outside CUA was used. CUA screenshots were
visually inspected after each checkpoint; the native AX tree exposes only window
chrome for this build, so AX output alone cannot certify Slint focus.

| Step | Native input and observed result |
| --- | --- |
| Initial | Window screenshot 1600 × 1200 physical pixels (800 × 600 logical, scale 2), light mode, English, docked sidebar. |
| Control | Click Home and bell, scroll to footer, click page 2: first row changes from message 1000 to 0980. Without an established keyboard focus, Return does nothing. This is a control, not a passed keyboard test. |
| Establish focus | Press Tab while Messages is visible. A blue focus ring appears around sidebar Home. Wheel scrolling preserves that ring. |
| Reproduce | With Home's keyboard focus established, single-click page 4 at the footer. Messages remains visible and the first row becomes `UI120 message 0940`; the pointer operation succeeded. |
| Failure | Press Return once. The native screenshot changes to Home with the blue Home focus ring. The router log records `Navigate to: Home` at 10:49:20.758; Inbox logs `active=false projected_rows=0`. |

This is the UI-120 failing-before case. Merely selecting Home with a pointer does
not establish its keyboard focus; the explicit Tab/focus-ring prerequisite is
necessary to distinguish an unfocused Return no-op from a real focus transfer.
Earlier pointer-only attempts were controls and did not reproduce the failure.

The close button follows existing hide-to-tray behavior. The disposable baseline
was then exited with native Command+Q; the owned PID was confirmed absent. This
record does not claim that Command+Q exercised the application's graceful drain
pipeline. The temporary bundle omits release tray-icon resources, producing the
existing text-icon fallback warning; tray visuals are outside this focus check.
