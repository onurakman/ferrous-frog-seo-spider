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
                apply_first_inlink_sources(&mut records, &inner.link_edges);
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
