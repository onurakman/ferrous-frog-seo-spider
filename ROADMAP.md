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
- [ ] (Partial) Phase 4 visualization and external data work is underway: graph storage, graph UI, directory tree, URL segments, path helpers, archive-based crawl comparison, integration provider contracts, Google Search Console metric merge, and manual PageSpeed measurements saved with individual crawl rows exist. GA4, bulk PageSpeed/grid reporting, field Core Web Vitals and backlink providers remain.
- [ ] Phase 5 automation and AI assist is parked until earlier crawler/reporting work is stronger.

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
- [ ] Improve active large-crawl query/frontier performance using measured benchmarks; incremental summaries, realistic edge/frontier loads and physical-disk runs remain open.
- [x] Add manual PageSpeed measurement for the selected URL, optional OS-keyring credentials in Settings and latest-result persistence. Preserve device/date/URL attribution, exact List occurrences, cancellation and previous results on failures.
- [ ] Extend PageSpeed with bounded bulk measurement, category choices, quota-aware retry/resume and dedicated metric grid/export fields.

See [FEATURE_COMPARISON.md](docs/FEATURE_COMPARISON.md) for the evidence-based comparison and the limits of implemented features.

## Requested Workbench And Configuration Improvements

These items extend the existing workbench, using Ferrous Frog's own design and terminology. Implement supported controls with persisted configuration and engine tests; do not expose inactive placeholders.

The detailed [configuration coverage backlog](docs/CONFIGURATION_BACKLOG.md) tracks the full settings families, current code evidence, delivery order and account-dependent work, including Content, API access, authentication and post-crawl analysis from the supplied screenshots.

