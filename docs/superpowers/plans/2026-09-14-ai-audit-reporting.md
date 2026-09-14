# AI Audit Reporting Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans when implementation is requested. Work task by task; the checkboxes below track implementation, not planning completion.

**Status:** AI-R01–AI-R06 implemented and verified on 2026-09-14. Full CI, offline navigation, the 100,000-page / 1,000,001-link workload and the Linux native report lifecycle passed. AI fixtures use local mocks; no external AI requests or private crawl data were required. Windows/macOS and installer validation remain separate roadmap work.

**Goal:** Produce a polished, shareable site audit with useful AI explanations and access to every affected URL and evidence occurrence, including reports comparing two crawls.

**Architecture:** Build a reproducible report from saved crawl evidence using existing typed audits and storage queries. AI adds validated explanations to those findings; it does not own counts, affected-URL membership or comparison status. A full workspace and a portable HTML report package read the same persisted report snapshot.

**Tech Stack:** Existing Rust analysis/storage/export/integrations crates, SQLite, Tauri workers, React, TanStack virtualization and MiniJinja. Reuse configured LLM providers, credential storage and HTTP clients; no new plugin runtime or provider SDK is required.

**Spec:** The scope, decisions, evidence contract and acceptance criteria in this document are the feature specification.

## Reference review

The user supplied `docs/tmp/ai_rapor.html` and `docs/tmp/ai_rapor_2026_08_25.html` as private visual/workflow references. Leave them unchanged and outside version control; use synthetic data in implementation fixtures.

- Preserve the readable report header, crawl metadata, priority cards, concise executive summary, collapsible findings, severity/team filters, problem/evidence/recommendation structure, measurement table and staged action list.
- The first example contains 39 findings and the second 41. Their renderers display the supplied finding lists; the missing detail is already absent from the data. Evidence consists of short prose lists, grouped examples and abbreviated URLs, without a complete affected-record collection.
- Some summary figures are manually duplicated: the first report's medium-priority card says 17 while its finding data contains 18. Derive cards, filters and exports from one report model.
- The second example adds stable finding numbers, previous/current evidence and status filters. Preserve that workflow with measured comparison states and explicit coverage limits.
- Claims about live browser checks, analytics events, forms, accessibility or legal compliance in these examples do not establish crawler capabilities. Reports may describe a check only when its evidence was collected; unsupported checks remain unmeasured or require review.

## Approach and scope

| Approach | Trade-off | Decision |
| --- | --- | --- |
| Ask an LLM to write one HTML document from crawl data | Small prototype, but model/context limits lose URL detail and HTML/counts become unreliable | Do not use as the report data model |
| Store complete findings/evidence, then add AI narrative and controlled presentation | Reuses current engine and providers; supports exact counts, large crawls and repeatable exports | Recommended |
| Introduce a general plugin marketplace, autonomous browser agents and a hosted report service | Adds unrelated distribution, execution and hosting work | Outside this feature |

“AI extension” means an optional built-in reporting capability using **Settings > AI**. A useful deterministic report must work without an API key, including when AI is disabled, cancelled or unavailable. Existing selected-URL intent, meta-description and spelling actions remain available.

The first release includes a single-crawl report, complete evidence browsing, optional AI explanations, and an offline report package. The next increment adds the two-crawl follow-up report. Existing HTML Report and CSV/XLSX exports remain available during delivery. PDF generation, hosted sharing, arbitrary HTML templates, automatic site edits, browser interaction audits and general-purpose plugin loading are separate work.

## Global constraints

- Repository artifacts, source, UI labels and fixtures remain English. Exported user reports may select English or Turkish independently of the application UI; user crawl content is preserved verbatim and never committed.
- Report generation starts explicitly from a stopped/completed saved crawl. A stopped or failed crawl is labelled partial; its scope, exclusions, crawl limits, blocked/failed requests and capture availability accompany the report.
- Capture a consistent source/configuration snapshot before analysis. Freeze rule versions, thresholds, source identities and comparison policy; later recrawls, Settings changes or source deletion must not alter an existing report.
- Keep crawl records and complete evidence in native storage. Query result windows through commands; do not send the full crawl to React or the LLM.
- Counts, percentages, identities, coverage and measured status come from engine data. Narrative never supplies the authoritative totals or implies a missing measurement was successful.
- Reuse global duplicate/reference/graph diagnostics. Calling `analyze_records` separately on each page would break cross-page rules and is not a valid paging implementation.
- Do not refetch website URLs while writing a report. Use captured fields and opt-in retained text; unavailable, truncated, HTTP and rendered evidence remain distinguishable.
- Keep secrets in the OS credential store. Show the selected provider and transmitted data categories before AI generation; omit headers, cookies, credentials and full source bodies from default prompts. Page-derived text is untrusted data, never instructions.
- Release workspace locks before provider/network work. Use bounded workers, progress, cancellation, request identity and stale-result guards; do not change active crawl/session state.

