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

At the time of this measurement, the crawler saved its complete pending queue, seen set and completed count on dispatch and completion. Previously, SQLite prepared the same INSERT SQL separately for every queued and seen entry. It now prepares each INSERT once per checkpoint, retaining the complete replacement in one transaction.

This ignored release-profile workload calls the public `SqliteStore::try_save_frontier_state` method against in-memory SQLite. Each case warms an initial checkpoint, then measures seven replacements with rotating queue order and an updated completed count. Fixture construction, snapshot cloning and verification reads are outside the timed region. The record and edge tables are empty, so these measurements isolate checkpoint persistence.

| Pending entries | Seen entries | Inserts per checkpoint, excluding metadata | Median before | Median after |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 | 2,000 | 3,000 | 3.806 ms | 1.095 ms |
| 5,000 | 10,000 | 15,000 | 20.663 ms | 6.362 ms |
| 500 | 10,500 | 11,000 | 11.882 ms | 4.073 ms |

Checkpoint time decreased 66–71% in these matched local runs. Each timed save still changes exactly `2 × (pending + seen + 1)` rows: the previous checkpoint is deleted and the complete replacement inserted. The workload verifies the final checkpoint, and ordinary fixtures cover List occurrences sharing a URL, queue order, all saved metadata, seen deduplication, shrinking and empty checkpoints. Injected queue, seen and metadata insertion failures retain the previous checkpoint; subsequent saves succeed.

