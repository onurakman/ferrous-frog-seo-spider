#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ferrous_frog_analysis::analyze_records;
use ferrous_frog_crawler_core::{
    CrawlConfig, CrawlControl, CrawlerEvent, RobotsTxtBatchTestRequest, RobotsTxtBatchTestResult,
    RobotsTxtDownloadRequest, RobotsTxtDownloadResult, RobotsTxtTestRequest, RobotsTxtTestResult,
    crawl, download_robots_txt as run_robots_txt_download, test_robots_txt as run_robots_txt_test,
    test_robots_txt_batch as run_robots_txt_batch_test,
};
use ferrous_frog_export::{
    graph_nodes_to_csv_string, link_edges_to_csv_string, records_to_csv_string,
    records_to_html_report, records_to_sitemap_xml, records_to_xlsx_bytes,
    redirect_chains_to_csv_string, sitemap_validation_to_csv_string,
};
use ferrous_frog_integrations::{
    DateRange, MetricRequest, SearchConsoleConfig, SearchConsoleProvider, UrlMetricProvider,
};
use ferrous_frog_storage::{
    ActiveStore, AnchorTextResponse, CrawlFrontierState, CrawlGraph, CrawlGraphQuery,
    CrawlPathQuery, CrawlPathResponse, CrawlRecord, CrawlStore, GridQuery, GridResponse,
    ImageAsset, ImageAssetQuery, ImageAssetResponse, Issue, LinkEdge, LinkEdgeQuery,
    LinkEdgeResponse, SearchConsoleMetricRow, SitemapValidationQuery, SitemapValidationResponse,
    is_broken_record, is_no_response_record, summarize,
};
use keyring::{Entry, Error as KeyringError};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