## Report workflow and presentation

1. Open **Tools > Audit reports**, select a saved crawl and choose **Create report**. Select whole crawl (default) or an explicit saved filter/segment, report title and language; inspect the resulting scope. Enable **AI explanations** optionally using the existing provider configuration.
2. Prepare deterministic findings first. Show progress and capture coverage, then estimate the request/input budget for optional AI work. After preparation, **Review AI data and budget** previews the bounded request; the separate **Send evidence** action authorizes generation. Ordinary report opening/exporting sends nothing.
3. Open a full workspace with report metadata, measured summary cards, an executive summary, finding filters and a prioritized action list. AI interpretation is identified separately from observed facts. Do not invent a numeric SEO score, delivery estimate, launch-blocker verdict or guaranteed ranking impact.
4. Each finding includes severity, category, suggested responsible team, explanation, affected unique URL count, record/occurrence count, available evidence and recommended fix/verification steps. Distinguish a detected issue from a suggested cause or broader AI interpretation.
5. **View all affected URLs** opens a dedicated evidence area, not another cramped modal. Provide backend search, sorting, filters, 100-row virtualized pages, full URL copy, selected evidence inspection and **Export all matching evidence**. Show `1–100 of N`; a small preview is explicitly labelled as examples.
6. Preserve original/final URLs and List occurrence identity. Link findings to every available source page, target, anchor/position, observed value and related captured field. If legacy storage cannot attribute an edge to an exact occurrence, report that limitation instead of inventing a match.
7. Support keyboard navigation, focus restoration, both themes, narrow windows, print styles and plain-text rendering of captured HTML. Use Ferrous Frog's visual identity and native controls; do not execute model-provided markup.

## Evidence and persistence contract

Use a report-owned SQLite snapshot, with Memory/SQLite source adapters exposing equivalent report results. Copy only the fields and evidence needed by the report through bounded reads. Retain source identities, rule eligibility and measurement availability for unaffected records too, so a future follow-up can distinguish a verified fix from an unobserved page after source deletion. Preserve global rule context in indexed native storage; do not copy the full dataset into a JSON field or a frontend store.

| Entity | Required contents |
| --- | --- |
| Report run | Stable ID, schema/rule/prompt versions, source session IDs and revisions, timestamps, scope/configuration/thresholds, language, coverage, generation status and optional baseline ID |
| Finding | Stable rule/group key, measured severity, category, count units, affected unique URLs, affected source records, evidence occurrences, coverage and optional comparison state |
| Evidence row | Stable report-local ID, finding key, source storage key/record ID/List occurrence, original/final URL, evidence kind, observed values and available target/source/position/provenance |
| AI annotation | Existing finding/evidence IDs, explanation, proposed cause, recommendation, verification steps, suggested team, model/prompt/input digest, generation time and completion state |

Unique URLs, List records, link/image occurrences and finding counts are different units. For example, one broken destination referenced 600 times on 40 pages must expose **1 target / 40 pages / 600 references** rather than one ambiguous “600 issues” figure. Percentages use an explicit eligible population and handle zero denominators.

The implemented command boundary includes `prepare_audit_report`, `query_audit_report_findings`, `query_audit_report_evidence`, `run_audit_report_ai`, `cancel_audit_report` (run-ID-based cancellation for preparation, AI and export), `export_audit_report` and `delete_audit_report`. Queries take a report ID plus validated filter/sort/offset/limit and return rows with a complete matching total. Use a 100-row default and a 1,000-row maximum; cap individual text previews separately at 64 KiB. Single-crawl and comparison AI use corresponding preview/get/run commands with the same bounded job, status and cancellation model.

