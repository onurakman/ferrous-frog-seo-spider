use super::*;

fn snapshot_json() -> serde_json::Value {
    serde_json::json!({
        "strategy": "mobile",
        "requestedUrl": "https://example.test/caf%C3%A9",
        "completedAtMs": 1_789_000_000_000i64,
        "finalUrl": "https://example.test/caf%C3%A9?view=mobile",
        "fetchedAt": "2026-09-09T09:10:11.000Z",
        "lighthouseVersion": "13.0.0",
        "performanceScore": 0.0,
        "accessibilityScore": null,
        "bestPracticesScore": 0.91,
        "seoScore": 1.0,
        "lcpMs": 0.0,
        "cls": 0.0,
        "tbtMs": null
    })
}

fn snapshot() -> PageSpeedSnapshot {
    serde_json::from_value(snapshot_json()).unwrap()
}

fn temporary_database(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "ferrous-frog-pagespeed-{label}-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn frontier() -> CrawlFrontierState {
    CrawlFrontierState {
        queued: vec![CrawlFrontierItem {
            url: "https://example.test/queued".into(),
            depth: 3,
            from_sitemap: true,
            storage_key: "list:8:https://example.test/queued".into(),
            list_position: Some(8),
            list_duplicate_index: 1,
        }],
        seen: vec!["https://example.test/seen".into()],
        crawled: 2,
    }
}

#[test]
fn pagespeed_archive_record_preserves_zero_missing_and_attribution_values() {
    let mut value = serde_json::to_value(CrawlRecord::pending(
        "https://example.test/caf%C3%A9".into(),
        2,
    ))
    .unwrap();
    value["pageSpeed"] = snapshot_json();
    let record: CrawlRecord = serde_json::from_value(value).unwrap();
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let imported = store.upsert(record.clone());
        let selected = store.try_records_by_ids(&[imported.id]).unwrap();
        assert_eq!(
            serde_json::to_value(&selected[0]).unwrap()["pageSpeed"],
            snapshot_json()
        );
    }
}

#[test]
fn pagespeed_old_records_and_missing_optional_metrics_remain_unknown() {
    let mut archived =
        serde_json::to_value(CrawlRecord::pending("https://example.test/old".into(), 0)).unwrap();
    archived.as_object_mut().unwrap().remove("pageSpeed");
    let restored: CrawlRecord = serde_json::from_value(archived).unwrap();
    assert_eq!(restored.page_speed, None);
    let snapshot: PageSpeedSnapshot = serde_json::from_value(serde_json::json!({
        "strategy": "desktop", "requestedUrl": "https://example.test/old", "completedAtMs": 0
    }))
    .unwrap();
    assert_eq!(snapshot.strategy, PageSpeedStrategy::Desktop);
    let encoded = serde_json::to_value(snapshot).unwrap();
    for field in [
        "finalUrl",
        "fetchedAt",
        "lighthouseVersion",
        "performanceScore",
        "accessibilityScore",
        "bestPracticesScore",
        "seoScore",
        "lcpMs",
        "cls",
        "tbtMs",
    ] {
        assert_eq!(encoded[field], serde_json::Value::Null, "{field}");
    }
}

#[test]
fn pagespeed_save_updates_only_the_selected_list_record_and_keeps_frontier_and_metadata() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let mut first = CrawlRecord::pending("https://example.test/caf%C3%A9".into(), 2);
        first.status_code = Some(200);
        first.content_type = Some("text/html".into());
        first.title = Some("Café — measured page".into());
        first.inlink_count = 7;
        first.in_sitemap = true;
        first.custom_extractions.push(CustomExtractionValue {
            name: "Product".into(),
            values: vec!["Çağrı".into()],
        });
        let first = store.upsert(first);
        let mut duplicate = first.clone();
        duplicate.storage_key = "list:2:https://example.test/caf%C3%A9".into();
        duplicate.list_position = Some(2);
        duplicate.list_duplicate_index = 1;
        duplicate.title = Some("Separate List occurrence".into());
        let duplicate = store.upsert(duplicate);
        let mut alias = CrawlRecord::pending("https://example.test/alias".into(), 1);
        alias.final_url = first.final_url.clone();
        let alias = store.upsert(alias);
        store.save_frontier_state(frontier());
        let ids = [first.id, duplicate.id, alias.id];
        let mut expected = serde_json::to_value(store.try_records_by_ids(&ids).unwrap()).unwrap();
        store.try_save_page_speed(first.id, snapshot()).unwrap();
        expected[0]["pageSpeed"] = snapshot_json();
        assert_eq!(
            serde_json::to_value(store.try_records_by_ids(&ids).unwrap()).unwrap(),
            expected
        );
        assert_eq!(store.load_frontier_state(), Some(frontier()));
        let mut latest = snapshot();
        latest.strategy = PageSpeedStrategy::Desktop;
        latest.completed_at_ms += 1_000;
        latest.performance_score = Some(0.82);
        latest.tbt_ms = Some(125.0);
        store.try_save_page_speed(first.id, latest.clone()).unwrap();
        expected[0]["pageSpeed"] = serde_json::to_value(&latest).unwrap();
        for id in [0, alias.id + 1, u64::MAX] {
            assert!(
                matches!(store.try_save_page_speed(id, snapshot()), Err(StorageError::RecordNotFound(missing)) if missing == id)
            );
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut invalid = snapshot();
            invalid.performance_score = Some(value);
            assert!(matches!(
                store.try_save_page_speed(first.id, invalid),
                Err(StorageError::InvalidPageSpeedSnapshot(_))
            ));
        }
        assert_eq!(
            serde_json::to_value(store.try_records_by_ids(&ids).unwrap()).unwrap(),
            expected
        );
        assert_eq!(store.load_frontier_state(), Some(frontier()));

        // Fresh crawl metadata has no PSI result of its own and retains the dated snapshot.
        let mut refreshed = first;
        refreshed.title = Some("Updated crawl title".into());
        refreshed.page_speed = None;
        let refreshed = store.upsert(refreshed);
        assert_eq!(refreshed.page_speed, Some(latest));
        assert_eq!(refreshed.title.as_deref(), Some("Updated crawl title"));
        let mut imported = refreshed;
        imported.page_speed = Some(snapshot());
        let imported = store.upsert(imported);
        assert_eq!(imported.page_speed, Some(snapshot()));
        assert_eq!(
            store.try_records_by_ids(&[imported.id]).unwrap()[0].page_speed,
            imported.page_speed
        );
        assert_eq!(
            store
                .try_records_by_ids(&[duplicate.id, alias.id])
                .unwrap()
                .iter()
                .filter(|row| row.page_speed.is_some())
                .count(),
            0
        );
    }
}

