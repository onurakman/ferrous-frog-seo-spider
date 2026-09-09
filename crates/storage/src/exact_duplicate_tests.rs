use super::*;

fn exact_view() -> IssueView {
    serde_json::from_value(serde_json::json!("exactDuplicate")).unwrap()
}

fn exact_count(summary: &CrawlSummary) -> usize {
    serde_json::to_value(summary).unwrap()["exactDuplicates"]
        .as_u64()
        .expect("Exact duplicate count must be serialized") as usize
}

fn page(path: &str, hash: Option<&str>) -> CrawlRecord {
    let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
    row.status_code = Some(200);
    row.status_text = "OK".into();
    row.content_type = Some("text/html; charset=utf-8".into());
    row.indexability = "Indexable".into();
    row.indexability_status = "Indexable".into();
    row.response_hash = hash.map(str::to_string);
    row
}

fn insert_fixture() -> (MemoryStore, SqliteStore) {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut rows = vec![page("a", Some("shared")), page("b", Some("shared"))];
    rows[1].indexability = "Non-indexable".into();
    rows[1].indexability_status = "Meta noindex".into();
    rows[1].classification = UrlClassification::External;
    let mut repeated = rows[0].clone();
    repeated.storage_key = "list:3:https://example.test/a".into();
    repeated.list_position = Some(3);
    let mut redirect = page("redirect", Some("shared"));
    redirect.final_url = rows[0].final_url.clone();
    rows.extend([repeated, redirect, page("unique", Some("unique"))]);
    for (path, status, content_type, indexability_status) in [
        ("failed", Some(404), "text/html", "Client error"),
        (
            "incomplete",
            Some(200),
            "text/html",
            "Response body incomplete",
        ),
        ("pending", None, "text/html", "Pending"),
        ("image", Some(200), "image/png", "Indexable"),
    ] {
        let mut row = page(path, Some("unique"));
        row.status_code = status;
        row.content_type = Some(content_type.into());
        row.indexability_status = indexability_status.into();
        rows.push(row);
    }
    rows.push(page("unknown", None));
    rows.push(page("blank1", Some("\t\u{2003}")));
    rows.push(page("blank2", Some("\t\u{2003}")));
    for (index, mut row) in rows.into_iter().enumerate() {
        row.list_position = Some((index + 1) as u32);
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    (memory, sqlite)
}

#[test]
fn exact_duplicate_eligibility_and_occurrence_counts_match_memory_and_sqlite() {
    let (memory, sqlite) = insert_fixture();
    for response in [
        memory.query(GridQuery {
            view: exact_view(),
            ..GridQuery::default()
        }),
        sqlite
            .try_query(GridQuery {
                view: exact_view(),
                ..GridQuery::default()
            })
            .unwrap(),
    ] {
        assert_eq!(response.total, 4);
        assert_eq!(exact_count(&response.summary), 4);
        assert_eq!(
            response.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        assert_eq!(response.summary.total, 12);
    }
}

#[test]
fn exact_duplicates_require_distinct_normalized_final_urls() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        for (path, final_url) in [
            ("first", "https://EXAMPLE.test:443"),
            ("second", "https://example.test/#section"),
            ("third", "https://example.test/"),
        ] {
            let mut row = page(path, Some("shared"));
            row.final_url = final_url.into();
            store.upsert(row);
        }
        assert_eq!(exact_count(&store.summary()), 0);
        assert!(
            store
                .query(GridQuery {
                    view: exact_view(),
                    ..GridQuery::default()
                })
                .rows
                .is_empty()
        );
        for (path, hash) in [
            ("Case", "case"),
            ("case", "case"),
            ("query?x=1", "query"),
            ("query?x=2", "query"),
        ] {
            store.upsert(page(path, Some(hash)));
        }
        assert_eq!(
            exact_count(&store.summary()),
            4,
            "Path case and query values remain distinct"
        );
    }
}

#[test]
fn exact_duplicate_membership_is_whole_crawl_while_sql_pages_decode_only_requested_rows() {
    let (memory, sqlite) = insert_fixture();
    sqlite
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET response_time_ms = 'invalid' WHERE id = 4",
            [],
        )
        .unwrap();
    assert!(sqlite.try_records().is_err());
    let mut query = GridQuery {
        view: exact_view(),
        segment_pattern: Some("/a$".into()),
        segment_regex: true,
        global_search: Some("example.test".into()),
        filters: Some(GridFilterGroup {
            match_mode: GridFilterMatch::All,
            rules: vec![GridFilterRule {
                field: GridFilterField::StatusCode,
                operator: GridFilterOperator::Equals,
                value: "200".into(),
            }],
        }),
        sort_by: Some("id".into()),
        offset: 1,
        limit: 1,
        ..GridQuery::default()
    };
    // Limit by original URL, since the redirect's final URL also matches the segment.
    query.filters.as_mut().unwrap().rules.push(GridFilterRule {
        field: GridFilterField::Url,
        operator: GridFilterOperator::Equals,
        value: "https://example.test/a".into(),
    });
    for response in [
        memory.query(query.clone()),
        sqlite.try_query(query).unwrap(),
    ] {
        assert_eq!((response.total, response.rows.len()), (2, 1));
        assert_eq!(response.rows[0].id, 3);
        assert_eq!(exact_count(&response.summary), 4);
    }
}

