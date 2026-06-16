# ROADMAP.md

Ferrous Frog development roadmap.

## Current Status

The project has a working Phase 1 crawler slice and several Phase 2 audit/reporting foundations. The workspace, Tauri shell, React frontend, in-memory store, SQLite store, parser, analysis crate, export crate, extractor crate, and async crawler core are in place.

Implemented:

- Cargo workspace and Tauri 2 application shell.
- React, TypeScript, Vite frontend.
- Async spider mode from one seed URL.
- Bounded concurrency.
- Per-host requests-per-second rate limiting through `governor`.
- robots.txt support, respected by default.
- robots.txt `Crawl-delay` parsing with measured integration coverage.
- Include and exclude URL regex scope controls in crawler config and Settings UI.
- Resource-type crawl toggles for HTML, images, CSS, JavaScript, external URLs, and other files, with parser discovery for image, stylesheet, and script resources.
- Query-string controls for parameter sorting, full query stripping, regex-based parameter stripping, and maximum retained parameter count.
- Basic Spider/List mode selection with pasted URL lists. List mode crawls supplied URLs without spider expansion.
- Custom robots.txt override support in crawler config and Settings UI, including override crawl-delay parsing.
- robots.txt tester command and Settings UI check for arbitrary URLs against overridden robots rules.
- robots.txt download command and Settings UI action to populate override text from the current root URL.
- Manual redirect recording.
- Basic broken-link detection through failed responses and 4xx/5xx statuses.
- In-memory result storage.
- SQLite database mode for the current crawl.
- Resume option for the current database-backed crawl.
- SQLite-backed named crawl session metadata with per-session database files, reopen/delete commands, and Settings UI controls.
- Server-side query windows for filtering, sorting, searching, and pagination.
- Live crawl progress events.
- Virtualized result grid.
- Issue views for response families, titles, meta descriptions, H1/H2, canonicals, directives, images, mobile, hreflang, structured data, security, near-duplicates, and broken links.
- CSV export for the current queried view.
- XLSX export for the current queried view.
- XML sitemap export for the current queried view.
- Crawl graph storage with queryable URL nodes and source-to-target link edges, including anchor text, rel attributes, link type, status, depth, source position, and discovery order.
- Graph JSON export for a capped crawl graph snapshot.
- Interactive Sigma/Graphology crawl graph modal backed by stored URL nodes and source-to-target edges, with ForceAtlas2 layout, theme-aware styling, and live refresh while a crawl is running.
- Crawl graph filters for internal-only mode, status, depth, SVG layout mode, selected-node details, and filtered graph JSON export.
- HTML report export rendered with MiniJinja, standalone CSS, and dark/light report theme support for team handoff.
- Link edge CSV export and redirect-chain CSV export.
- Dedicated link report modal for all links, internal links, external links, broken or unresolved links, nofollow links, selected URL in-links, selected URL out-links, anchor-text aggregation, and redirect-chain rows.
- Server-side search and sorting for dedicated link report tables.
- Custom extraction through CSS text, CSS attributes, XPath, and regex, wired into crawl config, stored URL details, exports, and dynamic grid columns.
- Default same-origin `/sitemap.xml` ingestion for root crawls, with sitemap-only orphan fixture coverage.
- Sitemap orphan URL reporting through `in_sitemap` storage, summary counters, issue view filtering, Overview drill-down, and grid visibility.
- BLAKE3 response hashes, word counts, text-to-code ratio, SimHash fingerprints, and near-duplicate cluster views with a configurable threshold.
- Per-URL capture for meta robots, X-Robots-Tag, H1/H2 counts, multiple canonicals, image alt issues, mixed content, insecure forms, security-header presence, viewport, AMP, rel next/prev, hreflang basics, JSON-LD validity, Open Graph, and Twitter Cards.
- Per-request network timings for DNS lookup, TTFB/header wait, download, total network time, transfer rate, resolved IP count, and redirect-hop timing.
- Redirect loop detection with fixture coverage.
- Mock-site integration coverage for redirects, broken links, duplicate metadata, edge storage, robots blocking, invalid hreflang, invalid JSON-LD, and redirect loops.
- Dark and light themes using Etiya tokens.
- Toolbar cleanup with crawl-state moved to the status bar, read-only seed URL while crawling, and a primary Start/Pause/Resume state button.
- Bottom status bar with crawl state, progress, crawled/discovered/queued counts, and crawl speed.
- Persistent, resizable right-side Overview panel with progress, crawl counters, status-code chart, indexability chart, URL distribution, metadata, heading, canonical/directive, image, technical, and issue-signal summaries.
- Overview click-through filters for status, URL distribution, metadata, headings, canonical/directive, images, technical checks, and issue signals.
- Overview crawl-speed history with compact trend chart fed by live progress events.
- Settings dialog, About dialog, export dropdown, scrollable issue tabs, closable detail panel, and blurred modal backdrops.
- SQLite-backed named configuration profiles with save/load/delete controls in Settings.

