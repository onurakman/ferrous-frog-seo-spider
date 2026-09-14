use super::*;
use ferrous_frog_crawler_core::CrawlMode;
use ferrous_frog_storage::{CrawlFrontierItem, SqliteStore};
use std::sync::atomic::AtomicUsize;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrous-frog-sessions-{}-{}-{}",
            std::process::id(),
            now_ms(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn idle_state() -> AppState {
    AppState {
        store: Mutex::new(ActiveStore::memory()),
        control: Mutex::new(None),
        crawl_task: tokio::sync::Mutex::new(None),
        current_session_id: Mutex::new(None),
        comparison: Mutex::new(crate::ComparisonState::default()),
        frontend_ready: AtomicBool::new(true),
        exit_confirmed: AtomicBool::new(false),
    }
}

fn record(path: &str) -> CrawlRecord {
    let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
    record.status_code = Some(200);
    record.content_type = Some("text/html; charset=utf-8".to_string());
    record.indexability = "Indexable".to_string();
    record.indexability_status = "Indexable".to_string();
    record.title = Some("Shared title".to_string());
    record.meta_description = Some("Shared description".to_string());
    record
}

#[tokio::test]
async fn comparison_workspace_keeps_snapshots_and_preserves_the_open_crawl() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let index = sessions::index_connection(&dir.0).unwrap();
    let (baseline, old) =
        sessions::create_session(&index, &dir.0, "Baseline", "", None, "ready").unwrap();
    let (current, new) =
        sessions::create_session(&index, &dir.0, "Current", "", None, "ready").unwrap();
    old.upsert(record("changed"));
    let mut changed = record("changed");
    changed.title = Some("Updated title".into());
    new.upsert(changed);
    state
        .store
        .lock()
        .unwrap()
        .upsert(record("active-workbench"));
    *state.current_session_id.lock().unwrap() = Some("untouched".into());
    // A comparison must not run library initialization and register this legacy file.
    let legacy = SqliteStore::open(dir.0.join("ferrous-frog-current.sqlite3")).unwrap();
    legacy.upsert(record("unregistered-legacy"));

    let page = prepare_crawl_comparison(
        &state,
        dir.0.clone(),
        OpenCrawlComparisonRequest {
            comparison_id: "first".into(),
            baseline_session_id: Some(baseline.id),
            current_session_id: Some(current.id),
            archive_path: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    let key = page.rows[0].key;
    new.upsert(record("later"));
    let detail = with_comparison_worker(&state, "first", move |workspace| workspace.detail(key))
        .await
        .unwrap();
    assert_eq!(
        detail.current.unwrap().title.as_deref(),
        Some("Updated title")
    );
    let page = with_comparison_worker(&state, "first", |workspace| {
        workspace.query(&ComparisonQuery::default())
    })
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(
        state.current_session_id.lock().unwrap().as_deref(),
        Some("untouched")
    );
    assert_eq!(
        state.store.lock().unwrap().records()[0].url,
        "https://example.test/active-workbench"
    );
    assert_eq!(query_sessions(&index, None).unwrap().len(), 2);

    let path = dir.0.join("comparison.csv");
    let export = with_comparison_worker(&state, "first", move |workspace| {
        write_atomic_export(&path, |file| {
            workspace.write_csv(&ComparisonQuery::default(), file)
        })
    })
    .await
    .unwrap();
    assert_eq!(export.row_count, 1);
    assert!(
        fs::read_to_string(&export.path)
            .unwrap()
            .contains("Updated title")
    );
    state.comparison.lock().unwrap().close("first");
    assert!(
        with_comparison_worker(&state, "first", |workspace| workspace
            .query(&ComparisonQuery::default()))
        .await
        .is_err()
    );
}

#[tokio::test]
async fn comparison_archive_survives_removal_and_failed_replacement_clears_pending_state() {
    let dir = TestDirectory::new();
    let state = idle_state();
    state.store.lock().unwrap().upsert(record("current"));
    let path = dir.0.join("baseline.json");
    let mut baseline = record("previous");
    baseline.id = 1;
    fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1, "records": [baseline], "linkEdges": []
        }))
        .unwrap(),
    )
    .unwrap();
    let request = |id: &str| OpenCrawlComparisonRequest {
        comparison_id: id.into(),
        baseline_session_id: None,
        current_session_id: None,
        archive_path: Some(path.to_string_lossy().into_owned()),
    };
    let page = prepare_crawl_comparison(&state, dir.0.clone(), request("archive"))
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    fs::remove_file(&path).unwrap();
    assert_eq!(
        with_comparison_worker(&state, "archive", |workspace| workspace
            .query(&ComparisonQuery::default()))
        .await
        .unwrap()
        .total,
        2
    );
    assert!(
        prepare_crawl_comparison(&state, dir.0.clone(), request("missing"))
            .await
            .is_err()
    );
    assert!(state.comparison.lock().unwrap().id.is_none());
    assert_eq!(state.store.lock().unwrap().records().len(), 1);
    assert!(!dir.0.join("ferrous-frog-sessions.sqlite3").exists());
}

