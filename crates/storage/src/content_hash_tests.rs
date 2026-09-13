use super::*;

fn page(path: &str, response_hash: &str, content_hash: &str, context: &str) -> CrawlRecord {
    let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
    record.status_code = Some(200);
    record.content_type = Some("text/html".into());
    record.response_hash = Some(response_hash.into());
    record.content_hash = Some(content_hash.into());
    record.content_hash_context = Some(context.into());
    record
}

#[test]
fn content_hash_round_trips_and_updates_without_changing_response_duplicate_evidence() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let first = store.upsert(page("first", "raw-a", "content-a", "context-a"));
        store.upsert(page("same-content", "raw-b", "content-a", "context-a"));
        store.upsert(page("same-response", "raw-a", "content-b", "context-b"));
        let selected = store.try_records_by_ids(&[first.id]).unwrap();
        assert_eq!(selected[0].content_hash.as_deref(), Some("content-a"));
        assert_eq!(
            selected[0].content_hash_context.as_deref(),
            Some("context-a")
        );
        let query = GridQuery {
            view: IssueView::ExactDuplicate,
            sort_by: Some("url".into()),
            ..GridQuery::default()
        };
        let duplicates = store.query(query.clone());
        assert_eq!(duplicates.total, 2);
        assert_eq!(
            duplicates
                .rows
                .iter()
                .map(|row| row.url.as_str())
                .collect::<Vec<_>>(),
            [
                "https://example.test/first",
                "https://example.test/same-response"
            ]
        );
        for (hash, context) in [
            (Some("changed-content"), Some("changed-context")),
            (None, None),
        ] {
            let mut updated = first.clone();
            updated.content_hash = hash.map(str::to_string);
            updated.content_hash_context = context.map(str::to_string);
            store.upsert(updated);
            let restored = store.try_records_by_ids(&[first.id]).unwrap();
            assert_eq!(restored[0].content_hash.as_deref(), hash);
            assert_eq!(restored[0].content_hash_context.as_deref(), context);
            assert_eq!(restored[0].response_hash.as_deref(), Some("raw-a"));
            assert_eq!(store.query(query.clone()).total, 2);
        }
    }
}

#[test]
fn content_hash_migrations_and_old_json_preserve_unknown_values_and_response_hashes() {
    for legacy in [false, true] {
        for rebuild in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "ferrous-frog-content-hash-{}-{}.sqlite3",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let store = SqliteStore::open(&path).unwrap();
            store
                .try_upsert(page("saved", "raw", "content", "context"))
                .unwrap();
            {
                let conn = store.connection().unwrap();
                if legacy {
                    conn.execute_batch(
                        "ALTER TABLE crawl_records DROP COLUMN content_hash;
                         ALTER TABLE crawl_records DROP COLUMN content_hash_context;",
                    )
                    .unwrap();
                }
                if rebuild {
                    conn.execute_batch(
                        "CREATE UNIQUE INDEX old_final_url_unique ON crawl_records(final_url)",
                    )
                    .unwrap();
                }
            }
            drop(store);
            let reopened = SqliteStore::open(&path).unwrap();
            let records = reopened.try_records().unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].response_hash.as_deref(), Some("raw"));
            assert_eq!(
                records[0].content_hash.as_deref(),
                (!legacy).then_some("content")
            );
            assert_eq!(
                records[0].content_hash_context.as_deref(),
                (!legacy).then_some("context")
            );
            let archived = serde_json::to_value(&records[0]).unwrap();
            let restored: CrawlRecord = serde_json::from_value(archived).unwrap();
            assert_eq!(restored.content_hash, records[0].content_hash);
            assert_eq!(
                restored.content_hash_context,
                records[0].content_hash_context
            );
            drop(reopened);
            std::fs::remove_file(path).unwrap();
        }
    }
    let mut old = serde_json::to_value(page("archive", "raw", "content", "context")).unwrap();
    old.as_object_mut().unwrap().remove("contentHash");
    old.as_object_mut().unwrap().remove("contentHashContext");
    let restored: CrawlRecord = serde_json::from_value(old).unwrap();
    assert!(restored.content_hash.is_none());
    assert!(restored.content_hash_context.is_none());
    assert_eq!(restored.response_hash.as_deref(), Some("raw"));
}
