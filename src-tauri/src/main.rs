use ferrous_frog_analysis::analyze_records;
use ferrous_frog_crawler_core::{
    CrawlConfig, CrawlControl, CrawlerEvent, RobotsTxtDownloadRequest, RobotsTxtDownloadResult,
    RobotsTxtTestRequest, RobotsTxtTestResult, crawl,
    download_robots_txt as run_robots_txt_download, test_robots_txt as run_robots_txt_test,
};
use ferrous_frog_export::{
    link_edges_to_csv_string, records_to_csv_string, records_to_html_report,
    records_to_sitemap_xml, records_to_xlsx_bytes, redirect_chains_to_csv_string,
};
use ferrous_frog_storage::{
    ActiveStore, AnchorTextResponse, CrawlGraph, CrawlGraphQuery, CrawlStore, GridQuery,
    GridResponse, Issue, LinkEdgeQuery, LinkEdgeResponse,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    store: Mutex<ActiveStore>,
    control: Mutex<Option<CrawlControl>>,
    current_session_id: Mutex<Option<String>>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum StorageMode {
    Memory,
    Database,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrawlSession {
    id: String,
    name: String,
    start_url: String,
    database_path: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    is_current: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionRequest {
    name: String,
    start_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigProfile {
    id: String,
    name: String,
    config: CrawlConfig,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveConfigProfileRequest {
    name: String,
    config: CrawlConfig,
}

#[tauri::command(rename_all = "camelCase")]
async fn start_crawl(
    app: AppHandle,
    state: State<'_, AppState>,
    config: CrawlConfig,
    storage_mode: StorageMode,
    resume: bool,
) -> Result<(), String> {
    let control = CrawlControl::default();
    {
        let mut current = state
            .control
            .lock()
            .map_err(|_| "crawl control lock poisoned".to_string())?;
        if let Some(existing) = current.take() {
            existing.cancel();
        }
        *current = Some(control.clone());
    }

    let store = prepare_store(&app, &state, storage_mode, resume)?;
    if storage_mode == StorageMode::Database {
        touch_current_session(&app, &state, &config.start_url)?;
    }
    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        let event_app = app_for_task.clone();
        let emit = move |event: CrawlerEvent| {
            let _ = event_app.emit("crawl-event", event);
        };

        if let Err(error) = crawl(config, store, control, emit).await {
            let _ = app_for_task.emit("crawl-event", CrawlerEvent::error(error.to_string()));
        }
    });

    Ok(())
}

#[tauri::command]
fn pause_crawl(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(control) = state
        .control
        .lock()
        .map_err(|_| "crawl control lock poisoned".to_string())?
        .as_ref()
    {
        control.pause();
    }
    Ok(())
}

#[tauri::command]
fn resume_crawl(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(control) = state
        .control
        .lock()
        .map_err(|_| "crawl control lock poisoned".to_string())?
        .as_ref()
    {
        control.resume();
    }
    Ok(())
}

#[tauri::command]
fn stop_crawl(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(control) = state
        .control
        .lock()
        .map_err(|_| "crawl control lock poisoned".to_string())?
        .as_ref()
    {
        control.cancel();
    }
    Ok(())
}

#[tauri::command]
fn test_robots_txt(request: RobotsTxtTestRequest) -> Result<RobotsTxtTestResult, String> {
    run_robots_txt_test(request).map_err(|error| error.to_string())
}

#[tauri::command]
async fn download_robots_txt(
    request: RobotsTxtDownloadRequest,
) -> Result<RobotsTxtDownloadResult, String> {
    run_robots_txt_download(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_rows(state: State<'_, AppState>, query: GridQuery) -> GridResponse {
    state
        .store
        .lock()
        .expect("store lock poisoned")
        .query(query)
}

#[tauri::command]
fn get_issues(state: State<'_, AppState>) -> Vec<Issue> {
    let records = state.store.lock().expect("store lock poisoned").records();
    analyze_records(&records)
}

#[tauri::command]
fn get_link_edges(state: State<'_, AppState>, query: LinkEdgeQuery) -> LinkEdgeResponse {
    state
        .store
        .lock()
        .expect("store lock poisoned")
        .link_edges(query)
}

#[tauri::command]
fn get_anchor_texts(state: State<'_, AppState>, query: LinkEdgeQuery) -> AnchorTextResponse {
    state
        .store
        .lock()
        .expect("store lock poisoned")
        .anchor_texts(query)
}

#[tauri::command]
fn get_crawl_graph(state: State<'_, AppState>, query: CrawlGraphQuery) -> CrawlGraph {
    state
        .store
        .lock()
        .expect("store lock poisoned")
        .crawl_graph(query)
}

#[tauri::command]
fn list_crawl_sessions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<CrawlSession>, String> {
    let current_session_id = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?
        .clone();
    let conn = session_index_connection(&app)?;
    query_sessions(&conn, current_session_id.as_deref())
}

#[tauri::command]
fn create_crawl_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CreateSessionRequest,
) -> Result<CrawlSession, String> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err("session name is required".to_string());
    }
    let id = format!("session-{}", now_ms());
    let database_path = sessions_dir(&app)?.join(format!("{id}.sqlite3"));
    let database_path_string = database_path.to_string_lossy().into_owned();
    let now = now_ms();
    let conn = session_index_connection(&app)?;
    conn.execute(
        "INSERT INTO crawl_sessions (
            id,
            name,
            start_url,
            database_path,
            created_at_ms,
            updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            &id,
            name,
            request.start_url.trim(),
            &database_path_string,
            now,
            now
        ],
    )
    .map_err(|error| error.to_string())?;
    let session = get_session(&conn, &id, Some(&id))?;
    *state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())? = Some(id);
    Ok(session)
}

