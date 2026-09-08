# SEO Crawler Feature Comparison

Reviewed on 2026-09-08 against the working source, Rust tests, a production frontend build and the real React screen exercised with Tauri IPC fixtures. This is a capability comparison, not a claim of feature parity or a comparison of measured crawl speed.

The reference product publishes a broad [feature overview](https://www.screamingfrog.co.uk/seo-spider/) and documents category tabs, filters, lower-window URL/link details and overview panels in its [user guide](https://www.screamingfrog.co.uk/seo-spider/user-guide/). Ferrous Frog already implements much of the basic crawler pipeline. The largest immediate gaps were unreliable audit classification and incomplete access to existing results.

| Area | Screaming Frog SEO Spider | Ferrous Frog after this repair |
| --- | --- | --- |
| Spider and List crawls | Both modes, configurable discovery | Both modes; pasted URLs, files and sitemap lists. Rust preserves duplicate List rows; file import currently deduplicates them. |
| Politeness | Robots and configurable crawl speed | Robots on by default, per-origin rules, shared request spacing, retries, pause/stop. Rules now apply to redirect targets and sitemap HTTP requests. |
| Response codes and redirects | Errors, chains and loops | Status families, failures, source links and full followed chains. A stored row combines the requested URL with its final response; intermediate hops are not separate result rows. |
| Titles, descriptions and headings | Missing, duplicate, size and multiple-tag checks | Missing/duplicate/length filters, heading counts and estimated pixel widths. Failed responses and non-HTML resources no longer generate missing metadata issues. Multiple title/description detection is still absent. |
| Indexability and canonicals | Meta/header directives, canonicals and validation | Generic noindex/none, repeated robots tags/headers, HTTP status precedence, canonical presence/count and canonicalized status. Bot-specific precedence, HTTP Link canonicals and canonical-chain audits remain incomplete. |
| Results workspace | Category/filter tables and contextual details | Grouped left audit views, relevant/all columns, virtualized 500-row pages, server-side search/sort, URL details and direct inlink/outlink access. Previously only the first 1,000 URL rows were reachable. |
| Link diagnostics | Inlinks, outlinks, anchor text and bulk reports | Stored edges with sources, anchors, positions, nofollow, status and paths. Link/anchor/redirect/sitemap reports now paginate beyond 500 rows. Unknown and robots-blocked targets are separated from known failures. |
| Images and resources | Image/resource crawling and audits | Optional image/CSS/JS/external crawling, alt checks, image references and dimensions. Resource crawling is disabled by default except HTML; full responsive-image and background-image discovery is incomplete. |
| Sitemaps | XML and image sitemap generation and checks | XML export, root `/sitemap.xml`, sitemap-index inputs, membership/orphan/validation reports. Advanced image sitemap generation and complete robots-declared sitemap discovery remain gaps. |
| Custom data | CSS, XPath, regex and source search | CSS text/attribute, XPath and regex extraction; raw/rendered custom search; queryable dynamic columns and export. XPath still expects XML-compatible markup. |
| Duplicate content | Exact and near duplicate analysis | Response hashes and SimHash clusters exist. No dedicated exact-duplicate content view; similarity clustering is a heuristic and not scale-certified. |
| JavaScript sites | Integrated Chromium rendering and diagnostics | Optional Chrome CDP backend and raw/rendered differences. Standard builds exclude this feature; it requires Chrome and `--features js-rendering`. Browser navigation/subresources do not yet share all Rust robots/rate controls. |
| Technical audits | International, structured data, mobile and validation | Hreflang syntax/return/canonical checks, basic JSON-LD/schema checks, viewport, mixed content and headers. Full structured-data validation, AMP/pagination audits and accessibility checks are incomplete or absent. |
| Storage and comparison | Saved crawls, comparison and large-site modes | Memory/SQLite stores, sessions, profiles, persisted frontier, archives, segments and comparison. Duplicate/regex SQLite queries still materialize records; a million-URL crawl benchmark remains unverified. |
| Google and other integrations | Analytics, Search Console, PageSpeed and link data | Search Console access-token storage and metric merge exist. OAuth refresh/login is incomplete. PageSpeed is a provider scaffold; GA4, complete CWV persistence and backlink providers are missing. |
| Export and automation | Multiple reports, scheduling and CLI workflows | CSV, XLSX, XML, HTML and graph/link exports exist. Major text exports stream; XLSX/HTML/archives still buffer. Scheduling, a production crawl CLI, authenticated login flows, AI and spelling/grammar workflows are absent. |

## Repairs made

- The UI keeps one bounded result page, refreshes storage queries during a crawl and rejects stale responses. Duplicate and cross-page filters now use the same engine logic during and after crawling.
- Audit categories are navigable from a grouped sidebar or a compact selector. Relevant columns place the current evidence near the URL; all existing columns remain available.
- Link reports have the same page controls as the main grid. Original URLs are visible separately from final URLs, including links into redirects.
- Robots decisions use the target origin, including scheme and port. Concurrent requests share pacing, and unavailable robots policies fail conservatively. Known exclusions do not inflate broken-link counts.
- On-page audit eligibility and known network failures share predicates across typed analysis, memory storage, SQLite summaries/queries, the tree and HTML reports.
- HTML parsing cannot make an HTTP error indexable. Generic directives apply to non-HTML responses too, and page-level nofollow uses the configured link-following choice.
- Heavy row queries run off the desktop UI thread. Fatal crawl errors restore the Start control.
- Opening, deleting or importing a crawl clears the previous URL selection, page and progress; session switching is disabled during an active crawl.
- The UX follow-up adds direct Settings access, Enter-to-start, filter recovery, pinned URL cells on wide grids and keyboard-accessible detail sections. Dialog actions display feedback inside the active dialog; an empty filter no longer disables whole-crawl exports. Secondary text and error colors are clearer in both themes, and duplicate overview counters were removed.
- Appearance offers System, Light and Dark, defaulting to the system on first launch and following live OS changes. Explicit preferences are remembered and applied before the app loads. The dark palette uses neutral near-black surfaces; graph colors follow the same tokens and update while open.
- The compact workbench puts category navigation above 28-pixel virtual rows, preserves a bottom URL inspector and moves progress into the status bar. Inlinks/outlinks stay in the workspace with server-side paging, search, sorting, selection cancellation and retry. Right-side Overview/Issues tabs provide summary drill-down; issue counts cover available summary checks and can overlap. The full audit tree and report dialogs remain available.

## Verification and remaining priorities

Verification passed: 118 Rust tests (one synthetic benchmark ignored), Rust formatting, the production frontend build and headless UI checks. The optional rendering path was also checked with `cargo check -p ferrous-frog-app --features js-rendering` during the crawler repair. The frontend retains its existing warning about a bundle exceeding 500 kB.

Run `make verify`, `make test-ui` and `make check-js-rendering`. The UI check launches a temporary Vite server and headless Chrome, injects the installed Tauri IPC mock, and exercises the real screen. It verifies access past 1,000 URLs and 500 links, filter resets, stale-response ordering, live duplicate views, fatal errors, details, graph exclusions, themes, small-screen layout and selection cleanup when switching databases. It does not replace native desktop end-to-end testing or a large-site benchmark.

Prioritize rendering reliability and capability detection, complete URL/redirect identity reporting, indexability/canonical edge cases, and SQLite query/frontier performance before adding more provider integrations. The pre-existing broad roadmap remains useful, but a feature appearing in Settings is not evidence of complete implementation.
