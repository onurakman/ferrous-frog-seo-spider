# Ferrous Frog SEO Spider

*It used to croak. Now it compiles.*

![Ferrous Frog workspace with dark and light themes joined along a diagonal cut](docs/images/workspace-themes.png)

<p align="center"><sub>Dark and light themes · Sample crawl data · <a href="docs/images/workspace-dark.png">Dark screenshot</a> · <a href="docs/images/workspace-light.png">Light screenshot</a></sub></p>

Ferrous Frog is a clean-room desktop SEO crawler built with Rust and Tauri. It crawls websites like a search-engine bot, captures technical and on-page SEO signals, runs audit rules, and presents the results in a fast, filterable desktop UI.

## Status

This repository contains a working crawler with several partially implemented advanced features. See the [verified feature comparison and remaining gaps](docs/FEATURE_COMPARISON.md) before treating a roadmap item as full feature parity.

Recent workflow and correctness repairs:

- Direct Settings access and keyboard crawl submission, recoverable empty filters, readable secondary text in both themes, and a compact layout down to the minimum desktop window size.
- Compact workbench with category tabs, 28-pixel virtualized rows, an optional audit tree, a fixed bottom URL inspector and right-side Overview/Issues tabs. Progress lives in the status bar.
- URL columns stay visible during horizontal scrolling in wide grids. The inspector has keyboard-accessible URL details, Inlinks, Outlinks, Links & indexing, Technical and Custom data tabs.
- Dismissible errors and notifications appear inside the active dialog. Whole-crawl exports remain available when the current filter has no results.
- Categories initially show all URLs with relevant columns; their filter dropdown narrows to a specific check. The complete audit tree and all-columns option remain available.
- **Configure columns** hides individual columns, changes their order with keyboard-accessible buttons, and saves named layouts across restarts. The URL column stays available. Relevant columns follow the active audit; custom and saved layouts remain fixed. Delete a layout from the same dialog.
- Inline inlinks/outlinks use bounded server-side pages, search and sorting. Selection changes reset paging and discard old requests; live refresh waits for each request to finish, and failed queries can be retried. Full link, anchor-text, redirect and sitemap-validation reports remain available.
- The Issues inspector lists checks with positive summary counts and opens their result filters. Counts can overlap and cover the available crawl summary, not every engine audit.
- Live duplicate filters use storage queries; stale query responses cannot overwrite newer filters. Live directory-tree refresh waits for each query to finish, including responses slower than the refresh interval.
- **Advanced filters** combines up to 20 conditions using **All** or **Any**, with text checks and numeric comparisons for status, depth, words and response time. It applies alongside the audit, search and segment to the grid, directory tree and filtered CSV/XLSX/sitemap exports. Text comparisons ignore case and normalize whitespace; missing status codes do not match numeric comparisons. Apply validates a draft; Cancel discards it, including pending validation. Filters reset when switching crawl datasets or clearing filters; they are not saved with crawl configuration.
- Switching crawl datasets clears old URL details and progress; fatal crawl errors restore the Start control.
- Per-origin robots rules and shared request spacing cover redirects and sitemap HTTP requests.
- Failed pages, incomplete downloads and non-HTML resources do not inflate missing metadata audits; robots exclusions are separate from broken requests in results and reports.
- **Multiple Titles / Multiple Descriptions** finds successful HTML pages with more than one title or description tag. Sortable tag counts appear beside the retained first value and in URL details, CSV/XLSX and audit reports. Empty tags count; SVG titles and template content do not. Old crawls have unknown counts until recrawled, shown as empty cells rather than measured zero.
- Page titles, descriptions, robots directives, viewport and heading values come from active HTML elements. Inert template metadata and SVG labels cannot override the page title or prevent real page links from being crawled. Existing saved results require recrawling to use the corrected extraction.
- HTTP errors retain non-indexable status, and generic robots directives apply to repeated meta/header fields and non-HTML responses.
- HTTP `Link` canonicals are captured on HTML and documents such as PDFs, including repeated headers and relative targets. HTML declarations take display precedence and contribute to the combined canonical count.
- Canonical views flag uncrawled same-host targets, redirects, response errors, non-indexable targets, chains and loops. Successful HTML source rows retain original/final redirect identity and separate List occurrences. Self-canonicals are valid; loop counts also include paths entering a loop. Unknown cross-host/subdomain targets remain unclassified because storage does not own crawl scope. SQLite builds a cached compact graph from URL metadata and keeps filtered result paging in SQL; frequent progress events do not rebuild it.
- **Pagination > Next URL to Error / Previous URL to Error** lists complete, successful HTML source rows whose captured `rel="next"` or `rel="prev"` target has a known HTTP or connection failure. The grid exposes sortable target columns and separate source counts, including List occurrences. Pending, uncrawled and robots-blocked targets are not failures; successful redirects retain their route evidence. SQLite shares the canonical alias cache and returns bounded result pages.
- **Next URL Loops / Previous URL Loops** checks each direction independently, including self-links and paths entering a cycle. Ordinary next/previous links between adjacent pages are valid. Only captured relations between known, complete successful HTML pages provide loop evidence; unknown targets end the path.
- **Next URL Non-Reciprocal / Previous URL Non-Reciprocal** warns when the captured target lacks the opposite relation or that relation is observed to return to a different page. Checks require complete successful HTML evidence, retain redirect aliases and List occurrences, and leave unresolved return destinations unclassified. These advisory checks use the first captured `<link>` for each direction, including the legacy `previous` synonym for `prev`. They do not prove sequence completeness or a search-ranking requirement; documents may belong to [multiple sequences](https://html.spec.whatwg.org/multipage/links.html#sequential-link-types).
- **AMP > AMP URL to Error** applies the same observed-target checks to captured `rel="amphtml"` URLs, with a sortable target column and source counts. This does not validate AMP markup or canonical reciprocity, and an uncrawled target is not reported as broken.
- SQLite duplicate, URL-regex and cross-record hreflang filters query bounded row windows directly. Return-link and canonical-target audits join URL aliases inside SQLite, preserve redirect/List evidence and avoid expanding shared redirect aliases into duplicate matches. Summaries and canonical graphs use a persisted record revision, so queue/seen-set writes no longer invalidate them. Record updates, including other database connections, still invalidate cached evidence. [Storage measurements](docs/BENCHMARKS.md) document the improvement and remaining scale limits.
- SQLite search uses Unicode lowercasing and matches literal `_`/`%` characters, custom values and first-inlink fields. Default List order and custom extraction sorting match the headless memory backend.

Existing capabilities:

- Cargo workspace with engine crates.
- Tauri 2 desktop shell.
- Original graphite-and-mint frog icon across desktop installers, the workspace toolbar, splash screen and browser tab. Icon source and generation notes live in [src-tauri/icons/README.md](src-tauri/icons/README.md).
- A background GitHub release check after startup, plus **More > Check for updates**. New stable releases show the installed and available versions. **Download update** opens the release notes and installers in your browser; **Remind me later** postpones automatic checks for 24 hours across restarts. Offline checks stay quiet at startup, while manual checks show errors and offer a retry.
- A 520×340 splash window pairs the original frog with an animated crawl network and progress indicator, follows the saved System/Light/Dark appearance and respects reduced motion. It stays visible for at least 2.2 seconds before opening the ready crawl library; slower startup adds no extra delay. Failed history loads show a retry action; a 12-second fallback reveals the main window if startup never signals readiness.
- The main window opens centered on first launch, then remembers its position, normal size and maximized state through Tauri's [window-state plugin](https://v2.tauri.app/plugin/window-state/). Multiple monitors and negative screen coordinates are supported; unavailable displays or an unreachable title bar fall back to a visible centered window. Splash visibility stays under startup control. Native window-manager verification on Windows/macOS remains part of the platform checklist.
- Production builds suppress the browser context menu in the workspace and splash window. Development builds retain it for inspection.
- More > Quit and native window/app exit requests use a Yes/No confirmation. No or Escape cancels; Yes waits for crawl cancellation and final frontier saving before exiting. Desktop crawl results are saved automatically, including when the current filter is empty.
- React, TypeScript, and Vite frontend.
- A centered launcher opens first: enter a website URL, select Spider or List and scope, and start a crawl. **Saved crawls** slides up from the bottom when opened and remembers its open/closed preference; it starts closed on a fresh installation. Compact cards show the site, date, mode, status and URL count. Search the panel or open a card to enter the existing workbench. Escape closes the panel and restores focus to its toggle. The brand button returns to the library, and an active crawl can continue while its results are out of view.
- Desktop crawls use SQLite automatically. Every new run gets a separate session database, retaining earlier results without a manual save. Existing memory preferences switch to SQLite; the in-memory engine backend remains available for headless use and tests. The library loads session metadata rather than full crawl records.
- Queued URLs, the seen set and scheduler progress remain recoverable. Opening a saved crawl restores its configuration when available and its results without starting requests; **Settings > Storage > Resume database** is available when the selected crawl has queued work. New starts from the library always create a separate crawl.
- Existing named sessions and the previous current database remain accessible. Session configuration, status and counts persist in the index; missing database files produce an error instead of silently opening an empty crawl. Each card has a top-right Delete action with confirmation; deleting a different saved crawl preserves the current workbench. Settings uses the same deletion confirmation and retains the custom database controls.
- Select two saved cards and choose **Compare crawls** to compare them directly, with the older crawl as the baseline. Native SQLite comparison preserves the active workspace, returns complete aggregate counts and shows up to 1,000 changed URL rows. Archive comparison remains available from the workbench Mode menu.
- SQLite-backed named configuration profiles with save/load/delete controls.
- **Apply / Cancel / OK** for configuration drafts, including loaded profiles, resource/query/rendering options and storage preferences. Apply validates engine rules and saves locally; OK also closes the dialog. Cancel, Escape and the close button discard pending edits, including pending validation. A failed save preserves the previous active configuration and lets you retry the draft. Reopening restores the last applied settings; malformed snapshots fall back to safe defaults. Crawl results and credentials are excluded from this snapshot. Explicit session/archive, profile-save and credential actions run when clicked. Workspace changes require a clean draft and lock editing and Start until complete; the latest requested profile wins, and cancelled draft requests cannot publish late results or errors.
- A toolbar **Mode** menu for Spider/List, crawl comparison and SERP preview. Spider and List remember separate start URLs across mode changes and app restarts. Mode changes and comparison are locked during active crawls; scope and crawl configuration stay readable until you stop.
- **SERP preview** edits local URL/title/description drafts without changing crawl results or making search-provider requests. Use the selected URL, create a draft, or import CSV with `url` (or `address`), `title`, and `description` (or `meta_description`) headers. Import replaces the current drafts only after all rows validate; it supports up to 1,000 rows / 5 MiB and 20,000 bytes per field. Desktop/mobile previews share the crawler's estimated pixel widths. Export CSV to the normal exports directory to keep drafts after quitting; formula-like cells receive a leading apostrophe for spreadsheet safety. Previews are illustrative, not search-engine ranking or display predictions.
- Searchable Settings navigation groups the current controls under Spider, Analysis and Workspace, with indented children, keyboard-toggleable headers and a fixed search field. Groups start collapsed when opening Settings; search expands matches and clearing it collapses the groups again. Search by section or control keywords such as User-Agent, XPath or Chrome. Narrow windows use a grouped section picker, and changing sections resets the form scroll position. Invalid numeric input reveals only its containing group.
- Crawl settings expose request timeout and redirect limits alongside the existing concurrency, rate, retry and crawl limits; User-Agent lives under **HTTP headers**. Invalid scope/query regex, extraction/search rules and non-HTTP URL sources are rejected before applying configuration or replacing results for a new crawl. Profiles can omit a target; starting requires a Spider seed or List sources, checked in both the engine and desktop command before changing the current workspace.
- Async spider crawl loop with bounded concurrency, per-host rate limiting, robots.txt support, retry/backoff, manual redirect recording, and live progress events.
- Rust page, robots and sitemap requests honor `Retry-After` on HTTP 429/503, accepting seconds or HTTP dates and sharing the longest observed delay across the origin. Existing retry attempts/backoff still apply; exhausted attempts retain the final response. Server waits are independent of the request timeout and remain pausable/cancellable, with a visible notice. Browser requests obey delays observed by Rust HTTP, but browser response headers and the standalone robots download tool do not supply cooldowns. Pause/Resume preserves cooldowns; Stop/restart creates a fresh request policy.
- Default speed is 8 concurrent requests, a 10 requests/second limit per host, and 100 ms minimum spacing per origin. robots.txt and longer site-declared crawl delays still apply. Saved speed choices remain unchanged; use **Settings > Crawl > Default preset**, then **Apply**, to adopt the new defaults.
- Stop cancels active worker requests without waiting for the request timeout.
- Pause/Resume stops new scheduler work and blocks worker request boundaries while allowing already in-flight HTTP requests to finish.
- Default crawl URL cap set to 5,000 URLs, adjustable in Settings.
- **Settings > Crawl > Max response MiB** caps each Rust HTTP response at 20 MiB by default, including pages, resources, robots.txt and sitemaps. The limit is checked while reading decoded bytes, so compressed and chunked responses cannot bypass it. Incomplete responses retain their HTTP status and show an error without being parsed for on-page audits. The setting accepts positive byte limits up to 1 GiB; Chromium's own network downloads are outside this limit.
- robots.txt `Crawl-delay` parsing with measured integration coverage.
- Include and exclude URL regex scope controls in crawler config and Settings UI.
- Persisted toolbar scope presets synchronized with Settings: **Current host**, **Start folder**, **All subdomains**, and **Exact URL**. All subdomains uses a bundled Public Suffix List, including private suffixes (for example, it keeps separate `github.io` projects apart); IP, local and unknown-suffix hosts remain on the exact host. **Host + subdomains** preserves existing profiles, including their leading `www.` handling, and advanced folder combinations appear as **Custom scope**.
- Exact URL crawls only the seed and its redirects, retains discovered links, and skips sitemap discovery. Robots checks still apply; optional DOM rendering can load page resources. List mode processes its explicit sources independently of Spider scope. Resuming a saved Spider crawl checks queued URLs against the current scope without deleting previously collected results.
- Separate internal/external `nofollow` choices in Settings > Scope, including page-level meta/header directives, while retaining link evidence. Older profiles keep their original combined choice. These preferences affect new discovery; explicit List inputs and already queued resume URLs are preserved. Standalone `sponsored`/`ugc` values are retained in link reports and do not currently have separate discovery policies.
- **Settings > Resources > Reference discovery** independently enables canonical, hreflang, pagination and AMP targets in Spider mode. All four default to off; List and Exact URL do not expand them. HTML and repeated HTTP Link canonicals follow the existing scope, robots, resource, nofollow, depth and deduplication rules. Metadata stays captured regardless of discovery choices. References do not add hyperlink edges or inflate inlink/outlink counts; separate retention controls and reference-source attribution remain pending.
- **Settings > Scope > Check links outside start folder** optionally checks discovered same-scope URLs outside the start folder once, retaining their response evidence without expanding links or sitemaps from those pages. Recursive crawling outside the folder remains a separate choice. Robots, exclusions, resource permissions and redirect checks still apply; the new option defaults to off.
- **Settings > HTTP headers** exposes the User-Agent and editable name/value rows. New configurations use Chrome desktop defaults: a reduced Chrome 153 User-Agent, navigation `Accept`, `Accept-Language: en-US,en;q=0.9` and `Upgrade-Insecure-Requests: 1`. **Use Chrome defaults** restores this preset; **Use Ferrous Frog defaults** restores crawler identification and clears the overrides. Both actions stay in the draft until Apply. Saved custom values and explicitly empty header lists survive upgrades.
- Header overrides apply only to the starting origin, including robots, sitemaps and browser requests, and are stripped on foreign-origin redirects/resources. List mode uses its first URL or sitemap as the origin. Duplicate names, invalid values, credential-like names and transport-controlled headers are rejected before configuration is saved. Chrome rendering keeps native resource negotiation for default `Accept` and upgrade values; compression and `Sec-*` headers remain under the HTTP client/browser's control. The HTTP preset omits signed exchanges because the Rust fetcher has no decoder for that representation. It uses Chrome-style headers, not Chrome's network implementation. The preset follows [Chromium's reduced User-Agent format](https://www.chromium.org/updates/ua-reduction/); its major version was checked against [Google's stable-version API](https://versionhistory.googleapis.com/v1/chrome/platforms/win/channels/stable/versions?pageSize=1).
- Resource-type crawl toggles for HTML, images, CSS, JavaScript, external URLs, and other files. External fetching is off by default. Enabling it checks linked external URLs and follows their redirects without expanding links from external pages; include/exclude rules and robots still apply. Exact URL never expands external links, and Spider sitemap seeds remain internal. Turning external fetching off removes external pending entries when resuming.
- Query-string controls for parameter sorting, full query stripping, regex-based parameter stripping, and maximum retained parameter count.
- Spider/List mode selection with pasted URL lists, uploaded URL files, sitemap URL list sources, duplicate-preserving List rows, and original-list-order exports. File import appends HTTP/HTTPS URLs in their input order, including repeated entries, and stays in the Settings draft until applied.
- Custom robots.txt override support in crawler config and Settings UI.
- Crawl Settings presets for safe defaults and an explicit benchmark mode that disables robots.txt.
- Single-URL and batch robots.txt tester commands with Settings UI checks for overridden robots rules.
- robots.txt download action to populate override text from the current root URL.
- **Settings > Sitemaps** controls Spider sitemap discovery: the seed origin's robots.txt declarations, an origin `/sitemap.xml` probe, HTML-linked sitemaps and explicit sitemap URLs. Sources are enabled by default; the master switch disables discovery. Sitemap indexes expand recursively within 128 document attempts, depth 4 and the scoped URL budget, with shared robots, request pacing and cancellation. Discovered pages obey Spider scope; late sitemap discovery also updates membership for already crawled URLs. Exact URL disables discovery, while List mode retains its separate sitemap inputs. Invalid or unreadable explicit sources show an error.
- Sitemap orphan reporting with `in_sitemap` storage, issue view filtering, Overview drill-down, and grid visibility.
- Sitemap validation report for status errors, redirects, non-indexable URLs, orphan sitemap URLs, canonical mismatches, and CSV export. SQLite counts, searches, sorts and pages report rows before decoding them, preserving repeated List occurrences and a consistent read snapshot. Five 200-row queries over 20,000 records fell from about 16 seconds to 0.3 seconds in the [local debug measurement](docs/BENCHMARKS.md); queries still scan candidate metadata.
- HTML parsing for titles, meta descriptions, estimated title/meta pixel widths, H1/H2, canonicals, directives, images, mobile viewport, AMP, pagination, hreflang alternate URLs, JSON-LD syntax and basic schema.org/rich-result fields, Open Graph, Twitter Cards, deprecated HTML tags, duplicate id attributes, links, and image/stylesheet/script resources.
- HTTP security-header capture for HSTS, CSP, X-Frame-Options, and X-Content-Type-Options.
- Per-request network timing capture for DNS lookup, TCP connect probe, TLS handshake probe, header wait/TTFB, body download, total network time, transfer rate, resolved IP count, and redirect-hop timing.
- Content fingerprinting with BLAKE3 response hashes, word counts, text-to-code ratio, and SimHash near-duplicate clusters.
- **Exact Response Duplicates** finds complete successful HTML responses with the same BLAKE3 hash at two or more distinct normalized final URLs. Repeated List entries or redirect aliases alone do not create a duplicate group; matching record occurrences remain visible. Hashes cover the decoded response bytes before text decoding, rendering or content selection. SQLite caches matching record IDs by evidence revision and pages results in SQL; progress events preserve the last queried count.
- **Settings > Content** selects include/exclude CSS regions for word counts, text-to-code ratio, near-duplicate fingerprints and rendered word deltas, with up to 100 selectors per list and 2,000 characters per selector. Overlapping selections count text once; exclusions win, and unmatched includes yield empty text. Metadata, links, custom extraction and exact response hashes keep their existing scope. Both arrays empty retain legacy body-text behavior; configured regions omit script/style and other non-content elements but do not inspect computed CSS visibility. Preview pasted HTML (up to 512 KiB) with the same parser; only an 8,000-character text preview returns to the UI, and sample HTML is never saved. Existing crawl records require a new crawl to apply changed regions.
- Optional JavaScript rendering backend using Chrome DevTools Protocol through `chromiumoxide`, behind the `js-rendering` Cargo feature.
- Settings controls for rendered DOM crawling, Chrome CDP backend selection, and post-load wait timing. Rendering availability shows build support and the detected browser, with **Check again** after installing a browser. Unavailable rendering cannot be enabled, but an existing saved choice can be turned off without losing its options. Start checks availability before replacing the current results or resumable crawl state.
- Raw HTML versus rendered DOM comparison for rendered crawls, including DOM-change flag, word-count delta, and link-count delta.
- Crawl graph storage with queryable URL nodes and source-to-target link edges, including source position.
- Crawl Graph groups URLs by host and first path segment on a large canvas, with searchable keyboard navigation, incoming/outgoing connections, and pan, zoom and fit controls. URL sections, depth and radial layouts retain positions during theme changes and live refreshes. Status/depth and source-to-target filters preserve accurate broken-link and redirect evidence; JSON export follows those filters. The graph tool and its WebGL libraries load when opened, with an interactive SVG fallback. The offline snippet editor also loads on demand; production JavaScript chunks are checked against a 500 kB limit.
- Graph snapshots use bounded node metadata and edge windows in both storage backends, without cloning or decoding full crawl records. SQLite uses one read snapshot; Memory still scans borrowed records and edges. Native reads run in guarded background workers. Live polling waits for each query to finish, so slow queries can still update the graph. Failed reads offer a retry and retain the previous usable graph and camera. [Local measurements](docs/BENCHMARKS.md) document the gains and remaining query work.
- Dedicated link reports for all links, internal links, external links, broken or unresolved links, nofollow links, selected URL in-links/out-links, anchor-text aggregation, and redirect chains.
- First-source tracking for broken or failed URL rows, including source URL, anchor text, and source position.
- Internal crawl path explorer in URL details, showing the source-to-target hop chain from crawl start to a selected URL.
- User-defined URL segments for filtering result views, tree view, and exports by contains patterns or regex.
- Archive-based crawl comparison for added, removed, changed URLs and issue metric deltas.
- Integration provider contracts for external data merges, plus Google Search Console Search Analytics fetch/test support.
- Google Search Console Settings controls for site URL, OS credential-store token storage, Search Analytics testing, and merging clicks, impressions, CTR, and average position onto crawled URL rows.
- Manual PageSpeed Insights measurements from the selected URL's **PageSpeed** tab, with Mobile/Desktop choice, four Lighthouse category scores and lab LCP, CLS and TBT. Google measures the fetched URL remotely; stop the crawl before running a measurement. INP remains unavailable because standard Lighthouse navigation does not measure user interactions; see [Google's metric guidance](https://web.dev/articles/vitals).
- Optional PageSpeed API keys in **Settings → Integrations**, saved only in the OS credential store. Requests are cancellable, have a 90-second deadline and accept at most 16 MiB of decoded response data. The latest result for that exact row, including device, URLs, date and Lighthouse version, survives SQLite reopening and crawl archive export/import. Failed or cancelled refreshes preserve the previous result. Bulk measurement, result history, dedicated grid/export fields and field Core Web Vitals remain separate work.
- Resizable Overview panel with clickable drill-down filters for status, URL distribution, metadata, headings, canonicals, images, technical checks, and issue signals.
- Overview crawl-speed history with a compact live trend chart.
- URL tree view grouped by scheme, host, path, and query, with prominent 4xx/5xx/no-response branch counts and selectable leaf details.
- Issue views for response families, titles and meta descriptions including estimated pixel-width checks, H1/H2, canonicals, directives, images, mobile, hreflang syntax, hreflang return links, hreflang canonical targets, structured-data errors and warnings, HTML validation signals, rendered DOM differences, security, exact response duplicates, near-duplicates, and broken links.
- Native file exports for CSV, XLSX, XML sitemap, graph JSON, graph node/edge CSV, HTML report, link edge CSV, redirect-chain CSV, and sitemap validation CSV under the user's Downloads/Ferrous Frog/exports directory, with app-data fallback.
- Chunked file streaming for grid CSV, XML sitemap, link edge CSV, redirect-chain CSV, and sitemap validation CSV exports so large text reports are not buffered as one in-memory string.
- Select result rows with Ctrl/Cmd-click, extend a range with Shift-click, or select the current server page with Ctrl/Cmd+A while a row has focus. **Selected Rows CSV** and **Copy Selected Rows** export the standard CSV columns for those exact record IDs in selection order. Selection clears when filters/pages change. Clipboard output is capped at 4 MiB; native selected-record requests accept up to 1,000 IDs.
- **Queued URLs CSV** exports saved pending/in-flight frontier entries with depth, sitemap provenance and List identity. It remains available while crawling and for resumable stopped crawls, independently of result filters. A worker streams a stable queue under the storage lock; it does not clone the crawl records or seen set.
- **Image Alt Text CSV** exports every captured image occurrence in the stopped crawl, including source page, image URL, alt text/length, missing/long flags, declared dimensions, source position and known response size. Shared image queries page directly in SQLite and clone only returned rows in Memory. SQLite caches image-record aliases until record evidence changes; image references remain live. Repeated export/oversized pages took about 36% less time in the [local debug workload](docs/BENCHMARKS.md), while the initial index build costs more. Graph JSON and node/edge CSV use the same bounded graph queries and write directly to atomic export files; query failures preserve existing exports.
- Image, link, anchor-text and sitemap-validation queries run on native blocking workers, return storage failures to the active UI and preserve the selected crawl while reading. Active/paused crawls remain queryable. Reads are not cancellable and workspace changes wait for an in-progress read; the headless Memory backend still builds the full sitemap report before paging.
- Filtered **XLSX** exports stream bounded query pages into temporary worksheet files and publish only a completed file. Stop the crawl first so sorting/filtering remains stable across pages. Sheets exceeding Excel's 1,048,575 data-row limit fail explicitly. The older bytes-returning XLSX command is limited to 10,000 rows; use the file export for larger reports.
- **Export > Audit Workbook (XLSX)** creates eight sheets: Summary, URLs, Broken Links, Redirects, Titles, Descriptions, Canonicals and Content, for the whole stopped or completed crawl regardless of grid filters. Metadata sheets have one row per matching issue; the reported URL count counts each source record once. Broken Links contains failed URL records with first-inlink evidence. Content includes exact duplicate response hashes, original/final URLs, sizes and text metrics. Bounded storage pages and temporary worksheet files keep cell data off the UI and avoid buffering the whole report. A sheet exceeding Excel's 1,048,575 data-row limit rejects the export; failures leave no published partial file. The existing **XLSX** action still exports the filtered grid.
- Mock-site integration coverage for redirects, broken links, duplicate metadata, graph edges, robots blocking, invalid hreflang, invalid JSON-LD, and redirect loops.
- Custom extraction through CSS text, CSS attributes, XPath, and regex, wired into crawl config, stored URL details, exports, dynamic grid columns, server-side search, and server-side sorting.
- **Settings > Extraction > Test extraction** runs a selected draft rule against pasted HTML before applying it. The preview accepts 512 KiB of input and shows at most 100 values of 2,000 characters, with truncation notices and per-rule errors. XPath uses XML-compatible HTML, as in crawls. Closing and reopening Settings preserves an in-flight worker guard; sample HTML and preview results are never saved.
- Custom raw and rendered HTML search for text or regex patterns, with match counts, snippets, URL detail display, dynamic grid columns, server-side search/sort, and CSV/XLSX export.
- System, Light and Dark appearance choices under More > Appearance. First launch follows the operating-system theme, including live changes; explicit choices persist across launches. The softer charcoal dark palette uses layered surfaces and a muted mint accent. Both themes share tokens with the crawl graph, and the saved preference applies before the app loads.
- Left-tabbed Settings dialog for crawl limits, scope, resources, query handling, storage, profiles, rendering, and extraction.
- Checkboxes align with adjacent input controls across Settings, including Crawl, Storage, and Rendering.
- Short screen, drawer, sidebar, disclosure, dialog and detail transitions follow the operating system's reduced-motion preference. Dialogs retain their content through closing, hidden controls leave keyboard navigation, and focus returns after dismissal. Live virtualized rows stay stable during progress updates.

See `ROADMAP.md` for the phased implementation plan.

## Goals

- Crawl websites concurrently with configurable safety and throughput controls.
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
- In-memory storage for headless engine use and tests.
- Automatic SQLite desktop sessions and a searchable crawl library.
- Resume option for the current database crawl with persisted frontier state.
- SQLite reuses queue/seen INSERT statements within each complete checkpoint transaction. Local checkpoint persistence took 66–71% less time in the [matched workload](docs/BENCHMARKS.md#sqlite-frontier-checkpoint-inserts); full frontier copying and summary scans still grow with the crawl.
- SQLite combines progress counts in one query and avoids repeated populated duplicate-text normalization. Fresh summaries over 10,000 records took 17–22% less time in the [matched workload](docs/BENCHMARKS.md#fresh-sqlite-progress-summaries), preserving counts and event frequency. Record scans and duplicate grouping remain.
- Crash recovery status for queued database crawl state.
- Current and custom SQLite database path controls.
- Live UI updates.
- Virtualized results grid.
- URL tree view.
- Light and charcoal dark themes, with System as the default preference.
- Basic audit views.
- CSV export.
- XLSX export.
- XML sitemap export.
- Sitemap validation report.
- Graph JSON export.
- Graph node and edge CSV exports.
- Broken-link and redirect graph shortcuts for source-to-target diagnosis.
- Portable crawl archive export and import.
- **Export > Crawl Archive** writes the whole stopped or completed crawl to `.ffcrawl.json`, independently of grid filters. The background worker writes schema 1 directly to an atomic file, preserving record order, List occurrences, links, images and complete resume state. SQLite records and all link/image collections use 10,000-item pages; the former one-million-link/image cutoff is removed. Failures preserve existing files and release the workspace.
- Archive exports retain one full Memory record snapshot and hydrate the full frontier; Memory query work and archive import/comparison are still unbounded. Counts changing during export trigger a retry error; same-count edits from an external database connection are not a transactional snapshot guarantee.
- HTML report export.
- Link edge and redirect-chain CSV exports.
- Link report modal and crawl graph modal.
- Crawl capacity estimates for memory and database modes.

Captured per URL:

- Original URL and final URL.
- List position and duplicate index for List mode crawls.
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
- Image counts, missing alt counts, long alt counts, per-image source URLs, alt text state, declared dimensions, and oversized status when the image response is crawled.
- Mixed-content references and insecure forms.
- Security-header presence.
- Mobile viewport, AMP, pagination, hreflang counts and alternate URLs, JSON-LD counts, structured-data issues, and social metadata counts.
- In-link and out-link counts.
- First in-link source URL, anchor text, and source position for diagnosing broken URLs.
- Source-to-target link edges for crawl graph snapshots.
- Google Search Console clicks, impressions, CTR, and average position when Search Analytics metrics have been merged.

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
- SQLite through `rusqlite`

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

Install the Rust toolchain specified in [rust-toolchain.toml](rust-toolchain.toml), Node.js 24 LTS, and the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/). On Ubuntu 22.04 or newer, the native build dependencies are:

```bash
sudo apt-get update
sudo apt-get install -y build-essential libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils rpm
```

Windows builds need the Visual Studio C++ build tools and WebView2; macOS builds need Xcode command-line tools. `make` targets use Bash; the underlying npm and Cargo commands also work directly from PowerShell.

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
npm ci
cargo test --workspace --locked
npm run dev
npm run tauri:dev
npm run build
make check-js-rendering
make test-ui
make bench-synthetic BENCH_URLS=1000000
make ci
make build
make release
```

`make ci` runs version consistency, release publication guards, formatting, Clippy, Rust tests, the production frontend build, browser smoke tests, optional rendering compilation, browser discovery and real-browser HTTP politeness tests. `make build` produces the desktop release executable without installers. `make release` bundles it into platform installers under `target/release/bundle/`; explicit cross-target builds use `target/<target>/release/bundle/`. `npm run tauri:build -- -- --locked` is the direct packaging command.

Workbook tests in `make test` / `make ci` require Python 3 (`python3` or `python`) to inspect the generated XLSX ZIP/XML with the standard library. They check worksheet contents, typed audit rows, Unicode, Excel limits and failed-export cleanup.

[Storage and local crawler measurements](docs/BENCHMARKS.md) include a 1,000-page HTTP fixture with cycles, duplicate URLs, planted failures, robots exclusions and stop/reopen/resume in Memory and SQLite. It is an ignored release-profile test, separate from the normal CI suite and the one-million-record storage benchmark.

Use `npm run tauri:dev` for the desktop app. Opening the Vite URL directly in a browser is useful for layout work, but crawl commands require the Tauri runtime.

JavaScript rendering is optional and requires a compatible Chrome, Chromium or Microsoft Edge executable. Start a rendering-enabled desktop build with `npm run tauri:dev -- --features js-rendering`. Standard builds do not include the rendering backend. Settings checks the build and browser path without launching a browser. Rendering uses `CHROME` when set, otherwise the existing Chromium detector searches the usual installation paths; an invalid explicit path produces an error instead of silently selecting another browser. Settings and actual browser launch use the same discovery and file validation. This check does not guarantee that a browser can launch or render every site. `make check-js-rendering` compiles the optional backend and tests discovery.

`make test-rendering` launches Chrome against local fixtures and checks shared robots rules, sustained request pacing, configured User-Agent, cross-site frames, workers, popups, redirects and Pause/Resume/Stop, including pauses longer than the timeout. Headless policy tests verify exact request-admission intervals and concurrent callers; browser timing checks exclude startup and allow delivery jitter across the request window. Set `CHROME` to an installed browser path; its native sandbox must work. Each rendered page uses an isolated temporary profile, and cancellation drops its browser process and event handler. Pause abandons unfinished browser renders; Resume reloads those pages for rendering with fresh command deadlines. It keeps the crawl row pending and does not convert a long pause into a raw-HTML fallback. HTTP requests use the crawler policy; WebSocket/WebRTC traffic and full browser network diagnostics are not covered by these checks. An actual render timeout or browser failure falls back to the fetched HTML with the error attached to its row.

`make test-ui` requires Node and a Chrome/Chromium executable (`CHROME_BIN` overrides `google-chrome`). It runs graph model checks and the actual React screen with synthetic Tauri IPC responses. Coverage includes the saved-crawl library and comparison, deletion and session isolation, result/report paging, live audit counts, sorting and filters, exports, Settings drafts/persistence/recovery, and bounded content/extractor previews. Graph checks cover deferred loading, SVG fallback, URL search/keyboard navigation, edge filters and camera preservation. Motion checks exercise retained exits, rapid reopening, nested-dialog and menu focus, closed-control keyboard guards and reduced motion. It leaves crawl data untouched. Theme checks cover system preferences, overrides and secondary-text/active Stop-button contrast against the [4.5:1 minimum](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html); this is not a complete accessibility audit. Production JavaScript chunks must stay at or below 500 kB. Rust integration tests separately exercise local HTTP sites. Set `UI_SCREENSHOT=/tmp/ferrous-frog-ui.png` to save both-theme library, workbench, Settings and responsive screenshots.

Production-shell checks load the built application and splash assets and verify context-menu suppression. Startup and quit UI checks cover the splash page in each appearance mode, failed history queries, checkbox alignment, Yes/No behavior, focus restoration, repeated close requests, and quit errors. With `UI_SCREENSHOT` set, they also save splash, quit, and Crawl/Storage/Rendering Settings screenshots. A Rust lifecycle test checks that stopping waits for task cleanup. Native window-manager behavior still needs testing in a desktop session.

The synthetic benchmark target is ignored by the normal test suite and uses a release build. Run `make bench-synthetic BENCH_URLS=1000000`, or use a smaller value for smoke checks. See [storage measurements](docs/BENCHMARKS.md) for the completed one-million-record run, hardware, workload and limits; this is not an end-to-end site crawl benchmark.

## GitHub Builds and Releases

CI checks pushes and pull requests to `master` or `main`. Release Please opens a version/changelog PR from Conventional Commits such as `fix: save settings` and `feat: add crawl reports`. Merging that PR starts the installer builds for Linux, macOS and Windows, each on x64 and ARM64.

Packages are uploaded to a draft release. The workflow publishes it only after the release commit passes CI and all six builds succeed, then adds `SHA256SUMS`. Failed builds leave the release as a draft and can be retried. GitHub Actions are pinned to commit SHAs; Dependabot groups weekly Cargo, npm and Actions updates.

See [Releasing](docs/RELEASING.md) for the one-time GitHub setting, package formats, manual retries, signing limitations and local build commands. The workflows are configured; their first complete Windows/macOS/Linux run must be verified after pushing to GitHub.

## Crawl Etiquette

Ferrous Frog should default to safe behavior:

- Respect robots.txt.
- Use conservative rate limits.
- Bound concurrency.
- Use request timeouts.
- Keep User-Agent editable, with Chrome desktop and Ferrous Frog presets.
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
