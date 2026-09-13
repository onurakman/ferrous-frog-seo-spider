# Roadmap Continuation Implementation Plan

**Goal:** Complete the next practical storage, Settings navigation, HTML export and reference retention gaps.

**Architecture:** Reuse the existing storage trait, Settings drafts and atomic export writer. Persist only changed frontier entries during ordinary completions; retain full snapshots for startup and Stop. Keep the existing string HTML API for callers that need it. Store reference evidence separately from audit metadata, query bounded pages and carry it through the existing archive workflow.

**Constraints:** Preserve robots defaults, List occurrence identities, resumability, bounded UI queries, existing saved preferences, locked dependencies and English repository artifacts.

- [x] Add a shared frontier update operation with in-place Memory changes and transactional SQLite deletes/appends/sitemap flags. Prove parity, rollback, reopening, bounded SQL changes and unchanged audit revisions in storage tests.
- [x] Wire crawler completions to frontier updates, preserving discovery, cancellation, errors and active siblings. Verify the existing abort/reopen test and local-site fixture; compare release measurements with the saved pre-change executable.
- [x] Split Settings controls into Limits, Speed and robots.txt; add search-to-control focus and contextual help with existing browser smoke coverage.
- [x] Stream HTML rendering through the existing writer and atomic native export path. Verify output, escaping, I/O errors and preservation of existing files.
- [x] Run relevant workspace, lint, browser and rendering checks; review changes and update README, ROADMAP and benchmark evidence. All 457 Rust tests, browser smoke, rendering fixtures, lint and production build passed before the reference-retention follow-up.
- [x] Add independent default-on reference evidence retention with per-source occurrence identity, paged Memory/SQLite queries, a bounded detail inspector and backward-compatible archive persistence. Preserve metadata used by audits; effective Spider discovery forces retention, while disabled List/Exact URL discovery does not override Store.
- [x] Verify reference retention, settings dependencies, session/archive isolation and bounded UI queries; repeat the combined checks and update feature scope. Final verification passed: all 463 Rust tests including seven serialized Chrome fixtures, Clippy, formatting, version/release guards, production build, graph/schedule checks and the complete browser smoke. Independent review found no actionable correctness issues.

Broader performance work, detailed comparison, audit coverage and platform/account-dependent work remain separate follow-up items after these deliverables.