#[test]
fn pagespeed_sqlite_reopen_and_both_schema_migrations_preserve_snapshots() {
    let path = temporary_database("migrations");
    let store = SqliteStore::open(&path).unwrap();
    let mut row = CrawlRecord::pending("https://example.test/old".into(), 4);
    row.title = Some("Legacy row".into());
    let row = store.try_upsert(row).unwrap();
    store.try_save_frontier_state(frontier()).unwrap();
    store
        .connection()
        .unwrap()
        .execute_batch("ALTER TABLE crawl_records DROP COLUMN page_speed;")
        .unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    let restored = store.try_records().unwrap();
    assert_eq!(restored[0].page_speed, None);
    assert_eq!(restored[0].title, row.title);
    assert_eq!(store.try_load_frontier_state().unwrap(), Some(frontier()));
    store.try_save_page_speed(row.id, snapshot()).unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.try_records().unwrap()[0].page_speed, Some(snapshot()));
    // Old databases may still enforce final-URL uniqueness. Their table rebuild must copy PSI.
    store
        .connection()
        .unwrap()
        .execute_batch("CREATE UNIQUE INDEX old_final_url ON crawl_records(final_url);")
        .unwrap();
    drop(store);
    let store = SqliteStore::open(&path).unwrap();
    let restored = store.try_records().unwrap();
    assert_eq!(restored[0].id, row.id);
    assert_eq!(restored[0].page_speed, Some(snapshot()));
    assert_eq!(restored[0].title, row.title);
    assert_eq!(restored[0].depth, row.depth);
    assert_eq!(store.try_load_frontier_state().unwrap(), Some(frontier()));
    assert!(
        !has_unique_index_on_columns(
            &store.connection().unwrap(),
            "crawl_records",
            &["final_url"]
        )
        .unwrap()
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn pagespeed_sqlite_failed_update_rolls_back_and_selected_reads_stay_bounded() {
    let sqlite = SqliteStore::in_memory().unwrap();
    let first = sqlite
        .try_upsert(CrawlRecord::pending("https://example.test/first".into(), 0))
        .unwrap();
    let other = sqlite
        .try_upsert(CrawlRecord::pending("https://example.test/other".into(), 1))
        .unwrap();
    let store = ActiveStore::Sqlite(sqlite.clone());
    store.try_save_page_speed(first.id, snapshot()).unwrap();
    sqlite.try_save_frontier_state(frontier()).unwrap();
    let before = serde_json::to_value(store.try_records_by_ids(&[first.id]).unwrap()).unwrap();
    let revision = crawl_audit_revision(&sqlite.connection().unwrap()).unwrap();
    // RAISE(FAIL) in an AFTER trigger keeps the statement's changes without an outer transaction.
    sqlite
        .connection()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_page_speed AFTER UPDATE OF page_speed ON crawl_records
         BEGIN SELECT RAISE(FAIL, 'injected snapshot failure'); END;",
        )
        .unwrap();
    let mut latest = snapshot();
    latest.performance_score = Some(0.99);
    assert!(
        store
            .try_save_page_speed(first.id, latest.clone())
            .unwrap_err()
            .to_string()
            .contains("injected snapshot failure")
    );
    assert_eq!(
        serde_json::to_value(store.try_records_by_ids(&[first.id]).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        crawl_audit_revision(&sqlite.connection().unwrap()).unwrap(),
        revision
    );
    assert_eq!(sqlite.try_load_frontier_state().unwrap(), Some(frontier()));
    sqlite
        .connection()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_page_speed;")
        .unwrap();
    sqlite
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET page_speed = 'malformed snapshot' WHERE id = ?1",
            [other.id as i64],
        )
        .unwrap();
    assert!(sqlite.try_records().is_err());
    assert!(store.try_records_by_ids(&[other.id]).is_err());
    assert_eq!(
        store.try_records_by_ids(&[first.id]).unwrap()[0].page_speed,
        Some(snapshot())
    );
    store.try_save_page_speed(first.id, latest.clone()).unwrap();
    assert_eq!(
        store.try_records_by_ids(&[first.id]).unwrap()[0].page_speed,
        Some(latest)
    );
}
