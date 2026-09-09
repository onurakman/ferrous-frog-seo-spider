use super::*;

const NEXT: &str = "paginationNextToError";
const PREV: &str = "paginationPrevToError";
const NEXT_LOOP: &str = "paginationNextLoop";
const PREV_LOOP: &str = "paginationPrevLoop";
const NEXT_NON_RECIPROCAL: &str = "paginationNextNonReciprocal";
const PREV_NON_RECIPROCAL: &str = "paginationPrevNonReciprocal";

fn url(path: &str) -> String {
    format!("https://example.test/{path}")
}

fn page(path: &str) -> CrawlRecord {
    let mut row = CrawlRecord::pending(url(path), 0);
    row.status_code = Some(200);
    row.content_type = Some("text/html; charset=utf-8".into());
    row.indexability = "Indexable".into();
    row.indexability_status = "Indexable".into();
    row
}

fn source(path: &str, next: Option<&str>, prev: Option<&str>) -> CrawlRecord {
    let mut row = page(path);
    row.rel_next = next.map(url);
    row.rel_prev = prev.map(url);
    row
}

fn failed(path: &str, status: Option<u16>) -> CrawlRecord {
    let mut row = page(path);
    row.status_code = status;
    row.error = Some("Request failed".into());
    row
}

fn occurrence(mut row: CrawlRecord, position: u32) -> CrawlRecord {
    row.storage_key = format!("list:{position}:{}", row.url);
    row.list_position = Some(position);
    row
}

fn redirect(mut row: CrawlRecord, final_path: &str, hops: &[&str]) -> CrawlRecord {
    row.final_url = url(final_path);
    row.redirect_chain = hops
        .iter()
        .map(|path| RedirectHop {
            url: url(path),
            status_code: 301,
            location: Some(url(final_path)),
            dns_lookup_time_ms: None,
            tcp_connect_time_ms: None,
            tls_handshake_time_ms: None,
            ttfb_ms: None,
            elapsed_ms: None,
        })
        .collect();
    row
}

fn query(name: &str) -> GridQuery {
    GridQuery {
        view: serde_json::from_value(serde_json::json!(name))
            .expect("Pagination audit view must be supported"),
        sort_by: Some("url".into()),
        ..GridQuery::default()
    }
}

fn keys(response: &GridResponse) -> Vec<&str> {
    response
        .rows
        .iter()
        .map(|row| row.storage_key.as_str())
        .collect()
}

