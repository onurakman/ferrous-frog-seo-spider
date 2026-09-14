# Streaming Schema-1 Archive Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking. The parent coordinates the existing workers; do not spawn additional agents or commit before parent review.

**Goal:** Import a complete schema-1 crawl archive into a saved SQLite session without retaining the input bytes or complete record, evidence, capture, or frontier arrays in application memory.

**Architecture:** A strict Serde seed writes typed items into an isolated SQLite database, reusing the comparison record staging implementation. Bounded storage append helpers preserve captured evidence identities and metadata. Only a completely decoded, validated and closed database is published and activated; the active source remains untouched on failure.

**Tech Stack:** Existing Rust, serde/serde_json, rusqlite, tempfile, Tokio and Tauri dependencies. No new packages.

**Spec:** `AGENTS.md` engine ownership/responsiveness/testing requirements; `ROADMAP.md` remaining archive import memory ceiling; schema-1 `CrawlArchive` and `import_crawl_archive` in `src-tauri/src/main.rs`; parent authorization to extend the proven archive comparison staging checkpoint (`7af74a1`).

## Global Constraints

- Keep the engine independent from the UI. UI code must not own the full crawl dataset.
- Push filtering, sorting, and pagination into storage/engine commands.
- Do not store raw HTML unless a feature explicitly requires it. This importer restores only the archive's existing optional captures.
- All repository artifacts must be written in English.
- Preserve public native command/request/result DTOs, saved-session behavior and existing crawler insert/replace APIs.
- Keep comparison's records-only reader permissive about unused sections; full import validates all known sections.
- Preserve `.idea`, `docs/tmp`, unrelated pagination/diagnostic changes and user datasets. Coordinate Cargo and isolated measurements with the parent.

## Contract and call sites

`import_crawl_archive` currently holds the crawl lifecycle lock, reads the entire file, decodes `CrawlArchive`, then calls `import_archive_into_session`. The latter groups images/references and capture ownership in memory, creates an importing session, and holds active-store locks during insertion. `src/App.tsx` invokes this command through its existing workspace action. `session_tests.rs` and `export_tests.rs` call the helper directly. Only those helper tests need adapting; no frontend API changes are required.

The replacement accepts required `schemaVersion`, `exportedAtMs`, `records`, `linkEdges`, and `imageAssets`; missing `pageReferences`/`pageCaptures` means empty. Missing or null `frontierState` means absent; an object requires `queued`, `seen`, and `crawled`. Known duplicate fields, malformed known sections, unsupported schema versions and trailing JSON fail. Unknown fields remain ignored. Field order is arbitrary.

Records retain sparse/shuffled IDs, normalized legacy empty storage keys, List occurrence fields and all stored payload columns. Unique IDs and normalized storage keys are required, matching the comparison integrity checks; ambiguous duplicate IDs/keys now fail instead of silently renumbering/upserting. Evidence IDs, source/discovery positions, captured status/depth and sibling rows survive. Captured `inlink_count` is not reconstructed from edges: schema 1 does not serialize its separate bookkeeping table. Image `size_bytes`/`oversized` and record first-inlink fields retain existing query-derived semantics, rather than adding columns for inconsistent hand-authored derived values.

Array encounter order supplies the first-record seed, first-queued fallback, and frontier queue positions. Explicit record IDs/List positions and evidence IDs/positions retain the existing query ordering; no global archive-ordinal query model is introduced. Seen URLs retain the existing SQLite set semantics. Captures retain all optional values, header order, bodies and truncation flags; duplicate occurrence captures and captures without a matching normalized record key fail.

## Ownership and interfaces

Parent owns `crates/storage/src/archive_restore.rs`, its focused tests, and the narrow module/error integration in `crates/storage/src/lib.rs`. It must avoid the other worker's reference-diagnostic regions.

```rust
impl SqliteStore {
    pub fn try_append_archive_link_edges(&self, rows: &[LinkEdge]) -> Result<(), StorageError>;
    pub fn try_append_archive_image_assets(&self, rows: &[ImageAsset]) -> Result<(), StorageError>;
    pub fn try_append_archive_page_references(&self, rows: &[PageReference]) -> Result<(), StorageError>;
    pub fn try_append_archive_frontier_queue(&self, start_position: usize, rows: &[CrawlFrontierItem]) -> Result<(), StorageError>;
    pub fn try_append_archive_seen(&self, rows: &[String]) -> Result<(), StorageError>;
    pub fn try_set_archive_crawled(&self, crawled: usize) -> Result<(), StorageError>;
}
```