#[test]
fn comparison_lifecycle_rejects_late_work_and_close_for_an_older_id() {
    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("empty.json");
    fs::write(&archive, r#"{"schemaVersion":1,"records":[]}"#).unwrap();
    let sources =
        comparison_sources::ComparisonSources::archive(&archive, &ActiveStore::memory()).unwrap();
    let workspace = ComparisonWorkspace::new(sources).unwrap();
    let mut state = ComparisonState::default();
    assert!(state.reserve("").is_err());
    let first_generation = state.reserve("first").unwrap();
    assert!(state.reserve("first").is_err());
    assert!(state.get("first").is_err());
    state.reserve("second").unwrap();
    state.close("first");
    assert_eq!(state.id.as_deref(), Some("second"));
    assert!(state.get("first").is_err());
    state.close("second");
    assert!(state.id.is_none());
    state.reserve("first").unwrap();
    assert!(state.publish("first", first_generation, workspace).is_err());
    assert_eq!(state.id.as_deref(), Some("first"));
    assert!(state.workspace.is_none());
}

#[test]
fn fresh_crawls_have_independent_files_and_open_restores_configuration() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let config = CrawlConfig {
        start_url: "https://example.test/".into(),
        user_agent: "SessionTest/1.0".into(),
        ..CrawlConfig::default()
    };
    let (first, first_store, _, index) =
        prepare_crawl_session(&state, &dir.0, config, false).unwrap();
    first_store.upsert(record("first"));
    sessions::save_progress(&index, &first.id, "finished", 1).unwrap();
    let list_config = CrawlConfig {
        mode: CrawlMode::List,
        start_url: String::new(),
        list_urls: vec!["https://example.test/list".into()],
        max_urls: 23,
        resume_from_state: true,
        ..CrawlConfig::default()
    };
    let (second, second_store, engine_config, _) =
        prepare_crawl_session(&state, &dir.0, list_config, false).unwrap();
    assert_ne!(first.id, second.id);
    assert_ne!(first.database_path, second.database_path);
    assert!(Path::new(&first.database_path).is_file());
    assert!(Path::new(&second.database_path).is_file());
    assert_eq!(first_store.query(GridQuery::default()).total, 1);
    assert_eq!(second_store.query(GridQuery::default()).total, 0);
    assert!(!engine_config.resume_from_state);
    let opened = activate_session(&state, &index, &first.id).unwrap();
    assert_eq!(opened.config.unwrap().user_agent, "SessionTest/1.0");
    assert_eq!(opened.crawled, Some(1));
    assert_eq!(opened.status, "finished");
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .query(GridQuery::default())
            .total,
        1
    );
    let opened = activate_session(&state, &index, &second.id).unwrap();
    assert_eq!(opened.mode, CrawlMode::List);
    let config = opened.config.unwrap();
    assert_eq!(config.list_urls, ["https://example.test/list"]);
    assert_eq!(config.max_urls, 23);
    assert_eq!(opened.start_url, "https://example.test/list");
    let history = query_sessions(&sessions::index_connection(&dir.0).unwrap(), None).unwrap();
    assert_eq!(history.len(), 2);
    assert!(history.iter().all(|session| {
        serde_json::to_value(session)
            .unwrap()
            .get("config")
            .is_none()
    }));
}

