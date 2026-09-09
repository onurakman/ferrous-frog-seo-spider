use crate::*;

impl ActiveStore {
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
