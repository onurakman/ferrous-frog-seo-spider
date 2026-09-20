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

#[test]
fn amp_document_marker_preserves_measured_true_false_and_legacy_unknown() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        for marker in [
            serde_json::json!(true),
            serde_json::json!(false),
            serde_json::Value::Null,
        ] {
            let mut value = serde_json::to_value(page("marker", None)).unwrap();
            value["ampDocument"] = marker.clone();
            store.upsert(serde_json::from_value(value).unwrap());
            let restored = serde_json::to_value(&store.records()[0]).unwrap();
            assert_eq!(restored["ampDocument"], marker);
        }
        let mut legacy = serde_json::to_value(page("legacy-marker", None)).unwrap();
        legacy.as_object_mut().unwrap().remove("ampDocument");
        let restored: CrawlRecord = serde_json::from_value(legacy).unwrap();
        assert!(serde_json::to_value(restored).unwrap()["ampDocument"].is_null());
    }
}

fn query() -> GridQuery {
    GridQuery {
        view: serde_json::from_value(serde_json::json!("ampToError"))
            .expect("AMP audit view must be supported"),
        sort_by: Some("url".into()),
        ..GridQuery::default()
    }
}

fn reciprocity_query() -> GridQuery {
    GridQuery {
        view: serde_json::from_value(serde_json::json!("ampNonReciprocal"))
            .expect("AMP reciprocity audit view must be supported"),
        sort_by: Some("url".into()),
        ..GridQuery::default()
    }
}

fn marker_query() -> GridQuery {
    GridQuery {
        view: serde_json::from_value(serde_json::json!("ampTargetMissingMarker"))
            .expect("AMP target marker audit must be supported"),
        sort_by: Some("url".into()),
        ..GridQuery::default()
    }
}

#[test]
fn amp_target_missing_marker_requires_measured_complete_html_and_checks_every_declaration() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let mut missing = page("missing-marker", None);
        missing.amp_document = Some(false);
        let mut present = page("present-marker", None);
        present.amp_document = Some(true);
        let mut later = page("later-source", Some("present-marker"));
        later.amphtml_targets = Some(vec![
            url("present-marker"),
            url("missing-marker#section"),
            url("missing-marker"),
        ]);
        let mut repeated = later.clone();
        repeated.storage_key = "list:9:later-source".into();
        repeated.list_position = Some(9);
        let mut empty = page("empty-source", Some("missing-marker"));
        empty.amphtml_targets = Some(vec![]);
        let mut redirect = page("redirect-target", None);
        redirect.final_url = url("redirect-final");
        redirect.amp_document = Some(false);
        redirect.redirect_chain.push(RedirectHop {
            url: url("redirect-target"),
            status_code: 301,
            location: Some(url("redirect-final")),
            dns_lookup_time_ms: None,
            tcp_connect_time_ms: None,
            tls_handshake_time_ms: None,
            ttfb_ms: None,
            elapsed_ms: None,
        });
        for row in [
            missing,
            present,
            page("unknown-marker", None),
            later,
            repeated,
            empty,
            page("legacy-source", Some("missing-marker")),
            page("present-source", Some("present-marker")),
            page("unknown-source", Some("unknown-marker")),
            page("unseen-source", Some("not-crawled")),
            redirect,
            page("redirect-source", Some("redirect-target")),
            page("final-source", Some("redirect-final")),
        ] {
            store.upsert(row);
        }
        // Ineligible sources and targets cannot turn missing/stale marker data into evidence.
        for state in [
            "pending",
            "blocked",
            "failed",
            "incomplete",
            "non-html",
            "redirect-only",
        ] {
            let mut target = page(&format!("{state}-target"), None);
            target.amp_document = Some(false);
            let mut source = page(&format!("{state}-source"), Some("missing-marker"));
            for row in [&mut target, &mut source] {
                match state {
                    "pending" => row.status_code = None,
                    "blocked" => {
                        row.status_code = None;
                        row.error = Some("Blocked by robots.txt".into());
                    }
                    "failed" => row.status_code = Some(500),
                    "incomplete" => row.indexability_status = "Response body incomplete".into(),
                    "non-html" => row.content_type = Some("image/png".into()),
                    "redirect-only" => row.status_code = Some(302),
                    _ => unreachable!(),
                }
            }
            store.upsert(page(
                &format!("source-of-{state}"),
                Some(&format!("{state}-target")),
            ));
            store.upsert(target);
            store.upsert(source);
        }
        let response = store.query(marker_query());
        assert_eq!(
            response
                .rows
                .iter()
                .map(|row| row.url.clone())
                .collect::<Vec<_>>(),
            [
                "final-source",
                "later-source",
                "later-source",
                "legacy-source",
                "redirect-source"
            ]
            .map(url)
        );
        assert_eq!(response.total, 5);
        assert_eq!(
            serde_json::to_value(response.summary).unwrap()["ampTargetMissingMarker"],
            5
        );
        assert_eq!(
            serde_json::to_value(store.summary()).unwrap()["ampTargetMissingMarker"],
            5
        );
        assert!(response.rows.iter().any(|row| row.list_position == Some(9)));
        let filtered = store.query(GridQuery {
            offset: 1,
            limit: 1,
            global_search: Some("later-source".into()),
            ..marker_query()
        });
        assert_eq!(filtered.total, 2);
        assert_eq!(filtered.rows[0].list_position, Some(9));
        let mut corrected = page("missing-marker", None);
        corrected.amp_document = Some(true);
        store.upsert(corrected);
        assert_eq!(
            store.query(marker_query()).total,
            2,
            "marker changes must invalidate diagnostics"
        );
    }
}

