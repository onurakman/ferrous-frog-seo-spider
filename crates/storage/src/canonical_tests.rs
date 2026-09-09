use super::*;

const VIEWS: [&str; 6] = [
    "canonicalUncrawled",
    "canonicalToRedirect",
    "canonicalToError",
    "canonicalNonIndexable",
    "canonicalChain",
    "canonicalLoop",
];

fn view(name: &str) -> IssueView {
    serde_json::from_value(serde_json::json!(name)).expect("Canonical audit view must be supported")
}

fn page(path: &str, canonical: Option<&str>) -> CrawlRecord {
    let url = format!("https://example.test/{path}");
    let mut row = CrawlRecord::pending(url, 0);
    row.status_code = Some(200);
    row.status_text = "OK".into();
    row.content_type = Some("text/html; charset=utf-8".into());
    row.indexability = "Indexable".into();
    row.indexability_status = "Indexable".into();
    row.canonical = canonical.map(|path| {
        if path.starts_with("https://") {
            path.to_string()
        } else {
            format!("https://example.test/{path}")
        }
    });
    row.canonical_count = usize::from(canonical.is_some());
    row
}

fn redirect_hop(path: &str) -> RedirectHop {
    RedirectHop {
        url: format!("https://example.test/{path}"),
        status_code: 301,
        location: Some("https://example.test/target".into()),
        dns_lookup_time_ms: None,
        tcp_connect_time_ms: None,
        tls_handshake_time_ms: None,
        ttfb_ms: None,
        elapsed_ms: None,
    }
}

fn target_fixture() -> Vec<CrawlRecord> {
    let mut redirect = page("old", Some("target"));
    redirect.final_url = "https://example.test/target".into();
    redirect.redirect_chain = vec![redirect_hop("old"), redirect_hop("middle")];
    let mut error = page("missing", None);
    error.status_code = Some(404);
    error.indexability = "Non-indexable".into();
    error.indexability_status = "Client error".into();
    let mut failed = CrawlRecord::pending("https://example.test/failed".into(), 0);
    failed.error = Some("Connection refused".into());
    let mut blocked = CrawlRecord::pending("https://example.test/blocked".into(), 0);
    blocked.status_text = "Blocked by robots.txt".into();
    blocked.error = Some("Blocked by robots.txt".into());
    let mut noindex = page("noindex", None);
    noindex.indexability = "Non-indexable".into();
    noindex.indexability_status = "Meta noindex".into();
    let mut incomplete = page("incomplete-source", Some("unseen"));
    incomplete.indexability_status = "Response body incomplete".into();
    let mut not_html = page("image-source", Some("unseen"));
    not_html.content_type = Some("image/png".into());
    let mut external = page("external-source", Some("unseen"));
    external.classification = UrlClassification::External;
    let mut direct_target = page("target", Some("target"));
    direct_target.storage_key = "list:20:https://example.test/target".into();
    let mut duplicate_target = page("missing", Some("missing"));
    duplicate_target.storage_key = "list:21:https://example.test/missing".into();
    let mut duplicate_source = page("to-error", Some("missing"));
    duplicate_source.storage_key = "list:22:https://example.test/to-error".into();
    vec![
        redirect,
        error,
        failed,
        blocked,
        noindex,
        CrawlRecord::pending("https://example.test/pending".into(), 0),
        page("to-redirect", Some("old#fragment")),
        page("to-hop", Some("middle")),
        page("to-final", Some("target#fragment")),
        page("to-error", Some("missing")),
        page("to-failed", Some("failed")),
        page("to-blocked", Some("blocked")),
        page("to-noindex", Some("noindex")),
        page("to-unseen", Some("unseen")),
        page("to-pending", Some("pending")),
        page("to-external", Some("https://uncrawled.test/page")),
        page("self", Some("self#fragment")),
        page("", Some("https://example.test")),
        incomplete,
        not_html,
        external,
        direct_target,
        duplicate_target,
        duplicate_source,
    ]
}