struct AppState {
    store: Mutex<ActiveStore>,
    control: Mutex<Option<CrawlControl>>,
    crawl_task: tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    current_session_id: Mutex<Option<String>>,
    frontend_ready: AtomicBool,
    exit_confirmed: AtomicBool,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportFileRequest {
    kind: ExportFileKind,
    query: Option<GridQuery>,
    graph_query: Option<CrawlGraphQuery>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ExportFileKind {
    Csv,
    Xlsx,
    Sitemap,
    LinkEdgesCsv,
    RedirectChainsCsv,
    HtmlReport,
    GraphJson,
    GraphNodesCsv,
    GraphEdgesCsv,
    SitemapValidationCsv,
    CrawlArchive,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportFileResult {
    path: String,
    row_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportCrawlArchiveRequest {
    path: String,
    storage_mode: StorageMode,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrawlArchiveImportResult {
    records: usize,
    link_edges: usize,
    image_assets: usize,
    frontier_items: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompareCrawlArchiveRequest {
    path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrawlComparisonResponse {
    baseline_records: usize,
    current_records: usize,
    added: usize,
    removed: usize,
    changed: usize,
    status_changed: usize,
    title_changed: usize,
    meta_description_changed: usize,
    indexability_changed: usize,
    hash_changed: usize,
    rows: Vec<CrawlComparisonRow>,
    metric_deltas: Vec<ComparisonMetricDelta>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrawlComparisonRow {
    url: String,
    change: String,
    previous_status_code: Option<u16>,
    current_status_code: Option<u16>,
    previous_title: Option<String>,
    current_title: Option<String>,
    previous_indexability: Option<String>,
    current_indexability: Option<String>,
    previous_response_hash: Option<String>,
    current_response_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonMetricDelta {
    label: String,
    previous: usize,
    current: usize,
    delta: isize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveSearchConsoleCredentialsRequest {
    site_url: String,
    access_token: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchConsoleCredentialStatus {
    site_url: Option<String>,
    token_saved: bool,
    keyring_available: bool,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestSearchConsoleCredentialsRequest {
    start_date: String,
    end_date: String,
    row_limit: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchConsoleTestResult {
    rows: usize,
    clicks: f64,
    impressions: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeSearchConsoleMetricsRequest {
    start_date: String,
    end_date: String,
    row_limit: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchConsoleMergeResult {
    fetched_rows: usize,
    matched_rows: usize,
    clicks: f64,
    impressions: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DatabaseLocation {
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrawlRecoveryState {
    recoverable: bool,
    queued: usize,
    seen: usize,
    crawled: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrawlArchive {
    schema_version: u32,
    exported_at_ms: i64,
    records: Vec<CrawlRecord>,
    link_edges: Vec<LinkEdge>,
    image_assets: Vec<ImageAsset>,
    frontier_state: Option<CrawlFrontierState>,
}

const CRAWL_ARCHIVE_SCHEMA_VERSION: u32 = 1;
const EXPORT_STREAM_PAGE_SIZE: usize = 10_000;
const URL_TREE_RECORD_LIMIT: usize = 10_000;
const KEYRING_SERVICE: &str = "ferrous-frog-seo-spider";
const GSC_KEYRING_ACCOUNT: &str = "google-search-console";
const GSC_SITE_URL_SETTING: &str = "google_search_console_site_url";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UrlTreeResponse {
    nodes: Vec<UrlTreeNode>,
    total_urls: usize,
    rendered_urls: usize,
    capped: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UrlTreeNode {
    id: String,
    label: String,
    path: String,
    url: Option<String>,
    record: Option<CrawlRecord>,
    depth: usize,
    total: usize,
    success: usize,
    redirects: usize,
    client_errors: usize,
    server_errors: usize,
    no_response: usize,
    broken: usize,
    children: Vec<UrlTreeNode>,
}

impl UrlTreeNode {
    fn new(id: String, label: String, path: String, depth: usize) -> Self {
        Self {
            id,
            label,
            path,
            url: None,
            record: None,
            depth,
            total: 0,
            success: 0,
            redirects: 0,
            client_errors: 0,
            server_errors: 0,
            no_response: 0,
            broken: 0,
            children: Vec::new(),
        }
    }
}

#[tauri::command(rename_all = "camelCase")]
async fn start_crawl(
    app: AppHandle,
    state: State<'_, AppState>,
    mut config: CrawlConfig,
    storage_mode: StorageMode,
    resume: bool,
) -> Result<(), String> {
    let mut current_task = state.crawl_task.lock().await;
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("the application is closing".to_string());
    }
    stop_active_crawl(&state, &mut current_task).await?;
    let control = CrawlControl::default();
    {
        let mut current = state
            .control
            .lock()
            .map_err(|_| "crawl control lock poisoned".to_string())?;
        *current = Some(control.clone());
    }

    let store = prepare_store(&app, &state, storage_mode, resume)?;
    if storage_mode == StorageMode::Database {
        touch_current_session(&app, &state, &config.start_url)?;
    }
    config.resume_from_state = storage_mode == StorageMode::Database && resume;
    let app_for_task = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let event_app = app_for_task.clone();
        let emit = move |event: CrawlerEvent| {
            let _ = event_app.emit("crawl-event", event);
        };

        if let Err(error) = crawl(config, store, control, emit).await {
            let mut event = CrawlerEvent::error(error.to_string());
            event.kind = "failed".to_string();
            let _ = app_for_task.emit("crawl-event", event);
        }
    });
    *current_task = Some(task);

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
async fn stop_crawl(state: State<'_, AppState>) -> Result<(), String> {
    let mut task = state.crawl_task.lock().await;
    stop_active_crawl(&state, &mut task).await
}

async fn stop_active_crawl(
    state: &AppState,
    task: &mut Option<tauri::async_runtime::JoinHandle<()>>,
) -> Result<(), String> {
    if let Some(control) = state
        .control
        .lock()
        .map_err(|_| "crawl control lock poisoned".to_string())?
        .take()
    {
        control.cancel();
    }
    if let Some(task) = task.take() {
        task.await
            .map_err(|error| format!("failed to finish crawl cleanup: {error}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn quit_app(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut task = state.crawl_task.lock().await;
    stop_active_crawl(&state, &mut task).await?;
    state.exit_confirmed.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?;
    main.show().map_err(|error| error.to_string())?;
    let _ = main.unminimize();
    let _ = main.set_focus();
    if let Some(splash) = app.get_webview_window("splashscreen") {
        splash.destroy().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn complete_startup(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.frontend_ready.store(true, Ordering::SeqCst);
    show_main_window(&app)
}

fn request_quit(app: &AppHandle) {
    if let Err(error) = show_main_window(app).and_then(|()| {
        app.emit_to("main", "quit-requested", ())
            .map_err(|error| error.to_string())
    }) {
        eprintln!("failed to request quit confirmation: {error}");
    }
}

#[tauri::command]
fn test_robots_txt(request: RobotsTxtTestRequest) -> Result<RobotsTxtTestResult, String> {
    run_robots_txt_test(request).map_err(|error| error.to_string())
}

#[tauri::command]
fn test_robots_txt_batch(
    request: RobotsTxtBatchTestRequest,
) -> Result<RobotsTxtBatchTestResult, String> {
    run_robots_txt_batch_test(request).map_err(|error| error.to_string())
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
async fn get_rows(state: State<'_, AppState>, query: GridQuery) -> Result<GridResponse, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .clone();
    tauri::async_runtime::spawn_blocking(move || store.query(query))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_url_tree(state: State<'_, AppState>, mut query: GridQuery) -> UrlTreeResponse {
    query.offset = 0;
    query.limit = URL_TREE_RECORD_LIMIT;
    let response = state
        .store
        .lock()
        .expect("store lock poisoned")
        .query(query);

    let mut nodes = Vec::new();
    for record in &response.rows {
        let segments = url_tree_segments(record);
        insert_url_tree_record(&mut nodes, &segments, record, 0, "");
    }
    sort_url_tree_nodes(&mut nodes);

    UrlTreeResponse {
        nodes,
        total_urls: response.total,
        rendered_urls: response.rows.len(),
        capped: response.total > response.rows.len(),
    }
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
fn get_image_assets(state: State<'_, AppState>, query: ImageAssetQuery) -> ImageAssetResponse {
    state
        .store
        .lock()
        .expect("store lock poisoned")
        .image_assets(query)
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
fn get_sitemap_validation(
    state: State<'_, AppState>,
    query: SitemapValidationQuery,
) -> SitemapValidationResponse {
    state
        .store
        .lock()
        .expect("store lock poisoned")
        .sitemap_validation(query)
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
fn get_crawl_path(state: State<'_, AppState>, query: CrawlPathQuery) -> CrawlPathResponse {
    state
        .store
        .lock()
        .expect("store lock poisoned")
        .crawl_path(query)
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
fn get_database_location(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DatabaseLocation, String> {
    Ok(DatabaseLocation {
        path: active_database_path(&app, &state)?
            .to_string_lossy()
            .into_owned(),
    })
}

#[tauri::command(rename_all = "camelCase")]
fn open_database_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<DatabaseLocation, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("database path is required".to_string());
    }
    let database_path = PathBuf::from(trimmed);
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create database directory: {error}"))?;
    }
    let store = ActiveStore::sqlite(&database_path)
        .map_err(|error| format!("failed to open database path: {error}"))?;
    *state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())? = store;
    *state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())? = None;
    Ok(DatabaseLocation {
        path: database_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
fn get_recovery_state(state: State<'_, AppState>) -> CrawlRecoveryState {
    let frontier_state = state
        .store
        .lock()
        .expect("store lock poisoned")
        .load_frontier_state();
    match frontier_state {
        Some(frontier) if !frontier.queued.is_empty() => CrawlRecoveryState {
            recoverable: true,
            queued: frontier.queued.len(),
            seen: frontier.seen.len(),
            crawled: frontier.crawled,
        },
        Some(frontier) => CrawlRecoveryState {
            recoverable: false,
            queued: 0,
            seen: frontier.seen.len(),
            crawled: frontier.crawled,
        },
        None => CrawlRecoveryState {
            recoverable: false,
            queued: 0,
            seen: 0,
            crawled: 0,
        },
    }
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
fn get_search_console_credential_status(
    app: AppHandle,
) -> Result<SearchConsoleCredentialStatus, String> {
    search_console_credential_status(&app)
}

#[tauri::command]
fn save_search_console_credentials(
    app: AppHandle,
    request: SaveSearchConsoleCredentialsRequest,
) -> Result<SearchConsoleCredentialStatus, String> {
    let site_url = request.site_url.trim();
    if site_url.is_empty() {
        return Err("Google Search Console site URL is required".to_string());
    }
    set_integration_setting(&app, GSC_SITE_URL_SETTING, site_url)?;

    if let Some(access_token) = request.access_token.as_ref().map(|token| token.trim())
        && !access_token.is_empty()
    {
        search_console_keyring_entry()?
            .set_password(access_token)
            .map_err(|error| format!("failed to save Search Console token: {error}"))?;
    }

    search_console_credential_status(&app)
}

#[tauri::command]
fn clear_search_console_credentials(
    app: AppHandle,
) -> Result<SearchConsoleCredentialStatus, String> {
    delete_integration_setting(&app, GSC_SITE_URL_SETTING)?;
    match search_console_keyring_entry()?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => search_console_credential_status(&app),
        Err(error) => Err(format!("failed to clear Search Console token: {error}")),
    }
}

#[tauri::command]
async fn test_search_console_credentials(
    app: AppHandle,
    request: TestSearchConsoleCredentialsRequest,
) -> Result<SearchConsoleTestResult, String> {
    let site_url = get_integration_setting(&app, GSC_SITE_URL_SETTING)?
        .ok_or_else(|| "Google Search Console site URL is not configured".to_string())?;
    let access_token = read_search_console_access_token()?
        .ok_or_else(|| "Google Search Console access token is not configured".to_string())?;
    let provider = SearchConsoleProvider::new(SearchConsoleConfig {
        site_url,
        access_token,
        row_limit: request.row_limit.clamp(1, 25_000),
    });
    let response = provider
        .fetch_metrics(MetricRequest {
            urls: Vec::new(),
            date_range: Some(DateRange {
                start_date: request.start_date,
                end_date: request.end_date,
            }),
        })
        .await
        .map_err(|error| error.to_string())?;
    let mut clicks = 0.0;
    let mut impressions = 0.0;
    for row in &response.rows {
        if let Some(metrics) = &row.search_console {
            clicks += metrics.clicks;
            impressions += metrics.impressions;
        }
    }
    Ok(SearchConsoleTestResult {
        rows: response.rows.len(),
        clicks,
        impressions,
    })
}

#[tauri::command]
async fn merge_search_console_metrics(
    app: AppHandle,
    state: State<'_, AppState>,
    request: MergeSearchConsoleMetricsRequest,
) -> Result<SearchConsoleMergeResult, String> {
    let site_url = get_integration_setting(&app, GSC_SITE_URL_SETTING)?
        .ok_or_else(|| "Google Search Console site URL is not configured".to_string())?;
    let access_token = read_search_console_access_token()?
        .ok_or_else(|| "Google Search Console access token is not configured".to_string())?;
    let provider = SearchConsoleProvider::new(SearchConsoleConfig {
        site_url,
        access_token,
        row_limit: request.row_limit,
    });
    let response = provider
        .fetch_metrics(MetricRequest {
            urls: Vec::new(),
            date_range: Some(DateRange {
                start_date: request.start_date,
                end_date: request.end_date,
            }),
        })
        .await
        .map_err(|error| error.to_string())?;

    let mut clicks = 0.0;
    let mut impressions = 0.0;
    let metric_rows = response
        .rows
        .into_iter()
        .filter_map(|row| {
            let metric = row.search_console?;
            clicks += metric.clicks;
            impressions += metric.impressions;
            Some(SearchConsoleMetricRow {
                url: row.url,
                clicks: metric.clicks,
                impressions: metric.impressions,
                ctr: metric.ctr,
                average_position: metric.average_position,
            })
        })
        .collect::<Vec<_>>();
    let fetched_rows = metric_rows.len();
    let matched_rows = {
        let store = state.store.lock().expect("store lock poisoned");
        store.merge_search_console_metrics(metric_rows)
    };

    Ok(SearchConsoleMergeResult {
        fetched_rows,
        matched_rows,
        clicks,
        impressions,
    })
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

fn stream_export_file(
    app: &AppHandle,
    state: &State<'_, AppState>,
    request: &ExportFileRequest,
    timestamp: i64,
) -> Result<Option<ExportFileResult>, String> {
    let filename = match &request.kind {
        ExportFileKind::Csv => format!("ferrous-frog-export-{timestamp}.csv"),
        ExportFileKind::Sitemap => format!("sitemap-{timestamp}.xml"),
        ExportFileKind::LinkEdgesCsv => format!("ferrous-frog-link-edges-{timestamp}.csv"),
        ExportFileKind::RedirectChainsCsv => {
            format!("ferrous-frog-redirect-chains-{timestamp}.csv")
        }
        ExportFileKind::SitemapValidationCsv => {
            format!("ferrous-frog-sitemap-validation-{timestamp}.csv")
        }
        _ => return Ok(None),
    };
    let path = export_path(app, &filename)?;
    let mut file = fs::File::create(&path)
        .map_err(|error| format!("failed to create export file: {error}"))?;
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;

    let row_count = match &request.kind {
        ExportFileKind::Csv => write_grid_csv_stream(
            &store,
            request.query.clone().unwrap_or_else(export_grid_query),
            &mut file,
        )?,
        ExportFileKind::Sitemap => write_sitemap_xml_stream(
            &store,
            request.query.clone().unwrap_or_else(export_grid_query),
            &mut file,
        )?,
        ExportFileKind::LinkEdgesCsv => write_link_edges_csv_stream(&store, &mut file)?,
        ExportFileKind::RedirectChainsCsv => write_redirect_chains_csv_stream(&store, &mut file)?,
        ExportFileKind::SitemapValidationCsv => {
            write_sitemap_validation_csv_stream(&store, &mut file)?
        }
        _ => unreachable!("non-streaming export kind returned earlier"),
    };

    Ok(Some(ExportFileResult {
        path: path.to_string_lossy().into_owned(),
        row_count,
    }))
}

fn write_grid_csv_stream(
    store: &ActiveStore,
    mut query: GridQuery,
    file: &mut fs::File,
) -> Result<usize, String> {
    query.offset = 0;
    query.limit = EXPORT_STREAM_PAGE_SIZE;
    let mut row_count = 0;
    let mut wrote_header = false;

    loop {
        let response = store.query(query.clone());
        let chunk_len = response.rows.len();
        let csv = records_to_csv_string(&response.rows).map_err(|error| error.to_string())?;
        write_csv_chunk(file, &csv, !wrote_header)?;
        wrote_header = true;
        row_count += chunk_len;
        if chunk_len == 0 || chunk_len < query.limit || row_count >= response.total {
            break;
        }
        query.offset += chunk_len;
    }

    Ok(row_count)
}

fn write_link_edges_csv_stream(store: &ActiveStore, file: &mut fs::File) -> Result<usize, String> {
    let mut query = LinkEdgeQuery {
        offset: 0,
        limit: EXPORT_STREAM_PAGE_SIZE,
        ..LinkEdgeQuery::default()
    };
    let mut row_count = 0;
    let mut wrote_header = false;

    loop {
        let response = store.link_edges(query.clone());
        let chunk_len = response.edges.len();
        let csv = link_edges_to_csv_string(&response.edges).map_err(|error| error.to_string())?;
        write_csv_chunk(file, &csv, !wrote_header)?;
        wrote_header = true;
        row_count += chunk_len;
        if chunk_len == 0 || chunk_len < query.limit || row_count >= response.total {
            break;
        }
        query.offset += chunk_len;
    }

    Ok(row_count)
}

fn write_redirect_chains_csv_stream(
    store: &ActiveStore,
    file: &mut fs::File,
) -> Result<usize, String> {
    let mut query = GridQuery {
        offset: 0,
        limit: EXPORT_STREAM_PAGE_SIZE,
        ..GridQuery::default()
    };
    let mut row_count = 0;
    let mut wrote_header = false;

    loop {
        let response = store.query(query.clone());
        let chunk_len = response.rows.len();
        let csv =
            redirect_chains_to_csv_string(&response.rows).map_err(|error| error.to_string())?;
        write_csv_chunk(file, &csv, !wrote_header)?;
        wrote_header = true;
        row_count += response
            .rows
            .iter()
            .map(|record| record.redirect_chain.len())
            .sum::<usize>();
        if chunk_len == 0 || chunk_len < query.limit || query.offset + chunk_len >= response.total {
            break;
        }
        query.offset += chunk_len;
    }

    Ok(row_count)
}

fn write_sitemap_validation_csv_stream(
    store: &ActiveStore,
    file: &mut fs::File,
) -> Result<usize, String> {
    let mut query = SitemapValidationQuery {
        offset: 0,
        limit: EXPORT_STREAM_PAGE_SIZE,
        ..SitemapValidationQuery::default()
    };
    let mut row_count = 0;
    let mut wrote_header = false;

    loop {
        let response = store.sitemap_validation(query.clone());
        let chunk_len = response.rows.len();
        let csv =
            sitemap_validation_to_csv_string(&response.rows).map_err(|error| error.to_string())?;
        write_csv_chunk(file, &csv, !wrote_header)?;
        wrote_header = true;
        row_count += chunk_len;
        if chunk_len == 0 || chunk_len < query.limit || row_count >= response.total {
            break;
        }
        query.offset += chunk_len;
    }

    Ok(row_count)
}

fn write_sitemap_xml_stream(
    store: &ActiveStore,
    mut query: GridQuery,
    file: &mut fs::File,
) -> Result<usize, String> {
    query.offset = 0;
    query.limit = EXPORT_STREAM_PAGE_SIZE;
    file.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
"#,
    )
    .map_err(|error| format!("failed to write sitemap export: {error}"))?;

    let mut row_count = 0;
    loop {
        let response = store.query(query.clone());
        let chunk_len = response.rows.len();
        for record in response
            .rows
            .iter()
            .filter(|record| sitemap_record_eligible(record))
        {
            file.write_all(b"  <url>\n    <loc>")
                .and_then(|_| file.write_all(xml_escape(&record.final_url).as_bytes()))
                .and_then(|_| file.write_all(b"</loc>\n  </url>\n"))
                .map_err(|error| format!("failed to write sitemap export: {error}"))?;
            row_count += 1;
        }
        if chunk_len == 0 || chunk_len < query.limit || query.offset + chunk_len >= response.total {
            break;
        }
        query.offset += chunk_len;
    }

    file.write_all(b"</urlset>\n")
        .map_err(|error| format!("failed to write sitemap export: {error}"))?;
    Ok(row_count)
}

fn write_csv_chunk(file: &mut fs::File, csv: &str, include_header: bool) -> Result<(), String> {
    if include_header {
        return file
            .write_all(csv.as_bytes())
            .map_err(|error| format!("failed to write export file: {error}"));
    }

    if let Some(index) = csv.find('\n') {
        file.write_all(&csv.as_bytes()[index + 1..])
            .map_err(|error| format!("failed to write export file: {error}"))?;
    }
    Ok(())
}

fn sitemap_record_eligible(record: &CrawlRecord) -> bool {
    matches!(record.status_code, Some(code) if (200..300).contains(&code))
        && record.indexability == "Indexable"
        && (record.final_url.starts_with("http://") || record.final_url.starts_with("https://"))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[tauri::command(rename_all = "camelCase")]
fn export_file(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ExportFileRequest,
) -> Result<ExportFileResult, String> {
    let timestamp = now_ms();
    if let Some(result) = stream_export_file(&app, &state, &request, timestamp)? {
        return Ok(result);
    }

    let (filename, bytes, row_count) = {
        let store = state
            .store
            .lock()
            .map_err(|_| "store lock poisoned".to_string())?;
        match request.kind {
            ExportFileKind::Csv => {
                let records = store
                    .query(request.query.unwrap_or_else(export_grid_query))
                    .rows;
                let row_count = records.len();
                let csv = records_to_csv_string(&records).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-export-{timestamp}.csv"),
                    csv.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::Xlsx => {
                let records = store
                    .query(request.query.unwrap_or_else(export_grid_query))
                    .rows;
                let row_count = records.len();
                let bytes = records_to_xlsx_bytes(&records).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-export-{timestamp}.xlsx"),
                    bytes,
                    row_count,
                )
            }
            ExportFileKind::Sitemap => {
                let records = store
                    .query(request.query.unwrap_or_else(export_grid_query))
                    .rows;
                let row_count = records.len();
                let xml = records_to_sitemap_xml(&records);
                (
                    format!("sitemap-{timestamp}.xml"),
                    xml.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::LinkEdgesCsv => {
                let edges = store
                    .link_edges(LinkEdgeQuery {
                        offset: 0,
                        limit: 1_000_000,
                        ..LinkEdgeQuery::default()
                    })
                    .edges;
                let row_count = edges.len();
                let csv = link_edges_to_csv_string(&edges).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-link-edges-{timestamp}.csv"),
                    csv.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::RedirectChainsCsv => {
                let records = store.records();
                let row_count = records
                    .iter()
                    .map(|record| record.redirect_chain.len())
                    .sum::<usize>();
                let csv =
                    redirect_chains_to_csv_string(&records).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-redirect-chains-{timestamp}.csv"),
                    csv.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::SitemapValidationCsv => {
                let rows = store
                    .sitemap_validation(SitemapValidationQuery {
                        offset: 0,
                        limit: 1_000_000,
                        ..SitemapValidationQuery::default()
                    })
                    .rows;
                let row_count = rows.len();
                let csv =
                    sitemap_validation_to_csv_string(&rows).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-sitemap-validation-{timestamp}.csv"),
                    csv.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::HtmlReport => {
                let records = store.records();
                let edges = store
                    .link_edges(LinkEdgeQuery {
                        offset: 0,
                        limit: 1_000_000,
                        ..LinkEdgeQuery::default()
                    })
                    .edges;
                let row_count = records.len();
                let html =
                    records_to_html_report(&records, &edges).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-seo-report-{timestamp}.html"),
                    html.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::GraphJson => {
                let graph = store.crawl_graph(request.graph_query.unwrap_or_default());
                let row_count = graph.nodes.len();
                let bytes = serde_json::to_vec_pretty(&graph).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-graph-{timestamp}.json"),
                    bytes,
                    row_count,
                )
            }
            ExportFileKind::GraphNodesCsv => {
                let graph = store.crawl_graph(request.graph_query.unwrap_or_default());
                let row_count = graph.nodes.len();
                let csv =
                    graph_nodes_to_csv_string(&graph.nodes).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-graph-nodes-{timestamp}.csv"),
                    csv.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::GraphEdgesCsv => {
                let graph = store.crawl_graph(request.graph_query.unwrap_or_default());
                let row_count = graph.edges.len();
                let csv =
                    link_edges_to_csv_string(&graph.edges).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-graph-edges-{timestamp}.csv"),
                    csv.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::CrawlArchive => {
                let records = store.records();
                let link_edges = store
                    .link_edges(LinkEdgeQuery {
                        offset: 0,
                        limit: 1_000_000,
                        ..LinkEdgeQuery::default()
                    })
                    .edges;
                let image_assets = store
                    .image_assets(ImageAssetQuery {
                        offset: 0,
                        limit: 1_000_000,
                        ..ImageAssetQuery::default()
                    })
                    .images;
                let row_count = records.len();
                let archive = CrawlArchive {
                    schema_version: CRAWL_ARCHIVE_SCHEMA_VERSION,
                    exported_at_ms: timestamp,
                    records,
                    link_edges,
                    image_assets,
                    frontier_state: store.load_frontier_state(),
                };
                let bytes =
                    serde_json::to_vec_pretty(&archive).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-crawl-archive-{timestamp}.ffcrawl.json"),
                    bytes,
                    row_count,
                )
            }
        }
    };

    let path = export_path(&app, &filename)?;
    fs::write(&path, bytes).map_err(|error| format!("failed to write export file: {error}"))?;
    Ok(ExportFileResult {
        path: path.to_string_lossy().into_owned(),
        row_count,
    })
}

#[tauri::command(rename_all = "camelCase")]
fn import_crawl_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ImportCrawlArchiveRequest,
) -> Result<CrawlArchiveImportResult, String> {
    let bytes = fs::read(request.path.trim())
        .map_err(|error| format!("failed to read crawl archive: {error}"))?;
    let archive: CrawlArchive = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid crawl archive: {error}"))?;
    if archive.schema_version != CRAWL_ARCHIVE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported crawl archive schema version {}",
            archive.schema_version
        ));
    }

    let store = prepare_store(&app, &state, request.storage_mode, false)?;
    let record_count = archive.records.len();
    let link_edge_count = archive.link_edges.len();
    let image_asset_count = archive.image_assets.len();
    let frontier_item_count = archive
        .frontier_state
        .as_ref()
        .map(|state| state.queued.len())
        .unwrap_or(0);

    for record in archive.records {
        store.upsert(record);
    }
    for edge in archive.link_edges {
        store.add_link_edge(edge);
    }

    let mut image_assets_by_page = HashMap::<String, Vec<ImageAsset>>::new();
    for image in archive.image_assets {
        image_assets_by_page
            .entry(image.page_url.clone())
            .or_default()
            .push(image);
    }
    for (page_url, images) in image_assets_by_page {
        store.add_image_assets(&page_url, images);
    }

    if let Some(frontier_state) = archive.frontier_state {
        store.save_frontier_state(frontier_state);
    } else {
        store.clear_frontier_state();
    }

    Ok(CrawlArchiveImportResult {
        records: record_count,
        link_edges: link_edge_count,
        image_assets: image_asset_count,
        frontier_items: frontier_item_count,
    })
}

#[tauri::command(rename_all = "camelCase")]
fn compare_crawl_archive(
    state: State<'_, AppState>,
    request: CompareCrawlArchiveRequest,
) -> Result<CrawlComparisonResponse, String> {
    let bytes = fs::read(request.path.trim())
        .map_err(|error| format!("failed to read crawl archive: {error}"))?;
    let archive: CrawlArchive = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid crawl archive: {error}"))?;
    if archive.schema_version != CRAWL_ARCHIVE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported crawl archive schema version {}",
            archive.schema_version
        ));
    }

    let current_records = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .records();
    Ok(compare_records(&archive.records, &current_records))
}

#[tauri::command(rename_all = "camelCase")]
fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = url::Url::parse(url.trim()).map_err(|error| format!("invalid URL: {error}"))?;
    match parsed.scheme() {
        "http" | "https" => app
            .opener()
            .open_url(parsed.as_str(), None::<&str>)
            .map_err(|error| error.to_string()),
        _ => Err("only http and https URLs can be opened externally".to_string()),
    }
}

fn compare_records(
    baseline_records: &[CrawlRecord],
    current_records: &[CrawlRecord],
) -> CrawlComparisonResponse {
    let baseline_by_url = baseline_records
        .iter()
        .map(|record| (record.final_url.clone(), record))
        .collect::<HashMap<_, _>>();
    let current_by_url = current_records
        .iter()
        .map(|record| (record.final_url.clone(), record))
        .collect::<HashMap<_, _>>();

    let mut added = 0;
    let mut removed = 0;
    let mut changed = 0;
    let mut status_changed = 0;
    let mut title_changed = 0;
    let mut meta_description_changed = 0;
    let mut indexability_changed = 0;
    let mut hash_changed = 0;
    let mut rows = Vec::new();

    for (url, current) in &current_by_url {
        if !baseline_by_url.contains_key(url) {
            added += 1;
            push_comparison_row(&mut rows, comparison_row("added", url, None, Some(current)));
        }
    }

    for (url, previous) in &baseline_by_url {
        match current_by_url.get(url) {
            None => {
                removed += 1;
                push_comparison_row(
                    &mut rows,
                    comparison_row("removed", url, Some(previous), None),
                );
            }
            Some(current) => {
                let status_diff = previous.status_code != current.status_code;
                let title_diff = previous.title != current.title;
                let meta_diff = previous.meta_description != current.meta_description;
                let indexability_diff = previous.indexability != current.indexability
                    || previous.indexability_status != current.indexability_status;
                let hash_diff = previous.response_hash != current.response_hash;
                if status_diff || title_diff || meta_diff || indexability_diff || hash_diff {
                    changed += 1;
                    status_changed += usize::from(status_diff);
                    title_changed += usize::from(title_diff);
                    meta_description_changed += usize::from(meta_diff);
                    indexability_changed += usize::from(indexability_diff);
                    hash_changed += usize::from(hash_diff);
                    push_comparison_row(
                        &mut rows,
                        comparison_row("changed", url, Some(previous), Some(current)),
                    );
                }
            }
        }
    }

    rows.sort_by(|left, right| {
        comparison_change_order(&left.change)
            .cmp(&comparison_change_order(&right.change))
            .then_with(|| left.url.cmp(&right.url))
    });
    rows.truncate(1_000);

    let baseline_summary = summarize(baseline_records);
    let current_summary = summarize(current_records);
    CrawlComparisonResponse {
        baseline_records: baseline_records.len(),
        current_records: current_records.len(),
        added,
        removed,
        changed,
        status_changed,
        title_changed,
        meta_description_changed,
        indexability_changed,
        hash_changed,
        rows,
        metric_deltas: comparison_metric_deltas(&baseline_summary, &current_summary),
    }
}

fn push_comparison_row(rows: &mut Vec<CrawlComparisonRow>, row: CrawlComparisonRow) {
    if rows.len() < 2_000 {
        rows.push(row);
    }
}

fn comparison_row(
    change: &str,
    url: &str,
    previous: Option<&CrawlRecord>,
    current: Option<&CrawlRecord>,
) -> CrawlComparisonRow {
    CrawlComparisonRow {
        url: url.to_string(),
        change: change.to_string(),
        previous_status_code: previous.and_then(|record| record.status_code),
        current_status_code: current.and_then(|record| record.status_code),
        previous_title: previous.and_then(|record| record.title.clone()),
        current_title: current.and_then(|record| record.title.clone()),
        previous_indexability: previous.map(|record| record.indexability_status.clone()),
        current_indexability: current.map(|record| record.indexability_status.clone()),
        previous_response_hash: previous.and_then(|record| record.response_hash.clone()),
        current_response_hash: current.and_then(|record| record.response_hash.clone()),
    }
}

fn comparison_change_order(change: &str) -> u8 {
    match change {
        "added" => 0,
        "removed" => 1,
        "changed" => 2,
        _ => 3,
    }
}

fn comparison_metric_deltas(
    previous: &ferrous_frog_storage::CrawlSummary,
    current: &ferrous_frog_storage::CrawlSummary,
) -> Vec<ComparisonMetricDelta> {
    vec![
        metric_delta("URLs", previous.total, current.total),
        metric_delta("Broken", previous.broken, current.broken),
        metric_delta("Indexable", previous.indexable, current.indexable),
        metric_delta(
            "Non-indexable",
            previous.non_indexable,
            current.non_indexable,
        ),
        metric_delta(
            "Missing titles",
            previous.title_missing,
            current.title_missing,
        ),
        metric_delta(
            "Duplicate titles",
            previous.title_duplicate,
            current.title_duplicate,
        ),
        metric_delta("Missing meta", previous.meta_missing, current.meta_missing),
        metric_delta(
            "Duplicate meta",
            previous.meta_duplicate,
            current.meta_duplicate,
        ),
        metric_delta("Missing H1", previous.h1_missing, current.h1_missing),
        metric_delta(
            "Near duplicates",
            previous.near_duplicates,
            current.near_duplicates,
        ),
    ]
}

fn metric_delta(label: &str, previous: usize, current: usize) -> ComparisonMetricDelta {
    ComparisonMetricDelta {
        label: label.to_string(),
        previous,
        current,
        delta: current as isize - previous as isize,
    }
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

        CREATE TABLE IF NOT EXISTS integration_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
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

fn get_integration_setting(app: &AppHandle, key: &str) -> Result<Option<String>, String> {
    let conn = session_index_connection(app)?;
    conn.query_row(
        "SELECT value FROM integration_settings WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn set_integration_setting(app: &AppHandle, key: &str, value: &str) -> Result<(), String> {
    let conn = session_index_connection(app)?;
    conn.execute(
        "INSERT INTO integration_settings (key, value, updated_at_ms)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at_ms = excluded.updated_at_ms",
        params![key, value, now_ms()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn delete_integration_setting(app: &AppHandle, key: &str) -> Result<(), String> {
    let conn = session_index_connection(app)?;
    conn.execute("DELETE FROM integration_settings WHERE key = ?1", [key])
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn search_console_keyring_entry() -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, GSC_KEYRING_ACCOUNT)
        .map_err(|error| format!("failed to open OS credential store: {error}"))
}

fn read_search_console_access_token() -> Result<Option<String>, String> {
    match search_console_keyring_entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(format!("failed to read Search Console token: {error}")),
    }
}

fn search_console_credential_status(
    app: &AppHandle,
) -> Result<SearchConsoleCredentialStatus, String> {
    let site_url = get_integration_setting(app, GSC_SITE_URL_SETTING)?;
    let token_status = match search_console_keyring_entry() {
        Ok(entry) => match entry.get_password() {
            Ok(token) => (true, !token.trim().is_empty(), None),
            Err(KeyringError::NoEntry) => (true, false, None),
            Err(error) => (true, false, Some(error.to_string())),
        },
        Err(error) => (false, false, Some(error)),
    };

    Ok(SearchConsoleCredentialStatus {
        site_url,
        keyring_available: token_status.0,
        token_saved: token_status.1,
        message: token_status.2,
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

fn export_grid_query() -> GridQuery {
    GridQuery {
        offset: 0,
        limit: 1_000_000,
        ..GridQuery::default()
    }
}

fn url_tree_segments(record: &CrawlRecord) -> Vec<String> {
    let raw_url = if record.final_url.trim().is_empty() {
        record.url.as_str()
    } else {
        record.final_url.as_str()
    };

    if let Ok(parsed) = url::Url::parse(raw_url) {
        let mut host = match parsed.host_str() {
            Some(host) => format!("{}://{}", parsed.scheme(), host),
            None => parsed.scheme().to_string(),
        };
        if let Some(port) = parsed.port() {
            host.push(':');
            host.push_str(&port.to_string());
        }

        let mut segments = vec![host];
        let path_segments = parsed
            .path_segments()
            .map(|segments| {
                segments
                    .filter(|segment| !segment.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if path_segments.is_empty() {
            segments.push("/".to_string());
        } else {
            segments.extend(path_segments);
        }

        if let Some(query) = parsed.query() {
            segments.push(format!("?{query}"));
        }

        return segments;
    }

    let mut fallback_segments = raw_url
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if fallback_segments.is_empty() {
        fallback_segments.push(raw_url.to_string());
    }
    fallback_segments
}

fn insert_url_tree_record(
    nodes: &mut Vec<UrlTreeNode>,
    segments: &[String],
    record: &CrawlRecord,
    segment_index: usize,
    parent_path: &str,
) {
    if segment_index >= segments.len() {
        return;
    }

    let label = segments[segment_index].clone();
    let path = url_tree_path(parent_path, &label);
    let node_index = nodes
        .iter()
        .position(|node| node.path == path)
        .unwrap_or_else(|| {
            nodes.push(UrlTreeNode::new(
                path.clone(),
                label.clone(),
                path.clone(),
                segment_index,
            ));
            nodes.len() - 1
        });

    let is_leaf = segment_index == segments.len() - 1;
    let node = &mut nodes[node_index];
    apply_url_tree_counts(node, record);

    if is_leaf {
        node.url = Some(record.final_url.clone());
        node.record = Some(record.clone());
    } else {
        insert_url_tree_record(
            &mut node.children,
            segments,
            record,
            segment_index + 1,
            &path,
        );
    }
}

fn url_tree_path(parent_path: &str, label: &str) -> String {
    if parent_path.is_empty() {
        return label.to_string();
    }
    if label == "/" {
        return format!("{parent_path}/");
    }
    if label.starts_with('?') {
        return format!("{parent_path}{label}");
    }
    format!("{parent_path}/{label}")
}

fn apply_url_tree_counts(node: &mut UrlTreeNode, record: &CrawlRecord) {
    node.total += 1;
    match record.status_code {
        Some(200..=299) => node.success += 1,
        Some(300..=399) => node.redirects += 1,
        Some(400..=499) => node.client_errors += 1,
        Some(500..=599) => node.server_errors += 1,
        None if is_no_response_record(record) => node.no_response += 1,
        _ => {}
    }

    if is_broken_record(record) {
        node.broken += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_counts_exclude_robots_and_pending_urls_from_failures() {
        let mut node = UrlTreeNode::new("root".into(), "Root".into(), "/".into(), 0);
        let mut record = CrawlRecord::pending("https://example.test/".into(), 0);
        apply_url_tree_counts(&mut node, &record);
        record.status_text = "Blocked by robots.txt".into();
        record.error = Some("Blocked by robots.txt".into());
        apply_url_tree_counts(&mut node, &record);
        assert_eq!((node.no_response, node.broken), (0, 0));
        record.status_text = "Request failed".into();
        record.error = Some("Connection refused".into());
        apply_url_tree_counts(&mut node, &record);
        record.status_code = Some(404);
        apply_url_tree_counts(&mut node, &record);
        assert_eq!(
            (node.no_response, node.client_errors, node.broken),
            (1, 1, 2)
        );
    }
}

fn sort_url_tree_nodes(nodes: &mut [UrlTreeNode]) {
    nodes.sort_by(|left, right| {
        left.children
            .is_empty()
            .cmp(&right.children.is_empty())
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });

    for node in nodes {
        sort_url_tree_nodes(&mut node.children);
    }
}

fn export_path(app: &AppHandle, filename: &str) -> Result<PathBuf, String> {
    let base = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|error| format!("failed to resolve export directory: {error}"))?;
    let dir = base.join("Ferrous Frog").join("exports");
    fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create export directory: {error}"))?;
    Ok(dir.join(filename))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            store: Mutex::new(ActiveStore::memory()),
            control: Mutex::new(None),
            crawl_task: tokio::sync::Mutex::new(None),
            current_session_id: Mutex::new(None),
            frontend_ready: AtomicBool::new(false),
            exit_confirmed: AtomicBool::new(false),
        })
        .setup(|app| {
            let app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Keep the main window reachable if the frontend cannot finish startup.
                tokio::time::sleep(Duration::from_secs(12)).await;
                if app.get_webview_window("splashscreen").is_some() {
                    let _ = show_main_window(&app);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            complete_startup,
            quit_app,
            start_crawl,
            pause_crawl,
            resume_crawl,
            stop_crawl,
            test_robots_txt,
            test_robots_txt_batch,
            download_robots_txt,
            get_rows,
            get_url_tree,
            get_issues,
            get_link_edges,
            get_image_assets,
            get_anchor_texts,
            get_sitemap_validation,
            get_crawl_graph,
            get_crawl_path,
            list_crawl_sessions,
            create_crawl_session,
            open_crawl_session,
            delete_crawl_session,
            get_database_location,
            open_database_path,
            get_recovery_state,
            list_config_profiles,
            save_config_profile,
            load_config_profile,
            delete_config_profile,
            get_search_console_credential_status,
            save_search_console_credentials,
            clear_search_console_credentials,
            test_search_console_credentials,
            merge_search_console_metrics,
            export_csv,
            export_xlsx,
            export_sitemap,
            export_link_edges_csv,
            export_redirect_chains_csv,
            export_html_report,
            export_file,
            import_crawl_archive,
            compare_crawl_archive,
            open_external_url
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Ferrous Frog")
        .run(|app, event| {
            let state = app.state::<AppState>();
            let confirm = state.frontend_ready.load(Ordering::SeqCst)
                && !state.exit_confirmed.load(Ordering::SeqCst);
            match event {
                tauri::RunEvent::WindowEvent {
                    label,
                    event: tauri::WindowEvent::CloseRequested { api, .. },
                    ..
                } => {
                    if confirm {
                        api.prevent_close();
                        request_quit(app);
                    } else if label == "splashscreen" {
                        app.exit(0);
                    }
                }
                tauri::RunEvent::ExitRequested { api, .. } if confirm => {
                    api.prevent_exit();
                    request_quit(app);
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn stopping_waits_for_crawl_cleanup_before_returning() {
        let control = CrawlControl::default();
        let task_control = control.clone();
        let cleaned_up = Arc::new(AtomicBool::new(false));
        let task_cleaned_up = cleaned_up.clone();
        let task = tauri::async_runtime::spawn(async move {
            while !task_control.is_cancelled() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            task_cleaned_up.store(true, Ordering::SeqCst);
        });
        let state = AppState {
            store: Mutex::new(ActiveStore::memory()),
            control: Mutex::new(Some(control.clone())),
            crawl_task: tokio::sync::Mutex::new(Some(task)),
            current_session_id: Mutex::new(None),
            frontend_ready: AtomicBool::new(true),
            exit_confirmed: AtomicBool::new(false),
        };
        let mut task = state.crawl_task.lock().await;
        tokio::time::timeout(Duration::from_secs(1), stop_active_crawl(&state, &mut task))
            .await
            .unwrap()
            .unwrap();
        assert!(control.is_cancelled());
        assert!(cleaned_up.load(Ordering::SeqCst));
        stop_active_crawl(&state, &mut task).await.unwrap();
    }
}
