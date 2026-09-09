# Crawl Library Implementation Plan

> **For agentic workers:** Use the parallel-agent workflow for the independent native session lifecycle and home component tasks; integrate and verify them in the current workspace.

**Goal:** Open the application on a URL launcher and saved crawl cards, automatically store each crawl in SQLite, and open or compare saved crawls without losing previous results.

**Architecture:** Reuse the SQLite session index and one database per crawl. The launcher is a React component above the existing workbench; only session metadata reaches its cards. The desktop start command validates first, creates a fresh session for each new crawl, and resumes only explicitly selected recoverable sessions. Compare saved SQLite sessions natively, returning aggregate counts and at most 1,000 changed rows.

**Tech Stack:** Existing Rust, rusqlite, Tauri, React, TypeScript and CSS. No new dependencies.

**Spec:** User-requested flow: a centered URL, crawl type and Start action; previous crawls below as cards with opening and comparison; starting or opening reveals the existing results screen. SQLite is the desktop storage model.

## Constraints

- Preserve existing crawl data, profiles, archives, robots policies and headless memory storage.
- Keep artifacts in English and retain dark/light/system appearance and keyboard access.
- Keep full crawl records in Rust/SQLite; history loads metadata and the workbench retains paged queries.
- All fresh crawls get independent database files. Never clear an older session to start a new crawl.
- Validate targets and rendering availability before creating sessions or changing the workspace. Serialize native workspace mutations with active crawl lifecycle actions.
- Preserve legacy sessions and the old current database; distinguish missing files from empty crawls.
- Keep changes in the current authorized workspace and leave commits to the user's next commit instruction.

## Tasks

- [x] Native session lifecycle (`src-tauri/src/main.rs`, a session module if useful): migrate index fields for mode, status, crawled count and stored configuration; retain existing session IDs; register the legacy current database; automatically create/open separate SQLite stores; save progress and terminal status; restore a session's configuration when opening. `start_crawl` returns `CrawlSession`; `open_crawl_session` returns the same metadata with optional `config`. List metadata excludes configuration bodies.
- [x] Saved comparison (native session code): `compare_crawl_sessions({ baselineSessionId, currentSessionId })` returns the existing `CrawlComparisonResponse` without replacing the active store; use bounded SQL projections and a 1,000-row detail cap; reject identical IDs, missing files and active sessions. Add native tests for migration, creation isolation, reopen, failed start preservation and comparison.
- [x] Home component (`src/CrawlHome.tsx`, `src/crawl-home.css`): centered launcher, Spider/List controls, optional scope selector, list textarea, saved cards with mode/date/status/count, search, two-card selection and Compare, empty/loading/error/retry states. Use current brand and theme tokens. Expose callbacks; do not call native commands or own crawl records.
- [x] App integration (`src/App.tsx`, `src/styles.css`): SQLite desktop preferences, home as first screen, startup ready after history settles, new session start and card open transitions, Home/return navigation, safe resume selection, native saved-session comparison in the existing dialog, stale async response protection, failure feedback and busy controls. Keep Settings and quit/update dialogs available from home.
- [x] Browser regression (`scripts/smoke-ui.mjs`): fresh/empty/error home, create/open/reload, history retention, selection/compare, current crawl navigation, failed actions, persisted SQLite defaults, responsive and dark/light screenshots; continue the existing workbench tests after opening a saved crawl.
- [x] Documentation and verification (`README.md`, `ROADMAP.md`, relevant backlog notes): document automatic SQLite sessions and the new startup flow. Run native focused tests, full `make ci`, inspect both theme screenshots, and review lifecycle/data preservation before completion.

## Acceptance checks

1. Starting two crawls retains both session databases and exposes both history cards after restarting.
2. Opening a card restores its results and stored mode/configuration without launching requests; resuming requires saved frontier work.
3. Invalid URLs, unavailable rendering, missing files and failed session operations preserve the previous workspace.
4. Comparing two cards produces added/removed/changed counts, preserves the active results and displays which crawl is baseline/current.
5. Home works at 390 px and 1440 px with keyboard controls and both themes; splash completes even when history loading fails.

## Verification

- `make ci` passed on Linux: version and release checks, formatting, Clippy, 162 default-feature Rust tests, production frontend build, browser smoke, optional rendering compilation, browser discovery and two real-browser rendering tests.
- The browser regression reproduced an older graph response replacing a newly opened crawl before the fix. It now verifies isolation for graph, tree, crawl path, recovery state and database location, alongside the existing workbench checks.
- Native session tests cover independent files, migration/reopen, configuration restore, failed operations, queued-only List recovery, current-database deletion and comparison counts/detail limits. A comparison scaling test checks SQLite VM work as row counts increase.
- Inspected library screenshots at 1440 px and 390 px in dark and light themes. Native Windows/macOS runtime checks remain part of platform verification.
