# Open roadmap implementation

Continue through independent open work without treating completion of one item as the end of the roadmap.

- [x] Preserve comparison identities for original request URLs and repeated List occurrences across saved, archive and compatibility commands; show redirect destination changes and occurrence evidence in the workspace and CSV.
- [x] Add opt-in bounded page evidence, separate from grid records: client-observed response headers, raw HTML, rendered HTML and visible text. Default all retention off, redact credential-bearing headers, preserve exact source occurrences and report truncation.
- [x] Add bounded selected-record inspection, filtered streaming exports and archive round trips for retained evidence. Preserve the active crawl and existing export publication guarantees.
- [x] Expand the local crawler measurement through 2,000 pages with concurrent bounded grid/link/recovery queries on physical NVMe, including Stop/reopen/resume. Ordinary Stop now reuses the durable frontier. Larger and denser datasets, incremental summaries and desktop responsiveness remain in ROADMAP.md.
- [x] Exercise the Linux embedded-assets desktop through crawl, SQLite reopen, saved audit-report creation/reopen/export and clean quit. The normal Linux Make packaging route and privately extracted payloads also passed with the documented private build-tool prerequisites. Windows/macOS, host installation and signing remain explicit roadmap gaps.
- [ ] Continue remaining audit/configuration work after these prerequisites and update the roadmap with verified outcomes rather than blanket completion claims.

The existing HTTP client normalizes response headers when decoding compressed bodies. Retention therefore records client-observed headers, not complete wire headers or authenticated request headers. Bodies are bounded per captured representation (default 1 MiB, configurable 1 KiB–1 MiB); headers have a separate 64 KiB ceiling. Raw and rendered source retention are independent, and incomplete/failed captures must remain distinguishable from complete empty values.

Backend records and source bodies remain outside frontend grid state. Existing Memory/SQLite query models, automatic desktop sessions, robots/pacing controls and archive schema compatibility remain intact. Every increment receives focused fixtures, followed by integrated Rust and browser checks; Chrome rendering fixtures run serially.

Verification: 482 default-feature workspace tests, Clippy, three rendering unit tests, seven serial Chrome fixtures, frontend build and browser smoke passed. The Linux native driver/display preflight and desktop build passed. The first native attempt opened the library but failed clearing the URL input before crawling; the complete workflow remains unverified.

Work paused at the user-requested interim commit. Remaining benchmark, native and audit/configuration items above are intentionally open.

Continuation on 2026-09-14: the complete Linux debug workflow subsequently passed; the first-attempt failure above was fixed. Verified follow-ups include observed AMP canonical-return warnings, all pagination declaration capture with bounded inspection, streamed complete HTML/link evidence, archive comparison record staging, and concurrent-query crawler measurements. Commits `e8d1e29`, `2940098`, `2f189c0`, `9c9682b`, `64cf05a` and `7af74a1` record these checkpoints. Focused Rust, Clippy, build and browser checks passed for their respective changes; the earlier combined-check counts do not describe these later commits. Per-target pagination diagnostics, streamed full archive import and the remaining ROADMAP.md items continue.

The final pre-pause checkpoint also completes measured per-target pagination diagnostics and streamed full schema-1 import. Final verification passed `make test-ui` and `make ci -o test-ui`: 583 default workspace tests, three rendering unit tests, seven real-Chrome fixtures, Clippy, formatting, build, offline packages and release/version checks. Larger remaining roadmap items stay open; work pauses at the user's request after the current commits.
