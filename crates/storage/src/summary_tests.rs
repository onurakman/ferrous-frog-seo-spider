use super::*;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

fn page(index: usize) -> CrawlRecord {
    let mut row = CrawlRecord::pending(format!("https://example.test/page/{index}"), 1);
    row.status_code = Some(200);
    row.status_text = "OK".into();
    row.content_type = Some("text/html; charset=utf-8".into());
    row.indexability = "Indexable".into();
    row.indexability_status = "Indexable".into();
    row.title = Some("Shared ÄBC title".into());
    row.meta_description = Some("Shared description".into());
    row.h1 = Some("Shared heading".into());
    row.h2 = Some("Shared subheading".into());
    row.title_count = Some(1);
    row.meta_description_count = Some(1);
    row.h1_count = 1;
    row.h2_count = 1;
    row.canonical = Some(row.final_url.clone());
    row.canonical_count = 1;
    row
}

fn assert_summary_eq(actual: CrawlSummary, expected: CrawlSummary) {
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

fn count_text_work(store: &SqliteStore) -> Arc<AtomicUsize> {
    let work = Arc::new(AtomicUsize::new(0));
    let calls = work.clone();
    store
        .connection()
        .unwrap()
        .create_scalar_function(
            "ff_text_key",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            move |context| {
                calls.fetch_add(1, AtomicOrdering::Relaxed);
                Ok(normalize_text_key(
                    context
                        .get::<Option<String>>(0)?
                        .as_deref()
                        .unwrap_or_default(),
                ))
            },
        )
        .unwrap();
    work
}

#[test]
fn sqlite_progress_summary_matches_memory_for_empty_mixed_unicode_and_list_records() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    assert_summary_eq(sqlite.progress_summary(), memory.progress_summary());
    for index in 0..22 {
        let mut row = page(index);
        row.near_duplicate_cluster_id = Some(1);
        row.in_sitemap = true;
        match index {
            0 => {
                row.title = Some("  shared\u{a0}äbc\t title  ".into());
                row.meta_description = Some(" SHARED\n description ".into());
                row.h1 = Some("SHARED\tHEADING".into());
                row.h2 = Some(" shared\u{2003}subheading ".into());
                row.title_count = Some(2);
                row.meta_description_count = Some(3);
                row.images_missing_alt = 2;
                row.images_alt_too_long = 1;
                row.mixed_content_count = 1;
                row.insecure_form_count = 1;
                row.hreflang_invalid_count = 1;
                row.structured_data_error_count = 1;
                row.structured_data_warning_count = 1;
                row.deprecated_html_tag_count = 1;
                row.duplicate_id_count = 1;
                row.rendered_dom_changed = true;
                row.canonical_count = 2;
            }
            1 => {
                row.title = None;
                row.meta_description = Some("   ".into());
                row.h1 = None;
                row.h2 = Some(String::new());
                row.canonical = None;
                row.title_count = None;
                row.meta_description_count = None;
            }
            2 => row.indexability_status = "Response body incomplete".into(),
            3 => row.content_type = Some("image/png".into()),
            4 => row.content_type = None,
            5 => {
                row.status_code = None;
                row.error = Some("Connection failed".into());
            }
            6 => {
                row.status_code = None;
                row.status_text = "Blocked by robots.txt".into();
                row.error = Some("Blocked by robots.txt".into());
            }
            7 => {
                row.status_code = None;
                row.error = Some("Blocked by robots.txt".into());
            }
            8 => row.status_code = None,
            9 => {
                row.status_code = Some(302);
                row.error = Some("Redirect limit".into());
            }
            10 => row.status_code = Some(404),
            11 => row.status_code = Some(503),
            12 => row.status_code = Some(600),
            13 => {
                row.status_code = Some(404);
                row.indexability = "Non-indexable".into();
                row.indexability_status = "Meta robots noindex".into();
            }
            14 => {
                row.classification = UrlClassification::External;
                row.viewport = true;
                row.hsts_header = true;
            }
            15 => {
                row.redirect_chain = vec![RedirectHop {
                    url: row.url.clone(),
                    status_code: 301,
                    location: Some(row.final_url.clone()),
                    dns_lookup_time_ms: None,
                    tcp_connect_time_ms: None,
                    tls_handshake_time_ms: None,
                    ttfb_ms: None,
                    elapsed_ms: None,
                }]
            }
            16 => {
                row.url = "http://example.test/plain".into();
                row.final_url = row.url.clone();
                row.storage_key = row.url.clone();
                row.content_type = Some("TEXT/HTML".into());
            }
            17 => row.indexability = "Unknown".into(),
            18 | 19 => {
                row.url = "https://example.test/list".into();
                row.final_url = "https://example.test/list-final".into();
                row.storage_key = format!("list:{index}:{}", row.url);
                row.list_position = Some((30 - index) as u32);
                row.list_duplicate_index = (index - 18) as u32;
            }
            20 => row.json_ld_invalid_count = 1,
            _ => row.inlink_count = 2,
        }
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
        assert_summary_eq(sqlite.progress_summary(), memory.progress_summary());
    }
    let expected = memory.progress_summary();
    let progress = ActiveStore::Sqlite(sqlite.clone()).progress_summary();
    assert_summary_eq(progress, expected);
    // Summary counts do not decode unrelated record payloads or apply the grid window.
    sqlite.connection().unwrap().execute_batch(
        "UPDATE crawl_records SET custom_extractions = '{invalid JSON', response_time_ms = 'invalid integer';"
    ).unwrap();
    assert!(sqlite.try_records().is_err());
    assert_summary_eq(sqlite.progress_summary(), memory.progress_summary());
}