#[test]
fn invalid_starts_and_missing_files_preserve_the_active_crawl() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let (first, store, _, index) =
        prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), false).unwrap();
    store.upsert(record("kept"));
    let invalid = CrawlConfig {
        start_url: "file:///private".into(),
        ..CrawlConfig::default()
    };
    assert!(prepare_crawl_session(&state, &dir.0, invalid, false).is_err());
    #[cfg(not(feature = "js-rendering"))]
    {
        let mut invalid = CrawlConfig::default();
        invalid.rendering.enabled = true;
        assert!(prepare_crawl_session(&state, &dir.0, invalid, false).is_err());
    }
    assert_eq!(query_sessions(&index, None).unwrap().len(), 1);
    let missing = dir.0.join("missing.sqlite3");
    index.execute("INSERT INTO crawl_sessions (id, name, start_url, database_path, created_at_ms, updated_at_ms)
        VALUES ('missing', 'Missing', '', ?1, 1, 1)", [missing.to_string_lossy().as_ref()]).unwrap();
    let error = activate_session(&state, &index, "missing").unwrap_err();
    assert!(error.contains("unavailable"));
    assert!(!missing.exists());
    assert_eq!(
        get_session(&index, "missing", None).unwrap().status,
        "unavailable"
    );
    assert_eq!(
        state.current_session_id.lock().unwrap().as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(
        state.store.lock().unwrap().query(GridQuery::default()).rows[0].url,
        "https://example.test/kept"
    );
    let broken_directory = TestDirectory::new();
    fs::write(broken_directory.0.join("sessions"), b"blocked").unwrap();
    assert!(
        prepare_crawl_session(&state, &broken_directory.0, CrawlConfig::default(), false).is_err()
    );
    assert_eq!(
        state.current_session_id.lock().unwrap().as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(
        query_sessions(
            &sessions::index_connection(&broken_directory.0).unwrap(),
            None
        )
        .unwrap()
        .len(),
        0
    );
}

#[test]
fn resume_requires_selected_session_saved_targets_and_queued_work() {
    let dir = TestDirectory::new();
    let state = idle_state();
    assert!(prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), true).is_err());
    let config = CrawlConfig::default();
    let (first, store, _, _) =
        prepare_crawl_session(&state, &dir.0, config.clone(), false).unwrap();
    store.upsert(record("kept"));
    assert!(prepare_crawl_session(&state, &dir.0, config.clone(), true).is_err());
    store.save_frontier_state(CrawlFrontierState {
        queued: vec![CrawlFrontierItem {
            url: "https://example.com/next".into(),
            storage_key: "https://example.com/next".into(),
            depth: 1,
            from_sitemap: false,
            list_position: None,
            list_duplicate_index: 0,
        }],
        seen: vec!["https://example.com/next".into()],
        crawled: 1,
    });
    let changed_target = CrawlConfig {
        start_url: "https://other.test/".into(),
        ..config.clone()
    };
    assert!(prepare_crawl_session(&state, &dir.0, changed_target, true).is_err());
    let mut resumed_config = config;
    resumed_config.max_urls = 20_000;
    let (resumed, resumed_store, engine_config, index) =
        prepare_crawl_session(&state, &dir.0, resumed_config, true).unwrap();
    assert_eq!(resumed.id, first.id);
    assert_eq!(resumed.crawled, Some(1));
    assert!(engine_config.resume_from_state);
    assert_eq!(resumed_store.query(GridQuery::default()).total, 1);
    assert_eq!(query_sessions(&index, None).unwrap().len(), 1);
    assert_eq!(
        activate_session(&state, &index, &first.id)
            .unwrap()
            .config
            .unwrap()
            .max_urls,
        20_000
    );
}

#[test]
fn legacy_list_resume_ignores_blank_lines_but_preserves_order_and_duplicates() {
    let dir = TestDirectory::new();
    let index = sessions::index_connection(&dir.0).unwrap();
    let saved = CrawlConfig {
        mode: CrawlMode::List,
        start_url: String::new(),
        list_urls: vec![
            " https://example.test/a ".into(),
            "".into(),
            "https://example.test/b".into(),
            "https://example.test/a".into(),
        ],
        list_sitemap_urls: vec![" https://example.test/map.xml ".into(), "\n".into()],
        ..CrawlConfig::default()
    };
    let (session, _) = sessions::create_session(
        &index,
        &dir.0,
        "Legacy List",
        sessions::seed_url(&saved),
        Some(&saved),
        "stopped",
    )
    .unwrap();
    let session = activate_session(&idle_state(), &index, &session.id).unwrap();
    let mut config = saved.clone();
    config.list_urls = vec![
        "https://example.test/a".into(),
        "https://example.test/b".into(),
        "https://example.test/a".into(),
    ];
    config.list_sitemap_urls = vec!["https://example.test/map.xml".into()];
    assert!(sessions::validate_resume(&session, &config).is_ok());
    let normalized = config.clone();
    config.list_urls.pop();
    assert!(
        sessions::validate_resume(&session, &config).is_err(),
        "Duplicate List targets must remain part of resume identity"
    );
    config = normalized.clone();
    config.list_urls.swap(1, 2);
    assert!(
        sessions::validate_resume(&session, &config).is_err(),
        "List target order must remain part of resume identity"
    );
    config = normalized;
    config.list_sitemap_urls.clear();
    assert!(
        sessions::validate_resume(&session, &config).is_err(),
        "Changing sitemap sources must require a new crawl"
    );
}