Partially implemented or limited:

- Pause/Resume is visible in the UI and stops/resumes scheduler progress, but active in-flight requests are allowed to finish. Request-level cancellation and integration coverage are still needed.
- Network timing currently includes independent DNS lookup, header wait/TTFB, download, total, transfer rate, and redirect-hop timings. Separate TCP connect and TLS handshake timing needs lower-level connector instrumentation.
- Hreflang validation covers syntax and self-reference basics, but return-link and canonical consistency checks are not complete.
- Structured-data validation currently catches invalid JSON-LD syntax, not full schema.org or rich-result validation.
- Custom extraction columns are displayed and exported, but server-side sort/filter over custom extraction values is not complete.
- Sitemap export, default same-origin sitemap ingestion, and orphan URL reporting are in place. Dedicated sitemap validation is still needed.
- HTML report can flag large crawled image asset responses and page-level image alt issues. Per-image byte size now depends on enabling image resource crawling.
- List mode crawls supplied URLs, but duplicate preservation and original-list-order export require storage changes.

Remaining Phase 1 hardening:

- More robust external link recording and broken external link reporting.
- Better UI coverage for row details and issue counts.
- Dedicated sitemap validation.
- Server-side sort/filter support for custom extraction values.
- Deeper hreflang validation with return-link and canonical consistency checks.
- Structured-data validation beyond JSON parse validity.
- Lower-level network instrumentation for separate TCP connect and TLS handshake timings. The current `reqwest` path exposes reliable header wait and download timings, but not Chrome-grade phase events.
Next implementation targets:

- Duplicate-preserving List mode storage with original-list-order export.
- Richer robots tester history and batch checks.

Reference research backlog:

The following backlog items come from reviewing public desktop SEO spider user-guide patterns, especially the Screaming Frog SEO Spider general user guide. These are functional product ideas only. Ferrous Frog remains an independent clean-room implementation and must not copy branding, assets, UI images, proprietary data, or proprietary code.

- Packaging and installation:
  - Produce signed Windows, macOS, and Linux release packages through Tauri.
  - Document minimum and recommended hardware for memory and database storage modes.
  - Add first-run checks for 64-bit OS, available memory, writable app data directory, and SQLite database location.
  - Add CLI-friendly launch documentation and later silent install or package-manager notes for enterprise deployment.
- Settings architecture:
  - Split settings into Crawl, Scope, Storage and Memory, Robots, Rendering, Extraction, Search, Auth, APIs, Scheduling, Export, and User Interface sections.
  - Add configuration profiles: save current as default, save as named profile, load profile, load recent profile, clear default, and reset to built-in defaults.
  - Add storage and memory settings with estimated crawl capacity, memory budget, database location, and warnings when a crawl is likely to exceed memory mode.
  - Resource-type toggles are in place for internal HTML, images, CSS, JavaScript, external URLs, and other files. Next expansion is richer asset-specific reporting.
  - Query string controls are in place for parameter limits, sorting, full stripping, and regex-based parameter stripping.