#[test]
fn amp_target_missing_marker_cache_is_bounded_and_observes_external_marker_updates() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-amp-marker-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteStore::open(&path).unwrap();
    let writer = SqliteStore::open(&path).unwrap();
    for source in ["a", "b"] {
        writer.try_upsert(page(source, Some("target"))).unwrap();
    }
    let mut target = page("target", None);
    target.amp_document = Some(false);
    writer.try_upsert(target).unwrap();
    writer
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET response_time_ms = 'invalid off-page value' WHERE url != ?1",
            [url("a")],
        )
        .unwrap();
    assert_eq!(
        store
            .try_query(GridQuery {
                limit: 1,
                ..marker_query()
            })
            .unwrap()
            .total,
        2
    );
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    for _ in 0..3 {
        assert_eq!(
            store
                .try_query(GridQuery {
                    limit: 0,
                    ..marker_query()
                })
                .unwrap()
                .total,
            2
        );
    }
    assert_eq!(
        URL_ALIAS_EXPANSIONS.with(|count| count.get()),
        0,
        "unchanged marker audits must reuse evidence"
    );
    for (marker, expected) in [(Some(true), 0), (None, 0), (Some(false), 2)] {
        writer
            .connection()
            .unwrap()
            .execute(
                "UPDATE crawl_records SET amp_document = ?1 WHERE url = ?2",
                params![marker, url("target")],
            )
            .unwrap();
        assert_eq!(
            store
                .try_query(GridQuery {
                    limit: 1,
                    ..marker_query()
                })
                .unwrap()
                .total,
            expected
        );
    }
    drop(writer);
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        store
            .try_query(GridQuery {
                limit: 1,
                ..marker_query()
            })
            .unwrap()
            .total,
        2
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amp_reciprocity_uses_only_measured_html_targets_and_preserves_list_occurrences() {
    let mut wrong_target = page("amp-wrong", None);
    wrong_target.canonical = Some(url("known-other"));
    let mut correct_target = page("amp-correct", None);
    correct_target.canonical = Some("https://EXAMPLE.test:443/correct-return#section".into());
    let mut invalid_return_target = page("amp-invalid-return", None);
    invalid_return_target.canonical = Some("mailto:unknown@example.test".into());
    let mut unknown_return_target = page("amp-unknown-return", None);
    unknown_return_target.canonical = Some(url("not-crawled-return"));
    let mut redirect_source = page("redirect-return", Some("amp-redirect"));
    redirect_source.final_url = url("redirect-final");
    let mut redirect_target = page("amp-redirect", None);
    redirect_target.canonical = Some(url("redirect-return"));
    let mut blocked = CrawlRecord::pending(url("amp-blocked"), 0);
    blocked.error = Some("Blocked by robots.txt".into());
    let mut failed = page("amp-failed", None);
    failed.status_code = Some(404);
    let mut incomplete = page("amp-incomplete", None);
    incomplete.indexability_status = "Response body incomplete".into();
    let mut non_html = page("amp-image", None);
    non_html.content_type = Some("image/png".into());
    let mut ineligible_source = page("ineligible-source", Some("amp-no-canonical"));
    ineligible_source.indexability_status = "Response body incomplete".into();
    let mut list_source = page("missing-return", Some("amp-no-canonical"));
    list_source.storage_key = "list:30:missing-return".into();
    list_source.list_position = Some(30);
    let rows = [
        page("missing-return", Some("amp-no-canonical")),
        list_source,
        page("amp-no-canonical", None),
        page("wrong-return", Some("amp-wrong")),
        wrong_target,
        page("known-other", None),
        page("correct-return", Some("amp-correct")),
        correct_target,
        page("invalid-return-source", Some("amp-invalid-return")),
        invalid_return_target,
        page("unknown-return-source", Some("amp-unknown-return")),
        unknown_return_target,
        redirect_source,
        redirect_target,
        page("unknown-source", Some("amp-unknown")),
        page("blocked-source", Some("amp-blocked")),
        blocked,
        page("failed-source", Some("amp-failed")),
        failed,
        page("incomplete-source", Some("amp-incomplete")),
        incomplete,
        page("non-html-source", Some("amp-image")),
        non_html,
        ineligible_source,
    ];
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for row in rows {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for response in [
        memory.query(reciprocity_query()),
        sqlite.try_query(reciprocity_query()).unwrap(),
    ] {
        assert_eq!(response.total, 3);
        assert_eq!(
            response
                .rows
                .iter()
                .map(|row| row.url.as_str())
                .collect::<Vec<_>>(),
            [
                url("missing-return"),
                url("missing-return"),
                url("wrong-return")
            ]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
        );
        assert!(
            response
                .rows
                .iter()
                .any(|row| row.list_position == Some(30))
        );
        assert_eq!(
            serde_json::to_value(response.summary).unwrap()["ampNonReciprocal"],
            3
        );
    }
    let mut corrected = page("amp-no-canonical", None);
    corrected.canonical = Some(url("missing-return"));
    memory.upsert(corrected.clone());
    sqlite.try_upsert(corrected).unwrap();
    for response in [
        memory.query(reciprocity_query()),
        sqlite.try_query(reciprocity_query()).unwrap(),
    ] {
        assert_eq!(
            response.total, 1,
            "changing a target canonical invalidates cached diagnostics"
        );
        assert_eq!(response.rows[0].url, url("wrong-return"));
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
    value.as_object_mut().unwrap().remove("ampNonReciprocal");
    value
        .as_object_mut()
        .unwrap()
        .remove("ampTargetMissingMarker");
    let restored: CrawlSummary = serde_json::from_value(value).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap()["ampToError"], 0);
    assert_eq!(
        serde_json::to_value(&restored).unwrap()["ampTargetMissingMarker"],
        0
    );
    assert_eq!(
        serde_json::to_value(&restored).unwrap()["ampNonReciprocal"],
        0
    );
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

#[test]
fn amp_later_declarations_are_retained_and_audited_with_backend_parity() {
    let mut source = serde_json::to_value(page("source", Some("good"))).unwrap();
    source["amphtmlTargets"] = serde_json::json!([url("good"), url("broken"), url("good")]);
    let source: CrawlRecord = serde_json::from_value(source).unwrap();
    let mut good = page("good", None);
    good.canonical = Some(url("source"));
    let mut broken = page("broken", None);
    broken.status_code = Some(404);
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        for row in [source.clone(), good.clone(), broken.clone()] {
            store.upsert(row);
        }
        let retained = store
            .records()
            .into_iter()
            .find(|row| row.url == url("source"))
            .unwrap();
        assert_eq!(
            serde_json::to_value(retained).unwrap()["amphtmlTargets"],
            serde_json::json!([url("good"), url("broken"), url("good")])
        );
        let response = store.query(query());
        assert_eq!(
            response.total, 1,
            "later failed AMP target must not be hidden by first successful target"
        );
        assert_eq!(response.rows[0].url, url("source"));
    }
}

#[test]
fn amp_multiple_inventory_counts_sources_searches_all_targets_and_keeps_unknowns() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut multiple = page("source", Some("first"));
    multiple.amphtml_targets = Some(vec![url("first"), url("later-🐸"), url("first")]);
    let mut duplicate = multiple.clone();
    duplicate.storage_key = "list:2:source".into();
    duplicate.list_position = Some(2);
    let mut incomplete = multiple.clone();
    incomplete.url = url("incomplete");
    incomplete.storage_key = incomplete.url.clone();
    incomplete.indexability_status = "Response body incomplete".into();
    let mut image = multiple.clone();
    image.url = url("image");
    image.storage_key = image.url.clone();
    image.content_type = Some("image/png".into());
    for row in [
        multiple.clone(),
        duplicate,
        incomplete,
        image,
        page("legacy", Some("first")),
    ] {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    let query = GridQuery {
        view: IssueView::AmpMultipleTargets,
        limit: 1,
        offset: 1,
        global_search: Some("later-🐸".into()),
        ..Default::default()
    };
    for response in [
        memory.query(query.clone()),
        sqlite.try_query(query).unwrap(),
    ] {
        assert_eq!(response.total, 2);
        assert_eq!(response.summary.amp_multiple_targets, 2);
        assert_eq!(response.rows[0].storage_key, "list:2:source");
    }
    assert_eq!(memory.progress_summary().amp_multiple_targets, 2);
    assert_eq!(
        sqlite.try_progress_summary().unwrap().amp_multiple_targets,
        2
    );
    // Updating existing evidence invalidates both the inventory count and search predicates.
    multiple.amphtml_targets = Some(vec![]);
    memory.upsert(multiple.clone());
    sqlite.try_upsert(multiple).unwrap();
    assert_eq!(memory.progress_summary().amp_multiple_targets, 1);
    assert_eq!(
        sqlite.try_progress_summary().unwrap().amp_multiple_targets,
        1
    );
    let mut legacy = serde_json::to_value(page("archive", Some("first"))).unwrap();
    legacy.as_object_mut().unwrap().remove("amphtmlTargets");
    let mut restored: CrawlRecord = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored.amphtml_targets, None);
    restored.amphtml_targets = Some(vec![]);
    let restored: CrawlRecord =
        serde_json::from_value(serde_json::to_value(restored).unwrap()).unwrap();
    assert_eq!(restored.amphtml_targets, Some(vec![]));
    let mut summary = serde_json::to_value(CrawlSummary::default()).unwrap();
    summary
        .as_object_mut()
        .unwrap()
        .remove("ampMultipleTargets");
    assert_eq!(
        serde_json::from_value::<CrawlSummary>(summary)
            .unwrap()
            .amp_multiple_targets,
        0
    );
}

