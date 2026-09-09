use super::*;

fn page(path: &str) -> CrawlRecord {
    let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 1);
    record.status_code = Some(200);
    record.status_text = "OK".into();
    record.indexability = "Indexable".into();
    record.indexability_status = "Indexable".into();
    record.inlink_count = 1;
    record.in_sitemap = true;
    record
}

fn fixture() -> Vec<CrawlRecord> {
    let mut records = vec![page("ok")];
    for (path, status, text, error) in [
        ("pending", None, "Pending", None),
        ("no-response", None, "Failed", Some("Connection refused")),
        ("empty-error", None, "Failed", Some("")),
        ("blank-error", None, "Failed", Some("\t\u{2003}")),
        ("robots-text", None, "Blocked by robots.txt", None),
        (
            "robots-error",
            None,
            "Failed",
            Some("Blocked by robots.txt"),
        ),
        ("redirect", Some(302), "Found", None),
        ("client", Some(404), "Not Found", None),
        ("server", Some(503), "Unavailable", None),
        ("nonstandard", Some(600), "Nonstandard", None),
        (
            "known-with-error",
            Some(200),
            "Blocked by robots.txt",
            Some("Blocked by robots.txt"),
        ),
        ("résumé", Some(200), "CAFÉ 50%_done", Some("\u{2003}")),
    ] {
        let mut record = page(path);
        record.status_code = status;
        record.status_text = text.into();
        record.error = error.map(str::to_string);
        records.push(record);
    }
    let mut noindex = page("noindex");
    noindex.indexability_status = "Meta noindex".into();
    records.push(noindex);
    let mut nonindexable = page("non-indexable");
    nonindexable.indexability = "Non-indexable".into();
    records.push(nonindexable);
    for canonical in [
        None,
        Some(""),
        Some("\u{2003}"),
        Some(" https://example.test/canonical "),
        Some("https://example.test/canonical#fragment"),
        Some("https://example.test/preferred"),
    ] {
        let mut record = page("canonical");
        record.storage_key = format!("list:{}:{}", records.len(), record.url);
        record.canonical = canonical.map(str::to_string);
        records.push(record);
    }
    for target in [
        Some(""),
        Some("\t\u{2003}"),
        Some("https://example.test/target"),
    ] {
        let mut record = page("redirect-target");
        record.storage_key = format!("list:{}:{}", records.len(), record.url);
        record.redirect_target = target.map(str::to_string);
        records.push(record);
    }
    let mut orphan = page("orphan");
    orphan.inlink_count = 0;
    records.push(orphan.clone());
    orphan.url = "https://external.test/orphan".into();
    orphan.final_url = orphan.url.clone();
    orphan.storage_key = orphan.url.clone();
    orphan.classification = UrlClassification::External;
    records.push(orphan);
    let mut redirected = page("original");
    redirected.final_url = "https://example.test/ok".into();
    redirected.status_text = "Retained original response".into();
    records.push(redirected);
    let mut chain_only = page("chain-only");
    chain_only.redirect_chain.push(RedirectHop {
        url: chain_only.url.clone(),
        status_code: 301,
        location: Some("https://example.test/ok".into()),
        dns_lookup_time_ms: None,
        tcp_connect_time_ms: None,
        tls_handshake_time_ms: None,
        ttfb_ms: None,
        elapsed_ms: None,
    });
    records.push(chain_only);
    let mut hidden = page("not-in-sitemap");
    hidden.in_sitemap = false;
    records.push(hidden);
    for (index, record) in records.iter_mut().enumerate() {
        record.list_position = Some(index as u32 + 1);
    }
    records
}

#[test]
fn sitemap_pages_match_memory_for_findings_search_sort_and_occurrences() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let records = fixture();
    for record in &records {
        memory.upsert(record.clone());
        sqlite.try_upsert(record.clone()).unwrap();
    }
    let whole = sqlite
        .try_sitemap_validation(SitemapValidationQuery::default())
        .unwrap();
    assert_eq!(whole.total, records.len() - 1);
    assert_eq!(
        whole
            .rows
            .iter()
            .filter(|row| row.final_url == "https://example.test/ok")
            .count(),
        2
    );
    let row = |path: &str| {
        whole
            .rows
            .iter()
            .find(|row| row.url.ends_with(path))
            .unwrap()
    };
    assert_eq!(row("/pending").issues, ["OK"]);
    assert_eq!(row("/nonstandard").issues, ["OK"]);
    assert_eq!(row("/chain-only").issues, ["OK"]);
    assert_eq!(
        row("/robots-text").issues,
        ["Robots-blocked URL in sitemap"]
    );
    assert_eq!(row("/empty-error").issues, ["No response URL in sitemap"]);
    assert_eq!(
        row("/known-with-error").issues,
        ["Fetch error for sitemap URL"]
    );
    assert_eq!(
        row("/no-response").issues,
        ["No response URL in sitemap", "Fetch error for sitemap URL"]
    );

    for sort_by in [
        None,
        Some("url"),
        Some("finalUrl"),
        Some("statusCode"),
        Some("statusText"),
        Some("indexability"),
        Some("indexabilityStatus"),
        Some("inlinkCount"),
        Some("redirectTarget"),
        Some("canonical"),
        Some("issueCount"),
        Some("severity"),
        Some("issues"),
        Some("unknown"),
    ] {
        for direction in [SortDirection::Asc, SortDirection::Desc] {
            for search in [
                None,
                Some("URL in sitemap"),
                Some("CANONICAL"),
                Some("RÉSUMÉ"),
                Some("CAFÉ"),
                Some("50%_"),
                Some("sitemap; Fetch"),
                Some("200"),
                Some("OK"),
                Some("\u{2003}"),
            ] {
                for (offset, limit) in [(0, usize::MAX), (1, 3)] {
                    let query = SitemapValidationQuery {
                        offset,
                        limit,
                        global_search: search.map(str::to_string),
                        sort_by: sort_by.map(str::to_string),
                        sort_dir: direction.clone(),
                    };
                    let expected = memory.sitemap_validation(query.clone());
                    let actual = sqlite.try_sitemap_validation(query.clone()).unwrap();
                    assert_eq!(actual.total, expected.total, "{query:?}");
                    assert_eq!(actual.rows, expected.rows, "{query:?}");
                }
            }
        }
    }
}