fn source_paths(rows: &[CrawlRecord]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row.url
                .strip_prefix("https://example.test/")
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn canonical_target_views_and_summary_preserve_alias_evidence_and_list_occurrences() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for row in target_fixture() {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    let expected: [Vec<&str>; 6] = [
        vec!["to-pending", "to-unseen"],
        vec!["to-hop", "to-redirect"],
        vec!["to-error", "to-error", "to-failed"],
        vec!["to-blocked", "to-noindex"],
        vec![],
        vec![],
    ];
    for (name, expected) in VIEWS.into_iter().zip(expected) {
        let query = GridQuery {
            view: view(name),
            sort_by: Some("url".into()),
            ..GridQuery::default()
        };
        let actual = sqlite.try_query(query.clone()).unwrap();
        let in_memory = memory.query(query.clone());
        assert_eq!(source_paths(&actual.rows), expected, "{name}");
        assert_eq!(source_paths(&in_memory.rows), expected, "{name}");
        assert_eq!(actual.total, expected.len(), "{name}");
        for summary in [actual.summary, in_memory.summary] {
            assert_eq!(
                serde_json::to_value(summary).unwrap()[name],
                expected.len(),
                "{name}"
            );
        }
        for (offset, search, segment) in [
            (0, None, None),
            (1, None, None),
            (0, Some("to-error"), None),
            (0, Some("no-match"), None),
            (0, None, Some("to-(error|pending)")),
        ] {
            let query = GridQuery {
                offset,
                limit: 1,
                global_search: search.map(str::to_string),
                segment_pattern: segment.map(str::to_string),
                segment_regex: true,
                sort_dir: SortDirection::Desc,
                ..query.clone()
            };
            let expected = memory.query(query.clone());
            let actual = sqlite.try_query(query).unwrap();
            assert_eq!(actual.total, expected.total, "{name} offset={offset}");
            assert_eq!(
                actual
                    .rows
                    .iter()
                    .map(|r| &r.storage_key)
                    .collect::<Vec<_>>(),
                expected
                    .rows
                    .iter()
                    .map(|r| &r.storage_key)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn canonical_chains_and_loops_ignore_self_and_shared_final_aliases() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        for row in [
            page("a", Some("b")),
            page("b", Some("c")),
            page("c", Some("c#fragment")),
            page("x", Some("y")),
            page("y", Some("z")),
            page("z", Some("x#fragment")),
            page("enters-loop", Some("x")),
            page("self", Some("self")),
        ] {
            store.upsert(row);
        }
        let mut alias = page("self-alias", Some("self"));
        alias.final_url = "https://example.test/self".into();
        alias.redirect_chain = vec![redirect_hop("self-alias")];
        store.upsert(alias);
        for (name, paths) in [
            ("canonicalChain", vec!["a", "enters-loop", "x", "y", "z"]),
            ("canonicalLoop", vec!["enters-loop", "x", "y", "z"]),
        ] {
            let response = store.query(GridQuery {
                view: view(name),
                sort_by: Some("url".into()),
                ..GridQuery::default()
            });
            assert_eq!(source_paths(&response.rows), paths, "{name}");
            assert_eq!(
                serde_json::to_value(response.summary).unwrap()[name],
                paths.len()
            );
        }
    }
}

#[test]
fn canonical_queries_read_narrow_evidence_and_invalidate_after_target_changes() {
    let store = SqliteStore::in_memory().unwrap();
    store.try_upsert(page("source", Some("target"))).unwrap();
    let query = GridQuery {
        view: view("canonicalUncrawled"),
        ..GridQuery::default()
    };
    assert_eq!(store.try_query(query.clone()).unwrap().total, 1);
    store.try_upsert(page("target", Some("target"))).unwrap();
    // The selected source is valid; target audit evidence must not hydrate unrelated fields.
    store.connection().unwrap().execute("UPDATE crawl_records SET response_time_ms = 'unselected-payload' WHERE url = 'https://example.test/target'", []).unwrap();
    assert!(store.try_records().is_err());
    assert_eq!(store.try_query(query.clone()).unwrap().total, 0);
    let mut changed = page("target", None);
    changed.status_code = Some(503);
    store.try_upsert(changed).unwrap();
    let errors = GridQuery {
        view: view("canonicalToError"),
        ..GridQuery::default()
    };
    assert_eq!(store.try_query(errors).unwrap().total, 1);
    store.try_clear().unwrap();
    assert_eq!(store.try_query(query).unwrap().total, 0);
}

#[test]
fn canonical_loop_detection_handles_long_paths_without_a_recursion_limit() {
    let store = SqliteStore::in_memory().unwrap();
    for index in 0..1500 {
        store
            .try_upsert(page(
                &format!("cycle/{index}"),
                Some(&format!("cycle/{}", (index + 1) % 1500)),
            ))
            .unwrap();
    }
    let response = store
        .try_query(GridQuery {
            view: view("canonicalLoop"),
            offset: 1499,
            limit: 1,
            ..GridQuery::default()
        })
        .unwrap();
    assert_eq!(response.total, 1500);
    assert_eq!(response.rows.len(), 1);
    assert_eq!(
        serde_json::to_value(response.summary).unwrap()["canonicalLoop"],
        1500
    );
}

#[test]
fn canonical_cache_is_shared_by_queries_and_never_built_by_progress() {
    let store = SqliteStore::in_memory().unwrap();
    store.try_upsert(page("source", Some("missing"))).unwrap();
    assert_eq!(store.progress_summary().total, 1);
    assert!(store.reference_cache.lock().unwrap().is_none());
    assert_eq!(store.try_summary().unwrap().canonical_uncrawled, 1);
    let revision = store
        .reference_cache
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .revision;
    for name in VIEWS {
        store
            .try_query(GridQuery {
                view: view(name),
                limit: 1,
                ..GridQuery::default()
            })
            .unwrap();
        assert_eq!(
            store
                .reference_cache
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .revision,
            revision
        );
    }
    store.try_upsert(page("missing", Some("missing"))).unwrap();
    assert_eq!(store.progress_summary().total, 2);
    assert_eq!(
        store
            .reference_cache
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .revision,
        revision,
        "Each progress update must avoid rebuilding cross-page diagnostics"
    );
    assert_eq!(store.try_summary().unwrap().canonical_uncrawled, 0);
    assert_ne!(
        store
            .reference_cache
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .revision,
        revision
    );
}

#[test]
fn canonical_cache_observes_changes_from_another_sqlite_connection() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-canonical-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let reader = SqliteStore::open(&path).unwrap();
    let writer = SqliteStore::open(&path).unwrap();
    writer.try_upsert(page("source", Some("target"))).unwrap();
    assert_eq!(reader.try_summary().unwrap().canonical_uncrawled, 1);
    writer.try_upsert(page("target", Some("source"))).unwrap();
    let summary = reader.try_summary().unwrap();
    assert_eq!(
        (summary.canonical_uncrawled, summary.canonical_loop),
        (0, 2)
    );
    writer.try_upsert(page("target", Some("target"))).unwrap();
    assert_eq!(reader.try_summary().unwrap().canonical_loop, 0);
    drop(reader);
    drop(writer);
    std::fs::remove_file(path).unwrap();
}

fn count_aggregate_work(store: &SqliteStore) -> Arc<std::sync::atomic::AtomicUsize> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let work = Arc::new(AtomicUsize::new(0));
    let calls = work.clone();
    store
        .connection()
        .unwrap()
        .create_scalar_function(
            "ff_text_key",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            move |context| {
                calls.fetch_add(1, Ordering::Relaxed);
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

fn frontier_fixture(count: usize) -> CrawlFrontierState {
    CrawlFrontierState {
        queued: (0..count)
            .map(|index| {
                let url = format!("https://example.test/queued/{index}");
                CrawlFrontierItem {
                    storage_key: url.clone(),
                    url,
                    depth: 1,
                    from_sitemap: false,
                    list_position: None,
                    list_duplicate_index: 0,
                }
            })
            .collect(),
        seen: (0..count)
            .map(|index| format!("https://example.test/seen/{index}"))
            .collect(),
        crawled: count,
    }
}

#[test]
fn audit_caches_ignore_frontier_temporary_and_edge_only_writes() {
    use std::sync::atomic::Ordering;
    let store = SqliteStore::in_memory().unwrap();
    let work = count_aggregate_work(&store);
    store.try_upsert(page("source", Some("target"))).unwrap();
    store.try_summary().unwrap();
    let expected_work = work.load(Ordering::Relaxed);
    let revision = store
        .reference_cache
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .revision;
    for operation in ["frontier", "temporary", "edge", "clear frontier"] {
        match operation {
            "frontier" => store.try_save_frontier_state(frontier_fixture(50)).unwrap(),
            "temporary" => {
                store.connection().unwrap().execute_batch("CREATE TEMP TABLE audit_probe (value INTEGER); INSERT INTO audit_probe VALUES (1)").unwrap();
            }
            "edge" => {
                store
                    .try_add_link_edge(LinkEdge {
                        id: 0,
                        source_url: "https://example.test/source".into(),
                        target_url: "https://example.test/target".into(),
                        anchor_text: "Target".into(),
                        rel: String::new(),
                        rel_nofollow: false,
                        link_type: LinkType::Internal,
                        source_status_code: None,
                        target_status_code: None,
                        source_depth: 0,
                        target_depth: None,
                        source_position: 1,
                        discovery_order: 0,
                    })
                    .unwrap();
            }
            _ => store.try_clear_frontier_state().unwrap(),
        }
        assert_eq!(store.progress_summary().total, 1);
        assert_eq!(store.try_summary().unwrap().canonical_uncrawled, 1);
        assert_eq!(
            work.load(Ordering::Relaxed),
            expected_work,
            "{operation} must reuse aggregate queries"
        );
        assert_eq!(
            store
                .reference_cache
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .revision,
            revision,
            "{operation} must reuse canonical graph"
        );
    }
    // A link-count change on the record is audit evidence (e.g. sitemap orphan status).
    store.try_add_inlink("https://example.test/source").unwrap();
    store.try_summary().unwrap();
    assert!(work.load(Ordering::Relaxed) > expected_work);
}

#[test]
fn audit_cache_revision_rolls_back_with_record_changes() {
    use std::sync::atomic::Ordering;
    let store = SqliteStore::in_memory().unwrap();
    let work = count_aggregate_work(&store);
    store.try_upsert(page("a", Some("b"))).unwrap();
    store.try_upsert(page("b", Some("a"))).unwrap();
    assert_eq!(store.try_summary().unwrap().canonical_loop, 2);
    let expected_work = work.load(Ordering::Relaxed);
    {
        let mut conn = store.connection().unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute(
            "UPDATE crawl_records SET canonical = final_url WHERE url = 'https://example.test/b'",
            [],
        )
        .unwrap();
        tx.execute(
            "DELETE FROM crawl_records WHERE url = 'https://example.test/a'",
            [],
        )
        .unwrap();
        tx.rollback().unwrap();
    }
    assert_eq!(store.try_summary().unwrap().canonical_loop, 2);
    assert_eq!(
        work.load(Ordering::Relaxed),
        expected_work,
        "Rolled-back rows must preserve cached evidence"
    );
    store
        .connection()
        .unwrap()
        .execute(
            "DELETE FROM crawl_records WHERE url = 'https://example.test/b'",
            [],
        )
        .unwrap();
    let summary = store.try_summary().unwrap();
    assert_eq!(
        (
            summary.total,
            summary.canonical_loop,
            summary.canonical_uncrawled
        ),
        (1, 0, 1)
    );
    assert!(work.load(Ordering::Relaxed) > expected_work);
}

#[test]
fn audit_revision_triggers_survive_migration_and_external_writes() {
    use std::sync::atomic::Ordering;
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-audit-revision-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = SqliteStore::open(&path).unwrap();
        store.try_upsert(page("a", Some("b"))).unwrap();
        store.try_upsert(page("b", Some("a"))).unwrap();
    }
    // Reproduce a pre-revision database with the old final-URL unique constraint. Migration
    // rebuilds crawl_records, so triggers must be installed after that replacement.
    Connection::open(&path).unwrap().execute_batch("DROP TRIGGER IF EXISTS crawl_records_audit_insert;
        DROP TRIGGER IF EXISTS crawl_records_audit_update; DROP TRIGGER IF EXISTS crawl_records_audit_delete;
        DROP TABLE IF EXISTS crawl_audit_revision;
        CREATE UNIQUE INDEX legacy_final_url ON crawl_records(final_url);").unwrap();
    let reader = SqliteStore::open(&path).unwrap();
    let work = count_aggregate_work(&reader);
    assert_eq!(reader.try_summary().unwrap().canonical_loop, 2);
    let expected_work = work.load(Ordering::Relaxed);
    let writer = SqliteStore::open(&path).unwrap();
    writer
        .try_save_frontier_state(frontier_fixture(50))
        .unwrap();
    assert_eq!(reader.try_summary().unwrap().canonical_loop, 2);
    assert_eq!(
        work.load(Ordering::Relaxed),
        expected_work,
        "An external frontier write must not invalidate audit evidence"
    );
    // Use a plain SQLite writer too; correctness must not depend on calling store methods.
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE crawl_records SET canonical = final_url WHERE url = 'https://example.test/b'",
            [],
        )
        .unwrap();
    assert_eq!(reader.try_summary().unwrap().canonical_loop, 0);
    assert!(work.load(Ordering::Relaxed) > expected_work);
    drop(reader);
    drop(writer);
    let reopened = SqliteStore::open(&path).unwrap();
    assert_eq!(reopened.try_summary().unwrap().canonical_loop, 0);
    reopened.try_clear().unwrap();
    assert_eq!(reopened.try_summary().unwrap().total, 0);
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "measures a repeated 10,000-record frontier/query workload"]
fn audit_cache_frontier_query_workload() {
    use std::sync::atomic::Ordering;
    let store = SqliteStore::in_memory().unwrap();
    let work = count_aggregate_work(&store);
    for index in 0..10_000 {
        let mut row = page(&format!("page/{index}"), Some("unseen"));
        row.title = Some("Shared title".into());
        store.try_upsert(row).unwrap();
    }
    let initial = store.try_summary().unwrap();
    assert_eq!(
        (initial.total, initial.canonical_uncrawled),
        (10_000, 10_000)
    );
    let initial_work = work.load(Ordering::Relaxed);
    let started = std::time::Instant::now();
    for iteration in 0..8 {
        store
            .try_save_frontier_state(frontier_fixture(100))
            .unwrap();
        assert_eq!(store.progress_summary().total, 10_000);
        let page = store
            .try_query(GridQuery {
                view: IssueView::CanonicalUncrawled,
                limit: 50,
                offset: iteration * 50,
                ..GridQuery::default()
            })
            .unwrap();
        assert_eq!((page.total, page.rows.len()), (10_000, 50));
    }
    let calls = work.load(Ordering::Relaxed) - initial_work;
    eprintln!(
        "10,000 records; 8 frontier(100)+progress+50-row canonical queries: {:?}; {calls} aggregate text-function calls",
        started.elapsed()
    );
    assert_eq!(
        calls, 0,
        "Unchanged record evidence must reuse both audit caches"
    );
}
