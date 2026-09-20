use super::*;
use ferrous_frog_storage::is_no_response_record;
use std::hash::Hasher;

/// Stream a legacy summary report with the same counts and sample order as the slice API.
/// Keep the crawl idle across both record passes and the edge pass. Each built-in record
/// pass is consistent; count/content fingerprint changes between record passes reject the
/// report before output. Record and edge passes are not one cross-collection snapshot.
/// Duplicate metadata and failed-URL indexes still grow with distinct observed values.
pub fn store_to_html_report_writer<S: CrawlStore, W: Write>(
    store: &S,
    thresholds: &AuditThresholds,
    writer: W,
) -> Result<usize, String> {
    let mut titles = HashMap::new();
    let mut descriptions = HashMap::new();
    let mut headings = HashMap::new();
    let mut failed_urls = HashSet::new();
    let mut summary = CrawlSummary::default();
    let mut before = RecordFingerprint::default();
    let total = store
        .try_visit_records(&mut |record| {
            serde_json::to_writer(&mut before, &record)?;
            // Only the summary counters displayed by this report need aggregation.
            summary.total += 1;
            summary.non_indexable += usize::from(record.indexability == "Non-indexable");
            summary.no_response += usize::from(is_no_response_record(&record));
            summary.broken += usize::from(is_broken_record(&record));
            summary.redirects += usize::from(
                !record.redirect_chain.is_empty() || matches!(record.status_code, Some(300..=399)),
            );
            match record.status_code {
                Some(200..=299) => summary.success += 1,
                Some(400..=499) => summary.client_errors += 1,
                Some(500..) => summary.server_errors += 1,
                _ => {}
            }
            if is_success_html_record(&record) {
                for (value, counts) in [
                    (record.title.as_deref(), &mut titles),
                    (record.meta_description.as_deref(), &mut descriptions),
                    (record.h1.as_deref(), &mut headings),
                ] {
                    if let Some(value) = value {
                        let key = normalize_text_key(value);
                        if !key.is_empty() {
                            *counts.entry(key).or_insert(0) += 1;
                        }
                    }
                }
            }
            // Preserve original/final URLs, List keys and intermediate redirect evidence.
            failed_urls.extend(
                failed_report_urls(std::slice::from_ref(&record))
                    .into_iter()
                    .map(str::to_owned),
            );
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    let mut sections = [
        broken_url_section(&[]),
        BrokenLinkReport::default().section(),
        metadata_section(&[], thresholds),
        headings_and_canonicals_section(&[], thresholds),
        image_section(&[], thresholds),
        performance_section(&[]),
        security_section(&[]),
        structured_data_section(&[]),
        html_validation_section(&[]),
        rendering_section(&[]),
    ];
    let mut add_batch = |records: &[CrawlRecord]| {
        let html = records
            .iter()
            .filter(|record| is_success_html_record(record));
        let parts = [
            broken_url_section(records),
            BrokenLinkReport::default().section(),
            metadata_section_with_counts(html.clone(), thresholds, &titles, &descriptions),
            headings_section_with_counts(html, thresholds, &headings),
            image_section(records, thresholds),
            performance_section(records),
            security_section(records),
            structured_data_section(records),
            html_validation_section(records),
            rendering_section(records),
        ];
        for (section, part) in sections.iter_mut().zip(parts) {
            section.count += part.count;
            section.rows.extend(
                part.rows
                    .into_iter()
                    .take(HTML_REPORT_ROW_LIMIT - section.rows.len()),
            );
        }
    };
    let mut batch = Vec::with_capacity(256);
    let mut after = RecordFingerprint::default();
    let repeated_total = store
        .try_visit_records(&mut |record| {
            serde_json::to_writer(&mut after, &record)?;
            batch.push(record);
            if batch.len() == 256 {
                add_batch(&batch);
                batch.clear();
            }
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    add_batch(&batch);
    if total != repeated_total || before.0.finish() != after.0.finish() {
        return Err("crawl changed during HTML report export; retry when the crawl is idle".into());
    }
    for section in &mut sections {
        section.has_rows = !section.rows.is_empty();
        section.count_label = if section.count == 1 {
            "issue"
        } else {
            "issues"
        }
        .into();
    }
    let mut links = BrokenLinkReport::default();
    store
        .try_visit_link_edges(&mut |edge| {
            links.observe(edge, &failed_urls);
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    sections[1] = links.section();
    render_html_report_writer(report_from_sections(summary, sections), writer)
        .map_err(|error| error.to_string())?;
    Ok(total)
}

// This is an in-process change detector, not a persisted or security-sensitive digest.
#[derive(Default)]
struct RecordFingerprint(std::collections::hash_map::DefaultHasher);

impl Write for RecordFingerprint {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{ActiveStore, LinkEdgeQuery, LinkType, SqliteStore};

    #[test]
    fn streamed_html_matches_all_sections_across_batches_and_preserves_samples() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            for count in [0, 513] {
                for index in 0..count {
                    let mut record = CrawlRecord::pending(
                        format!("https://example.test/page/{index}?x=<🐸>"),
                        0,
                    );
                    record.list_position = Some((count - index) as u32);
                    record.status_code = Some(200);
                    record.content_type = Some("text/html".into());
                    record.title = Some(format!("Unique title {index}"));
                    record.meta_description = Some(format!("Unique description {index}"));
                    record.h1 = Some(format!("Unique heading {index}"));
                    if index == 1 || index == count - 1 {
                        // The only duplicates are separated by more than a decode batch.
                        record.title = Some("Shared title".into());
                        record.meta_description = Some("Shared description".into());
                        record.h1 = Some("Shared heading".into());
                    }
                    record.images_missing_alt = 1;
                    record.response_time_ms = 3500;
                    record.mixed_content_count = 1;
                    record.structured_data_error_count = 1;
                    record.deprecated_html_tag_count = 1;
                    record.rendered_dom_changed = true;
                    if index == 0 {
                        record.status_code = Some(404);
                    }
                    store.upsert(record);
                }
                if count > 0 {
                    store.add_link_edge(LinkEdge {
                        id: 0,
                        source_url: "https://example.test/source".into(),
                        target_url: "https://example.test/page/0?x=<🐸>".into(),
                        anchor_text: "<script>🐸</script>".into(),
                        rel: String::new(),
                        rel_nofollow: false,
                        link_type: LinkType::Internal,
                        source_status_code: Some(200),
                        target_status_code: Some(404),
                        source_depth: 0,
                        target_depth: Some(0),
                        source_position: 1,
                        discovery_order: 1,
                    });
                }
                let records = store.records();
                let edges = store.link_edges(LinkEdgeQuery::default()).edges;
                let thresholds = AuditThresholds::default();
                let expected = records_to_html_report(&records, &edges, &thresholds).unwrap();
                let mut output = Vec::new();
                assert_eq!(
                    store_to_html_report_writer(&store, &thresholds, &mut output).unwrap(),
                    count
                );
                assert_eq!(String::from_utf8(output).unwrap(), expected);
                if count > 0 {
                    let report = build_html_report(&records, &edges, &thresholds);
                    assert!(report.sections.iter().all(|section| section.count > 0));
                    assert_eq!(report.sections[2].rows.len(), HTML_REPORT_ROW_LIMIT);
                    assert!(report.sections[2].count > HTML_REPORT_ROW_LIMIT);
                    let mut short = [0; 10];
                    assert!(
                        store_to_html_report_writer(&store, &thresholds, short.as_mut_slice())
                            .is_err()
                    );
                }
            }
        }
    }
}
