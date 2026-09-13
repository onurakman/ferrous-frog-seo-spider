# Comparison workspace

Replace the limited comparison dialog with a full viewport workspace. Preserve the active crawl and reuse the existing content/response hash classification.

- [x] Prepare private record-only SQLite snapshots once for saved crawls or an archive and the current crawl. Never migrate source databases or register temporary library sessions.
- [x] Materialize complete comparison results once, then query bounded pages with search, change/field filters, sorting, complete summary counts, selected-record detail and streaming filtered CSV export.
- [x] Add an opaque comparison lifecycle that discards superseded preparation and closes temporary data without changing the crawl workspace.
- [x] Build the accessible, responsive comparison workspace with a virtualized table and side-by-side captured record details.
- [x] Verify source immutability, results beyond 1,000 rows, filters, details, export, stale requests and browser interactions; update capability and roadmap documentation.

This increment keeps the existing representative policy: exact final URL, greatest List position (or record ID), then record ID. Original URL and repeated List occurrence identity remain separate roadmap work. Record-only snapshots omit first-inlink fields derived from link edges. Archives are parsed and staged once in the backend; their initial record-vector allocation remains a documented scale ceiling.

Validation completed: 471 default workspace Rust tests, Clippy with warnings denied, formatting, version/release checks, graph/schedule checks, production web build, full browser smoke, and `make test-rendering` (3 rendering unit tests plus 7 serialized Chrome fixtures). Browser checks include more than 1,000 changes, server filters/sorting, exact selected-record details, full filtered export, expanded details, narrow side-by-side columns, focus restoration, stale responses and serialized rapid close/reopen preparation.