#[test]
fn amp_later_reciprocity_and_explicit_empty_targets_use_measured_evidence() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let mut source = page("source", Some("good"));
        source.amphtml_targets = Some(vec![url("good"), url("missing-return")]);
        let mut good = page("good", None);
        good.canonical = Some(url("source"));
        store.upsert(source.clone());
        store.upsert(good);
        store.upsert(page("missing-return", None));
        assert_eq!(store.query(reciprocity_query()).total, 1);
        source.amphtml_targets = Some(vec![]);
        source.amphtml = Some(url("missing-return"));
        store.upsert(source);
        assert_eq!(
            store.query(reciprocity_query()).total,
            0,
            "explicit empty evidence must not fall back to legacy first target"
        );
    }
}

#[test]
fn amp_legacy_sqlite_reopen_keeps_unknown_evidence_and_saves_new_declarations() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-amp-migration-{}.sqlite3",
        std::process::id()
    ));
    let store = SqliteStore::open(&path).unwrap();
    store.try_upsert(page("legacy", Some("first"))).unwrap();
    store
        .connection()
        .unwrap()
        .execute_batch("ALTER TABLE crawl_records DROP COLUMN amphtml_targets; ALTER TABLE crawl_records DROP COLUMN amp_document;")
        .unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    let mut row = store.try_records().unwrap().remove(0);
    assert_eq!(row.amphtml_targets, None);
    assert_eq!(row.amp_document, None);
    assert_eq!(row.amphtml, Some(url("first")));
    row.amphtml_targets = Some(vec![url("first"), url("second"), url("first")]);
    row.amp_document = Some(true);
    store.try_upsert(row).unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        store.try_records().unwrap()[0].amphtml_targets,
        Some(vec![url("first"), url("second"), url("first")])
    );
    assert_eq!(store.try_records().unwrap()[0].amp_document, Some(true));
    drop(store);
    std::fs::remove_file(path).unwrap();
}
