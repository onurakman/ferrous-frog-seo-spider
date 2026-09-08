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
- [ ] (Partial) Phase 4 visualization and external data work is underway: graph storage, graph UI, directory tree, URL segments, path helpers, archive-based crawl comparison, integration provider contracts, Google Search Console metric merge, and a PageSpeed Insights provider scaffold exist, but GA4, PageSpeed UI merge, Core Web Vitals persistence, and backlink providers are not implemented.
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
- [ ] Detect rendering availability in Settings and integrate browser requests with crawler politeness controls.
- [ ] Complete redirect URL identity, HTTP canonical-header and canonical-chain reporting.
- [ ] Remove SQLite full-record materialization from duplicate/regex queries and measure large-crawl behavior.
- [ ] PageSpeed Insights Settings UI, API key storage, and URL metric merge path after the crawler correctness and scale work above.

See [FEATURE_COMPARISON.md](docs/FEATURE_COMPARISON.md) for the evidence-based comparison and the limits of implemented features.

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
- [ ] Verify the first hosted six-target installer matrix and native installation/startup/quit on Windows, macOS and Linux.
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
- [x] URL normalization and deduplication.
- [x] Depth limit.
- [x] Total URL limit.
- [x] Default total URL limit raised to 5,000.
- [x] Bounded async worker pool.
- [x] Per-host requests-per-second rate limiting with `governor`.
- [x] Configurable request delay.
- [x] robots.txt support, respected by default.
- [x] Settings presets for safe defaults and explicit benchmark crawling with robots.txt disabled.
- [x] robots.txt `Crawl-delay` parsing with measured coverage.
- [x] Custom robots.txt override.
- [x] robots.txt single-URL tester.
- [x] robots.txt download action for override text.
- [x] Batch robots.txt tester.
- [x] Configurable User-Agent.
- [x] Request timeout.
- [x] Retry policy with exponential backoff.
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
- [x] Reopen and delete named sessions.
- [x] Stable storage keys for duplicate-preserving List mode rows.
- [x] SQLite migration path from older `final_url` unique tables to `storage_key` unique tables.
- [x] Full resumable frontier state with queue, seen set, and scheduler state.
- [x] Portable crawl archive export/import.

### Analysis And Issue Views

- [x] Internal and External views.
- [x] Response-code groups: 2xx, 3xx, 4xx, 5xx, no response.
- [x] Broken links view.
- [x] Page title missing, duplicate, too short, too long, pixel-width, same as H1.
- [x] Meta description missing, duplicate, too short, too long, and pixel-width.
- [x] H1/H2 missing, duplicate, too long.
- [x] Canonical missing and multiple canonical views.
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
- [x] Sitemap orphan view.
- [x] Dedicated sitemap validation report.
- [x] HTML validation and deprecated tag checks.

### UI

- [x] Theme-aware splash window with readiness-based handoff, visible initial-query errors, and a startup fallback.
- [x] GitHub stable-release notifications after startup, manual update checks, persistent 24-hour reminders, and browser-based downloads with offline/error recovery.
- [x] Yes/No confirmation for More > Quit and native close/exit requests, with keyboard focus restoration and cancellation cleanup before exit.
- [x] Align Settings checkboxes with adjacent inputs and selects.
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
- [x] Left-tabbed Settings modal.
- [x] Automatically persist and restore the last crawl configuration, storage mode, and resume preference, with safe defaults for invalid snapshots and visible storage failure feedback.
- [x] About modal.
- [x] Blurred modal backdrops with dark-mode support.
- [x] Light and neutral near-black dark palettes with shared graph tokens. System is the first-launch default and follows live OS changes; explicit Light/Dark choices persist and apply before app startup.
- [x] Bottom status bar with crawl state, progress, crawled/discovered/queued counts, and crawl speed.
- [x] Persistent, resizable right-side Overview panel.
- [x] Right-side Issues tab with summary counts and direct audit-filter navigation.
- [x] Overview status, indexability, URL distribution, metadata, heading, canonical/directive, image, technical, and issue summaries.
- [x] Overview click-through filters.
- [x] Overview crawl-speed trend chart.
- [ ] (Partial) Column visibility controls. Relevant/all column presets exist; individual column selection remains.
- [ ] Column reorder controls.
- [ ] Saved layout presets.
- [ ] Advanced search builder.

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
- [ ] Clipboard export.
- [ ] Selected-row export.
- [ ] Export presets.
- [ ] Consolidated XLSX workbook with multiple tabs.
- [ ] Queued URL export.
- [ ] Header/source-text/image-alt bulk exports.

