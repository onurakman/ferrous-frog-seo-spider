use super::*;

fn url(path: &str) -> String {
    format!("https://example.test/{path}")
}

fn page(path: &str, amp: Option<&str>) -> CrawlRecord {
    let mut row = CrawlRecord::pending(url(path), 0);
    row.status_code = Some(200);
    row.content_type = Some("text/html".into());
    row.amphtml = amp.map(url);
    row
}

fn query() -> GridQuery {
    GridQuery {
        view: serde_json::from_value(serde_json::json!("ampToError"))
            .expect("AMP audit view must be supported"),
        sort_by: Some("url".into()),
        ..GridQuery::default()
    }
}

#[test]
fn amp_audits_only_known_failed_targets_and_preserves_list_and_alias_evidence() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut missing = page("missing", None);
    missing.status_code = Some(404);
    let mut failed = CrawlRecord::pending(url("failed"), 0);
    failed.error = Some("Connection refused".into());
    let mut blocked = CrawlRecord::pending(url("blocked"), 0);
    blocked.error = Some("Blocked by robots.txt".into());
    let mut noindex = page("noindex", None);
    noindex.indexability = "Non-indexable".into();
    noindex.indexability_status = "Meta noindex".into();
    let mut incomplete = page("incomplete", Some("missing"));
    incomplete.indexability_status = "Response body incomplete".into();
    let mut image = page("image", Some("missing"));
    image.content_type = Some("image/png".into());
    let mut unsuccessful = page("unsuccessful", Some("missing"));
    unsuccessful.status_code = Some(500);
    let mut invalid = page("invalid", None);
    invalid.amphtml = Some("mailto:missing@example.test".into());
    let mut list_source = page("a", Some("missing#amp"));
    list_source.storage_key = "list:30:a".into();
    list_source.list_position = Some(30);
    let mut failed_self = page("self", None);
    failed_self.status_code = Some(404);
    let mut successful_self = page("self", Some("self#amp"));
    successful_self.storage_key = "list:31:self".into();
    let mut redirect = page("redirect", None);
    redirect.final_url = url("final");
    redirect.status_code = Some(503);
    redirect.redirect_chain.push(RedirectHop {
        url: url("redirect"),
        status_code: 301,
        location: Some(url("final")),
        dns_lookup_time_ms: None,
        tcp_connect_time_ms: None,
        tls_handshake_time_ms: None,
        ttfb_ms: None,
        elapsed_ms: None,
    });
    let mut all_references = page("a", Some("missing#amp"));
    all_references.canonical = Some(url("missing"));
    all_references.rel_next = Some(url("missing"));
    all_references.rel_prev = Some(url("missing"));
    for row in [
        missing,
        failed,
        blocked,
        noindex,
        incomplete,
        image,
        unsuccessful,
        invalid,
        all_references,
        list_source,
        failed_self,
        successful_self,
        redirect,
        page("b", Some("failed")),
        page("c", Some("redirect#amp")),
        page("d", Some("final")),
        page("unknown", Some("not-crawled")),
        page("robots", Some("blocked")),
        page("noindex-source", Some("noindex")),
        page("same-url-distinct-query", Some("missing?other=1")),
    ] {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for response in [memory.query(query()), sqlite.try_query(query()).unwrap()] {
        assert_eq!(
            response
                .rows
                .iter()
                .map(|row| row.url.clone())
                .collect::<Vec<_>>(),
            ["a", "a", "b", "c", "d"].map(url)
        );
        assert_eq!(response.total, 5);
        let summary = serde_json::to_value(response.summary).unwrap();
        assert_eq!(summary["ampToError"], 5);
        for name in [
            "canonicalToError",
            "paginationNextToError",
            "paginationPrevToError",
        ] {
            assert_eq!(summary[name], 1, "Widening flags preserves {name}");
        }
    }
    for (offset, search, segment) in [
        (0, None, None),
        (1, None, None),
        (0, Some("missing#amp"), None),
        (0, None, Some("/(a|d)$")),
    ] {
        let query = GridQuery {
            offset,
            limit: 1,
            global_search: search.map(str::to_string),
            segment_pattern: segment.map(str::to_string),
            segment_regex: true,
            ..query()
        };
        let expected = memory.query(query.clone());
        let actual = sqlite.try_query(query).unwrap();
        assert_eq!(actual.total, expected.total);
        assert_eq!(
            actual
                .rows
                .iter()
                .map(|row| &row.storage_key)
                .collect::<Vec<_>>(),
            expected
                .rows
                .iter()
                .map(|row| &row.storage_key)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn amp_sorting_and_old_summary_defaults_match_both_stores() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for row in [
        page("a", Some("z")),
        page("b", Some("a")),
        page("c", None),
        page("d", Some("a")),
    ] {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for (direction, paths) in [
        (SortDirection::Asc, ["c", "b", "d", "a"]),
        (SortDirection::Desc, ["a", "b", "d", "c"]),
    ] {
        for (offset, path) in paths.into_iter().enumerate() {
            let query = GridQuery {
                offset,
                limit: 1,
                sort_by: Some("amphtml".into()),
                sort_dir: direction.clone(),
                ..GridQuery::default()
            };
            for response in [
                memory.query(query.clone()),
                sqlite.try_query(query).unwrap(),
            ] {
                assert_eq!(response.rows[0].url, url(path));
            }
        }
    }
    let mut value = serde_json::to_value(CrawlSummary::default()).unwrap();
    value.as_object_mut().unwrap().remove("ampToError");
    let restored: CrawlSummary = serde_json::from_value(value).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap()["ampToError"], 0);
}

#[test]
fn amp_queries_share_the_reference_cache_and_keep_hydration_bounded() {
    let store = SqliteStore::in_memory().unwrap();
    store.try_upsert(page("a", Some("target"))).unwrap();
    store.try_upsert(page("b", Some("target"))).unwrap();
    let mut target = page("target", None);
    target.status_code = Some(404);
    store.try_upsert(target).unwrap();
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET response_time_ms = 'invalid off-page value' WHERE url != ?1",
            [url("a")],
        )
        .unwrap();
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    assert_eq!(store.try_progress_summary().unwrap().total, 3);
    assert_eq!(URL_ALIAS_EXPANSIONS.with(|count| count.get()), 0);
    let response = store
        .try_query(GridQuery {
            limit: 1,
            ..query()
        })
        .unwrap();
    assert_eq!(response.total, 2);
    assert_eq!(response.rows[0].url, url("a"));
    assert!(
        store
            .try_query(GridQuery {
                offset: 1,
                limit: 1,
                ..query()
            })
            .is_err()
    );
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    store
        .try_save_frontier_state(CrawlFrontierState::default())
        .unwrap();
    for _ in 0..3 {
        store
            .try_query(GridQuery {
                limit: 0,
                ..query()
            })
            .unwrap();
    }
    assert_eq!(URL_ALIAS_EXPANSIONS.with(|count| count.get()), 0);
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET amphtml = NULL WHERE url = ?1",
            [url("b")],
        )
        .unwrap();
    assert_eq!(
        store
            .try_query(GridQuery {
                limit: 1,
                ..query()
            })
            .unwrap()
            .total,
        1
    );
    assert!(URL_ALIAS_EXPANSIONS.with(|count| count.get()) > 0);
}