#[test]
fn migration_registers_legacy_data_once_and_marks_only_interrupted_runs() {
    let dir = TestDirectory::new();
    let legacy_path = dir.0.join("ferrous-frog-current.sqlite3");
    let legacy = SqliteStore::open(&legacy_path).unwrap();
    legacy.try_upsert(record("legacy")).unwrap();
    let index_path = dir.0.join("ferrous-frog-sessions.sqlite3");
    let index = Connection::open(&index_path).unwrap();
    index
        .execute_batch(
            "CREATE TABLE crawl_sessions (
        id TEXT PRIMARY KEY, name TEXT NOT NULL, start_url TEXT NOT NULL,
        database_path TEXT NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL
    ); CREATE TABLE config_profiles (
        id TEXT PRIMARY KEY, name TEXT NOT NULL, config_json TEXT NOT NULL,
        created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL
    ); INSERT INTO config_profiles VALUES ('profile', 'Kept', '{}', 1, 2);",
        )
        .unwrap();
    index
        .execute(
            "INSERT INTO crawl_sessions VALUES ('old', 'Old', '', ?1, 1, 2)",
            [dir.0.join("old.sqlite3").to_string_lossy().as_ref()],
        )
        .unwrap();
    let index = sessions::index_connection(&dir.0).unwrap();
    assert_eq!(query_sessions(&index, None).unwrap().len(), 2);
    let history = get_session(&index, "legacy-current", None).unwrap();
    assert_eq!(history.crawled, Some(1));
    assert_eq!(history.start_url, "https://example.test/legacy");
    assert!(history.config.is_none());
    assert_eq!(legacy.try_records().unwrap().len(), 1);
    sessions::save_progress(&index, "legacy-current", "running", 3).unwrap();
    sessions::mark_interrupted(&index).unwrap();
    assert_eq!(
        get_session(&index, "legacy-current", None).unwrap().status,
        "interrupted"
    );
    sessions::save_progress(&index, "legacy-current", "finished", 4).unwrap();
    sessions::mark_interrupted(&index).unwrap();
    assert_eq!(
        get_session(&index, "legacy-current", None).unwrap().status,
        "finished"
    );
    sessions::save_progress(&index, "legacy-current", "paused", 5).unwrap();
    let reopened = sessions::index_connection(&dir.0).unwrap();
    assert_eq!(query_sessions(&reopened, None).unwrap().len(), 2);
    assert_eq!(
        get_session(&reopened, "legacy-current", None)
            .unwrap()
            .status,
        "paused"
    );
    assert_eq!(
        reopened
            .query_row(
                "SELECT name FROM config_profiles WHERE id = 'profile'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "Kept"
    );
}

#[test]
fn archive_import_creates_a_saved_file_without_replacing_previous_data() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let (first, store, _, index) =
        prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), false).unwrap();
    store.upsert(record("original"));
    let mut imported = record("imported");
    imported.list_position = Some(0);
    import_archive_into_session(
        &state,
        &dir.0,
        CrawlArchive {
            page_captures: Vec::new(),
            schema_version: 1,
            exported_at_ms: 1,
            records: vec![imported],
            link_edges: Vec::new(),
            image_assets: Vec::new(),
            page_references: Vec::new(),
            frontier_state: None,
        },
    )
    .unwrap();
    let history = query_sessions(&index, None).unwrap();
    assert_eq!(history.len(), 2);
    let imported = history
        .iter()
        .find(|session| session.id != first.id)
        .unwrap();
    assert_eq!(imported.status, "imported");
    assert_eq!(imported.crawled, Some(1));
    assert_eq!(imported.mode, CrawlMode::List);
    assert_eq!(
        store.query(GridQuery::default()).rows[0].url,
        "https://example.test/original"
    );
    assert_eq!(
        state.store.lock().unwrap().query(GridQuery::default()).rows[0].url,
        "https://example.test/imported"
    );
}

