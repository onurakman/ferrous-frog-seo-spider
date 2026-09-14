use crate::*;

fn archive_integer(value: u64, field: &str) -> Result<i64, StorageError> {
    i64::try_from(value)
        .map_err(|_| StorageError::InvalidArchive(format!("{field} exceeds SQLite's range")))
}

impl SqliteStore {
    /// Restore one bounded archive batch into private staging, preserving captured identities
    /// and metadata. Unlike crawler writes, these inserts do not refresh target evidence.
    pub fn try_append_archive_link_edges(&self, edges: &[LinkEdge]) -> Result<(), StorageError> {
        let mut conn = self.connection()?;
        let transaction = conn.transaction()?;
        {
            let mut insert = transaction.prepare_cached(
                "INSERT INTO link_edges (id, source_url, target_url, anchor_text, rel,
                 rel_nofollow, link_type, source_status_code, target_status_code,
                 source_depth, target_depth, source_position, discovery_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )?;
            for edge in edges {
                insert.execute(params![
                    archive_integer(edge.id, "link ID")?,
                    edge.source_url,
                    edge.target_url,
                    edge.anchor_text,
                    edge.rel,
                    edge.rel_nofollow,
                    link_type_to_str(&edge.link_type),
                    edge.source_status_code,
                    edge.target_status_code,
                    archive_integer(edge.source_depth as u64, "link source depth")?,
                    edge.target_depth
                        .map(|depth| archive_integer(depth as u64, "link target depth"))
                        .transpose()?,
                    i64::from(edge.source_position),
                    archive_integer(edge.discovery_order, "link discovery order")?,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Append archived image occurrences without replacing siblings from the same page.
    /// Size and oversized flags remain derived from the restored crawl records at query time.
    pub fn try_append_archive_image_assets(
        &self,
        images: &[ImageAsset],
    ) -> Result<(), StorageError> {
        let mut conn = self.connection()?;
        let transaction = conn.transaction()?;
        {
            let mut insert = transaction.prepare_cached(
                "INSERT INTO image_assets (id, page_url, image_url, alt_text, alt_len,
                 missing_alt, alt_too_long, width, height, source_position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for image in images {
                insert.execute(params![
                    archive_integer(image.id, "image ID")?,
                    image.page_url,
                    image.image_url,
                    image.alt_text,
                    i64::from(image.alt_len),
                    image.missing_alt,
                    image.alt_too_long,
                    image.width.map(i64::from),
                    image.height.map(i64::from),
                    i64::from(image.source_position),
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Append archived reference occurrences without replacing siblings from the same source.
    pub fn try_append_archive_page_references(
        &self,
        references: &[PageReference],
    ) -> Result<(), StorageError> {
        let mut conn = self.connection()?;
        let transaction = conn.transaction()?;
        {
            let mut insert = transaction.prepare_cached(
                "INSERT INTO page_references
                 (id, source_storage_key, source_url, target_url, kind, rel_nofollow)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for reference in references {
                insert.execute(params![
                    archive_integer(reference.id, "reference ID")?,
                    reference.source_storage_key,
                    reference.source_url,
                    reference.target_url,
                    page_reference_kind_to_str(reference.kind),
                    reference.rel_nofollow,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Restore the next queue window at its original position in a private archive database.
    pub fn try_append_archive_frontier_queue(
        &self,
        start_position: usize,
        items: &[CrawlFrontierItem],
    ) -> Result<(), StorageError> {
        let mut conn = self.connection()?;
        let transaction = conn.transaction()?;
        {
            let mut insert = transaction.prepare_cached(
                "INSERT INTO crawl_frontier_queue
                 (position, url, depth, from_sitemap, storage_key, list_position, list_duplicate_index)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (offset, item) in items.iter().enumerate() {
                let position = start_position.checked_add(offset).ok_or_else(|| {
                    StorageError::InvalidArchive("frontier position overflow".into())
                })?;
                insert.execute(params![
                    archive_integer(position as u64, "frontier position")?,
                    item.url,
                    archive_integer(item.depth as u64, "frontier depth")?,
                    item.from_sitemap,
                    item.storage_key,
                    item.list_position.map(i64::from),
                    i64::from(item.list_duplicate_index),
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Restore a seen-key window with the same deduplication as a saved frontier snapshot.
    pub fn try_append_archive_seen(&self, seen: &[String]) -> Result<(), StorageError> {
        let mut conn = self.connection()?;
        let transaction = conn.transaction()?;
        {
            let mut insert = transaction
                .prepare_cached("INSERT OR IGNORE INTO crawl_frontier_seen (url) VALUES (?1)")?;
            for key in seen {
                insert.execute([key])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn try_set_archive_crawled(&self, crawled: usize) -> Result<(), StorageError> {
        self.connection()?.execute(
            "INSERT INTO crawl_frontier_meta (key, value) VALUES ('crawled', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [crawled.to_string()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(id: u64) -> LinkEdge {
        LinkEdge {
            id,
            source_url: "https://example.test/source".into(),
            target_url: "https://example.test/image.png".into(),
            anchor_text: format!("Anchor {id}"),
            rel: "nofollow".into(),
            rel_nofollow: true,
            link_type: LinkType::Internal,
            source_status_code: Some(202),
            target_status_code: Some(404),
            source_depth: 7,
            target_depth: Some(9),
            source_position: 4,
            discovery_order: 50 + id,
        }
    }

    fn image(id: u64) -> ImageAsset {
        ImageAsset {
            id,
            page_url: "https://example.test/source".into(),
            image_url: "https://example.test/image.png".into(),
            alt_text: Some(format!("Alt {id}")),
            alt_len: 5,
            missing_alt: false,
            alt_too_long: false,
            width: Some(100),
            height: Some(50),
            source_position: id as u32,
            size_bytes: None,
            oversized: false,
        }
    }

    fn reference(id: u64) -> PageReference {
        PageReference {
            id,
            source_storage_key: "list:2:https://example.test/source".into(),
            source_url: "https://example.test/source".into(),
            target_url: format!("https://example.test/target/{id}"),
            kind: PageReferenceKind::Pagination,
            rel_nofollow: true,
        }
    }

    #[test]
    fn archive_evidence_batches_preserve_identities_metadata_and_siblings() {
        let store = SqliteStore::in_memory().unwrap();
        // Existing record evidence must not rewrite archived edge status/depth.
        let mut target = CrawlRecord::pending("https://example.test/image.png".into(), 1);
        target.status_code = Some(200);
        target.content_type = Some("image/png".into());
        target.size_bytes = 300_000;
        target.inlink_count = 77;
        store.try_upsert(target).unwrap();
        for id in [8, 0, 3] {
            store.try_append_archive_link_edges(&[edge(id)]).unwrap();
            store.try_append_archive_image_assets(&[image(id)]).unwrap();
            store
                .try_append_archive_page_references(&[reference(id)])
                .unwrap();
        }
        let edges = store
            .try_link_edges(LinkEdgeQuery::default())
            .unwrap()
            .edges;
        assert_eq!(edges.len(), 3);
        for (actual, id) in edges.iter().zip([0, 3, 8]) {
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(edge(id)).unwrap()
            );
        }
        let images = store
            .try_image_assets(ImageAssetQuery::default())
            .unwrap()
            .images;
        assert_eq!(images.len(), 3);
        for actual in images {
            let mut expected = image(actual.id);
            expected.size_bytes = Some(300_000);
            expected.oversized = true;
            assert_eq!(actual, expected);
        }
        let references = store
            .try_page_references(PageReferenceQuery::default())
            .unwrap()
            .references;
        assert_eq!(references.len(), 3);
        for (actual, id) in references.iter().zip([0, 3, 8]) {
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(reference(id)).unwrap()
            );
        }
        assert_eq!(store.try_records().unwrap()[0].inlink_count, 77);
        // A failure late in any chunk must also undo the valid first insert.
        assert!(
            store
                .try_append_archive_link_edges(&[edge(20), edge(3)])
                .is_err()
        );
        assert!(
            store
                .try_append_archive_image_assets(&[image(20), image(3)])
                .is_err()
        );
        assert!(
            store
                .try_append_archive_page_references(&[reference(20), reference(3)])
                .is_err()
        );
        assert!(
            store
                .try_append_archive_link_edges(&[edge(21), edge(u64::MAX - 50)])
                .is_err()
        );
        assert!(
            store
                .try_append_archive_image_assets(&[image(21), image(u64::MAX)])
                .is_err()
        );
        assert!(
            store
                .try_append_archive_page_references(&[reference(21), reference(u64::MAX)])
                .is_err()
        );
        for table in ["link_edges", "image_assets", "page_references"] {
            assert_eq!(
                store
                    .connection()
                    .unwrap()
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                3
            );
        }
    }

    #[test]
    fn archive_frontier_batches_preserve_order_seen_dedup_and_rollback() {
        let store = SqliteStore::in_memory().unwrap();
        store.try_set_archive_crawled(0).unwrap();
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT value FROM crawl_frontier_meta WHERE key = 'crawled'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "0"
        );
        let item = |position| CrawlFrontierItem {
            url: "https://example.test/repeated".into(),
            depth: 4,
            from_sitemap: true,
            storage_key: format!("list:{position}:https://example.test/repeated"),
            list_position: Some(position),
            list_duplicate_index: position,
        };
        store
            .try_append_archive_frontier_queue(0, &[item(9)])
            .unwrap();
        store
            .try_append_archive_frontier_queue(1, &[item(2), item(5)])
            .unwrap();
        let seen = vec![
            item(9).storage_key,
            item(2).storage_key,
            item(5).storage_key,
        ];
        store.try_append_archive_seen(&seen).unwrap();
        store.try_append_archive_seen(&seen[..1]).unwrap();
        store.try_set_archive_crawled(17).unwrap();
        assert!(
            store
                .try_append_archive_frontier_queue(2, &[item(6)])
                .is_err()
        );
        // The first position fits; the second overflows SQLite or usize on a 32-bit host.
        let last_position = usize::try_from(i64::MAX).unwrap_or(usize::MAX);
        assert!(
            store
                .try_append_archive_frontier_queue(last_position, &[item(6), item(7)])
                .is_err()
        );
        let frontier = store.try_load_frontier_state().unwrap().unwrap();
        assert_eq!(frontier.queued, [item(9), item(2), item(5)]);
        assert_eq!(
            frontier.seen.into_iter().collect::<HashSet<_>>(),
            seen.into_iter().collect()
        );
        assert_eq!(frontier.crawled, 17);
        assert_eq!(store.try_frontier_summary().unwrap().unwrap().queued, 3);
    }
}
