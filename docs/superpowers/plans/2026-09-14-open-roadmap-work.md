# Open roadmap implementation

Continue through independent open work without treating completion of one item as the end of the roadmap.

- [x] Preserve comparison identities for original request URLs and repeated List occurrences across saved, archive and compatibility commands; show redirect destination changes and occurrence evidence in the workspace and CSV.
- [x] Add opt-in bounded page evidence, separate from grid records: client-observed response headers, raw HTML, rendered HTML and visible text. Default all retention off, redact credential-bearing headers, preserve exact source occurrences and report truncation.
- [x] Add bounded selected-record inspection, filtered streaming exports and archive round trips for retained evidence. Preserve the active crawl and existing export publication guarantees.
- [ ] Expand mixed active-crawl/query/frontier measurements and use the results to choose the next performance improvement.
- [ ] Exercise native desktop workflows locally where available; keep unavailable operating-system and signing checks explicit.
- [ ] Continue remaining audit/configuration work after these prerequisites and update the roadmap with verified outcomes rather than blanket completion claims.

The existing HTTP client normalizes response headers when decoding compressed bodies. Retention therefore records client-observed headers, not complete wire headers or authenticated request headers. Bodies are bounded per captured representation (default 1 MiB, configurable 1 KiB–1 MiB); headers have a separate 64 KiB ceiling. Raw and rendered source retention are independent, and incomplete/failed captures must remain distinguishable from complete empty values.

Backend records and source bodies remain outside frontend grid state. Existing Memory/SQLite query models, automatic desktop sessions, robots/pacing controls and archive schema compatibility remain intact. Every increment receives focused fixtures, followed by integrated Rust and browser checks; Chrome rendering fixtures run serially.

Verification: 482 default-feature workspace tests, Clippy, three rendering unit tests, seven serial Chrome fixtures, frontend build and browser smoke passed. The Linux native driver/display preflight and desktop build passed. The first native attempt opened the library but failed clearing the URL input before crawling; the complete workflow remains unverified.

Work paused at the user-requested interim commit. Remaining benchmark, native and audit/configuration items above are intentionally open.
