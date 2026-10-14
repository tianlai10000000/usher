# Tray locale regression / 托盘语言回归

The user observed the literal `tray.quit` label and no response to language
changes after the first tray-menu fix. The application-linked regression
`tray::macos::tests::native_menu_copy_resolves_in_each_packaged_language`
failed before the build-path correction: actual `tray.quit`, expected `Quit`.
Its runtime i18n build output watched another temporary baseline checkout and
had no `tray.quit` entry. Crate-only i18n tests selected a fresh catalog and
therefore did not catch the application's cached dependency.

`crates/usher-i18n/build.rs` now reads Cargo's current `CARGO_MANIFEST_DIR` at
execution time. The reused executable test switches between two isolated
catalog roots and verifies distinct source/Chinese copy plus all watched input
paths. The original executable fails and the corrected executable passes;
captured output is in [before](build-path-before.txt) and
[after](build-path-after.txt). This is a correctness fix with no performance
claim. Other build scripts that use compile-time manifest paths were inspected
but remain outside this i18n correction.

The source-built macOS `test-fixtures` application ran with disposable state at
`/private/tmp/usher-tray-locale.k5UIrc`, offline mode and
`USHER_TRAY_LOCALE_FIXTURE=1`. It invoked the root language callback used by
Settings, then read the actual existing `NSMenu` item's `NSString` title through
English → Chinese → English. All stages verified the same menu identity and
produced `Quit` → `退出` → `Quit`. [Native records](native-locale.txt) retain the
unconditional fixture output; the application completed normal shutdown with
`MainWindow dropped, native resources released` and exit 0. This verifies native
menu localization without recreating the menu; physical Settings-picker and
tray pointer-click interaction were not automated.

Validation outcomes:

- `cargo fmt --all` and `cargo fmt --all -- --check`: pass.
- `cargo build --locked`: pass; normal application catalog rebuilt.
- `cargo check --locked`: pass.
- `cargo build --locked --features test-fixtures`: pass.
- Native fixture: three stages and completion pass; normal exit 0.
- `cargo test --lib --locked`: 833 passed, 0 failed, 2 ignored; run outside the
  sandbox to allow the existing localhost HTTP fixtures.
- `cargo test --doc --locked`: 1 passed, 0 failed, 1 ignored.
- `cargo test -p usher-i18n --locked`: 7 passed.
- `scripts/check-i18n.sh`, UI policy and architecture structure gates: pass.
- Python script suite: 104 passed, including the reused build-script fixture.
- `git diff --check`: pass.

macOS first right/Control-click, dismissal, hidden/minimized-window and Dock
interaction remain pending from UI-152. Windows was not run. Existing unrelated
working-tree changes were preserved.
