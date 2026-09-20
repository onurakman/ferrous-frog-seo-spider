use crate::*;

impl MemoryStore {
    /// Stream borrowed edges under one read lock; the visitor must not reenter this store.
    pub fn try_visit_link_edges(
        &self,
        visitor: &mut dyn FnMut(&LinkEdge) -> std::io::Result<()>,
    ) -> Result<usize, StorageError> {
        let inner = self.inner.read().map_err(|_| StorageError::LockPoisoned)?;
        for edge in &inner.link_edges {
            visitor(edge)?;
        }
        Ok(inner.link_edges.len())
    }
}

impl SqliteStore {
    /// Stream one SQLite statement snapshot without an edge Vec or repeated count queries.
    pub fn try_visit_link_edges(
        &self,
        visitor: &mut dyn FnMut(&LinkEdge) -> std::io::Result<()>,
    ) -> Result<usize, StorageError> {
        let conn = self.connection()?;
        let mut statement = conn.prepare("SELECT * FROM link_edges ORDER BY id ASC")?;
        let mut count = 0;
        for edge in statement.query_map([], link_edge_from_row)? {
            visitor(&edge?)?;
            count += 1;
        }
        Ok(count)
    }
}

impl ActiveStore {
    /// Visit records in the same order as `records`, including derived first-inlink fields.
    /// SQLite decodes at most 256 records at once under one read transaction.
    /// Memory clones one record at a time, retaining an index of first-inlink sources.
    /// The visitor runs under the storage lock and must not reenter this store.
    pub fn try_visit_records(
        &self,
        mut visitor: impl FnMut(CrawlRecord) -> std::io::Result<()>,
    ) -> Result<usize, StorageError> {
        let mut count = 0;
        match self {
            Self::Memory(store) => {
                let inner = store.inner.read().map_err(|_| StorageError::LockPoisoned)?;
                for record in &inner.records {
                    let mut record = record.clone();
                    annotate_first_inlink_sources(
                        std::slice::from_mut(&mut record),
                        &inner.first_inlink_sources,
                    );
                    visitor(record)?;
                    count += 1;
                }
            }
            Self::Sqlite(store) => {
                let conn = store.connection()?;
                let tx = conn.unchecked_transaction()?;
                // ponytail: SQLite still sorts the full result; add an order index if profiling
                // shows this dominates. Rust record hydration is bounded independently.
                let mut statement = tx.prepare(
                    "SELECT * FROM crawl_records ORDER BY COALESCE(list_position, id) ASC, id ASC",
                )?;
                let mut rows = statement.query_map([], record_from_row)?;
                loop {
                    let mut records = rows.by_ref().take(256).collect::<Result<Vec<_>, _>>()?;
                    if records.is_empty() {
                        break;
                    }
                    annotate_sqlite_first_inlink_sources(&tx, &mut records)?;
                    for record in records {
                        visitor(record)?;
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }

    /// Write the schema-1 frontier JSON without cloning the queue or seen set.
    /// SQLite holds one read transaction across all three fields; Memory borrows its state.
    /// The writer runs under the storage lock and must not reenter this store.
    pub fn try_write_frontier_state_json(
        &self,
        writer: &mut impl std::io::Write,
    ) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => {
                let inner = store.inner.read().map_err(|_| StorageError::LockPoisoned)?;
                serde_json::to_writer(writer, &inner.frontier_state)?;
            }
            Self::Sqlite(store) => {
                let conn = store.connection()?;
                let tx = conn.unchecked_transaction()?;
                let populated: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM crawl_frontier_queue)
                         OR EXISTS(SELECT 1 FROM crawl_frontier_seen)",
                    [],
                    |row| row.get(0),
                )?;
                if !populated {
                    writer.write_all(b"null")?;
                    return Ok(());
                }
                writer.write_all(b"{\"queued\":[")?;
                let mut statement =
                    tx.prepare("SELECT * FROM crawl_frontier_queue ORDER BY position ASC")?;
                for (index, item) in statement.query_map([], frontier_item_from_row)?.enumerate() {
                    if index != 0 {
                        writer.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut *writer, &item?)?;
                }
                writer.write_all(b"],\"seen\":[")?;
                let mut statement =
                    tx.prepare("SELECT url FROM crawl_frontier_seen ORDER BY url ASC")?;
                for (index, url) in statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .enumerate()
                {
                    if index != 0 {
                        writer.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut *writer, &url?)?;
                }
                let crawled = tx
                    .query_row(
                        "SELECT value FROM crawl_frontier_meta WHERE key = 'crawled'",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                write!(writer, "],\"crawled\":{crawled}}}")?;
            }
        }
        Ok(())
    }

    /// Visit a stable edge snapshot without cloning the full collection.
    /// The visitor runs under the storage lock and must not reenter this store.
    pub fn try_visit_link_edges(
        &self,
        visitor: &mut dyn FnMut(&LinkEdge) -> std::io::Result<()>,
    ) -> Result<usize, StorageError> {
        match self {
            Self::Memory(store) => store.try_visit_link_edges(visitor),
            Self::Sqlite(store) => store.try_visit_link_edges(visitor),
        }
    }

    /// Read only the requested records, preserving ID order and omitting unknown IDs.
    pub fn try_records_by_ids(&self, ids: &[u64]) -> Result<Vec<CrawlRecord>, StorageError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let records = match self {
            Self::Memory(store) => {
                let inner = store.inner.read().map_err(|_| StorageError::LockPoisoned)?;
                let selected = ids.iter().copied().collect::<HashSet<_>>();
                let mut records = inner
                    .records
                    .iter()
                    .filter(|record| selected.contains(&record.id))
                    .cloned()
                    .collect::<Vec<_>>();
                annotate_first_inlink_sources(&mut records, &inner.first_inlink_sources);
                records
            }
            Self::Sqlite(store) => {
                let conn = store.connection()?;
                let mut records = Vec::new();
                for ids in ids.chunks(500) {
                    let placeholders = std::iter::repeat_n("?", ids.len())
                        .collect::<Vec<_>>()
                        .join(",");
                    let args = ids.iter().map(u64::to_string).collect::<Vec<_>>();
                    records.extend(query_records_with_args(
                        &conn,
                        &format!("SELECT * FROM crawl_records WHERE id IN ({placeholders})"),
                        &args,
                    )?);
                }
                records
            }
        };
        let mut records = records
            .into_iter()
            .map(|record| (record.id, record))
            .collect::<HashMap<_, _>>();
        Ok(ids.iter().filter_map(|id| records.remove(id)).collect())
    }

