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
        // A later local mutation cannot hide the intervening external commit.
        reader.try_upsert(page(2)).unwrap();
        let mixed = reader.try_progress_summary().unwrap();
        assert_eq!(
            (mixed.total, mixed.title_missing, mixed.title_duplicate),
            (2, 1, 0)
        );
        writer
            .execute("DELETE FROM crawl_records WHERE id != 1", [])
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
fn sqlite_progress_summary_recounts_only_changed_records() {
    let sqlite = SqliteStore::in_memory().unwrap();
    let memory = MemoryStore::new();
    for index in 0..256 {
        let mut row = page(index);
        row.near_duplicate_cluster_id = Some((index / 2) as u64);
        sqlite.try_upsert(row.clone()).unwrap();
        memory.upsert(row);
    }
    assert_summary_eq(
        sqlite.try_progress_summary().unwrap(),
        memory.progress_summary(),
    );
    let work = count_text_work(&sqlite);
    let mut changed = page(0);
    changed.title = None;
    changed.h1 = Some("Unique heading".into());
    changed.near_duplicate_cluster_id = Some(900);
    changed.in_sitemap = true;
    sqlite.try_upsert(changed.clone()).unwrap();
    memory.upsert(changed);
    sqlite
        .try_add_inlink("https://example.test/page/0")
        .unwrap();
    memory.add_inlink("https://example.test/page/0");
    assert_summary_eq(
        sqlite.try_progress_summary().unwrap(),
        memory.progress_summary(),
    );
    assert!(
        work.load(AtomicOrdering::Relaxed) <= 8,
        "One changed record must not normalize unchanged records: {} calls",
        work.load(AtomicOrdering::Relaxed)
    );
    work.store(0, AtomicOrdering::Relaxed);
    sqlite.try_upsert(page(256)).unwrap();
    memory.upsert(page(256));
    assert_summary_eq(
        sqlite.try_progress_summary().unwrap(),
        memory.progress_summary(),
    );
    assert!(work.load(AtomicOrdering::Relaxed) <= 8);
    sqlite.connection().unwrap().execute_batch(
        "BEGIN; UPDATE crawl_records SET title = NULL; DELETE FROM crawl_records WHERE id = 1; ROLLBACK;"
    ).unwrap();
    work.store(0, AtomicOrdering::Relaxed);
    assert_summary_eq(
        sqlite.try_progress_summary().unwrap(),
        memory.progress_summary(),
    );
    assert_eq!(work.load(AtomicOrdering::Relaxed), 0);
    sqlite.try_clear().unwrap();
    assert_summary_eq(
        sqlite.try_progress_summary().unwrap(),
        CrawlSummary::default(),
    );
    sqlite.try_upsert(page(0)).unwrap();
    let summary = sqlite.try_progress_summary().unwrap();
    assert_eq!(
        (
            summary.total,
            summary.title_duplicate,
            summary.near_duplicates
        ),
        (1, 0, 0)
    );
}

#[test]
fn sqlite_incremental_summary_removes_old_ids_and_duplicate_membership() {
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut a = page(0);
    a.near_duplicate_cluster_id = Some(7);
    let mut b = page(1);
    b.near_duplicate_cluster_id = Some(7);
    sqlite.try_upsert(a).unwrap();
    sqlite.try_upsert(b).unwrap();
    let summary = sqlite.try_progress_summary().unwrap();
    assert_eq!(
        (
            summary.total,
            summary.title_duplicate,
            summary.near_duplicates
        ),
        (2, 2, 2)
    );
    sqlite
        .connection()
        .unwrap()
        .execute_batch(
            "UPDATE crawl_records SET id = 100, content_type = 'image/png' WHERE id = 1;",
        )
        .unwrap();
    let summary = sqlite.try_progress_summary().unwrap();
    assert_eq!(
        (
            summary.total,
            summary.title_duplicate,
            summary.near_duplicates
        ),
        (2, 0, 0)
    );
    sqlite.connection().unwrap().execute_batch(
        "DELETE FROM crawl_records WHERE id = 100;
         UPDATE crawl_records SET id = 100, title = NULL, near_duplicate_cluster_id = NULL WHERE id = 2;"
    ).unwrap();
    let summary = sqlite.try_progress_summary().unwrap();
    assert_eq!(
        (
            summary.total,
            summary.title_missing,
            summary.title_duplicate,
            summary.near_duplicates
        ),
        (1, 1, 0, 0)
    );
    // A failing refresh must discard its partially adjusted cache before retrying.
    sqlite
        .connection()
        .unwrap()
        .execute_batch("UPDATE crawl_records SET amphtml_targets = 'invalid JSON' WHERE id = 100;")
        .unwrap();
    assert!(sqlite.try_progress_summary().is_err());
    sqlite
        .connection()
        .unwrap()
        .execute_batch("UPDATE crawl_records SET amphtml_targets = '[]' WHERE id = 100;")
        .unwrap();
    let summary = sqlite.try_progress_summary().unwrap();
    assert_eq!(
        (
            summary.total,
            summary.title_missing,
            summary.title_duplicate
        ),
        (1, 1, 0)
    );
}

#[test]
fn sqlite_progress_summary_uses_one_snapshot_during_external_commit() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-summary-snapshot-{}-{}.sqlite3",
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
        let writer_path = path.clone();
        let written = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let did_write = written.clone();
        reader
            .connection()
            .unwrap()
            .create_scalar_function(
                "ff_text_key",
                1,
                rusqlite::functions::FunctionFlags::SQLITE_UTF8,
                move |context| {
                    if !did_write.swap(true, AtomicOrdering::SeqCst) {
                        Connection::open(&writer_path)?
                            .execute("UPDATE crawl_records SET title = NULL", [])?;
                    }
                    Ok(normalize_text_key(
                        context
                            .get::<Option<String>>(0)?
                            .as_deref()
                            .unwrap_or_default(),
                    ))
                },
            )
            .unwrap();
        let before = reader.try_progress_summary().unwrap();
        assert!(written.load(AtomicOrdering::SeqCst));
        assert_eq!(
            (before.total, before.title_missing, before.title_duplicate),
            (2, 0, 2)
        );
        let after = reader.try_progress_summary().unwrap();
        assert_eq!(
            (after.total, after.title_missing, after.title_duplicate),
            (2, 2, 0)
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