Persist report evidence independently of source sessions, and delete it when its report is deleted. Partial preparation never becomes a ready report. AI failure preserves the deterministic report and prior completed annotations; interrupted runs can resume only unfinished batches for the same report/input/provider/model/prompt configuration. Changed inputs create a new generation version.

## AI contract and budget

- Send typed finding summaries, exact aggregate values and bounded representative evidence, using existing input/request limits. Start with at most 10 evidence examples per finding; display the actual sampled count and eligible population. Sampling never changes stored membership or exports.
- Process bounded batches and then synthesize an executive summary from validated finding annotations. A request budget may leave findings without AI text, but those findings and all their URLs remain visible with deterministic explanations.
- Reuse `LlmClient`, provider/model settings, keyring access and rate limiting. Reuse PageSpeed's cancellation/progress/resume approach without copying its 500-selected-row scope restriction.
- Accept structured, length-bounded annotations only. Reject unknown finding/evidence IDs, model-authored URL lists, malformed/truncated output and unsupported properties. Render text through escaped templates/React, not raw HTML.
- Example annotation shape: `{"findingId":"title.missing","evidenceIds":["ev-12"],"explanation":"The captured page has no title.","proposedCause":null,"recommendation":"Add a descriptive title.","verification":"Recrawl and confirm the title is present.","suggestedTeam":"Content"}`. The engine supplies the title, counts, severity and measured status. References can be validated mechanically; interpretive text remains labelled AI-generated and reviewable.
- Report actual provider token usage when available. Preflight estimates are labelled estimates; unavailable usage/pricing stays unavailable. Bound unknown-length provider responses while reading, not only after buffering the body. Apply an explicit maximum request/input budget and bounded retries, honour provider retry delays, and permit immediate cancellation without discarding local evidence.

## Complete offline export

The complete deliverable is a portable folder with `index.html`, numbered finding/evidence HTML pages, per-finding CSV files and a small manifest containing schema, coverage and exact counts. Open it directly from disk; all styles, navigation and scripts are local. No app server, API key or hosted service is needed.

Keep the index concise and visually similar to the references. Every finding links to its full evidence pages and complete CSV. Generate at most 1,000 evidence rows per HTML page with stable previous/next/index links; full URLs and stored values remain available even when a visual cell is shortened. Native workspace search covers the whole finding; offline text search, if supplied, must clearly state whether it searches only the current page. Complete CSV remains available for cross-page analysis.

Stream native query windows to a temporary sibling directory and publish a new complete destination only after every page, CSV and manifest succeeds. Verify exported totals; cancellation, disk-full and write errors leave no published partial report and preserve earlier reports. Build filenames from internal IDs and escape content/links; do not derive filesystem paths from crawled URLs. Check downloaded CSV cells for formula injection.

A standalone summary HTML or printed summary cannot be presented as the complete report. Do not embed the full site as one enormous JavaScript array or silently reuse the current 50-row section sample cap / 1,000,000-edge input ceiling. If preparation cannot obtain complete evidence, it must explicitly mark the affected coverage as incomplete or fail the complete export.

## Follow-up report between two crawls

Reuse the comparison request-URL/occurrence identity policy and compatible content contexts. Match findings by versioned rule/group keys, not model-written titles; report both counts and the added/resolved/persisting evidence sets.

Measured states are **New**, **Resolved**, **Improved**, **Unchanged**, **Worsened**, **Mixed changes** and **Not comparable**. A decrease/increase refers to the comparable evidence population and an identified unit; simultaneous additions and removals remain visible even if net counts match. Mark baseline-only, blocked, failed, out-of-scope or missing-capture pages as **Not observed** or **Not comparable**, not automatically resolved. A rule is resolved only when eligible current evidence verifies that its prior failure condition is absent.

Show before/after values, changed coverage and separate full lists for resolved, persisting, newly affected and unobserved records. Different scopes, audit thresholds, rule versions or unavailable reference evidence require an explicit comparability notice. AI explains these computed changes; it does not decide whether a fix was verified.

## Delivery tasks

### AI-R01: Complete deterministic evidence foundation

