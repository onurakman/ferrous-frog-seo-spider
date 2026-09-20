use super::*;

#[derive(Default)]
pub(super) struct CachedSummary {
    revision: i64,
    summary: CrawlSummary,
    // ponytail: retain narrow audit contributions, O(records + distinct metadata).
    // Move these indexes to SQLite if their measured memory cost becomes limiting.
    records: HashMap<i64, Contribution>,
    duplicates: [HashMap<String, usize>; 4],
    clusters: HashMap<i64, usize>,
}

struct Contribution {
    flags: u64,
    metadata: [String; 4],
    cluster: Option<i64>,
}

pub(super) fn initialize(conn: &Connection) -> Result<(), StorageError> {
    // TEMP triggers only run on this connection, require no functions on external
    // connections, and roll back with their record changes. Count local revisions
    // independently of the persistent trigger's execution order.
    conn.execute_batch(
        "CREATE TEMP TABLE ff_progress_dirty (id INTEGER PRIMARY KEY);
         CREATE TEMP TABLE ff_progress_revision (revision INTEGER NOT NULL);
         INSERT INTO ff_progress_revision SELECT revision FROM crawl_audit_revision;
         CREATE TEMP TRIGGER ff_progress_insert AFTER INSERT ON main.crawl_records BEGIN
             INSERT INTO ff_progress_dirty VALUES (NEW.id) ON CONFLICT DO NOTHING;
             UPDATE ff_progress_revision SET revision = revision + 1;
         END;
         CREATE TEMP TRIGGER ff_progress_update AFTER UPDATE ON main.crawl_records BEGIN
             INSERT INTO ff_progress_dirty VALUES (OLD.id) ON CONFLICT DO NOTHING;
             INSERT INTO ff_progress_dirty VALUES (NEW.id) ON CONFLICT DO NOTHING;
             UPDATE ff_progress_revision SET revision = revision + 1;
         END;
         CREATE TEMP TRIGGER ff_progress_delete AFTER DELETE ON main.crawl_records BEGIN
             INSERT INTO ff_progress_dirty VALUES (OLD.id) ON CONFLICT DO NOTHING;
             UPDATE ff_progress_revision SET revision = revision + 1;
         END;",
    )?;
    Ok(())
}

impl SqliteStore {
    pub(super) fn progress_summary_with_connection(
        &self,
        conn: &Connection,
    ) -> Result<CrawlSummary, StorageError> {
        let mut cache = self
            .summary_cache
            .lock()
            .map_err(|_| StorageError::LockPoisoned)?;
        let transaction = conn.unchecked_transaction()?;
        let revision = crawl_audit_revision(&transaction)?;
        if let Some(cached) = &*cache
            && cached.revision == revision
        {
            return Ok(cached.summary.clone());
        }
        let local_revision: i64 =
            transaction.query_row("SELECT revision FROM ff_progress_revision", [], |row| {
                row.get(0)
            })?;
        let rebuild = cache.is_none() || local_revision != revision;
        // Any read/commit error drops the partial cache; the next call rebuilds it.
        let mut next = if rebuild {
            cache.take();
            CachedSummary::default()
        } else {
            cache.take().expect("existing progress cache")
        };
        let mask = progress_filters()
            .iter()
            .enumerate()
            .map(|(bit, filter)| {
                let predicate = filter.strip_prefix(" WHERE ").unwrap_or("1");
                format!("(CAST(COALESCE(({predicate}), 0) AS INTEGER) << {bit})")
            })
            .collect::<Vec<_>>()
            .join(" | ");
        let restriction = if rebuild {
            ""
        } else {
            " WHERE id IN (SELECT id FROM ff_progress_dirty)"
        };
        let sql = format!(
            "SELECT id, {mask},
            CASE WHEN {SUCCESS_HTML_SQL} THEN ff_text_key(title) ELSE '' END,
            CASE WHEN {SUCCESS_HTML_SQL} THEN ff_text_key(meta_description) ELSE '' END,
            CASE WHEN {SUCCESS_HTML_SQL} THEN ff_text_key(h1) ELSE '' END,
            CASE WHEN {SUCCESS_HTML_SQL} THEN ff_text_key(h2) ELSE '' END,
            CASE WHEN {SUCCESS_HTML_SQL} THEN near_duplicate_cluster_id END
            FROM crawl_records{restriction}"
        );
        if !rebuild {
            let mut statement = transaction.prepare("SELECT id FROM ff_progress_dirty")?;
            for id in statement.query_map([], |row| row.get::<_, i64>(0))? {
                if let Some(previous) = next.records.remove(&id?) {
                    next.apply(&previous, false);
                }
            }
        }
        {
            let mut statement = transaction.prepare(&sql)?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let contribution = Contribution {
                    flags: row.get::<_, i64>(1)? as u64,
                    metadata: [row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?],
                    cluster: row.get(6)?,
                };
                next.apply(&contribution, true);
                next.records.insert(row.get(0)?, contribution);
            }
        }
        transaction.execute("DELETE FROM ff_progress_dirty", [])?;
        transaction.execute("UPDATE ff_progress_revision SET revision = ?1", [revision])?;
        transaction.commit()?;
        next.revision = revision;
        let summary = next.summary.clone();
        *cache = Some(next);
        Ok(summary)
    }
}

