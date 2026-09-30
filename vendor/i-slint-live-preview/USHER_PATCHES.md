# Usher patches to i-slint-live-preview 1.18.1

## Spurious macOS watcher reload loop

macOS can emit repeated file-change notifications for open `.slint` files even
when their modification time has not changed. With Usher's large `MainWindow`
tree, each notification starts another live-preview compile and can freeze the
application while CPU and memory usage climb.

Before reloading, the local patch compares the last-seen `mtime` of every
non-`builtin:` watch path. It ignores a notification when no watched file has
actually changed.

Upstream comparison: Slint v1.18.1 inspected on 2026-09-30 does not contain an
equivalent guard in `live_component.rs`.

This crate is selected only by Usher's opt-in `live-preview` Cargo feature.
Default and production builds do not compile it.
