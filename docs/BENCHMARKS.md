# Storage And Local Crawler Measurements

Measured on 2026-09-09 with synthetic storage fixtures and the headless local-site crawler fixture. These measurements do not establish real-website throughput or compare Ferrous Frog with another product.

## Reproduce

```bash
make bench-synthetic BENCH_URLS=100000
make bench-synthetic BENCH_URLS=1000000
```

The target uses the locked dependencies and an optimized Rust release build. It is ignored by the normal test suite. Each invocation creates a temporary database, inserts generated records, queries it and removes it. Rust's temporary directory follows `TMPDIR` on Unix; set that to an existing directory on the storage device you intend to measure.

For process memory measurements, compile first, then run the emitted test executable under `/usr/bin/time -v` with `BENCH_URLS=1000000` and arguments `sqlite_large_synthetic_storage_benchmark --ignored --nocapture`. Timing `cargo test` also includes Cargo and any compilation work.

## Environment And Workload

- AMD Ryzen 9 9955HX, 16 cores / 32 threads, 59 GiB system memory.
- Ubuntu 25.10, Linux 6.17.0-41-generic, Rust 1.98.1, bundled SQLite through rusqlite 0.40.2.
- Database on `/tmp`, a 30 GiB tmpfs. These results do not measure physical-disk latency or durability costs.
- One insertion thread; SQLite WAL and `synchronous=NORMAL`, matching the store configuration.
- Sequential URLs with HTML records, every 97th response a 404, 10,000 repeating titles, distinct descriptions and H1 values. Link counts are populated, but there are no stored link edges, image records, HTML bodies, sessions or frontier entries.
- Last-page, duplicate-title and regex queries request ten rows. Duplicate filtering excludes failed pages; the regex matches odd-numbered URLs. Every query returns its filtered count and whole-crawl summary.

## Summary Cache Baseline: One Million Records

These earlier runs use SQL duplicate/regex filters and contain no hreflang annotations. The baseline recomputes the whole-crawl summary for every page; the cached version reuses it until a database write and preserves List input ordering. Each column is one local run, not a statistical distribution.

| Operation | Baseline | With summary cache |
| --- | ---: | ---: |
| Insert 1,000,000 records | 81.61 s | 77.88 s |
| First summary | 6.22 s | 6.02 s |
| Last ten rows, including summary | 6.27 s | 23.24 ms |
| Duplicate-title page, 989,690 matches | 8.21 s | 1.90 s |
| Regex page, 500,000 matches | 6.37 s | 276.64 ms |
| Database file | 745,512,960 bytes | 745,512,960 bytes |
| Test process peak RSS | 10,800 KiB | 10,972 KiB |

The cached run took 86.09 seconds overall. Peak RSS excludes the tmpfs database, kernel page cache, browser and desktop UI; it is not total application memory consumption. The insertion difference is run-to-run variation, not a cache benefit: neither run queries summaries during insertion. Unicode/literal global searches and first-inlink sorting have correctness fixtures but are not timed by this workload.

A preliminary 100,000-record baseline took 7.78 seconds to insert, 621.37 ms for the first summary, 618.93 ms for the last page, 803.08 ms for a duplicate page and 628.54 ms for the regex page. Its database was 74,252,288 bytes.

## Hreflang Queries: One Million Records

The current fixture additionally gives every 100th record a valid hreflang link to the following record, which has a canonical pointing elsewhere and no return annotation. Of those 10,000 sources, 9,896 have successful HTML responses and match each audit. These annotations are stored in the record payload; this still does not model the separate link-edge or frontier tables.

| Operation | Final SQL implementation |
| --- | ---: |
| Insert 1,000,000 records | 76.07 s |
| First summary | 5.91 s |
| Last ten rows, including summary | 21.83 ms |
| Duplicate-title page, 989,690 matches | 1.82 s |
| Regex page, 500,000 matches | 266.96 ms |
| Missing hreflang return-link page, 9,896 matches | 4.13 s |
| Non-canonical hreflang target page, 9,896 matches | 4.46 s |
| Database file | 745,512,960 bytes |
| Test process peak RSS | 26,572 KiB |