#[test]
fn sqlite_progress_summary_bounds_record_visits_without_timing_thresholds() {
    const RECORDS: usize = 128;
    let sqlite = SqliteStore::in_memory().unwrap();
    for index in 0..RECORDS {
        let mut row = page(index);
        row.near_duplicate_cluster_id = Some(1);
        sqlite.try_upsert(row).unwrap();
    }
    let visits = Arc::new(AtomicUsize::new(0));
    let work = visits.clone();
    {
        let conn = sqlite.connection().unwrap();
        conn.create_scalar_function(
            "count_summary_visit",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            move |_| {
                work.fetch_add(1, AtomicOrdering::Relaxed);
                Ok(true)
            },
        )
        .unwrap();
        // This view counts visits while preserving every record and its indexed predicates.
        conn.execute_batch("ALTER TABLE crawl_records RENAME TO summary_records;
            CREATE VIEW crawl_records AS SELECT * FROM summary_records WHERE count_summary_visit(id);")
            .unwrap();
    }
    let summary = sqlite.try_progress_summary().unwrap();
    assert_eq!(
        (
            summary.total,
            summary.near_duplicates,
            summary.title_duplicate
        ),
        (RECORDS, RECORDS, RECORDS)
    );
    let actual = visits.load(AtomicOrdering::Relaxed);
    eprintln!("progress summary: {RECORDS} records; {actual} counted record visits");
    assert!(
        actual <= 7 * RECORDS,
        "Repeated summary scans visited {actual} records"
    );
    sqlite.try_progress_summary().unwrap();
    assert_eq!(
        visits.load(AtomicOrdering::Relaxed),
        actual,
        "An unchanged summary must use its existing revision cache"
    );
}

#[test]
fn sqlite_progress_summary_normalizes_each_populated_duplicate_value_once() {
    const RECORDS: usize = 128;
    let sqlite = SqliteStore::in_memory().unwrap();
    for index in 0..RECORDS {
        sqlite.try_upsert(page(index)).unwrap();
    }
    let work = count_text_work(&sqlite);
    for column in ["title", "meta_description", "h1", "h2"] {
        assert_eq!(
            sqlite_duplicate_count(&sqlite.connection().unwrap(), column).unwrap(),
            RECORDS
        );
    }
    let calls = work.load(AtomicOrdering::Relaxed);
    eprintln!("four duplicate summaries: {RECORDS} records; {calls} text normalizations");
    assert!(
        calls <= 5 * RECORDS,
        "Duplicate summaries normalized text {calls} times"
    );
    sqlite.connection().unwrap().execute_batch(
        "UPDATE crawl_records SET title = NULL, meta_description = '', h1 = char(9, 160), h2 = '   ';"
    ).unwrap();
    for column in ["title", "meta_description", "h1", "h2"] {
        assert_eq!(
            sqlite_duplicate_count(&sqlite.connection().unwrap(), column).unwrap(),
            0
        );
    }
}

#[test]
fn sqlite_progress_summary_keeps_revision_invalidation_external_writes_and_rollback() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-progress-summary-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let reader = SqliteStore::open(&path).unwrap();
        reader.try_upsert(page(0)).unwrap();
        reader.try_upsert(page(1)).unwrap();
        let work = count_text_work(&reader);
        let initial = reader.try_progress_summary().unwrap();
        assert_eq!((initial.total, initial.title_duplicate), (2, 2));
        let initial_work = work.load(AtomicOrdering::Relaxed);
        reader
            .try_save_frontier_state(CrawlFrontierState::default())
            .unwrap();
        assert_summary_eq(reader.try_progress_summary().unwrap(), initial.clone());
        assert_eq!(work.load(AtomicOrdering::Relaxed), initial_work);

        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("BEGIN; UPDATE crawl_records SET title = NULL; DELETE FROM crawl_records WHERE id = 2; ROLLBACK;").unwrap();
        assert_summary_eq(reader.try_progress_summary().unwrap(), initial);
        assert_eq!(work.load(AtomicOrdering::Relaxed), initial_work);
        writer
            .execute("UPDATE crawl_records SET title = NULL WHERE id = 1", [])
            .unwrap();
        let changed = reader.try_progress_summary().unwrap();
        assert_eq!(
            (
                changed.total,
                changed.title_missing,
                changed.title_duplicate
            ),
            (2, 1, 0)
        );
        assert!(work.load(AtomicOrdering::Relaxed) > initial_work);
        writer
            .execute("DELETE FROM crawl_records WHERE id = 2", [])
            .unwrap();
        assert_eq!(reader.try_progress_summary().unwrap().total, 1);
        reader.try_upsert(page(0)).unwrap();
        assert_eq!(reader.try_progress_summary().unwrap().title_missing, 0);
        reader.try_clear().unwrap();
        assert_summary_eq(
            reader.try_progress_summary().unwrap(),
            CrawlSummary::default(),
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "measures fresh SQLite progress summaries after record writes"]
fn sqlite_progress_summary_workload() {
    const RECORDS: usize = 10_000;
    for mode in ["mixed", "repeated", "empty"] {
        let sqlite = SqliteStore::in_memory().unwrap();
        for index in 0..RECORDS {
            let mut row = page(index);
            row.title = Some(format!("Synthetic title {}", index % 100));
            if mode == "mixed" {
                row.meta_description = Some(format!("Synthetic description {index}"));
                row.h1 = Some(format!("Synthetic heading {index}"));
                row.h2 = Some(format!("Synthetic subheading {index}"));
            } else if mode == "empty" {
                row.title = None;
                row.meta_description = Some("\t\u{a0} ".into());
                row.h1 = Some(String::new());
                row.h2 = None;
            }
            match index % 29 {
                0 => row.status_code = Some(404),
                1 => row.indexability_status = "Response body incomplete".into(),
                2 => row.content_type = Some("image/png".into()),
                3 => {
                    row.status_code = None;
                    row.error = Some("Blocked by robots.txt".into());
                }
                4 => {
                    row.status_code = None;
                    row.error = Some("Connection failed".into());
                }
                5 => row.status_code = Some(503),
                _ => {}
            }
            row.near_duplicate_cluster_id = (index % 5 == 0).then_some(index as u64 / 10);
            sqlite.try_upsert(row).unwrap();
        }
        let expected = sqlite.try_progress_summary().unwrap();
        let mut samples = Vec::new();
        for pass in 0..7 {
            sqlite
                .connection()
                .unwrap()
                .execute(
                    "UPDATE crawl_records SET response_time_ms = ?1 WHERE id = 1",
                    [pass],
                )
                .unwrap();
            let start = std::time::Instant::now();
            let actual = sqlite.try_progress_summary().unwrap();
            samples.push(start.elapsed());
            assert_summary_eq(actual, expected.clone());
        }
        samples.sort();
        eprintln!(
            "progress records={RECORDS} mode={mode} samples={} min_ms={:.3} median_ms={:.3} max_ms={:.3}",
            samples.len(),
            samples[0].as_secs_f64() * 1_000.0,
            samples[samples.len() / 2].as_secs_f64() * 1_000.0,
            samples[samples.len() - 1].as_secs_f64() * 1_000.0
        );
    }
}
