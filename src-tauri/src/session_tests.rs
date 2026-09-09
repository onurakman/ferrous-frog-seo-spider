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
            schema_version: 1,
            exported_at_ms: 1,
            records: vec![imported],
            link_edges: Vec::new(),
            image_assets: Vec::new(),
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
    let expected = compare_records(&old.try_records().unwrap(), &new.try_records().unwrap());
    // A full CrawlRecord decoder rejects this; comparison needs only scalar columns.
    Connection::open(&baseline.database_path)
        .unwrap()
        .execute(
            "UPDATE crawl_records SET custom_extractions = 'invalid json'",
            [],
        )
        .unwrap();
    activate_session(&state, &index, &current.id).unwrap();
    let result = sessions::compare_sessions(&index, &baseline.id, &current.id, None).unwrap();
    assert_eq!((result.baseline_records, result.current_records), (5, 4));
    assert_eq!((result.added, result.removed, result.changed), (1, 1, 1));
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
    assert!(sessions::compare_sessions(&index, &baseline.id, &baseline.id, None).is_err());
    assert!(
        sessions::compare_sessions(&index, &baseline.id, &current.id, Some(&current.id)).is_err()
    );
    assert!(sessions::compare_sessions(&index, "missing", &current.id, None).is_err());
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
    let result = sessions::compare_sessions(&index, &baseline.id, &current.id, None).unwrap();
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
        sessions::compare_sessions(&index, &baseline.id, &current.id, None)
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
            schema_version: 1,
            exported_at_ms: 1,
            records: Vec::new(),
            link_edges: Vec::new(),
            image_assets: Vec::new(),
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
