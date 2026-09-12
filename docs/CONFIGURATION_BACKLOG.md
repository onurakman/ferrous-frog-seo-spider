# Configuration Coverage And Implementation Backlog

Reviewed on 2026-09-12 against the current repository, the supplied reference screenshots, the public [configuration index](https://www.screamingfrog.co.uk/seo-spider/user-guide/), and relevant [configuration guidance](https://www.screamingfrog.co.uk/seo-spider/user-guide/configuration/). The scope includes the full settings families, beyond the screenshots. Detailed provider parameters will be verified against each provider's current API when implemented.

Status describes Ferrous Frog: **Available** means a working setting exists; **Partial** means capture, engine support, or a smaller workflow exists; **Missing** means implementation is still required. A report or captured field does not imply its configuration is complete. This is a backlog, not a feature availability claim.

## Delivery Order

1. Finish request politeness, URL identity/canonicals, and SQLite query correctness and scale.
2. Build the searchable settings tree and draft Apply/Cancel workflow; expose scope and split crawl/store controls with engine support.
3. Add limits, extraction choices, content controls, configurable audit thresholds, and cancellable post-crawl analysis.
4. Complete PageSpeed and external metrics; add authenticated providers and login flows with credentials kept outside profiles.
5. Add advanced rendering diagnostics, provider-specific extensions, scheduling and optional AI tools after their prerequisites are stable.

Local implementation and fixture testing can proceed without user input. Real account consent, paid provider access, signing identities, and native testing on unavailable operating systems require external resources. Those dependencies do not block unrelated items.

## Workbench And Settings

Evidence: [App.tsx](../src/App.tsx), [appearance.js](../public/appearance.js), [main.rs](../src-tauri/src/main.rs).

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| UI-01 | Scope shortcuts | Available: current host, start folder, all subdomains and exact URL in the toolbar, synchronized with Settings and persisted. Legacy host/descendant and custom folder choices remain supported; active crawls lock scope changes. |
| UI-02 | Operating mode | Available: Mode menu exposes Spider/List, archive comparison and offline SERP preview. Each crawl mode keeps its own start URL across restarts; List sources remain intact. Running crawls lock mode changes/comparison with explanatory text. |
| UI-03 | Snippet editing | Available: offline CSV import/edit/export and desktop/mobile previews, bounded to 1,000 rows / 5 MiB. Measurements reuse the parser's estimates; drafts never replace crawl results and need export to survive quitting. |
| UI-04 | API-only runs | Missing: allow supported metric providers to process a URL list without starting a site crawl. |
| UI-05 | Settings navigation | Partial: twelve sections, including Sitemaps, HTTP headers and Content, have fixed keyword search, initially collapsed indented groups, breadcrumbs and keyboard-accessible headers. Search reveals matching groups; clearing it collapses them. Narrow windows use a grouped section picker; switching sections resets form scrolling. Add dedicated sections as controls grow, direct field focus and contextual help. |
| UI-06 | Draft configuration | Available: separate draft with Apply/Cancel/OK, native rule validation, dirty state, cancellation on dismissal and write-failure recovery. Loaded profiles remain drafts until Apply, with the latest selection taking precedence. Explicit workspace/credential/profile-save actions retain their own buttons and immediate effects; workspace mutations serialize and lock editing/start until complete. |
| UI-07 | Configuration profiles | Available: named profiles and automatic persistence. Extend migration and validation as settings grow; sanitize portable profiles. |
| UI-08 | Grid layouts | Available: relevant/all/custom columns, individual visibility, keyboard-accessible ordering and named layouts persisted across restarts. URL remains available; saved layouts can be deleted. |
| UI-09 | Appearance | Available: System/Light/Dark with live OS changes and softer charcoal dark surfaces. Theme-aware splash has a 2.2-second minimum. Screen, panel and dialog transitions respect reduced motion. Accent customization is missing. |
| UI-10 | Localization | Missing: the app and repository currently use English. Introduce UI translation resources when this phase is reached. |
| UI-11 | Advanced filters | Available: All/Any groups of up to 20 typed text/numeric rules, draft Apply/Cancel and native validation. Grid, tree and filtered exports use the same rules with existing audit/search/segments. Filters reset between datasets. |

## Crawl Discovery And Retention

Evidence: `CrawlConfig`, `CrawlResourceTypes`, scope/frontier code in [crawler-core](../crates/crawler-core/src/lib.rs), resource discovery in [parser](../crates/parser/src/lib.rs). Requirements include the separate Crawl/Store choices in the supplied screenshots.

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| CR-01 | Resource matrix | Partial: HTML, images, CSS, JavaScript, external and other-file crawl toggles with separate default-on store choices; crawled types are always stored, and uncrawled types with store off drop their edges, counts and image references. Add explicit media discovery. |
| CR-02 | Hyperlink matrix | Partial: links are captured and queued; optional external checks fetch linked URLs without expanding external pages. Separate internal/external store choices retain or drop edges and inlink/outlink counts for hyperlinks that are not crawled; crawled hyperlinks are always stored. Source diagnostics for dropped links are not retained. |
| CR-03 | Alternate-link matrix | Partial: canonical, hreflang, pagination and AMP metadata is captured, with independent default-off Spider discovery controls for each type. Repeated HTTP Link canonicals and rendered references follow existing request/scope/robots/resource/nofollow/depth rules; List/Exact URL do not expand. Metadata and hyperlink counts remain unchanged. Independent retention, stored reference-source attribution and generic/mobile alternates remain missing. |
| CR-04 | Embedded navigation | Partial: default-off meta-refresh and iframe discovery follows the reference-link rules (scope, robots, resources, nofollow, depth; not in List/Exact URL). Retention of the targets on records, source labels and mobile-alternate discovery remain missing. |
| CR-05 | Legacy assets | Partial: generic other-file crawling. Keep SWF as an asset type if needed; no Flash execution. |
| CR-06 | Folder boundary | Available: start-folder/exact-folder rules and separate one-hop checking outside the start folder without expanding those pages. Redirect, resume, robots, resource and nofollow rules remain enforced. |
| CR-07 | Subdomain boundary | Available: explicit all-subdomain scope uses the bundled Public Suffix List including private suffixes; IP, local and unknown-suffix hosts remain exact. Existing host/descendant profiles keep their original boundaries. |
| CR-08 | Link directives | Partial: separate internal/external nofollow choices, page-level meta/header directives and legacy-profile migration are implemented with retained edge evidence. Settings affect new discovery; existing queued resume entries lack rel provenance and retain their eligibility. Standalone sponsored/ugc values are retained in reports; distinct discovery policies remain missing. |
| CR-09 | Invalid references | Partial: URL normalization discards malformed targets. Retain diagnostic references without issuing invalid requests. |
| CR-10 | Sitemap sources | Available: persisted Spider master switch, seed-origin robots-advertised discovery, origin probe, HTML-linked sources and explicit URLs. Indexes recurse with document/depth/URL bounds, shared request policy and cross-source deduplication. Late discoveries update stored membership. Exact URL disables discovery; List inputs remain independent. |
| CR-11 | URL transformations | Partial: sorting, stripping and limiting query parameters exist. Add ordered rewrite rules with before/after preview and collision diagnostics. |
| CR-12 | Host aliases/CDNs | Available: CDN host entries with optional path prefixes classify matching URLs as internal for links, records, scope and resource rules; external crawl permission stays separate. Host aliases for duplicate-site detection remain missing. |

## Extraction And Audit Inputs

Evidence: `PageSignals` and fixture tests in [parser](../crates/parser/src/lib.rs), `CrawlRecord` in [storage](../crates/storage/src/lib.rs). Current parsing is mostly unconditional.

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| EX-01 | Metadata selection | Partial: titles, descriptions, meta keywords, headings and directives are captured; meta keywords are searchable, sortable and exported. Field-group switches remain missing. |
| EX-02 | Text statistics | Partial: word count, text/code ratio and near-duplicate fingerprints use configurable include/exclude regions. Exact response hashes retain full-body scope. Readability analysis remains missing. |
| EX-03 | Response details | Partial: timings, hashes, size and selected header flags exist, with bounded Rust HTTP response reads. Add optional complete headers and separate browser download limits. |
| EX-04 | Structured markup | Partial: JSON-LD syntax and selected schema checks exist. Add extraction switches, richer validation and Microdata/RDFa handling. |
| EX-05 | Social metadata | Partial: Open Graph/Twitter counts exist. Add individual values, validation and retention choices. |
| EX-06 | HTML retention | Missing: raw/rendered HTML is transient. Add opt-in storage with size limits, cleanup and archive compatibility before source inspection. |
| EX-07 | PDF inspection | Missing: extract text, metadata and hyperlinks with bounded document processing. |
| EX-08 | Forms/accessibility | Partial: insecure-form counts. Add form details and an optional rendered accessibility audit with visible prerequisites. |
| EX-09 | Responsive images | Partial: parser selects an initial source candidate. Add complete srcset/picture candidates, CSS background images and rendered-size diagnostics. |

## Limits And Request Controls

Evidence: `CrawlConfig`, `RequestPolicy`, normalization and retry functions in [crawler-core](../crates/crawler-core/src/lib.rs).

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| LM-01 | Total/depth budgets | Available: total URLs, link depth and redirect count. Keep behavior consistent across fresh and resumed runs. |
| LM-02 | Group quotas | Missing: budgets per depth, host and path pattern; persist counters in resumable state. |
| LM-03 | Path depth | Available: max folder depth counts directory segments of discovered URLs, separate from link depth; 0 is unlimited. |
| LM-04 | URL/link bounds | Partial: max URL length and max links queued per page exist (0 = unlimited); link evidence stays complete. Per-document retained-link caps and truncation diagnostics remain missing. |
| LM-05 | Download bounds | Partial: Rust HTTP responses have a persisted 20 MiB default limit, adjustable up to 1 GiB, checked during decoded streaming for pages, robots.txt and sitemaps. Incomplete HTML retains HTTP evidence and is excluded from on-page audits. Chromium network downloads still need separate bounds. |
| LM-06 | Query limits | Available: retained parameter cap. Distinct URL-variant budgets are missing. |
| LM-07 | Pacing | Available: concurrency, per-host rate and request spacing. Maintain the same policy for redirects, sitemaps and rendered HTTP requests. |
| LM-08 | Timeout/retry | Partial: timeout, attempts, exponential backoff and origin-shared Retry-After for Rust page/robots/sitemap 429/503 responses. Seconds/dates/overflow, retry budgets and pause/stop are covered. Browser requests honor observed delays; browser response observation, cross-run cooldowns and configurable retry status categories remain open. |
| LM-09 | robots policy | Available: enabled by default, override, download, single/batch testers and crawl delay. Add clearer policy diagnostics; keep safe defaults in every mode. |
| LM-10 | Agent/headers | Partial: Chrome desktop defaults, a Ferrous Frog preset, editable User-Agent and validated non-secret headers, restricted to the starting origin across pages, robots, sitemaps and browser requests. Preset actions use the existing Settings draft; explicit saved values survive upgrades. Browser rendering retains native resource negotiation for default Accept/upgrade values. Credential/transport headers are rejected before saving. Mobile/device presets and dedicated authenticated requests remain missing; credentials belong in the OS store. |
| LM-11 | Proxy/certificates | Partial: platform TLS verification. Add explicit proxy configuration and custom trust management without disabling verification by default. |

## Rendering And Advanced Behavior

The [JavaScript crawling guide](https://www.screamingfrog.co.uk/seo-spider/tutorials/crawl-javascript-seo/) identifies browser diagnostics as a separate area from basic DOM extraction. Ferrous Frog's current implementation is in [rendering.rs](../crates/crawler-core/src/rendering.rs).

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| JS-01 | Rendering availability | Available: optional backend, browser detection, recheck and safe start validation. |
| JS-02 | Browser requests | Partial: shared HTTP robots/pacing/pause/stop is verified with real Chrome. Add resource-level include/exclude choices, blocked-resource diagnostics and unsupported-transport handling. |
| JS-03 | Load completion | Partial: a fixed post-load wait and timeout. Add clear readiness choices and timeout diagnostics. |
| JS-04 | Device emulation | Missing: viewport, scale and mobile presets, kept consistent with the configured agent. |
| JS-05 | Browser evidence | Missing: screenshots, console/runtime errors and resource failures, with bounded retention. |
| JS-06 | Composed DOM | Missing: optional shadow-root and frame inspection; preserve document/source identity. |
| JS-07 | Offline capture | Missing: opt-in page/resource capture after HTML retention and streaming exports are ready. |
| AD-01 | Cookie lifetime | Missing: explicit none/page/crawl-session choices, inspection and reset; isolate crawl credentials. |
| AD-02 | Audit eligibility | Partial: successful-HTML gating exists. Add policy choices for non-indexable pages and paginated duplicates without deleting original records. |
| AD-03 | Chained references | Partial: manual redirects are followed and recorded. Complete canonical traversal and configurable List-mode behavior. |
| AD-04 | Indexability policies | Partial: generic robots/canonical decisions. Add bot-specific precedence and reporting options for pagination/meta refresh. |
| AD-05 | HTTPS policy | Partial: HTTPS and security headers are inspected. Add explicit HSTS handling with recorded synthetic transitions. |
| AD-06 | Fragment checks | Missing: optional anchor/bookmark validation, separate from normal fragment-free URL identity. |
| AD-07 | HTML validation | Partial: duplicate IDs and deprecated tags. Add configurable validation with contextual evidence. |
| AD-08 | Missing MIME type | Available: responses without a Content-Type header whose body starts with an HTML document are recorded as `text/html (inferred)` and audited as HTML; other typeless responses keep an empty content type. |
| AD-09 | Environmental estimate | Missing, low priority: optional transfer-based estimate with disclosed assumptions; no misleading precision. |

## Content And Thresholds

The supplied Content screen and [duplicate-analysis guide](https://www.screamingfrog.co.uk/seo-spider/tutorials/how-to-check-for-duplicate-content/) inform this workflow. Evidence for the current behavior: `visible_text`, SimHash clustering and duplicate query filters in the parser, crawler and storage crates.

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| CT-01 | Analysis region | Available: persisted include/exclude CSS selectors and bounded pasted-HTML text preview share the crawl parser. Overlap is deduplicated, exclusions win, unmatched includes yield no text. Only text metrics/fingerprints change; metadata/discovery and exact response hashes retain their scope. DOM selection does not evaluate computed CSS visibility; existing records require recrawling. |
| CT-02 | Duplicate policy | Partial: exact metadata duplicates, near-duplicate clusters and an Exact Response Duplicates audit with grid/count/workbook evidence. Complete successful HTML with equal decoded-response hashes must span distinct normalized final URLs; repeated List rows or aliases alone do not qualify. Hash scope is independent of rendering/content regions. Configurable eligibility, similarity interpretation and paired text evidence remain missing. |
| CT-03 | Duplicate inspection | Missing: paired content evidence and recomputation after policy edits; depends on bounded text retention. |
| CT-04 | Language quality | Partial: the AI tab reports spelling/grammar issues with suggestions and the detected language per URL. Dictionary controls, ignore lists and bulk evidence views remain missing. |
| CT-05 | Semantic analysis | Partial: Settings > AI configures the provider, model, rate limit and page-text bound; intent classification and meta description drafts record the model and time. Embeddings and similarity analysis remain missing. |
| TH-01 | Metadata thresholds | Available: Settings > Thresholds edits title/description character and pixel limits plus H1/H2 lengths. One typed setting drives memory and SQLite views, analysis issues and the HTML report; XLSX/CSV rows carry raw lengths. |
| TH-02 | Content/image thresholds | Partial: heading lengths, the HTML report's large-image limit, the thin-content word count and the minimum text ratio are configurable. Alt-length and oversized-image flags stay fixed at capture time. |
| TH-03 | Link thresholds | Missing: configurable depth, internal/external outlink counts and weak-anchor patterns. |
| TH-04 | Reset/reanalyse | Partial: a reset-to-defaults action exists and views re-query immediately because thresholds are evaluated per query. Named presets remain missing. |

## Custom Processing

Evidence: [extractors](../crates/extractors/src/lib.rs) and the Extraction section in [App.tsx](../src/App.tsx).

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| CU-01 | Search rules | Available baseline: raw/rendered HTML and text searches. Extend grouped rule editing, preview and import/export as required. |
| CU-02 | Extractor rules | Available: CSS text/attributes, XPath and regex with a selected-rule sample tester and local error feedback. Preview accepts up to 512 KiB of HTML and returns at most 100 values of 2,000 Unicode characters; normal crawl extraction retains its existing behavior. XPath requires XML-compatible HTML. Samples and preview results are not saved. |
| CU-03 | Link regions | Partial: source positions are retained. Add user-defined region selectors and stable labels in edge reports. |
| CU-04 | Browser scripts | Missing: explicit user-authored scripts with timeout/cancellation and bounded output; no proprietary runtime API compatibility. |

## External Data And Authentication

The API provider and authentication groups are explicitly requested in the supplied screenshots. Existing contracts and parsers are in [integrations](../crates/integrations/src/lib.rs); only Search Console has the current desktop credential/merge workflow. The [authentication guide](https://www.screamingfrog.co.uk/seo-spider/tutorials/crawling-password-protected-websites/) is a workflow reference.

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| API-01 | Search Console | Partial: a connected Google account (desktop OAuth client, loopback + PKCE, automatic refresh) or a pasted token, property input, connection test and metric merge with a date range. Dimension/filter choices, quotas and URL inspection remain missing. |
| API-02 | GA4 | Partial: property ID, date range and merge of sessions/engaged sessions/key events/revenue by host and path through the connected Google account, with grid/export columns. Property listing, filters and custom metrics remain missing. |
| API-03 | PageSpeed | Available: selected-URL and bulk selected-row Mobile/Desktop measurements with Lighthouse category choices, optional OS-keyring API key, cancellation, quota-aware retry with resume, 90-second/16-MiB request bounds, latest-result persistence, grid columns and CSV/XLSX fields. No INP measurement and no result history. |
| API-04 | Field performance | Partial: the Chrome UX Report provider fetches p75 LCP/INP/CLS/FCP/TTFB per form factor with the collection period and a no-data state for the selected URL, stored per row beside the Lighthouse snapshot. Bulk fetching and grid/export columns remain missing. |
| API-05 | Backlink services | Partial: a generic endpoint-template adapter with an OS-stored credential header merges backlink, referring-domain and authority values per URL. Majestic, Ahrefs and Moz specific adapters, index choices and quota handling remain missing. |
| API-06 | Provider-derived URLs | Missing: opt-in discovery, orphan comparison and provenance for URLs absent from the crawl. |
| API-07 | Optional AI providers | Missing: remote/local provider configuration, keyring, model selection, bounded prompt execution, testing and reusable prompts. Keep Phase 5 priority. |
| AU-01 | HTTP authentication | Available: Basic and Digest (MD5, qop=auth) credentials are saved in the OS credential store, enabled per configuration and used only for the starting origin; engine tests cover internal/external requests, the RFC digest vector, one challenge retry and serialized profiles. Browser-rendering authentication remains missing. |
| AU-02 | Interactive login | Partial: form login posts saved credentials once with configurable field names and extra fields; the cookie jar carries the session through the crawl (engine test with a cookie-gated site). Expiry/logout detection, browser cookie transfer and resumable authenticated sessions remain missing. |
| AU-03 | Login profiles | Missing: named credential references and safe profile import/export; never serialize passwords or tokens into plain settings. |

Universal Analytics from the older screenshot is retired; target GA4 and optional historical-file import instead of a new live UA connector. See [Google's migration timeline](https://support.google.com/analytics/answer/11583528).

## Post-Crawl Analysis

Requirements below come directly from the supplied Crawl Analysis screen. Some existing checks run during crawling or querying, but there is no independent, configurable analysis-job workflow.

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| AN-01 | Analysis jobs | Missing: selected rule groups, manual start, optional run at completion, progress, cancel and rerun on a stable dataset revision. |
| AN-02 | Link importance | Missing: graph-based internal importance score with documented treatment of redirects, nofollow and dangling nodes. |
| AN-03 | Redirect analysis | Partial: chains/loops per fetch. Add dataset-wide identity and combined redirect/canonical diagnostics. |
| AN-04 | Content analysis | Partial: online near-duplicate clusters and queryable exact response groups, with revision-cached SQLite membership and bounded result pages. Add repeatable post-crawl near-duplicate recomputation using CT-01/CT-02 settings. |
| AN-05 | Image analysis | Partial: alt/dimension/size records. Add background discovery and natural-versus-rendered sizing. |
| AN-06 | Canonical analysis | Partial: missing/multiple, uncrawled same-host targets, redirects, response errors, non-indexable targets, chains and loops have typed audits and Memory/SQLite parity. Cross-host scope, unlinked targets and conflicting-signal reports remain open. |
| AN-07 | Pagination analysis | Partial: captured next/prev fields (including legacy previous), target errors, direction-specific loops and advisory non-reciprocal warnings, sortable targets and typed issues with Memory/SQLite parity. Reciprocity uses the first captured link per direction and known complete HTML; unknown returns remain unclassified. Unlinked sequence members, multiple relations and sequence consistency checks remain. |
| AN-08 | Hreflang analysis | Partial: syntax, self/return and canonical checks. Add unlinked targets and completeness diagnostics. |
| AN-09 | Inlink analysis | Partial: stored edges and counts. Add depth thresholds, follow/nofollow mixes and only-non-indexable-source cases. |
| AN-10 | Orphan analysis | Partial: sitemap baseline. Add separate Analytics/Search Console sources and explicit unavailable-provider states. |
| AN-11 | Result freshness | Missing: completed/pending/stale state per analysis group; invalidate results after crawl resume or relevant settings edits. |

## Application Preferences And Operations

Evidence: storage/session commands, release handling and theme initialization in the current repository. These also cover remaining application-preference families identified by the public guide index.

| ID | Control/workflow | Current state and remaining work |
| --- | --- | --- |
| OP-01 | Storage | Available: automatic SQLite desktop sessions, centered startup launcher, initially closed bottom history panel with a saved disclosure preference, configuration restore, two-card comparison, location, archives and frontier recovery. Memory remains a headless backend. Complete query/export scale work. |
| OP-02 | Resource budget | Partial: capacity estimates. Add measured memory/disk limits and backpressure; Rust does not need a Java heap-allocation setting. |
| OP-03 | Retention | Missing: opt-in age/count limits, preview of affected sessions and pinned-session protection. |
| OP-04 | Notifications | Partial: release checks/reminders. Add crawl-completion and failure notifications; optional delivery adapters later. |
| OP-05 | Automation | Partial: headless CLI (crawl/list, profiles, export presets), automatic preset exports, completion webhook, desktop notification and in-app once/interval schedules exist. OS-level scheduling while the app is closed remains outside scope; combine the CLI with cron or Task Scheduler for that. |
| OP-06 | Local tool access | Missing, later: optional MCP endpoint with explicit access controls and bounded queries, after headless commands stabilize. |

## Acceptance For Every New Setting

- Persist and migrate a typed value; define units, defaults and valid ranges.
- Apply the value in the engine or storage path it controls, including resume and List mode where applicable.
- Keep sensitive values in the credential store and out of exports, logs and source control.
- Add a focused positive/negative fixture check, and a UI workflow check when interaction is meaningful.
- Verify keyboard access, both themes, small-window layout and a clear unavailable/error state.
- Update this inventory and the phase tracker only after implementation and verification.
