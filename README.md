# Ferrous Frog SEO Spider

Ferrous Frog is a clean-room desktop SEO crawler built with Rust and Tauri. It crawls websites like a search-engine bot, captures technical and on-page SEO signals, runs audit rules, and presents the results in a fast, filterable desktop UI.

Tagline: "It used to croak. Now it compiles."

## Status

This repository now contains the first implementation slice of the Phase 1 MVP:

- Cargo workspace with engine crates.
- Tauri 2 desktop shell.
- React, TypeScript, and Vite frontend.
- In-memory crawl result store.
- SQLite-backed database mode with resumable current-crawl storage.
- SQLite-backed named crawl sessions with per-session database files and reopen/delete controls.
- SQLite-backed named configuration profiles with save/load/delete controls.
- Async spider crawl loop with bounded concurrency, per-host rate limiting, robots.txt support, manual redirect recording, and live progress events.
- robots.txt `Crawl-delay` parsing with measured integration coverage.
- Include and exclude URL regex scope controls in crawler config and Settings UI.
- Resource-type crawl toggles for HTML, images, CSS, JavaScript, external URLs, and other files.
- Query-string controls for parameter sorting, full query stripping, regex-based parameter stripping, and maximum retained parameter count.
- Basic Spider/List mode selection with pasted URL lists.
- Custom robots.txt override support in crawler config and Settings UI.
- robots.txt tester command and Settings UI check for overridden robots rules.
- robots.txt download action to populate override text from the current root URL.
- Default same-origin `/sitemap.xml` ingestion for root crawls.
- Sitemap orphan reporting with `in_sitemap` storage, issue view filtering, Overview drill-down, and grid visibility.
- HTML parsing for titles, meta descriptions, H1/H2, canonicals, directives, images, mobile viewport, AMP, pagination, hreflang, JSON-LD, Open Graph, Twitter Cards, links, and image/stylesheet/script resources.
- HTTP security-header capture for HSTS, CSP, X-Frame-Options, and X-Content-Type-Options.
- Per-request network timing capture for DNS lookup, header wait/TTFB, body download, total network time, transfer rate, resolved IP count, and redirect-hop timing.
- Content fingerprinting with BLAKE3 response hashes, word counts, text-to-code ratio, and SimHash near-duplicate clusters.
- Crawl graph storage with queryable URL nodes and source-to-target link edges, including source position.
- Interactive crawl graph visualization with Sigma, Graphology, theme-aware styling, live updates during active crawls, filters, layout controls, selected-node details, and filtered JSON export.
- Dedicated link reports for all links, internal links, external links, broken or unresolved links, nofollow links, selected URL in-links/out-links, anchor-text aggregation, and redirect chains.
- Resizable Overview panel with clickable drill-down filters for status, URL distribution, metadata, headings, canonicals, images, technical checks, and issue signals.
- Overview crawl-speed history with a compact live trend chart.
- Issue views for response families, titles, meta descriptions, H1/H2, canonicals, directives, images, mobile, hreflang, structured data, security, near-duplicates, and broken links.
- CSV, XLSX, XML sitemap, graph JSON, HTML report, link edge CSV, and redirect-chain CSV export.
- Mock-site integration coverage for redirects, broken links, duplicate metadata, graph edges, robots blocking, invalid hreflang, invalid JSON-LD, and redirect loops.
- Custom extraction through CSS text, CSS attributes, XPath, and regex, wired into crawl config, stored URL details, exports, and dynamic grid columns.
- Explicit light/dark theme toggle using Etiya design tokens.

See `ROADMAP.md` for the phased implementation plan.

## Goals

- Crawl websites concurrently while staying polite and configurable.
- Respect robots.txt by default.
- Stream live crawl progress to the desktop UI.
- Keep the engine decoupled from the UI.
- Keep the frontend from owning the full crawl dataset.
- Support very large crawls through disk-backed storage in later phases.
- Export crawl data and audit reports.

## Clean-Room Notice

Ferrous Frog does not use the name, logo, branding, proprietary assets, or proprietary code of Screaming Frog SEO Spider or any other commercial crawler. It is an independent implementation of general SEO crawler functionality.

## Planned Architecture