#[test]
fn sql_comparison_matches_existing_semantics_without_decoding_full_records() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let index = sessions::index_connection(&dir.0).unwrap();
    let (baseline, old) =
        sessions::create_session(&index, &dir.0, "Baseline", "", None, "ready").unwrap();
    let (current, new) =
        sessions::create_session(&index, &dir.0, "Current", "", None, "ready").unwrap();
    let mut changed = record("changed");
    changed.response_hash = Some("previous-hash".into());
    changed.near_duplicate_cluster_id = Some(7);
    let mut removed = record("removed");
    removed.near_duplicate_cluster_id = Some(7);
    let mut blocked = record("blocked");
    blocked.status_code = None;
    blocked.status_text = "Blocked by robots.txt".into();
    blocked.error = Some("Blocked by robots.txt".into());
    old.try_upsert(changed.clone()).unwrap();
    old.try_upsert(removed).unwrap();
    old.try_upsert(blocked.clone()).unwrap();
    let mut list_first = record("alias");
    list_first.final_url = "https://example.test/changed".into();
    list_first.list_position = Some(0);
    list_first.title = Some("Earlier duplicate".into());
    old.try_upsert(list_first).unwrap();
    changed.status_code = Some(404);
    changed.title = None;
    changed.meta_description = Some("Changed description".into());
    changed.indexability = "Non-indexable".into();
    changed.indexability_status = "Client error".into();
    changed.response_hash = Some("changed-hash".into());
    new.try_upsert(changed).unwrap();
    new.try_upsert(blocked).unwrap();
    new.try_upsert(record("added")).unwrap();
    let mut incomplete = record("incomplete");
    incomplete.title = None;
    incomplete.meta_description = None;
    incomplete.indexability = "Unknown".into();
    incomplete.indexability_status = "Response body incomplete".into();
    incomplete.error = Some("Response exceeds configured limit".into());
    old.try_upsert(incomplete.clone()).unwrap();
    new.try_upsert(incomplete).unwrap();
    let expected = compare_records(
        &old.try_records().unwrap(),
        &new.try_records().unwrap(),
        false,
    );
    // A full CrawlRecord decoder rejects this; comparison needs only scalar columns.
    Connection::open(&baseline.database_path)
        .unwrap()
        .execute(
            "UPDATE crawl_records SET custom_extractions = 'invalid json'",
            [],
        )
        .unwrap();
    activate_session(&state, &index, &current.id).unwrap();
    let result =
        sessions::compare_sessions(&index, &baseline.id, &current.id, None, false).unwrap();
    assert_eq!((result.baseline_records, result.current_records), (5, 4));
    assert_eq!((result.added, result.removed, result.changed), (1, 2, 1));
    assert_eq!(
        (
            result.status_changed,
            result.title_changed,
            result.meta_description_changed,
            result.indexability_changed,
            result.hash_changed
        ),
        (1, 1, 1, 1, 1)
    );
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(
        state.current_session_id.lock().unwrap().as_deref(),
        Some(current.id.as_str())
    );
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .query(GridQuery::default())
            .total,
        4
    );
    assert!(sessions::compare_sessions(&index, &baseline.id, &baseline.id, None, false).is_err());
    assert!(
        sessions::compare_sessions(&index, &baseline.id, &current.id, Some(&current.id), false)
            .is_err()
    );
    assert!(sessions::compare_sessions(&index, "missing", &current.id, None, false).is_err());
}

#[test]
fn sql_comparison_separates_response_noise_content_changes_and_unavailable_evidence() {
    let dir = TestDirectory::new();
    let index = sessions::index_connection(&dir.0).unwrap();
    let (baseline, old) =
        sessions::create_session(&index, &dir.0, "Baseline", "", None, "ready").unwrap();
    let (current, new) =
        sessions::create_session(&index, &dir.0, "Current", "", None, "ready").unwrap();
    for path in [
        "unchanged",
        "nonce-only",
        "content",
        "metadata",
        "status",
        "raw-gained",
        "raw-lost",
        "content-gained",
        "context-gained",
        "different-context",
        "legacy",
        "legacy-response",
        "content-lost",
        "context-lost",
        "counts-gained",
        "counts-lost",
    ] {
        let mut previous = record(path);
        previous.response_hash = Some("response-before".into());
        previous.content_hash = Some("stable-text".into());
        previous.content_hash_context = Some("html-text-v1".into());
        previous.title_count = Some(1);
        previous.meta_description_count = Some(1);
        let mut next = previous.clone();
        match path {
            "nonce-only" => next.response_hash = Some("response-with-new-nonce".into()),
            "content" => {
                next.response_hash = Some("response-after".into());
                next.content_hash = Some("edited-text".into());
            }
            "metadata" => {
                next.title_count = Some(2);
                next.meta_description_count = Some(2);
                next.h1 = Some("New heading".into());
                next.h2_count = 2;
                next.canonical_count = 2;
                next.x_robots_tag = Some("nofollow".into());
            }
            "status" => {
                next.status_code = Some(404);
                next.indexability_status = "Client error".into();
            }
            "raw-gained" => previous.response_hash = None,
            "raw-lost" => next.response_hash = None,
            "content-gained" => previous.content_hash = None,
            "context-gained" => previous.content_hash_context = None,
            "different-context" => {
                next.response_hash = Some("response-after".into());
                next.content_hash = Some("different-region-text".into());
                next.content_hash_context = Some("another-content-region".into());
            }
            "legacy" | "legacy-response" => {
                previous.content_hash = None;
                previous.content_hash_context = None;
                next.content_hash = None;
                next.content_hash_context = None;
                if path == "legacy" {
                    previous.response_hash = None;
                    next.response_hash = None;
                } else {
                    next.response_hash = Some("response-after".into());
                }
            }
            "content-lost" => next.content_hash = None,
            "context-lost" => next.content_hash_context = None,
            "counts-gained" => {
                previous.title_count = None;
                previous.meta_description_count = None;
            }
            "counts-lost" => {
                next.title_count = None;
                next.meta_description_count = None;
            }
            _ => {}
        }
        old.try_upsert(previous).unwrap();
        new.try_upsert(next).unwrap();
    }
    old.try_upsert(record("removed")).unwrap();
    new.try_upsert(record("added")).unwrap();
    let previous = old.try_records().unwrap();
    let next = new.try_records().unwrap();
    Connection::open(&baseline.database_path)
        .unwrap()
        .execute(
            "UPDATE crawl_records SET custom_extractions = 'invalid json'",
            [],
        )
        .unwrap();
    for include_response_only in [false, true] {
        let result = sessions::compare_sessions(
            &index,
            &baseline.id,
            &current.id,
            None,
            include_response_only,
        )
        .unwrap();
        assert_eq!((result.added, result.removed, result.changed), (1, 1, 3));
        assert_eq!(
            (
                result.hash_changed,
                result.content_changed,
                result.response_only,
                result.content_unavailable
            ),
            (4, 1, 3, 7)
        );
        assert_eq!(result.rows.len(), if include_response_only { 8 } else { 5 });
        for row in &result.rows {
            let (fields, comparison): (&[&str], &str) = match row.url.rsplit('/').next().unwrap() {
                "added" | "removed" => (&[], "notApplicable"),
                "content" => (&["content", "responseHash"], "changed"),
                "metadata" => (
                    &[
                        "title",
                        "metaDescription",
                        "headings",
                        "canonical",
                        "robotsDirectives",
                    ],
                    "unchanged",
                ),
                "status" => (&["statusCode", "indexability"], "unchanged"),
                "nonce-only" => (&["responseHash"], "unchanged"),
                "different-context" | "legacy-response" => (&["responseHash"], "unavailable"),
                other => panic!("unexpected comparison row {other}"),
            };
            assert_eq!(row.changed_fields, fields);
            assert_eq!(row.content_comparison, comparison);
            if fields == ["responseHash"] {
                assert_eq!(row.change, "responseOnly");
            }
        }
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(compare_records(&previous, &next, include_response_only)).unwrap(),
        );
    }
}