The run took 92.71 seconds overall. Each audit includes the matching count, ten decoded rows and the cached summary. SQLite builds narrow alias/reference joins per query; the Rust layer does not copy the entire crawl into a second memory store. URL aliases account for original/final URLs, fragments and host-only forms. Conflicting List/redirect records resolve to the lowest stored record ID in both backends.

A separate regression doubles a fixture where all original URLs redirect to the same final page and asserts that SQLite VM instructions grow by less than three times. This catches the former quadratic alias join without using a timing threshold. Run it with `cargo test -p ferrous-frog-storage sqlite_hreflang_work_does_not_multiply_shared_redirect_aliases`. Semantic fixtures also cover missing targets, invalid annotations, self-references, paging/search, updates and unselected payloads that cannot be decoded.

## Record Revisions During Frontier Writes

```bash
cargo test --locked -p ferrous-frog-storage canonical_tests::audit_cache_frontier_query_workload -- --ignored --nocapture
```

This separate debug-profile workload uses in-memory SQLite with 10,000 complete successful HTML records, a shared title and uncrawled same-host canonical targets. After warming the summary and canonical graph, eight timed cycles replace 100 queued URLs and 100 seen entries, request a progress summary and query 50 canonical-uncrawled rows. The SQL text-normalization function counts aggregate work rather than relying only on timing.

| Cache invalidation | Eight cycles | Extra aggregate text-function calls |
| --- | ---: | ---: |
| All database changes | 5.050 s | 400,000 |
| Record-only revision | 251.7 ms | 0 |

A persistent revision counter changes transactionally on record insertion, update or deletion. Frontier, seen-set and metadata-only writes leave record evidence cached; record changes from another connection invalidate it, and rolled-back changes do not. This one local workload improved approximately 20 times. It does not measure active record ingestion, website throughput, physical disks or a release build. The regression checks that frontier-only writes perform no extra aggregate text normalization.

## SQLite Frontier Checkpoint Inserts

```bash
cargo test --release --locked -p ferrous-frog-storage sqlite_frontier_checkpoint_workload -- --ignored --nocapture
```

The crawler saves its complete pending queue, seen set and completed count on dispatch and completion. Previously, SQLite prepared the same INSERT SQL separately for every queued and seen entry. It now prepares each INSERT once per checkpoint, retaining the complete replacement in one transaction.

This ignored release-profile workload calls the public `SqliteStore::try_save_frontier_state` method against in-memory SQLite. Each case warms an initial checkpoint, then measures seven replacements with rotating queue order and an updated completed count. Fixture construction, snapshot cloning and verification reads are outside the timed region. The record and edge tables are empty, so these measurements isolate checkpoint persistence.

| Pending entries | Seen entries | Inserts per checkpoint, excluding metadata | Median before | Median after |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 | 2,000 | 3,000 | 3.806 ms | 1.095 ms |
| 5,000 | 10,000 | 15,000 | 20.663 ms | 6.362 ms |
| 500 | 10,500 | 11,000 | 11.882 ms | 4.073 ms |

Checkpoint time decreased 66–71% in these matched local runs. Each timed save still changes exactly `2 × (pending + seen + 1)` rows: the previous checkpoint is deleted and the complete replacement inserted. The workload verifies the final checkpoint, and ordinary fixtures cover List occurrences sharing a URL, queue order, all saved metadata, seen deduplication, shrinking and empty checkpoints. Injected queue, seen and metadata insertion failures retain the previous checkpoint; subsequent saves succeed.

Checkpoint frequency, transaction boundaries and SQLite durability settings are unchanged. Full frontier cloning, seen sorting and replacement still grow with crawl size. These measurements exclude network requests, progress-summary scans, session-index writes, concurrent UI queries and physical-disk latency; they do not establish crawler throughput.