```text
.
|-- crates/
|   |-- crawler-core/    # Frontier, scheduler, fetcher, politeness, redirects
|   |-- parser/          # HTML to structured page signals
|   |-- analysis/        # Rule-based SEO issue engine
|   |-- extractors/      # CSS, XPath, and regex extraction
|   |-- storage/         # Memory and SQLite storage backends
|   |-- integrations/    # GSC, GA4, PageSpeed, AI providers, backlink APIs
|   `-- export/          # CSV, XLSX, sitemap, and report writers
|-- src-tauri/           # Tauri commands, events, and desktop packaging
`-- src/                 # React + TypeScript frontend
```

The engine owns crawl data. The frontend requests paginated, filtered, sorted windows through Tauri commands and receives progress summaries through events/channels.

## MVP Scope

Phase 1 focuses on a shippable crawler:

- Spider mode from a seed URL.
- Bounded concurrency and rate limiting.
- Configurable per-host requests-per-second limit.
- robots.txt support.
- Configurable User-Agent.
- Manual redirect recording.
- Broken-link detection.
- In-memory storage.
- SQLite database storage mode.
- Resume option for the current database crawl.
- Live UI updates.
- Virtualized results grid.
- Light and dark themes.
- Basic audit views.
- CSV export.
- XLSX export.
- XML sitemap export.
- Graph JSON export.
- HTML report export.
- Link edge and redirect-chain CSV exports.
- Link report modal and crawl graph modal.

Captured per URL:

- Original URL and final URL.
- Status code and status text.
- Content type.
- Response time.
- Network timings: DNS lookup, TTFB/header wait, download time, total network time, transfer rate, and resolved IP count.
- Response hash.
- Word count.
- Text-to-code ratio.
- SimHash near-duplicate cluster.
- Crawl depth.
- Title.
- Meta description.
- H1.
- H2.
- Canonical.
- Indexability and reason.
- Meta robots and X-Robots-Tag.
- Image counts, missing alt counts, and long alt counts.
- Mixed-content references and insecure forms.
- Security-header presence.
- Mobile viewport, AMP, pagination, hreflang, JSON-LD, and social metadata counts.
- In-link and out-link counts.
- Source-to-target link edges for crawl graph snapshots.

## Target Stack

Rust engine:

- `tokio`
- `reqwest` with Rustls TLS, compression, cookie support, and manual redirects
- `scraper` or `tl`
- `url`
- `serde`
- `tracing`
- `thiserror` and `anyhow`
- `governor` or equivalent rate limiting
- `blake3` for response hashes
- `simhash` for near-duplicate detection
- SQLite through `rusqlite` or `sqlx` in later phases

Desktop and frontend:

- Tauri 2
- React
- TypeScript
- Vite
- Zustand or equivalent state management
- AG Grid Community or TanStack Table with TanStack Virtual
- Recharts or equivalent charting library
- Sigma and Graphology for WebGL crawl graph visualization

Dependency versions should be verified against current stable releases before they are pinned.

## Development Workflow

The intended implementation order is:

1. Scaffold the Cargo workspace and Tauri app.
2. Implement and test `crawler-core` headlessly.
3. Add parser fixtures and audit-rule tests.
4. Wire engine commands and live crawl events into Tauri.
5. Build the virtualized grid and audit views.
6. Add streaming CSV export.
7. Expand phase by phase.

Common commands:

```bash
cargo test --workspace
npm install
npm run dev
npm run tauri:dev
npm run build
```

Use `npm run tauri:dev` for the desktop app. Opening the Vite URL directly in a browser is useful for layout work, but crawl commands require the Tauri runtime.

## Crawl Etiquette

Ferrous Frog should default to safe behavior:

- Respect robots.txt.
- Use conservative rate limits.
- Bound concurrency.
- Use request timeouts.
- Identify itself with a clear User-Agent.
- Make "ignore robots.txt" an explicit opt-in.

## Testing Plan

Planned tests include:

- Unit tests for URL normalization.
- Unit tests for robots.txt handling.
- Redirect-chain and redirect-loop tests.
- Parser fixture tests for titles, meta descriptions, headings, canonicals, links, and directives.
- Audit-rule tests.
- Local mock-site integration tests with planted SEO issues.
- Stress tests for frontier and dedup behavior.

## Roadmap

- Phase 1: Working spider MVP with live grid and CSV export.
- Phase 2: SQLite mode, resumable crawls, full audits, XLSX, and dashboard.
- Phase 3: JavaScript rendering, custom extraction, custom search, sitemaps, and near-duplicates.
- Phase 4: GSC, GA4, PageSpeed, graph visualizations, segments, and crawl comparison.
- Phase 5: AI assist, scheduling, login flows, spelling/grammar, and extreme-scale hardening.

See `ROADMAP.md` for details.