#[test]
fn sql_comparison_filters_response_noise_before_limiting_details() {
    let dir = TestDirectory::new();
    let index = sessions::index_connection(&dir.0).unwrap();
    let (baseline, old) =
        sessions::create_session(&index, &dir.0, "Baseline", "", None, "ready").unwrap();
    let (current, new) =
        sessions::create_session(&index, &dir.0, "Current", "", None, "ready").unwrap();
    for number in 0..1_005 {
        let mut previous = record(&format!("{number:04}"));
        previous.response_hash = Some("before".into());
        previous.content_hash = Some("stable".into());
        previous.content_hash_context = Some("html-text-v1".into());
        old.try_upsert(previous.clone()).unwrap();
        previous.response_hash = Some("after".into());
        new.try_upsert(previous).unwrap();
    }
    let mut changed = record("zz-meaningful");
    old.try_upsert(changed.clone()).unwrap();
    changed.meta_description = Some("Edited description".into());
    new.try_upsert(changed).unwrap();
    for include_response_only in [false, true] {
        let result = sessions::compare_sessions(
            &index,
            &baseline.id,
            &current.id,
            None,
            include_response_only,
        )
        .unwrap();
        assert_eq!(
            (result.changed, result.response_only, result.hash_changed),
            (1, 1_005, 1_005)
        );
        assert_eq!(
            result.rows.len(),
            if include_response_only { 1_000 } else { 1 }
        );
        assert!(
            result
                .rows
                .iter()
                .any(|row| row.url.ends_with("/zz-meaningful"))
        );
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(compare_records(
                &old.try_records().unwrap(),
                &new.try_records().unwrap(),
                include_response_only
            ))
            .unwrap(),
        );
    }
}

#[test]
fn sql_comparison_caps_details_but_counts_every_change() {
    let dir = TestDirectory::new();
    let index = sessions::index_connection(&dir.0).unwrap();
    let (baseline, _) =
        sessions::create_session(&index, &dir.0, "Baseline", "", None, "ready").unwrap();
    let (current, new) =
        sessions::create_session(&index, &dir.0, "Current", "", None, "ready").unwrap();
    for number in 0..1_005 {
        new.try_upsert(record(&format!("{number:04}"))).unwrap();
    }
    let result =
        sessions::compare_sessions(&index, &baseline.id, &current.id, None, false).unwrap();
    assert_eq!(result.added, 1_005);
    assert_eq!(result.current_records, 1_005);
    assert_eq!(result.rows.len(), 1_000);
    assert_eq!(result.rows[0].url, "https://example.test/0000");
    assert_eq!(result.rows[999].url, "https://example.test/0999");
    let missing = dir.0.join("lost.sqlite3");
    index
        .execute(
            "UPDATE crawl_sessions SET database_path = ?1 WHERE id = ?2",
            params![missing.to_string_lossy().as_ref(), baseline.id],
        )
        .unwrap();
    assert!(
        sessions::compare_sessions(&index, &baseline.id, &current.id, None, false)
            .unwrap_err()
            .contains("unavailable")
    );
    assert!(!missing.exists());
}