Checkpoint frequency, transaction boundaries and SQLite durability settings were unchanged in this storage-only comparison. Full frontier cloning, seen sorting and replacement still grow with crawl size. These measurements exclude network requests, progress-summary scans, session-index writes, concurrent UI queries and physical-disk latency; they do not establish crawler throughput. The later [scheduler comparison](#avoiding-dispatch-only-checkpoints) measures skipping redundant saves.

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

## Avoiding Dispatch-Only Checkpoints

Measured on 2026-09-14 with the unchanged `synthetic_local_site_crawler_load` fixture above. Dispatch moves a URL from the waiting queue to the active set, both of which are already included in the saved frontier. The scheduler now omits that redundant checkpoint. Startup, completed results, worker errors and Stop retain their existing persistence paths. An uninterrupted 1,030-record fixture avoids 1,030 complete frontier snapshots, including queue/seen copying, seen sorting and SQLite table replacement.

The same optimized before/after binaries were run serially on the Ryzen 9 9955HX with Rust 1.98.1 and locked dependencies. The first comparison uses `/tmp` on tmpfs; each entry is one run, excluding compilation and final verification queries.

| Backend and run | Before | After |
| --- | ---: | ---: |
| Memory, uninterrupted | 6.317 s | 6.018 s |
| Memory, stop/resume | 6.330 s | 6.043 s |
| SQLite, uninterrupted | 3.888 s | 3.254 s |
| SQLite, stop/reopen/resume | 3.959 s | 3.283 s |

SQLite elapsed time fell 16–17% in this tmpfs comparison. Every run retained 1,030 records, 7,110 edges and the expected request counts: 1,031 uninterrupted or 1,039 with Stop/Resume. Robots exclusions, concurrency limits, completed record IDs and empty final frontiers remain verified by the fixture.

For the physical-storage follow-up, `TMPDIR` pointed to a fresh temporary directory under `/home/onur-akman/.cache` on ext4, backed by `/dev/nvme0n1p2`. Three before/after pairs ran serially, alternating binaries and excluding a preliminary NVMe run. Each invocation still exercised both backends and both recovery cases; no build or other test suite ran concurrently. Values below are medians, with the observed SQLite ranges in parentheses.

| Backend and run | Before | After |
| --- | ---: | ---: |
| Memory, uninterrupted | 6.114 s | 5.918 s |
| Memory, stop/resume | 6.115 s | 5.968 s |
| SQLite, uninterrupted | 4.419 s (4.402–5.786) | 3.788 s (3.782–3.826) |
| SQLite, stop/reopen/resume | 4.454 s (4.435–4.513) | 3.791 s (3.776–3.819) |

SQLite medians fell 14–15% in this NVMe fixture. To reproduce on another device, set `TMPDIR` to an existing directory on that device before running the local-site command above; repeat against both revisions. The database uses the existing WAL/`synchronous=NORMAL` settings and normal filesystem caches. These are physical-storage runs, not cold-cache or power-loss tests.

The ordinary regression `cargo test --locked -p ferrous-frog-crawler-core aborted_dispatch_preserves_active_and_waiting_frontier_for_resume` blocks two HTTP responses while two URLs remain waiting, then aborts the crawl task without a final Stop checkpoint. It compares the saved pending identities and metadata, reopens SQLite and resumes all pending work. The Memory/SQLite matrix covers duplicate List occurrences and Spider discovery while another request is active; completed records retain their IDs. This tests task interruption and database reopening, not power loss.

This comparison predates the incremental completion updates below; at this stage each remaining checkpoint still copied and replaced the complete frontier. The timing observations are not a general throughput or latency guarantee.

## Incremental Frontier Checkpoints

```bash
cargo test --release --locked -p ferrous-frog-storage sqlite_incremental_frontier_workload -- --ignored --nocapture
```

Measured on 2026-09-14. Ordinary crawler completions now delete the completed storage key, append newly discovered queue entries and seen keys, update affected sitemap flags, and save the completed count in one SQLite transaction. Pending order and separate List identities remain intact. At this measurement, startup, Stop and worker-error checkpoints retained full replacement; the later [Stop workload](#stopping-without-replacing-the-durable-frontier) covers removal of ordinary Stop and worker-error snapshots. Memory mutates its saved state in place, but still scans the queue and sorts seen keys when discoveries are added.

The ignored storage workload compares the full-save and update APIs in the same optimized build. Each in-memory SQLite case warms a frontier, then times seven one-completion/one-discovery checkpoints; preparation, full snapshot cloning and verification are excluded. Both paths finish with the same queue, seen set and counter. The full-save path includes the new URL index used for sitemap-flag updates.

| Pending | Initial seen | Full-save median | Incremental median | Full-save row changes per call | Incremental row changes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 2,000 | 1.435 ms | 0.015 ms | 6,003–6,015 | 4 |
| 5,000 | 10,000 | 7.486 ms | 0.017 ms | 30,003–30,015 | 4 |
| 500 | 100,000 | 39.244 ms | 0.020 ms | 201,003–201,015 | 4 |

The ordinary regression additionally changes one existing sitemap flag and bounds writes to five rows at both 100 and 10,000 seen URLs; the preceding full-replacement implementation wrote 303 rows in the smaller case. Further tests cover List metadata, shared URLs with different keys, pending order, retained seen keys, rollback at queue/seen/sitemap/counter writes, reopening and unchanged audit revisions. Writes grow with new entries and affected sitemap rows; indexed lookups still have a size-dependent cost.

The unchanged 1,000-page HTTP fixture was also measured in three serial before/after pairs on the same NVMe/ext4 filesystem as above. The before binary already omits dispatch-only checkpoints. Both binaries precede the separate reference-evidence retention feature, so these timings isolate checkpoint changes. Compilation and other test suites were excluded; the table shows medians and SQLite ranges.

| Backend and run | Before incremental updates | After |
| --- | ---: | ---: |
| Memory, uninterrupted | 5.950 s | 5.900 s |
| Memory, stop/resume | 5.966 s | 5.910 s |
| SQLite, uninterrupted | 3.792 s (3.766–5.733) | 3.155 s (3.144–3.206) |
| SQLite, stop/reopen/resume | 3.843 s (3.791–3.846) | 3.218 s (3.196–3.223) |

SQLite medians fell another 16–17% in this fixture. Every run verified 1,030 records, 7,110 edges, 1,031 uninterrupted or 1,039 interrupted HTTP requests, robots/scope exclusions and preservation of completed IDs. The task-abort regression also checks late linked-sitemap flags on an active sibling and newly queued work, including SQLite reopening without a final Stop snapshot.

These measurements do not bound whole-crawl cost: progress summaries and other record/edge operations still grow with the dataset. Larger mixed sites, concurrent UI queries, initial and interrupted-discovery snapshot hydration and broader disk/memory measurements remain open.

## Stopping Without Replacing The Durable Frontier

```bash
cargo test --locked -p ferrous-frog-crawler-core large_frontier_stop_workload -- --ignored --nocapture
```

Measured on 2026-09-14 in the unoptimized test build. The scheduler already saves waiting and active entries at startup and updates the durable frontier after each completed item. Ordinary Stop now reuses that state. Worker errors remove their completed key incrementally; Stop during linked-sitemap discovery still writes a full checkpoint to preserve additions and sitemap flags collected before cancellation. Cancellation while loading initial sitemaps reads saved counts without hydrating the queue.

The synthetic workload holds 500 pending URLs, 100,000 seen URLs and no crawl records or edges. It resumes and cancels synchronously at the `started` callback, before dispatching any HTTP request. Each backend runs seven trials. Timing starts at cancellation and ends after the crawl future returns, including scheduler cleanup; fixture construction, startup hydration/normalization/checkpointing and assertions are excluded. Both backends retain the identical complete frontier. SQLite is in memory. The before build differs only by retaining the full final checkpoint for Stop; the after build skips it.

| Backend | Before median (range) | After median (range) |
| --- | ---: | ---: |
| Memory | 97.287 ms (87.917–107.809) | 8.580 ms (6.131–10.142) |
| SQLite | 274.708 ms (265.670–304.188) | 9.233 ms (8.764–11.307) |

These are one local before/after batch during development verification, not an isolated system-load experiment or optimized release/physical-disk latency guarantees. Dropping the scheduler's queue and seen set still costs time proportional to their size. Active network cancellation, populated-record progress summaries, concurrent UI queries and peak memory are outside this workload. Set `FERROUS_STOP_SEEN` to at least 500 to repeat at another size.

The ordinary `stop_` regressions verify `PRAGMA data_version` remains unchanged after cancellation at startup or after a completed record, then reopen the SQLite file and resume to completion. A separate Memory/SQLite fixture cancels the second linked sitemap and verifies the first sitemap's pending URL and membership survive and complete on resume. Existing worker-completion races, request cancellation and List/reference recovery checks also pass.

## Recovery Counts Without Frontier Hydration

```bash
cargo test --release --locked -p ferrous-frog-storage frontier_recovery_summary_workload -- --ignored --nocapture
```

Measured on 2026-09-14 using the CPU/toolchain described above. The desktop recovery query previously loaded every pending item and seen URL just to return queued, seen and completed counts. This query runs when opening saved sessions/databases and Settings, and after stopping a crawl. SQLite now reads the two counts and completed metadata in one statement; Memory reads the lengths of its borrowed checkpoint. The native command performs the query on a blocking worker after releasing the application store mutex.

The ignored optimized workload stores 500 pending entries and either 100,000 or 1,000,000 seen URLs. Crawl record and edge tables are empty; SQLite is in memory. Both paths warm once, then seven paired calls alternate measurement order. Fixture creation, insertion, compilation and result assertions are excluded. The old path includes full snapshot loading, deriving the three counts and dropping the snapshot; the new path returns the same counts directly.

| Backend | Seen URLs | Full hydration median | Counts-only median |
| --- | ---: | ---: | ---: |
| Memory | 100,000 | 3.422349 ms | 0.000060 ms |
| SQLite | 100,000 | 6.638960 ms | 0.034355 ms |
| Memory | 1,000,000 | 38.547188 ms | 0.000141 ms |
| SQLite | 1,000,000 | 72.260097 ms | 1.158042 ms |

SQLite hydration ranges were 6.585–7.193 ms and 71.460–73.247 ms; counts-only ranges were 0.023–0.042 ms and 0.450–1.265 ms. Memory counts-only measurements are near the clock/measurement overhead floor and should not be interpreted as reliable speedup ratios. These are one local seven-pair run per case, not application latency guarantees. No allocation or RSS measurement was taken.

Run the focused correctness checks with `cargo test --locked -p ferrous-frog-storage frontier_summary`. They cover separate List occurrences, pending/seen-only/empty states, incremental changes, clear, external commits, rollback, reopening and existing completed-counter defaults. A deterministic guard puts invalid values in unused queue and seen fields: full hydration fails, while the summary and ActiveStore forwarding return correct counts. Missing storage tables still produce an error rather than an empty recovery state. Custom storage implementations retain a default method based on their existing snapshot API.

This counts-only change did not alter scheduler checkpoint persistence, resume normalization, record summaries or event frequency; subsequent Stop changes are measured separately above. SQLite counts still traverse database pages as needed. Physical-disk behavior, concurrent ingestion/UI queries, total Stop latency, memory peaks and broader frontier hardening remain unmeasured here.

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
- Ordinary SQLite completion checkpoints and worker errors write only changed frontier rows; ordinary Stop reuses that durable state. Startup and interrupted discovery still hydrate or replace the full frontier. Memory updates scan its pending vector and sort seen keys after discoveries. Those remaining paths need larger-scale measurements.
- Duplicate queries still normalize and group matching text inside SQLite. They decode only the requested row window, but grouping remains proportional to dataset size.
- Hreflang audits rebuild temporary alias joins for their count and page queries. Repeated audits during active large crawls may justify persistent indexed aliases or cached issue membership, with explicit invalidation.
- Memory maintains URL aliases and first-inlink sources as records and edges arrive. Upsert still scans existing edges for status updates, and progress summaries still scan retained records. The retained indexes consume memory proportional to aliases; ingestion is not constant-time.
- Graph hydration is capped, but its record/key scans and internal-edge counts still grow with the crawl. Measure concurrent ingestion and SQLite temporary working memory before claiming constant-cost refreshes at larger scales.
- The local crawler now covers 10,000 live pages, retained edges/frontiers, interrupted/reopened crawls and concurrent bounded storage queries on physical NVMe. Two serial before/after runs extend the physical-disk evidence, but larger and denser workloads, repeated unchanged-build trials, other storage devices and actual packaged desktop responsiveness/memory remain open.

## Frozen Audit Reports: 100,000 Pages and 1,000,001 Links

Measured on 2026-09-14 with an AMD Ryzen 9 9955HX, approximately 59.5 GiB RAM, Linux 6.17.0, the pinned Rust toolchain and the **debug** profile. SQLite source/report/package files were on `/tmp` tmpfs. These are single-run acceptance measurements, not release-profile throughput guarantees or end-to-end desktop memory measurements.

```bash
cargo run -p ferrous-frog-export --example audit_report_scale --locked -- /tmp/ff-report-scale 100000 1000001
# Reuse the same frozen input for a query/export implementation comparison:
cargo run -p ferrous-frog-export --example audit_report_scale --locked -- /tmp/ff-report-scale 100000 1000001 --reuse
```

The workload creates 100,000 successful HTML pages missing titles and one 404 target, with 1,000,001 retained links distributed across those pages. The frozen findings reconcile as **100,000 title occurrences + 1 response error + 1,000,001 link occurrences = 1,100,002 evidence rows**. Broken-link units remain **1 target / 100,000 source pages / 1,000,001 references**; exact List-record attribution remains unavailable and is labelled incomplete.

| Operation | Observed result |
| --- | --- |
| Insert 100,001 source records | 21.63 s |
| Insert 1,000,001 source links | 97.51 s |
| Prepare consistent frozen report, including pruning and vacuum | 138.65 s |
| Preparation-process peak RSS | 75,716 KiB (73.94 MiB) |
| Frozen report database | 3,040,432,128 bytes |
| Export first 50,000 rows with repeated counts and offsets | 96.32 s; baseline run interrupted after this measurement |
| Export first 50,000 rows with native sequence cursor | 6.15 s, approximately 15.7 times faster |
| Export all 1,100,002 rows using the cursor | 138.84 s |
| Export plus streaming CSV reconciliation | 146.42 s |
| Export-process peak RSS, reopening the existing frozen report | 15,520 KiB (15.16 MiB) |
| Complete portable package | 1,109 files, 3,144,579,949 bytes |

The report query fixture reads first, middle and last windows of 100 rows for both the 100,000-row page finding and the 1,000,001-row link finding. It asserts every matching total, expected page length and final evidence ID. Example serialized responses range from 67,800 to 93,227 bytes; these sizes describe this synthetic text, not worst-case retained fields. Native IPC still bounds rows and individual previews independently.

The portable writer now reads an immutable finding total once and advances through an indexed sequence cursor. It verifies the expected count and the absence of an unexpected trailing row. The workload then streams every emitted CSV, checks its exact row count, and reconciles the number of 1,000-row HTML pages. A separate 1,205-row Chrome fixture verifies real `file://` navigation to the last row, escaped text, local assets and both themes. Comparison exports use the equivalent indexed cursor with request-URL/occurrence ordering.

Limits: source ingestion, snapshot preparation and the export process were measured separately; kernel page cache and tmpfs file memory are excluded from RSS. Full stored values make complete exports large. Filtered or custom-sort evidence CSV keeps the corresponding query's offset semantics and may cost more than the unfiltered cursor path. Preparation still uses native global diagnostic context, and a running diagnostic statement or vacuum completes before cancellation is observed. Large-scale physical-disk runs, concurrent desktop UI measurements and other-platform runs remain separate acceptance work.

## Legacy HTML And Link CSV: 1,000,001 Edges

The legacy ten-section HTML summary now streams retained link edges through a storage visitor. Memory borrows each edge under one read lock; SQLite decodes one row at a time from a single ordered statement. The report retains an exact broken-edge count and at most 50 display rows. Its record snapshot and global duplicate/audit predicates remain unchanged. CLI and automatic whole-link CSV exports use the same visitor and preserve the existing columns and quoting.

Reproduce each backend/mode in a separate process after compiling the test binary:

```sh
cargo test --locked --release -p ferrous-frog-export legacy_html_edge_stream_workload --no-run
FF_HTML_BACKEND=memory FF_HTML_MODE=snapshot /usr/bin/time -f 'peak_rss_kib=%M' target/release/deps/ferrous_frog_export-<hash> --ignored --exact tests::legacy_html_edge_stream_workload --nocapture
FF_HTML_BACKEND=memory FF_HTML_MODE=stream /usr/bin/time -f 'peak_rss_kib=%M' target/release/deps/ferrous_frog_export-<hash> --ignored --exact tests::legacy_html_edge_stream_workload --nocapture
FF_HTML_BACKEND=sqlite FF_HTML_MODE=snapshot /usr/bin/time -f 'peak_rss_kib=%M' target/release/deps/ferrous_frog_export-<hash> --ignored --exact tests::legacy_html_edge_stream_workload --nocapture
FF_HTML_BACKEND=sqlite FF_HTML_MODE=stream /usr/bin/time -f 'peak_rss_kib=%M' target/release/deps/ferrous_frog_export-<hash> --ignored --exact tests::legacy_html_edge_stream_workload --nocapture
```

Use the executable path printed by the compilation command. `FF_HTML_EDGES` optionally changes the default 1,000,001 edges. Each process creates two records and repeated links with distinct positions and Unicode, quotes and newlines in their anchors. SQLite runs in memory. One warmup precedes three measured renders; timing includes edge traversal, rendering and disposal of the temporary edge snapshot, but excludes insertion, record hydration, assertions and compilation.

The matched snapshot baseline uses the same visitor to collect a full edge vector before rendering. It measures the removed allocation and retention while preserving identical input; the old public query itself could return only one million edges. This is a complete-input comparison, not a successful timing of the old capped desktop command.

| Backend | Complete edge snapshot median | Streaming median | Snapshot process peak RSS | Streaming process peak RSS |
| --- | ---: | ---: | ---: | ---: |
| Memory | 132.557 ms | 5.650 ms | 632.012 MiB | 319.070 MiB |
| SQLite | 1,313.368 ms | 1,067.597 ms | 558.020 MiB | 288.488 MiB |

The three measured ranges were 132.437–133.320 ms versus 5.630–6.045 ms for Memory, and 1,287.293–1,326.815 ms versus 1,062.753–1,074.367 ms for SQLite. Process peak RSS includes fixture insertion and the retained store, including SQLite C allocations; it is not an isolated Rust allocation count. Streaming processes additionally write the actual HTML and link CSV files through `write_export_files`, read every CSV record, verify all 1,000,001 IDs and original anchors including the final row, then remove their temporary output. Those CSV operations are outside the HTML timing measurements.

Focused fixtures also cover all ten populated HTML sections, global duplicates, List storage keys and redirect aliases, borrowed Memory payloads, late SQLite decode errors, visitor/write failures, escaping and a broken edge first encountered at position 1,000,001. Desktop HTML tests preserve existing files on failure. Legacy whole-link CSV and HTML string commands now use the idle background worker, reject active or closing crawls, and retain their complete output strings.

This removes edge-count-dependent report input buffers and the HTML/whole-link CSV ceiling. It does not make full record hydration, duplicate analysis, arbitrary captured string sizes, archive import or every export format bounded. The HTML remains a sampled summary; the separate frozen audit package supplies complete per-finding evidence pages. These measurements exclude HTTP, rendering, physical-disk crawl storage and concurrent ingestion.

## Larger Local Crawls With Concurrent Queries

```bash
FERROUS_CRAWLER_PAGES=2000 FERROUS_CRAWLER_QUERIES=1 cargo test --release --locked -p ferrous-frog-crawler-core synthetic_local_site_crawler_load -- --ignored --nocapture
```

Measured on 2026-09-14 with the optimized crawler test executable at the checkpoint recorded by `64cf05a`, before the later per-target pagination diagnostics. The existing local HTTP fixture now accepts a page count (100 to 100,000, in multiples of 100; default 1,000) and optional concurrent storage queries. Each run still exercises Memory and file-backed SQLite, both uninterrupted and stopped/reopened/resumed. Set `TMPDIR` to a private directory on the desired filesystem for the SQLite files. The fixture removes its databases after a successful run.

With `FERROUS_CRAWLER_QUERIES=1`, a separate thread queries a sorted 50-row grid window, a searched 50-edge window and recovery counts, then waits 1.5 seconds like the workbench refresh. Offsets rotate through ten windows. Each poll checks bounded response sizes and frontier consistency. The worker stops and releases its store before reopening SQLite, and wakes immediately at shutdown. Reported poll latency includes storage-lock waits and all three queries; it excludes IPC, React rendering and window interaction.

| Backend / run | 1,000 pages, no polls (tmpfs) | 1,000 pages, polls (tmpfs) | 2,000 pages, polls (NVMe/ext4) |
| --- | ---: | ---: | ---: |
| Memory, uninterrupted | 6.589 s | 6.574 s | 27.949 s |
| Memory, stop/resume | 6.775 s | 6.740 s | 27.879 s |
| SQLite, uninterrupted | 3.416 s | 3.348 s | 12.393 s |
| SQLite, stop/reopen/resume | 3.353 s | 3.351 s | 12.634 s |

Each cell is one run; the small differences in the 1,000-page pair do not establish a speedup or a general polling cost. The larger case also changes the SQLite filesystem, so it is not an isolated size-scaling comparison. Memory's data remains in process memory in every case. Compilation, browser checks and package compression were kept outside the measured runs.

| Backend / run, 2,000 pages | Polls | Median poll | Maximum poll |
| --- | ---: | ---: | ---: |
| Memory, uninterrupted | 19 | 38.746 ms | 50.686 ms |
| Memory, stop/resume | 20 | 36.864 ms | 54.810 ms |
| SQLite, uninterrupted | 9 | 15.749 ms | 28.077 ms |
| SQLite, stop/reopen/resume | 9 | 15.868 ms | 23.976 ms |

The 1,000-page polling cases produced only 3–6 samples each, with medians of 17.356–18.014 ms for Memory and 6.529–9.373 ms for SQLite. These sample counts do not characterize tail latency. Peak process RSS across all four cases was 27.49 MiB without polls and 32.78 MiB with polls at 1,000 pages, and 52.75 MiB at 2,000 pages. This includes the mock server, retained request log, allocator state and both backends; it is not a per-backend allocation measurement or a desktop memory estimate.

The larger fixture verified 2,060 unique stored URLs, 14,220 edges, 20 planted HTTP failures, 20 robots-blocked URLs and 20 redirects. It made 2,061 uninterrupted or 2,069 interrupted HTTP requests, never requested excluded/robots paths or stripped tracking queries, and retained all completed IDs after stopping at 500 records. The smaller case retained the original 1,030 URLs, 7,110 edges and request-count invariants. No external website was crawled.

At this checkpoint, more than 2,000 live pages and repeated physical-disk trials were unmeasured; the [5,000-page follow-up](#5000-page-crawls-and-maintained-first-inlink-sources) below extends both. Progress summaries and other whole-record operations still grow with the dataset. Denser reference/resource graphs, rendering and actual desktop frame/input responsiveness remain open scale work.

## 5,000-Page Crawls And Maintained First-Inlink Sources

Measured on 2026-09-20 with the existing local HTTP fixture, increasing the page count from 2,000 to 5,000 without changing its checks or 600-second phase deadlines. Each executable runs Memory and file-backed SQLite, both uninterrupted and stopped/resumed, with concurrent queries enabled. SQLite is closed and reopened before resuming.

```bash
cargo test --release --locked -p ferrous-frog-crawler-core synthetic_local_site_crawler_load --no-run
crawl_tmp=$(mktemp -d "$HOME/.cache/ferrous-crawler-scale.XXXXXX")
TMPDIR="$crawl_tmp" FERROUS_CRAWLER_PAGES=5000 FERROUS_CRAWLER_QUERIES=1 /usr/bin/time -f 'peak_rss_kib=%M elapsed_seconds=%e' target/release/deps/ferrous_frog_crawler_core-<hash> --ignored --exact tests::synthetic_local_site_crawler_load --nocapture
rmdir "$crawl_tmp"
```

Use the executable path printed by compilation. The measured runs used separate copied executables and fresh private temporary directories under the user's cache directory on `/dev/nvme0n1p2` (ext4). SQLite retained WAL and `synchronous=NORMAL`. The machine had an AMD Ryzen 9 9955HX, 59 GiB RAM, Ubuntu 26.04.1, Linux 7.0.0-31-generic and Rust 1.98.1. Compilation, repository tests, browser fixtures and packaging did not overlap either timed run. Host background services remained active; caches were not flushed and CPU frequency was not fixed. The OS and implementation differ from the historical 2,000-page run, so this is not an isolated size-scaling comparison.

Both builds came from the working tree based on `6299b66`, including archive frontier streaming and AMP target capture/storage changes. The baseline preceded the generic record visitor and HTML streaming changes. The second build additionally contained those changes and the maintained Memory first-inlink index; the fixture does not export reports or invoke that visitor. Copied executable SHA-256 values were `6f747b736c0f7c46363876a92b5ecd5d8d0775fb06280613b5e0b45aac4542e1` before and `55a7715a54d35257fc44b158f29dc20ae6d59f952cec14159e413cf1e9e9752c` after. These are two serial runs with different builds, one observation per case/build, not repeated unchanged-build samples or crawl-time medians.

Previously, Memory record upserts and reads rebuilt first-inlink sources from all retained edges. It now indexes the earliest source for each target alias when appending an edge, and reuses that index for annotations. Source ordering, fragment/host aliases, redirect/List occurrences and clear behavior remain covered by semantic tests. `cargo test --locked -p ferrous-frog-storage memory_first_inlink_index -- --nocapture` also checks that growing the edge fixture from 50 to 500 does not reintroduce complete source-index rebuilding during record writes, snapshots, selected-ID reads or streaming visits. Edge-status updates still scanned edges in these measured builds; the endpoint-index follow-up below removes that scan.

| Backend / case | Before | After |
| --- | ---: | ---: |
| Memory, uninterrupted | 161.165 s | 101.295 s |
| Memory, stop/resume | 161.596 s | 101.719 s |
| SQLite, uninterrupted | 103.165 s | 99.614 s |
| SQLite, stop/reopen/resume | 154.044 s | 98.053 s |

Memory elapsed time decreased approximately 37% in both cases in this paired observation. The Memory index does not change SQLite ingestion; its differing times show run-to-run variation rather than an attributable index benefit. For example, reaching Stop at 1,250 SQLite records took 46.572 seconds before and 5.260 seconds after. These runs do not identify the cause of that variation or establish a general throughput guarantee.

| Backend / case | Build | Polls | Median poll | p95 poll | Maximum poll |
| --- | --- | ---: | ---: | ---: | ---: |
| Memory, uninterrupted | Before | 102 | 89.438 ms | 133.978 ms | 151.975 ms |
| Memory, uninterrupted | After | 65 | 79.766 ms | 121.998 ms | 128.677 ms |
| Memory, stop/resume | Before | 103 | 89.481 ms | 131.295 ms | 142.836 ms |
| Memory, stop/resume | After | 66 | 81.061 ms | 116.099 ms | 132.453 ms |
| SQLite, uninterrupted | Before | 65 | 56.888 ms | 294.596 ms | 689.022 ms |
| SQLite, uninterrupted | After | 63 | 63.778 ms | 250.434 ms | 352.713 ms |
| SQLite, stop/reopen/resume | Before | 95 | 77.677 ms | 370.514 ms | 475.774 ms |
| SQLite, stop/reopen/resume | After | 62 | 60.421 ms | 285.111 ms | 361.731 ms |

Each poll requests a sorted 50-row grid window, a searched 50-edge window and frontier counts, then waits 1.5 seconds. The table includes storage-lock waits and all three queries; the reported percentiles describe only these samples. Polls do not drive Tauri IPC, React, frames or input handling.

| Whole test process | Before | After |
| --- | ---: | ---: |
| Elapsed, including final verification | 581.41 s | 401.93 s |
| Peak RSS | 85,444 KiB (83.44 MiB) | 125,436 KiB (122.50 MiB) |

Peak RSS increased in the second process. The maintained index retains source information proportional to distinct target aliases, but this aggregate measurement does not isolate its allocation cost: it also includes both backends, SQLite allocations, the mock server/request log, final verification snapshots and allocator retention across cases. Kernel/filesystem page-cache memory and a desktop window are excluded. Per-crawl timings exclude final assertions; interrupted totals include Stop, reopen and Resume.

All eight completed cases verified exactly 5,150 unique stored URLs and 35,550 edges, with 50 planted HTTP failures, 50 robots-blocked URLs and 50 redirects. Requests totaled 5,151 uninterrupted or 5,159 interrupted; no robots-blocked, excluded or stripped-query path was requested. Delayed response-handler overlap peaked at six or seven under the configured limit of eight. Stop at 1,250 records retained a pending frontier and every completed ID; each finished frontier was empty. Query windows remained bounded and recovery counts consistent. Both test processes passed and removed their temporary databases. Request spacing was disabled only for this localhost fixture, response delay remained 5 ms, robots stayed enabled and no external site was crawled.

This extends live-crawl coverage beyond 2,000 pages and repeats the physical NVMe workload across two builds. At that checkpoint, more than 5,000 live pages remained unmeasured; the 10,000-page follow-up below extends coverage. Denser resource/reference graphs, unchanged-build distributions, rendering, other devices/platforms and actual desktop frame/input responsiveness remain open. Full progress scans and startup/interrupted-discovery frontier hydration remain separate costs; the endpoint-index follow-up removes the earlier edge-status scan.

## Archive Comparison Record Staging: 50,000 Records

The comparison workspace now decodes an archive's `records` array one row at a time with Serde into its existing private SQLite snapshot. A private SQL table validates unique IDs/storage keys and maps allocated IDs back to the original sparse or shuffled IDs. Finalization restores IDs transactionally and drops that table. No complete archive-record vector, identity HashSets, or ID-mapping vector is retained. The saved-database read-only SQL copy path is unchanged.

Reproduce after compiling the native test executable:

```sh
cargo test --locked --release -p ferrous-frog-app comparison_sources::tests::archive_record_stream_workload --no-run
TMPDIR="$HOME/.cache" FF_ARCHIVE_MODE=snapshot /usr/bin/time -f 'peak_rss_kib=%M' target/release/deps/ferrous_frog-<hash> --ignored --exact comparison_sources::tests::archive_record_stream_workload --nocapture
TMPDIR="$HOME/.cache" FF_ARCHIVE_MODE=stream /usr/bin/time -f 'peak_rss_kib=%M' target/release/deps/ferrous_frog-<hash> --ignored --exact comparison_sources::tests::archive_record_stream_workload --nocapture
```

Use the executable path printed by compilation and an existing temporary-file parent on the intended filesystem. `FF_ARCHIVE_RECORDS` optionally changes the default 50,000 rows. The measured runs used separate processes and a private temporary parent under the user's cache directory on NVMe/ext4, with crawler, browser and packaging workloads stopped. Generated files were removed after each run.

Each process writes a 127,596,336-byte (121.685 MiB) archive incrementally before timing. Its records have distinct List storage keys/positions, reversed sparse IDs, metadata, exact-content fingerprints, typed tag counts, metrics and Unicode custom-extraction values. Timing includes record decoding, SQLite insertion, identity validation/restoration and finalization. It excludes fixture generation, compilation, current-source copying, comparison-result materialization and result assertions. Verification queries the exact row count and selected endpoint IDs/List positions/payloads without hydrating the complete output.

| Archive input decoding | Staging elapsed, one run | Process peak RSS |
| --- | ---: | ---: |
| Complete `Vec<CrawlRecord>` followed by shared SQL staging | 16,445.803 ms | 144.270 MiB |
| Incremental record decoding into the same SQL staging | 17,331.341 ms | 13.508 MiB |

This matched baseline isolates whole-array allocation. It is **not** a timing of the historical HashSet/ID-vector implementation: both modes use the new SQL identity checks, which add writes. The result demonstrates lower peak process memory, not a latency improvement. Process RSS includes fixture generation and SQLite C allocations, but not filesystem page-cache memory. These are single-run observations, not medians or a general throughput guarantee.

Focused tests prove a record reaches SQLite while later JSON remains unread; preserve original IDs including zero and SQLite's upper boundary, List ordering and full typed payloads; retain comparison's intentional skipping of unused non-record sections; and remove private staging on duplicate identities, unsupported/missing/duplicate metadata, malformed/trailing JSON, read errors or late storage errors. The current crawl and saved source files remain unchanged. Existing comparison-source tests also retain WAL-aware read-only copying, private legacy migrations and source-deletion independence.

The archive workspace now holds one decoded record at a time, so a single unusually large field/record can still require substantial memory. Current Memory and in-memory SQLite sources still hydrate one complete record snapshot before staging; saved SQLite sources use SQL copying. At this checkpoint, full archive import and the legacy `compare_crawl_archive` command remained buffered. The full-import follow-up below removes the former input buffer; the legacy comparison command remains unchanged.

## Complete Schema-1 Archive Import: 50,000 Records and 1,000,001 Links

The native importer now decodes the complete archive with Serde into a private SQLite database on a blocking worker. Records reuse the comparison identity stager; images, references and frontier rows use bounded append transactions. Captures decode individually with the existing size/ownership validation. Links first enter a temporary SQLite table and are replayed after records, preserving captured status/depth even when the JSON puts links first. Consumed staging rows are deleted during replay so SQLite can reuse their pages; ordinary DELETE/DROP does not shrink the file, and some final freelist space may remain. Only a validated, checkpointed and closed database is published as a completed saved session.

Run the opt-in workload separately from browser/crawler tests:

```bash
cargo test --locked --release -p ferrous-frog-app archive_import::tests::archive_import_full_stream_workload --no-run
TMPDIR="$HOME/.cache" FF_IMPORT_RECORDS=50000 FF_IMPORT_EDGES=1000001 /usr/bin/time -f 'peak_rss_kib=%M elapsed_seconds=%e' target/release/deps/ferrous_frog-<hash> --ignored --exact archive_import::tests::archive_import_full_stream_workload --nocapture
```

The local run on 2026-09-14 used an optimized build from the working tree based on `9573ec8`, including the full-import implementation, raw archive restore helpers and the then-uncommitted pagination diagnostic changes subsequently committed as `79a0edf`. It ran alone after the frontend Chrome smoke completed, using a private directory under `$HOME/.cache` on `/dev/nvme0n1p2` (ext4). Import behavior was unchanged between this run and the final checks; later changes only addressed lint, added negative test assertions and updated documentation.

| Archive collection | Verified rows |
| --- | ---: |
| Records | 50,000 |
| Link edges | 1,000,001 |
| Images | 50,000 |
| Page references | 50,000 |
| Page captures | 50,000 |
| Queued frontier items | 50,000 |
| Seen frontier keys | 100,000 |

| Measurement | Result |
| --- | ---: |
| Input JSON bytes | 496,945,043 |
| Decode, private staging and completed-session publication | 45,124.384 ms |
| Peak process RSS | 17,236 KiB (16.832 MiB) |
| Entire test process | 45.88 s |

The workload generates its input item by item before starting the import timer. Records contain reversed sparse IDs, List occurrence keys/positions, Unicode metadata and a captured inlink count deliberately independent of edge counts. Links carry captured status/depth values that disagree with matching records. Images, references, retained page bodies, queued order and seen keys populate every schema-1 collection. After timed session publication, SQL checks all seven complete counts, the final link ID/status/Unicode anchor and an endpoint record's preserved inlink count. A smaller 100-record/1,205-link fixture passed before the large run.

This is one local observation, not a median, latency guarantee or before/after speedup comparison. The timer excludes fixture generation, compilation and result assertions; it includes opening the session index and final database publication/opening, but does not drive the Tauri IPC or frontend session activation. Peak RSS covers the entire test process, including input generation, SQLite allocations and verification; it excludes filesystem page-cache memory. The input file, SQLite dataset and private staging still require disk space.

Import memory is bounded by individual records/captures and batches of at most 256 evidence/frontier rows, rather than by the total array sizes. Individual fields can still be large; capture limits remain the existing 1 MiB per body/text value and 512 headers / 64 KiB. Older optional reference/capture fields remain optional. Duplicate record/evidence identities or normalized record keys now fail instead of being silently renumbered or overwritten. Stored record inlink counts and captured link metadata survive; image size/oversized and first-inlink annotations retain their existing query-derived behavior. At that checkpoint, archive export frontier hydration remained; the follow-up below removes it. Memory export/query snapshots and the legacy immediate archive comparison buffer remain separate limitations.


## Archive Resume State: 50,000 Queued and 1,000,000 Seen

Schema-1 archive export now writes the saved frontier without creating full queue/seen vectors. Memory serializes its borrowed checkpoint under one read lock. SQLite decodes one queued item or seen key at a time, in the same order as the previous loader, and holds one read transaction across both arrays and the completed count. This snapshot covers the frontier section, not the complete paged archive.

Run the two modes in separate processes so the hydrated baseline does not contaminate the streaming peak RSS:

```bash
cargo test --release --locked -p ferrous-frog-storage frontier_json_export_workload --no-run
FF_FRONTIER_EXPORT_SEEN=1000000 FF_FRONTIER_EXPORT_MODE=hydrated /usr/bin/time -f 'peak_rss_kib=%M elapsed_seconds=%e' target/release/deps/ferrous_frog_storage-<hash> --ignored --exact native_exports::tests::frontier_json_export_workload --nocapture
FF_FRONTIER_EXPORT_SEEN=1000000 FF_FRONTIER_EXPORT_MODE=streamed /usr/bin/time -f 'peak_rss_kib=%M elapsed_seconds=%e' target/release/deps/ferrous_frog_storage-<hash> --ignored --exact native_exports::tests::frontier_json_export_workload --nocapture
```

Measured on 2026-09-19 from the working tree based on `6299b66`, using Rust 1.98.1 and the optimized test build. Both modes ran serially before CI, with the database and output under `/tmp` (tmpfs). SQL generates the fixture without allocating a Rust collection: 50,000 queued List occurrences of one request URL with distinct storage keys/positions, varying depth/sitemap flags, 1,000,000 ordered seen keys, and a completed count of 12,345. There are no crawl records, links, images or captures.

| Measurement | Full frontier hydration | Streamed frontier |
| --- | ---: | ---: |
| Median serialization/write time, 3 samples | 127.714 ms | 102.917 ms |
| Peak process RSS | 88,580 KiB | 10,460 KiB |
| Complete JSON bytes | 45,352,825 | 45,352,825 |
| Same-build output digest | `ce541aafa0e40990` | `ce541aafa0e40990` |

The baseline calls the existing fallible full loader, then Serde; the new mode calls the same storage writer used by desktop archive export. Both use a buffered file writer and include flush, but not `fsync`, in the timer. Peak RSS covers fixture generation, all three exports and verification; it excludes tmpfs/page-cache storage. The digest is Rust's standard-library non-cryptographic hasher over fixed-size read chunks, used only for byte parity within this build. No complete output is loaded during verification.

This local workload reduced peak process RSS by approximately 88%; the timing samples are observations, not a physical-disk or whole-desktop performance guarantee. The SQL fixture, retained database, output file and any individually large URL still consume storage/memory. Scheduler startup and interrupted-discovery hydration, Memory record snapshots, and consistency against external writes across other archive sections remain separate work.

Focused storage tests preserve empty/null backend behavior, queue-only/seen-only checkpoints, Unicode and List occurrence evidence. A WAL writer changes all frontier tables during export to verify one snapshot and successful lock/transaction release after writer errors. Desktop archive tests verify valid early rows reach the writer before later queue/seen decode failures, reject partial publication, preserve earlier exports, and retain complete archive round trips.

Final verification passed `make ci`: 586 default workspace tests, Clippy, formatting, frontend build/browser smoke, both offline report packages, three rendering unit tests, seven serialized real-Chrome fixtures and version/release guards. `cargo build --locked -p ferrous-frog-app` also passed. This Linux run used privately extracted Ubuntu GTK/WebKit development packages via pkg-config/library paths; no host packages were installed. Native installer/GUI checks were not repeated for this storage-only change.


## Memory Query Payload Allocation

Memory summaries now borrow stored records. Grid and link queries filter/sort borrowed candidates and clone only the returned window; anchor aggregation also borrows its input. First-inlink search and sorting use the maintained source index rather than stale imported annotation fields. Existing SQLite/List/alias/filter tests preserve observable query behavior.

```bash
cargo test --locked -p ferrous-frog-storage --test memory_query_allocations -- --nocapture
```

The isolated integration-test executable measures Rust allocation requests after fixture creation. Its 64 records hold 8 MiB of custom-extraction values, and 64 link occurrences hold another 8 MiB of anchor text. It requests a summary, zero-row and one-row searched/sorted grid windows, and one searched/sorted link. Each operation must allocate less than 1 MiB while preserving exact totals, the last row/link and full returned values. Before the borrowing change, the regression failed because summaries and even empty result windows copied all retained record payloads; the link regression independently caught full edge copying. This is a regression ceiling, not a general per-query memory limit.

The measurement counts allocation/reallocation request sizes, not live bytes or peak process RSS; it excludes stored fixture data, filesystem cache and non-Rust allocations. The test contains one synchronous case to avoid interference from other test workloads. Candidate lists, audit/reference/duplicate indexes, matching work and anchor groups still scale with the crawl and field lengths. Each query holds a read lock until it completes, so Memory writers wait through filtering, sorting and audit work. Actual concurrent writer latency and desktop frame responsiveness require separate measurements; SQLite remains the default desktop backend.

## Memory Link Endpoint Updates

Memory now indexes source and target URL aliases when appending an edge. Upserting a record updates only edges matching its current aliases, without parsing unrelated endpoints again. The append path reuses its parsed aliases for record lookup and first-inlink indexing. The indexes retain URL keys and edge positions proportional to endpoint-alias memberships; they do not make the graph constant-memory.

```bash
cargo test --locked -p ferrous-frog-storage memory_endpoint_updates -- --nocapture
cargo test --locked -p ferrous-frog-storage memory_endpoint_update_workload -- --ignored --nocapture
```

On 2026-09-20, the failing work-count regression performed 112 alias expansions with 50 unrelated edges before the change. The indexed implementation performs 12 with either 50 or 500 edges. A second workload updates one existing record attached to four self-edges, with 50 or 5,000 unrelated edges, and checks every edge after each of seven samples:

| Unrelated edges | Before alias expansions | After alias expansions | Before median | After median |
| ---: | ---: | ---: | ---: | ---: |
| 50 | 123 | 15 | 0.479 ms | 0.054 ms |
| 5,000 | 10,023 | 15 | 37.022 ms | 0.054 ms |

These are local debug-profile public-upsert samples, excluding fixture creation and assertions. The baseline may have overlapped native/build work, so elapsed times are illustrative observations; deterministic work counts establish removal of the unrelated-edge scan. They do not establish a whole-crawl throughput gain or isolated index memory cost. A scan oracle and Memory/SQLite fixtures preserve latest-upsert status/depth, earliest-record lookup on edge append, shared redirect/List identities, changing aliases, unknown status overwrites and clear/reuse. Per-record progress semantics and SQLite updates are unchanged.

## 10,000-Page Live Crawl Follow-Up

The existing localhost fixture passed all four cases on 2026-09-20 with `FERROUS_CRAWLER_PAGES=10000` and `FERROUS_CRAWLER_QUERIES=1`. Use the same release-build command as the 5,000-page run, changing only the page-count environment variable. The copied test executable SHA-256 was `bd9998a03adbe669c44ee7bc961323164d1d33cb60be23037686d7e125df5dc0`, built from the working tree based on `6299b66` after borrowed Memory query windows, endpoint indexing and the new pagination/AMP/doctype audits. SQLite used a fresh private directory on the same physical NVMe volume, WAL and `synchronous=NORMAL`.

| Backend / case | Elapsed | Polls | Median poll | p95 poll | Maximum poll |
| --- | ---: | ---: | ---: | ---: | ---: |
| Memory, uninterrupted | 46.450 s | 30 | 102.354 ms | 142.565 ms | 142.823 ms |
| Memory, stop/resume | 46.380 s | 30 | 99.292 ms | 136.762 ms | 139.163 ms |
| SQLite, uninterrupted | 351.902 s | 217 | 115.550 ms | 255.019 ms | 1,252.763 ms |
| SQLite, stop/reopen/resume | 348.946 s | 216 | 110.764 ms | 216.372 ms | 682.382 ms |

Each completed case retained exactly 10,300 records and 71,100 edges, including 100 planted failures, 100 redirects and 100 robots-blocked URLs. HTTP requests totaled 10,301 uninterrupted or 10,309 interrupted, with no disallowed, excluded or stripped-query request. Delayed-handler overlap peaked at seven in Memory and six in SQLite, below the configured concurrency of eight. Stop occurred at exactly 2,500 records; reopening/resuming preserved completed IDs and ended with an empty frontier. All bounded grid/link/recovery polls passed. Memory reached Stop in 3.408 seconds and SQLite in 16.447 seconds; completed interrupted timings include Stop, reopen and Resume.

The whole test process, including final assertions, took 795.81 seconds and peaked at 146,980 KiB RSS (143.54 MiB). This includes both backends, endpoint/first-inlink indexes, mock-server request records, verification snapshots and allocator retention; it does not isolate index cost or desktop memory. Compilation, repository tests, browser fixtures and packaging did not overlap the timed run. Lightweight source/document work and one read-only SQLite count check occurred while it ran; host services remained active, caches were not flushed and CPU frequency was not fixed. The retained local log is `/tmp/ferrous-roadmap-10k/run.log`.

This is one observation per case, not a median across unchanged-build runs. It is not a paired performance comparison with the older 5,000-page build: dataset size, query borrowing, endpoint indexing and audit fields all changed. It extends live-crawl correctness coverage to 10,000 pages; larger/denser graphs, repeated trials, other devices, rendered crawls and actual desktop frame/input responsiveness remain open. At this point, SQLite progress summaries still rescanned the dataset after record changes; the incremental follow-up below addresses local refreshes. Scheduler startup/interrupted-discovery hydration remains size-dependent.

The final working tree also passed `make ci`: 633 default workspace tests, formatting/Clippy/version/release checks, the frontend build and browser smoke, complete offline single/follow-up report fixtures, three rendering unit tests and seven serialized Chrome fixtures. An embedded-assets debug desktop build passed the expanded [native crawl/reopen/report/export/quit smoke](NATIVE_TESTING.md). These checks ran separately from the timed workload.

## Incremental SQLite Progress Summaries

SQLite now journals locally changed record IDs in connection-local TEMP tables. Transactional triggers count local revisions independently of the persistent record revision. A matching revision permits refreshing only those IDs; an external record commit causes a complete snapshot rebuild. Deleted IDs remove their previous contribution. The journal and revision changes roll back with their records, and a failed refresh discards its partial cache before retrying. Ordinary counters reuse grid predicates; normalized title/description/H1/H2 frequencies and eligible near-duplicate cluster frequencies maintain exact member counts across the one/two-member boundary.

The cache retains one predicate bitmask, four normalized metadata strings and an optional cluster ID per record, plus group frequencies. It does not decode or retain full records, custom fields, captures or HTML. Memory nevertheless grows with record count, distinct metadata and string lengths. First use, reopening and external record changes still scan narrow audit metadata; reference/exact-duplicate graphs and filtered grid queries retain their existing behavior. Every completed crawl record still emits its current progress counts.

```bash
cargo test --locked -p ferrous-frog-storage summary_tests -- --nocapture
cargo test --release --locked -p ferrous-frog-storage sqlite_progress_summary_workload -- --ignored --nocapture
```

The deterministic changed-record regression starts with 256 records. Replacing one record and updating its inlink count previously invoked text normalization 1,024 times; incremental refresh invokes it four times, with a ceiling of eight to reject unrelated-row work. Initial summary visits in the existing 128-record fixture fall from 768 to 128. Fixtures also cover Unicode and empty metadata, List occurrences, failed/non-HTML/incomplete eligibility, group membership removal, changed/reused IDs, local rollback, external writes followed by local writes, a commit during a summary read, malformed audit data and successful retry, clear/reuse and unrelated corrupt payloads. Sitemap membership updates now include constant journal work: 334 VM steps for both 500 and 2,000 records; the regression retains its size-scaling bound.

Two copied release executables were run serially on 2026-09-20 using the existing 10,000-record workload. Seven samples follow a warm summary, each after a local response-time update to one record. Fixture construction, warm-up and writes are outside the timed interval; each result must match the warm summary. No Ferrous Frog compilation, browser or other repository tests overlapped these samples; unrelated host activity was not controlled.

| Metadata case | Previous median | Incremental median | Incremental min–max |
| --- | ---: | ---: | ---: |
| Mixed | 34.731 ms | 0.154 ms | 0.145–0.198 ms |
| Repeated | 33.373 ms | 0.149 ms | 0.143–0.191 ms |
| Empty/Unicode whitespace | 32.061 ms | 0.152 ms | 0.147–0.201 ms |

These are local cached-dataset observations, not a whole-crawl speedup or a bound on first-use/external-write rebuilds. The timed mutation changes a failed row's response time; the separate regression exercises successful HTML metadata and group changes. Temporary logs: `/tmp/ff-summary-paired-before.log` and `/tmp/ff-summary-paired-after.log`.

The same 10,000-page live fixture then passed using copied executable SHA-256 `ddb045f851898d7663f1bca2f1847198d06d19cebf32c14337ba6d5a04ebf16b`, a fresh private SQLite directory on the physical NVMe volume, and concurrent bounded queries:

| Backend / case | Elapsed | Polls | Median poll | p95 / maximum poll |
| --- | ---: | ---: | ---: | ---: |
| Memory, uninterrupted | 49.095 s | 31 | 104.527 ms | 162.022 / 174.883 ms |
| Memory, stop/resume | 64.913 s | 41 | 135.118 ms | 237.874 / 313.473 ms |
| SQLite, uninterrupted | 18.845 s | 12 | 100.422 ms | 151.405 / 151.405 ms |
| SQLite, stop/reopen/resume | 17.502 s | 12 | 68.854 ms | 131.132 / 131.132 ms |

All four cases retained 10,300 records and 71,100 edges with the same planted failures, redirects, robots exclusions and empty final frontier. Fresh requests totaled 10,301; interrupted requests totaled 10,309 in Memory and 10,308 in SQLite, within the fixture's bounded allowance for requests already in flight at Stop. Peak delayed-handler overlap was seven. Stop occurred at exactly 2,500 records in 3.484 seconds for Memory and 4.365 seconds for SQLite; completed IDs survived resume. The process took 152.59 seconds and peaked at 135,660 KiB RSS (132.48 MiB), including both backends and fixture/verification data. This does not isolate the new metadata cache's memory cost.

The previous 10,000-page observations above were 351.902/348.946 seconds for SQLite, compared with 18.845/17.502 seconds here. These are individual runs of successive builds, not an interleaved unchanged-host distribution: unrelated host work was active, and Memory's unchanged path also varied. No other Ferrous Frog build/test/browser task overlapped this run. Deterministic work-count tests establish removal of full progress-summary rescans; the live fixture establishes retained behavior under concurrent queries. Denser graphs, repeated trials, other devices and actual desktop responsiveness remain open. Log: `/tmp/ff-incremental-summary-10k/run.log`.

The updated working tree passed `make ci`: 636 default workspace tests, formatting, Clippy, version/release guards, frontend/browser checks, complete offline single/follow-up report fixtures, three rendering unit tests and seven serialized real-Chrome fixtures. The embedded-assets debug desktop build and full native crawl/reopen/report/export/quit smoke also passed. Native evidence and platform limits are recorded in [NATIVE_TESTING.md](NATIVE_TESTING.md). These checks ran separately from the timed workloads.

### Three trials with the unchanged crawler executable

Two more serial runs used the same copied executable (SHA-256 `ddb045f851898d7663f1bca2f1847198d06d19cebf32c14337ba6d5a04ebf16b`), fixture, page/query settings and fresh private NVMe database directories. All twelve backend/case combinations across the three runs passed exact record/edge totals, robots and exclusion constraints, bounded queries and completed-ID preservation through Stop/reopen/Resume.

| Backend / case | Run 1 | Run 2 | Run 3 | Median | Range |
| --- | ---: | ---: | ---: | ---: | ---: |
| Memory, uninterrupted | 49.095 s | 48.852 s | 60.549 s | 49.095 s | 48.852–60.549 s |
| Memory, stop/resume | 64.913 s | 52.702 s | 69.001 s | 64.913 s | 52.702–69.001 s |
| SQLite, uninterrupted | 18.845 s | 24.013 s | 19.013 s | 19.013 s | 18.845–24.013 s |
| SQLite, stop/reopen/resume | 17.502 s | 20.391 s | 25.204 s | 20.391 s | 17.502–25.204 s |

Whole-process peak RSS was 135,660 / 158,172 / 144,708 KiB (132.48 / 154.46 / 141.32 MiB). The runs include verification allocations and both backends, so this range does not isolate cache memory. Host work was uncontrolled and caches were not flushed; the unchanged Memory path demonstrates material timing variation. Three samples document local repeatability without establishing a statistical latency guarantee or a paired before/after speedup distribution. No other Ferrous Frog build, test or browser workload overlapped these timed runs. The extra logs are `/tmp/ff-incremental-summary-10k/run-2.log` and `run-3.log`. Denser graphs, larger live crawls, other devices and desktop frame/input responsiveness remain open.

### One-million-record storage follow-up

The existing `sqlite_large_synthetic_storage_benchmark` passed again with `BENCH_URLS=1000000` after the incremental-summary change. Copied release executable SHA-256: `97774253edab9cd0d850abfcc3c665a697ed86c18db04e55e22cd49ca08d8a4d`. SQLite used another fresh private directory on physical NVMe. The fixture retains planted failures, repeated titles, distinct descriptions/headings, hreflang pairs and canonical targets, and requests ten-row query windows.

| Operation | Observed time | Matching rows where reported |
| --- | ---: | ---: |
| Insert 1,000,000 records | 201.70 s | 1,000,000 |
| First full summary | 13.50 s | 1,000,000 |
| Last ten rows, cached summary | 59.41 ms | 1,000,000 |
| Duplicate-title page | 2.60 s | 989,690 |
| Regex page | 292.09 ms | 500,000 |
| Hreflang return-link page | 4.76 s | 9,896 |
| Hreflang canonical-target page | 5.02 s | 9,896 |

The database was 819,830,784 bytes. The complete process took 228.51 seconds with peak RSS 1,337,872 KiB (about 1.276 GiB), including SQLite, progress metadata/group caches, canonical/reference work and hreflang query temporaries. This is not an isolated measure of progress-cache memory or a desktop/live-crawl memory budget. First-summary time includes full reference/exact-duplicate preparation, not just progress counters. Other host activity was uncontrolled; no Ferrous Frog build/browser/test ran concurrently, apart from one read-only fixture-count check during insertion. The private database was removed on success; log: `/tmp/ff-summary-million.log`.

This run verifies the current implementation at the existing one-million-record storage scale; the older measurements near the start of this document used a different feature/schema state, so they are not a paired baseline for this change. Initial hydration and global audit/query costs remain substantial. A million-page live HTTP/desktop crawl, denser graphs, cache-memory isolation and responsiveness on other devices remain unverified.