Every call is a transaction, including range validation and duplicate rejection. These methods append to a private empty import destination, never delete siblings, enrich evidence from records, or increment record inlink counts. Seen insertion uses `INSERT OR IGNORE` as before. IDs/discovery/depth/queue positions must fit SQLite integers.

Storage agent owns new `src-tauri/src/archive_staging.rs` (the actually shared record identity stager), new `src-tauri/src/archive_import.rs` (strict decoder, private staged import, focused tests), scoped `comparison_sources.rs` extraction, main command/helper/module wiring, and a narrow prepared-session publication helper in `sessions.rs`. Existing session/export fixtures remain in their current files. Native work depends only on the six signatures above.

## Task 1: Transactional evidence and frontier append primitives

- [x] Write focused storage tests with two sibling images/references in separate calls, deliberately sparse/reversed IDs, link metadata disagreeing with matching records, and a nonzero captured record inlink count. Assert persisted IDs/positions/metadata and the unchanged count. Append a batch containing one valid and one duplicate/out-of-range row; verify neither is added.
- [x] Prove queue insertion at positions 0 and 2 preserves supplied order, duplicate positions roll back the entire call, seen strings deduplicate without normalization, and crawled zero differs from absent frontier metadata.
- [x] Run the focused tests red, implement the six direct prepared-statement transactions, and rerun them green. Do not route through the crawler's enriching or replacing APIs.
- [x] Parent reports the compiling storage contract and focused results before native integration verification.

## Task 2: Shared record staging and strict bounded decoder

- [x] Extract `RecordStage` from `comparison_sources.rs` without changing its SQL identity map or comparison seed behavior. Expose only `new`, `insert`, `finish`, and read access to its store/connection for the actual full-import consumer. Preserve existing comparison tests.
- [x] Add a minimal full-import reader acceptance test:

```rust
let directory = tempfile::tempdir().unwrap();
let input = br#"{"schemaVersion":1,"exportedAtMs":0,"records":[],"linkEdges":[],"imageAssets":[]}"#;
let staged = stage_archive_reader(&input[..], directory.path()).unwrap();
assert_eq!(staged.records, 0);
assert_eq!(staged.frontier_items, 0);
assert!(staged.start_url.is_empty());
```

`stage_archive_reader(reader: impl Read, sessions_directory: &Path) -> Result<StagedArchive, String>` creates its own TempDir below the supplied directory. `StagedArchive` retains the TempDir, closed database path, first-record/queue seed, crawl mode and fixed scalar counts. It contains no dataset arrays or live store handles after successful finalization.

- [x] Add a seven-byte-chunk reader test that queries the private database after the first typed item but before EOF, proving actual incremental insertion. Add large rows after that first item so the assertion cannot pass through whole-file decoding.
- [x] Implement a map visitor with one presence flag per known field; use typed sequence seeds with a fixed maximum 256-row batch, and a separate nested frontier visitor. Flush batches before requesting more input. A single record/capture/string remains the unavoidable item-size bound. Use `Deserializer::end()` after the seed succeeds.
- [x] Append images/references and frontier batches directly. Insert captures using existing validation/replace after SQL duplicate-key detection, then verify ownership with an indexed SQL anti-join at EOF. This permits captures before records.
- [x] Defer only links into `archive_pending_edges(ordinal INTEGER PRIMARY KEY, payload TEXT NOT NULL)` because later record upserts rewrite link status/depth. Serialize one typed edge at a time. Once `RecordStage::finish` restores record identities, replay bounded ordinal-keyset pages through `try_append_archive_link_edges`. Drop each SELECT statement before appending the page, then drop the temporary table.
- [x] Cover reordered fields, metadata last, shuffled sparse/zero/maximum IDs, repeated final URLs with distinct List keys, nondefault record integration/pagination fields, relation siblings spanning batches, capture optional empty values and headers, queue order and duplicate seen values. Compare public queries and raw stored metadata where query fields are intentionally derived.
- [x] Cover unsupported/missing/duplicate metadata, malformed required/optional sections, missing frontier members, duplicate captures, missing capture owners, duplicate/out-of-range identities, malformed late JSON, trailing second values, late reader I/O failure and append failure. After every failure, assert the staging parent has no child import directory.
- [x] Run focused `archive_import` and `comparison_sources` tests. The records-only comparison must still ignore malformed unused section types.