- [x] Add a persisted toolbar scope selector for current host, start folder, all subdomains, and exact URL, synchronized with Settings and locked during a crawl. Preserve legacy host/descendant profiles and custom folder combinations; use public/private suffix boundaries for all subdomains, skip sitemap discovery for exact URL, and recheck saved Spider queues against the selected scope on resume.
- [x] Add a Mode menu for Spider/List, archive comparison and offline SERP preview/import/export. Preserve mode-specific start URLs, lock unavailable actions with explanations, and keep snippet drafts separate from crawl results with bounded CSV imports and shared width estimates.
- [x] Add keyword search, collapsible groups, breadcrumbs and keyboard-accessible headings for the existing Settings sections. Verify navigation leaves saved crawl preferences unchanged and fits both themes and small screens.
- [ ] Expand Settings into dedicated Limits, Advanced, Content, robots.txt, Include/Exclude, Speed, User-Agent, Custom, API access and Crawl Analysis sections as their controls are implemented; add direct control focus and contextual help.
- [x] Add Apply/Cancel/OK to configuration drafts, including profile loads, with native rule validation, number-field focus, persisted preferences, cancellation of pending validation, storage-failure recovery and both-theme/mobile checks. Explicit session/archive/credential/profile-save actions remain immediate; workspace-changing actions require a clean draft and lock editing/start until complete. The last requested profile wins, and cancelled draft requests cannot publish late results or errors.
- [ ] Separate crawl and store choices for resource types (images, CSS, JavaScript, and other files) and page-link types (internal/external hyperlinks, canonicals, pagination, hreflang, AMP, meta refresh, and iframes). Keep dependencies between choices clear and preserve bounded storage queries.
- [x] Add independent default-off canonical, hreflang, pagination and AMP discovery in Spider mode, including rendered references and repeated HTTP Link canonicals. Preserve metadata/hyperlink evidence and request/scope/resource/nofollow rules; List and Exact URL do not expand. Retention controls and reference-source attribution remain separate work.
- [x] Separate internal/external nofollow choices with legacy-profile migration, page-level directives, retained link evidence and explicit List behavior. These choices govern new discovery; queued resume URLs retain their original eligibility because saved frontier entries do not contain rel provenance.
- [x] Repair the external resource opt-in so linked external URLs can be checked without expanding their pages. Preserve robots, include/exclude rules, Exact URL limits, internal sitemap seeds and external permissions on resume.
- [x] Separate checking links outside the start folder from recursively following them, including redirect/resume/nofollow/resource behavior. Keep robots and existing speed defaults.
- [x] Expand sitemap configuration with HTML-linked sitemaps, robots.txt discovery, an origin probe and explicit Spider sources, sharing robots/rate/cancellation controls. Preserve recursive indexes, List inputs, scope, cross-source deduplication and late membership updates.
- [x] Add a persisted HTTP response limit (20 MiB default, up to 1 GiB), enforced during decoded streaming for pages, robots.txt and sitemaps. Retain known response evidence and exclude incomplete HTML from on-page audits. Chromium network download bounds remain separate work.
- [x] Add validated non-secret HTTP headers restricted to the starting origin across pages, robots, sitemaps, redirects and browser requests; persist through settings/profiles and reject credential/transport headers before saving.
- [x] Use editable Chrome desktop request defaults with Chrome/Ferrous Frog restore actions in HTTP headers. Preserve explicit saved settings and empty header lists. Keep native Chrome resource negotiation and supported HTTP compression; verify default/custom headers across page, robots, sitemap and browser requests.
- [ ] Add CDN host classification while keeping external crawl permissions separate.
- [ ] Link authentication settings to the Phase 5 basic/digest/form-login work once implemented. Keep SERP preview separate from any future search-provider integration requiring credentials.

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
- [ ] Verify native installation/startup/quit on Windows, macOS and Linux.
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
- [x] Stream schema-1 archives through an idle background worker and atomic buffered writer, preserving backend record order, imported seed and complete frontier state. Page SQLite records and all edges/images without the former one-million-item cutoff; reject incomplete/changed pages and preserve existing files on failure. Memory record snapshots, frontier hydration and archive import/compare buffers remain scale work.

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
- [x] AMP target-error view, sortable captured `amphtml` target, source counts and typed issues through the same cached target evidence. AMP markup validation and canonical reciprocity remain open.
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
- [ ] Export presets.
- [x] Consolidated XLSX workbook with Summary, URLs, Broken Links, Redirects, Titles, Descriptions, Canonicals and Content tabs. Include exact response duplicate evidence. Export the whole idle crawl through bounded queries and temporary worksheet files; reject oversized Excel sheets and publish only complete reports.
- [x] Stable queued/in-flight CSV export, available for live and resumable stopped crawls without materializing records or the seen set.
- [x] Image-alt bulk CSV with source URLs, alt text/state, dimensions and known sizes, using bounded shared image queries and atomic publication for stopped crawls.
- [x] Cache normalized image-record aliases by evidence revision for repeated query/export pages; verify external writes, rollbacks and live image-only changes and document the workload.
- [x] Move image/link/anchor/sitemap read commands onto guarded workers and propagate SQLite errors. Preserve existing paging and active-crawl availability; SQLite sitemap validation now pages before record decoding. Query cancellation remains separate work.
- [ ] Complete-header and source-text bulk exports after opt-in retention.

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
- [ ] (Partial) Full audit-rule set for all technical SEO tabs. Core row audits, canonical target/chains/loops, pagination target errors/loops and captured-relation reciprocity warnings, AMP target errors, hreflang checks, structured-data checks, security checks, sitemap orphaning, near-duplicates, and HTML validation signals exist; remaining gaps include complete pagination sequences and multiple relations, AMP markup/reciprocity, meta keywords, and richer HTML validation.
- [ ] (Partial) Streaming exports for every bulk report. Grid, selected/queued URL, image-alt and link/redirect/sitemap-validation CSV plus XML sitemap write incrementally. Filtered XLSX and the audit workbook use bounded queries and temporary worksheet files; graph JSON and node/edge CSV serialize bounded snapshots directly. Archives stream JSON with SQLite record paging and complete edge/image paging, while retaining Memory snapshots and full frontier hydration. HTML reports retain format buffers; archive import/comparison still hydrate full files.
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

### External Data