## Phase 2 - Scale And Reports

Goal: handle large and persistent crawls.

- [x] SQLite database mode.
- [x] Named crawl sessions.
- [x] Server-side filtering, sorting, and pagination pushed into SQLite for current standard views.
- [x] Full queue/seen-set persistence for true stop-close-resume behavior.
- [x] Memory budget and crawl capacity estimates.
- [x] Database location controls.
- [x] Crash recovery messaging and behavior.
- [x] Import/export of database-backed crawls.
- [ ] (Partial) Full audit-rule set for all technical SEO tabs. Core row audits, hreflang checks, structured-data checks, security checks, sitemap orphaning, near-duplicates, and HTML validation signals exist; remaining gaps include pagination/AMP-specific issue views, meta keywords, canonical-chain audits, and richer HTML validation.
- [ ] (Partial) Streaming exports for every bulk report. Grid CSV, XML sitemap, link edge CSV, redirect-chain CSV, and sitemap validation CSV are written in chunks; XLSX, HTML report, graph JSON, and crawl archive still buffer their format payloads.
- [ ] (Partial) Large synthetic 1M+ URL benchmark. Ignored SQLite synthetic benchmark harness and `make bench-synthetic` target exist; a 1M run still needs to be executed and documented on target hardware.

## Phase 3 - Rendering And Extraction

Goal: support advanced crawling and custom analysis.

- [x] JavaScript rendering backend abstraction.
- [x] Chrome CDP rendering backend spike behind the `js-rendering` Cargo feature.
- [x] Optional rendered DOM crawling toggle and Settings UI controls.
- [x] Raw HTML versus rendered DOM diff.
- [x] JS-injected link extraction through the rendered DOM parser path when rendering is enabled.
- [x] Rendered content extraction through the rendered DOM parser path when rendering is enabled.
- [x] Custom extraction by CSS text.
- [x] Custom extraction by CSS attribute.
- [x] Custom extraction by XPath.
- [x] Custom extraction by regex.
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
- [x] Graph JSON export.
- [x] Sigma/Graphology graph modal.
- [x] Theme-aware graph styling.
- [x] Live graph refresh while crawling.
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
- [ ] (Partial) PageSpeed Insights integration. Provider scaffold parses Lighthouse category scores plus LCP, INP, and CLS from the official API; Settings UI, API key storage, crawl-row persistence, and merge workflow remain.
- [ ] (Partial) Core Web Vitals merge onto crawled URLs. PageSpeed provider parses LCP, INP, and CLS; storage/UI merge remains.
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
- [x] Storage tests for memory and SQLite records, query filters, near duplicates, duplicate list URLs, link edges, graph nodes, anchor text aggregation, image asset records, and frontier state roundtrips.
- [x] Export tests for CSV, XLSX, sitemap XML, HTML report, link edge CSV, and redirect-chain CSV.
- [x] Extractor tests for CSS text, CSS attribute, XPath, and regex.
- [ ] Property tests for frontier/dedup logic.
- [ ] (Partial) Load test against a large synthetic site. SQLite storage benchmark harness exists; crawler-level synthetic site load test is still pending.
- [x] Headless Chrome UI smoke checks for full result/report paging, stale responses, live duplicates, errors, details, graph exclusions, themes and small-screen layouts (`make test-ui`). Optional screenshots use `UI_SCREENSHOT`.
- [ ] Native desktop end-to-end crawl checks beyond IPC-fixture browser coverage.

## Definition Of Done

Each item is complete only when:

- [x] Code is implemented.
- [x] Tests or targeted verification cover the changed behavior.
- [x] The app builds.
- [x] Documentation reflects current behavior.
- [x] Known limitations are recorded.
