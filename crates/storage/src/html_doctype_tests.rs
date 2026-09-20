use super::*;

fn page(path: &str, doctype: Option<bool>) -> CrawlRecord {
    let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
    row.status_code = Some(200);
    row.content_type = Some("text/html".into());
    let mut value = serde_json::to_value(row).unwrap();
    value["htmlDoctype"] = serde_json::json!(doctype);
    serde_json::from_value(value).unwrap()
}

fn query() -> GridQuery {
    GridQuery {
        view: serde_json::from_value(serde_json::json!("htmlMissingDoctype")).unwrap(),
        sort_by: Some("url".into()),
        ..GridQuery::default()
    }
}

#[test]
fn html_doctype_measured_absence_requires_complete_successful_html_with_store_parity() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut rows = vec![
        page("absent", Some(false)),
        page("present", Some(true)),
        page("unknown", None),
    ];
    for (path, status, content_type, incomplete) in [
        ("failed", Some(404), "text/html", false),
        ("pending", None, "text/html", false),
        ("redirect", Some(301), "text/html", false),
        ("pdf", Some(200), "application/pdf", false),
        ("incomplete", Some(200), "text/html", true),
    ] {
        let mut row = page(path, Some(false));
        row.status_code = status;
        row.content_type = Some(content_type.into());
        if incomplete {
            row.indexability_status = "Response body incomplete".into();
        }
        rows.push(row);
    }
    for position in 1..=2 {
        let mut row = page("list", Some(false));
        row.storage_key = format!("list:{position}:{}", row.url);
        row.list_position = Some(position);
        rows.push(row);
    }
    for row in rows {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for offset in 0..3 {
        let query = GridQuery {
            offset,
            limit: 1,
            ..query()
        };
        let left = memory.query(query.clone());
        let right = sqlite.try_query(query).unwrap();
        assert_eq!(left.total, 3);
        assert_eq!(right.total, 3);
        assert_eq!(left.rows[0].storage_key, right.rows[0].storage_key);
        assert_eq!(
            serde_json::to_value(&left.rows[0]).unwrap()["htmlDoctype"],
            false
        );
        assert_eq!(
            serde_json::to_value(left.summary).unwrap()["missingHtmlDoctype"],
            3
        );
        assert_eq!(
            serde_json::to_value(right.summary).unwrap()["missingHtmlDoctype"],
            3
        );
    }
    assert_eq!(
        serde_json::to_value(memory.progress_summary()).unwrap()["missingHtmlDoctype"],
        3
    );
    assert_eq!(
        serde_json::to_value(sqlite.try_progress_summary().unwrap()).unwrap()["missingHtmlDoctype"],
        3
    );
    memory.upsert(page("absent", Some(true)));
    sqlite.try_upsert(page("absent", Some(true))).unwrap();
    assert_eq!(memory.query(query()).total, 2);
    assert_eq!(sqlite.try_query(query()).unwrap().total, 2);
    assert_eq!(
        serde_json::to_value(
            sqlite
                .try_records()
                .unwrap()
                .iter()
                .find(|row| row.url.ends_with("/absent"))
                .unwrap()
        )
        .unwrap()["htmlDoctype"],
        true
    );
}

#[test]
fn html_doctype_old_captures_remain_unknown_and_nullable_evidence_survives_reopening() {
    let mut legacy = serde_json::to_value(page("legacy", None)).unwrap();
    legacy.as_object_mut().unwrap().remove("htmlDoctype");
    let restored: CrawlRecord = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap()["htmlDoctype"],
        serde_json::Value::Null
    );
    let mut summary = serde_json::to_value(CrawlSummary::default()).unwrap();
    summary
        .as_object_mut()
        .unwrap()
        .remove("missingHtmlDoctype");
    let restored: CrawlSummary = serde_json::from_value(summary).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap()["missingHtmlDoctype"],
        0
    );
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-doctype-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteStore::open(&path).unwrap();
    store.try_upsert(page("old", Some(false))).unwrap();
    store
        .connection()
        .unwrap()
        .execute("ALTER TABLE crawl_records DROP COLUMN html_doctype", [])
        .unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        serde_json::to_value(&store.try_records().unwrap()[0]).unwrap()["htmlDoctype"],
        serde_json::Value::Null
    );
    assert_eq!(store.try_query(query()).unwrap().total, 0);
    store.try_upsert(page("old", Some(false))).unwrap();
    let writer = SqliteStore::open(&path).unwrap();
    assert_eq!(store.try_query(query()).unwrap().total, 1);
    writer
        .connection()
        .unwrap()
        .execute("UPDATE crawl_records SET html_doctype = 1", [])
        .unwrap();
    assert_eq!(store.try_query(query()).unwrap().total, 0);
    writer
        .connection()
        .unwrap()
        .execute("UPDATE crawl_records SET html_doctype = NULL", [])
        .unwrap();
    assert_eq!(store.try_query(query()).unwrap().total, 0);
    drop(writer);
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        serde_json::to_value(&store.try_records().unwrap()[0]).unwrap()["htmlDoctype"],
        serde_json::Value::Null
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn html_doctype_survives_old_unique_schema_rebuild_and_queries_only_selected_payloads() {
    let store = SqliteStore::in_memory().unwrap();
    store.try_upsert(page("absent", Some(false))).unwrap();
    store.try_upsert(page("present", Some(true))).unwrap();
    {
        let conn = store.connection().unwrap();
        conn.execute(
            "CREATE UNIQUE INDEX old_final_url ON crawl_records(final_url)",
            [],
        )
        .unwrap();
        migrate_final_url_unique_constraint(&conn).unwrap();
        conn.execute("UPDATE crawl_records SET response_time_ms = 'invalid off-page payload' WHERE url LIKE '%/present'", []).unwrap();
    }
    assert!(store.try_records().is_err());
    let response = store
        .try_query(GridQuery {
            limit: 1,
            ..query()
        })
        .unwrap();
    assert_eq!(response.total, 1);
    assert_eq!(response.rows[0].html_doctype, Some(false));
    assert_eq!(response.summary.missing_html_doctype, 1);
    let conn = store.connection().unwrap();
    assert!(
        conn.query_row(
            "SELECT html_doctype FROM crawl_records WHERE url LIKE '%/present'",
            [],
            |row| row.get::<_, bool>(0)
        )
        .unwrap()
    );
}