**Files:** Add `crates/storage/src/audit_reports.rs` and focused report tests; integrate through `crates/storage/src/lib.rs`. Reuse storage `IssueView` predicates and global duplicate/reference diagnostics; use `crates/analysis/src/lib.rs` as a parity oracle. Do not introduce a storage-to-analysis dependency because analysis already depends on storage. Keep new report code outside the large general-purpose modules.

- [x] Add failing Memory/SQLite parity fixtures for a rule affecting 1,205 records, duplicate List URLs, redirected aliases and a broken target shared across many source pages; assert distinct count units and exact membership.
- [x] Build consistent snapshots with frozen scope, thresholds, rule versions and bounded evidence queries. Map report findings to existing rule/IssueView semantics; list unsupported report checks explicitly.
- [x] Verify first/middle/last pages, sort ties, full search, eligibility, missing evidence and errors. Changing source records or global thresholds after preparation must leave the report unchanged. Verify that non-affected eligible records and missing/failed measurements remain distinguishable in the frozen evidence.
- [x] Run `cargo test -p ferrous-frog-storage -p ferrous-frog-analysis --locked`, document supported rule coverage. The implementation checkpoint is tracked below.

### AI-R02: Saved report lifecycle and full workspace

**Files:** Add `src-tauri/src/audit_reports.rs` and `src/AuditReportWorkspace.tsx`; wire `src-tauri/src/main.rs`, `src/App.tsx` and `src/audit-report.css`. Extend `scripts/smoke-ui.mjs` with synthetic report IPC.

- [x] Exercise prepare/query/delete, cancellation, partial preparation, source deletion, restart/reopen and stale responses before adding UI wiring. Require no website requests and no active-session replacement.
- [x] Implement the report-owned worker lifecycle and proposed bounded commands, then the report launcher, summary/findings view and full evidence area. Persist report language and scope on the report itself.
- [x] Prove that `1–100 of 1,205` can reach record 1,205, search can find an off-page record, and count cards/filter totals remain consistent. Verify keyboard, both themes, narrow layouts and 64 KiB preview truncation.
- [x] Run `cargo test -p ferrous-frog-app --locked` and `make test-ui`; update user documentation and commit.

### AI-R03: Optional AI explanations over frozen findings

**Files:** Reuse `src-tauri/src/ai.rs` settings and add report orchestration in `src-tauri/src/audit_report_ai.rs`. Add report prompt/validation helpers in `crates/integrations/src/report_ai.rs` and bounded transport handling in `llm.rs`; extend the report workspace and targeted fixtures.

- [x] Add mock-provider failures for unknown evidence IDs, invalid/oversized/truncated output, refusal, timeout, rate limiting and injected page instructions. Assert that no provider output can replace counts or evidence membership.
- [x] Implement opt-in bounded batches, data/budget preview, progress, cancellation, usage reporting and resumable annotation versions. Reuse credentials without serializing them into reports or archives.
- [x] Test a report with more findings than its AI budget permits: all findings and all evidence must remain accessible, while unprocessed explanations stay explicitly pending. Preserve existing annotations on retry failure.
- [x] Run `cargo test -p ferrous-frog-integrations -p ferrous-frog-app --locked` and the report UI smoke; document provider/data limits and commit.

### AI-R04: Portable report with every affected URL

**Files:** Add `crates/export/src/audit_report.rs` and a dedicated report template alongside `crates/export/templates/seo_report.html.j2`; integrate `crates/export/src/lib.rs` and native report export commands. Add a focused offline report check under `scripts/`.

- [x] Test a 1,205-row finding with a unique final record before implementing export. Require two linked evidence pages, exactly 1,205 CSV data rows, matching manifest totals and a reachable last record.
- [x] Build the self-contained HTML/CSV folder through bounded writers and atomic directory publication; reuse template escaping, spreadsheet safety and existing export destination conventions.
- [x] Verify direct `file://` navigation offline, full URL/value preservation, no remote assets, print styles and no lost rows at page boundaries. Inject write/cancellation failures and confirm prior exports survive.
- [x] Run `cargo test -p ferrous-frog-export -p ferrous-frog-app --locked` plus the offline browser check; document that the complete artifact includes its evidence folder and commit.

### AI-R05: Comparison and remediation follow-up