    /// Visit one stable queued/in-flight snapshot without loading records or the seen set.
    /// The visitor runs under the storage lock and must not reenter this store.
    pub fn try_visit_frontier(
        &self,
        mut visitor: impl FnMut(&CrawlFrontierItem) -> std::io::Result<()>,
    ) -> Result<usize, StorageError> {
        let mut count = 0;
        match self {
            Self::Memory(store) => {
                let inner = store.inner.read().map_err(|_| StorageError::LockPoisoned)?;
                if let Some(frontier) = &inner.frontier_state {
                    for item in &frontier.queued {
                        visitor(item)?;
                        count += 1;
                    }
                }
            }
            Self::Sqlite(store) => {
                let conn = store.connection()?;
                let mut statement =
                    conn.prepare("SELECT * FROM crawl_frontier_queue ORDER BY position ASC")?;
                for item in statement.query_map([], frontier_item_from_row)? {
                    visitor(&item?)?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn stores() -> [ActiveStore; 2] {
        [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ]
    }

    fn populate_record_stream(store: &ActiveStore) {
        for index in 0..513 {
            let mut record = CrawlRecord::pending("https://example.test/repeated".into(), 2);
            record.storage_key = format!("list:{index}");
            record.list_position = Some(513 - index);
            record.list_duplicate_index = index + 1;
            record.title = Some(format!("Original 🐸 {index}"));
            record.final_url = "https://example.test/final".into();
            store.upsert(record);
        }
        store.add_link_edge(LinkEdge {
            id: 0,
            source_url: "https://example.test/source".into(),
            target_url: "https://example.test/final".into(),
            anchor_text: "Original anchor 🐸".into(),
            rel: String::new(),
            rel_nofollow: false,
            link_type: LinkType::Internal,
            source_status_code: Some(200),
            target_status_code: None,
            source_depth: 1,
            target_depth: Some(2),
            source_position: 7,
            discovery_order: 1,
        });
    }

    #[test]
    fn generic_record_visitor_preserves_payloads_and_propagates_late_sqlite_errors() {
        fn collect<S: CrawlStore>(store: &S) -> Vec<CrawlRecord> {
            let mut records = Vec::new();
            store
                .try_visit_records(&mut |record| {
                    records.push(record);
                    Ok(())
                })
                .unwrap();
            records
        }
        let memory = MemoryStore::new();
        let sqlite = SqliteStore::in_memory().unwrap();
        for index in 0..257 {
            let record = CrawlRecord::pending(format!("https://example.test/{index}"), 0);
            memory.upsert(record.clone());
            sqlite.try_upsert(record).unwrap();
        }
        assert_eq!(
            serde_json::to_value(collect(&memory)).unwrap(),
            serde_json::to_value(collect(&sqlite)).unwrap()
        );
        sqlite
            .connection()
            .unwrap()
            .execute(
                "UPDATE crawl_records SET title_len = 'invalid' WHERE id = 257",
                [],
            )
            .unwrap();
        let mut count = 0;
        assert!(
            CrawlStore::try_visit_records(&sqlite, &mut |_| {
                count += 1;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(
            count, 256,
            "generic SQLite visitor must decode incrementally"
        );
        sqlite.clear();
        assert!(collect(&sqlite).is_empty());
    }

    #[test]
    fn record_visit_preserves_order_payloads_inlinks_and_stops_on_error() {
        for store in stores() {
            assert_eq!(
                store.try_visit_records(|_| panic!("empty store")).unwrap(),
                0
            );
            populate_record_stream(&store);
            let expected = store.records();
            let mut visited = Vec::new();
            assert_eq!(
                store
                    .try_visit_records(|record| {
                        assert_eq!(
                            record.first_inlink_anchor_text.as_deref(),
                            Some("Original anchor 🐸")
                        );
                        assert_eq!(record.first_inlink_source_position, Some(7));
                        visited.push(record);
                        Ok(())
                    })
                    .unwrap(),
                513
            );
            assert_eq!(
                serde_json::to_value(visited).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
            let mut calls = 0;
            let error = store
                .try_visit_records(|_| {
                    calls += 1;
                    Err(std::io::Error::other("record writer failed"))
                })
                .unwrap_err();
            assert_eq!(calls, 1);
            assert!(error.to_string().contains("record writer failed"));
            store.clear();
            assert_eq!(
                store
                    .try_visit_records(|_| panic!("cleared store"))
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn record_visit_keeps_records_and_inlinks_in_one_sqlite_snapshot() {
        let path = std::env::temp_dir().join(format!(
            "ferrous-record-stream-snapshot-{}.sqlite3",
            std::process::id()
        ));
        let store = ActiveStore::sqlite(&path).unwrap();
        populate_record_stream(&store);
        let expected = serde_json::to_value(store.records()).unwrap();
        let external = Connection::open(&path).unwrap();
        let mut visited = Vec::new();
        store
            .try_visit_records(|record| {
                if visited.is_empty() {
                    external
                        .execute_batch(
                            "BEGIN; UPDATE crawl_records SET title = 'Changed';
                     UPDATE link_edges SET anchor_text = 'Changed'; COMMIT;",
                        )
                        .unwrap();
                }
                visited.push(record);
                Ok(())
            })
            .unwrap();
        assert_eq!(serde_json::to_value(visited).unwrap(), expected);
        let changed = store.records();
        assert!(
            changed
                .iter()
                .all(|record| record.title.as_deref() == Some("Changed")
                    && record.first_inlink_anchor_text.as_deref() == Some("Changed"))
        );
        drop(external);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn frontier_json_preserves_empty_states_list_occurrences_and_unicode() {
        let queued = [9, 2].map(|position| CrawlFrontierItem {
            url: "https://example.test/🐸?q=\"quoted\"".into(),
            depth: 3,
            from_sitemap: position == 2,
            storage_key: format!("list:{position}:same-url"),
            list_position: Some(position),
            list_duplicate_index: position,
        });
        for store in stores() {
            for state in [
                None,
                Some(CrawlFrontierState {
                    crawled: 19,
                    ..Default::default()
                }),
                Some(CrawlFrontierState {
                    queued: queued.to_vec(),
                    seen: vec![],
                    crawled: 41,
                }),
                Some(CrawlFrontierState {
                    queued: vec![],
                    seen: vec!["z\\\"\n🐸".into(), "a".into(), "a".into()],
                    crawled: 42,
                }),
            ] {
                store.clear_frontier_state();
                if let Some(state) = state.clone() {
                    store.save_frontier_state(state);
                }
                let mut expected = state;
                if matches!(store, ActiveStore::Sqlite(_))
                    && let Some(frontier) = &mut expected
                {
                    frontier.seen.sort();
                    frontier.seen.dedup();
                    if frontier.queued.is_empty() && frontier.seen.is_empty() {
                        expected = None;
                    }
                }
                let mut bytes = Vec::new();
                store.try_write_frontier_state_json(&mut bytes).unwrap();
                assert_eq!(
                    serde_json::from_slice::<Option<CrawlFrontierState>>(&bytes).unwrap(),
                    expected
                );
            }
        }
    }

    #[test]
    fn frontier_json_keeps_one_sqlite_snapshot_and_releases_failed_writes() {
        use std::io::Write;

        struct UpdatingWriter<'a> {
            connection: &'a Connection,
            bytes: Vec<u8>,
            fail: bool,
        }
        impl Write for UpdatingWriter<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.bytes.is_empty() {
                    self.connection
                        .execute_batch(
                            "BEGIN; DELETE FROM crawl_frontier_queue;
                         DELETE FROM crawl_frontier_seen;
                         INSERT INTO crawl_frontier_seen(url) VALUES ('changed');
                         UPDATE crawl_frontier_meta SET value = '999'; COMMIT;",
                        )
                        .unwrap();
                }
                self.bytes.extend_from_slice(bytes);
                if self.fail {
                    return Err(std::io::Error::other("frontier disk full"));
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ferrous-frontier-json-snapshot-{}.sqlite3",
            std::process::id()
        ));
        let store = ActiveStore::sqlite(&path).unwrap();
        let connection = Connection::open(&path).unwrap();
        let expected = CrawlFrontierState {
            queued: vec![CrawlFrontierItem {
                url: "https://example.test/first".into(),
                depth: 2,
                from_sitemap: true,
                storage_key: "first".into(),
                list_position: Some(7),
                list_duplicate_index: 3,
            }],
            seen: vec!["original".into()],
            crawled: 42,
        };
        for fail in [false, true] {
            store.save_frontier_state(expected.clone());
            let mut writer = UpdatingWriter {
                connection: &connection,
                bytes: vec![],
                fail,
            };
            let result = store.try_write_frontier_state_json(&mut writer);
            if fail {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("frontier disk full")
                );
            } else {
                result.unwrap();
                assert_eq!(
                    serde_json::from_slice::<CrawlFrontierState>(&writer.bytes).unwrap(),
                    expected
                );
            }
            let current = store.load_frontier_state().unwrap();
            assert!(current.queued.is_empty());
            assert_eq!(current.seen, ["changed"]);
            assert_eq!(current.crawled, 999);
            store.clear_frontier_state();
            let mut bytes = Vec::new();
            store.try_write_frontier_state_json(&mut bytes).unwrap();
            assert_eq!(bytes, b"null");
        }
        drop(connection);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    #[ignore = "isolated frontier JSON export memory and timing workload"]
    fn frontier_json_export_workload() {
        use std::hash::{Hash, Hasher};
        use std::io::{Read, Write};

        let count: usize = std::env::var("FF_FRONTIER_EXPORT_SEEN")
            .unwrap_or_else(|_| "1000000".into())
            .parse()
            .unwrap();
        assert!(count >= 20);
        let hydrated =
            std::env::var("FF_FRONTIER_EXPORT_MODE").is_ok_and(|mode| mode == "hydrated");
        let directory = std::env::temp_dir().join(format!(
            "ferrous-frontier-json-workload-{}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let store = ActiveStore::sqlite(directory.join("frontier.sqlite3")).unwrap();
        let ActiveStore::Sqlite(sqlite) = &store else {
            unreachable!()
        };
        sqlite.connection().unwrap().execute_batch(&format!(
            "BEGIN;
             WITH RECURSIVE sequence(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM sequence WHERE n+1 < {count})
             INSERT INTO crawl_frontier_seen(url) SELECT printf('https://example.test/page/%09d', n) FROM sequence;
             WITH RECURSIVE sequence(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM sequence WHERE n+1 < {queued})
             INSERT INTO crawl_frontier_queue(position,url,depth,from_sitemap,storage_key,list_position,list_duplicate_index)
             SELECT n, 'https://example.test/repeated', n%8, n%2, printf('list:%09d',n), n+1, n+1 FROM sequence;
             INSERT INTO crawl_frontier_meta(key,value) VALUES ('crawled','12345'); COMMIT;",
            queued = count / 20,
        )).unwrap();
        let path = directory.join("frontier.json");
        let mut elapsed = Vec::new();
        for _ in 0..3 {
            let mut writer = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
            let started = std::time::Instant::now();
            if hydrated {
                serde_json::to_writer(&mut writer, &sqlite.try_load_frontier_state().unwrap())
                    .unwrap();
            } else {
                store.try_write_frontier_state_json(&mut writer).unwrap();
            }
            writer.flush().unwrap();
            elapsed.push(started.elapsed());
        }
        elapsed.sort();
        // Compare identical byte counts/digests across separate baseline/streaming processes
        // without hydrating JSON during RSS verification.
        let mut reader = std::fs::File::open(&path).unwrap();
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let mut buffer = [0; 65536];
        loop {
            let count = reader.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            buffer[..count].hash(&mut hash);
        }
        assert_eq!(
            store.try_frontier_summary().unwrap(),
            Some(CrawlFrontierSummary {
                queued: count / 20,
                seen: count,
                crawled: 12345,
            })
        );
        eprintln!(
            "frontier_export mode={} queued={} seen={count} samples=3 median_ms={:.3} bytes={} digest={:016x}",
            if hydrated { "hydrated" } else { "streamed" },
            count / 20,
            elapsed[1].as_secs_f64() * 1000.0,
            reader.metadata().unwrap().len(),
            hash.finish()
        );
        drop(reader);
        drop(store);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn selected_records_use_exact_ids_and_caller_order_in_both_stores() {
        for store in stores() {
            let first = store.upsert(CrawlRecord::pending("https://example.test/same".into(), 0));
            let mut duplicate = first.clone();
            duplicate.storage_key = "list:2:https://example.test/same".into();
            let duplicate = store.upsert(duplicate);
            let third = store.upsert(CrawlRecord::pending("https://example.test/third".into(), 1));
            let selected = store
                .try_records_by_ids(&[third.id, 999, first.id])
                .unwrap();
            assert_eq!(
                selected.iter().map(|record| record.id).collect::<Vec<_>>(),
                [third.id, first.id]
            );
            assert!(selected.iter().all(|record| record.id != duplicate.id));
            assert!(store.try_records_by_ids(&[]).unwrap().is_empty());
        }
    }

    #[test]
    fn link_edge_visit_preserves_exact_order_values_and_stops_on_error() {
        for store in stores() {
            let edges = (0..57)
                .map(|index| {
                    store.add_link_edge(LinkEdge {
                        id: 0,
                        source_url: format!("https://example.test/source/{index}"),
                        target_url: "https://example.test/target?x=🐸&y=1".into(),
                        anchor_text: format!("<script>é {index}</script>"),
                        rel: "ugc".into(),
                        rel_nofollow: false,
                        link_type: LinkType::Internal,
                        source_status_code: Some(200),
                        target_status_code: Some(404),
                        source_depth: 1,
                        target_depth: Some(2),
                        source_position: index,
                        discovery_order: u64::from(index),
                    })
                })
                .collect::<Vec<_>>();
            // Identical values alone would not detect cloning the entire edge collection.
            let original_urls = match &store {
                ActiveStore::Memory(memory) => Some(
                    memory
                        .inner
                        .read()
                        .unwrap()
                        .link_edges
                        .iter()
                        .map(|edge| edge.source_url.as_ptr() as usize)
                        .collect::<Vec<_>>(),
                ),
                ActiveStore::Sqlite(_) => None,
            };
            let mut visited = Vec::new();
            assert_eq!(
                store
                    .try_visit_link_edges(&mut |edge| {
                        if let Some(urls) = &original_urls {
                            assert_eq!(
                                edge.source_url.as_ptr() as usize,
                                urls[visited.len()],
                                "Memory visitor must borrow the retained edge payload"
                            );
                        }
                        visited.push(edge.clone());
                        Ok(())
                    })
                    .unwrap(),
                edges.len()
            );
            assert_eq!(visited, edges);
            let mut calls = 0;
            let error = store
                .try_visit_link_edges(&mut |_| {
                    calls += 1;
                    Err(std::io::Error::other("cancelled"))
                })
                .unwrap_err();
            assert_eq!(calls, 1);
            assert!(error.to_string().contains("cancelled"));
            store.clear();
            assert_eq!(
                store
                    .try_visit_link_edges(&mut |_| panic!("empty edges"))
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn sqlite_link_edge_visit_reports_query_and_late_decode_errors() {
        let store = SqliteStore::in_memory().unwrap();
        let conn = store.connection().unwrap();
        conn.execute_batch("INSERT INTO link_edges (id, source_url, target_url, anchor_text,
            rel, rel_nofollow, link_type, source_depth, source_position, discovery_order)
            VALUES (1, 'https://example.test/a', 'https://example.test/b', '', '', 0, 'internal', 0, 1, 1),
                   (2, 'https://example.test/c', 'https://example.test/b', '', '', 0, 'internal', 'invalid-depth', 2, 2)").unwrap();
        drop(conn);
        let mut calls = 0;
        assert!(
            store
                .try_visit_link_edges(&mut |_| {
                    calls += 1;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(calls, 1, "late decode failure must stop the stream");
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE link_edges")
            .unwrap();
        assert!(
            store
                .try_visit_link_edges(&mut |_| panic!("query failed"))
                .is_err()
        );
    }

    #[test]
    fn frontier_visit_keeps_queue_order_and_does_not_export_fetched_or_seen_urls() {
        for store in stores() {
            let mut fetched = CrawlRecord::pending("https://example.test/fetched".into(), 0);
            fetched.status_code = Some(200);
            store.upsert(fetched);
            let queued = ["second", "first"].map(|path| CrawlFrontierItem {
                url: format!("https://example.test/{path}"),
                depth: 1,
                from_sitemap: true,
                storage_key: format!("https://example.test/{path}"),
                list_position: None,
                list_duplicate_index: 0,
            });
            store.save_frontier_state(CrawlFrontierState {
                queued: queued.to_vec(),
                seen: vec!["https://example.test/seen-only".into()],
                crawled: 1,
            });
            let mut visited = Vec::new();
            assert_eq!(
                store
                    .try_visit_frontier(|item| {
                        visited.push(item.clone());
                        Ok(())
                    })
                    .unwrap(),
                2
            );
            assert_eq!(visited, queued);
            let error = store
                .try_visit_frontier(|_| Err(std::io::Error::other("disk full")))
                .unwrap_err();
            assert!(error.to_string().contains("disk full"));
            store.clear_frontier_state();
            assert_eq!(
                store
                    .try_visit_frontier(|_| panic!("empty queue has no rows"))
                    .unwrap(),
                0
            );
        }
    }
}
