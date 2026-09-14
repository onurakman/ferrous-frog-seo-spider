# ROADMAP.md

Ferrous Frog development roadmap and implementation tracker.

## Legend

- [x] Done and verified in the current codebase.
- [ ] (Partial) A usable slice exists, but listed gaps remain.
- [ ] Not started.

## Current Snapshot

- [x] Cargo workspace, Tauri 2 shell, React/Vite frontend, and core engine crates are in place.
- [x] Phase 1 crawler MVP is usable for Spider mode and basic List mode.
- [ ] (Partial) Phase 2 scale/reporting work is underway: SQLite, sessions, resumable frontier state, exports, overview, graph, and many audits exist, but full audit coverage, fully streaming exports, and large-scale benchmarks are not complete.
- [x] Phase 3 advanced extraction baseline is implemented: custom extraction, raw/rendered HTML custom search, near duplicates, sitemap export, robots tooling, resource toggles, rendered/raw DOM diff, and an optional Chrome CDP rendering backend exist.
- [x] Phase 4 visualization and external data work: graph storage, graph UI, directory tree, URL segments, path helpers, archive-based crawl comparison, integration provider contracts, Google Search Console metric merge, and manual PageSpeed measurements saved with individual crawl rows exist. GA4, bulk PageSpeed, Chrome UX Report field data and a generic backlink endpoint are integrated; vendor-specific backlink adapters remain.
- [ ] (Partial) Phase 5 automation and AI assist: CLI, automation, scheduling, authentication flows and per-URL AI assistance exist; saved sitewide/follow-up AI audit reports with complete evidence access are available; extreme-scale hardening remains.

## Immediate Next Work