**Files:** Extend report storage/native/export/workspace modules. Reuse `src-tauri/src/comparison.rs`, `src-tauri/src/comparison_sources.rs` and comparison identity helpers only where their semantics apply; record-only comparison snapshots do not supply link/image evidence automatically.

- [x] Add fixtures for a verified fix, recurrence, additions/removals with unchanged net counts, changed rule thresholds, response-only noise, shuffled duplicate List inputs and a removed/blocked baseline URL.
- [x] Compute before/after membership and comparability from frozen snapshots. Expose status filters, measured deltas and complete evidence sets in both the workspace and portable report.
- [x] Verify that missing pages cannot become “Resolved” and AI text cannot change computed status. Keep report generation isolated from the active comparison workspace and crawl.
- [x] Run the affected Rust suites, comparison/report UI smoke and offline export check; update comparison documentation and commit.

### AI-R06: Scale and release acceptance

- [x] Exercise at least 100,000 affected evidence rows and more than 1,000,000 link occurrences. Demonstrate bounded UI/IPC/export pages, full final-record reachability and count reconciliation; record memory, preparation, query and export measurements in `docs/BENCHMARKS.md`.
- [x] Verify incomplete/legacy evidence is labelled accurately, report restart/reopen remains independent of source edits, and cancelled generation/export frees temporary resources.
- [x] Run `make ci` and the new offline report check. Fix and complete the existing native Linux harness before relying on a real desktop report workflow check; document unavailable platform checks separately.
- [x] Mark individual AI-R items complete only after their evidence passes. Update `README.md`, `ROADMAP.md` and `docs/CONFIGURATION_BACKLOG.md`; do not mark sitewide reporting available based solely on existing per-URL AI.

## Main acceptance example

For a finding with **12,480 affected pages**, the report may display 10 labelled examples and send only bounded samples to AI. **View all affected URLs** must query all 12,480 pages, sorting/search must cover that full population, and the portable report must include all 12,480 records in reachable evidence pages and complete CSV. The last record must be testable, totals must reconcile, and a provider failure must not remove any affected page.

## Implementation checkpoint

- AI-R01: 16 page rules and retained broken-link evidence use consistent frozen Memory/SQLite snapshots. Counts, source identities, eligibility, unsupported checks and imported/partial provenance are explicit.
- AI-R02: the full workspace supports saved reports, measured priorities, severity/category/team filters, bounded evidence search/sort/HTTP status filtering and complete matching CSV export. UI smoke reaches record 1,205, preserves stale-response isolation and checks themes, keyboard access and narrow windows.
- AI-R03: optional annotations and executive overviews use strict reference validation, bounded provider reads, request/input budgets, retry metadata, cancellation and resumable sidecar versions. Failed new attempts preserve earlier usable generations with explicit model/version provenance in UI and export.
- AI-R04: complete offline HTML/CSV packages preserve full stored evidence, escape captured/model text, reconcile all counts and publish atomically. Real Chrome opens local pages and reaches the last row; cancellation/write failures preserve earlier exports.
- AI-R05: independent frozen comparisons retain complete before/after evidence after source-report deletion. Missing/incompatible observations never imply a verified fix. Optional AI consumes typed computed changes through the same runner, and complete comparison packages include validated commentary and generation provenance.
- AI-R06: the scale fixture reconciles 1,100,002 evidence rows from 100,000 affected pages and 1,000,001 links. Indexed export cursors eliminate repeated full counts/offset scans on complete exports; measurements and known limits are in `docs/BENCHMARKS.md`.
- `make ci` passed, including full workspace tests, formatting, Clippy, UI/offline checks and seven serialized real-Chrome rendering fixtures. After the final shared deletion rollback fix, all 121 native application tests, application Clippy and formatting passed again. The rebuilt embedded-assets debug application passed the complete Linux crawl/report/restart/export/quit smoke.
- Additional authorized roadmap work: recovery summaries now read native counts without hydrating the full queue/seen checkpoint. The one-million-seen SQLite median fell from 72.260 ms to 1.158 ms in the isolated benchmark; UI Resume selection uses the same stale-response guard as the recovery data.
- Windows, macOS, installers, broader physical-disk/concurrent-UI benchmarks and remaining technical audit rules are still tracked separately; this delivery does not claim those complete.