- Crawl mode and scope:
  - Spider/List mode switching is in place. Compare mode is still pending.
  - Add scope controls for subdomain, all subdomains, exact folder/subfolder, and explicit include/exclude regex.
  - Custom robots.txt override, download action, and single-URL tester are in place. Batch tester is still pending.
  - Basic pasted List mode is in place. Upload support, duplicate preservation, and original-list-order export are still pending.
- Saved crawl library:
  - Promote database mode from "current crawl" to named crawl sessions.
  - Add a Crawls manager with open, recent, rename, folder/project organization, duplicate, delete, bulk delete, export, and import.
  - Add crash recovery language and behavior for automatically committed database crawls.
  - Support portable crawl archives for sharing crawls between machines.
- UI customization:
  - Add tab visibility, close tab, close others, reset tabs, and configure tabs.
  - Add column visibility, reorder columns, configure columns, and reset columns for all tables.
  - Add resizable right and bottom panes.
  - Add focus mode that hides irrelevant tabs until their related configuration or data is present.
  - Add advanced search with plain text, regex, does-not-match, case sensitivity, selected columns, and/or rule groups.
- Export model:
  - Add exports for current view, selected filter, selected rows, all in-links, all out-links, redirect chains, headers, source text, image alt text, queued URLs, and issue reports.
  - Add export presets with output folder, timestamped folders, overwrite mode, CSV, XLSX, consolidated workbook tabs, and clipboard support.
  - Add database crawl export/import as a faster path than large spreadsheet exports.
- Scheduling and headless runs:
  - Add one-off and interval scheduled crawl tasks with task name, project, mode, URL/list source, configuration profile, auth profile, and export preset.
  - Add a headless run mode that disables interactive UI actions during scheduled execution.
  - Add completion notifications and optional email/webhook notification hooks.
  - Add per-schedule API selection for GA4, Search Console, PageSpeed, and backlink providers once integrations exist.
- Command line:
  - Add CLI commands for headless crawl, list mode, config profile selection, output folder, export preset, save/open crawl, and help output.
  - Allow CLI to reuse saved UI configuration profiles when an option is not exposed as a dedicated flag.
  - Keep credentials out of command histories where OAuth or API keys are involved.

## Phase 0: Foundation

Goal: create a buildable project skeleton with clear boundaries.

Deliverables:

- Cargo workspace.
- Tauri 2 application shell.
- React, TypeScript, and Vite frontend.
- Workspace crates:
  - `crawler-core`
  - `parser`
  - `analysis`
  - `extractors`
  - `storage`
  - `integrations`
  - `export`
- Basic tracing/logging.
- CI-ready formatting, linting, and test commands.
- Dependency versions verified against current stable releases before pinning.

Acceptance criteria:

- `cargo test` runs for the workspace.
- Frontend installs and builds.
- Tauri dev window opens.
- Engine crates compile without depending on UI crates.

## Phase 1: Make The Frog Crawl

Goal: ship a small but complete SEO crawler workflow.

Crawler:

- Spider mode from a seed URL.
- URL normalization and deduplication.
- Scope controls for depth limit and total URL limit.
- Bounded async worker pool.
- Per-host politeness and configurable rate limit.
- robots.txt support, respected by default.
- Configurable User-Agent.
- Request timeout and retry policy.
- Manual redirect handling with full redirect-chain recording.
- Broken-link detection.

Capture:

- Original URL and final URL.
- HTTP status code and status text.
- Content type.
- Response time.
- Crawl depth.
- Page title.
- Meta description.
- H1.
- Canonical URL.
- Indexability and indexability reason.
- In-link count.
- Out-link count.

Storage:

- In-memory store.
- Query windows for paginated, filtered, sorted result slices.
- Summary counters for live progress.

Analysis:

- Response-code groups.
- Missing, duplicate, too-short, and too-long titles.
- Missing, duplicate, too-short, and too-long meta descriptions.
- Broken links.

UI:

- Top toolbar with URL input, mode selector, start, pause, and stop.
- Left issue tree.
- Central virtualized grid.
- Bottom detail panel.
- Live counters for crawled, queued, failed, and crawl speed.
- CSV export for current view.

Acceptance criteria:

- A local fixture site crawl completes.
- Results stream into the UI during crawl.
- Pause and stop behave cleanly.
- Basic audit views update from stored results.
- CSV export streams from the store.
- Tests cover URL normalization, redirect chains, parsing, and audit rules.

## Phase 2: Scale And Reports

Goal: handle large and persistent crawls.

Deliverables:

- SQLite database mode.
- Resumable crawl state for the current database-backed crawl.
- Server-side filtering, sorting, and pagination pushed down into SQLite for most current views.
- Full audit-rule set for canonicals, directives, headings, images, hreflang, structured data, sitemap issues, links, security, mobile, AMP, pagination, and validation.
- XLSX export.
- Bulk exports for in-links, out-links, issues, and redirect chains.
- Resizable Overview dashboard with status, indexability, URL distribution, metadata, heading, canonical/directive, image, technical, and top-issue summaries.
- Remaining Overview work: click-through filtering, trend charts, saved layout presets, and richer crawl-speed history.

Acceptance criteria:

- Database mode can process large synthetic crawls without unbounded memory growth.
- A closed crawl can be reopened and resumed.
- Exports do not buffer the entire dataset in memory.

## Phase 3: Rendering And Extraction

Goal: support advanced crawling and custom analysis.

Deliverables:

- Optional JavaScript rendering through a headless Chromium/CDP integration.
- Raw HTML versus rendered DOM diff.
- Rendered link and content extraction.
- Custom extraction by CSS selector, XPath, and regex.
- Custom search across raw and/or rendered HTML.
- XML sitemap generation.
- Near-duplicate detection with configurable similarity threshold. Initial BLAKE3 and SimHash implementation is in place.
- robots.txt tester.

Acceptance criteria:

- Rendering is opt-in and visibly slower by design.
- Custom extractors create named result columns.
- Sitemap generation streams from stored crawl data.

## Phase 4: Integrations And Visualization

Goal: enrich crawl data and improve investigation workflows.

Deliverables:

- Google Search Console integration.
- Google Analytics 4 integration.
- PageSpeed Insights integration.
- Pluggable backlink API integration points.
- Crawl visualization graph backed by queryable nodes and edges. Initial graph storage, graph JSON export, and Sigma/Graphology graph UI are in place.
- Remaining graph work: CSV node/edge exports, broken-link edge shortcuts, redirect edge shortcuts, internal-only crawl paths, node details, and layout controls.
- Directory tree visualization.
- User-defined URL segments.
- Crawl comparison for added, removed, changed URLs and issue deltas.

Acceptance criteria:

- OAuth-backed integrations keep credentials out of source control.
- External metrics can be joined onto crawled URLs.
- Crawl comparison works across two stored crawls.

## Phase 5: Automation And AI Assist

Goal: automate recurring work and add optional AI-assisted analysis.

Deliverables:

- Configurable LLM provider support.
- User-managed API keys.
- Rate-limited prompts against page content.
- Content intent classification.
- Draft meta description generation.
- Thin-content and quality flags.
- Scheduled crawls.
- Auto-export workflows.
- Auth/login crawl flows.
- Spelling and grammar analysis.
- Extreme-scale performance hardening.

Acceptance criteria:

- AI features are optional and provider-configurable.
- Scheduling can run crawls and exports without manual UI interaction.
- Large-crawl benchmarks are documented.

## Definition Of Done

Each phase is complete only when:

- Tests pass.
- The app builds.
- A short demo scenario works.
- Documentation reflects current behavior.
- Known limitations are recorded.