#[tauri::command]
fn open_crawl_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CrawlSession, String> {
    let conn = session_index_connection(&app)?;
    let session = get_session(&conn, &session_id, Some(&session_id))?;
    let store = ActiveStore::sqlite(&session.database_path)
        .map_err(|error| format!("failed to open crawl session: {error}"))?;
    *state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())? = store;
    *state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())? = Some(session_id);
    Ok(session)
}

#[tauri::command]
fn delete_crawl_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let conn = session_index_connection(&app)?;
    let session = get_session(&conn, &session_id, None)?;
    conn.execute("DELETE FROM crawl_sessions WHERE id = ?1", [&session_id])
        .map_err(|error| error.to_string())?;
    let _ = fs::remove_file(&session.database_path);
    let mut current = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    if current.as_deref() == Some(session_id.as_str()) {
        *current = None;
        *state
            .store
            .lock()
            .map_err(|_| "store lock poisoned".to_string())? = ActiveStore::memory();
    }
    Ok(())
}

#[tauri::command]
fn list_config_profiles(app: AppHandle) -> Result<Vec<ConfigProfile>, String> {
    let conn = session_index_connection(&app)?;
    query_config_profiles(&conn)
}

#[tauri::command]
fn save_config_profile(
    app: AppHandle,
    request: SaveConfigProfileRequest,
) -> Result<ConfigProfile, String> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err("profile name is required".to_string());
    }
    let id = format!("profile-{}", now_ms());
    let now = now_ms();
    let config_json = serde_json::to_string(&request.config).map_err(|error| error.to_string())?;
    let conn = session_index_connection(&app)?;
    conn.execute(
        "INSERT INTO config_profiles (
            id,
            name,
            config_json,
            created_at_ms,
            updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![&id, name, config_json, now, now],
    )
    .map_err(|error| error.to_string())?;
    get_config_profile(&conn, &id)
}

#[tauri::command]
fn load_config_profile(app: AppHandle, profile_id: String) -> Result<ConfigProfile, String> {
    let conn = session_index_connection(&app)?;
    get_config_profile(&conn, &profile_id)
}

#[tauri::command]
fn delete_config_profile(app: AppHandle, profile_id: String) -> Result<(), String> {
    let conn = session_index_connection(&app)?;
    conn.execute("DELETE FROM config_profiles WHERE id = ?1", [&profile_id])
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn export_csv(state: State<'_, AppState>, query: GridQuery) -> Result<String, String> {
    let records = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .query(query)
        .rows;
    records_to_csv_string(&records).map_err(|error| error.to_string())
}

#[tauri::command]
fn export_xlsx(state: State<'_, AppState>, query: GridQuery) -> Result<Vec<u8>, String> {
    let records = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .query(query)
        .rows;
    records_to_xlsx_bytes(&records).map_err(|error| error.to_string())
}

#[tauri::command]
fn export_sitemap(state: State<'_, AppState>, query: GridQuery) -> Result<String, String> {
    let records = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .query(query)
        .rows;
    Ok(records_to_sitemap_xml(&records))
}

#[tauri::command]
fn export_link_edges_csv(state: State<'_, AppState>) -> Result<String, String> {
    let edges = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .link_edges(LinkEdgeQuery {
            offset: 0,
            limit: 1_000_000,
            ..LinkEdgeQuery::default()
        })
        .edges;
    link_edges_to_csv_string(&edges).map_err(|error| error.to_string())
}

#[tauri::command]
fn export_redirect_chains_csv(state: State<'_, AppState>) -> Result<String, String> {
    let records = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .records();
    redirect_chains_to_csv_string(&records).map_err(|error| error.to_string())
}

#[tauri::command]
fn export_html_report(state: State<'_, AppState>) -> Result<String, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let records = store.records();
    let edges = store
        .link_edges(LinkEdgeQuery {
            offset: 0,
            limit: 1_000_000,
            ..LinkEdgeQuery::default()
        })
        .edges;
    records_to_html_report(&records, &edges).map_err(|error| error.to_string())
}

fn prepare_store(
    app: &AppHandle,
    state: &State<'_, AppState>,
    storage_mode: StorageMode,
    resume: bool,
) -> Result<ActiveStore, String> {
    let store = match storage_mode {
        StorageMode::Memory => ActiveStore::memory(),
        StorageMode::Database => ActiveStore::sqlite(active_database_path(app, state)?)
            .map_err(|error| format!("failed to open database store: {error}"))?,
    };

    if !resume {
        store.clear();
    }

    *state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())? = store.clone();

    Ok(store)
}