#[test]
fn sqlite_sitemap_pages_decode_only_selected_report_columns_and_rows() {
    let sqlite = SqliteStore::in_memory().unwrap();
    for index in 0..10 {
        sqlite.try_upsert(page(&format!("page/{index}"))).unwrap();
    }
    sqlite.connection().unwrap().execute_batch(
        "UPDATE crawl_records SET response_time_ms = 'unrelated invalid integer', custom_extractions = 'invalid JSON';
         UPDATE crawl_records SET url = X'80' WHERE final_url = 'https://example.test/page/9';",
    ).unwrap();
    assert!(sqlite.try_records().is_err());
    let query = SitemapValidationQuery {
        offset: 3,
        limit: 2,
        sort_by: Some("finalUrl".into()),
        sort_dir: SortDirection::Asc,
        ..SitemapValidationQuery::default()
    };
    for response in [
        sqlite.try_sitemap_validation(query.clone()).unwrap(),
        sqlite.sitemap_validation(query.clone()),
        ActiveStore::Sqlite(sqlite.clone()).sitemap_validation(query.clone()),
    ] {
        assert_eq!(response.total, 10);
        assert_eq!(response.rows.len(), 2);
        assert_eq!(response.rows[0].url, "https://example.test/page/3");
    }
    for (offset, limit) in [(0, 0), (10, 1), (usize::MAX, usize::MAX)] {
        let response = sqlite
            .try_sitemap_validation(SitemapValidationQuery {
                offset,
                limit,
                ..query.clone()
            })
            .unwrap();
        assert_eq!(response.total, 10);
        assert!(response.rows.is_empty());
    }
    assert!(
        sqlite
            .try_sitemap_validation(SitemapValidationQuery {
                offset: 9,
                limit: 1,
                ..query
            })
            .is_err()
    );
}

#[test]
fn sqlite_sitemap_ties_preserve_list_order_and_original_occurrences() {
    let sqlite = SqliteStore::in_memory().unwrap();
    for (path, position) in [("third", 30), ("first", 10), ("second", 20)] {
        let mut record = page(path);
        record.final_url = "https://example.test/shared".into();
        record.list_position = Some(position);
        sqlite.try_upsert(record).unwrap();
    }
    for sort_by in [None, Some("finalUrl"), Some("severity"), Some("unknown")] {
        for sort_dir in [SortDirection::Asc, SortDirection::Desc] {
            let query = SitemapValidationQuery {
                limit: 1,
                sort_by: sort_by.map(str::to_string),
                sort_dir,
                ..SitemapValidationQuery::default()
            };
            let expected = build_sitemap_validation_report(
                &sqlite.try_records().unwrap(),
                SitemapValidationQuery {
                    limit: 10,
                    ..query.clone()
                },
            );
            for (offset, row) in expected.rows.iter().enumerate() {
                let actual = sqlite
                    .try_sitemap_validation(SitemapValidationQuery {
                        offset,
                        ..query.clone()
                    })
                    .unwrap();
                assert_eq!(actual.total, 3);
                assert_eq!(&actual.rows[0], row);
            }
            assert!(expected.rows[0].url.ends_with("/first"));
        }
    }
}