## Fresh SQLite Progress Summaries

```bash
cargo test --release --locked -p ferrous-frog-storage sqlite_progress_summary_workload -- --ignored --nocapture
```

The crawler requests a progress summary synchronously after each completed record. Record changes invalidate the existing summary cache, so this path still performs aggregate work during ingestion. SQLite now evaluates 34 ordinary counts in one SELECT using the existing grid-view predicates. Four separate duplicate-group queries remain; moving empty-group exclusion after grouping avoids normalizing each populated metadata value twice. Summary fields, cache invalidation and progress-event frequency are unchanged.

This ignored release-profile workload calls the public `SqliteStore::try_progress_summary` method against in-memory SQLite with 10,000 records per metadata case. The mixed case has repeating titles and distinct descriptions/H1/H2 values; the repeated case shares those other values; the empty case uses NULL, empty strings and Unicode whitespace. Each case includes planted HTTP failures, robots exclusions, non-HTML responses, incomplete bodies and near-duplicate clusters.

After one warm query, seven fresh summaries are timed. A response-time update before each query changes the record revision without changing audit findings. Fixture insertion, these updates and verification are outside the timed region; each result must equal the initial full progress summary.

| Metadata case | Median before | Median after |
| --- | ---: | ---: |
| Mixed | 53.128 ms | 41.483 ms |
| Repeated | 50.633 ms | 39.641 ms |
| Empty | 44.028 ms | 36.729 ms |

Fresh-summary time decreased 17–22% in these matched local runs. Separate deterministic regressions use 128 records: counted record visits fell from 4,352 to 768, and the four duplicate summaries reduced text normalizations from 1,024 to 512. Instrumentation is absent from the timing workload. Run those work checks and the semantic fixtures with `cargo test --locked -p ferrous-frog-storage summary_tests -- --nocapture`.

The fixtures compare Memory and SQLite for empty and mixed records, populated Unicode normalization, eligibility rules and separate List occurrences. They also check normalized empty duplicate groups, malformed unrelated payloads, ActiveStore forwarding, unchanged-cache reuse, frontier-only writes, direct and external record changes, deletion, clear and rollback. The aggregation change retained the existing grid predicates and normalization rules.

A subsequent correctness fix replaces SQLite's ordinary-space-only metadata trimming with the existing Rust `ff_trim` function in ten shared view predicates. Imported tabs and Unicode whitespace now count consistently with Memory in missing, short/narrow and title-same-as-H1 views; raw values are preserved. Four additional fixtures cover these rules, eligibility, List occurrences, summary parity and advanced-filter composition. An adjacent rerun of the original pre-aggregation binary and the corrected implementation measured mixed/repeated/empty medians of 50.165/49.017/42.088 ms and 34.656/33.779/31.855 ms, respectively. This follow-up includes the predicate correction; it does not isolate trimming overhead. An earlier corrected run varied up to 108 ms per query, so timings remain local observations rather than a latency guarantee.

There is no new cache or schema, and fresh summaries still scan records and group duplicate text after each record revision. These measurements exclude edges, frontier checkpoints, session writes, HTTP requests, concurrent UI queries and physical-disk latency. They measure one storage call, not whole-crawl throughput or constant-cost progress at larger scales.

The existing 1,000-page local crawler fixture was also run before and after the summary-query change, with prepared frontier INSERTs present in both versions. Each run retained 1,030 records and 7,110 edges, with 1,031 HTTP requests when uninterrupted and 1,039 after stop/reopen/resume.

| Local crawler case | Before summary change | After summary change |
| --- | ---: | ---: |
| SQLite, uninterrupted | 5.053 s | 4.742 s |
| SQLite, stop/reopen/resume | 5.080 s | 4.686 s |
| Memory, uninterrupted | 6.749 s | 7.611 s |
| Memory, stop/resume | 6.763 s | 7.463 s |

