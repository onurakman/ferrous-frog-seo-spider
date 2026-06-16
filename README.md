# Ferrous Frog SEO Spider

Ferrous Frog is a clean-room desktop SEO crawler built with Rust and Tauri. It crawls websites like a search-engine bot, captures technical and on-page SEO signals, runs audit rules, and presents the results in a fast, filterable desktop UI.

Tagline: "It used to croak. Now it compiles."

## Status

This repository now contains the first implementation slice of the Phase 1 MVP:

- Cargo workspace with engine crates.
- Tauri 2 desktop shell.
- React, TypeScript, and Vite frontend.
- In-memory crawl result store.
- Async spider crawl loop with bounded concurrency, robots.txt support, manual redirect recording, and live progress events.
- HTML parsing for title, meta description, H1, canonical, indexability, and links.
- Basic issue views and CSV export.

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
- robots.txt support.
- Configurable User-Agent.
- Manual redirect recording.
- Broken-link detection.
- In-memory storage.
- Live UI updates.
- Virtualized results grid.
- Basic audit views.
- CSV export.

Captured per URL:

- Original URL and final URL.
- Status code and status text.
- Content type.
- Response time.
- Crawl depth.
- Title.
- Meta description.
- H1.
- Canonical.
- Indexability and reason.
- In-link and out-link counts.

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
- SQLite through `rusqlite` or `sqlx` in later phases

Desktop and frontend:

- Tauri 2
- React
- TypeScript
- Vite
- Zustand or equivalent state management
- AG Grid Community or TanStack Table with TanStack Virtual
- Recharts or equivalent charting library

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