fn active_database_path(app: &AppHandle, state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let current_session_id = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?
        .clone();
    if let Some(session_id) = current_session_id {
        let conn = session_index_connection(app)?;
        let session = get_session(&conn, &session_id, Some(&session_id))?;
        return Ok(PathBuf::from(session.database_path));
    }
    database_path(app)
}

fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create app data directory: {error}"))?;
    Ok(dir.join("ferrous-frog-current.sqlite3"))
}

fn sessions_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?
        .join("sessions");
    fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create sessions directory: {error}"))?;
    Ok(dir)
}

fn session_index_connection(app: &AppHandle) -> Result<Connection, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create app data directory: {error}"))?;
    let conn = Connection::open(dir.join("ferrous-frog-sessions.sqlite3"))
        .map_err(|error| error.to_string())?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS crawl_sessions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            start_url TEXT NOT NULL,
            database_path TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_crawl_sessions_updated_at
            ON crawl_sessions(updated_at_ms DESC);

        CREATE TABLE IF NOT EXISTS config_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            config_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_config_profiles_updated_at
            ON config_profiles(updated_at_ms DESC);
        ",
    )
    .map_err(|error| error.to_string())?;
    Ok(conn)
}

fn query_sessions(
    conn: &Connection,
    current_session_id: Option<&str>,
) -> Result<Vec<CrawlSession>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, start_url, database_path, created_at_ms, updated_at_ms
             FROM crawl_sessions
             ORDER BY updated_at_ms DESC, name ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| session_from_row(row, current_session_id))
        .map_err(|error| error.to_string())?;
    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row.map_err(|error| error.to_string())?);
    }
    Ok(sessions)
}

fn get_session(
    conn: &Connection,
    session_id: &str,
    current_session_id: Option<&str>,
) -> Result<CrawlSession, String> {
    conn.query_row(
        "SELECT id, name, start_url, database_path, created_at_ms, updated_at_ms
         FROM crawl_sessions
         WHERE id = ?1",
        [session_id],
        |row| session_from_row(row, current_session_id),
    )
    .optional()
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "crawl session not found".to_string())
}

fn session_from_row(
    row: &rusqlite::Row<'_>,
    current_session_id: Option<&str>,
) -> rusqlite::Result<CrawlSession> {
    let id: String = row.get(0)?;
    Ok(CrawlSession {
        is_current: current_session_id == Some(id.as_str()),
        id,
        name: row.get(1)?,
        start_url: row.get(2)?,
        database_path: row.get(3)?,
        created_at_ms: row.get(4)?,
        updated_at_ms: row.get(5)?,
    })
}

fn query_config_profiles(conn: &Connection) -> Result<Vec<ConfigProfile>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, config_json, created_at_ms, updated_at_ms
             FROM config_profiles
             ORDER BY updated_at_ms DESC, name ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], config_profile_from_row)
        .map_err(|error| error.to_string())?;
    let mut profiles = Vec::new();
    for row in rows {
        profiles.push(row.map_err(|error| error.to_string())?);
    }
    Ok(profiles)
}

fn get_config_profile(conn: &Connection, profile_id: &str) -> Result<ConfigProfile, String> {
    conn.query_row(
        "SELECT id, name, config_json, created_at_ms, updated_at_ms
         FROM config_profiles
         WHERE id = ?1",
        [profile_id],
        config_profile_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "config profile not found".to_string())
}

fn config_profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConfigProfile> {
    let config_json: String = row.get(2)?;
    let config = serde_json::from_str(&config_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(ConfigProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        config,
        created_at_ms: row.get(3)?,
        updated_at_ms: row.get(4)?,
    })
}

fn touch_current_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
    start_url: &str,
) -> Result<(), String> {
    let Some(session_id) = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?
        .clone()
    else {
        return Ok(());
    };
    let conn = session_index_connection(app)?;
    conn.execute(
        "UPDATE crawl_sessions
         SET start_url = ?1, updated_at_ms = ?2
         WHERE id = ?3",
        params![start_url, now_ms(), session_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            store: Mutex::new(ActiveStore::memory()),
            control: Mutex::new(None),
            current_session_id: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            start_crawl,
            pause_crawl,
            resume_crawl,
            stop_crawl,
            test_robots_txt,
            download_robots_txt,
            get_rows,
            get_issues,
            get_link_edges,
            get_anchor_texts,
            get_crawl_graph,
            list_crawl_sessions,
            create_crawl_session,
            open_crawl_session,
            delete_crawl_session,
            list_config_profiles,
            save_config_profile,
            load_config_profile,
            delete_config_profile,
            export_csv,
            export_xlsx,
            export_sitemap,
            export_link_edges_csv,
            export_redirect_chains_csv,
            export_html_report
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Ferrous Frog");
}