These are single crawler runs, including scheduler and frontier work, rather than the seven-sample storage medians above. The Memory ingestion implementation was unchanged and its timings also varied. Treat the crawler timings as observations; they do not establish a general throughput gain. The local-crawler section below describes the fixture and its tmpfs, concurrency and response-delay limits.

## Repeated Image Queries

```bash
cargo test -p ferrous-frog-storage --locked image_alias_repeated_page_workload -- --ignored --nocapture
```

This debug-profile, in-memory SQLite workload contains 50,000 crawl records (10,000 image responses) and 50,000 image occurrences across 5,000 source pages. It measures a cold 100-row page, five 10,000-row export pages, then ten 100-row oversized-image pages. These are individual local runs; the measurements include filtering, counts and page decoding.

| Operation | Rebuild aliases per query | Revision-cached TEMP index |
| --- | ---: | ---: |
| First 100-row page | 0.186 s | 0.225 s |
| Five export pages | 2.559 s | 1.648 s |
| Ten oversized-image pages | 12.525 s | 8.073 s |
| Extra image-record alias normalizations for export pages | 50,000 | 0 |
| Extra image-record alias normalizations for oversized pages | 200,000 | 0 |

The initial query pays for an indexed connection-local alias table. Later pages reuse it until a record insertion, update or deletion changes the evidence revision; the highest record ID wins when aliases collide. Reference-only edits stay visible without rebuilding record aliases. Tests cover direct and external writes, rollbacks and bounded decoding. Repeated pages improved approximately 36% in this workload, while cold startup became slower. Per-occurrence normalization, filtering, counting and OFFSET scans remain; this does not measure physical disks or active crawl throughput.

## Local-Site Crawler And Recovery

```bash
cargo test --release --locked -p ferrous-frog-crawler-core synthetic_local_site_crawler_load -- --ignored --nocapture
```

This ignored integration fixture crawls a local HTTP server with 1,000 linked HTML pages, cyclic paths, repeated fragment/query variants, ten broken URLs, ten redirects and ten robots-blocked URLs. Another ten excluded targets appear in link evidence without receiving requests. Each response waits 5 ms; concurrency is explicitly set to eight, request spacing is disabled for this local benchmark, robots remains enabled and sitemap discovery is disabled. Product defaults are unchanged.

Each completed run contains exactly 1,030 records and 7,110 link edges. Interrupted cases stop after 250 records, retain pending URLs and original record IDs, then resume. SQLite is closed and reopened before resuming.

| Backend and run | Before Memory indexing | After Memory indexing | HTTP requests in both runs |
| --- | ---: | ---: | ---: |
| Memory, uninterrupted | 27.103 s | 6.572 s | 1,031 |
| Memory, stop/resume | 26.496 s | 6.603 s | 1,039 |
| SQLite, uninterrupted | 6.117 s | 6.574 s | 1,031 |
| SQLite, stop/reopen/resume | 6.169 s | 6.626 s | 1,039 |

Peak overlap in delayed response handlers was six to seven, below the configured limit. No robots-blocked or excluded target received a request. Stopping can cancel in-flight requests that Resume must fetch again; the fixture bounds this extra work and verifies all completed IDs survive. Timings exclude compilation and final assertion queries; interrupted totals include Stop, reopen and Resume. The SQLite file uses the temporary directory, which was tmpfs in this environment.

A sampled optimized Memory run found 52 of 100 GDB snapshots inside link-edge record lookup, which repeatedly scanned records and normalized their URL aliases. Another 15 snapshots were rebuilding first-inlink annotations through the default progress summary. Memory now maintains alias membership during upsert and summarizes borrowed records for progress. Status lookups retain the earliest matching record; inlink counts still update every matching redirect/List occurrence, including after URL replacement or clear.

