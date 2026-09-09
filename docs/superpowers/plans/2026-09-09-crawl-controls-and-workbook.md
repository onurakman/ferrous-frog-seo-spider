# Crawl Controls And Audit Workbook Implementation Plan

> **For agentic workers:** Use the parallel-agent workflow for independent engine and export tasks; integrate the frontend in the current workspace and verify the combined result.

**Goal:** Complete usable roadmap work for Spider sitemap sources, bounded HTTP response downloads and a consolidated XLSX audit workbook.

**Architecture:** Extend the existing typed crawl configuration and shared HTTP request path. Keep sitemap discovery in crawler-core and audit workbook creation in the export/native layer. The frontend exposes persisted controls and invokes the existing export command; it never receives the full crawl dataset.

**Tech Stack:** Existing Rust, reqwest, serde, quick-xml, rust_xlsxwriter, Tauri and React. Reuse current dependencies.

**Spec:** ROADMAP.md requested sitemap controls, download limits (LM-05 / CR-10 in docs/CONFIGURATION_BACKLOG.md), and consolidated XLSX workbook.

## Constraints

- Preserve current uncommitted work, automatic SQLite sessions and the compact library. Leave delete-icon positioning for the user's later review.
- Respect robots.txt, host/folder scope, request pacing and cancellation. Keep existing List-mode sources working independently from Spider sitemap choices.
- Repository artifacts use English. UI text should explain a necessary choice, with no decorative descriptions.
- Validate new configuration before replacing results. Restore defaults for old profiles and save edited settings through the existing Apply/Cancel/OK flow.
- Do not add credentials or external account requirements. Do not commit or push in this batch.

## Task 1: Engine controls

**Owner:** Engine agent. **Files:** crates/crawler-core/src/lib.rs, rendering.rs or parser code only if needed for an actual discovery path.

**Contract:** `CrawlConfig.sitemap: SitemapConfig` serializes as `sitemap: { enabled, discoverFromRobots, probeDefault, followLinked, urls }`. All booleans default to true; urls defaults to an empty array. `max_response_bytes` serializes as `maxResponseBytes`, defaults to 20 MiB and accepts positive values up to 1 GiB.

- [x] Add failing tests for old-config defaults, validation, oversized declared/chunked responses, exact-limit success, and robots/sitemap body limits.
- [x] Replace unbounded Rust HTTP body reads with one checked streaming reader. Stop on the size limit without parsing truncated HTML or manufacturing missing-title issues. Preserve known HTTP status and expose the download error. Keep pause/cancellation behavior.
- [x] Add Spider explicit sitemap URLs, robots-advertised sitemap discovery, root sitemap probing and linked sitemap controls. Deduplicate documents/URLs across sources, preserve bounded sitemap traversal, apply scope to discovered pages and retain sitemap provenance. Exact URL mode must not expand sitemap discovery. Explicit-source errors must be visible.
- [x] Verify positive and negative local-site fixtures, sitemap indexes/redirects, disabled choices, legacy List behavior and Stop; run locked crawler tests and Clippy.

## Task 2: Consolidated workbook

**Owner:** Export agent. **Files:** crates/export/src/lib.rs and src-tauri/src/main.rs export paths/tests. Coordinate any manifest changes; do not edit frontend, storage or core files.

**Contract:** `export_file` adds `kind: "auditWorkbook"`, exporting the whole current crawl. Return the existing `{ path, rowCount }` shape, where rowCount counts source crawl records once.

- [x] Add a failing workbook inspection test covering summary, URL results and populated/empty audit tabs, sheet headers, original/final URL identity and Unicode text.
- [x] Build a workbook with Summary, URLs, Broken Links, Redirects, Titles, Descriptions and Canonicals tabs using current typed queries/audit rules. Use bounded store pages rather than requesting all crawl records. Document Excel row limits and retain complete output by splitting large sheets or rejecting before writing a misleading partial report.
- [x] Keep native export work off the webview thread, preserve error feedback and avoid leaving a partial report presented as success. Retain all existing single-sheet/export commands.
- [x] Run locked export/native tests and Clippy, and return the exact worksheet/row-count semantics for frontend documentation.

## Task 3: Frontend and verification

**Owner:** Root. **Files:** src/App.tsx, scripts/smoke-ui.mjs, README.md, ROADMAP.md and docs/CONFIGURATION_BACKLOG.md.

- [x] Add persisted sitemap controls and an HTTP response limit in MiB. Merge missing nested fields in old settings/profiles. Keep List sources separate, disable irrelevant dependent inputs and validate before Apply.
- [x] Add an Audit Workbook export action for the whole crawl, preserving the existing filtered CSV/XLSX actions.
- [x] Exercise Apply/Cancel, reload, legacy defaults, dependent controls, invalid inputs and the new native export request in the existing browser smoke test. Inspect dark/light and narrow-window screenshots.
- [x] Review the integrated engine/export changes, run `make ci`, and update roadmap/backlog statuses only for verified behavior. Record any remaining browser-download or large-workbook limitations explicitly.


## Verification And Review

Completed on 2026-09-09. `make ci` passed: 191 default Rust tests, formatting, Clippy, version/release checks, production frontend build and browser smoke test, optional rendering compilation, two browser-discovery tests and two real-Chrome crawling tests. The intentionally ignored large-storage benchmark was not rerun for this batch.

Review fixes cover scoped sitemap URL budgets, preserved source query strings, decoded compressed-response limits, in-flight HTTP pause behavior, linked-sitemap Stop/resume, ordinary XML resource fallback, and ignored robots delays. Incomplete responses retain known HTTP/header decisions; shared audit eligibility also covers saved-crawl comparison. SQLite sitemap membership uses indexes for both original and final URLs, with a VM-step regression preventing full-crawl scans.

Settings preserve raw multiline edits until Apply, profile save or Start. Legacy List resume accepts whitespace/blank-line normalization while still rejecting changed source order, duplicates or sitemap targets. Browser checks cover Enter-then-type editing, persisted settings, partial/malformed snapshots, both themes at 1280px and 390px, whole-crawl export requests, pending-export locking and error recovery. Screenshots were inspected from `/tmp/ferrous-frog-crawl-controls.png.sitemaps-*.png`; they contain synthetic data.

Workbook tests inspect the actual ZIP/XML with Python 3's standard library. Native export checks cover idle/paused guards, source counts, existing-file preservation and failure cleanup. The workbook rejects sheets beyond Excel's row limit instead of truncating them; filtered single-sheet XLSX and other buffered formats remain separate roadmap work. Rust response limits exclude browser transfers; automatic robots sitemap discovery uses the seed origin's cached metadata.