- [x] `integrations` crate.
- [ ] (Partial) Google Search Console integration. Search Analytics provider, OS keyring token storage, Settings UI status/test controls, and merge-to-grid workflow exist; a full browser OAuth consent/refresh-token flow remains.
- [ ] Google Analytics 4 integration.
- [ ] (Partial) PageSpeed Insights integration. Manual selected-URL Mobile/Desktop runs, optional OS-keyring credentials, cancellable bounded requests, four Lighthouse category scores and lab LCP/CLS/TBT persist on the selected row and in archives. INP remains unavailable. Bulk scheduling/retry, category controls, result history and dedicated grid/export fields remain.
- [ ] Core Web Vitals field-data provider and merge onto crawled URLs, with collection period, device strategy and missing-data states. Lighthouse lab metrics remain distinct from field measurements.
- [ ] Pluggable backlink API integration points.
- [ ] (Partial) OAuth credential storage outside source control. Google Search Console access tokens are stored in the OS credential store; reusable refresh-token flows remain.

## Phase 5 - Automation And AI Assist

Goal: automate recurring work and add optional AI-assisted analysis.

Status: parked for now. Do not prioritize these items until Phase 2, Phase 3, and core Phase 4 crawler/reporting gaps are materially stronger.

- [ ] Headless CLI crawl command.
- [ ] CLI list mode command.
- [ ] CLI config profile selection.
- [ ] CLI output folder and export preset options.
- [ ] Scheduled one-off crawls.
- [ ] Scheduled interval crawls.
- [ ] Auto-export workflows.
- [ ] Completion notifications.
- [ ] Webhook notification hooks.
- [ ] Auth/login crawl flows.
- [ ] Basic auth.
- [ ] Digest auth.
- [ ] Form login and cookie jar workflow.
- [ ] Configurable LLM provider support.
- [ ] User-managed API keys.
- [ ] Rate-limited prompts against page content.
- [ ] Content intent classification.
- [ ] Draft meta description generation.
- [ ] Thin-content and quality flags.
- [ ] Spelling and grammar analysis.
- [ ] Extreme-scale performance hardening and documented benchmarks.

## Testing Tracker

- [x] Parser fixture tests for titles, metadata, headings, canonicals, links, directives, hreflang, JSON-LD, and resources.
- [x] Crawler mock-site tests for redirects, broken links, duplicate metadata, graph edges, robots blocking, invalid hreflang, invalid JSON-LD, redirect loops, scope filters, query normalization, asset crawling, duplicate-preserving List mode, List sitemap sources, robots override, robots tester, robots download, persisted frontier resume, and disabled rendering behavior.
- [x] Unavailable-rendering checks preserve results/frontier and make no HTTP requests; browser discovery rejects missing paths, directories and non-executable Unix files. UI checks cover availability, recovery, saved preferences, rejected starts and stale probe responses.
- [x] Storage tests for memory and SQLite records, query filters, near duplicates, duplicate list URLs, link edges, graph nodes, anchor text aggregation, image asset records, and frontier state roundtrips.
- [x] Export tests for CSV, XLSX, sitemap XML, HTML report, link edge CSV, and redirect-chain CSV.
- [x] Extractor tests for CSS text, CSS attribute, XPath, and regex.
- [ ] Property tests for frontier/dedup logic.
- [ ] (Partial) Load test against a large synthetic site. The one-million-record storage benchmark and a 1,000-page local HTTP crawler fixture are documented. The crawler verifies cycles, deduplication, errors, robots, concurrency and stop/reopen/resume in Memory/SQLite; larger mixed sites, active UI queries and physical-disk runs remain pending.
- [x] Headless Chrome UI smoke checks for full result/report paging, stale responses, live duplicates, errors, details, graph exclusions, themes and small-screen layouts (`make test-ui`). Optional screenshots use `UI_SCREENSHOT`.
- [ ] Native desktop end-to-end crawl checks beyond IPC-fixture browser coverage.

## Definition Of Done

Each item is complete only when:

- [x] Code is implemented.
- [x] Tests or targeted verification cover the changed behavior.
- [x] The app builds.
- [x] Documentation reflects current behavior.
- [x] Known limitations are recorded.