The work regression `cargo test --locked -p ferrous-frog-storage memory_ingestion_lookups_do_not_scan_unrelated_records -- --nocapture` checks 50 and 500 records. Both sizes now require two URL-alias expansions per edge insertion and four per single-target inlink update; the original 50-record case required 400 and 151. A separate progress regression verifies unchanged counts without any URL/source annotation work. The alias counter is compiled only into storage unit tests, outside this release crawl measurement.

Memory elapsed time decreased 75–76%, approximately four times the throughput in this fixture. SQLite's implementation was unchanged. These are single optimized runs with tiny generated HTML and no desktop UI, Chromium, remote latency or physical disk. The original debug Memory run exceeded the 120-second phase deadline; use the release command above for this workload. The new index retains additional memory proportional to URL aliases; peak memory was not measured. Larger sites and active UI query traffic still need separate profiling.

## Paged Sitemap Validation

```bash
cargo test --locked -p ferrous-frog-storage sqlite_sitemap_repeated_page_workload -- --ignored --nocapture
```

This debug-profile, in-memory SQLite workload stores 20,000 crawl records with generated URLs and HTML title metadata. Half belong to sitemaps, including 2,000 URLs with status 404. Each measurement requests five consecutive 200-row pages using the report's default ordering; insertion and compilation are excluded.

| Operation | Decode and sort all crawl records | SQL report paging |
| --- | ---: | ---: |
| Five unfiltered pages, 10,000 total matches | 16.188 s | 0.286 s |
| Five pages searching for `4xx`, 2,000 total matches | 16.309 s | 0.327 s |

Each operation returns 1,000 rows. These single local runs improved approximately 57 and 50 times. SQLite now evaluates report findings, count, search, ordering and pagination before decoding only the requested report columns and rows. Both direct queries and the storage trait use this path, including existing native report and CSV callers. Count and rows share one read transaction so an external commit cannot split their snapshot. No cache or schema change is involved.

Fixtures compare every supported sort in both directions, literal Unicode searches and individual issue labels against Memory. They preserve original/final URLs, separate List and redirect occurrences, and SQLite's existing List-position/ID order when all report sort keys tie. Changes to records, sitemap membership and inlinks are visible immediately, including external writes, rollbacks, deletion and reopening. A bounded-decoding fixture supplies invalid unrelated crawl metadata and an invalid report field outside the requested page; valid pages still succeed, while selecting the invalid report field returns an error.

This measures storage queries, not website throughput, physical disks, CSV file writing or the desktop UI. SQLite still scans candidate rows to count, evaluate findings and sort, with additional work for later OFFSET pages. Memory avoids cloning full crawl records for this report but still constructs and sorts all report rows before paging.

## Bounded Graph Snapshots

```bash
cargo test --release --locked -p ferrous-frog-storage graph_snapshot_storage_workload -- --ignored --nocapture
```

This ignored workload creates 50,000 records and 150,000 directed edges in each backend. Records contain generated titles, descriptions, headings, canonicals and custom extraction values. Every 37th non-root URL is external, every 97th response is a 404, and each source has three links, primarily cyclic; every 100th source includes an uncrawled target. SQLite runs in memory without a persisted crawl database. There is no HTTP traffic, rendering or raw HTML.

The UI query requests 700 nodes and 1,500 edges and returns 700 nodes with 1,495 edges whose endpoints are both present. Its internal-only variant returns 700 nodes and 1,455 edges from 145,947 eligible edges. The export-sized query requests 5,000 nodes and 10,000 edges and returns 5,000 nodes with 9,966 edges. These existing limits and selection rules are unchanged.

Before changing the graph path, a separate optimized probe timed the public edge query, full record read and graph request on the same dataset. The full record read took median 142.8 ms in Memory and 2,077.0 ms in SQLite; the 1,500-edge page took 6.8 ms and 1.8 ms. Every graph request loaded all records and rebuilt first-inlink annotations that its output never used. Memory also cloned all edges before paging and retained the large vector allocation.

