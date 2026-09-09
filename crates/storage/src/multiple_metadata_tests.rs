use super::*;

fn page(path: &str, title_count: Option<usize>, meta_count: Option<usize>) -> CrawlRecord {
    let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
    row.status_code = Some(200);
    row.content_type = Some("text/html".into());
    row.title = Some("Retained title".into());
    row.meta_description = Some("Retained description".into());
    let mut value = serde_json::to_value(row).unwrap();
    value["titleCount"] = serde_json::json!(title_count);
    value["metaDescriptionCount"] = serde_json::json!(meta_count);
    serde_json::from_value(value).unwrap()
}

fn query(view: &str) -> GridQuery {
    GridQuery {
        view: serde_json::from_value(serde_json::json!(view))
            .expect("Multiple metadata view must be supported"),
        sort_by: Some("url".into()),
        ..GridQuery::default()
    }
}

#[test]
fn multiple_metadata_views_summaries_and_paging_agree_for_both_stores() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut duplicate = page("both", Some(2), Some(3));
    duplicate.storage_key = "list:30:both".into();
    duplicate.list_position = Some(30);
    let mut empty = page("empty", Some(2), Some(2));
    empty.title = None;
    empty.meta_description = None;
    let mut incomplete = page("incomplete", Some(2), Some(2));
    incomplete.indexability_status = "Response body incomplete".into();
    let mut image = page("image", Some(2), Some(2));
    image.content_type = Some("image/png".into());
    let mut failed = page("failed", Some(2), Some(2));
    failed.status_code = Some(503);
    for row in [
        page("both", Some(2), Some(3)),
        duplicate,
        empty,
        incomplete,
        image,
        failed,
        page("title", Some(3), Some(1)),
        page("meta", Some(1), Some(2)),
        page("unknown", None, None),
        page("zero", Some(0), Some(0)),
        page("single", Some(1), Some(1)),
    ] {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    for summary in [
        memory.progress_summary(),
        sqlite.try_progress_summary().unwrap(),
    ] {
        assert_eq!((summary.title_multiple, summary.meta_multiple), (4, 4));
    }
    for (view, paths) in [
        ("titleMultiple", ["both", "both", "empty", "title"]),
        ("metaMultiple", ["both", "both", "empty", "meta"]),
    ] {
        for response in [
            memory.query(query(view)),
            sqlite.try_query(query(view)).unwrap(),
        ] {
            assert_eq!(
                response
                    .rows
                    .iter()
                    .map(|row| row.url.clone())
                    .collect::<Vec<_>>(),
                paths.map(|path| format!("https://example.test/{path}"))
            );
            assert_eq!(response.total, 4);
            assert_eq!(serde_json::to_value(response.summary).unwrap()[view], 4);
        }
        for (offset, search, segment) in [
            (0, None, None),
            (1, None, None),
            (0, Some("both"), None),
            (0, None, Some("empty|title")),
        ] {
            let query = GridQuery {
                offset,
                limit: 1,
                global_search: search.map(str::to_string),
                segment_pattern: segment.map(str::to_string),
                segment_regex: true,
                ..query(view)
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
    for (column, expected_paths) in [
        ("titleCount", ["unknown", "zero", "meta"]),
        ("metaDescriptionCount", ["unknown", "zero", "title"]),
    ] {
        for (offset, path) in expected_paths.into_iter().enumerate() {
            let query = GridQuery {
                offset,
                limit: 1,
                sort_by: Some(column.into()),
                ..GridQuery::default()
            };
            for response in [
                memory.query(query.clone()),
                sqlite.try_query(query).unwrap(),
            ] {
                assert_eq!(response.rows[0].url, format!("https://example.test/{path}"));
            }
        }
    }
    let mut changed = page("both", Some(1), Some(1));
    changed.title = Some("Updated retained title".into());
    sqlite.try_upsert(changed).unwrap();
    assert_eq!(sqlite.try_query(query("titleMultiple")).unwrap().total, 3);
    assert_eq!(sqlite.try_query(query("metaMultiple")).unwrap().total, 3);
    sqlite.connection().unwrap().execute("UPDATE crawl_records SET response_time_ms = 'invalid off-page value' WHERE url = 'https://example.test/title'", []).unwrap();
    assert!(sqlite.try_records().is_err());
    assert_eq!(
        sqlite
            .try_query(GridQuery {
                limit: 1,
                ..query("titleMultiple")
            })
            .unwrap()
            .rows
            .len(),
        1
    );
}

#[test]
fn multiple_metadata_counts_migrate_and_round_trip_without_inventing_old_evidence() {
    for measured in [false, true] {
        let path = std::env::temp_dir().join(format!(
            "ferrous-frog-metadata-{}-{}-{measured}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteStore::open(&path).unwrap();
        store.try_upsert(page("saved", Some(2), Some(3))).unwrap();
        {
            let conn = store.connection().unwrap();
            if !measured {
                for column in ["title_count", "meta_description_count"] {
                    if column_exists(&conn, "crawl_records", column).unwrap() {
                        conn.execute_batch(&format!(
                            "ALTER TABLE crawl_records DROP COLUMN {column}"
                        ))
                        .unwrap();
                    }
                }
            }
            conn.execute_batch(
                "CREATE UNIQUE INDEX old_final_url_unique ON crawl_records(final_url)",
            )
            .unwrap();
        }
        drop(store);
        let reopened = SqliteStore::open(&path).unwrap();
        let rows = reopened.try_records().unwrap();
        let value = serde_json::to_value(&rows[0]).unwrap();
        assert_eq!(
            value["titleCount"],
            if measured {
                serde_json::json!(2)
            } else {
                serde_json::Value::Null
            }
        );
        assert_eq!(
            value["metaDescriptionCount"],
            if measured {
                serde_json::json!(3)
            } else {
                serde_json::Value::Null
            }
        );
        assert_eq!(rows[0].title.as_deref(), Some("Retained title"));
        assert_eq!(
            reopened.try_query(query("titleMultiple")).unwrap().total,
            usize::from(measured)
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    let mut archive = serde_json::to_value(page("archive", Some(2), Some(3))).unwrap();
    archive.as_object_mut().unwrap().remove("titleCount");
    archive
        .as_object_mut()
        .unwrap()
        .remove("metaDescriptionCount");
    let restored: CrawlRecord = serde_json::from_value(archive).unwrap();
    let restored = serde_json::to_value(restored).unwrap();
    assert_eq!(restored["titleCount"], serde_json::Value::Null);
    assert_eq!(restored["metaDescriptionCount"], serde_json::Value::Null);
    let mut summary = serde_json::to_value(CrawlSummary::default()).unwrap();
    for key in ["titleMultiple", "metaMultiple"] {
        summary.as_object_mut().unwrap().remove(key);
    }
    let restored: CrawlSummary = serde_json::from_value(summary).unwrap();
    let restored = serde_json::to_value(restored).unwrap();
    assert_eq!(restored["titleMultiple"], 0);
    assert_eq!(restored["metaMultiple"], 0);
}
