# Crawl Workflow Repair Plan

**Goal:** Make the existing crawler reliable for daily SEO audits and document its actual feature coverage.

**Architecture:** Keep Rust storage authoritative for result filtering, sorting and paging. Reuse the existing React grid, Tauri commands, parser and storage backends. Preserve the current uncommitted implementation and independent branding.

**Constraints:** English repository artifacts, no new dependencies, robots respected by default, bounded UI data, memory/SQLite query parity, existing export/rendering/integration paths preserved.

- [x] Repair result access in `src/App.tsx`: fetch bounded pages beyond the first 1,000 URLs, reset paging on filter changes, refresh server queries during crawling, reject stale responses. Remove duplicated client-side audit predicates.
- [x] Organize existing audit views into a left issue tree with relevant columns, compact controls, useful empty states and accessible row selection. Keep all existing columns and report actions available.
- [x] Repair robots policy selection and request timing in `crates/crawler-core/src/lib.rs`, including cross-origin seeds, redirect targets and sitemap requests. Prove blocked targets are not fetched using local integration tests.
- [x] Align typed audit rules, memory queries, SQLite queries and summaries. Do not report missing page metadata on failed or non-HTML responses, or robots exclusions as network failures.
- [x] Verify HTTP errors cannot become indexable after HTML parsing; cover applicable robots directives with focused regressions.
- [x] Record comparison evidence, implemented fixes and remaining feature gaps in `docs/FEATURE_COMPARISON.md`; update `README.md` and `ROADMAP.md` to reflect verified scope.
- [x] Validate with `cargo test --workspace`, `cargo fmt --all -- --check`, `npm run build`, optional rendering compilation and a headless browser smoke test covering paging, stale requests, live duplicate views, selection and small-screen controls.

Baseline: 93 Rust tests passed, one synthetic benchmark ignored; frontend build passed with its existing bundle-size warning. The initial UI always requested `offset: 0, limit: 1000`, independently reimplemented audit rules and did not update duplicate filters during a crawl.

Result: 117 Rust tests passed, one benchmark ignored. Frontend build, headless UI checks, formatting and desktop compilation with `js-rendering` passed. Review also found and verified fixes for cached robots delays on policy redirects, broken-edge classification, small-screen overview dismissal and selection cleanup on dataset replacement. Native GUI end-to-end crawling and million-URL performance remain unverified.