- [x] Restore access to all grid/report rows with bounded server-side pages and stale-response protection.
- [x] Group audit navigation and show relevant columns with an all-columns option.
- [x] Align successful-HTML audit eligibility and known failure counts across memory, SQLite, analysis and HTML reports.
- [x] Apply per-origin robots rules and shared request pacing to redirects and sitemap HTTP requests.
- [x] Preserve HTTP status in indexability decisions and honor generic repeated robots directives.
- [x] Clear stale selection/progress when switching crawl datasets and recover controls after fatal crawl errors.
- [x] Improve daily UX with direct Settings access, keyboard submission, pinned URL context, grouped URL details, dismissible dialog feedback and recoverable empty filters. Verify both themes and minimum desktop dimensions.
- [x] Compact the workbench with category filters, 28-pixel rows, a stable bottom inspector, inline paged links and right-side Overview/Issues tabs. Keep the complete audit tree available and move progress counters to the status bar.
- [x] Detect rendering availability in Settings, support rechecking browser discovery, and reject unavailable rendering before replacing results or resumable frontier state.
- [x] Integrate rendered HTTP navigation and subresources with shared robots, pacing, pause and cancellation controls; verify cross-site frames, dedicated/service workers, popups and redirects with a real browser.
- [x] Separate exact request-admission checks from browser delivery timing: verify robots/configured delays and concurrent callers directly, and check sustained browser pacing without treating compressed server-arrival gaps as policy violations.
- [x] Serialize real-browser fixtures and separate pause/deadline semantics into virtual-time tests of the actual render lifecycle. Retain real Chrome pause/resume/stop coverage with normal render budgets and explicit fallback diagnostics.
- [x] Limit Linux CI and packaging dependency installation to Ubuntu sources so unrelated third-party package-index failures cannot block builds; document retrying existing drafts with updated workflows.
- [x] Keep unfinished renders pending through long pauses and preserve the resumable URL when Stop races with worker completion.
- [x] Capture HTTP Link canonicals on HTML and non-HTML responses, combining repeated fields and HTML declarations without overriding HTTP error indexability.
- [x] Add canonical target, chain and loop reporting with original/final redirect alias evidence, valid self-canonicals, List occurrence counts, typed issues and Memory/SQLite parity. Cache a compact graph for queried summaries; keep progress counters independent. Unknown cross-host/subdomain targets remain unclassified until crawl scope is available in storage.
- [x] Push title/description/heading duplicate filters and URL regex matching into SQLite with bounded row decoding and shared Unicode text normalization.
- [x] Preserve Unicode/literal search, first-inlink and custom-field matching, List ordering and custom extraction sorting when querying SQLite directly.
- [x] Cache SQLite summaries and canonical graphs against a persisted record revision, including external-connection writes and rollbacks. Frontier-only writes no longer invalidate them; document the [storage and active-frontier measurements](docs/BENCHMARKS.md).
- [x] Reuse queue/seen INSERT statements within each full SQLite checkpoint transaction, with order/List/deduplication/rollback fixtures and matched public-method measurements. Local checkpoint persistence time falls 66–71%; full frontier replacement and aggregate scans remain open scale work.
- [x] Combine SQLite progress counts using existing view predicates and avoid repeated populated duplicate-text normalization. Empty/mixed/List and revision fixtures pass; fresh summaries over 10,000 records take 17–22% less time in the matched local workload. Record scans and duplicate grouping remain.
- [x] Match SQLite metadata blank checks to Memory for tabs and Unicode whitespace in imported records. Reuse shared trimming without rewriting evidence; verify missing/short/narrow/same-as-H1 views, summary parity, List occurrences and advanced-filter composition.
- [x] Remove the remaining cross-record hreflang memory fallback with SQL alias joins, deterministic target selection and bounded record decoding. Verify redirected/List URLs, missing/invalid evidence, paging, updates and SQLite work growth when many records share a final URL.
- [x] Skip redundant dispatch-only frontier checkpoints while retaining waiting/in-flight recovery, List metadata and completed records. Verify task-abort/reopen/resume in Memory/SQLite and measure the existing 1,000-page crawler on tmpfs and physical NVMe; SQLite NVMe medians fall 14–15% in the [matched local workload](docs/BENCHMARKS.md#avoiding-dispatch-only-checkpoints).
- [x] Persist ordinary crawl completions incrementally: remove completed keys, append discoveries/seen keys and update sitemap flags/counters in one SQLite transaction; mutate Memory frontiers in place. Verify List identity, rollback, late sitemap provenance and abort/reopen/resume. The [storage workload](docs/BENCHMARKS.md#incremental-frontier-checkpoints) writes four changed rows instead of up to 201,015; the local NVMe crawler improves a further 16–17% over dispatch-only checkpoint removal.
- [ ] Improve active large-crawl query/frontier performance using measured benchmarks; incremental summaries, startup and interrupted-discovery snapshot hydration, larger mixed edge/frontier loads, concurrent UI queries and broader physical-disk runs remain open.
- [x] Reuse the last durable frontier on ordinary Stop and persist worker errors incrementally. Preserve partial linked-sitemap discoveries with a fallback checkpoint and use recovery counts when initial sitemap loading is cancelled. Verify no extra SQLite commit on Stop plus reopen/resume and Memory/SQLite partial-discovery recovery; the [100,000-seen local workload](docs/BENCHMARKS.md#stopping-without-replacing-the-durable-frontier) reduces SQLite Stop median from 274.708 ms to 9.233 ms in the debug build. Startup hydration and scheduler cleanup remain size-dependent.
- [x] Extend the local crawler workload to configurable sizes and concurrent bounded grid/link/recovery queries. Verify 2,000 local pages on physical NVMe, all 2,060 stored URLs and 14,220 edges, plus Stop/reopen/resume in both backends; document [poll latency, memory and limits](docs/BENCHMARKS.md#larger-local-crawls-with-concurrent-queries). Larger live datasets and actual desktop responsiveness remain open.
- [x] Query recovery counts without hydrating the complete queue/seen checkpoint, preserving Memory/SQLite recovery semantics and custom-backend compatibility. Native reads use a background worker; the [isolated one-million-seen SQLite workload](docs/BENCHMARKS.md#recovery-counts-without-frontier-hydration) falls from a 72.260 ms median to 1.158 ms. Scheduler startup, interrupted-discovery snapshots and broader active-crawl performance remain open.
- [x] Deliver the [AI audit reporting plan](docs/superpowers/plans/2026-09-14-ai-audit-reporting.md): polished single-crawl and follow-up reports, grounded AI explanations, complete affected-URL inspection and portable HTML evidence exports. Deterministic frozen evidence, optional single-crawl/comparison AI, bounded workspace queries and complete offline HTML/CSV packages are verified by AI-R01–AI-R06 below.
- [x] Add manual PageSpeed measurement for the selected URL, optional OS-keyring credentials in Settings and latest-result persistence. Preserve device/date/URL attribution, exact List occurrences, cancellation and previous results on failures.
- [x] Extend PageSpeed with bounded bulk measurement (up to 500 selected rows, one request at a time, cancellable, progress events), Lighthouse category choices, quota-aware retry (429/503 backoff) and resume (rows already measured with the same device are skipped), plus PageSpeed/field-data grid columns and CSV/XLSX fields.

See [FEATURE_COMPARISON.md](docs/FEATURE_COMPARISON.md) for the evidence-based comparison and the limits of implemented features.

## Requested Workbench And Configuration Improvements

These items extend the existing workbench, using Ferrous Frog's own design and terminology. Implement supported controls with persisted configuration and engine tests; do not expose inactive placeholders.

The detailed [configuration coverage backlog](docs/CONFIGURATION_BACKLOG.md) tracks the full settings families, current code evidence, delivery order and account-dependent work, including Content, API access, authentication and post-crawl analysis from the supplied screenshots.

- [x] Add a persisted toolbar scope selector for current host, start folder, all subdomains, and exact URL, synchronized with Settings and locked during a crawl. Preserve legacy host/descendant profiles and custom folder combinations; use public/private suffix boundaries for all subdomains, skip sitemap discovery for exact URL, and recheck saved Spider queues against the selected scope on resume.
- [x] Add a Mode menu for Spider/List, archive comparison and offline SERP preview/import/export. Preserve mode-specific start URLs, lock unavailable actions with explanations, and keep snippet drafts separate from crawl results with bounded CSV imports and shared width estimates.
- [x] Add keyword search, collapsible groups, breadcrumbs and keyboard-accessible headings for the existing Settings sections. Verify navigation leaves saved crawl preferences unchanged and fits both themes and small screens.
- [x] Expand Settings into seventeen sections, including dedicated Limits, Speed and robots.txt panes. Search actual control labels with a grouped picker and Enter-to-focus; expose contextual help and unavailable controls while preserving drafts, validation, saved preferences, both themes and narrow layouts.
- [x] Add configurable audit thresholds (title/description characters and pixels, H1/H2 length, large-image size) evaluated per query in memory and SQLite views, analysis and the HTML report, with validation and reset.
- [x] Add Apply/Cancel/OK to configuration drafts, including profile loads, with native rule validation, number-field focus, persisted preferences, cancellation of pending validation, storage-failure recovery and both-theme/mobile checks. Explicit session/archive/credential/profile-save actions remain immediate; workspace-changing actions require a clean draft and lock editing/start until complete. The last requested profile wins, and cancelled draft requests cannot publish late results or errors.
- [x] Add optional max folder depth, max URL length and max links-per-page discovery limits with seed, sitemap, resume and link-evidence rules preserved.
- [x] Separate Crawl and Store choices for resources (images, CSS, JavaScript and other files), internal/external hyperlinks, canonical, pagination, hreflang, AMP, meta refresh and iframe references. Effective Crawl forces Store; disabled List/Exact URL reference discovery does not override Store. Resource retention controls edges/counts/images, while reference evidence preserves exact source occurrences, resolved targets, types and nofollow independently of audit metadata and hyperlink counts. Memory/SQLite expose bounded reference queries; the References inspector pages 100 rows and protects against stale selections/session responses. SQLite and backward-compatible schema-1 archives preserve evidence; older crawls require recrawling.
- [x] Add independent default-off canonical, hreflang, pagination and AMP discovery in Spider mode, including rendered references and repeated HTTP Link canonicals. Preserve metadata/hyperlink evidence and request/scope/resource/nofollow rules; List and Exact URL do not expand. Reference retention and source occurrence inspection are implemented; HTTP-versus-DOM provenance remains separate work.
- [x] Separate internal/external nofollow choices with legacy-profile migration, page-level directives, retained link evidence and explicit List behavior. These choices govern new discovery; queued resume URLs retain their original eligibility because saved frontier entries do not contain rel provenance.
- [x] Repair the external resource opt-in so linked external URLs can be checked without expanding their pages. Preserve robots, include/exclude rules, Exact URL limits, internal sitemap seeds and external permissions on resume.
- [x] Separate checking links outside the start folder from recursively following them, including redirect/resume/nofollow/resource behavior. Keep robots and existing speed defaults.
- [x] Expand sitemap configuration with HTML-linked sitemaps, robots.txt discovery, an origin probe and explicit Spider sources, sharing robots/rate/cancellation controls. Preserve recursive indexes, List inputs, scope, cross-source deduplication and late membership updates.
- [x] Add a persisted HTTP response limit (20 MiB default, up to 1 GiB), enforced during decoded streaming for pages, robots.txt and sitemaps. Retain known response evidence and exclude incomplete HTML from on-page audits. Chromium network download bounds remain separate work.
- [x] Add validated non-secret HTTP headers restricted to the starting origin across pages, robots, sitemaps, redirects and browser requests; persist through settings/profiles and reject credential/transport headers before saving.
- [x] Use editable Chrome desktop request defaults with Chrome/Ferrous Frog restore actions in HTTP headers. Preserve explicit saved settings and empty header lists. Keep native Chrome resource negotiation and supported HTTP compression; verify default/custom headers across page, robots, sitemap and browser requests.
- [x] Add CDN host classification while keeping external crawl permissions separate.
- [x] Link authentication settings to the Phase 5 basic/digest/form-login work: Settings > HTTP headers holds HTTP (Basic/Digest) and form login credentials. SERP preview stays separate from any future search-provider integration requiring credentials.

## Phase 0 - Foundation

Goal: keep a buildable project skeleton with clear crate boundaries.

- [x] Cargo workspace.
- [x] Tauri 2 application shell.
- [x] React, TypeScript, and Vite frontend.
- [x] `crawler-core` crate.
- [x] `parser` crate.
- [x] `analysis` crate.
- [x] `extractors` crate.
- [x] `storage` crate.
- [x] `export` crate.
- [x] `integrations` crate.
- [x] Root `Makefile` for common development commands.
- [x] Dependency versions refreshed against current stable releases during implementation.
- [x] Workspace tests and frontend build pass.
- [x] GitHub CI and Release Please workflows, synchronized workspace/npm/Tauri versions, and desktop installer configuration.
- [x] Verify the first hosted six-target installer matrix: v0.2.0 published 12 Windows, macOS and Linux packages with verified SHA256SUMS.
- [ ] Verify native installation/startup/quit on Windows, macOS and Linux. The Linux embedded-assets debug app passes a real WebDriver crawl, robots/crawl-delay checks, planted 404, saved SQLite reopen without requests and cancel/confirm quit with process exit. Installer validation and Windows/macOS checks remain pending. See [native testing](docs/NATIVE_TESTING.md).
- [ ] Developer ID signing/notarization and Windows code signing before trusted signed distribution.

## Phase 1 - Make The Frog Crawl

Goal: ship a small but complete SEO crawler workflow.

### Crawl Engine

- [x] Spider mode from a seed URL.
- [x] Basic List mode from pasted URLs.
- [x] Duplicate preservation for List mode.
- [x] Original-list-order storage and export.
- [x] Sitemap URL input source for List mode.
- [x] File upload input source for List mode.
- [x] Preserve file-import occurrences and append order in Settings drafts and saved List inputs, matching the engine's duplicate-preserving behavior.
- [x] URL normalization and deduplication.
- [x] Depth limit.
- [x] Total URL limit.
- [x] Default total URL limit raised to 5,000.
- [x] Bounded async worker pool.
- [x] Per-host requests-per-second rate limiting with `governor`.
- [x] Configurable request delay.
- [x] robots.txt support, respected by default.
- [x] Default speed of 8 concurrent requests, 10 requests/second per host and 100 ms minimum request spacing, with robots.txt and site-declared crawl delays respected. Settings provides a default preset and an explicit benchmark preset with robots.txt disabled; existing saved speeds remain unchanged.
- [x] robots.txt `Crawl-delay` parsing with measured coverage.
- [x] Custom robots.txt override.
- [x] robots.txt single-URL tester.
- [x] robots.txt download action for override text.
- [x] Batch robots.txt tester.
- [x] Configurable User-Agent.
- [x] Request timeout.
- [x] Retry policy with exponential backoff.
- [x] Shared per-origin Retry-After delays for Rust page/robots/sitemap HTTP 429/503 responses, including dates, overflow, repeated headers, concurrent waiters, pause/stop and exhausted-attempt evidence. Browser requests honor observed delays; browser response observation and cross-run cooldown persistence remain separate work.
- [x] Manual redirect handling.
- [x] Full redirect-chain recording.
- [x] Redirect-loop detection.
- [x] Broken-link detection through failed responses and 4xx/5xx statuses.
- [x] Pause/Resume pauses the scheduler and worker request boundaries; active in-flight HTTP requests are allowed to finish.
- [x] Request-level cancellation coverage for Stop by aborting active worker tasks.

### Scope And Discovery

- [x] Include URL regex rules.
- [x] Exclude URL regex rules.
- [x] Query-string sorting.
- [x] Query-string full stripping.
- [x] Regex-based query parameter stripping.
- [x] Maximum retained query parameter count.
- [x] Resource-type crawl toggles for HTML, images, CSS, JavaScript, external URLs, and other files.
- [x] Parser discovery for image, stylesheet, and script resources.
- [x] Subdomain scope modes.
- [x] Exact-folder and subfolder scope modes.
- [x] Follow or ignore `nofollow` behavior toggle.
- [x] Crawl outside start folder toggle.

### Capture

- [x] Original URL and final URL.
- [x] Internal/external classification.
- [x] Sitemap membership flag.
- [x] HTTP status code and status text.
- [x] Content type.
- [x] Indexability and indexability reason.
- [x] Response time.
- [x] DNS lookup timing.
- [x] Separate TCP connect timing.
- [x] Separate TLS handshake timing.
- [x] TTFB/header wait timing.
- [x] Download timing.
- [x] Total network timing.
- [x] Transfer rate.
- [x] Resolved IP count.
- [x] Response size.
- [x] Redirect target and redirect type.
- [x] Response hash with BLAKE3.
- [x] Crawl depth.
- [x] Title text and length.
- [x] Title pixel width.
- [x] Meta description text and length.
- [x] Meta description pixel width.
- [x] Measured title and description tag counts, nullable for old or unparsed records, with sortable grid/detail evidence and CSV/XLSX export.
- [x] H1/H2 text, lengths, and counts.
- [x] Meta robots.
- [x] X-Robots-Tag.
- [x] Canonical URL and multiple canonical count.
- [x] AMP URL.
- [x] rel next/prev.
- [x] Hreflang count, syntax basics, and normalized alternate URL storage.
- [x] JSON-LD count, syntax validity, and basic schema.org/rich-result field validation.
- [x] Open Graph and Twitter Card counts.
- [x] Word count.
- [x] Text-to-code ratio.
- [x] SimHash near-duplicate fingerprint and cluster id.
- [x] Image count.
- [x] Missing image alt count.
- [x] Long image alt count.
- [x] Per-image asset records with dimensions and byte-size thresholds.
- [x] Mixed content count.
- [x] Insecure form count.
- [x] HSTS, CSP, X-Frame-Options, and X-Content-Type-Options header flags.
- [x] Viewport/mobile flag.
- [x] In-link and out-link counts.
- [x] First in-link source URL, anchor text, and source position for diagnosing broken URLs.
- [x] Source-to-target link edges with anchor text, rel, nofollow, status, depth, source position, and discovery order.

### Storage

- [x] In-memory store.
- [x] Query windows for paginated, filtered, sorted result slices.
- [x] Server-side search and sorting for core URL rows.
- [x] Summary counters for live progress.
- [x] SQLite database mode.
- [x] SQLite-backed named crawl sessions.
- [x] SQLite-first desktop startup with a centered URL/type launcher, searchable saved crawl cards in a bottom panel and Home/return navigation. History starts collapsed, remembers the user's preference and closes with Escape/focus restoration.
- [x] Separate automatically saved SQLite sessions for new crawls, preserved legacy databases, stored configuration restore and explicit frontier resume.
- [x] Two-card saved crawl comparison with native SQL, complete aggregate counts and a bounded detail response, preserving the active workspace.
- [x] Reopen and delete saved sessions from compact history cards with deletion at the upper-right; confirm deletion, retain failed deletions for retry, and preserve unrelated open results.
- [x] Stable storage keys for duplicate-preserving List mode rows.
- [x] SQLite migration path from older `final_url` unique tables to `storage_key` unique tables.
- [x] Full resumable frontier state with queue, seen set, and scheduler state.
- [x] Portable crawl archive export/import.
- [x] Stream schema-1 archives through an idle background worker and atomic buffered writer, preserving backend record order, imported seed and complete frontier state. Page SQLite records and all edges/images/references without the former one-million-item cutoff; reject incomplete/changed pages and preserve existing files on failure. Memory record snapshots, frontier hydration and archive import/compare buffers remain scale work.

### Analysis And Issue Views

- [x] Internal and External views.
- [x] Response-code groups: 2xx, 3xx, 4xx, 5xx, no response.
- [x] Broken links view.
- [x] Page title missing, duplicate, too short, too long, pixel-width, same as H1.
- [x] Meta description missing, duplicate, too short, too long, and pixel-width.
- [x] Multiple-title/description warnings on complete successful HTML, including empty tags while excluding SVG titles and template content. Preserve unknown counts in older archives/databases; keep typed issues, Memory/SQLite summaries, HTML reports and audit workbooks consistent.
- [x] Align retained titles, descriptions, robots, viewport and headings with active HTML extraction; exclude inert template metadata and foreign-namespace labels while preserving active order, repeated robots merging and content-region independence.
- [x] H1/H2 missing, duplicate, too long.
- [x] Canonical missing and multiple canonical views.
- [x] Separate next/previous pagination target-error views with source counts, sortable declared-target columns, typed issues, redirect/List evidence and bounded SQLite query parity. Unknown and robots-blocked targets remain unclassified.
- [x] Direction-specific next/previous pagination loops, including self-links and paths entering cycles, with typed issues, source counts and bounded Memory/SQLite query parity. Normal next/previous reciprocity stays valid; unknown or incomplete targets terminate evidence paths.
- [x] Direction-specific non-reciprocal pagination warnings using the first captured relation, observed complete HTML targets, successful redirect aliases and separate List occurrences. Unknown return destinations remain unclassified; self-pagination is covered by loop errors. Capture and optional discovery recognize the legacy `previous` synonym for `prev`.
- [x] Preserve ordered valid `next`/`prev` target arrays, including repeated declarations and the `previous` synonym, with nullable legacy unknown state in Memory/SQLite and schema-1 archives. Add an informational multiple-target inventory with bounded query parity, inspectable full values, source counts and raw CSV/XLSX columns. First-target graph diagnostics remain unchanged; full per-target sequence analysis is separate work.
- [x] AMP target-error view, sortable captured `amphtml` target, source counts and typed issues through the same cached target evidence.
- [x] AMP canonical-return warning for observed complete HTML targets using the first captured `amphtml` relation, known canonical destinations, redirect aliases and separate List occurrences. Unknown, blocked, incomplete and non-HTML targets, plus malformed or unresolved canonical returns, remain unclassified; AMP markup validation and multiple declarations remain open.
- [x] Directives/noindex view.
- [x] Image missing alt and long alt views.
- [x] Security mixed content, insecure forms, and missing security header views.
- [x] Mobile missing viewport view.
- [x] Hreflang invalid and missing self-reference views.
- [x] Hreflang return-link validation.
- [x] Hreflang canonical consistency validation.
- [x] Structured-data invalid JSON-LD syntax view.
- [x] Basic schema.org or rich-result structured-data validation.
- [x] Near-duplicate cluster view.
- [x] Exact Response Duplicates audit for complete successful HTML hashes spanning distinct normalized final URLs, with typed issues, query-owned counts, bounded SQLite pages and workbook evidence. Repeated List rows or redirect aliases alone do not create a group.
- [x] Sitemap orphan view.
- [x] Dedicated sitemap validation report.
- [x] Page sitemap-validation rows directly in SQLite with shared-snapshot count/page queries, Unicode search, sorting, duplicate-preserving List order and bounded decoding. Native queries and CSV exports share the path; see [repeated-page measurements](docs/BENCHMARKS.md).
- [x] HTML validation and deprecated tag checks.

### UI

- [x] Theme-aware 520×340 splash with the original frog, animated crawl network, reduced-motion support and a 2.2-second minimum display. Preserve crawl-library readiness handoff, visible history-load errors/retry and the 12-second startup fallback.
- [x] GitHub stable-release notifications after startup, manual update checks, persistent 24-hour reminders, and browser-based downloads with offline/error recovery.
- [x] Yes/No confirmation for More > Quit and native close/exit requests, with keyboard focus restoration and cancellation cleanup before exit.
- [x] Align Settings checkboxes with adjacent inputs and selects.
- [x] Disable the default browser context menu in production workspace/splash pages; verify built assets while preserving development inspection.
- [x] Top toolbar with URL input, mode selector, Start/Pause/Resume, Stop, Export, and More actions.
- [x] Seed URL is read-only while crawling.
- [x] Start button changes to Pause and Resume.
- [x] Stop button is destructive styled.
- [x] Scrollable issue tabs above the table.
- [x] Central result grid with virtualized rows.
- [x] Server-side paginated row loading.
- [x] URL tree view grouped by host/path alongside the table view.
- [x] Prominent 4xx, 5xx, and no-response highlighting in table and tree views.
- [x] URL detail explains whether a URL was found from a page link, sitemap, List mode input, or the toolbar start URL.
- [x] Fixed URL inspector with clear selection, keyboard tabs and inline paged Inlinks/Outlinks.
- [x] Settings modal with initially collapsed, indented navigation groups, fixed search that reveals matches, a grouped section picker on narrow windows and scroll reset when changing sections.
- [x] Automatically persist and restore the last crawl configuration, storage mode, and resume preference, with safe defaults for invalid snapshots and visible storage failure feedback.
- [x] About modal.
- [x] Blurred modal backdrops with dark-mode support.
- [x] Light and softer charcoal dark palettes with layered surfaces and shared graph tokens. System is the first-launch default and follows live OS changes; explicit Light/Dark choices persist and apply before app startup.
- [x] Bottom status bar with crawl state, progress, crawled/discovered/queued counts, and crawl speed.
- [x] Persistent, resizable right-side Overview panel.
- [x] Center the main window on first launch and persist its position, size and maximized state. Keep splash visibility independent and recover unreachable title bars after monitor changes; verify monitor geometry and startup policy with native unit tests. Real multi-monitor desktop testing remains in the platform checklist.
- [x] Right-side Issues tab with summary counts and direct audit-filter navigation.
- [x] Overview status, indexability, URL distribution, metadata, heading, canonical/directive, image, technical, and issue summaries.
- [x] Overview click-through filters.
- [x] Overview crawl-speed trend chart.
- [x] Individual column visibility with the URL column retained and relevant/all defaults.
- [x] Keyboard-accessible column reorder controls.
- [x] Named layout presets with restart persistence, validation and deletion.
- [x] Screen, drawer, sliding sidebar, disclosure, dialog and detail transitions with reduced-motion support. Retain closing content, remove hidden controls from keyboard navigation and restore focus; preserve stable live grid rows.
- [x] Typed advanced filter builder with All/Any groups, text and numeric operators, native validation, Apply/Cancel and bounded Memory/SQLite query parity. Preserve filters in tree view and CSV/XLSX/sitemap exports; reset on dataset changes.

### Export

- [x] CSV export for current queried view.
- [x] XLSX export for current queried view.
- [x] XML sitemap export for current queried view.
- [x] Graph JSON export.
- [x] Graph node CSV export.
- [x] Graph edge CSV export.
- [x] HTML report export rendered with MiniJinja.
- [x] Link edge CSV export.
- [x] Redirect-chain CSV export.
- [x] Sitemap validation CSV export.
- [x] List position and duplicate index in CSV/XLSX exports.
- [x] Native file export command that writes files to Downloads/Ferrous Frog/exports with app-data fallback.
- [x] Selected-row CSV clipboard export with a 4 MiB cap and visible access errors.
- [x] Exact-ID selected-row CSV export with Ctrl/Cmd/Shift selection, page-bounded select-all and stale-selection cleanup.
- [x] Export presets (`basic`, `audit`, `full`) defined once in the export crate and used by the CLI.
- [x] Consolidated XLSX workbook with Summary, URLs, Broken Links, Redirects, Titles, Descriptions, Canonicals and Content tabs. Include exact response duplicate evidence. Export the whole idle crawl through bounded queries and temporary worksheet files; reject oversized Excel sheets and publish only complete reports.
- [x] Stable queued/in-flight CSV export, available for live and resumable stopped crawls without materializing records or the seen set.
- [x] Image-alt bulk CSV with source URLs, alt text/state, dimensions and known sizes, using bounded shared image queries and atomic publication for stopped crawls.
- [x] Cache normalized image-record aliases by evidence revision for repeated query/export pages; verify external writes, rollbacks and live image-only changes and document the workload.
- [x] Move image/link/anchor/sitemap read commands onto guarded workers and propagate SQLite errors. Preserve existing paging and active-crawl availability; SQLite sitemap validation now pages before record decoding. Query cancellation remains separate work.
- [x] Opt-in retained client-observed response headers, raw/rendered HTML and scoped visible text, with independent default-off switches, bounded storage, sensitive-header redaction, exact occurrence keys, selected plain-text previews and filtered streaming CSV exports. Preserve Memory/SQLite and backward-compatible archive round trips; report unavailable versus empty captures and storage/preview truncation. Original wire, request and browser headers remain outside this capture scope.

## Phase 2 - Scale And Reports

Goal: handle large and persistent crawls.

- [x] SQLite database mode.
- [x] Named crawl sessions.
- [x] Server-side filtering, sorting, and pagination pushed into SQLite for current standard views.
- [x] Full queue/seen-set persistence for true stop-close-resume behavior.
- [x] Memory budget and crawl capacity estimates.
- [x] Index Memory record aliases while preserving earliest-record and List occurrence semantics; summarize borrowed records for progress. The local crawler workload drops from about 27 s to 6.6 s with unchanged request counts and crawl limits; see [measurements and limitations](docs/BENCHMARKS.md).
- [x] Database location controls.
- [x] Crash recovery messaging and behavior.
- [x] Import/export of database-backed crawls.
- [ ] (Partial) Full audit-rule set for all technical SEO tabs. Core row audits, canonical target/chains/loops, pagination target errors/loops, captured-relation reciprocity warnings and multiple-target inventory, AMP target errors and canonical-return warnings, hreflang checks, structured-data checks, security checks, sitemap orphaning, near-duplicates, and HTML validation signals exist; meta keywords are captured, searchable, sortable and exported; remaining gaps include full per-target pagination graph and sequence analysis, AMP markup and multiple declarations, and richer HTML validation.
- [ ] (Partial) Streaming exports for every bulk report. Grid, selected/queued URL, image-alt and link/redirect/sitemap-validation CSV plus XML sitemap write incrementally. Filtered XLSX and the audit workbook use bounded queries and temporary worksheet files; graph JSON and node/edge CSV serialize bounded snapshots directly. Archives stream JSON with SQLite record paging and complete edge/image/reference/capture paging, while retaining Memory snapshots and full frontier hydration. HTML file reports stream template output and all link edges, with atomic desktop publication and full record input snapshots; CLI/automatic whole-link CSV exports also stream without an edge ceiling. Legacy string APIs and archive import still retain full output/input buffers. Comparison stages archives once before bounded queries; initial archive/Memory record hydration remains unbounded.
- [x] Stream legacy HTML link evidence and whole-link CSV exports through borrowed Memory edges or one SQLite cursor. Preserve all ten HTML audit sections and exact counts above one million edges; full record hydration remains. See [the isolated edge workload](docs/BENCHMARKS.md#legacy-html-and-link-csv-1000001-edges).
- [x] Run and document a [large synthetic 1M URL storage benchmark](docs/BENCHMARKS.md), including duplicate/regex pages, summary latency and process memory. Physical-disk and active-crawler load tests remain separate work.

## Phase 3 - Rendering And Extraction

Goal: support advanced crawling and custom analysis.

- [x] JavaScript rendering backend abstraction.
- [x] Chrome CDP rendering backend spike behind the `js-rendering` Cargo feature.
- [x] Optional rendered DOM crawling toggle and Settings UI controls.
- [x] Settings reports build support and the detected browser executable, refreshes availability on demand, and preserves saved options when rendering is unavailable. Start checks rendering before clearing results; HTML crawling remains available.
- [x] Raw HTML versus rendered DOM diff.
- [x] JS-injected link extraction through the rendered DOM parser path when rendering is enabled.
- [x] Rendered content extraction through the rendered DOM parser path when rendering is enabled.
- [x] Content include/exclude CSS regions with shared word-count, text/code, near-duplicate and rendered-delta semantics. Preserve full-document metadata/discovery and exact response hashes; validate selectors and preview bounded pasted HTML without saving it.
- [x] Custom extraction by CSS text.
- [x] Custom extraction by CSS attribute.
- [x] Custom extraction by XPath.
- [x] Custom extraction by regex.
- [x] Test a selected draft extractor against bounded pasted HTML, with per-rule errors, output limits, stale-result rejection and a worker guard that survives closing/reopening Settings. Keep sample HTML out of saved configuration.
- [x] Custom extraction values stored in URL details.
- [x] Custom extraction values exported.
- [x] Server-side sort/filter over custom extraction values.
- [x] Custom search across raw HTML.
- [x] Custom search across rendered HTML.
- [x] XML sitemap generation from stored crawl data.
- [x] Near-duplicate detection with BLAKE3, SimHash, and configurable threshold.

## Phase 4 - Integrations And Visualization

Goal: enrich crawl data and improve investigation workflows.

### Visualization

- [x] Crawl graph storage with queryable URL nodes and source-to-target edges.
- [x] Build bounded graph snapshots from borrowed Memory records and selected SQLite graph columns, preserving literal URL/List order, node metadata and edge placeholders. Share fallible snapshot queries between guarded native workers and atomic exports; retain the previous graph on refresh failure. Verify read-snapshot consistency and document the 50,000-record workload.
- [x] Graph JSON export.
- [x] Sigma/Graphology graph modal.
- [x] Large section-cluster canvas, searchable keyboard URL navigation, related-node inspection, responsive navigation rail and pan/zoom/fit controls in WebGL and SVG. Preserve geometry and camera across live/theme updates; keep capped-snapshot counts visible.
- [x] Load WebGL libraries and the offline snippet editor on demand, preserve the SVG fallback, and enforce the existing 500 kB production chunk budget in the UI smoke check.
- [x] Theme-aware graph styling.
- [x] Live graph refresh while crawling, scheduling the next query after completion so slow snapshots cannot be continually discarded. Browser coverage preserves the camera and final completion refresh.
- [x] Graph filters for internal-only, status, and depth.
- [x] SVG fallback layout mode.
- [x] Selected-node details.
- [x] Graph node CSV export.
- [x] Graph edge CSV export.
- [x] Broken-link edge shortcuts.
- [x] Redirect edge shortcuts.
- [x] Internal crawl path explorer.
- [x] Directory tree visualization.
- [x] User-defined URL segments.
- [x] Crawl comparison for added, removed, changed URLs and issue deltas.
- [x] Correct noisy comparison classifications: raw response hashes alone no longer mark a URL Changed. Compatible exact content fingerprints and tracked status/title/description/indexability/heading/canonical/robots fields determine changes; HTML fingerprints exclude non-content subtrees and retain version/source/selector context, while non-HTML files compare complete bytes. Raw hashes and Exact Response Duplicates remain unchanged. SQL and archive comparison expose complete response-only/content-unavailable counts, readable reasons and an opt-in response-only filter before the row cap. Fixtures cover volatile HTML, real text/SEO changes, binary/empty bodies, legacy/missing/context-mismatched evidence and response-only result flooding. Text-node whitespace and CSS visibility retain the documented parser limits; the detailed workspace and occurrence identity policy are described below.
- [x] Full comparison workspace for saved sessions and archives: private record snapshots and materialized results, server-side search/change/field filters and sorting, complete aggregates, bounded virtualized pages without a total row cap, expandable side-by-side captured field/audit details, and streaming filtered CSV export. Preserve the active crawl, keyboard focus and narrow-screen access. Source databases remain read-only; archives stage once, with initial record hydration and omitted edge-derived first-inlink fields documented.
- [x] Preserve distinct comparison identities for redirect aliases and repeated List inputs across saved sessions, archives and compatibility commands. Match normalized original request URL plus one-based occurrence: preserve complete unique positive duplicate indexes, otherwise rank relative List position and record ID. Retain gaps in explicit occurrence indexes and ignore unrelated global List reordering. Show final destination changes, original/final URLs, source positions and occurrence evidence in the workspace and CSV; fixtures verify cross-route parity, legacy fallbacks and exact selected occurrence details.

### External Data

- [x] `integrations` crate.
- [x] Google Search Console integration: Search Analytics provider, merge-to-grid workflow, Settings status/test controls and a desktop OAuth flow (loopback redirect, PKCE, automatic refresh) through a connected Google account; a pasted access token remains the fallback.
- [x] Google Analytics 4 integration: the Data API `runReport` per host and path merges sessions, engaged sessions, key events and revenue onto crawled URLs (scheme-insensitive), with grid columns and CSV/XLSX fields, using the connected Google account.
- [x] PageSpeed Insights integration. Selected-URL and bulk selected-row Mobile/Desktop runs with category choices, optional OS-keyring credentials, cancellable bounded requests, quota-aware retry and resume; four Lighthouse category scores and lab LCP/CLS/TBT persist per row, in archives, grid columns and CSV/XLSX exports. INP remains unavailable from Lighthouse; result history is not kept beyond the latest snapshot.
- [x] Core Web Vitals field-data provider: the Chrome UX Report API returns p75 LCP/INP/CLS/FCP/TTFB per form factor with the collection period and an explicit no-data state; snapshots persist per row in Memory/SQLite and archives, using the saved Google API key. Lighthouse lab metrics stay separate. Grid/export columns remain open.
- [x] Pluggable backlink API integration points: a user-configured HTTP(S) endpoint template with `{url}` and an optional credential header (kept in the OS credential store) returns `backlinks`, `referringDomains` and `authorityScore` per crawled internal HTML URL, merged into grid columns and CSV/XLSX fields. Vendor-specific adapters can build on the same contract.
- [x] OAuth credential storage outside source control: the OAuth client and access/refresh tokens live in the OS credential store; refresh happens transparently before Search Console and Analytics requests.

## Phase 5 - Automation And AI Assist

Goal: automate recurring work and add optional AI-assisted analysis.

Status: existing automation and per-URL AI actions are available. User-requested saved audit reporting is tracked below; remaining scale work follows the core crawler/reporting priorities.

- [x] Headless CLI crawl command (`ferrous-frog-cli crawl <url>`), sharing the engine, validation, Memory/SQLite storage and progress reporting.
- [x] CLI list mode command (`ferrous-frog-cli list <urls...|file>`).
- [x] CLI config profile selection from the app's saved profiles (`--profile`, `profiles`) or an exported JSON file (`--config`).
- [x] CLI output folder and export preset options (`--output`, `--export`, `--preset basic|audit|full`, `--database`).
- [x] Scheduled one-off crawls: Settings > Automation > Schedule runs the configured start URL once at a chosen time while the app is open and idle.
- [x] Scheduled interval crawls: repeat every 5 minutes to 7 days from an optional first-run time; progress survives restarts and resets when the schedule changes. No background service runs while the app is closed.
- [x] Auto-export workflows: Settings > Automation runs the basic/audit/full preset into a new exports folder when a crawl finishes.
- [x] Completion notifications: optional desktop notification plus in-app notices for export/webhook outcomes.
- [x] Webhook notification hooks: an HTTP(S) endpoint receives a JSON summary (status, counts, summary, export paths) after finished and failed crawls; failures are reported as notices.
- [x] Auth/login crawl flows: Basic, Digest and form login credentials live in the OS credential store and are enabled per configuration. Browser-rendered logins and cookie transfer to the rendering browser remain outside these flows.
- [x] Basic auth: Settings > HTTP headers saves username/password in the OS credential store; the enabled flag persists with profiles while secrets never do. The CLI reads `FERROUS_FROG_BASIC_USER`/`FERROUS_FROG_BASIC_PASSWORD`.
- [x] Digest auth: a `WWW-Authenticate: Digest` challenge from the starting origin is answered once per request with the saved HTTP credentials (MD5/MD5-sess, `qop=auth` or legacy); unsupported algorithms or `auth-int` leave the 401 in the results.
- [x] Form login and cookie jar workflow: a saved username/password is posted once to the configured login URL with the chosen field names and extra fields; the session cookies then accompany every crawl request. Browser rendering does not share the jar.
- [x] Configurable LLM provider support: Anthropic's Messages API (default model `claude-opus-5`, server-side refusal fallbacks) or any OpenAI-compatible chat-completions endpoint, with model and base URL settings.
- [x] User-managed API keys: the AI provider key lives in the OS credential store, outside profiles and browser storage.
- [x] Rate-limited prompts against page content: a per-minute request window, re-fetched page text bounded by a character limit, script/style-free visible text and a prompt that treats page text as untrusted data.
- [x] Content intent classification (informational / navigational / transactional / commercial with confidence and rationale) saved per row.
- [x] Draft meta description generation with alternatives, saved per row.
- [x] Thin-content and quality flags: Thin Content (word count below the threshold) and Low Text Ratio (visible text below a percentage of the HTML) views, analysis issues and configurable thresholds for successful HTML pages.
- [x] Spelling and grammar analysis: model-reported issues with suggestions and detected language, saved per row.
- [ ] Extreme-scale performance hardening and documented benchmarks.

### AI Audit Reporting

Delivered from the supplied initial/follow-up HTML references. The [design and delivery plan](docs/superpowers/plans/2026-09-14-ai-audit-reporting.md) preserves concise summaries and readable finding cards while making every affected URL and evidence occurrence accessible. Reuse the built-in AI provider/keyring configuration; this does not require a general plugin framework. The saved-report workflow is separate from existing per-URL AI and the sampled summary HTML export.

- [x] **AI-R01 — Complete report evidence:** consistent saved-crawl snapshots, versioned rules and frozen thresholds, exact finding/unique-URL/record/reference counts, explicit coverage and bounded Memory/SQLite queries. Sixteen page rules and broken-link evidence share existing storage predicates and global duplicate/reference context. Frozen source identities and eligibility survive source deletion; unsupported checks and legacy edge attribution remain explicit. Fixtures cover 1,205 records, duplicate List occurrences, 600 links to one target, scope parity, cancellation and source changes. Large-scale measurements are documented under AI-R06.
- [x] **AI-R02 — Report workspace:** saved report lifecycle, title/language/scope selection, measured summary cards, executive summary, severity/category/team filters and problem/evidence/fix sections. The full evidence area has server-side search/sort and HTTP status filtering, virtualized pages, labelled large-value previews and complete matching exports; support both themes, keyboard and narrow windows.
- [x] **AI-R03 — Optional AI explanations:** reuse provider settings and OS credentials; send bounded finding summaries and labelled evidence samples with an explicit data/request budget. Validate structured evidence references; provide progress, cancellation, usage and resumable annotations with an executive overview. Earlier usable generations retain their model/version provenance in the workspace and export. AI-off/failure states retain the complete deterministic report; no automatic website refetch or invented measurements.
- [x] **AI-R04 — Complete portable HTML report:** a polished offline index, linked paged evidence and full per-finding CSV with a count/coverage manifest. Stream all matching evidence to an atomically published folder; no silent 50-row or one-million-edge cap, remote assets or full-site frontend array. Printed/standalone summaries must not be labelled complete evidence exports.
- [x] **AI-R05 — Follow-up report:** compare stable rule/request/occurrence identities and show before/after evidence, new/resolved/improved/unchanged/worsened/mixed states and complete affected sets. Missing, blocked, removed or incompatible observations remain explicit rather than automatically resolved; Optional AI explains typed computed changes and bounded before/after samples through the shared report runner; it cannot replace measured states or membership.
- [x] **AI-R06 — Verification and scale:** test last-row reachability beyond previews/pages, exact count/export parity, incomplete captures, hostile content, provider failures/cancellation and source/configuration changes. The [100,000-page / 1,000,001-link workload](docs/BENCHMARKS.md#frozen-audit-reports-100000-pages-and-1000001-links) reconciles all 1,100,002 exported evidence rows. Full CI, offline Chrome navigation and the Linux embedded-assets native report/restart/export/quit workflow pass; other-platform and installer checks remain separate.

## Testing Tracker

- [x] Parser fixture tests for titles, metadata, headings, canonicals, links, directives, hreflang, JSON-LD, and resources.
- [x] Crawler mock-site tests for redirects, broken links, duplicate metadata, graph edges, robots blocking, invalid hreflang, invalid JSON-LD, redirect loops, scope filters, query normalization, asset crawling, duplicate-preserving List mode, List sitemap sources, robots override, robots tester, robots download, persisted frontier resume, and disabled rendering behavior.
- [x] Unavailable-rendering checks preserve results/frontier and make no HTTP requests; browser discovery rejects missing paths, directories and non-executable Unix files. UI checks cover availability, recovery, saved preferences, rejected starts and stale probe responses.
- [x] Storage tests for memory and SQLite records, query filters, near duplicates, duplicate list URLs, link edges, graph nodes, anchor text aggregation, image asset records, and frontier state roundtrips.
- [x] Export tests for CSV, XLSX, sitemap XML, HTML report, link edge CSV, and redirect-chain CSV.
- [x] Extractor tests for CSS text, CSS attribute, XPath, and regex.
- [x] Property tests for frontier/dedup logic: a dependency-free generated-URL test checks query normalization idempotence, fragment removal, strip/limit/sort invariants and single seen-set keys across variants.
- [ ] (Partial) Load test against a large synthetic site. The one-million-record storage benchmark and configurable local HTTP crawler fixture are documented. Runs through 2,000 pages verify cycles, deduplication, errors, robots, concurrency and stop/reopen/resume in Memory/SQLite, including concurrent bounded grid/link/recovery polls and physical NVMe. Larger live datasets, denser resource/reference graphs, repeated disk runs and actual desktop responsiveness remain pending.
- [x] Headless Chrome UI smoke checks for full result/report paging, stale responses, live duplicates, errors, details, graph exclusions, themes and small-screen layouts (`make test-ui`). Optional screenshots use `UI_SCREENSHOT`.
- [x] Linux native desktop end-to-end crawl check beyond IPC fixtures: embedded-assets debug app, isolated Xvfb/D-Bus/WebKit WebDriver, robots/crawl delay, 404, SQLite persistence/reopen without requests and cancel/confirm quit with native process exit (`make test-native`).
- [ ] Extend native desktop end-to-end checks to Windows/macOS and installer builds. The Linux embedded-assets debug workflow verifies saved audit-report creation, restart/reopen, complete export and clean quit.

## Definition Of Done

Each item is complete only when:

- [x] Code is implemented.
- [x] Tests or targeted verification cover the changed behavior.
- [x] The app builds.
- [x] Documentation reflects current behavior.
- [x] Known limitations are recorded.