#[test]
fn exact_duplicate_summary_defaults_for_older_saved_payloads() {
    let mut json = serde_json::to_value(CrawlSummary::default()).unwrap();
    json.as_object_mut().unwrap().remove("exactDuplicates");
    let restored: CrawlSummary = serde_json::from_value(json).unwrap();
    assert_eq!(exact_count(&restored), 0);
}

#[test]
fn exact_duplicate_group_cache_reuses_queries_and_frontier_changes_without_running_in_progress() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut a = page("a", Some("hash:shared"));
    let mut b = page("b", Some("hash:shared"));
    sqlite.try_upsert(a.clone()).unwrap();
    sqlite.try_upsert(b.clone()).unwrap();
    sqlite
        .try_upsert(page("unique", Some("hash:unique")))
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counted_calls = calls.clone();
    let url_calls = Arc::new(AtomicUsize::new(0));
    let counted_urls = url_calls.clone();
    sqlite
        .connection()
        .unwrap()
        .create_scalar_function(
            "ff_final_url_key",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            move |context| {
                counted_urls.fetch_add(1, Ordering::Relaxed);
                Ok(normalized_final_url(
                    context.get_raw(0).as_str().unwrap_or_default(),
                ))
            },
        )
        .unwrap();
    sqlite
        .connection()
        .unwrap()
        .create_scalar_function(
            "ff_text_key",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            move |context| {
                let text = context.get::<Option<String>>(0)?.unwrap_or_default();
                if text.starts_with("hash:") {
                    counted_calls.fetch_add(1, Ordering::Relaxed);
                }
                Ok(normalize_text_key(&text))
            },
        )
        .unwrap();
    assert_eq!(exact_count(&sqlite.try_progress_summary().unwrap()), 0);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(sqlite.exact_duplicate_cache.lock().unwrap().is_none());
    assert_eq!(exact_count(&sqlite.try_summary().unwrap()), 2);
    let first_work = calls.load(Ordering::Relaxed);
    let first_url_work = url_calls.load(Ordering::Relaxed);
    assert!(first_work >= 3);
    for _ in 0..2 {
        sqlite
            .try_save_frontier_state(CrawlFrontierState::default())
            .unwrap();
        sqlite.connection().unwrap().execute_batch("CREATE TEMP TABLE IF NOT EXISTS unrelated (value); INSERT INTO unrelated VALUES (1);").unwrap();
        assert_eq!(
            sqlite
                .try_query(GridQuery {
                    view: exact_view(),
                    ..GridQuery::default()
                })
                .unwrap()
                .total,
            2
        );
        assert_eq!(
            calls.load(Ordering::Relaxed),
            first_work,
            "Unchanged evidence must reuse the hash groups"
        );
        assert_eq!(
            url_calls.load(Ordering::Relaxed),
            first_url_work,
            "Paged queries must reuse normalized final-URL membership"
        );
    }
    {
        let mut conn = sqlite.connection().unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute("UPDATE crawl_records SET response_hash = NULL", [])
            .unwrap();
        tx.rollback().unwrap();
    }
    assert_eq!(exact_count(&sqlite.try_summary().unwrap()), 2);
    assert_eq!(calls.load(Ordering::Relaxed), first_work);

    b.response_hash = Some("hash:changed".into());
    sqlite.try_upsert(b).unwrap();
    assert_eq!(exact_count(&sqlite.try_progress_summary().unwrap()), 0);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        first_work,
        "Progress must never build hash groups"
    );
    assert_eq!(exact_count(&sqlite.try_summary().unwrap()), 0);
    assert!(calls.load(Ordering::Relaxed) > first_work);

    a.response_hash = Some("hash:changed".into());
    sqlite.try_upsert(a.clone()).unwrap();
    assert_eq!(exact_count(&sqlite.try_summary().unwrap()), 2);
    a.indexability_status = "Response body incomplete".into();
    sqlite.try_upsert(a).unwrap();
    assert_eq!(exact_count(&sqlite.try_summary().unwrap()), 0);
}

#[test]
fn exact_duplicate_cache_observes_external_updates_deletes_and_reopening() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-exact-audit-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let reader = SqliteStore::open(&path).unwrap();
    reader.try_upsert(page("a", Some("shared"))).unwrap();
    reader.try_upsert(page("b", Some("shared"))).unwrap();
    assert_eq!(exact_count(&reader.try_summary().unwrap()), 2);
    let writer = Connection::open(&path).unwrap();
    writer
        .execute(
            "UPDATE crawl_records SET final_url = 'https://example.test/a#fragment' WHERE id = 2",
            [],
        )
        .unwrap();
    assert_eq!(exact_count(&reader.try_summary().unwrap()), 0);
    writer
        .execute(
            "UPDATE crawl_records SET final_url = 'https://example.test/b' WHERE id = 2",
            [],
        )
        .unwrap();
    assert_eq!(exact_count(&reader.try_summary().unwrap()), 2);
    writer
        .execute("DELETE FROM crawl_records WHERE id = 2", [])
        .unwrap();
    assert_eq!(exact_count(&reader.try_summary().unwrap()), 0);
    drop(writer);
    drop(reader);
    let reopened = SqliteStore::open(&path).unwrap();
    reopened.try_upsert(page("c", Some("shared"))).unwrap();
    assert_eq!(exact_count(&reopened.try_summary().unwrap()), 2);
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