impl CachedSummary {
    fn apply(&mut self, contribution: &Contribution, insert: bool) {
        for (bit, count) in progress_counts(&mut self.summary).enumerate() {
            if contribution.flags & (1 << bit) != 0 {
                if insert {
                    *count += 1;
                } else {
                    *count -= 1;
                }
            }
        }
        for ((groups, key), count) in self.duplicates.iter_mut().zip(&contribution.metadata).zip([
            &mut self.summary.title_duplicate,
            &mut self.summary.meta_duplicate,
            &mut self.summary.h1_duplicate,
            &mut self.summary.h2_duplicate,
        ]) {
            if !key.is_empty() {
                update_group(groups, key, count, insert);
            }
        }
        if let Some(cluster) = contribution.cluster {
            update_group(
                &mut self.clusters,
                &cluster,
                &mut self.summary.near_duplicates,
                insert,
            );
        }
    }
}

fn update_group<K: Eq + std::hash::Hash + Clone>(
    groups: &mut HashMap<K, usize>,
    key: &K,
    total: &mut usize,
    insert: bool,
) {
    let before = groups.get(key).copied().unwrap_or(0);
    let after = if insert { before + 1 } else { before - 1 };
    *total -= if before > 1 { before } else { 0 };
    *total += if after > 1 { after } else { 0 };
    if after == 0 {
        groups.remove(key);
    } else {
        groups.insert(key.clone(), after);
    }
}
// Keep bit positions, decoded counters and grid predicates in one declaration.
macro_rules! progress_fields {
    ($($field:ident: $filter:expr),+ $(,)?) => {
        fn progress_counts(summary: &mut CrawlSummary) -> impl Iterator<Item = &mut usize> {
            [$(&mut summary.$field),+].into_iter()
        }
        fn progress_filters() -> Vec<String> {
            vec![$($filter),+]
        }
    };
}

fn progress_view_filter(view: IssueView) -> String {
    let (filter, args) = query_filter_sql(&GridQuery {
        view,
        ..GridQuery::default()
    });
    debug_assert!(args.is_empty());
    filter
}

progress_fields! {
        total: progress_view_filter(IssueView::All),
        internal: progress_view_filter(IssueView::Internal),
        external: progress_view_filter(IssueView::External),
        success: progress_view_filter(IssueView::Status2xx),
        redirects: progress_view_filter(IssueView::Status3xx),
        client_errors: progress_view_filter(IssueView::Status4xx),
        server_errors: progress_view_filter(IssueView::Status5xx),
        no_response: progress_view_filter(IssueView::NoResponse),
        broken: progress_view_filter(IssueView::BrokenLinks),
        indexable: " WHERE indexability = 'Indexable'".to_string(),
        non_indexable: " WHERE indexability = 'Non-indexable'".to_string(),
        title_missing: progress_view_filter(IssueView::TitleMissing),
        title_multiple: progress_view_filter(IssueView::TitleMultiple),
        meta_missing: progress_view_filter(IssueView::MetaMissing),
        meta_multiple: progress_view_filter(IssueView::MetaMultiple),
        h1_missing: progress_view_filter(IssueView::H1Missing),
        h2_missing: progress_view_filter(IssueView::H2Missing),
        canonical_missing: progress_view_filter(IssueView::CanonicalMissing),
        canonical_multiple: progress_view_filter(IssueView::CanonicalMultiple),
        pagination_multiple_targets: progress_view_filter(IssueView::PaginationMultipleTargets),
        amp_multiple_targets: progress_view_filter(IssueView::AmpMultipleTargets),
        noindex: progress_view_filter(IssueView::DirectivesNoindex),
        images_missing_alt: progress_view_filter(IssueView::ImagesMissingAlt),
        images_alt_too_long: progress_view_filter(IssueView::ImagesAltTooLong),
        mixed_content: progress_view_filter(IssueView::SecurityMixedContent),
        insecure_forms: progress_view_filter(IssueView::SecurityInsecureForms),
        hreflang_invalid: progress_view_filter(IssueView::HreflangInvalid),
        structured_data_invalid: progress_view_filter(IssueView::StructuredDataInvalid),
        structured_data_warnings: progress_view_filter(IssueView::StructuredDataWarning),
        deprecated_html_tags: progress_view_filter(IssueView::HtmlDeprecatedTags),
        missing_html_doctype: progress_view_filter(IssueView::HtmlMissingDoctype),
        duplicate_ids: progress_view_filter(IssueView::HtmlDuplicateIds),
        rendered_dom_changed: progress_view_filter(IssueView::RenderedDomChanged),
        missing_viewport: progress_view_filter(IssueView::MobileMissingViewport),
        missing_hsts: progress_view_filter(IssueView::SecurityMissingHsts),
        sitemap_orphans: progress_view_filter(IssueView::SitemapOrphan),
}