#[tokio::test]
async fn active_task_blocks_workspace_replacement_until_cleanup_finishes() {
    let state = idle_state();
    let task = tauri::async_runtime::spawn(std::future::pending::<()>());
    *state.crawl_task.lock().await = Some(task);
    let mut task = state.crawl_task.lock().await;
    assert!(ensure_idle(&state, &task).is_err());
    task.as_ref().unwrap().abort();
    let _ = task.take().unwrap().await;
    assert!(ensure_idle(&state, &task).is_ok());
    state.exit_confirmed.store(true, Ordering::SeqCst);
    assert!(ensure_idle(&state, &task).is_err());
}

#[test]
fn opening_database_paths_restores_known_config_and_rejects_malformed_config_atomically() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let config = CrawlConfig {
        max_urls: 314,
        ..CrawlConfig::default()
    };
    let (first, first_store, _, index) =
        prepare_crawl_session(&state, &dir.0, config, false).unwrap();
    first_store.upsert(record("original"));
    let (second, _, _, _) =
        prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), false).unwrap();
    let opened = activate_database_path(&state, &index, Path::new(&first.database_path)).unwrap();
    assert_eq!(opened.id, first.id);
    assert!(opened.is_current);
    assert_eq!(opened.config.unwrap().max_urls, 314);
    index
        .execute(
            "UPDATE crawl_sessions SET config_json = 'invalid json' WHERE id = ?1",
            [&second.id],
        )
        .unwrap();
    assert!(activate_database_path(&state, &index, Path::new(&second.database_path)).is_err());
    assert!(activate_session(&state, &index, &second.id).is_err());
    assert_eq!(
        state.current_session_id.lock().unwrap().as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(
        state.store.lock().unwrap().query(GridQuery::default()).rows[0].url,
        "https://example.test/original"
    );
}

#[test]
fn configless_legacy_list_sessions_learn_mode_and_counts_when_opened() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let index = sessions::index_connection(&dir.0).unwrap();
    let (session, store) =
        sessions::create_session(&index, &dir.0, "Legacy list", "", None, "unknown").unwrap();
    let mut list_row = record("legacy-list");
    list_row.list_position = Some(0);
    store.try_upsert(list_row).unwrap();
    index
        .execute(
            "UPDATE crawl_sessions SET crawled = NULL, status = 'unavailable' WHERE id = ?1",
            [&session.id],
        )
        .unwrap();
    let opened = activate_session(&state, &index, &session.id).unwrap();
    assert_eq!(opened.mode, CrawlMode::List);
    assert_eq!(opened.start_url, "https://example.test/legacy-list");
    assert_eq!(opened.status, "unknown");
    assert_eq!(opened.crawled, Some(1));
    assert!(opened.config.is_none());
    assert_eq!(
        get_session(&index, &session.id, None).unwrap().mode,
        CrawlMode::List
    );
}

#[test]
fn failed_database_removal_keeps_the_card_and_active_results() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let (session, store, _, index) =
        prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), false).unwrap();
    store.upsert(record("kept"));
    let undeletable_path = dir.0.join("directory-instead-of-database");
    fs::create_dir(&undeletable_path).unwrap();
    index
        .execute(
            "UPDATE crawl_sessions SET database_path = ?1 WHERE id = ?2",
            params![undeletable_path.to_string_lossy().as_ref(), session.id],
        )
        .unwrap();
    assert!(delete_session(&state, &index, &session.id).is_err());
    assert!(get_session(&index, &session.id, None).is_ok());
    assert_eq!(
        state.current_session_id.lock().unwrap().as_deref(),
        Some(session.id.as_str())
    );
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .query(GridQuery::default())
            .total,
        1
    );
}