The same probe and fixture were run after the change. Each UI timing below is the median of three requests; internal-only and export timings are single requests. Each UI request follows the separate edge and full-record measurements, whose execution time is excluded but whose allocator/cache state can influence the graph measurement.

| Graph construction | Memory before | Memory after | SQLite before | SQLite after |
| --- | ---: | ---: | ---: | ---: |
| UI snapshot | 178.126 ms | 14.504 ms | 2,160.072 ms | 57.317 ms |
| UI, internal only | 168.890 ms | 2.586 ms | 2,032.979 ms | 47.832 ms |
| Export-sized snapshot | 165.806 ms | 6.044 ms | 2,057.950 ms | 68.237 ms |

The matched UI measurements improved approximately 12 and 38 times. A separate executable with a counting Rust allocator measured the allocation work of each UI request:

| Backend | Allocations before | Allocations after | Peak extra Rust heap before | Peak extra Rust heap after |
| --- | ---: | ---: | ---: | ---: |
| Memory | 3,453,128 | 8,022 | 119.271 MiB | 0.660 MiB |
| SQLite | 4,254,172 | 10,224 | 127.136 MiB | 0.662 MiB |

Allocation instrumentation was excluded from the timing table. These heap figures measure additional live Rust allocations during a request, excluding the retained store and SQLite's C allocations; they are not process RSS or total SQLite working memory.

The reproducible command above makes consecutive graph requests without the probe's intervening full-record reads. Its successful post-change run took 7.56 seconds including fixture setup, with median UI times of 1.550 ms for Memory and 36.044 ms for SQLite. These absolute timings use a different call sequence and should not be substituted into the before/after comparison.

Memory now borrows records and edges, retaining only the selected node winners and capped edge page. SQLite ranks narrow URL/order keys, then decodes only the selected graph columns and edge rows in one read transaction. Fallible direct and active-store queries expose storage failures to native graph queries and exports. Tests cover internal filtering before admission, zero limits, List ordering and ties, later occurrences sharing a final URL, original redirect aliases, edge-only placeholders, malformed unrelated payloads, changes and clear, and an external write between graph query stages.

The graph still uses literal final URLs and existing backend record order. Node metadata comes from the last eligible occurrence of each admitted final URL; placeholder endpoints retain their original strings. A queued URL without an admitted edge or record is absent, and `totalNodes` remains the retained snapshot count. Status, depth, search and selected-node focus operate on the returned snapshot. SQLite still scans/ranks candidate keys and may use temporary SQLite storage; Memory scans retained records/edges. These measurements cover graph construction, excluding JSON/CSV serialization, IPC, layout/rendering, concurrent ingestion and physical storage.

## What Remains

- The first summary still scans the dataset. Record writes invalidate the cache, including commits from another SQLite connection; frontier-only writes now retain it. Frequent queries while ingesting a million URLs still need incremental counters or fewer repeated aggregate scans.
- Frontier checkpoints still clone the complete pending queue and seen set, sort seen entries, and replace both tables on dispatch and completion. Reusing INSERT statements reduces SQL preparation without bounding that work as the crawl grows.
- Duplicate queries still normalize and group matching text inside SQLite. They decode only the requested row window, but grouping remains proportional to dataset size.
- Hreflang audits rebuild temporary alias joins for their count and page queries. Repeated audits during active large crawls may justify persistent indexed aliases or cached issue membership, with explicit invalidation.
- Memory upsert still scans existing edges for status updates and first-inlink source annotations, and its progress summaries still scan retained records. Alias indexing removes repeated record lookup scans without making all ingestion work constant-time.
- Graph hydration is capped, but its record/key scans and internal-edge counts still grow with the crawl. Measure concurrent ingestion and SQLite temporary working memory before claiming constant-cost refreshes at larger scales.
- The 1,000-page local crawler now covers edges, a live frontier and interrupted/reopened crawls. Increase workload variety and scale, add concurrent UI query traffic, physical SSD storage and packaged desktop memory before making large-site capacity claims.