#[test]
fn sitemap_pages_observe_membership_record_inlink_and_external_changes() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-sitemap-pages-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sqlite = SqliteStore::open(&path).unwrap();
    let memory = MemoryStore::new();
    let mut record = page("changed");
    record.in_sitemap = false;
    record.inlink_count = 0;
    for store in [
        ActiveStore::Memory(memory.clone()),
        ActiveStore::Sqlite(sqlite.clone()),
    ] {
        store.upsert(record.clone());
        assert_eq!(
            store
                .sitemap_validation(SitemapValidationQuery::default())
                .total,
            0
        );
        store.mark_sitemap_urls(&[record.url.clone()]);
        assert_eq!(
            store
                .sitemap_validation(SitemapValidationQuery::default())
                .rows[0]
                .issues,
            ["Orphan URL in sitemap"]
        );
        store.add_inlink(&record.url);
        assert_eq!(
            store
                .sitemap_validation(SitemapValidationQuery::default())
                .rows[0]
                .issues,
            ["OK"]
        );
    }
    record.in_sitemap = true;
    record.status_code = Some(404);
    record.indexability = "Non-indexable".into();
    record.canonical = Some("https://example.test/preferred".into());
    for store in [
        ActiveStore::Memory(memory.clone()),
        ActiveStore::Sqlite(sqlite.clone()),
    ] {
        store.upsert(record.clone());
    }
    let query = SitemapValidationQuery {
        global_search: Some("4xx".into()),
        ..SitemapValidationQuery::default()
    };
    assert_eq!(
        sqlite.try_sitemap_validation(query.clone()).unwrap().rows,
        memory.sitemap_validation(query.clone()).rows
    );
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch("BEGIN; UPDATE crawl_records SET in_sitemap = 0; ROLLBACK;")
        .unwrap();
    assert_eq!(
        sqlite.try_sitemap_validation(query.clone()).unwrap().total,
        1
    );
    writer
        .execute("UPDATE crawl_records SET status_code = 503", [])
        .unwrap();
    assert_eq!(sqlite.try_sitemap_validation(query).unwrap().total, 0);
    let query = SitemapValidationQuery {
        global_search: Some("5xx".into()),
        ..SitemapValidationQuery::default()
    };
    assert_eq!(
        sqlite.try_sitemap_validation(query.clone()).unwrap().total,
        1
    );
    drop(sqlite);
    let sqlite = SqliteStore::open(&path).unwrap();
    assert_eq!(
        sqlite.try_sitemap_validation(query.clone()).unwrap().total,
        1
    );
    writer.execute("DELETE FROM crawl_records", []).unwrap();
    assert_eq!(sqlite.try_sitemap_validation(query).unwrap().total, 0);
    drop(writer);
    drop(sqlite);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_sitemap_count_and_page_share_a_snapshot_during_external_writes() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-sitemap-snapshot-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sqlite = SqliteStore::open(&path).unwrap();
    sqlite.try_upsert(page("first")).unwrap();
    sqlite.try_upsert(page("second")).unwrap();
    let writer = Connection::open(&path).unwrap();
    let wrote = std::sync::atomic::AtomicBool::new(false);
    sqlite.connection().unwrap().create_scalar_function(
        "ff_contains", 2, rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        move |context| {
            if !wrote.swap(true, std::sync::atomic::Ordering::Relaxed) {
                writer.execute("UPDATE crawl_records SET in_sitemap = 0 WHERE url = 'https://example.test/first'", [])?;
            }
            Ok(context.get_raw(0).as_str().unwrap_or_default().to_lowercase().contains(context.get_raw(1).as_str()?))
        }
    ).unwrap();
    let query = SitemapValidationQuery {
        global_search: Some("example.test".into()),
        ..SitemapValidationQuery::default()
    };
    let snapshot = sqlite.try_sitemap_validation(query.clone()).unwrap();
    assert_eq!(snapshot.total, 2);
    assert_eq!(
        snapshot.rows.len(),
        2,
        "Count and rows observed different database revisions"
    );
    let updated = sqlite.try_sitemap_validation(query).unwrap();
    assert_eq!(updated.total, 1);
    assert_eq!(updated.rows.len(), 1);
    assert!(updated.rows[0].url.ends_with("/second"));
    drop(sqlite);
    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore = "measures repeated sitemap report pages over 20,000 crawl records"]
fn sqlite_sitemap_repeated_page_workload() {
    let sqlite = SqliteStore::in_memory().unwrap();
    for index in 0..20_000 {
        let mut record = page(&format!("page/{index:05}"));
        record.in_sitemap = index % 2 == 0;
        record.status_code = Some(if index % 10 == 0 { 404 } else { 200 });
        record.title = Some("Synthetic HTML metadata ".repeat(20));
        sqlite.try_upsert(record).unwrap();
    }
    for (name, search) in [("unfiltered", None), ("4xx findings", Some("4xx"))] {
        let started = std::time::Instant::now();
        let mut returned = 0;
        for offset in (0..1_000).step_by(200) {
            let response = sqlite
                .try_sitemap_validation(SitemapValidationQuery {
                    offset,
                    limit: 200,
                    global_search: search.map(str::to_string),
                    ..SitemapValidationQuery::default()
                })
                .unwrap();
            assert_eq!(
                response.total,
                if search.is_some() { 2_000 } else { 10_000 }
            );
            assert_eq!(response.rows.len(), 200);
            returned += response.rows.len();
        }
        eprintln!(
            "Sitemap pages {name}: 20,000 records, 5 pages, {returned} returned rows, {:.3}s",
            started.elapsed().as_secs_f64()
        );
    }
}