#[test]
fn deleting_missing_cards_and_inactive_databases_removes_only_the_selected_session() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let (first, store, _, index) =
        prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), false).unwrap();
    store.upsert(record("first"));
    drop(store);
    let (second, _, _, _) =
        prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), false).unwrap();
    delete_session(&state, &index, &first.id).unwrap();
    assert!(!Path::new(&first.database_path).exists());
    assert!(get_session(&index, &first.id, None).is_err());
    assert_eq!(
        state.current_session_id.lock().unwrap().as_deref(),
        Some(second.id.as_str())
    );
    index
        .execute(
            "UPDATE crawl_sessions SET database_path = ?1 WHERE id = ?2",
            params![
                dir.0.join("missing.sqlite3").to_string_lossy().as_ref(),
                second.id
            ],
        )
        .unwrap();
    delete_session(&state, &index, &second.id).unwrap();
    assert!(query_sessions(&index, None).unwrap().is_empty());
    assert!(state.current_session_id.lock().unwrap().is_none());
}

#[test]
fn deleting_the_current_database_releases_its_connection_before_removal() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let (session, store, _, index) =
        prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), false).unwrap();
    store.upsert(record("current"));
    drop(store);
    delete_session(&state, &index, &session.id).unwrap();
    assert!(!Path::new(&session.database_path).exists());
    assert!(get_session(&index, &session.id, None).is_err());
    assert!(state.current_session_id.lock().unwrap().is_none());
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .query(GridQuery::default())
            .total,
        0
    );
}

#[test]
fn queued_only_list_archives_restore_targets_and_can_resume() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let frontier = CrawlFrontierState {
        queued: vec![CrawlFrontierItem {
            url: "https://example.test/queued".into(),
            storage_key: "queued-list-item".into(),
            depth: 0,
            from_sitemap: false,
            list_position: Some(0),
            list_duplicate_index: 0,
        }],
        seen: vec!["queued-list-item".into()],
        crawled: 0,
    };
    let imported = import_archive_into_session(
        &state,
        &dir.0,
        CrawlArchive {
            page_captures: Vec::new(),
            schema_version: 1,
            exported_at_ms: 1,
            records: Vec::new(),
            link_edges: Vec::new(),
            image_assets: Vec::new(),
            page_references: Vec::new(),
            frontier_state: Some(frontier.clone()),
        },
    )
    .unwrap();
    assert_eq!(imported.session.mode, CrawlMode::List);
    assert_eq!(imported.session.start_url, "https://example.test/queued");
    let config = CrawlConfig {
        mode: imported.session.mode,
        start_url: imported.session.start_url,
        ..CrawlConfig::default()
    };
    let (resumed, _, _, index) = prepare_crawl_session(&state, &dir.0, config, true).unwrap();
    assert_eq!(resumed.id, imported.session.id);
    let external_path = dir.0.join("queued-external.sqlite3");
    let external = SqliteStore::open(&external_path).unwrap();
    external.try_save_frontier_state(frontier).unwrap();
    let opened = activate_database_path(&state, &index, &external_path).unwrap();
    assert_eq!(opened.mode, CrawlMode::List);
    assert_eq!(opened.start_url, "https://example.test/queued");
    assert_eq!(opened.crawled, Some(0));
}

#[test]
fn archive_import_late_failure_preserves_active_session_and_library() {
    let dir = TestDirectory::new();
    let state = idle_state();
    let (first, store, _, index) =
        prepare_crawl_session(&state, &dir.0, CrawlConfig::default(), false).unwrap();
    store.upsert(record("original"));
    let input = br#"{"schemaVersion":1,"exportedAtMs":0,"records":[],"linkEdges":[],"imageAssets":[]} false"#;
    assert!(import_archive_reader_into_session(&state, &dir.0, &input[..]).is_err());
    assert_eq!(
        state.current_session_id.lock().unwrap().as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(query_sessions(&index, None).unwrap().len(), 1);
    assert_eq!(store.query(GridQuery::default()).total, 1);
    assert_eq!(
        fs::read_dir(dir.0.join("sessions"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".archive-import-"))
            .count(),
        0
    );
}

#[test]
fn archive_import_publication_failure_removes_new_database() {
    let dir = TestDirectory::new();
    let input =
        br#"{"schemaVersion":1,"exportedAtMs":0,"records":[],"linkEdges":[],"imageAssets":[]}"#;
    let staged = archive_import::stage_archive_reader(&input[..], &dir.0).unwrap();
    let index = sessions::index_connection(&dir.0).unwrap();
    index.execute_batch("CREATE TRIGGER reject_import BEFORE INSERT ON crawl_sessions BEGIN SELECT RAISE(ABORT,'injected index failure'); END;").unwrap();
    assert!(
        sessions::publish_imported_session(&index, &dir.0, &staged.path, "", CrawlMode::Spider, 0)
            .is_err()
    );
    assert_eq!(query_sessions(&index, None).unwrap().len(), 0);
    assert_eq!(fs::read_dir(dir.0.join("sessions")).unwrap().count(), 0);
    assert!(staged.path.is_file());
}
