use super::*;

fn state_with_store(store: ActiveStore) -> AppState {
    AppState {
        store: Mutex::new(store),
        control: Mutex::new(None),
        crawl_task: tokio::sync::Mutex::new(None),
        current_session_id: Mutex::new(None),
        frontend_ready: AtomicBool::new(true),
        exit_confirmed: AtomicBool::new(false),
    }
}

fn archive_edge(position: u32) -> LinkEdge {
    LinkEdge {
        id: 0,
        source_url: "https://example.test/source".into(),
        target_url: "https://example.test/final".into(),
        anchor_text: "Résumé, \"link\"\n🐸".into(),
        rel: "nofollow".into(),
        rel_nofollow: true,
        link_type: ferrous_frog_storage::LinkType::Internal,
        source_status_code: Some(200),
        target_status_code: None,
        source_depth: 0,
        target_depth: None,
        source_position: position,
        discovery_order: u64::from(position),
    }
}

#[tokio::test]
async fn crawl_archive_round_trips_order_unicode_frontier_and_import_seed() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(ferrous_frog_storage::SqliteStore::in_memory().unwrap()),
    ] {
        for (name, position) in [("inserted-first", 20), ("list-first", 1)] {
            let mut row = CrawlRecord::pending(format!("https://example.test/{name}"), 0);
            row.storage_key = format!("list:{position}");
            row.list_position = Some(position);
            row.final_url = "https://example.test/final".into();
            row.title = Some(format!("{name}: Résumé, \"title\"\n🐸"));
            row.title_count = Some(2);
            row.meta_description_count = Some(3);
            store.upsert(row);
        }
        store.add_link_edge(archive_edge(3));
        store.add_image_assets(
            "https://example.test/source",
            vec![image_alt_asset(
                "https://example.test/source",
                "https://example.test/image.png",
                5,
                Some("Résumé, \"alt\"\n🐸"),
            )],
        );
        store.save_frontier_state(CrawlFrontierState {
            queued: [2, 1]
                .map(|position| ferrous_frog_storage::CrawlFrontierItem {
                    url: "https://example.test/queued".into(),
                    depth: 2,
                    from_sitemap: true,
                    storage_key: format!("queued:{position}"),
                    list_position: Some(position),
                    list_duplicate_index: position - 1,
                })
                .to_vec(),
            seen: vec!["seen-z".into(), "seen-a".into()],
            crawled: 2,
        });
        let expected = CrawlArchive {
            schema_version: 1,
            exported_at_ms: 42,
            records: store.records(),
            link_edges: store.link_edges(LinkEdgeQuery::default()).edges,
            image_assets: store.image_assets(ImageAssetQuery::default()).images,
            frontier_state: store.load_frontier_state(),
        };
        assert!(expected.records.iter().all(
            |row| row.first_inlink_source_url.as_deref() == Some("https://example.test/source")
        ));
        let expected_seed = expected.records[0].url.clone();
        let state = state_with_store(store.clone());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("crawl.ffcrawl.json");
        let result = export_crawl_archive_file(&state, path.clone(), 42)
            .await
            .unwrap();
        assert_eq!(result.row_count, 2);
        let bytes = fs::read(&path).unwrap();
        let archive: CrawlArchive = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            serde_json::to_value(&archive).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(
            compare_records(&archive.records, &store.records()).changed,
            0
        );
        let import_directory = tempfile::tempdir().unwrap();
        let imported =
            import_archive_into_session(&state, import_directory.path(), archive).unwrap();
        assert_eq!(imported.session.start_url, expected_seed);
        assert_eq!(
            imported.session.mode,
            ferrous_frog_crawler_core::CrawlMode::List
        );
        assert_eq!(
            (
                imported.records,
                imported.link_edges,
                imported.image_assets,
                imported.frontier_items
            ),
            (2, 1, 1, 2)
        );
        let active = state.store.lock().unwrap().clone();
        assert_eq!(active.records().len(), 2);
        assert_eq!(
            active.load_frontier_state().unwrap().queued,
            expected.frontier_state.as_ref().unwrap().queued
        );
        assert!(
            export_crawl_archive_file(&state, path.clone(), 43)
                .await
                .is_err()
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn crawl_archive_rejects_running_paused_and_closing_without_creating_files() {
    for paused in [false, true] {
        let state = state_with_store(ActiveStore::memory());
        let control = CrawlControl::default();
        if paused {
            control.pause();
        }
        *state.control.lock().unwrap() = Some(control);
        *state.crawl_task.lock().await = Some(tauri::async_runtime::spawn(std::future::pending()));
        let directory = tempfile::tempdir().unwrap();
        let error = export_crawl_archive_file(&state, directory.path().join("active.json"), 1)
            .await
            .unwrap_err();
        assert!(error.contains("stop") && error.contains("crawl"), "{error}");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        state.crawl_task.lock().await.take().unwrap().abort();
    }
    let state = state_with_store(ActiveStore::memory());
    state.exit_confirmed.store(true, Ordering::SeqCst);
    let directory = tempfile::tempdir().unwrap();
    assert!(
        export_crawl_archive_file(&state, directory.path().join("closing.json"), 1)
            .await
            .unwrap_err()
            .contains("closing")
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn crawl_archive_pages_every_collection_and_preserves_empty_schema_fields() {
    let store = ActiveStore::Sqlite(ferrous_frog_storage::SqliteStore::in_memory().unwrap());
    let directory = tempfile::tempdir().unwrap();
    let state = state_with_store(store.clone());
    let empty_path = directory.path().join("empty.json");
    assert_eq!(
        export_crawl_archive_file(&state, empty_path.clone(), 1)
            .await
            .unwrap()
            .row_count,
        0
    );
    let empty: CrawlArchive = serde_json::from_slice(&fs::read(empty_path).unwrap()).unwrap();
    assert!(
        empty.records.is_empty() && empty.link_edges.is_empty() && empty.image_assets.is_empty()
    );
    assert_eq!(empty.frontier_state, None);
    let count = EXPORT_STREAM_PAGE_SIZE + 1;
    for index in 0..count {
        let mut row = CrawlRecord::pending(format!("https://example.test/page-{index}"), 0);
        row.list_position = Some((count - index) as u32);
        store.upsert(row);
    }
    for index in 0..count {
        store.add_link_edge(archive_edge(index as u32));
    }
    store.add_image_assets(
        "https://example.test/source",
        (0..count)
            .rev()
            .map(|index| {
                image_alt_asset(
                    "https://example.test/source",
                    &format!("https://example.test/image-{index}.png"),
                    index as u32,
                    Some("An image"),
                )
            })
            .collect(),
    );
    let path = directory.path().join("pages.json");
    assert_eq!(
        export_crawl_archive_file(&state, path.clone(), 2)
            .await
            .unwrap()
            .row_count,
        count
    );
    let archive: CrawlArchive = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        (
            archive.records.len(),
            archive.link_edges.len(),
            archive.image_assets.len()
        ),
        (count, count, count)
    );
    assert_eq!(
        archive.records.first().unwrap().url,
        format!("https://example.test/page-{}", count - 1)
    );
    assert_eq!(
        archive.records.last().unwrap().url,
        "https://example.test/page-0"
    );
    for (index, edge) in archive.link_edges.iter().enumerate() {
        assert_eq!(edge.source_position as usize, index);
    }
    for (index, image) in archive.image_assets.iter().enumerate() {
        assert_eq!(image.source_position as usize, index);
    }
}

#[test]
fn crawl_archive_array_passes_the_old_cap_and_rejects_changed_or_short_pages() {
    let total = 1_000_001;
    let mut offsets = Vec::new();
    let count = write_archive_array(&mut std::io::sink(), "fixture", |offset| {
        offsets.push(offset);
        Ok((
            vec![0_u8; (total - offset).min(EXPORT_STREAM_PAGE_SIZE)],
            total,
        ))
    })
    .unwrap();
    assert_eq!(count, total);
    assert_eq!(offsets.last(), Some(&1_000_000));
    assert_eq!(offsets.len(), 101);

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("unchanged.json");
    fs::write(&path, "previous export").unwrap();
    for (second_count, second_total) in [(0, 10_001), (1, 10_002), (1, 10_000)] {
        let error = write_atomic_export(&path, |file| {
            write_archive_array(file, "fixture", |offset| {
                if offset == 0 {
                    Ok((
                        vec![0_u8; EXPORT_STREAM_PAGE_SIZE],
                        EXPORT_STREAM_PAGE_SIZE + 1,
                    ))
                } else {
                    assert_eq!(offset, EXPORT_STREAM_PAGE_SIZE);
                    Ok((vec![0_u8; second_count], second_total))
                }
            })
        })
        .unwrap_err();
        assert!(
            error.contains("fixture") && error.contains("changed or ended early"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "previous export");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn crawl_archive_late_image_and_frontier_errors_are_fallible_and_atomic() {
    for corrupt_image in [true, false] {
        let database = tempfile::tempdir().unwrap();
        let database_path = database.path().join("crawl.sqlite3");
        let store = ActiveStore::sqlite(&database_path).unwrap();
        store.upsert(CrawlRecord::pending(
            "https://example.test/source".into(),
            0,
        ));
        store.add_link_edge(archive_edge(0));
        store.add_image_assets(
            "https://example.test/source",
            vec![image_alt_asset(
                "https://example.test/source",
                "https://example.test/image.png",
                0,
                Some("Image"),
            )],
        );
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch(if corrupt_image {
                "UPDATE image_assets SET width = 'invalid width'"
            } else {
                "DROP TABLE crawl_frontier_seen"
            })
            .unwrap();
        let mut partial = Vec::new();
        let error = write_crawl_archive_stream(&store, 1, &mut partial).unwrap_err();
        assert!(!error.is_empty());
        assert!(String::from_utf8_lossy(&partial).contains("\"linkEdges\":"));
        assert!(serde_json::from_slice::<CrawlArchive>(&partial).is_err());

        let state = state_with_store(store);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("previous.json");
        fs::write(&path, "previous export").unwrap();
        let error = export_crawl_archive_file(&state, path.clone(), 1)
            .await
            .unwrap_err();
        assert!(
            !error.contains("worker failed") && !error.contains("panicked"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "previous export");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(state.crawl_task.try_lock().is_ok());
        assert!(!state.store.is_poisoned());
    }
}

#[test]
fn crawl_archive_write_and_flush_errors_do_not_publish_partial_files() {
    struct FailWriter<'a> {
        file: &'a mut fs::File,
        remaining: usize,
        fail_flush: bool,
    }
    impl Write for FailWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Err(std::io::Error::other("fixture write failure"));
            }
            let written = self.file.write(&bytes[..bytes.len().min(self.remaining)])?;
            self.remaining -= written;
            Ok(written)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if self.fail_flush {
                return Err(std::io::Error::other("fixture flush failure"));
            }
            self.file.flush()
        }
    }
    let store = ActiveStore::memory();
    store.upsert(CrawlRecord::pending(
        "https://example.test/source".into(),
        0,
    ));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("previous.json");
    fs::write(&path, "previous export").unwrap();
    for (remaining, fail_flush) in [(128, false), (usize::MAX, true)] {
        let error = write_atomic_export(&path, |file| {
            let mut writer = BufWriter::new(FailWriter {
                file,
                remaining,
                fail_flush,
            });
            write_crawl_archive_stream(&store, 1, &mut writer)
        })
        .unwrap_err();
        assert!(
            error.contains(if fail_flush {
                "flush failure"
            } else {
                "write failure"
            }),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "previous export");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn audit_workbook_export_preserves_existing_file_and_reports_source_count() {
    let request: ExportFileRequest = serde_json::from_value(serde_json::json!({
        "kind": "auditWorkbook", "query": null, "graphQuery": null
    }))
    .unwrap();
    assert!(matches!(request.kind, ExportFileKind::AuditWorkbook));
    let store = ActiveStore::memory();
    store.upsert(CrawlRecord::pending("https://example.test/".into(), 0));
    let state = state_with_store(store);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.xlsx");
    let result = export_audit_workbook(&state, path.clone()).await.unwrap();
    assert_eq!(result.path, path.to_string_lossy());
    assert_eq!(serde_json::to_value(result).unwrap()["rowCount"], 1);
    let original = fs::read(&path).unwrap();
    assert!(original.starts_with(b"PK"));

    assert!(export_audit_workbook(&state, path.clone()).await.is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn audit_workbook_export_refuses_running_and_paused_crawls() {
    for paused in [false, true] {
        let state = state_with_store(ActiveStore::memory());
        let control = CrawlControl::default();
        if paused {
            control.pause();
        }
        *state.control.lock().unwrap() = Some(control);
        *state.crawl_task.lock().await = Some(tauri::async_runtime::spawn(std::future::pending()));
        let directory = tempfile::tempdir().unwrap();
        let error = export_audit_workbook(&state, directory.path().join("audit.xlsx"))
            .await
            .unwrap_err();
        assert!(error.contains("stop") && error.contains("crawl"), "{error}");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        state.crawl_task.lock().await.take().unwrap().abort();
    }
}

#[tokio::test]
async fn audit_workbook_export_cleans_up_after_writer_or_storage_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.xlsx");
    let store = ActiveStore::memory();
    let mut record = CrawlRecord::pending("https://example.test/".into(), 0);
    record.title = Some("x".repeat(32_768));
    store.upsert(record);
    let error = export_audit_workbook(&state_with_store(store), path.clone())
        .await
        .unwrap_err();
    assert!(
        error.contains("32767") || error.contains("32,767"),
        "{error}"
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);

    let database_dir = tempfile::tempdir().unwrap();
    let database_path = database_dir.path().join("crawl.sqlite3");
    let store = ActiveStore::sqlite(&database_path).unwrap();
    Connection::open(&database_path)
        .unwrap()
        .execute_batch("DROP TABLE crawl_records")
        .unwrap();
    let error = export_audit_workbook(&state_with_store(store), path)
        .await
        .unwrap_err();
    assert!(error.contains("no such table"), "{error}");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn selected_csv_exports_exact_ids_in_requested_order_and_matches_clipboard() {
    let store = ActiveStore::memory();
    let first = store.upsert(CrawlRecord::pending("https://example.test/first".into(), 0));
    store.upsert(CrawlRecord::pending(
        "https://example.test/excluded".into(),
        0,
    ));
    let mut third = CrawlRecord::pending("https://example.test/third".into(), 1);
    third.title = Some("Résumé, \"Frog\"\nNext line 🐸".into());
    let third = store.upsert(third);
    let state = state_with_store(store);
    let ids = vec![third.id, first.id];
    let text = selected_csv_text(&state, ids.clone()).await.unwrap();
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let title_column = reader
        .headers()
        .unwrap()
        .iter()
        .position(|header| header == "title")
        .unwrap();
    let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(&rows[0][1], "https://example.test/third");
    assert_eq!(&rows[1][1], "https://example.test/first");
    assert_eq!(&rows[0][title_column], "Résumé, \"Frog\"\nNext line 🐸");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected.csv");
    let result = export_selected_csv_file(&state, path.clone(), ids.clone())
        .await
        .unwrap();
    assert_eq!(result.row_count, 2);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert!(
        export_selected_csv_file(&state, path.clone(), ids)
            .await
            .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn selected_csv_rejects_empty_duplicate_unknown_and_excessive_ids_without_a_file() {
    let store = ActiveStore::memory();
    let record = store.upsert(CrawlRecord::pending("https://example.test/".into(), 0));
    let state = state_with_store(store);
    let directory = tempfile::tempdir().unwrap();
    for ids in [
        vec![],
        vec![0],
        vec![record.id, record.id],
        vec![999],
        (1..=1_001).collect(),
        vec![9_007_199_254_740_992],
    ] {
        assert!(
            selected_csv_text(&state, ids.clone()).await.is_err(),
            "{ids:?}"
        );
        assert!(
            export_selected_csv_file(&state, directory.path().join("selected.csv"), ids)
                .await
                .is_err()
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn selected_csv_accepts_one_thousand_sqlite_rows_without_truncation() {
    let store = ActiveStore::Sqlite(ferrous_frog_storage::SqliteStore::in_memory().unwrap());
    let mut ids = (0..1_000)
        .map(|index| {
            store
                .upsert(CrawlRecord::pending(
                    format!("https://example.test/{index}"),
                    0,
                ))
                .id
        })
        .collect::<Vec<_>>();
    ids.reverse();
    let text = selected_csv_text(&state_with_store(store), ids.clone())
        .await
        .unwrap();
    let rows = csv::Reader::from_reader(text.as_bytes())
        .records()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 1_000);
    assert_eq!(
        rows.iter()
            .map(|row| row[0].parse::<u64>().unwrap())
            .collect::<Vec<_>>(),
        ids
    );
}

#[tokio::test]
async fn selected_csv_clipboard_limit_is_explicit_and_file_export_remains_available() {
    let store = ActiveStore::memory();
    let mut record = CrawlRecord::pending("https://example.test/large".into(), 0);
    record.title = Some("x".repeat(4 * 1024 * 1024));
    let record = store.upsert(record);
    let state = state_with_store(store);
    let error = selected_csv_text(&state, vec![record.id])
        .await
        .unwrap_err();
    assert!(error.contains("4 MiB") && error.contains("file"), "{error}");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.csv");
    assert_eq!(
        export_selected_csv_file(&state, path.clone(), vec![record.id])
            .await
            .unwrap()
            .row_count,
        1
    );
    assert!(fs::metadata(path).unwrap().len() > 4 * 1024 * 1024);
}

#[tokio::test]
async fn queued_csv_exports_frontier_membership_and_empty_headers_during_active_crawls() {
    let store = ActiveStore::memory();
    let mut fetched = CrawlRecord::pending("https://example.test/fetched".into(), 0);
    fetched.status_code = Some(200);
    store.upsert(fetched);
    store.upsert(CrawlRecord::pending(
        "https://example.test/pending-but-not-queued".into(),
        0,
    ));
    let queued = [2, 1].map(|position| ferrous_frog_storage::CrawlFrontierItem {
        url: "https://example.test/résumé?x=1,2".into(),
        depth: 3,
        from_sitemap: true,
        storage_key: format!("list:{position}:https://example.test/résumé?x=1,2"),
        list_position: Some(position),
        list_duplicate_index: position - 1,
    });
    store.save_frontier_state(CrawlFrontierState {
        queued: queued.to_vec(),
        seen: vec!["https://example.test/seen-only".into()],
        crawled: 1,
    });
    let state = state_with_store(store.clone());
    *state.crawl_task.lock().await = Some(tauri::async_runtime::spawn(std::future::pending()));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("queue.csv");
    let result = export_queued_urls_file(&state, path.clone()).await.unwrap();
    assert_eq!(result.row_count, 2);
    let mut reader = csv::Reader::from_path(path).unwrap();
    assert_eq!(
        reader.headers().unwrap().iter().collect::<Vec<_>>(),
        [
            "url",
            "depth",
            "from_sitemap",
            "storage_key",
            "list_position",
            "list_duplicate_index"
        ]
    );
    let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(&rows[0][0], queued[0].url);
    assert_eq!(&rows[0][4], "2");
    assert_eq!(&rows[1][4], "1");
    assert_eq!(&rows[0][2], "true");
    store.clear_frontier_state();
    let path = directory.path().join("empty.csv");
    assert_eq!(
        export_queued_urls_file(&state, path.clone())
            .await
            .unwrap()
            .row_count,
        0
    );
    assert_eq!(csv::Reader::from_path(path).unwrap().records().count(), 0);
    state.crawl_task.lock().await.take().unwrap().abort();
}

#[tokio::test]
async fn queued_csv_storage_error_preserves_existing_exports() {
    let database_dir = tempfile::tempdir().unwrap();
    let database_path = database_dir.path().join("crawl.sqlite3");
    let state = state_with_store(ActiveStore::sqlite(&database_path).unwrap());
    Connection::open(database_path)
        .unwrap()
        .execute_batch("DROP TABLE crawl_frontier_queue")
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("queue.csv");
    fs::write(&path, "previous export").unwrap();
    let error = export_queued_urls_file(&state, path.clone())
        .await
        .unwrap_err();
    assert!(error.contains("no such table"), "{error}");
    assert_eq!(fs::read_to_string(path).unwrap(), "previous export");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn filtered_xlsx_file_preserves_filters_and_previous_files_on_failure() {
    let store = ActiveStore::memory();
    for (path, status) in [("a", 404), ("b", 200), ("c", 404)] {
        let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
        record.status_code = Some(status);
        store.upsert(record);
    }
    let state = state_with_store(store.clone());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("filtered.xlsx");
    let query = GridQuery {
        offset: 100,
        limit: 1,
        view: ferrous_frog_storage::IssueView::Status4xx,
        ..GridQuery::default()
    };
    assert_eq!(
        export_xlsx_file(&state, path.clone(), query.clone())
            .await
            .unwrap()
            .row_count,
        2
    );
    let original = fs::read(&path).unwrap();
    assert!(original.starts_with(b"PK"));
    assert!(
        export_xlsx_file(&state, path.clone(), query.clone())
            .await
            .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let mut bad = CrawlRecord::pending("https://example.test/a".into(), 0);
    bad.status_code = Some(404);
    bad.title = Some("x".repeat(32_768));
    store.upsert(bad);
    assert!(export_xlsx_file(&state, path.clone(), query).await.is_err());
    assert_eq!(fs::read(path).unwrap(), original);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn filtered_xlsx_requires_idle_and_legacy_bytes_reject_excessive_windows() {
    let state = state_with_store(ActiveStore::memory());
    *state.crawl_task.lock().await = Some(tauri::async_runtime::spawn(std::future::pending()));
    let directory = tempfile::tempdir().unwrap();
    let error = export_xlsx_file(
        &state,
        directory.path().join("filtered.xlsx"),
        GridQuery::default(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("stop"), "{error}");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    assert!(
        xlsx_window_bytes(&state, GridQuery::default())
            .await
            .unwrap()
            .starts_with(b"PK")
    );
    let error = xlsx_window_bytes(
        &state,
        GridQuery {
            limit: 10_001,
            ..GridQuery::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        error.contains("10,000") && error.contains("export_file"),
        "{error}"
    );
    state.crawl_task.lock().await.take().unwrap().abort();
}

fn image_alt_asset(
    page_url: &str,
    image_url: &str,
    position: u32,
    alt: Option<&str>,
) -> ImageAsset {
    ImageAsset {
        id: 0,
        page_url: page_url.into(),
        image_url: image_url.into(),
        alt_text: alt.map(str::to_string),
        alt_len: alt.map_or(0, |text| text.chars().count() as u32),
        missing_alt: alt.is_none(),
        alt_too_long: false,
        width: None,
        height: None,
        source_position: position,
        size_bytes: None,
        oversized: false,
    }
}

#[tokio::test]
async fn image_queries_use_a_guarded_worker_and_propagate_sqlite_errors_without_panicking() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("crawl.sqlite3");
    for (store, paused) in [
        (ActiveStore::memory(), false),
        (ActiveStore::sqlite(&database_path).unwrap(), true),
    ] {
        let page_url = "https://example.test/résumé";
        store.add_image_assets(
            page_url,
            vec![
                image_alt_asset(page_url, "https://example.test/three.png", 3, None),
                image_alt_asset(page_url, "https://example.test/two.png", 2, Some("Has alt")),
                image_alt_asset(page_url, "https://example.test/one.png", 1, None),
            ],
        );
        store.add_image_assets(
            "https://example.test/other",
            vec![image_alt_asset(
                "https://example.test/other",
                "https://example.test/one.png",
                1,
                None,
            )],
        );
        let state = Arc::new(state_with_store(store));
        let control = CrawlControl::default();
        if paused {
            control.pause();
        }
        *state.control.lock().unwrap() = Some(control);
        *state.crawl_task.lock().await = Some(tauri::async_runtime::spawn(std::future::pending()));
        let worker_state = state.clone();
        let caller_thread = std::thread::current().id();
        let response = with_store_worker(&state, false, move |store| {
            assert_ne!(std::thread::current().id(), caller_thread);
            assert!(
                worker_state.crawl_task.try_lock().is_err(),
                "workspace changes must await the query"
            );
            assert!(
                worker_state.store.try_lock().is_ok(),
                "the app-state mutex must not be held during database work"
            );
            query_store_images(
                store,
                ImageAssetQuery {
                    offset: 1,
                    limit: 1,
                    page_url: Some(page_url.into()),
                    missing_alt_only: true,
                    global_search: Some("RÉSUMÉ".into()),
                    sort_by: Some("sourcePosition".into()),
                    sort_dir: ferrous_frog_storage::SortDirection::Desc,
                    ..ImageAssetQuery::default()
                },
            )
        })
        .await
        .unwrap();
        assert_eq!(response.total, 2);
        assert_eq!(response.images.len(), 1);
        assert_eq!(response.images[0].source_position, 1);
        assert_eq!(response.images[0].page_url, page_url);
        assert_eq!(
            serde_json::to_value(response).unwrap()["images"][0]["imageUrl"],
            "https://example.test/one.png"
        );
        if paused {
            Connection::open(&database_path)
                .unwrap()
                .execute_batch("DROP TABLE image_assets")
                .unwrap();
            let error = with_store_worker(&state, false, |store| {
                query_store_images(store, ImageAssetQuery::default())
            })
            .await
            .unwrap_err();
            assert!(error.contains("no such table"), "{error}");
            assert!(
                !error.contains("worker failed") && !error.contains("panicked"),
                "{error}"
            );
            assert!(
                state.crawl_task.try_lock().is_ok(),
                "read errors must release the lifecycle guard"
            );
            assert!(!state.store.is_poisoned());
        }
        state.crawl_task.lock().await.take().unwrap().abort();
        state.exit_confirmed.store(true, Ordering::SeqCst);
        let error = with_store_worker(&state, false, |store| {
            query_store_images(store, ImageAssetQuery::default())
        })
        .await
        .unwrap_err();
        assert!(error.contains("closing"), "{error}");
    }
}

#[tokio::test]
async fn related_detail_queries_keep_pagination_and_return_sqlite_failures() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("crawl.sqlite3");
    for store in [
        ActiveStore::memory(),
        ActiveStore::sqlite(&database_path).unwrap(),
    ] {
        for (position, anchor) in [(3, "Résumé link"), (2, "Other"), (1, "Résumé first")] {
            store.add_link_edge(LinkEdge {
                id: 0,
                source_url: "https://example.test/source".into(),
                target_url: format!("https://example.test/{position}"),
                anchor_text: anchor.into(),
                rel: String::new(),
                rel_nofollow: false,
                link_type: ferrous_frog_storage::LinkType::Internal,
                source_status_code: None,
                target_status_code: None,
                source_depth: 0,
                target_depth: None,
                source_position: position,
                discovery_order: 0,
            });
        }
        for (path, status) in [("résumé-a", 404), ("résumé-b", 200), ("other", 301)] {
            let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
            record.status_code = Some(status);
            record.in_sitemap = true;
            store.upsert(record);
        }
        let state = state_with_store(store);
        let edges = with_store_worker(&state, false, |store| {
            query_store_link_edges(
                store,
                LinkEdgeQuery {
                    offset: 1,
                    limit: 1,
                    global_search: Some("RÉSUMÉ".into()),
                    sort_by: Some("sourcePosition".into()),
                    sort_dir: ferrous_frog_storage::SortDirection::Desc,
                    ..LinkEdgeQuery::default()
                },
            )
        })
        .await
        .unwrap();
        assert_eq!(edges.total, 2);
        assert_eq!(edges.edges.len(), 1);
        assert_eq!(edges.edges[0].source_position, 1);
        let anchors = with_store_worker(&state, false, |store| {
            query_store_anchor_texts(
                store,
                LinkEdgeQuery {
                    offset: 1,
                    limit: 1,
                    global_search: Some("RÉSUMÉ".into()),
                    sort_by: Some("anchorText".into()),
                    sort_dir: ferrous_frog_storage::SortDirection::Asc,
                    ..LinkEdgeQuery::default()
                },
            )
        })
        .await
        .unwrap();
        assert_eq!(anchors.total, 2);
        assert_eq!(anchors.rows.len(), 1);
        assert_eq!(anchors.rows[0].anchor_text, "Résumé link");
        let sitemap = with_store_worker(&state, false, |store| {
            query_store_sitemap_validation(
                store,
                SitemapValidationQuery {
                    offset: 1,
                    limit: 1,
                    global_search: Some("RÉSUMÉ".into()),
                    sort_by: Some("statusCode".into()),
                    sort_dir: ferrous_frog_storage::SortDirection::Asc,
                },
            )
        })
        .await
        .unwrap();
        assert_eq!(sitemap.total, 2);
        assert_eq!(sitemap.rows.len(), 1);
        assert_eq!(sitemap.rows[0].url, "https://example.test/résumé-a");
        let graph = with_store_worker(&state, false, |store| {
            store
                .try_crawl_graph(CrawlGraphQuery {
                    max_nodes: 2,
                    max_edges: 1,
                    ..CrawlGraphQuery::default()
                })
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.total_edges, 3);
        assert_eq!(graph.nodes[0].url, "https://example.test/résumé-a");
        assert_eq!(graph.nodes[1].status_code, Some(200));
    }
    let state = state_with_store(ActiveStore::sqlite(&database_path).unwrap());
    Connection::open(&database_path)
        .unwrap()
        .execute_batch("DROP TABLE link_edges; DROP TABLE crawl_records;")
        .unwrap();
    for error in [
        with_store_worker(&state, false, |store| {
            query_store_link_edges(store, LinkEdgeQuery::default())
        })
        .await
        .unwrap_err(),
        with_store_worker(&state, false, |store| {
            query_store_anchor_texts(store, LinkEdgeQuery::default())
        })
        .await
        .unwrap_err(),
        with_store_worker(&state, false, |store| {
            query_store_sitemap_validation(store, SitemapValidationQuery::default())
        })
        .await
        .unwrap_err(),
        with_store_worker(&state, false, |store| {
            store
                .try_crawl_graph(CrawlGraphQuery::default())
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap_err(),
    ] {
        assert!(error.contains("no such table"), "{error}");
        assert!(
            !error.contains("worker failed") && !error.contains("panicked"),
            "{error}"
        );
    }
    assert!(state.crawl_task.try_lock().is_ok());
    assert!(!state.store.is_poisoned());
}

#[tokio::test]
async fn graph_query_failures_preserve_existing_exports_and_release_workers() {
    let database = tempfile::tempdir().unwrap();
    let path = database.path().join("crawl.sqlite3");
    let state = state_with_store(ActiveStore::sqlite(&path).unwrap());
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE crawl_records")
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    for (kind, name) in [
        (ExportFileKind::GraphJson, "graph.json"),
        (ExportFileKind::GraphNodesCsv, "nodes.csv"),
        (ExportFileKind::GraphEdgesCsv, "edges.csv"),
    ] {
        let path = directory.path().join(name);
        fs::write(&path, "previous export").unwrap();
        let error = export_graph_file(&state, path.clone(), CrawlGraphQuery::default(), kind)
            .await
            .unwrap_err();
        assert!(error.contains("no such table"), "{error}");
        assert!(!error.contains("worker failed"), "{error}");
        assert_eq!(fs::read_to_string(path).unwrap(), "previous export");
        assert!(state.crawl_task.try_lock().is_ok());
        assert!(!state.store.is_poisoned());
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 3);
}

#[tokio::test]
async fn image_alt_csv_preserves_occurrences_unicode_sizes_flags_and_existing_files() {
    let request: ExportFileRequest =
        serde_json::from_value(serde_json::json!({"kind":"imageAltCsv"})).unwrap();
    assert!(matches!(request.kind, ExportFileKind::ImageAltCsv));
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(ferrous_frog_storage::SqliteStore::in_memory().unwrap()),
    ] {
        let mut fetched = CrawlRecord::pending("https://example.test/hero.png".into(), 1);
        fetched.content_type = Some("image/png".into());
        fetched.status_code = Some(200);
        fetched.size_bytes = 300_000;
        store.upsert(fetched);
        let mut described = image_alt_asset(
            "https://example.test/z",
            "https://example.test/hero.png",
            3,
            Some("Résumé, \"Frog\"\nNext line 🐸"),
        );
        described.width = Some(1280);
        described.height = Some(720);
        store.add_image_assets(
            "https://example.test/z",
            vec![
                described,
                image_alt_asset(
                    "https://example.test/z",
                    "https://example.test/hero.png",
                    8,
                    None,
                ),
            ],
        );
        let mut long_alt = image_alt_asset(
            "https://example.test/a",
            "https://example.test/unfetched.svg",
            9,
            Some(&"x".repeat(120)),
        );
        long_alt.alt_too_long = true;
        store.add_image_assets(
            "https://example.test/a",
            vec![
                image_alt_asset(
                    "https://example.test/a",
                    "https://example.test/hero.png",
                    2,
                    Some(""),
                ),
                long_alt,
            ],
        );
        let state = state_with_store(store);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("images.csv");
        assert_eq!(
            export_image_alt_file(&state, path.clone())
                .await
                .unwrap()
                .row_count,
            4
        );
        let original = fs::read(&path).unwrap();
        let mut reader = csv::Reader::from_reader(original.as_slice());
        assert_eq!(
            reader.headers().unwrap().iter().collect::<Vec<_>>(),
            [
                "page_url",
                "image_url",
                "alt_text",
                "alt_len",
                "missing_alt",
                "alt_too_long",
                "source_position",
                "width",
                "height",
                "size_bytes",
                "oversized",
            ]
        );
        let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter()
                .map(|row| (&row[0], &row[6]))
                .collect::<Vec<_>>(),
            [
                ("https://example.test/a", "2"),
                ("https://example.test/a", "9"),
                ("https://example.test/z", "3"),
                ("https://example.test/z", "8"),
            ]
        );
        assert_eq!(&rows[2][2], "Résumé, \"Frog\"\nNext line 🐸");
        assert_eq!(&rows[2][3], "26");
        assert_eq!(&rows[2][7], "1280");
        assert_eq!(&rows[2][8], "720");
        assert_eq!(&rows[2][9], "300000");
        assert_eq!(&rows[2][10], "true");
        assert_eq!(&rows[0][4], "false");
        assert_eq!(&rows[3][4], "true");
        assert_eq!(&rows[1][5], "true");
        assert_eq!(&rows[1][9], "");
        assert_eq!(&rows[1][10], "false");
        assert!(export_image_alt_file(&state, path.clone()).await.is_err());
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn image_alt_csv_pages_past_ten_thousand_occurrences_and_exports_empty_headers() {
    let store = ActiveStore::Sqlite(ferrous_frog_storage::SqliteStore::in_memory().unwrap());
    let state = state_with_store(store.clone());
    let directory = tempfile::tempdir().unwrap();
    let empty = directory.path().join("empty.csv");
    assert_eq!(
        export_image_alt_file(&state, empty.clone())
            .await
            .unwrap()
            .row_count,
        0
    );
    let mut reader = csv::Reader::from_path(empty).unwrap();
    assert_eq!(reader.headers().unwrap().len(), 11);
    assert_eq!(reader.records().count(), 0);
    store.add_image_assets(
        "https://example.test/",
        (0..10_001)
            .map(|position| {
                image_alt_asset(
                    "https://example.test/",
                    "https://example.test/repeated.svg",
                    position,
                    None,
                )
            })
            .collect(),
    );
    let path = directory.path().join("many.csv");
    assert_eq!(
        export_image_alt_file(&state, path.clone())
            .await
            .unwrap()
            .row_count,
        10_001
    );
    let rows = csv::Reader::from_path(path)
        .unwrap()
        .records()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 10_001);
    assert_eq!(
        rows.iter()
            .map(|row| row[6].parse::<u32>().unwrap())
            .collect::<Vec<_>>(),
        (0..10_001).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn image_alt_csv_rejects_running_or_paused_crawls_and_preserves_files_on_storage_failure() {
    for paused in [false, true] {
        let state = state_with_store(ActiveStore::memory());
        let control = CrawlControl::default();
        if paused {
            control.pause();
        }
        *state.control.lock().unwrap() = Some(control);
        *state.crawl_task.lock().await = Some(tauri::async_runtime::spawn(std::future::pending()));
        let directory = tempfile::tempdir().unwrap();
        let error = export_image_alt_file(&state, directory.path().join("images.csv"))
            .await
            .unwrap_err();
        assert!(error.contains("stop"), "{error}");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        state.crawl_task.lock().await.take().unwrap().abort();
    }
    let database_dir = tempfile::tempdir().unwrap();
    let database_path = database_dir.path().join("crawl.sqlite3");
    let state = state_with_store(ActiveStore::sqlite(&database_path).unwrap());
    Connection::open(database_path)
        .unwrap()
        .execute_batch("DROP TABLE image_assets")
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("images.csv");
    fs::write(&path, "previous export").unwrap();
    let error = export_image_alt_file(&state, path.clone())
        .await
        .unwrap_err();
    assert!(error.contains("no such table"), "{error}");
    assert_eq!(fs::read_to_string(path).unwrap(), "previous export");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn graph_json_file_stream_preserves_content_limits_and_existing_exports() {
    let store = ActiveStore::memory();
    store.upsert(CrawlRecord::pending(
        "https://example.test/résumé".into(),
        0,
    ));
    store.upsert(CrawlRecord::pending(
        "https://example.test/excluded".into(),
        1,
    ));
    let query = CrawlGraphQuery {
        max_nodes: 1,
        ..CrawlGraphQuery::default()
    };
    let expected = serde_json::to_value(store.crawl_graph(query.clone())).unwrap();
    let state = state_with_store(store);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graph.json");
    assert_eq!(
        export_graph_file(
            &state,
            path.clone(),
            query.clone(),
            ExportFileKind::GraphJson
        )
        .await
        .unwrap()
        .row_count,
        1
    );
    let original = fs::read(&path).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&original).unwrap(),
        expected
    );
    assert!(
        export_graph_file(&state, path.clone(), query, ExportFileKind::GraphJson)
            .await
            .is_err()
    );
    assert_eq!(fs::read(path).unwrap(), original);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn graph_csv_files_preserve_queries_unicode_empty_headers_and_previous_exports() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(ferrous_frog_storage::SqliteStore::in_memory().unwrap()),
    ] {
        let source = "https://example.test/résumé?x=1,2";
        for (url, classification) in [
            (source, ferrous_frog_storage::UrlClassification::Internal),
            (
                "https://example.test/target",
                ferrous_frog_storage::UrlClassification::Internal,
            ),
            (
                "https://outside.test/",
                ferrous_frog_storage::UrlClassification::External,
            ),
        ] {
            let mut record = CrawlRecord::pending(url.into(), 0);
            record.classification = classification;
            record.status_code = Some(200);
            store.upsert(record);
        }
        for (target_url, link_type) in [
            (
                "https://example.test/target",
                ferrous_frog_storage::LinkType::Internal,
            ),
            (
                "https://outside.test/",
                ferrous_frog_storage::LinkType::External,
            ),
        ] {
            store.add_link_edge(LinkEdge {
                id: 0,
                source_url: source.into(),
                target_url: target_url.into(),
                anchor_text: "Résumé, \"link\"\nNext line 🐸".into(),
                rel: "nofollow".into(),
                rel_nofollow: true,
                link_type,
                source_status_code: None,
                target_status_code: None,
                source_depth: 0,
                target_depth: None,
                source_position: 7,
                discovery_order: 0,
            });
        }
        let state = state_with_store(store.clone());
        *state.crawl_task.lock().await = Some(tauri::async_runtime::spawn(std::future::pending()));
        for query in [
            CrawlGraphQuery::default(),
            CrawlGraphQuery {
                max_nodes: 2,
                max_edges: 1,
                internal_only: true,
            },
        ] {
            let graph = store.crawl_graph(query.clone());
            assert_eq!(graph.nodes.len(), if query.internal_only { 2 } else { 3 });
            assert_eq!(graph.edges.len(), if query.internal_only { 1 } else { 2 });
            for kind in [ExportFileKind::GraphNodesCsv, ExportFileKind::GraphEdgesCsv] {
                let nodes = matches!(kind, ExportFileKind::GraphNodesCsv);
                let expected = if nodes {
                    ferrous_frog_export::graph_nodes_to_csv_string(&graph.nodes).unwrap()
                } else {
                    link_edges_to_csv_string(&graph.edges).unwrap()
                };
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("graph.csv");
                let result = export_graph_file(&state, path.clone(), query.clone(), kind)
                    .await
                    .unwrap();
                assert_eq!(
                    result.row_count,
                    if nodes {
                        graph.nodes.len()
                    } else {
                        graph.edges.len()
                    }
                );
                assert_eq!(fs::read_to_string(&path).unwrap(), expected);
                let mut reader = csv::Reader::from_reader(expected.as_bytes());
                assert_eq!(reader.headers().unwrap().len(), if nodes { 9 } else { 13 });
                let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
                if !nodes {
                    assert_eq!(&rows[0][1], source);
                    assert_eq!(&rows[0][3], "Résumé, \"link\"\nNext line 🐸");
                }
                let kind = if nodes {
                    ExportFileKind::GraphNodesCsv
                } else {
                    ExportFileKind::GraphEdgesCsv
                };
                assert!(
                    export_graph_file(&state, path.clone(), query.clone(), kind)
                        .await
                        .is_err()
                );
                assert_eq!(fs::read_to_string(path).unwrap(), expected);
                assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
            }
        }
        state.crawl_task.lock().await.take().unwrap().abort();
    }
    let state = state_with_store(ActiveStore::memory());
    let directory = tempfile::tempdir().unwrap();
    for (kind, name, columns) in [
        (ExportFileKind::GraphNodesCsv, "nodes.csv", 9),
        (ExportFileKind::GraphEdgesCsv, "edges.csv", 13),
    ] {
        let path = directory.path().join(name);
        assert_eq!(
            export_graph_file(&state, path.clone(), CrawlGraphQuery::default(), kind)
                .await
                .unwrap()
                .row_count,
            0
        );
        let mut reader = csv::Reader::from_path(path).unwrap();
        assert_eq!(reader.headers().unwrap().len(), columns);
        assert_eq!(reader.records().count(), 0);
    }
    state.exit_confirmed.store(true, Ordering::SeqCst);
    for kind in [ExportFileKind::GraphNodesCsv, ExportFileKind::GraphEdgesCsv] {
        let error = export_graph_file(
            &state,
            directory.path().join("closing.csv"),
            CrawlGraphQuery::default(),
            kind,
        )
        .await
        .unwrap_err();
        assert!(error.contains("closing"), "{error}");
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

fn advanced_export_query(mode: &str) -> GridQuery {
    GridQuery {
        offset: 99,
        limit: 1,
        global_search: Some("retain".into()),
        segment_pattern: Some("/keep/".into()),
        sort_by: Some("depth".into()),
        sort_dir: ferrous_frog_storage::SortDirection::Desc,
        view: ferrous_frog_storage::IssueView::Status2xx,
        filters: Some(
            serde_json::from_value(serde_json::json!({
                "match":mode, "rules":[
                    {"field":"title", "operator":"contains", "value":"alpha"},
                    {"field":"depth", "operator":"greaterThan", "value":"2"}
                ]
            }))
            .unwrap(),
        ),
        ..GridQuery::default()
    }
}

#[tokio::test]
async fn advanced_filters_survive_csv_xlsx_and_sitemap_export_windows() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(ferrous_frog_storage::SqliteStore::in_memory().unwrap()),
    ] {
        for (path, depth, title, status, indexability) in [
            ("keep/a", 1, "Retain Alpha", 200, "Indexable"),
            ("keep/b", 3, "Retain Beta", 200, "Indexable"),
            ("keep/c", 4, "Discard", 200, "Indexable"),
            ("outside/d", 5, "Retain Alpha", 200, "Indexable"),
            ("keep/error", 5, "Retain Alpha", 404, "Indexable"),
            ("keep/f", 1, "Retain Beta", 200, "Indexable"),
            ("keep/g?x=1&y=2", 6, "Retain Alpha", 200, "Indexable"),
            ("keep/noindex", 8, "Retain Alpha", 200, "Non-Indexable"),
        ] {
            let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), depth);
            record.title = Some(title.into());
            record.status_code = Some(status);
            record.content_type = Some("text/html".into());
            record.indexability = indexability.into();
            store.upsert(record);
        }
        let state = state_with_store(store.clone());
        for (mode, expected_paths) in [
            ("all", vec!["keep/noindex", "keep/g?x=1&y=2"]),
            (
                "any",
                vec!["keep/noindex", "keep/g?x=1&y=2", "keep/b", "keep/a"],
            ),
        ] {
            let query = advanced_export_query(mode);
            let expected_urls = expected_paths
                .iter()
                .map(|path| format!("https://example.test/{path}"))
                .collect::<Vec<_>>();
            let directory = tempfile::tempdir().unwrap();
            let csv_path = directory.path().join("filtered.csv");
            let mut csv_file = fs::File::create(&csv_path).unwrap();
            assert_eq!(
                write_grid_csv_stream(&store, query.clone(), &mut csv_file).unwrap(),
                expected_urls.len()
            );
            let rows = csv::Reader::from_path(csv_path)
                .unwrap()
                .records()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(
                rows.iter()
                    .map(|row| row[1].to_string())
                    .collect::<Vec<_>>(),
                expected_urls
            );
            assert_eq!(
                export_xlsx_file(
                    &state,
                    directory.path().join("filtered.xlsx"),
                    query.clone()
                )
                .await
                .unwrap()
                .row_count,
                expected_urls.len()
            );

            let xml_path = directory.path().join("sitemap.xml");
            let mut xml_file = fs::File::create(&xml_path).unwrap();
            assert_eq!(
                write_sitemap_xml_stream(&store, query, &mut xml_file).unwrap(),
                expected_urls.len() - 1
            );
            let xml = fs::read_to_string(xml_path).unwrap();
            let exported_urls = xml
                .lines()
                .filter_map(|line| {
                    line.trim()
                        .strip_prefix("<loc>")
                        .and_then(|value| value.strip_suffix("</loc>"))
                })
                .collect::<Vec<_>>();
            let expected_xml_urls = expected_urls
                .iter()
                .skip(1)
                .map(|url| xml_escape(url))
                .collect::<Vec<_>>();
            assert_eq!(exported_urls, expected_xml_urls);
        }
    }
}

#[tokio::test]
async fn invalid_advanced_filters_fail_before_export_headers_or_destination_creation() {
    let mut query = advanced_export_query("all");
    query.filters.as_mut().unwrap().rules[0].field = ferrous_frog_storage::GridFilterField::Depth;
    for kind in [
        ExportFileKind::Csv,
        ExportFileKind::Xlsx,
        ExportFileKind::Sitemap,
    ] {
        assert!(
            validate_export_request(&ExportFileRequest {
                kind,
                query: Some(query.clone()),
                graph_query: None,
                record_ids: None
            })
            .is_err()
        );
    }
    for kind in [
        ExportFileKind::SelectedCsv,
        ExportFileKind::AuditWorkbook,
        ExportFileKind::QueuedUrlsCsv,
        ExportFileKind::ImageAltCsv,
        ExportFileKind::LinkEdgesCsv,
        ExportFileKind::RedirectChainsCsv,
        ExportFileKind::SitemapValidationCsv,
        ExportFileKind::HtmlReport,
        ExportFileKind::GraphJson,
        ExportFileKind::GraphNodesCsv,
        ExportFileKind::GraphEdgesCsv,
        ExportFileKind::CrawlArchive,
    ] {
        assert!(
            validate_export_request(&ExportFileRequest {
                kind,
                query: Some(query.clone()),
                graph_query: None,
                record_ids: None
            })
            .is_ok()
        );
    }
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(ferrous_frog_storage::SqliteStore::in_memory().unwrap()),
    ] {
        store.upsert(CrawlRecord::pending("https://example.test/".into(), 0));
        assert!(query_store_rows(&store, query.clone()).is_err());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("untouched.csv");
        let mut file = fs::File::create(&path).unwrap();
        assert!(write_grid_csv_stream(&store, query.clone(), &mut file).is_err());
        assert!(write_sitemap_xml_stream(&store, query.clone(), &mut file).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
        let error = export_xlsx_file(
            &state_with_store(store),
            directory.path().join("missing/invalid.xlsx"),
            query.clone(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("Filter rule"), "{error}");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn advanced_csv_and_sitemap_filters_remain_applied_after_the_first_storage_page() {
    let store = ActiveStore::memory();
    for index in 0..10_002 {
        let mut record = CrawlRecord::pending(format!("https://example.test/keep/{index:05}"), 3);
        record.title = Some(
            if index == 10_001 {
                "Retain Beta"
            } else {
                "Retain Alpha"
            }
            .into(),
        );
        record.status_code = Some(200);
        record.content_type = Some("text/html".into());
        record.indexability = "Indexable".into();
        store.upsert(record);
    }
    let directory = tempfile::tempdir().unwrap();
    let csv_path = directory.path().join("filtered.csv");
    let mut csv_file = fs::File::create(&csv_path).unwrap();
    assert_eq!(
        write_grid_csv_stream(&store, advanced_export_query("all"), &mut csv_file).unwrap(),
        10_001
    );
    let rows = csv::Reader::from_path(csv_path)
        .unwrap()
        .records()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 10_001);
    assert!(rows.iter().all(|row| row[24] == *"Retain Alpha"));
    assert_eq!(&rows[10_000][1], "https://example.test/keep/10000");
    let xml_path = directory.path().join("sitemap.xml");
    let mut xml_file = fs::File::create(&xml_path).unwrap();
    assert_eq!(
        write_sitemap_xml_stream(&store, advanced_export_query("all"), &mut xml_file).unwrap(),
        10_001
    );
    let xml = fs::read_to_string(xml_path).unwrap();
    assert_eq!(xml.matches("<loc>").count(), 10_001);
    assert!(xml.contains("/keep/10000</loc>"));
    assert!(!xml.contains("/keep/10001</loc>"));
}