#[test]
fn pagination_reciprocity_reports_missing_and_observed_mismatched_returns_per_source() {
    let mut blank = page("blank-target");
    blank.rel_prev = Some(" \t ".into());
    let mut noindex = source("noindex-source", Some("noindex-target"), None);
    noindex.indexability = "Non-indexable".into();
    noindex.indexability_status = "Meta noindex".into();
    let mut noindex_target = page("noindex-target");
    noindex_target.indexability = "Non-indexable".into();
    noindex_target.indexability_status = "Meta noindex".into();
    let mut external = source("external-source", Some("external-target"), None);
    external.classification = UrlClassification::External;
    let mut external_target = page("external-target");
    external_target.classification = UrlClassification::External;
    let mut canonical_return = source("canonical-return", Some("canonical-target"), None);
    canonical_return.canonical = Some(url("canonical-source"));
    let rows = [
        source("next-missing", Some("next-target"), None),
        page("next-target"),
        occurrence(source("next-missing", Some("next-target"), None), 1),
        occurrence(source("next-missing", Some("unknown"), None), 2),
        source("prev-missing", None, Some("prev-target")),
        page("prev-target"),
        source("next-mismatch", Some("next-other"), None),
        source("next-other", None, Some("next-return")),
        source("next-return", Some("next-other"), None),
        source("prev-mismatch", None, Some("prev-other")),
        source("prev-other", Some("prev-return"), None),
        source("prev-return", None, Some("prev-other")),
        source("blank-source", Some("blank-target"), None),
        blank,
        noindex,
        noindex_target,
        external,
        external_target,
        source("canonical-source", Some("canonical-target"), None),
        source("canonical-target", None, Some("canonical-return")),
        canonical_return,
        source("linear-a", Some("linear-b"), None),
        source("linear-b", Some("linear-c"), Some("linear-a")),
        source("linear-c", None, Some("linear-b")),
        source("self", Some("self#next"), Some("self#prev")),
        source("self-missing", Some("self-missing"), None),
        source("ring-a", Some("ring-b"), Some("ring-c")),
        source("ring-b", Some("ring-c"), Some("ring-a")),
        source("ring-c", Some("ring-a"), Some("ring-b")),
    ];
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for row in rows {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for (name, paths) in [
        (
            NEXT_NON_RECIPROCAL,
            vec![
                "blank-source",
                "canonical-source",
                "external-source",
                "next-mismatch",
                "next-missing",
                "next-missing",
                "noindex-source",
            ],
        ),
        (PREV_NON_RECIPROCAL, vec!["prev-mismatch", "prev-missing"]),
    ] {
        for response in [
            memory.query(query(name)),
            sqlite.try_query(query(name)).unwrap(),
        ] {
            assert_eq!(response.total, paths.len());
            assert_eq!(
                response
                    .rows
                    .iter()
                    .map(|row| row.url.clone())
                    .collect::<Vec<_>>(),
                paths.iter().map(|path| url(path)).collect::<Vec<_>>()
            );
            assert_eq!(
                serde_json::to_value(response.summary).unwrap()[name],
                paths.len()
            );
        }
        for (offset, search, segment) in [
            (0, None, None),
            (1, None, None),
            (0, Some("mismatch"), None),
            (0, None, Some("missing|noindex")),
        ] {
            let request = GridQuery {
                offset,
                limit: 1,
                global_search: search.map(str::to_string),
                segment_pattern: segment.map(str::to_string),
                segment_regex: true,
                ..query(name)
            };
            let expected = memory.query(request.clone());
            let actual = sqlite.try_query(request).unwrap();
            assert_eq!(actual.total, expected.total);
            assert_eq!(keys(&actual), keys(&expected));
        }
    }
    for name in [NEXT_LOOP, PREV_LOOP] {
        let request = GridQuery {
            global_search: Some("ring-".into()),
            ..query(name)
        };
        assert_eq!(memory.query(request.clone()).total, 3);
        assert_eq!(sqlite.try_query(request).unwrap().total, 3);
    }
}

#[test]
fn pagination_reciprocity_leaves_unobserved_or_ineligible_evidence_unknown() {
    let mut blocked = page("blocked");
    blocked.status_code = None;
    blocked.error = Some("Blocked by robots.txt".into());
    let mut incomplete = page("incomplete");
    incomplete.indexability_status = "Response body incomplete".into();
    let mut image = page("image");
    image.content_type = Some("image/png".into());
    let mut limit = redirect(failed("limit", None), "unrequested", &["limit"]);
    limit.error = Some("Redirect limit exceeded".into());
    let mut rows = vec![
        blocked,
        incomplete,
        image,
        failed("failed", None),
        failed("error", Some(404)),
        failed("server", Some(503)),
        failed("redirect-error", Some(302)),
        CrawlRecord::pending(url("pending"), 0),
        limit,
    ];
    for path in [
        "blocked",
        "incomplete",
        "image",
        "failed",
        "error",
        "server",
        "redirect-error",
        "pending",
        "unknown",
        "unrequested",
    ] {
        rows.push(source(&format!("outbound-{path}"), Some(path), Some(path)));
        let target = format!("return-{path}");
        rows.push(source(&target, Some(path), Some(path)));
        rows.push(source(
            &format!("to-{target}"),
            Some(&target),
            Some(&target),
        ));
    }
    for (path, value) in [
        ("relative", "/other"),
        ("non-http", "mailto:a@example.test"),
    ] {
        let mut target = page(path);
        target.rel_next = Some(value.into());
        target.rel_prev = Some(value.into());
        rows.push(target);
        rows.push(source(&format!("to-{path}"), Some(path), Some(path)));
    }
    // Ineligible sources must not produce a missing-return warning either.
    rows.push(page("missing-return"));
    let mut invalid_source = source(
        "incomplete-source",
        Some("missing-return"),
        Some("missing-return"),
    );
    invalid_source.indexability_status = "Response body incomplete".into();
    rows.push(invalid_source);
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        for row in &rows {
            store.upsert(row.clone());
        }
        for name in [NEXT_NON_RECIPROCAL, PREV_NON_RECIPROCAL] {
            let response = store.query(query(name));
            assert_eq!(response.total, 0, "{name}: {:?}", keys(&response));
        }
    }
}

