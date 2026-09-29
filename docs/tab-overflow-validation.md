# Shared Tab Overflow Validation

Date: 2026-09-29

## Reference And Behavior

- [Primer UnderlineNav](https://primer.style/product/components/underline-nav/guidelines/): move whole trailing items into More when available space runs out; preserve selection in the overflow menu.
- [Primer UnderlinePanels](https://primer.style/product/components/underline-panels/): same-page panel navigation.
- [Primer CounterLabel](https://primer.style/product/components/counter-label/): compact counts adjacent to navigation labels. The specific 99+ cap is a Usher requirement for tabs, not a universal Primer rule.

The application-owned `GitHubUnderlineNav` now accepts an item model. PR Review,
Workflow, Tag operations, and the component gallery use this same implementation.
Items retain their measured label, icon, and counter widths. The row stays 48px
high with 16px outer insets and 4px gaps. A root-owned More popup uses the shared
Select lifecycle. The selected hidden item gives More its selected underline.
Counts 0 through 99 are exact; 100 and above display 99+. Accessible labels retain
the complete value. Unknown conflict counts remain hidden.

The wider measured content layout must explicitly use `x: 0px`; otherwise Slint
centers it inside the smaller clipping viewport and clips the first tabs. The
focused test and native screenshot both exposed this during implementation.

## Verification

| Check | Result |
| --- | --- |
| `cargo fmt --all` | Passed |
| `cargo check --locked` | Passed |
| `cargo build --locked --features ui-components` | Passed, including final mock navigation binding |
| `cargo test --lib --locked` | 697 passed, 2 ignored |
| Shared navigation integration test | Passed: five items; widths 1100, 500, 280; count values 0, 10, 99, 100, 1000; pointer selection, Escape, outside dismissal, disabled entry, resize dismissal |
| Select, PR header/status bar, shared navigation integration tests | 5 passed across 4 test binaries |
| Full root Slint check, with all feature libraries enabled | Passed |
| `scripts/check-ui-policy.sh` | Passed |
| `scripts/check-i18n.sh` | Passed |
| `git diff --check` | Passed |

Native screenshots use `target/debug/usher`, compiled with `ui-components`, copied
into the temporary `/private/tmp/Usher Conflicts.app` bundle and ad-hoc signed.
Its identifier is `com.usher.conflicts-validation`. The installed Usher was not
used. The offline fixture has isolated state and makes no provider requests.

- [English PR, narrow content, More menu](designs/pr-conflicts/validation/tabs-en-narrow-more.png): 800px window with sidebar; Conflicts shows 99+ for 128 files.
- [English PR, all tabs inline](designs/pr-conflicts/validation/tabs-en-all-inline.png): same window with sidebar hidden; labels and badges remain aligned.
- [Five items, overflow menu](designs/pr-conflicts/validation/tabs-five-counts-menu.png): 10, 99, 99+ visible inline; another 99+ and a disabled fifth option in More.
- [Hidden item selected](designs/pr-conflicts/validation/tabs-more-selected.png): selecting Conflicts moves the underline to More.
- [Five items, restored width](designs/pr-conflicts/validation/tabs-five-all-inline.png): all five items reappear and the selected item is retained.
- [Chinese PR, 1200px, count 99](designs/pr-conflicts/validation/tabs-zh-wide-99.png): two-digit badge fits; diff statistics occupy remaining space.
- [Runtime switch to English](designs/pr-conflicts/validation/tabs-runtime-locale-en.png): the same process updates the tab labels and their measured widths while retaining count 99 and selection.
- [More opens Conflicts](designs/pr-conflicts/validation/tabs-menu-opens-conflicts.png): final source build opens the selected conflict workspace from the overflow menu.

## Reproduce From Source

The existing `test-fixtures` startup mechanism is reused. The `ui-components`
feature also enables it and exposes the five-tab gallery example.

```sh
env USHER_BENCHMARK_OFFLINE=1 \
  USHER_BENCHMARK_STATE_ROOT=/private/tmp/usher-tabs-manual \
  USHER_PR_CONFLICTS_FIXTURE=1 \
  USHER_PR_CONFLICTS_SCENE=review \
  USHER_PR_CONFLICTS_COUNT=128 \
  USHER_PR_CONFLICTS_LOCALE=en \
  USHER_PR_CONFLICTS_WIDTH=800 \
  cargo run --locked --features ui-components
```

Set `USHER_PR_CONFLICTS_SCENE=tabs` to open the gallery, then select Navigation.
Use `USHER_PR_CONFLICTS_COUNT=99` and `USHER_PR_CONFLICTS_LOCALE=zh-CN` for the
two-digit Chinese case. Optional `USHER_PR_CONFLICTS_SWITCH_LOCALE=en` switches
the running fixture after 60 seconds. Count input is bounded to 1000 mock files.

The isolated review fixture binds its open action to the fixture controller,
because it does not create the provider-backed active PR request used by normal
navigation. This binding exists only inside the explicit offline fixture.

Windows/Linux native checks and online provider integration were not run for this
presentation change. The known debug-logger doctest baseline was not rerun; the
required library unit tests passed. Existing unrelated working-tree edits,
including Hooks/settings work and earlier PR conflict changes, were preserved.
