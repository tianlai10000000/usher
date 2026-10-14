# P7 #85 Commit picker/catalog/paint policy — rejected this run

The candidate was assembled only in an independent adopted79 copy:17 modified
paths, five new paths,785 unchanged original paths. It adds seven complete
named-parameter catalog messages, a feature-owned Commit caption, identity/date
projection and a five-property paint-expression policy scanner. No candidate
code or translated state was applied to the canonical tree.

Actual results on 2026-10-04:

- Source tools:25 expression/shell tests and five native-reader tests passed.
- Actual old/new shell AB/BA/AB:132 positive/negative cells passed, including
  expected old-gate omissions and strict new grammar rejection. The recording
  Cargo stub validates dispatch only; it is not real catalog or runtime evidence.
- Full-root format and locked/offline check passed. First library run:734
  passed, one failed, two existing ignores. The new real-model test selected a
  bundled locale before creating its UI component, so Slint reported no bundled
  translations. Only test initialization was corrected by creating the same
  real MainWindow first; all existing assertions and locale guard remained.
  The complete retry passed735, failed0, ignored2. Original failure and source
  correction hashes are retained. These are model/catalog checks, not pixel QA.
- The actual `usher-i18n` package test command passed seven tests. Catalog
  parsing/build validation also ran during the full-root check.
- Actual full UI source inventory: baseline has eight diagnostics; candidate
  resolves the two Commit expressions but still has six. The actual new
  `scripts/check-i18n.sh` returned1 before reaching its catalog command.

The six remaining diagnostics have two different causes. Notification summary
contains three real untranslated literals: ` shown · `, ` more`, and
` notifications`. They require complete catalog messages with dynamic named
counts and immediate mounted locale refresh, not an allowlist. Project's FAT,
UAT and PROD engineering labels appear in a ternary whose final branch is a
compound summary; the scanner's existing narrow scoped-prefix rule only handles
a top-level addition and does not cover that exact expression. A future fix must
recognize only the inspected path/property/fields, still rejecting extra prose.
The current candidate was not widened or silently exempted to make this gate pass.

Decision: reject. The actual project policy gate is red, and controlled Release
native Commit/locale/dialog/pixel/Home acceptance was not executed. No runtime
or RSS benefit is claimed. The complete candidate, tool A/B, real catalog,
first failure, corrected test and actual compiled source manifests remain in
`memory-benchmark-results/p7-85-isolated-20261004/` plus the immutable source
preparation records. No failed source becomes #86's accepted baseline.