#[test]
fn pagination_reciprocity_preserves_redirect_aliases_and_list_source_context() {
    let watched = occurrence(source("source", Some("target"), Some("target")), 50);
    let check = |label: &str, rows: Vec<CrawlRecord>, expected: bool| {
        let memory = MemoryStore::new();
        let sqlite = SqliteStore::in_memory().unwrap();
        for row in rows {
            memory.upsert(row.clone());
            sqlite.try_upsert(row).unwrap();
        }
        for name in [NEXT_NON_RECIPROCAL, PREV_NON_RECIPROCAL] {
            let left = memory.query(query(name));
            let right = sqlite.try_query(query(name)).unwrap();
            assert_eq!(keys(&left), keys(&right), "{label} {name}");
            assert_eq!(
                left.rows
                    .iter()
                    .any(|row| row.storage_key == watched.storage_key),
                expected,
                "{label} {name}"
            );
        }
    };
    check(
        "successful source aliases override its older List failure",
        vec![
            occurrence(failed("source", Some(404)), 1),
            redirect(watched.clone(), "source-final", &["source", "source-hop"]),
            source("target", Some("source#fragment"), Some("source-hop")),
        ],
        false,
    );
    check(
        "an observed alternate return route reaches the same final page",
        vec![
            watched.clone(),
            source("target", Some("return-route"), Some("return-hop#fragment")),
            redirect(
                page("return-route"),
                "source",
                &["return-route", "return-hop"],
            ),
        ],
        false,
    );
    check(
        "direct return evidence outranks a competing redirect hop",
        vec![
            watched.clone(),
            source("target", Some("return"), Some("return")),
            redirect(page("return-route"), "source", &["return-route", "return"]),
            page("return"),
        ],
        true,
    );
    check(
        "return hop evidence outranks a competing final alias",
        vec![
            watched.clone(),
            source("target", Some("return"), Some("return")),
            redirect(page("return-route"), "source", &["return-route", "return"]),
            redirect(page("other-route"), "return", &["other-route"]),
        ],
        false,
    );
    for reciprocal in [false, true] {
        let return_path = reciprocal.then_some("source");
        check(
            "direct target evidence outranks final aliases",
            vec![
                watched.clone(),
                source("target", return_path, return_path),
                redirect(
                    source(
                        "other-route",
                        (!reciprocal).then_some("source"),
                        (!reciprocal).then_some("source"),
                    ),
                    "target",
                    &["other-route"],
                ),
            ],
            !reciprocal,
        );
    }
    check(
        "earliest observed List target supplies its captured return",
        vec![
            watched.clone(),
            occurrence(page("target"), 1),
            occurrence(source("target", Some("source"), Some("source")), 2),
        ],
        true,
    );
    check(
        "pending List target cannot hide its observed response",
        vec![
            watched.clone(),
            occurrence(CrawlRecord::pending(url("target"), 0), 1),
            occurrence(source("target", Some("source"), Some("source")), 2),
        ],
        false,
    );
    let mut via_final = watched.clone();
    via_final.rel_next = Some(url("target-final"));
    via_final.rel_prev = Some(url("target-final"));
    check(
        "return resolution uses the target's successful source context",
        vec![
            via_final,
            occurrence(failed("target", Some(404)), 1),
            redirect(
                occurrence(source("target", Some("target"), Some("target")), 2),
                "target-final",
                &["target"],
            ),
        ],
        true,
    );
    check(
        "successful aliases of a self-pagination target remain loop-only evidence",
        vec![
            watched.clone(),
            redirect(page("target"), "source", &["target"]),
        ],
        false,
    );
    check(
        "query strings remain distinct page identities",
        vec![
            watched.clone(),
            source("target", Some("source?other=1"), Some("source?other=1")),
            page("source?other=1"),
        ],
        true,
    );
    let mut root = watched.clone();
    root.url = "https://example.test".into();
    root.final_url = "https://example.test/".into();
    let mut root_target = page("target");
    root_target.rel_next = Some("https://EXAMPLE.test:443/#section".into());
    root_target.rel_prev = root_target.rel_next.clone();
    check(
        "normalized root and fragment aliases are reciprocal",
        vec![root, root_target],
        false,
    );
}

