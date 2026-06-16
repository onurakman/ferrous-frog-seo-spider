# AGENTS.md

Guidance for AI coding agents working on Ferrous Frog.

## Project

Ferrous Frog is a clean-room, cross-platform desktop SEO crawler inspired by the broad category of technical SEO spider tools. It must not use the Screaming Frog name, logo, branding, proprietary assets, or proprietary code.

Primary stack:

- Rust engine in a Cargo workspace.
- Tauri 2 desktop shell.
- React, TypeScript, and Vite frontend.
- Virtualized grid UI with server-side paging, sorting, and filtering.
- In-memory storage for quick crawls and SQLite-backed storage for larger/resumable current crawls.

## Non-Negotiables

- Keep the crawler polite by default: respect robots.txt, use sane rate limits, and send a clear User-Agent.
- Keep the engine independent from the UI. UI code must not own the full crawl dataset.
- Stream progress to the UI. Query result windows through commands.
- Avoid cloning large HTML bodies across pipeline stages.
- Keep the app responsive during active crawls.
- Prefer small, reviewable increments with tests.

## Repository Shape

Target workspace layout:

```text
.
|-- crates/
|   |-- crawler-core/
|   |-- parser/
|   |-- analysis/
|   |-- extractors/
|   |-- storage/
|   |-- integrations/
|   `-- export/
|-- src-tauri/
|-- src/
|-- AGENTS.md
|-- ROADMAP.md
`-- README.md
```

Do not introduce UI dependencies into engine crates.

## Work Order

1. Scaffold the Cargo workspace and Tauri 2 app.
2. Build and test `crawler-core` headlessly before wiring UI.
3. Add parser and analysis crates with focused fixture tests.
4. Connect engine to Tauri commands and progress events/channels.
5. Build the virtualized results grid and basic audit views.
6. Add CSV export.
7. Only then expand into SQLite, richer audits, rendering, integrations, and AI features.

## Phase 1 MVP

Phase 1 must ship end to end:

- Spider mode from one seed URL.
- Bounded concurrency and basic rate limiting.
- robots.txt support, enabled by default.
- Manual redirect recording.
- Broken-link detection.
- In-memory storage.
- Live progress updates.
- Virtualized results grid.
- Capture status, content type, title, meta description, H1, canonical, indexability, response time, depth, in-link count, and out-link count.
- Basic audit views for response codes, title issues, meta description issues, and broken links.
- CSV export.
- SQLite mode, XLSX export, XML sitemap export, custom extraction, near-duplicate clustering, and expanded audit views are already partially implemented. Preserve those paths when changing crawler, storage, export, or UI code.

## Engineering Rules

- Verify current stable versions before pinning Tauri, Rust crates, and frontend packages.
- Use `tokio`, `reqwest` with Rustls TLS, manual redirect policy, `scraper` or `tl`, `url`, `serde`, `tracing`, and `thiserror`/`anyhow` where appropriate.
- Use `governor` or an equivalent robust rate limiter for politeness.
- Use cancellation tokens for pause, resume, and stop.
- Use bounded channels for backpressure.
- Keep storage behind a trait. Memory and SQLite modes must expose the same query model.
- Push filtering, sorting, and pagination into storage/engine commands.
- Keep audit rules typed and extensible. A rule should emit structured issues with severity.
- Do not store raw HTML unless a feature explicitly requires it.

## Testing Expectations

Add tests with the feature they prove:

- URL normalization tests.
- robots.txt behavior tests.
- redirect-chain and loop tests.
- parser fixture tests for title, meta, headings, canonical, links, and directives.
- audit-rule tests with clear positive and negative cases.
- integration test using a local mock site with planted issues.
- property or stress tests for frontier and dedup behavior when practical.

## Frontend Expectations

- Build the actual app screen first, not a marketing landing page.
- Use a top crawl toolbar, left issue tree, central virtualized grid, and bottom detail panel.
- Keep the grid virtualized and backed by server-side queries.
- Include dark/light theme support.
- Ensure controls remain usable on small screens.
- Avoid decorative-only UI that slows repeated operational use.

## Safety And Etiquette

- Never default to ignoring robots.txt.
- Make aggressive crawl settings explicit user choices.
- Keep a clear default User-Agent, for example `FerrousFrogSeoSpider/0.1 (+https://example.invalid/ferrous-frog)`.
- Do not add trademarked names, logos, or assets from competing products.
- Do not commit API keys, crawl databases, generated exports, or user crawl data.

## Documentation

When changing behavior, update the relevant docs:

- `README.md` for setup, commands, and user-facing capabilities.
- `ROADMAP.md` for phase status and scope changes.
- This file for agent workflow, architecture, and contribution constraints.

## Language Policy

All repository artifacts must be written in English, even when the user writes in another language. This includes source code, comments, documentation, configuration, commit messages, test names, fixture text when practical, and generated project files.
