# Repository cleanup / 仓库清理

The user approved the audited cleanup and explicitly retained three root history
documents. `claude-proposal.md` was renamed to `optimizing-proposal.md` without
changing its bytes; all five incoming textual references were updated.
`code-quality-proposal.md` changed only those old-name references;
`memory-rss-diagnostics.md` remains present and unchanged. Their identities are
recorded in [preserved-documents.json](preserved-documents.json). The active local
AI Review configuration selects `ai-review-test-rule.md`, so that file was kept.

Deleted unreachable repository files:

- `index.html`: unsupported WASM template with a missing module target.
- `src/codereview/agent-auto-review.rs`: empty, undeclared module file.
- `src/chat/turn.rs`: undeclared module with no external callers.
- `ui/components/title_bar.slint`: no imports or mounted instances.
- `ui/assets/icons/settings.svg`, `about.svg`, `octicon-question-16.svg`,
  `octicon-home-16.svg`, `octicon-grabber-16.svg`, `provider-github.svg`,
  `provider-gitlab.svg`, `provider-gitee.svg`: no active consumer after removal
  of the unused title bar. Asset documentation and the old policy exception were
  updated; current ribbons, Lucide sidebar and Terminal controls remain intact.

These twelve files total 13,914 bytes. The unused `profile.dist` was removed;
`dist-workspace.toml` is retained in the old-release archive. The existing custom
release workflow and cargo-bundle paths were inspected and preserved.

Three historical conflict design PNGs moved byte-for-byte to
`docs/archive/pr-conflicts-20261014/`, with their README links repaired. Final
designs, all dated performance/validation records, canonical docs, source
fixtures and third-party trees were retained. Unrelated working-tree changes,
including the existing ownership extraction and tray fixes, were preserved.

Archive creation hashes every source file and records metadata; verification
reads every archive member and compares its content hash. All 114,386 benchmark
entries and 15 old-release entries verified before original paths were removed.
The original trees were then checked again against their recorded names, sizes
and modification times. The local archives occupy about 4.93 GiB including
manifests, and are ignored by Git. Their locations and restoration instructions
are in [local archives](../../local-archives.md); identifiers and original
allocated sizes are in [archive-summary.json](archive-summary.json).

Finder metadata, Python caches and the two completed offline tray-test temporary
directories were removed; see [removed-generated.json](removed-generated.json).
No app configuration, credentials, project data or active AI Review rule was
deleted. Cache cleanup ran last, after checking no Cargo/Rust/Usher process was
active. `cargo clean --locked --target-dir .../target` removed 26,250 files;
the standalone DPI clean removed 8,914 files. The measured allocated target sizes
were 46,659,100 and 3,279,680 KiB, respectively. Raw clean output and directory
measurements are retained alongside this report. Net reclaimed directory
allocation is approximately 64 GiB, including archive compression and temporary
QA files; it is not a runtime-performance claim. The next build regenerates
programs and caches and will take longer.

Validation completed before final Cargo/Python cache deletion:

- `cargo fmt --all`: pass.
- `cargo check --locked`: pass.
- `cargo test --lib --locked`: 833 passed, 0 failed, 2 ignored; localhost fixtures
  ran outside the sandbox.
- `cargo test --doc --locked`: 1 passed, 0 failed, 1 ignored.
- `scripts/ui-check.sh`: pass, full root Slint compile.
- UI policy, architecture structure and i18n gates: pass.
- Python script suite: 104 passed; its quality subset also passed 65 tests.
- `git diff --check`: pass after cleanup.
- Preserved-document content comparisons and moved-PNG hashes: pass.
- Archive hashes, per-member contents and original-tree pre-removal checks: pass.

The icon gate comparison used one legitimate Octicon control and two negative
fixtures. Control remained exit 0, ordinary legacy settings remained exit 1,
and the former title-bar exception changed exit 0 → 1. Candidate adopted; this
tightens the gate and removes no exemption from failures. Raw outputs are in
[baseline](ui-policy-baseline-results.json) and
[candidate](ui-policy-candidate-results.json). Original/candidate scripts are
retained in the ignored local archive directory for reproducibility.

Native layout interaction, Windows execution and release packaging were not
rerun: no mounted component or maintained packaging path changed. Cargo tests
were not rerun after deleting caches, to avoid recreating the requested cleanup.