## Task 3: Completed-session publication and activation

- [x] Add session tests proving a late invalid archive retains the previous active session/store, saved-session list and previous database contents. Inject publication/index failure and assert no new session file or row survives.
- [x] Keep the existing crawl lifecycle guard across worker staging and activation. Open `File`/`BufReader` and decode inside `spawn_blocking`; do not hold the active store or current-session mutex while decoding. Ignore legacy request `storageMode` as before.
- [x] Finalize the staged database only after all validation: close its stores/connections, checkpoint/truncate WAL with the remaining connection, close that connection, then return `StagedArchive`. Any busy/checkpoint error is a failed import.
- [x] Add `sessions::publish_imported_session(conn, dir, staged_path, start_url, mode, records) -> Result<(CrawlSession, SqliteStore), String>`. Allocate the existing internal random session ID, publish the closed database with a same-filesystem no-clobber hard link, open it at the final path, and insert one `Imported crawl` / `imported` index row using the explicit seed/mode/count. On open, SQL, or session-read failure, drop the store and remove the new row/database/sidecars. Existing report publication already uses the same no-clobber primitive.
- [x] Acquire active-store/current-session guards before final publication, then replace both only after the helper succeeds. Keep this final operation in blocking work if opening/index operations would block the async executor. Do not derive the seed from `ORDER BY id`; it comes from the first encountered record, or first queued item for an empty record array. Mode is List if any record or queued item has a List position.
- [x] Replace production `import_archive_into_session(CrawlArchive)` usage. Adapt existing test callers to serialize their already-small fixtures into the reader path; keep the `CrawlArchive` DTO for archive compatibility tests and unrelated legacy comparison code. Do not serialize a whole production archive as an adapter.
- [x] Run existing native session/archive/capture/pagination roundtrip tests plus active-crawl rejection and saved reopen-without-network tests. Confirm captures/frontier and sparse record/evidence IDs survive reopen.

## Task 4: Measurement, documentation and review

- [x] Add an ignored optimized native workload generating the archive item by item, including records, edges, images, references, captures and queued/seen URLs. Keep input generation outside the decode/publication timer. Validate scalar SQL counts, last identities and selected payloads without hydrating all output rows.
- [x] Run a small fixture first. Then coordinate a quiet process window for a large archive with 50,000 records and at least 1,000,001 edges. Record exact input bytes, all section counts, elapsed decode/stage/publication time, peak process RSS, filesystem and build mode. Include a matched full-deserialization baseline only if explicitly labeled as sharing the new SQL staging and identity checks; do not call it the previous importer.
- [x] State the remaining ceiling precisely: bounded row batches and one potentially large typed item, optional capture validation limits unchanged, SQLite/file I/O and on-disk staging retained. Legacy immediate archive comparison and archive export frontier hydration remain separate backlog items.
- [x] Update only the relevant `README.md`, `ROADMAP.md` and `docs/BENCHMARKS.md` paragraphs after results exist. Run focused native/storage tests, formatting and Clippy with the parent's Cargo coordination; parent handles full CI and the commit checkpoint.

## Execution Evidence

- Storage prerequisite tests: both transactional restore tests passed; parent owns storage implementation and review.
- Native focused archive regressions: 22 passed, two opt-in workloads ignored. Coverage includes existing Unicode/seed/frontier and retained-capture roundtrips, streaming before EOF, shuffled identities, arbitrary section order, late I/O/JSON/storage failures and publication rollback.
- Optimized all-collection workload: 50,000 records, 1,000,001 links, 50,000 images/references/captures/queued items and 100,000 seen keys; exact counts and final evidence verified. Input 496,945,043 bytes; import/publication 45,124.384 ms; peak RSS 17,236 KiB. See `docs/BENCHMARKS.md` for the measurement scope and reproducible command.
- Final checks passed: `make test-ui` on the frozen frontend, then `make ci -o test-ui` on the integrated source. This covers 583 default workspace tests, three rendering unit tests, seven serialized real-Chrome fixtures, Clippy, formatting, production build, version/release guards and complete offline report/comparison checks. The unchanged full UI smoke was reused instead of repeated. Independent storage/native reviews found no actionable issue. No further roadmap work follows this checkpoint; the user requested a pause after committing the current work.