#[test]
fn pagination_reciprocity_reuses_bounded_evidence_and_observes_external_commits() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-pagination-reciprocity-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteStore::open(&path).unwrap();
    let writer = SqliteStore::open(&path).unwrap();
    store.try_upsert(source("a", Some("b"), Some("b"))).unwrap();
    store.try_upsert(page("b")).unwrap();
    writer
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET response_time_ms = 'invalid off-page payload' WHERE url = ?1",
            [url("b")],
        )
        .unwrap();
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    let progress = serde_json::to_value(store.try_progress_summary().unwrap()).unwrap();
    assert_eq!(progress[NEXT_NON_RECIPROCAL], 0);
    assert_eq!(progress[PREV_NON_RECIPROCAL], 0);
    assert_eq!(URL_ALIAS_EXPANSIONS.with(|count| count.get()), 0);
    assert!(store.try_records().is_err());
    for name in [NEXT_NON_RECIPROCAL, PREV_NON_RECIPROCAL] {
        let response = store
            .try_query(GridQuery {
                limit: 1,
                ..query(name)
            })
            .unwrap();
        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].url, url("a"));
    }
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    store
        .try_save_frontier_state(CrawlFrontierState::default())
        .unwrap();
    store.connection().unwrap().execute_batch(
        "CREATE TEMP TABLE reciprocity_probe (value INTEGER); INSERT INTO reciprocity_probe VALUES (1);",
    ).unwrap();
    {
        let mut conn = writer.connection().unwrap();
        let transaction = conn.transaction().unwrap();
        transaction
            .execute(
                "UPDATE crawl_records SET rel_prev = ?1 WHERE url = ?2",
                [url("a"), url("b")],
            )
            .unwrap();
        transaction.rollback().unwrap();
    }
    for name in [
        NEXT_NON_RECIPROCAL,
        PREV_NON_RECIPROCAL,
        NEXT_LOOP,
        "canonicalLoop",
    ] {
        store
            .try_query(GridQuery {
                limit: 0,
                ..query(name)
            })
            .unwrap();
    }
    assert_eq!(URL_ALIAS_EXPANSIONS.with(|count| count.get()), 0);
    let summary = serde_json::to_value(store.try_summary().unwrap()).unwrap();
    assert_eq!(summary[NEXT_NON_RECIPROCAL], 1);
    assert_eq!(summary[PREV_NON_RECIPROCAL], 1);
    writer
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET rel_prev = ?1 WHERE url = ?2",
            [url("a"), url("b")],
        )
        .unwrap();
    let summary = serde_json::to_value(store.try_summary().unwrap()).unwrap();
    assert_eq!(summary[NEXT_NON_RECIPROCAL], 0);
    assert_eq!(summary[PREV_NON_RECIPROCAL], 1);
    assert!(URL_ALIAS_EXPANSIONS.with(|count| count.get()) > 0);
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    let summary = serde_json::to_value(store.try_summary().unwrap()).unwrap();
    assert_eq!(summary[NEXT_NON_RECIPROCAL], 0);
    assert_eq!(summary[PREV_NON_RECIPROCAL], 1);
    drop(store);
    drop(writer);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn pagination_loops_are_directional_and_include_entering_sources_and_self_links() {
    let mut self_link = source("self", Some("self#next"), Some("self#prev"));
    self_link.canonical = Some(url("self"));
    let rows = [
        source("next-a", Some("next-b"), None),
        source("next-b", Some("next-a"), None),
        source("next-enter", Some("next-a"), None),
        occurrence(source("next-enter", Some("next-a"), None), 10),
        occurrence(source("next-enter", Some("unknown"), None), 11),
        source("prev-a", None, Some("prev-b")),
        source("prev-b", None, Some("prev-a")),
        source("prev-enter", None, Some("prev-a")),
        source("linear-a", Some("linear-b"), None),
        source("linear-b", Some("linear-c"), Some("linear-a")),
        source("linear-c", None, Some("linear-b")),
        self_link,
    ];
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for row in rows {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for (name, paths) in [
        (
            NEXT_LOOP,
            vec!["next-a", "next-b", "next-enter", "next-enter", "self"],
        ),
        (PREV_LOOP, vec!["prev-a", "prev-b", "prev-enter", "self"]),
    ] {
        for response in [
            memory.query(query(name)),
            sqlite.try_query(query(name)).unwrap(),
        ] {
            assert_eq!(response.total, paths.len());
            assert_eq!(
                response
                    .rows
                    .iter()
                    .map(|row| row.url.clone())
                    .collect::<Vec<_>>(),
                paths.iter().map(|path| url(path)).collect::<Vec<_>>()
            );
            assert_eq!(
                serde_json::to_value(response.summary).unwrap()[name],
                paths.len()
            );
        }
        for (offset, search, segment) in [
            (0, None, None),
            (1, None, None),
            (0, Some("enter"), None),
            (0, None, Some("enter|self")),
        ] {
            let request = GridQuery {
                offset,
                limit: 1,
                global_search: search.map(str::to_string),
                segment_pattern: segment.map(str::to_string),
                segment_regex: true,
                ..query(name)
            };
            let expected = memory.query(request.clone());
            let actual = sqlite.try_query(request).unwrap();
            assert_eq!(actual.total, expected.total);
            assert_eq!(keys(&actual), keys(&expected));
        }
    }
    assert_eq!(
        memory.summary().canonical_loop,
        0,
        "Self-canonicals remain valid"
    );
    assert_eq!(sqlite.try_summary().unwrap().canonical_loop, 0);
}

#[test]
fn pagination_loops_respect_observed_aliases_and_stop_at_unknown_or_ineligible_targets() {
    let mut blocked = source("blocked", Some("to-blocked"), Some("to-blocked"));
    blocked.status_code = None;
    blocked.error = Some("Blocked by robots.txt".into());
    let mut incomplete = source("incomplete", Some("to-incomplete"), Some("to-incomplete"));
    incomplete.indexability_status = "Response body incomplete".into();
    let mut image = source("image", Some("to-image"), Some("to-image"));
    image.content_type = Some("image/png".into());
    let mut error = source("error", Some("to-error"), Some("to-error"));
    error.status_code = Some(404);
    let mut invalid = source("invalid", None, None);
    invalid.rel_next = Some("/invalid".into());
    invalid.rel_prev = Some("mailto:invalid@example.test".into());
    let mut noindex = source("noindex", Some("noindex"), Some("noindex"));
    noindex.indexability = "Non-indexable".into();
    noindex.indexability_status = "Meta noindex".into();
    let mut limit = redirect(failed("limit", None), "unrequested", &["limit"]);
    limit.error = Some("Redirect limit exceeded".into());
    let rows = vec![
        blocked,
        incomplete,
        image,
        error,
        invalid,
        noindex,
        limit,
        CrawlRecord::pending(url("pending"), 0),
        source("to-blocked", Some("blocked"), Some("blocked")),
        source("to-incomplete", Some("incomplete"), Some("incomplete")),
        source("to-image", Some("image"), Some("image")),
        source("to-error", Some("error"), Some("error")),
        source("to-unknown", Some("unknown"), Some("pending")),
        source("to-unrequested", Some("unrequested"), Some("unrequested")),
        occurrence(failed("self-route", Some(404)), 1),
        redirect(
            occurrence(
                source("self-route", Some("self-hop#fragment"), Some("self-route")),
                2,
            ),
            "self-final",
            &["self-route", "self-hop"],
        ),
        // A different observed route that returns to the source's final page is self-pagination.
        source(
            "same-final",
            Some("same-final-route"),
            Some("same-final-route"),
        ),
        redirect(
            page("same-final-route"),
            "same-final",
            &["same-final-route"],
        ),
        occurrence(CrawlRecord::pending(url("list-target"), 0), 3),
        occurrence(
            source("list-target", Some("list-source"), Some("list-source")),
            4,
        ),
        source("list-source", Some("list-target"), Some("list-target")),
        // Direct failed evidence outranks a successful final alias and ends the path.
        failed("direct-error", Some(404)),
        redirect(
            source(
                "error-route",
                Some("to-direct-error"),
                Some("to-direct-error"),
            ),
            "direct-error",
            &["error-route"],
        ),
        source(
            "to-direct-error",
            Some("direct-error"),
            Some("direct-error"),
        ),
        // Direct successful evidence outranks a final alias with a self-loop.
        page("direct-ok"),
        redirect(
            source("loop-route", Some("loop-route"), Some("loop-route")),
            "direct-ok",
            &["loop-route"],
        ),
        source("to-direct-ok", Some("direct-ok"), Some("direct-ok")),
        // Observed hop evidence outranks a competing final alias with no continuation.
        redirect(
            source("hop-route", Some("hop-source"), Some("hop-source")),
            "hop-final",
            &["hop-route", "shared-hop"],
        ),
        redirect(page("alias-route"), "shared-hop", &["alias-route"]),
        source("hop-source", Some("shared-hop"), Some("shared-hop")),
        source(
            "query-is-distinct",
            Some("noindex?different=1"),
            Some("noindex?different=1"),
        ),
    ];
    let expected = [
        "hop-route",
        "hop-source",
        "list-source",
        "list-target",
        "loop-route",
        "noindex",
        "same-final",
        "self-route",
    ];
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        for row in &rows {
            store.upsert(row.clone());
        }
        for name in [NEXT_LOOP, PREV_LOOP] {
            let response = store.query(query(name));
            assert_eq!(response.total, expected.len());
            assert_eq!(
                response
                    .rows
                    .iter()
                    .map(|row| row.url.clone())
                    .collect::<Vec<_>>(),
                expected.iter().map(|path| url(path)).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn pagination_loop_long_chain_is_iterative_and_keeps_reverse_chain_acyclic() {
    const LENGTH: usize = 1_500;
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for index in 0..LENGTH {
        let path = format!("page-{index:04}");
        let next = format!(
            "page-{:04}",
            if index + 1 == LENGTH {
                index - 1
            } else {
                index + 1
            }
        );
        let prev = index
            .checked_sub(1)
            .map(|previous| format!("page-{previous:04}"));
        let row = source(&path, Some(&next), prev.as_deref());
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for store in [ActiveStore::Memory(memory), ActiveStore::Sqlite(sqlite)] {
        URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
        let response = store.query(GridQuery {
            limit: 1,
            ..query(NEXT_LOOP)
        });
        assert_eq!(response.total, LENGTH);
        assert_eq!(response.rows.len(), 1);
        assert_eq!(response.rows[0].url, url("page-0000"));
        assert_eq!(
            serde_json::to_value(&response.summary).unwrap()[NEXT_LOOP],
            LENGTH
        );
        assert_eq!(
            serde_json::to_value(&response.summary).unwrap()[PREV_LOOP],
            0
        );
        assert!(URL_ALIAS_EXPANSIONS.with(|count| count.get()) < 30 * LENGTH);
        assert_eq!(store.query(query(PREV_LOOP)).total, 0);
    }
}

#[test]
fn pagination_loop_cache_is_bounded_and_observes_committed_external_relation_edits() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-pagination-loop-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteStore::open(&path).unwrap();
    let writer = SqliteStore::open(&path).unwrap();
    for row in [
        source("a", Some("b"), Some("b")),
        source("b", Some("a"), Some("a")),
    ] {
        store.try_upsert(row).unwrap();
    }
    writer
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET response_time_ms = 'invalid off-page payload' WHERE url = ?1",
            [url("b")],
        )
        .unwrap();
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    let progress = serde_json::to_value(store.try_progress_summary().unwrap()).unwrap();
    assert_eq!(progress[NEXT_LOOP], 0);
    assert_eq!(progress[PREV_LOOP], 0);
    assert_eq!(URL_ALIAS_EXPANSIONS.with(|count| count.get()), 0);
    let response = store
        .try_query(GridQuery {
            limit: 1,
            ..query(NEXT_LOOP)
        })
        .unwrap();
    assert_eq!(response.total, 2);
    assert_eq!(response.rows[0].url, url("a"));
    assert!(
        store
            .try_query(GridQuery {
                offset: 1,
                limit: 1,
                ..query(NEXT_LOOP)
            })
            .is_err()
    );
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    store
        .try_save_frontier_state(CrawlFrontierState::default())
        .unwrap();
    for name in [NEXT_LOOP, PREV_LOOP, "canonicalLoop", NEXT] {
        store
            .try_query(GridQuery {
                limit: 0,
                ..query(name)
            })
            .unwrap();
    }
    {
        let mut conn = writer.connection().unwrap();
        let transaction = conn.transaction().unwrap();
        transaction
            .execute(
                "UPDATE crawl_records SET rel_next = NULL WHERE url = ?1",
                [url("b")],
            )
            .unwrap();
        transaction.rollback().unwrap();
    }
    assert_eq!(
        serde_json::to_value(store.try_summary().unwrap()).unwrap()[NEXT_LOOP],
        2
    );
    assert_eq!(URL_ALIAS_EXPANSIONS.with(|count| count.get()), 0);
    writer
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET rel_next = NULL WHERE url = ?1",
            [url("b")],
        )
        .unwrap();
    let summary = serde_json::to_value(store.try_summary().unwrap()).unwrap();
    assert_eq!(summary[NEXT_LOOP], 0);
    assert_eq!(summary[PREV_LOOP], 2);
    assert!(URL_ALIAS_EXPANSIONS.with(|count| count.get()) > 0);
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    let summary = serde_json::to_value(store.try_summary().unwrap()).unwrap();
    assert_eq!(summary[NEXT_LOOP], 0);
    assert_eq!(summary[PREV_LOOP], 2);
    drop(store);
    drop(writer);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn pagination_views_match_known_failures_and_preserve_source_occurrences() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut blocked = CrawlRecord::pending(url("blocked"), 0);
    blocked.status_text = "Blocked by robots.txt".into();
    blocked.error = Some("Blocked by robots.txt".into());
    let mut noindex = page("noindex");
    noindex.indexability = "Non-indexable".into();
    noindex.indexability_status = "Meta noindex".into();
    let mut incomplete = source("incomplete", Some("missing"), Some("server"));
    incomplete.indexability_status = "Response body incomplete".into();
    let mut image = source("image", Some("missing"), Some("server"));
    image.content_type = Some("image/png".into());
    let mut bad_source = source("failed-source", Some("missing"), Some("server"));
    bad_source.status_code = Some(404);
    let mut invalid = page("invalid");
    invalid.rel_next = Some("mailto:missing@example.test".into());
    invalid.rel_prev = Some("/missing".into());
    let mut external = failed("external", Some(404));
    external.classification = UrlClassification::External;
    for row in [
        failed("missing", Some(404)),
        failed("server", Some(503)),
        failed("connection", None),
        failed("redirect-error", Some(302)),
        failed("non-http-code", Some(600)),
        blocked,
        noindex,
        CrawlRecord::pending(url("pending"), 0),
        external,
        source("next", Some("missing#section"), None),
        source("prev", None, Some("server")),
        source("both", Some("connection"), Some("redirect-error")),
        source("same", Some("missing"), Some("missing")),
        source("external-source", Some("external"), None),
        occurrence(source("next", Some("missing#section"), None), 30),
        source("unknown", Some("unknown-next"), Some("pending")),
        source("non-errors", Some("blocked"), Some("noindex")),
        source("non-http", Some("non-http-code"), None),
        source("query-is-distinct", Some("missing?different=1"), None),
        invalid,
        incomplete,
        image,
        bad_source,
    ] {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for (name, paths) in [
        (
            NEXT,
            vec!["both", "external-source", "next", "next", "same"],
        ),
        (PREV, vec!["both", "prev", "same"]),
    ] {
        let expected: Vec<_> = paths.iter().map(|path| url(path)).collect();
        for response in [
            memory.query(query(name)),
            sqlite.try_query(query(name)).unwrap(),
        ] {
            assert_eq!(
                response.rows.iter().map(|row| &row.url).collect::<Vec<_>>(),
                expected.iter().collect::<Vec<_>>()
            );
            assert_eq!(response.total, paths.len());
            assert_eq!(
                serde_json::to_value(response.summary).unwrap()[name],
                paths.len()
            );
        }
        for (offset, search, segment) in [
            (0, None, None),
            (1, None, None),
            (0, Some("missing"), None),
            (0, None, Some("both|same")),
        ] {
            let query = GridQuery {
                offset,
                limit: 1,
                global_search: search.map(str::to_string),
                segment_pattern: segment.map(str::to_string),
                segment_regex: true,
                ..query(name)
            };
            let expected = memory.query(query.clone());
            let actual = sqlite.try_query(query).unwrap();
            assert_eq!(actual.total, expected.total);
            assert_eq!(keys(&actual), keys(&expected));
        }
    }
}

#[test]
fn pagination_aliases_prefer_observed_direct_requests_and_successful_self_evidence() {
    let mut blocked = CrawlRecord::pending(url("placeholder"), 0);
    blocked.error = Some("Blocked by robots.txt".into());
    let mut limit = redirect(
        failed("limit", None),
        "unrequested",
        &["limit", "limit-hop"],
    );
    limit.error = Some("Redirect limit exceeded".into());
    let mut root = failed("", Some(404));
    root.url = "https://example.test".into();
    let mut root_source = page("root-source");
    root_source.rel_next = Some("https://EXAMPLE.test:443/#section".into());
    let rows = vec![
        occurrence(failed("direct-ok", Some(404)), 1),
        occurrence(page("direct-ok"), 2),
        occurrence(blocked, 3),
        occurrence(failed("placeholder", Some(404)), 4),
        occurrence(CrawlRecord::pending(url("pending-first"), 0), 5),
        occurrence(failed("pending-first", None), 6),
        redirect(
            failed("bad-route", Some(404)),
            "direct-good",
            &["bad-route", "bad-hop"],
        ),
        page("direct-good"),
        redirect(
            page("good-route"),
            "direct-bad",
            &["good-route", "good-hop"],
        ),
        failed("direct-bad", Some(500)),
        redirect(
            page("hop-priority"),
            "direct-ok",
            &["hop-priority", "bad-hop"],
        ),
        limit,
        root,
        source("to-first-list-failure", Some("direct-ok"), None),
        source("to-placeholder", Some("placeholder"), Some("pending-first")),
        source("to-failed-route", Some("bad-route"), Some("bad-hop")),
        source("to-successful-route", Some("good-route"), Some("good-hop")),
        source("to-direct", Some("direct-good"), Some("direct-bad")),
        source("to-limit-route", Some("limit"), Some("limit-hop")),
        source("to-unrequested", Some("unrequested"), None),
        occurrence(
            source("direct-ok", Some("direct-ok#self"), Some("direct-ok")),
            20,
        ),
        redirect(
            source("successful-self-route", Some("bad-hop"), Some("direct-ok")),
            "direct-ok",
            &["successful-self-route", "bad-hop"],
        ),
        root_source,
    ];
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        for row in &rows {
            store.upsert(row.clone());
        }
        for (name, expected) in [
            (
                NEXT,
                vec![
                    "root-source",
                    "to-failed-route",
                    "to-first-list-failure",
                    "to-limit-route",
                    "to-placeholder",
                ],
            ),
            (
                PREV,
                vec![
                    "to-direct",
                    "to-failed-route",
                    "to-limit-route",
                    "to-placeholder",
                ],
            ),
        ] {
            let response = store.query(query(name));
            assert_eq!(
                response
                    .rows
                    .iter()
                    .map(|row| row.url.clone())
                    .collect::<Vec<_>>(),
                expected.into_iter().map(url).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn pagination_columns_sort_and_page_with_memory_sqlite_parity() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for row in [
        source("a", Some("z"), None),
        source("b", Some("a"), Some("z")),
        source("c", None, Some("a")),
        source("d", Some("a"), Some("z")),
    ] {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for (column, paths) in [
        ("relNext", ["c", "b", "d", "a"]),
        ("relPrev", ["a", "c", "b", "d"]),
    ] {
        let asc = GridQuery {
            sort_by: Some(column.into()),
            ..GridQuery::default()
        };
        assert_eq!(
            sqlite
                .try_query(asc.clone())
                .unwrap()
                .rows
                .iter()
                .map(|row| row.url.clone())
                .collect::<Vec<_>>(),
            paths.into_iter().map(url).collect::<Vec<_>>()
        );
        for direction in [SortDirection::Asc, SortDirection::Desc] {
            for offset in 0..4 {
                let query = GridQuery {
                    offset,
                    limit: 1,
                    sort_dir: direction.clone(),
                    ..asc.clone()
                };
                let expected = memory.query(query.clone());
                let actual = sqlite.try_query(query).unwrap();
                assert_eq!(
                    keys(&actual),
                    keys(&expected),
                    "{column} {direction:?} {offset}"
                );
            }
        }
    }
}

#[test]
fn pagination_summary_fields_default_for_older_saved_summaries() {
    let mut value = serde_json::to_value(CrawlSummary::default()).unwrap();
    for name in [
        NEXT,
        PREV,
        NEXT_LOOP,
        PREV_LOOP,
        NEXT_NON_RECIPROCAL,
        PREV_NON_RECIPROCAL,
    ] {
        value.as_object_mut().unwrap().remove(name);
    }
    let restored: CrawlSummary = serde_json::from_value(value).unwrap();
    let value = serde_json::to_value(restored).unwrap();
    assert_eq!(value[NEXT], 0);
    assert_eq!(value[PREV], 0);
    assert_eq!(value[NEXT_LOOP], 0);
    assert_eq!(value[PREV_LOOP], 0);
    assert_eq!(value[NEXT_NON_RECIPROCAL], 0);
    assert_eq!(value[PREV_NON_RECIPROCAL], 0);
}

#[test]
fn pagination_shares_the_reference_cache_and_decodes_only_the_requested_page() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .try_upsert(source("a", Some("target"), Some("target")))
        .unwrap();
    store.try_upsert(source("b", Some("target"), None)).unwrap();
    store.try_upsert(failed("target", Some(404))).unwrap();
    // This payload cannot be hydrated, but is unrelated to compact reference evidence.
    store.connection().unwrap().execute(
        "UPDATE crawl_records SET response_time_ms = 'invalid off-page payload' WHERE url != ?1",
        [url("a")],
    ).unwrap();
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    assert_eq!(store.try_progress_summary().unwrap().total, 3);
    assert_eq!(URL_ALIAS_EXPANSIONS.with(|count| count.get()), 0);
    assert!(store.reference_cache.lock().unwrap().is_none());
    assert_eq!(store.try_summary().unwrap().pagination_next_to_error, 2);
    assert!(URL_ALIAS_EXPANSIONS.with(|count| count.get()) > 0);
    let page = store
        .try_query(GridQuery {
            limit: 1,
            ..query(NEXT)
        })
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.rows[0].url, url("a"));
    assert!(
        store
            .try_query(GridQuery {
                offset: 1,
                limit: 1,
                ..query(NEXT)
            })
            .is_err()
    );
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    store
        .try_save_frontier_state(CrawlFrontierState::default())
        .unwrap();
    store.connection().unwrap().execute_batch(
        "CREATE TEMP TABLE pagination_probe (value INTEGER); INSERT INTO pagination_probe VALUES (1);",
    ).unwrap();
    for name in [NEXT, PREV, "canonicalToError", NEXT] {
        store
            .try_query(GridQuery {
                limit: 0,
                ..query(name)
            })
            .unwrap();
    }
    assert_eq!(
        URL_ALIAS_EXPANSIONS.with(|count| count.get()),
        0,
        "All reference views reuse one cached target analysis"
    );
    {
        let mut conn = store.connection().unwrap();
        let transaction = conn.transaction().unwrap();
        transaction
            .execute(
                "UPDATE crawl_records SET status_code = 200 WHERE url = ?1",
                [url("target")],
            )
            .unwrap();
        transaction.rollback().unwrap();
    }
    assert_eq!(store.try_summary().unwrap().pagination_next_to_error, 2);
    assert_eq!(
        URL_ALIAS_EXPANSIONS.with(|count| count.get()),
        0,
        "Rollback preserves cached evidence"
    );
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET status_code = 200 WHERE url = ?1",
            [url("target")],
        )
        .unwrap();
    store.try_progress_summary().unwrap();
    assert_eq!(
        URL_ALIAS_EXPANSIONS.with(|count| count.get()),
        0,
        "Progress never rebuilds reference diagnostics"
    );
    let summary = store.try_summary().unwrap();
    assert_eq!(
        (
            summary.pagination_next_to_error,
            summary.pagination_prev_to_error
        ),
        (0, 0)
    );
    assert!(URL_ALIAS_EXPANSIONS.with(|count| count.get()) > 0);
}

#[test]
fn pagination_cache_observes_external_target_and_source_edits_and_reopen() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-pagination-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteStore::open(&path).unwrap();
    let writer = SqliteStore::open(&path).unwrap();
    store
        .try_upsert(source("source", Some("target"), None))
        .unwrap();
    assert_eq!(store.try_summary().unwrap().pagination_next_to_error, 0);
    writer.try_upsert(failed("target", None)).unwrap();
    assert_eq!(store.try_summary().unwrap().pagination_next_to_error, 1);
    writer
        .try_upsert(source("source", None, Some("target")))
        .unwrap();
    let summary = store.try_summary().unwrap();
    assert_eq!(
        (
            summary.pagination_next_to_error,
            summary.pagination_prev_to_error
        ),
        (0, 1)
    );
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.try_query(query(PREV)).unwrap().total, 1);
    writer
        .connection()
        .unwrap()
        .execute("DELETE FROM crawl_records WHERE url = ?1", [url("target")])
        .unwrap();
    assert_eq!(store.try_summary().unwrap().pagination_prev_to_error, 0);
    drop(store);
    drop(writer);
    std::fs::remove_file(path).unwrap();
}
