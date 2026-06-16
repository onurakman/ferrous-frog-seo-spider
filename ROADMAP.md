# ROADMAP.md

Ferrous Frog development roadmap.

## Current Status

The project has a working first implementation slice of Phase 1. The workspace, Tauri shell, React frontend, in-memory store, parser, basic analysis crate, CSV export crate, and async crawler core are in place.

Implemented:

- Cargo workspace and Tauri 2 application shell.
- React, TypeScript, Vite frontend.
- Async spider mode from one seed URL.
- Bounded concurrency.
- robots.txt support, respected by default.
- Manual redirect recording.
- Basic broken-link detection through failed responses and 4xx/5xx statuses.
- In-memory result storage.
- Server-side query windows for filtering, sorting, searching, and pagination.
- Live crawl progress events.
- Virtualized result grid.
- Basic issue views for response families, titles, meta descriptions, and broken links.
- CSV export for the current queried view.

Remaining Phase 1 hardening:

- Per-host rate limiting instead of a simple per-request delay.
- More complete robots.txt crawl-delay handling.
- Local mock-site integration test.
- Redirect loop fixture coverage.
- More robust external link recording and broken external link reporting.
- Better UI coverage for row details and issue counts.

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
- Resumable crawl state.
- Server-side filtering, sorting, and pagination pushed down into SQLite.
- Full audit-rule set for canonicals, directives, headings, images, hreflang, structured data, sitemap issues, links, security, mobile, AMP, pagination, and validation.
- XLSX export.
- Bulk exports for in-links, out-links, issues, and redirect chains.
- Overview dashboard with status, indexability, and top-issue charts.

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
- Near-duplicate detection with configurable similarity threshold.
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
- Crawl visualization graph.
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
