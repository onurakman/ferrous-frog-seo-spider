#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod archive_import;
mod archive_staging;
mod audit_report_ai;
mod audit_report_comparison;
mod audit_report_export;
mod audit_reports;
mod comparison;
mod comparison_sources;
mod content;
#[cfg(test)]
mod export_tests;
mod google_oauth;
mod http_auth;
mod page_captures;
mod pagespeed;
mod serp;
#[cfg(test)]
mod session_tests;
mod sessions;
mod updates;
mod window_state;

use comparison::{ComparisonDetail, ComparisonPage, ComparisonQuery, ComparisonWorkspace};
use ferrous_frog_analysis::analyze_records;
use ferrous_frog_crawler_core::{AutomationConfig, CrawlProgress};
use ferrous_frog_crawler_core::{
    CrawlConfig, CrawlControl, CrawlerEvent, RenderingStatus, RobotsTxtBatchTestRequest,
    RobotsTxtBatchTestResult, RobotsTxtDownloadRequest, RobotsTxtDownloadResult,
    RobotsTxtTestRequest, RobotsTxtTestResult, crawl,
    download_robots_txt as run_robots_txt_download, rendering_status,
    test_robots_txt as run_robots_txt_test, test_robots_txt_batch as run_robots_txt_batch_test,
    validate_configuration, validate_crawl_start, validate_rendering,
};
use ferrous_frog_export::{
    audit_workbook_to_writer, export_preset, graph_nodes_to_csv, link_edges_to_csv,
    link_edges_to_csv_string, query_to_xlsx_writer, records_to_csv, records_to_csv_string,
    records_to_html_report_store_writer, records_to_sitemap_xml, records_to_xlsx_bytes,
    redirect_chains_to_csv_string, sitemap_validation_to_csv_string, store_link_edges_to_csv,
    write_export_files,
};
use ferrous_frog_integrations::{
    AnalyticsConfig, BacklinkEndpointConfig, BacklinkEndpointProvider, DateRange,
    GoogleAnalyticsProvider, MetricRequest, SearchConsoleConfig, SearchConsoleProvider,
    UrlMetricProvider,
};
use ferrous_frog_storage::{
    ActiveStore, AnalyticsMetricRow, AnchorTextResponse, AuditThresholds, BacklinkMetricRow,
    CrawlFrontierState, CrawlGraph, CrawlGraphQuery, CrawlPathQuery, CrawlPathResponse,
    CrawlRecord, CrawlStore, GridQuery, GridResponse, ImageAsset, ImageAssetQuery,
    ImageAssetResponse, Issue, LinkEdge, LinkEdgeQuery, LinkEdgeResponse, MAX_CAPTURE_PAGE_SIZE,
    PageCapture, PageCaptureQuery, PageReference, PageReferenceQuery, PageReferenceResponse,
    SearchConsoleMetricRow, SitemapValidationQuery, SitemapValidationResponse, is_broken_record,
    is_no_response_record, summarize, validate_grid_query,
};
use keyring::{Entry, Error as KeyringError};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sessions::{CrawlSession, get_session, query_sessions};
use std::collections::HashSet;
use std::fs;
use std::io::{BufWriter, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_window_state::AppHandleExt;

// ponytail: process-wide audit thresholds; move into AppState if sessions ever need separate limits.
static AUDIT_THRESHOLDS: RwLock<AuditThresholds> = RwLock::new(AuditThresholds {
    title_min_chars: 30,
    title_max_chars: 60,
    title_min_pixels: 200,
    title_max_pixels: 580,
    meta_min_chars: 70,
    meta_max_chars: 160,
    meta_min_pixels: 400,
    meta_max_pixels: 920,
    h1_max_chars: 70,
    h2_max_chars: 70,
    large_image_bytes: 200 * 1024,
    thin_content_words: 200,
    min_text_ratio_percent: 10,
});

fn audit_thresholds() -> AuditThresholds {
    *AUDIT_THRESHOLDS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn with_thresholds(mut query: GridQuery) -> GridQuery {
    query.thresholds = audit_thresholds();
    query
}

#[tauri::command]
fn set_audit_thresholds(thresholds: AuditThresholds) -> Result<(), String> {
    thresholds.validate()?;
    *AUDIT_THRESHOLDS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = thresholds;
    Ok(())
}

struct AppState {
    store: Mutex<ActiveStore>,
    control: Mutex<Option<CrawlControl>>,
    crawl_task: tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    current_session_id: Mutex<Option<String>>,
    comparison: Mutex<ComparisonState>,
    frontend_ready: AtomicBool,
    exit_confirmed: AtomicBool,
}

#[derive(Default)]
struct ComparisonState {
    id: Option<String>,
    generation: u64,
    workspace: Option<Arc<Mutex<ComparisonWorkspace>>>,
}

impl ComparisonState {
    fn reserve(&mut self, id: &str) -> Result<u64, String> {
        if id.trim().is_empty() || id.len() > 128 {
            return Err("comparison ID must contain between 1 and 128 characters".into());
        }
        if self.id.as_deref() == Some(id) {
            return Err("this comparison ID is already in use".into());
        }
        self.id = Some(id.to_owned());
        self.generation = self.generation.wrapping_add(1);
        if let Some(workspace) = self.workspace.take() {
            drop_comparison(workspace);
        }
        Ok(self.generation)
    }

    fn close(&mut self, id: &str) {
        if self.id.as_deref() == Some(id) {
            self.id = None;
            if let Some(workspace) = self.workspace.take() {
                drop_comparison(workspace);
            }
        }
    }

    fn publish(
        &mut self,
        id: &str,
        generation: u64,
        workspace: ComparisonWorkspace,
    ) -> Result<(), String> {
        let workspace = Arc::new(Mutex::new(workspace));
        if self.id.as_deref() != Some(id) || self.generation != generation {
            drop_comparison(workspace);
            return Err("comparison preparation was superseded or closed".into());
        }
        self.workspace = Some(workspace);
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Arc<Mutex<ComparisonWorkspace>>, String> {
        if self.id.as_deref() != Some(id) {
            return Err("comparison is closed or has been replaced".into());
        }
        self.workspace
            .clone()
            .ok_or_else(|| "comparison is still being prepared".into())
    }
}

fn drop_comparison(workspace: Arc<Mutex<ComparisonWorkspace>>) {
    // SQLite close/checkpoint and temporary file deletion can block on disk I/O.
    tauri::async_runtime::spawn_blocking(move || drop(workspace));
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenCrawlComparisonRequest {
    comparison_id: String,
    baseline_session_id: Option<String>,
    current_session_id: Option<String>,
    archive_path: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum StorageMode {
    Memory,
    Database,
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
    record_ids: Option<Vec<u64>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ExportFileKind {
    Csv,
    Xlsx,
    AuditWorkbook,
    SelectedCsv,
    QueuedUrlsCsv,
    ImageAltCsv,
    ResponseHeadersCsv,
    RawHtmlCsv,
    RenderedHtmlCsv,
    VisibleTextCsv,
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
    session: CrawlSession,
    records: usize,
    link_edges: usize,
    image_assets: usize,
    frontier_items: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompareCrawlArchiveRequest {
    path: String,
    #[serde(default)]
    include_response_only: bool,
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
    content_changed: usize,
    response_only: usize,
    content_unavailable: usize,
    rows: Vec<CrawlComparisonRow>,
    metric_deltas: Vec<ComparisonMetricDelta>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrawlComparisonRow {
    url: String,
    identity_key: String,
    occurrence: usize,
    change: String,
    previous_url: Option<String>,
    current_url: Option<String>,
    previous_final_url: Option<String>,
    current_final_url: Option<String>,
    previous_list_position: Option<u32>,
    current_list_position: Option<u32>,
    previous_status_code: Option<u16>,
    current_status_code: Option<u16>,
    previous_title: Option<String>,
    current_title: Option<String>,
    previous_indexability: Option<String>,
    current_indexability: Option<String>,
    previous_response_hash: Option<String>,
    current_response_hash: Option<String>,
    previous_meta_description: Option<String>,
    current_meta_description: Option<String>,
    changed_fields: Vec<String>,
    content_comparison: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeAnalyticsMetricsRequest {
    property_id: String,
    start_date: String,
    end_date: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalyticsMergeResult {
    property_id: String,
    fetched_rows: usize,
    matched_rows: usize,
    sessions: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveBacklinkSettingsRequest {
    endpoint_template: String,
    header_name: String,
    header_value: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BacklinkSettingsStatus {
    endpoint_template: String,
    header_name: String,
    credential_saved: bool,
    keyring_available: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeBacklinkMetricsRequest {
    max_urls: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BacklinkMergeResult {
    requested_urls: usize,
    matched_rows: usize,
    failed_urls: usize,
    first_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DatabaseLocation {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<CrawlSession>,
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
    #[serde(default)]
    page_references: Vec<PageReference>,
    #[serde(default)]
    page_captures: Vec<PageCapture>,
    frontier_state: Option<CrawlFrontierState>,
}

const CRAWL_ARCHIVE_SCHEMA_VERSION: u32 = 1;
const EXPORT_STREAM_PAGE_SIZE: usize = 10_000;
const URL_TREE_RECORD_LIMIT: usize = 10_000;
const KEYRING_SERVICE: &str = "ferrous-frog-seo-spider";
const GSC_KEYRING_ACCOUNT: &str = "google-search-console";
const GSC_SITE_URL_SETTING: &str = "google_search_console_site_url";
const GA4_PROPERTY_SETTING: &str = "google_analytics_property_id";
const BACKLINK_ENDPOINT_SETTING: &str = "backlink_endpoint_template";
const BACKLINK_HEADER_SETTING: &str = "backlink_header_name";
const BACKLINK_KEYRING_ACCOUNT: &str = "backlink-api-key";

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

#[tauri::command]
async fn get_rendering_status() -> RenderingStatus {
    rendering_status()
}

#[tauri::command(rename_all = "camelCase")]
fn validate_crawl_configuration(config: CrawlConfig) -> Result<(), String> {
    validate_configuration(&config).map_err(|error| format!("{error:#}"))
}

#[tauri::command(rename_all = "camelCase")]
async fn start_crawl(
    app: AppHandle,
    state: State<'_, AppState>,
    mut config: CrawlConfig,
    resume: bool,
) -> Result<CrawlSession, String> {
    validate_crawl_start(&config).map_err(|error| format!("{error:#}"))?;
    if config.http_auth.enabled {
        config.basic_credentials = Some(http_auth::load_saved_credentials().await?);
    }
    if config.form_login.enabled {
        config.form_credentials = Some(http_auth::load_form_login_credentials().await?);
    }
    validate_rendering(&config.rendering).map_err(|error| error.to_string())?;
    let mut current_task = state.crawl_task.lock().await;
    ensure_idle(&state, &current_task)?;
    stop_active_crawl(&state, &mut current_task).await?;
    let mut current_control = state
        .control
        .lock()
        .map_err(|_| "crawl control lock poisoned".to_string())?;
    let (session, store, config, conn) =
        prepare_crawl_session(&state, &app_data_dir(&app)?, config, resume)?;
    let control = CrawlControl::default();
    *current_control = Some(control.clone());
    let session_id = session.id.clone();
    let automation = config.automation.clone();
    let thresholds = config.thresholds;
    let automation_store = store.clone();
    let task = tauri::async_runtime::spawn(async move {
        let progress_conn = Arc::new(Mutex::new(conn));
        let persistence_error = Arc::new(Mutex::new(None::<String>));
        let event_conn = progress_conn.clone();
        let event_error = persistence_error.clone();
        let event_control = control.clone();
        let event_session_id = session_id.clone();
        let event_app = app.clone();
        let emit = move |event: CrawlerEvent| {
            if let Some(progress) = &event.progress {
                let result = event_conn
                    .lock()
                    .map_err(|_| "session progress lock poisoned".to_string())
                    .and_then(|conn| {
                        sessions::save_progress(
                            &conn,
                            &event_session_id,
                            &progress.status,
                            progress.crawled,
                        )
                    });
                if let Err(error) = result {
                    *event_error.lock().expect("progress error lock poisoned") = Some(error);
                    event_control.cancel();
                }
            }
            let _ = event_app.emit("crawl-event", event);
        };
        // Observe engine panics as well as returned errors so history never stays "running".
        let result = tauri::async_runtime::spawn(crawl(config, store, control, emit)).await;
        let mut finished = None;
        let error = match result {
            Ok(Ok(progress)) => {
                finished = Some(progress);
                None
            }
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(error) => Some(format!("crawl task failed: {error}")),
        }
        .or_else(|| {
            persistence_error
                .lock()
                .ok()
                .and_then(|error| error.clone())
        });
        if let Some(mut error) = error {
            let saved = progress_conn
                .lock()
                .map_err(|_| "session progress lock poisoned".to_string())
                .and_then(|conn| sessions::save_status(&conn, &session_id, "failed"));
            if let Err(save_error) = saved {
                error.push_str(&format!("; {save_error}"));
            }
            let mut event = CrawlerEvent::error(error);
            event.kind = "failed".to_string();
            let _ = app.emit("crawl-event", event);
        }
        run_automation(
            &app,
            &automation,
            thresholds,
            automation_store,
            &session_id,
            finished.as_ref(),
        )
        .await;
    });
    *current_task = Some(task);
    Ok(session)
}

/// Post-crawl actions: preset exports, a webhook summary and a desktop notification.
/// Failures become notices; they never change the crawl outcome.
async fn run_automation(
    app: &AppHandle,
    automation: &AutomationConfig,
    thresholds: AuditThresholds,
    store: ActiveStore,
    session_id: &str,
    progress: Option<&CrawlProgress>,
) {
    let mut notices = Vec::new();
    let mut exported = Vec::new();
    if let (Some(_), Some(kinds)) = (progress, export_preset(&automation.export_preset)) {
        let dir = export_dir(app).map(|dir| dir.join(format!("auto-{}", now_ms())));
        let written = match dir {
            Ok(dir) => tauri::async_runtime::spawn_blocking(move || {
                write_export_files(&store, &thresholds, &kinds, &dir)
            })
            .await
            .map_err(|error| format!("export worker failed: {error}"))
            .and_then(|result| result),
            Err(error) => Err(error),
        };
        match written {
            Ok(paths) => {
                if let Some(dir) = paths.first().and_then(|path| path.parent()) {
                    notices.push(format!(
                        "Exported {} file(s) to {}",
                        paths.len(),
                        dir.display()
                    ));
                }
                exported = paths;
            }
            Err(error) => notices.push(format!("Automatic export failed: {error}")),
        }
    }
    if !automation.webhook_url.trim().is_empty() {
        let payload = automation_payload(session_id, progress, &exported);
        if let Err(error) = post_webhook(automation.webhook_url.trim(), &payload).await {
            notices.push(format!("Webhook failed: {error}"));
        }
    }
    if automation.notify_on_completion {
        use tauri_plugin_notification::NotificationExt;
        let body = match progress {
            Some(progress) => format!(
                "Crawl finished: {} URLs crawled, {} broken",
                progress.crawled, progress.summary.broken
            ),
            None => "Crawl failed".to_string(),
        };
        if let Err(error) = app
            .notification()
            .builder()
            .title("Ferrous Frog SEO Spider")
            .body(body)
            .show()
        {
            notices.push(format!("Notification failed: {error}"));
        }
    }
    if !notices.is_empty() {
        let _ = app.emit("crawl-event", CrawlerEvent::notice(notices.join(" · ")));
    }
}

fn automation_payload(
    session_id: &str,
    progress: Option<&CrawlProgress>,
    exported: &[PathBuf],
) -> serde_json::Value {
    serde_json::json!({
        "event": if progress.is_some() { "crawl.finished" } else { "crawl.failed" },
        "sessionId": session_id,
        "status": progress.map(|p| p.status.as_str()).unwrap_or("failed"),
        "crawled": progress.map(|p| p.crawled).unwrap_or(0),
        "queued": progress.map(|p| p.queued).unwrap_or(0),
        "discovered": progress.map(|p| p.discovered).unwrap_or(0),
        "elapsedMs": progress.map(|p| p.elapsed_ms).unwrap_or(0),
        "summary": progress.map(|p| serde_json::to_value(&p.summary).unwrap_or_default()),
        "exports": exported.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "sentAtMs": now_ms(),
    })
}

async fn post_webhook(url: &str, payload: &serde_json::Value) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    client
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn ensure_idle(
    state: &AppState,
    task: &Option<tauri::async_runtime::JoinHandle<()>>,
) -> Result<(), String> {
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("the application is closing".to_string());
    }
    if task
        .as_ref()
        .is_some_and(|task| !task.inner().is_finished())
    {
        return Err("stop the active crawl before changing or comparing saved crawls".to_string());
    }
    Ok(())
}

fn prepare_crawl_session(
    state: &AppState,
    dir: &Path,
    mut config: CrawlConfig,
    resume: bool,
) -> Result<(CrawlSession, ActiveStore, CrawlConfig, Connection), String> {
    config.resume_from_state = false;
    validate_crawl_start(&config).map_err(|error| format!("{error:#}"))?;
    validate_rendering(&config.rendering).map_err(|error| error.to_string())?;
    let mut active_store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let mut current_id = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let conn = sessions::index_connection(dir)?;
    let (mut session, store) = if resume {
        let id = current_id
            .as_deref()
            .ok_or("open a saved crawl before resuming")?;
        let mut session = get_session(&conn, id, Some(id))?;
        sessions::load_config(&conn, &mut session)?;
        sessions::validate_resume(&session, &config)?;
        let store = sessions::open_existing_database(Path::new(&session.database_path))?;
        let crawled = sessions::resume_count(Path::new(&session.database_path))?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        sessions::save_config(&tx, id, &config)?;
        sessions::save_progress(&tx, id, "starting", crawled)?;
        tx.commit().map_err(|error| error.to_string())?;
        (get_session(&conn, id, Some(id))?, store)
    } else {
        let seed = sessions::seed_url(&config);
        let name = url::Url::parse(seed)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
            .unwrap_or_else(|| "New crawl".to_string());
        sessions::create_session(&conn, dir, &name, seed, Some(&config), "starting")?
    };
    config.resume_from_state = resume;
    session.config = None;
    let store = ActiveStore::Sqlite(store);
    *active_store = store.clone();
    *current_id = Some(session.id.clone());
    Ok((session, store, config, conn))
}

#[tauri::command]
async fn pause_crawl(state: State<'_, AppState>) -> Result<(), String> {
    let _task = state.crawl_task.lock().await;
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
async fn resume_crawl(state: State<'_, AppState>) -> Result<(), String> {
    let _task = state.crawl_task.lock().await;
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
async fn quit_app(
    app: AppHandle,
    state: State<'_, AppState>,
    page_speed: State<'_, pagespeed::PageSpeedState>,
    reports: State<'_, audit_reports::AuditReportsState>,
) -> Result<(), String> {
    let page_speed_shutdown = page_speed.begin_shutdown()?;
    let mut task = state.crawl_task.lock().await;
    stop_active_crawl(&state, &mut task).await?;
    state.exit_confirmed.store(true, Ordering::SeqCst);
    drop(task);
    if let Err(error) = reports.cancel_for_shutdown().await {
        state.exit_confirmed.store(false, Ordering::SeqCst);
        return Err(error);
    }
    if let Err(error) = app.save_window_state(window_state::FLAGS) {
        eprintln!("failed to save window position: {error}");
    }
    page_speed_shutdown.commit();
    app.exit(0);
    Ok(())
}

fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?;
    if !main.is_visible().map_err(|error| error.to_string())? {
        // A disconnected or rearranged monitor must not leave the title bar unreachable.
        if let Err(error) = window_state::ensure_reachable(&main) {
            eprintln!("failed to restore a reachable window position: {error}");
        }
    }
    main.show().map_err(|error| error.to_string())?;
    let _ = main.unminimize();
    let _ = main.set_focus();
    if let Some(splash) = app.get_webview_window("splashscreen") {
        splash.destroy().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn complete_startup(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.frontend_ready.store(true, Ordering::SeqCst);
    if app.get_webview_window("splashscreen").is_some() {
        let remaining =
            Duration::from_millis(2_200).saturating_sub(app.state::<Instant>().elapsed());
        tokio::time::sleep(remaining).await;
        // A quit request or the startup fallback may already have revealed the main window.
        if app.get_webview_window("splashscreen").is_none() {
            return Ok(());
        }
    }
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
fn validate_result_filters(filters: ferrous_frog_storage::GridFilterGroup) -> Result<(), String> {
    ferrous_frog_storage::validate_grid_query(&GridQuery {
        filters: Some(filters),
        ..GridQuery::default()
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_rows(state: State<'_, AppState>, query: GridQuery) -> Result<GridResponse, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .clone();
    tauri::async_runtime::spawn_blocking(move || query_store_rows(&store, query))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn get_url_tree(
    state: State<'_, AppState>,
    mut query: GridQuery,
) -> Result<UrlTreeResponse, String> {
    query.offset = 0;
    query.limit = URL_TREE_RECORD_LIMIT;
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        let response = query_store_rows(&store, query)?;
        let mut nodes = Vec::new();
        for record in &response.rows {
            let segments = url_tree_segments(record);
            insert_url_tree_record(&mut nodes, &segments, record, 0, "");
        }
        sort_url_tree_nodes(&mut nodes);

        Ok(UrlTreeResponse {
            nodes,
            total_urls: response.total,
            rendered_urls: response.rows.len(),
            capped: response.total > response.rows.len(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn get_issues(state: State<'_, AppState>) -> Vec<Issue> {
    let records = state.store.lock().expect("store lock poisoned").records();
    analyze_records(&records, &audit_thresholds())
}

#[tauri::command]
async fn get_link_edges(
    state: State<'_, AppState>,
    query: LinkEdgeQuery,
) -> Result<LinkEdgeResponse, String> {
    with_store_worker(&state, false, move |store| {
        query_store_link_edges(store, query)
    })
    .await
}

#[tauri::command]
async fn get_image_assets(
    state: State<'_, AppState>,
    query: ImageAssetQuery,
) -> Result<ImageAssetResponse, String> {
    with_store_worker(&state, false, move |store| query_store_images(store, query)).await
}

#[tauri::command]
async fn page_references(
    state: State<'_, AppState>,
    query: PageReferenceQuery,
) -> Result<PageReferenceResponse, String> {
    with_store_worker(&state, false, move |store| {
        query_store_page_references(store, query)
    })
    .await
}

#[tauri::command]
async fn get_anchor_texts(
    state: State<'_, AppState>,
    query: LinkEdgeQuery,
) -> Result<AnchorTextResponse, String> {
    with_store_worker(&state, false, move |store| {
        query_store_anchor_texts(store, query)
    })
    .await
}

#[tauri::command]
async fn get_sitemap_validation(
    state: State<'_, AppState>,
    query: SitemapValidationQuery,
) -> Result<SitemapValidationResponse, String> {
    with_store_worker(&state, false, move |store| {
        query_store_sitemap_validation(store, query)
    })
    .await
}

#[tauri::command]
async fn get_crawl_graph(
    state: State<'_, AppState>,
    query: CrawlGraphQuery,
) -> Result<CrawlGraph, String> {
    with_store_worker(&state, false, move |store| {
        store
            .try_crawl_graph(query)
            .map_err(|error| error.to_string())
    })
    .await
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
async fn create_crawl_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CreateSessionRequest,
) -> Result<CrawlSession, String> {
    let task = state.crawl_task.lock().await;
    ensure_idle(&state, &task)?;
    let mut store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let mut current = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let dir = app_data_dir(&app)?;
    let conn = sessions::index_connection(&dir)?;
    let config = CrawlConfig {
        start_url: request.start_url.trim().to_string(),
        ..CrawlConfig::default()
    };
    validate_configuration(&config).map_err(|error| format!("{error:#}"))?;
    let (session, database) = sessions::create_session(
        &conn,
        &dir,
        &request.name,
        &config.start_url,
        Some(&config),
        "ready",
    )?;
    *store = ActiveStore::Sqlite(database);
    *current = Some(session.id.clone());
    Ok(session)
}

#[tauri::command]
async fn open_crawl_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CrawlSession, String> {
    let task = state.crawl_task.lock().await;
    ensure_idle(&state, &task)?;
    activate_session(&state, &session_index_connection(&app)?, &session_id)
}

fn activate_session(
    state: &AppState,
    conn: &Connection,
    session_id: &str,
) -> Result<CrawlSession, String> {
    let mut active_store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let mut current = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let mut session = get_session(conn, session_id, Some(session_id))?;
    sessions::load_config(conn, &mut session)?;
    let store = sessions::open_existing_database(Path::new(&session.database_path))?;
    sessions::refresh_legacy_metadata(conn, &mut session)?;
    *active_store = ActiveStore::Sqlite(store);
    *current = Some(session_id.to_string());
    Ok(session)
}

#[tauri::command]
async fn delete_crawl_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let task = state.crawl_task.lock().await;
    ensure_idle(&state, &task)?;
    delete_session(&state, &session_index_connection(&app)?, &session_id)
}

fn delete_session(state: &AppState, conn: &Connection, session_id: &str) -> Result<(), String> {
    let session = get_session(conn, session_id, None)?;
    let mut active_store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let mut current = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    tx.execute("DELETE FROM crawl_sessions WHERE id = ?1", [session_id])
        .map_err(|error| error.to_string())?;
    let path = Path::new(&session.database_path);
    // Stage removal so a failed index commit can restore the database at its original path.
    let staged_path =
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => return Err(
                "the saved database path is a directory; restore the crawl file before deleting it"
                    .to_string(),
            ),
            Ok(_) => {
                let suffix: String = tx
                    .query_row("SELECT lower(hex(randomblob(8)))", [], |row| row.get(0))
                    .map_err(|error| error.to_string())?;
                Some(path.with_extension(format!("deleting-{suffix}")))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(format!(
                    "could not read crawl database {}: {error}",
                    path.display()
                ));
            }
        };
    let release_current = current.as_deref() == Some(session_id) && staged_path.is_some();
    if release_current {
        // Windows cannot rename an open SQLite file. The store lock blocks new readers.
        *active_store = ActiveStore::memory();
    }
    let mut renamed = false;
    let remove = || -> Result<(), String> {
        if let Some(staged) = &staged_path {
            fs::rename(path, staged).map_err(|error| format!("could not remove crawl database {}: {error}; retry after pending queries finish", path.display()))?;
            renamed = true;
        }
        tx.commit()
            .map_err(|error| format!("failed to delete saved card: {error}"))
    };
    if let Err(error) = remove() {
        if renamed && let Some(staged) = &staged_path {
            fs::rename(staged, path).map_err(|restore_error| {
                format!(
                    "{error}; restore the preserved database from {}: {restore_error}",
                    staged.display()
                )
            })?;
        }
        if release_current {
            *active_store = ActiveStore::Sqlite(sessions::open_existing_database(path).map_err(
                |restore_error| format!("{error}; could not reopen saved crawl: {restore_error}"),
            )?);
        }
        return Err(error);
    }
    if current.as_deref() == Some(session_id) {
        *current = None;
        *active_store = ActiveStore::memory();
    }
    if let Some(staged) = staged_path
        && let Err(error) = fs::remove_file(&staged)
    {
        eprintln!(
            "saved card removed; database cleanup can be retried at {}: {error}",
            staged.display()
        );
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
        session: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
async fn open_database_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<DatabaseLocation, String> {
    let task = state.crawl_task.lock().await;
    ensure_idle(&state, &task)?;
    let database_path = PathBuf::from(path.trim());
    if path.trim().is_empty() {
        return Err("database path is required".to_string());
    }
    let conn = session_index_connection(&app)?;
    let session = activate_database_path(&state, &conn, &database_path)?;
    Ok(DatabaseLocation {
        path: session.database_path.clone(),
        session: Some(session),
    })
}

fn activate_database_path(
    state: &AppState,
    conn: &Connection,
    path: &Path,
) -> Result<CrawlSession, String> {
    let database_path = fs::canonicalize(path)
        .map_err(|error| format!("crawl database is unavailable: {error}"))?;
    let mut known = query_sessions(conn, None)?.into_iter().find(|session| {
        fs::canonicalize(&session.database_path).is_ok_and(|path| path == database_path)
    });
    if let Some(session) = &mut known {
        sessions::load_config(conn, session)?;
    }
    let mut store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let mut current = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    let database = sessions::open_existing_database(&database_path)?;
    let mut session = match known {
        Some(session) => session,
        None => sessions::register_database(conn, &database_path)?,
    };
    sessions::refresh_legacy_metadata(conn, &mut session)?;
    session.is_current = true;
    *store = ActiveStore::Sqlite(database);
    *current = Some(session.id.clone());
    Ok(session)
}

#[tauri::command]
async fn get_recovery_state(state: State<'_, AppState>) -> Result<CrawlRecoveryState, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned")?
        .clone();
    let frontier = tauri::async_runtime::spawn_blocking(move || store.try_frontier_summary())
        .await
        .map_err(|error| format!("Recovery query worker failed: {error}"))?
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    Ok(CrawlRecoveryState {
        recoverable: frontier.queued > 0,
        queued: frontier.queued,
        seen: frontier.seen,
        crawled: frontier.crawled,
    })
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
    let access_token = search_console_access_token().await?;
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
    let task = state.crawl_task.lock().await;
    ensure_idle(&state, &task)?;
    let site_url = get_integration_setting(&app, GSC_SITE_URL_SETTING)?
        .ok_or_else(|| "Google Search Console site URL is not configured".to_string())?;
    let access_token = search_console_access_token().await?;
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
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let records = query_store_rows(&store, query)?.rows;
    records_to_csv_string(&records).map_err(|error| error.to_string())
}

#[tauri::command]
async fn export_xlsx(state: State<'_, AppState>, query: GridQuery) -> Result<Vec<u8>, String> {
    xlsx_window_bytes(&state, query).await
}

#[tauri::command]
fn export_sitemap(state: State<'_, AppState>, query: GridQuery) -> Result<String, String> {
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let records = query_store_rows(&store, query)?.rows;
    Ok(records_to_sitemap_xml(&records))
}

#[tauri::command]
async fn export_link_edges_csv(state: State<'_, AppState>) -> Result<String, String> {
    complete_link_edges_csv(&state).await
}

async fn complete_link_edges_csv(state: &AppState) -> Result<String, String> {
    with_store_worker(state, true, move |store| {
        let mut output = Vec::new();
        store_link_edges_to_csv(store, &mut output)?;
        String::from_utf8(output).map_err(|error| error.to_string())
    })
    .await
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
async fn export_html_report(state: State<'_, AppState>) -> Result<String, String> {
    with_store_worker(&state, true, move |store| {
        let records = match store {
            ActiveStore::Memory(store) => store.records(),
            ActiveStore::Sqlite(store) => store.try_records().map_err(|error| error.to_string())?,
        };
        let mut output = Vec::new();
        records_to_html_report_store_writer(&records, store, &audit_thresholds(), &mut output)?;
        String::from_utf8(output).map_err(|error| error.to_string())
    })
    .await
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
    validate_grid_query(&query).map_err(|error| error.to_string())?;
    query.offset = 0;
    query.limit = EXPORT_STREAM_PAGE_SIZE;
    let mut row_count = 0;
    let mut wrote_header = false;

    loop {
        let response = query_store_rows(store, query.clone())?;
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
        let response = query_store_link_edges(store, query.clone())?;
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
    let mut query = with_thresholds(GridQuery {
        offset: 0,
        limit: EXPORT_STREAM_PAGE_SIZE,
        ..GridQuery::default()
    });
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
        let response = query_store_sitemap_validation(store, query.clone())?;
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
    validate_grid_query(&query).map_err(|error| error.to_string())?;
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
        let response = query_store_rows(store, query.clone())?;
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

async fn with_store_worker<T: Send + 'static>(
    state: &AppState,
    require_idle: bool,
    work: impl FnOnce(&ActiveStore) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    // Keep lifecycle changes waiting asynchronously while the worker reads this crawl.
    let task = state.crawl_task.lock().await;
    if require_idle
        && task
            .as_ref()
            .is_some_and(|task| !task.inner().is_finished())
    {
        return Err("stop or complete the active crawl before exporting this report".into());
    }
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("the application is closing".into());
    }
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .clone();
    let result = tauri::async_runtime::spawn_blocking(move || work(&store))
        .await
        .map_err(|error| format!("background worker failed: {error}"))?;
    drop(task);
    result
}

fn write_atomic_export(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> Result<usize, String>,
) -> Result<ExportFileResult, String> {
    let directory = path.parent().ok_or("export directory is missing")?;
    let mut file = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| format!("failed to create export file: {error}"))?;
    let row_count = write(file.as_file_mut())?;
    file.as_file()
        .sync_all()
        .map_err(|error| format!("failed to flush export file: {error}"))?;
    file.persist_noclobber(path).map_err(|error| {
        format!("failed to save export file (existing exports are preserved): {error}")
    })?;
    Ok(ExportFileResult {
        path: path.to_string_lossy().into_owned(),
        row_count,
    })
}

async fn export_audit_workbook(
    state: &AppState,
    path: PathBuf,
) -> Result<ExportFileResult, String> {
    with_store_worker(state, true, move |store| {
        write_atomic_export(&path, |file| {
            audit_workbook_to_writer(|query| query_store_rows(store, query), file)
        })
    })
    .await
}

async fn export_html_report_file(
    state: &AppState,
    path: PathBuf,
) -> Result<ExportFileResult, String> {
    with_store_worker(state, true, move |store| {
        write_atomic_export(&path, |file| {
            // ponytail: full record snapshot retains global audit semantics; link edges stream.
            let records = match store {
                ActiveStore::Memory(store) => store.records(),
                ActiveStore::Sqlite(store) => {
                    store.try_records().map_err(|error| error.to_string())?
                }
            };
            let mut writer = BufWriter::new(file);
            records_to_html_report_store_writer(&records, store, &audit_thresholds(), &mut writer)?;
            writer.flush().map_err(|error| error.to_string())?;
            Ok(records.len())
        })
    })
    .await
}

async fn export_crawl_archive_file(
    state: &AppState,
    path: PathBuf,
    timestamp: i64,
) -> Result<ExportFileResult, String> {
    with_store_worker(state, true, move |store| {
        write_atomic_export(&path, |file| {
            write_crawl_archive_stream(store, timestamp, &mut BufWriter::new(file))
        })
    })
    .await
}

fn write_crawl_archive_stream(
    store: &ActiveStore,
    timestamp: i64,
    writer: &mut impl Write,
) -> Result<usize, String> {
    write!(writer, "{{\n  \"schemaVersion\": {CRAWL_ARCHIVE_SCHEMA_VERSION},\n  \"exportedAtMs\": {timestamp},\n  \"records\": ")
        .map_err(|error| error.to_string())?;
    let row_count = match store {
        ActiveStore::Memory(memory) => {
            // ponytail: keep one Memory snapshot in insertion order; add a raw record visitor
            // if large headless archives need bounded hydration. Grid paging repeats full audits.
            let records = memory.records();
            serde_json::to_writer(&mut *writer, &records).map_err(|error| error.to_string())?;
            records.len()
        }
        ActiveStore::Sqlite(_) => write_archive_array(writer, "records", |offset| {
            let response = query_store_rows(
                store,
                GridQuery {
                    offset,
                    limit: EXPORT_STREAM_PAGE_SIZE,
                    ..GridQuery::default()
                },
            )?;
            Ok((response.rows, response.total))
        })?,
    };
    writer
        .write_all(b",\n  \"linkEdges\": ")
        .map_err(|error| error.to_string())?;
    write_archive_array(writer, "link edges", |offset| {
        let response = query_store_link_edges(
            store,
            LinkEdgeQuery {
                offset,
                limit: EXPORT_STREAM_PAGE_SIZE,
                ..LinkEdgeQuery::default()
            },
        )?;
        Ok((response.edges, response.total))
    })?;
    writer
        .write_all(b",\n  \"imageAssets\": ")
        .map_err(|error| error.to_string())?;
    write_archive_array(writer, "image assets", |offset| {
        let response = query_store_images(
            store,
            ImageAssetQuery {
                offset,
                limit: EXPORT_STREAM_PAGE_SIZE,
                ..ImageAssetQuery::default()
            },
        )?;
        Ok((response.images, response.total))
    })?;
    writer
        .write_all(b",\n  \"pageReferences\": ")
        .map_err(|error| error.to_string())?;
    write_archive_array(writer, "page references", |offset| {
        let response = query_store_page_references(
            store,
            PageReferenceQuery {
                offset,
                limit: EXPORT_STREAM_PAGE_SIZE,
                ..PageReferenceQuery::default()
            },
        )?;
        Ok((response.references, response.total))
    })?;
    writer
        .write_all(b",\n  \"pageCaptures\": ")
        .map_err(|error| error.to_string())?;
    write_archive_array_with_page_size(writer, "page captures", MAX_CAPTURE_PAGE_SIZE, |offset| {
        let response = store
            .try_page_captures(PageCaptureQuery {
                offset,
                limit: MAX_CAPTURE_PAGE_SIZE,
                ..Default::default()
            })
            .map_err(|error| error.to_string())?;
        Ok((response.captures, response.total))
    })?;
    // ponytail: hydrate the whole frontier; extend its visitor with seen/crawled metadata
    // if large resume snapshots need bounded memory.
    let frontier = match store {
        ActiveStore::Memory(memory) => memory.load_frontier_state(),
        ActiveStore::Sqlite(sqlite) => sqlite
            .try_load_frontier_state()
            .map_err(|error| error.to_string())?,
    };
    writer
        .write_all(b",\n  \"frontierState\": ")
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut *writer, &frontier).map_err(|error| error.to_string())?;
    writer
        .write_all(b"\n}\n")
        .map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())?;
    Ok(row_count)
}

fn write_archive_array<T: Serialize>(
    writer: &mut impl Write,
    name: &str,
    read_page: impl FnMut(usize) -> Result<(Vec<T>, usize), String>,
) -> Result<usize, String> {
    write_archive_array_with_page_size(writer, name, EXPORT_STREAM_PAGE_SIZE, read_page)
}

fn write_archive_array_with_page_size<T: Serialize>(
    writer: &mut impl Write,
    name: &str,
    page_size: usize,
    mut read_page: impl FnMut(usize) -> Result<(Vec<T>, usize), String>,
) -> Result<usize, String> {
    writer.write_all(b"[").map_err(|error| error.to_string())?;
    let mut offset = 0;
    let mut expected_total = None;
    loop {
        let (rows, current_total) = read_page(offset)?;
        let total = *expected_total.get_or_insert(current_total);
        // The idle worker prevents app writes. Count checks also catch external cardinality
        // changes, but cannot guarantee a snapshot against same-count external edits.
        if current_total != total || rows.len() != total.saturating_sub(offset).min(page_size) {
            return Err(format!(
                "crawl archive {name} changed or ended early while exporting; retry the export"
            ));
        }
        for (index, row) in rows.iter().enumerate() {
            if offset + index != 0 {
                writer.write_all(b",").map_err(|error| error.to_string())?;
            }
            writer.write_all(b"\n").map_err(|error| error.to_string())?;
            serde_json::to_writer(&mut *writer, row).map_err(|error| error.to_string())?;
        }
        offset += rows.len();
        if offset == total {
            writer
                .write_all(b"\n]")
                .map_err(|error| error.to_string())?;
            return Ok(offset);
        }
    }
}

fn query_store_rows(store: &ActiveStore, query: GridQuery) -> Result<GridResponse, String> {
    let query = with_thresholds(query);
    validate_grid_query(&query).map_err(|error| error.to_string())?;
    match store {
        ActiveStore::Memory(store) => Ok(store.query(query)),
        ActiveStore::Sqlite(store) => store.try_query(query).map_err(|error| error.to_string()),
    }
}

fn query_store_images(
    store: &ActiveStore,
    query: ImageAssetQuery,
) -> Result<ImageAssetResponse, String> {
    match store {
        ActiveStore::Memory(store) => Ok(store.image_assets(query)),
        ActiveStore::Sqlite(store) => store
            .try_image_assets(query)
            .map_err(|error| error.to_string()),
    }
}

fn query_store_page_references(
    store: &ActiveStore,
    query: PageReferenceQuery,
) -> Result<PageReferenceResponse, String> {
    store
        .try_page_references(query)
        .map_err(|error| error.to_string())
}

fn query_store_link_edges(
    store: &ActiveStore,
    query: LinkEdgeQuery,
) -> Result<LinkEdgeResponse, String> {
    match store {
        ActiveStore::Memory(store) => Ok(store.link_edges(query)),
        ActiveStore::Sqlite(store) => store
            .try_link_edges(query)
            .map_err(|error| error.to_string()),
    }
}

fn query_store_anchor_texts(
    store: &ActiveStore,
    query: LinkEdgeQuery,
) -> Result<AnchorTextResponse, String> {
    match store {
        ActiveStore::Memory(store) => Ok(store.anchor_texts(query)),
        ActiveStore::Sqlite(store) => store
            .try_anchor_texts(query)
            .map_err(|error| error.to_string()),
    }
}

fn query_store_sitemap_validation(
    store: &ActiveStore,
    query: SitemapValidationQuery,
) -> Result<SitemapValidationResponse, String> {
    match store {
        ActiveStore::Memory(store) => Ok(store.sitemap_validation(query)),
        ActiveStore::Sqlite(store) => store
            .try_sitemap_validation(query)
            .map_err(|error| error.to_string()),
    }
}

async fn export_xlsx_file(
    state: &AppState,
    path: PathBuf,
    query: GridQuery,
) -> Result<ExportFileResult, String> {
    validate_grid_query(&query).map_err(|error| error.to_string())?;
    with_store_worker(state, true, move |store| {
        write_atomic_export(&path, |file| {
            query_to_xlsx_writer(query, |page| query_store_rows(store, page), file)
        })
    })
    .await
}

async fn xlsx_window_bytes(state: &AppState, query: GridQuery) -> Result<Vec<u8>, String> {
    if query.limit > EXPORT_STREAM_PAGE_SIZE {
        return Err(
            "XLSX byte responses are limited to 10,000 rows; use export_file for larger exports"
                .into(),
        );
    }
    with_store_worker(state, false, move |store| {
        records_to_xlsx_bytes(&query_store_rows(store, query)?.rows)
            .map_err(|error| error.to_string())
    })
    .await
}

fn selected_records(store: &ActiveStore, ids: &[u64]) -> Result<Vec<CrawlRecord>, String> {
    if !(1..=1_000).contains(&ids.len()) {
        return Err("select between 1 and 1,000 rows".into());
    }
    let mut unique = HashSet::new();
    if ids
        .iter()
        .any(|id| *id == 0 || *id > 9_007_199_254_740_991 || !unique.insert(*id))
    {
        return Err("selected row IDs must be unique positive safe integers".into());
    }
    let records = store
        .try_records_by_ids(ids)
        .map_err(|error| error.to_string())?;
    if records.len() != ids.len() {
        return Err(
            "one or more selected rows no longer exist in this crawl; refresh the results".into(),
        );
    }
    Ok(records)
}

async fn export_selected_csv_file(
    state: &AppState,
    path: PathBuf,
    ids: Vec<u64>,
) -> Result<ExportFileResult, String> {
    with_store_worker(state, false, move |store| {
        write_atomic_export(&path, |file| {
            let records = selected_records(store, &ids)?;
            records_to_csv(&records, file).map_err(|error| error.to_string())?;
            Ok(records.len())
        })
    })
    .await
}

async fn selected_csv_text(state: &AppState, ids: Vec<u64>) -> Result<String, String> {
    with_store_worker(state, false, move |store| {
        let records = selected_records(store, &ids)?;
        let mut bytes = vec![0; 4 * 1024 * 1024];
        let mut cursor = Cursor::new(bytes.as_mut_slice());
        records_to_csv(&records, &mut cursor).map_err(|error| match error.kind() {
            csv::ErrorKind::Io(error) if error.kind() == std::io::ErrorKind::WriteZero => {
                "selected CSV exceeds the 4 MiB clipboard limit; export the selected rows to a file"
                    .into()
            }
            _ => error.to_string(),
        })?;
        let length = cursor.position() as usize;
        bytes.truncate(length);
        String::from_utf8(bytes).map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
async fn export_selected_csv(
    state: State<'_, AppState>,
    record_ids: Vec<u64>,
) -> Result<String, String> {
    selected_csv_text(&state, record_ids).await
}

async fn export_queued_urls_file(
    state: &AppState,
    path: PathBuf,
) -> Result<ExportFileResult, String> {
    with_store_worker(state, false, move |store| {
        write_atomic_export(&path, |file| {
            let mut writer = csv::Writer::from_writer(file);
            writer
                .write_record([
                    "url",
                    "depth",
                    "from_sitemap",
                    "storage_key",
                    "list_position",
                    "list_duplicate_index",
                ])
                .map_err(|error| error.to_string())?;
            let count = store
                .try_visit_frontier(|item| {
                    writer
                        .write_record([
                            item.url.clone(),
                            item.depth.to_string(),
                            item.from_sitemap.to_string(),
                            item.storage_key.clone(),
                            item.list_position
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            item.list_duplicate_index.to_string(),
                        ])
                        .map_err(std::io::Error::other)
                })
                .map_err(|error| error.to_string())?;
            writer.flush().map_err(|error| error.to_string())?;
            Ok(count)
        })
    })
    .await
}

async fn export_image_alt_file(
    state: &AppState,
    path: PathBuf,
) -> Result<ExportFileResult, String> {
    with_store_worker(state, true, move |store| {
        write_atomic_export(&path, |file| {
            let mut writer = csv::Writer::from_writer(file);
            writer
                .write_record([
                    "page_url",
                    "image_url",
                    "alt_text",
                    "alt_len",
                    "missing_alt",
                    "alt_too_long",
                    "source_position",
                    "width",
                    "height",
                    "size_bytes",
                    "oversized",
                ])
                .map_err(|error| error.to_string())?;
            let mut query = ImageAssetQuery {
                limit: EXPORT_STREAM_PAGE_SIZE,
                ..ImageAssetQuery::default()
            };
            let mut expected_total = None;
            loop {
                let response = query_store_images(store, query.clone())?;
                let total = *expected_total.get_or_insert(response.total);
                if response.total != total
                    || response.images.len() != (total - query.offset).min(query.limit)
                {
                    return Err("image assets changed while exporting; retry the export".into());
                }
                for image in &response.images {
                    writer
                        .serialize((
                            &image.page_url,
                            &image.image_url,
                            &image.alt_text,
                            image.alt_len,
                            image.missing_alt,
                            image.alt_too_long,
                            image.source_position,
                            image.width,
                            image.height,
                            image.size_bytes,
                            image.oversized,
                        ))
                        .map_err(|error| error.to_string())?;
                }
                query.offset += response.images.len();
                if query.offset == total {
                    writer.flush().map_err(|error| error.to_string())?;
                    return Ok(total);
                }
            }
        })
    })
    .await
}

async fn export_graph_file(
    state: &AppState,
    path: PathBuf,
    query: CrawlGraphQuery,
    kind: ExportFileKind,
) -> Result<ExportFileResult, String> {
    with_store_worker(state, false, move |store| {
        write_atomic_export(&path, |file| {
            let graph = store
                .try_crawl_graph(query)
                .map_err(|error| error.to_string())?;
            match kind {
                ExportFileKind::GraphJson => {
                    serde_json::to_writer_pretty(file, &graph)
                        .map_err(|error| error.to_string())?;
                    Ok(graph.nodes.len())
                }
                ExportFileKind::GraphNodesCsv => {
                    graph_nodes_to_csv(&graph.nodes, file).map_err(|error| error.to_string())?;
                    Ok(graph.nodes.len())
                }
                ExportFileKind::GraphEdgesCsv => {
                    link_edges_to_csv(&graph.edges, file).map_err(|error| error.to_string())?;
                    Ok(graph.edges.len())
                }
                _ => unreachable!("graph exporter only accepts graph formats"),
            }
        })
    })
    .await
}

fn validate_export_request(request: &ExportFileRequest) -> Result<(), String> {
    if matches!(
        request.kind,
        ExportFileKind::Csv
            | ExportFileKind::Xlsx
            | ExportFileKind::Sitemap
            | ExportFileKind::ResponseHeadersCsv
            | ExportFileKind::RawHtmlCsv
            | ExportFileKind::RenderedHtmlCsv
            | ExportFileKind::VisibleTextCsv
    ) && let Some(query) = &request.query
    {
        validate_grid_query(query).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
async fn export_file(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ExportFileRequest,
) -> Result<ExportFileResult, String> {
    validate_export_request(&request)?;
    let timestamp = now_ms();
    let capture_kind = match &request.kind {
        ExportFileKind::ResponseHeadersCsv => Some((
            "response-headers",
            page_captures::PageCaptureKind::ResponseHeaders,
        )),
        ExportFileKind::RawHtmlCsv => Some(("raw-html", page_captures::PageCaptureKind::RawHtml)),
        ExportFileKind::RenderedHtmlCsv => Some((
            "rendered-html",
            page_captures::PageCaptureKind::RenderedHtml,
        )),
        ExportFileKind::VisibleTextCsv => {
            Some(("visible-text", page_captures::PageCaptureKind::VisibleText))
        }
        _ => None,
    };
    if let Some((name, kind)) = capture_kind {
        let path = export_path(&app, &format!("ferrous-frog-{name}-{timestamp}.csv"))?;
        return page_captures::export_capture_file(
            &state,
            path,
            request.query.unwrap_or_else(export_grid_query),
            kind,
        )
        .await;
    }
    if matches!(&request.kind, ExportFileKind::AuditWorkbook) {
        let path = export_path(&app, &format!("ferrous-frog-audit-{timestamp}.xlsx"))?;
        return export_audit_workbook(&state, path).await;
    }
    if matches!(&request.kind, ExportFileKind::HtmlReport) {
        let path = export_path(&app, &format!("ferrous-frog-seo-report-{timestamp}.html"))?;
        return export_html_report_file(&state, path).await;
    }
    if matches!(&request.kind, ExportFileKind::CrawlArchive) {
        let path = export_path(
            &app,
            &format!("ferrous-frog-crawl-archive-{timestamp}.ffcrawl.json"),
        )?;
        return export_crawl_archive_file(&state, path, timestamp).await;
    }
    if matches!(&request.kind, ExportFileKind::Xlsx) {
        let path = export_path(&app, &format!("ferrous-frog-export-{timestamp}.xlsx"))?;
        return export_xlsx_file(
            &state,
            path,
            request.query.unwrap_or_else(export_grid_query),
        )
        .await;
    }
    if matches!(&request.kind, ExportFileKind::SelectedCsv) {
        let path = export_path(&app, &format!("ferrous-frog-selected-{timestamp}.csv"))?;
        return export_selected_csv_file(&state, path, request.record_ids.unwrap_or_default())
            .await;
    }
    if matches!(&request.kind, ExportFileKind::QueuedUrlsCsv) {
        let path = export_path(&app, &format!("ferrous-frog-queued-{timestamp}.csv"))?;
        return export_queued_urls_file(&state, path).await;
    }
    if matches!(&request.kind, ExportFileKind::ImageAltCsv) {
        let path = export_path(&app, &format!("ferrous-frog-image-alt-{timestamp}.csv"))?;
        return export_image_alt_file(&state, path).await;
    }
    if let Some(filename) = match &request.kind {
        ExportFileKind::GraphJson => Some(format!("ferrous-frog-graph-{timestamp}.json")),
        ExportFileKind::GraphNodesCsv => Some(format!("ferrous-frog-graph-nodes-{timestamp}.csv")),
        ExportFileKind::GraphEdgesCsv => Some(format!("ferrous-frog-graph-edges-{timestamp}.csv")),
        _ => None,
    } {
        let path = export_path(&app, &filename)?;
        return export_graph_file(
            &state,
            path,
            request.graph_query.unwrap_or_default(),
            request.kind,
        )
        .await;
    }
    if let Some(result) = stream_export_file(&app, &state, &request, timestamp)? {
        return Ok(result);
    }

    let (filename, bytes, row_count) = {
        let store = state
            .store
            .lock()
            .map_err(|_| "store lock poisoned".to_string())?;
        match request.kind {
            ExportFileKind::AuditWorkbook
            | ExportFileKind::HtmlReport
            | ExportFileKind::CrawlArchive
            | ExportFileKind::Xlsx
            | ExportFileKind::SelectedCsv
            | ExportFileKind::QueuedUrlsCsv
            | ExportFileKind::ImageAltCsv
            | ExportFileKind::ResponseHeadersCsv
            | ExportFileKind::RawHtmlCsv
            | ExportFileKind::RenderedHtmlCsv
            | ExportFileKind::VisibleTextCsv
            | ExportFileKind::GraphJson
            | ExportFileKind::GraphNodesCsv
            | ExportFileKind::GraphEdgesCsv => unreachable!("scoped export returned earlier"),
            ExportFileKind::Csv => {
                let records = store
                    .query(with_thresholds(
                        request.query.unwrap_or_else(export_grid_query),
                    ))
                    .rows;
                let row_count = records.len();
                let csv = records_to_csv_string(&records).map_err(|error| error.to_string())?;
                (
                    format!("ferrous-frog-export-{timestamp}.csv"),
                    csv.into_bytes(),
                    row_count,
                )
            }
            ExportFileKind::Sitemap => {
                let records = store
                    .query(with_thresholds(
                        request.query.unwrap_or_else(export_grid_query),
                    ))
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
async fn import_crawl_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ImportCrawlArchiveRequest,
) -> Result<CrawlArchiveImportResult, String> {
    let task = state.crawl_task.lock().await;
    ensure_idle(&state, &task)?;
    let dir = app_data_dir(&app)?;
    let _ = request.storage_mode; // Older clients still send this preference.
    let result = tauri::async_runtime::spawn_blocking(move || {
        let file = fs::File::open(request.path.trim())
            .map_err(|error| format!("failed to read crawl archive: {error}"))?;
        let state = app.state::<AppState>();
        import_archive_reader_into_session(&state, &dir, std::io::BufReader::new(file))
    })
    .await
    .map_err(|error| format!("archive import worker failed: {error}"))?;
    drop(task);
    result
}

fn import_archive_reader_into_session(
    state: &AppState,
    dir: &Path,
    reader: impl std::io::Read,
) -> Result<CrawlArchiveImportResult, String> {
    let sessions_dir = dir.join("sessions");
    let created_directory = !sessions_dir.exists();
    let staged = match archive_import::stage_archive_reader(reader, &sessions_dir) {
        Ok(staged) => staged,
        Err(error) => {
            if created_directory {
                // Remove only an empty directory created for this failed import.
                let _ = fs::remove_dir(&sessions_dir);
            }
            return Err(error);
        }
    };
    let mut active_store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?;
    let mut current = state
        .current_session_id
        .lock()
        .map_err(|_| "session lock poisoned".to_string())?;
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("the application is closing".into());
    }
    let conn = sessions::index_connection(dir)?;
    let (session, store) = sessions::publish_imported_session(
        &conn,
        dir,
        &staged.path,
        &staged.start_url,
        staged.mode,
        staged.records,
    )?;
    let result = CrawlArchiveImportResult {
        session: session.clone(),
        records: staged.records,
        link_edges: staged.link_edges,
        image_assets: staged.image_assets,
        frontier_items: staged.frontier_items,
    };
    *active_store = ActiveStore::Sqlite(store);
    *current = Some(session.id);
    Ok(result)
}

#[cfg(test)]
fn import_archive_into_session(
    state: &AppState,
    dir: &Path,
    archive: CrawlArchive,
) -> Result<CrawlArchiveImportResult, String> {
    // Existing small roundtrip fixtures exercise the same decoder as the native command.
    let bytes = serde_json::to_vec(&archive).map_err(|error| error.to_string())?;
    import_archive_reader_into_session(state, dir, bytes.as_slice())
}

async fn prepare_crawl_comparison(
    state: &AppState,
    directory: PathBuf,
    request: OpenCrawlComparisonRequest,
) -> Result<ComparisonPage, String> {
    enum Source {
        Saved(String, String),
        Archive(String),
    }
    let source = match (
        request.baseline_session_id,
        request.current_session_id,
        request.archive_path,
    ) {
        (Some(baseline), Some(current), None) if baseline != current => {
            Source::Saved(baseline, current)
        }
        (None, None, Some(path)) if !path.trim().is_empty() => Source::Archive(path),
        _ => return Err("choose two different saved crawls or one archive path".into()),
    };
    let id = request.comparison_id;
    let generation = state
        .comparison
        .lock()
        .map_err(|_| "comparison lock poisoned")?
        .reserve(&id)?;
    let result = async {
        // Hold lifecycle changes until the source snapshots are complete.
        let task = state.crawl_task.lock().await;
        if state.exit_confirmed.load(Ordering::SeqCst) {
            return Err("the application is closing".into());
        }
        let running = task
            .as_ref()
            .is_some_and(|task| !task.inner().is_finished());
        let active_id = if running {
            state
                .current_session_id
                .lock()
                .map_err(|_| "session lock poisoned")?
                .clone()
        } else {
            None
        };
        if running && matches!(source, Source::Archive(_)) {
            return Err("stop or complete the active crawl before comparing its archive".into());
        }
        let current_store = if matches!(source, Source::Archive(_)) {
            Some(
                state
                    .store
                    .lock()
                    .map_err(|_| "store lock poisoned")?
                    .clone(),
            )
        } else {
            None
        };
        let (workspace, page) = tauri::async_runtime::spawn_blocking(move || {
            let sources = match source {
                Source::Saved(baseline_id, current_id) => {
                    if active_id
                        .as_ref()
                        .is_some_and(|id| id == &baseline_id || id == &current_id)
                    {
                        return Err("stop the selected active crawl before comparing it".into());
                    }
                    let index = Connection::open_with_flags(
                        directory.join("ferrous-frog-sessions.sqlite3"),
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )
                    .map_err(|error| format!("failed to read saved crawl library: {error}"))?;
                    let baseline = get_session(&index, &baseline_id, None)?;
                    let current = get_session(&index, &current_id, None)?;
                    let baseline_path = fs::canonicalize(&baseline.database_path)
                        .map_err(|error| error.to_string())?;
                    let current_path = fs::canonicalize(&current.database_path)
                        .map_err(|error| error.to_string())?;
                    if baseline_path == current_path {
                        return Err("these saved cards refer to the same crawl database".into());
                    }
                    comparison_sources::ComparisonSources::saved(&baseline_path, &current_path)?
                }
                Source::Archive(path) => comparison_sources::ComparisonSources::archive(
                    Path::new(path.trim()),
                    current_store
                        .as_ref()
                        .ok_or("current crawl is unavailable")?,
                )?,
            };
            let workspace = ComparisonWorkspace::new(sources)?;
            let page = workspace.query(&ComparisonQuery::default())?;
            Ok::<_, String>((workspace, page))
        })
        .await
        .map_err(|error| format!("comparison preparation failed: {error}"))??;
        drop(task);
        state
            .comparison
            .lock()
            .map_err(|_| "comparison lock poisoned")?
            .publish(&id, generation, workspace)?;
        Ok(page)
    }
    .await;
    if result.is_err() {
        let mut comparison = state
            .comparison
            .lock()
            .map_err(|_| "comparison lock poisoned")?;
        if comparison.generation == generation {
            comparison.close(&id);
        }
    }
    result
}

#[tauri::command(rename_all = "camelCase")]
async fn open_crawl_comparison(
    app: AppHandle,
    state: State<'_, AppState>,
    request: OpenCrawlComparisonRequest,
) -> Result<ComparisonPage, String> {
    prepare_crawl_comparison(&state, app_data_dir(&app)?, request).await
}

async fn with_comparison_worker<T: Send + 'static>(
    state: &AppState,
    id: &str,
    work: impl FnOnce(&ComparisonWorkspace) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let workspace = state
        .comparison
        .lock()
        .map_err(|_| "comparison lock poisoned")?
        .get(id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let workspace = workspace
            .lock()
            .map_err(|_| "comparison workspace lock poisoned")?;
        work(&workspace)
    })
    .await
    .map_err(|error| format!("comparison worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
async fn query_crawl_comparison(
    state: State<'_, AppState>,
    comparison_id: String,
    query: ComparisonQuery,
) -> Result<ComparisonPage, String> {
    with_comparison_worker(&state, &comparison_id, move |workspace| {
        workspace.query(&query)
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
async fn get_crawl_comparison_detail(
    state: State<'_, AppState>,
    comparison_id: String,
    key: i64,
) -> Result<ComparisonDetail, String> {
    with_comparison_worker(&state, &comparison_id, move |workspace| {
        workspace.detail(key)
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
async fn export_crawl_comparison(
    app: AppHandle,
    state: State<'_, AppState>,
    comparison_id: String,
    query: ComparisonQuery,
) -> Result<ExportFileResult, String> {
    let path = export_path(&app, &format!("crawl-comparison-{}.csv", now_ms()))?;
    with_comparison_worker(&state, &comparison_id, move |workspace| {
        write_atomic_export(&path, |file| workspace.write_csv(&query, file))
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
fn close_crawl_comparison(state: State<'_, AppState>, comparison_id: String) -> Result<(), String> {
    state
        .comparison
        .lock()
        .map_err(|_| "comparison lock poisoned")?
        .close(&comparison_id);
    Ok(())
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
    Ok(compare_records(
        &archive.records,
        &current_records,
        request.include_response_only,
    ))
}

#[tauri::command(rename_all = "camelCase")]
async fn compare_crawl_sessions(
    app: AppHandle,
    state: State<'_, AppState>,
    baseline_session_id: String,
    current_session_id: String,
    include_response_only: Option<bool>,
) -> Result<CrawlComparisonResponse, String> {
    let task = state.crawl_task.lock().await;
    let active_id = if task
        .as_ref()
        .is_some_and(|task| !task.inner().is_finished())
    {
        state
            .current_session_id
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?
            .clone()
    } else {
        None
    };
    let dir = app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let conn = sessions::index_connection(&dir)?;
        sessions::compare_sessions(
            &conn,
            &baseline_session_id,
            &current_session_id,
            active_id.as_deref(),
            include_response_only.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| error.to_string())?
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

fn comparison_list_key(storage_key: &str) -> Option<(u32, &str)> {
    let (position, url) = storage_key.strip_prefix("list:")?.split_once(':')?;
    Some((position.parse().ok()?, url))
}

fn comparison_request_url(
    url: Option<&str>,
    storage_key: Option<&str>,
    final_url: Option<&str>,
) -> String {
    let storage_key = storage_key.unwrap_or_default();
    let stored_url = comparison_list_key(storage_key)
        .map(|(_, url)| url)
        .unwrap_or(storage_key);
    let value = url
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| {
            if url::Url::parse(stored_url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
            {
                stored_url
            } else {
                final_url.unwrap_or_default()
            }
        })
        .trim();
    match url::Url::parse(value) {
        Ok(mut url) => {
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => value.to_string(),
    }
}

fn comparison_position(list_position: Option<u32>, storage_key: &str, id: u64) -> u64 {
    list_position
        .or_else(|| comparison_list_key(storage_key).map(|(position, _)| position))
        .map(u64::from)
        .unwrap_or(id)
}

fn comparison_records_by_identity(
    records: &[CrawlRecord],
) -> std::collections::BTreeMap<(String, usize), &CrawlRecord> {
    let mut groups = std::collections::BTreeMap::<String, Vec<&CrawlRecord>>::new();
    for record in records {
        let url = comparison_request_url(
            Some(&record.url),
            Some(&record.storage_key),
            Some(&record.final_url),
        );
        groups.entry(url).or_default().push(record);
    }
    let mut result = std::collections::BTreeMap::new();
    for (url, mut records) in groups {
        let known = records
            .iter()
            .map(|record| record.list_duplicate_index)
            .collect::<HashSet<_>>();
        let use_known_occurrences = !known.contains(&0) && known.len() == records.len();
        records.sort_by(|left, right| {
            comparison_position(left.list_position, &left.storage_key, left.id)
                .cmp(&comparison_position(
                    right.list_position,
                    &right.storage_key,
                    right.id,
                ))
                .then_with(|| left.storage_key.cmp(&right.storage_key))
                .then_with(|| left.id.cmp(&right.id))
        });
        for (index, record) in records.into_iter().enumerate() {
            let occurrence = if use_known_occurrences {
                record.list_duplicate_index as usize
            } else {
                index + 1
            };
            result.insert((url.clone(), occurrence), record);
        }
    }
    result
}

fn compare_records(
    baseline_records: &[CrawlRecord],
    current_records: &[CrawlRecord],
    include_response_only: bool,
) -> CrawlComparisonResponse {
    let baseline_by_identity = comparison_records_by_identity(baseline_records);
    let current_by_identity = comparison_records_by_identity(current_records);

    let mut added = 0;
    let mut removed = 0;
    let mut changed = 0;
    let mut status_changed = 0;
    let mut title_changed = 0;
    let mut meta_description_changed = 0;
    let mut indexability_changed = 0;
    let mut hash_changed = 0;
    let mut content_changed = 0;
    let mut response_only = 0;
    let mut content_unavailable = 0;
    let mut rows = Vec::new();
    let mut response_rows = Vec::new();

    for (identity, current) in &current_by_identity {
        if !baseline_by_identity.contains_key(identity) {
            added += 1;
            push_comparison_row(
                &mut rows,
                comparison_row("added", identity, None, Some(current)),
            );
        }
    }

    for (identity, previous) in &baseline_by_identity {
        match current_by_identity.get(identity) {
            None => {
                removed += 1;
                push_comparison_row(
                    &mut rows,
                    comparison_row("removed", identity, Some(previous), None),
                );
            }
            Some(current) => {
                let mut row = comparison_row("changed", identity, Some(previous), Some(current));
                let has_change =
                    |field: &str| row.changed_fields.iter().any(|changed| changed == field);
                let hash_diff = has_change("responseHash");
                hash_changed += usize::from(hash_diff);
                content_unavailable += usize::from(row.content_comparison == "unavailable");
                if row
                    .changed_fields
                    .iter()
                    .any(|field| field != "responseHash")
                {
                    changed += 1;
                    status_changed += usize::from(has_change("statusCode"));
                    title_changed += usize::from(has_change("title"));
                    meta_description_changed += usize::from(has_change("metaDescription"));
                    indexability_changed += usize::from(has_change("indexability"));
                    content_changed += usize::from(has_change("content"));
                    push_comparison_row(&mut rows, row);
                } else if hash_diff {
                    response_only += 1;
                    if include_response_only {
                        row.change = "responseOnly".into();
                        push_comparison_row(&mut response_rows, row);
                    }
                }
            }
        }
    }

    rows.extend(response_rows);
    rows.sort_by(comparison_row_order);
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
        content_changed,
        response_only,
        content_unavailable,
        rows,
        metric_deltas: comparison_metric_deltas(&baseline_summary, &current_summary),
    }
}

fn push_comparison_row(rows: &mut Vec<CrawlComparisonRow>, row: CrawlComparisonRow) {
    rows.push(row);
    if rows.len() == 2_000 {
        rows.sort_by(comparison_row_order);
        rows.truncate(1_000);
    }
}

fn comparison_row_order(
    left: &CrawlComparisonRow,
    right: &CrawlComparisonRow,
) -> std::cmp::Ordering {
    comparison_change_order(&left.change)
        .cmp(&comparison_change_order(&right.change))
        .then_with(|| left.url.cmp(&right.url))
        .then_with(|| left.occurrence.cmp(&right.occurrence))
}

fn comparison_row(
    change: &str,
    identity: &(String, usize),
    previous: Option<&CrawlRecord>,
    current: Option<&CrawlRecord>,
) -> CrawlComparisonRow {
    let mut changed_fields = Vec::new();
    let mut content_comparison = "notApplicable";
    if let Some((previous, current)) = previous.zip(current) {
        content_comparison = match (
            previous.content_hash.as_deref(),
            current.content_hash.as_deref(),
            previous.content_hash_context.as_deref(),
            current.content_hash_context.as_deref(),
        ) {
            (Some(before), Some(after), Some(old_context), Some(new_context))
                if old_context == new_context =>
            {
                if before == after {
                    "unchanged"
                } else {
                    "changed"
                }
            }
            _ => "unavailable",
        };
        let known_count_diff = |before: Option<usize>, after: Option<usize>| {
            before
                .zip(after)
                .is_some_and(|(before, after)| before != after)
        };
        for (field, differs) in [
            (
                "finalUrl",
                comparison_request_url(Some(&previous.final_url), None, None)
                    != comparison_request_url(Some(&current.final_url), None, None),
            ),
            ("statusCode", previous.status_code != current.status_code),
            (
                "title",
                previous.title != current.title
                    || known_count_diff(previous.title_count, current.title_count),
            ),
            (
                "metaDescription",
                previous.meta_description != current.meta_description
                    || known_count_diff(
                        previous.meta_description_count,
                        current.meta_description_count,
                    ),
            ),
            (
                "indexability",
                previous.indexability != current.indexability
                    || previous.indexability_status != current.indexability_status,
            ),
            (
                "headings",
                previous.h1 != current.h1
                    || previous.h1_count != current.h1_count
                    || previous.h2 != current.h2
                    || previous.h2_count != current.h2_count,
            ),
            (
                "canonical",
                previous.canonical != current.canonical
                    || previous.canonical_count != current.canonical_count,
            ),
            (
                "robotsDirectives",
                previous.meta_robots != current.meta_robots
                    || previous.x_robots_tag != current.x_robots_tag,
            ),
            ("content", content_comparison == "changed"),
            (
                "responseHash",
                previous
                    .response_hash
                    .as_ref()
                    .zip(current.response_hash.as_ref())
                    .is_some_and(|(before, after)| before != after),
            ),
        ] {
            if differs {
                changed_fields.push(field.to_string());
            }
        }
    }
    CrawlComparisonRow {
        url: identity.0.clone(),
        identity_key: format!("{}:{}", identity.1, identity.0),
        occurrence: identity.1,
        change: change.to_string(),
        previous_url: previous.map(|record| record.url.clone()),
        current_url: current.map(|record| record.url.clone()),
        previous_final_url: previous.map(|record| record.final_url.clone()),
        current_final_url: current.map(|record| record.final_url.clone()),
        previous_list_position: previous.and_then(|record| record.list_position),
        current_list_position: current.and_then(|record| record.list_position),
        previous_status_code: previous.and_then(|record| record.status_code),
        current_status_code: current.and_then(|record| record.status_code),
        previous_title: previous.and_then(|record| record.title.clone()),
        current_title: current.and_then(|record| record.title.clone()),
        previous_indexability: previous.map(|record| record.indexability_status.clone()),
        current_indexability: current.map(|record| record.indexability_status.clone()),
        previous_response_hash: previous.and_then(|record| record.response_hash.clone()),
        current_response_hash: current.and_then(|record| record.response_hash.clone()),
        previous_meta_description: previous.and_then(|record| record.meta_description.clone()),
        current_meta_description: current.and_then(|record| record.meta_description.clone()),
        changed_fields,
        content_comparison: content_comparison.into(),
    }
}

fn comparison_change_order(change: &str) -> u8 {
    match change {
        "added" => 0,
        "removed" => 1,
        "changed" => 2,
        "responseOnly" => 3,
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

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))
}

fn session_index_connection(app: &AppHandle) -> Result<Connection, String> {
    sessions::index_connection(&app_data_dir(app)?)
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

/// The connected Google account wins; a manually pasted token remains the fallback.
async fn search_console_access_token() -> Result<String, String> {
    if let Some(token) = google_oauth::access_token().await? {
        return Ok(token);
    }
    read_search_console_access_token()?.ok_or_else(|| {
        "Connect a Google account or paste a Search Console access token in Settings > Integrations"
            .to_string()
    })
}

fn backlink_settings_status(app: &AppHandle) -> Result<BacklinkSettingsStatus, String> {
    let (credential_saved, keyring_available) =
        match Entry::new(KEYRING_SERVICE, BACKLINK_KEYRING_ACCOUNT)
            .and_then(|entry| entry.get_password())
        {
            Ok(value) => (!value.trim().is_empty(), true),
            Err(KeyringError::NoEntry) => (false, true),
            Err(_) => (false, false),
        };
    Ok(BacklinkSettingsStatus {
        endpoint_template: get_integration_setting(app, BACKLINK_ENDPOINT_SETTING)?
            .unwrap_or_default(),
        header_name: get_integration_setting(app, BACKLINK_HEADER_SETTING)?.unwrap_or_default(),
        credential_saved,
        keyring_available,
    })
}

#[tauri::command]
fn get_backlink_settings(app: AppHandle) -> Result<BacklinkSettingsStatus, String> {
    backlink_settings_status(&app)
}

#[tauri::command]
fn save_backlink_settings(
    app: AppHandle,
    request: SaveBacklinkSettingsRequest,
) -> Result<BacklinkSettingsStatus, String> {
    let template = request.endpoint_template.trim().to_string();
    if template.is_empty() {
        delete_integration_setting(&app, BACKLINK_ENDPOINT_SETTING)?;
        delete_integration_setting(&app, BACKLINK_HEADER_SETTING)?;
        if let Ok(entry) = Entry::new(KEYRING_SERVICE, BACKLINK_KEYRING_ACCOUNT) {
            let _ = entry.delete_credential();
        }
        return backlink_settings_status(&app);
    }
    BacklinkEndpointProvider::new(BacklinkEndpointConfig {
        endpoint_template: template.clone(),
        header_name: Some(request.header_name.trim().to_string()),
        header_value: None,
    })
    .map_err(|error| error.to_string())?;
    set_integration_setting(&app, BACKLINK_ENDPOINT_SETTING, &template)?;
    set_integration_setting(&app, BACKLINK_HEADER_SETTING, request.header_name.trim())?;
    if let Some(value) = request.header_value.as_deref().map(str::trim)
        && !value.is_empty()
    {
        if value.len() > 1024 || value.chars().any(char::is_control) {
            return Err("The backlink credential must be at most 1,024 printable bytes".into());
        }
        Entry::new(KEYRING_SERVICE, BACKLINK_KEYRING_ACCOUNT)
            .and_then(|entry| entry.set_password(value))
            .map_err(|_| "Could not save the backlink credential in the OS credential store")?;
    }
    backlink_settings_status(&app)
}

#[tauri::command]
async fn merge_backlink_metrics(
    app: AppHandle,
    state: State<'_, AppState>,
    request: MergeBacklinkMetricsRequest,
) -> Result<BacklinkMergeResult, String> {
    let task = state.crawl_task.lock().await;
    ensure_idle(&state, &task)?;
    let settings = backlink_settings_status(&app)?;
    if settings.endpoint_template.is_empty() {
        return Err("Configure the backlink endpoint in Settings > Integrations first".into());
    }
    let header_value = match Entry::new(KEYRING_SERVICE, BACKLINK_KEYRING_ACCOUNT)
        .and_then(|entry| entry.get_password())
    {
        Ok(value) => Some(value),
        Err(KeyringError::NoEntry) => None,
        Err(_) => {
            return Err("The OS credential store is unavailable. Unlock it and try again".into());
        }
    };
    let provider = BacklinkEndpointProvider::new(BacklinkEndpointConfig {
        endpoint_template: settings.endpoint_template,
        header_name: (!settings.header_name.is_empty()).then_some(settings.header_name),
        header_value,
    })
    .map_err(|error| error.to_string())?;
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .clone();
    let limit = request.max_urls.clamp(1, 5_000);
    let read_store = store.clone();
    let urls: Vec<String> = tauri::async_runtime::spawn_blocking(move || {
        let mut urls: Vec<String> = read_store
            .records()
            .into_iter()
            .filter(|record| {
                record.classification == ferrous_frog_storage::UrlClassification::Internal
                    && ferrous_frog_storage::is_success_html_record(record)
            })
            .map(|record| record.final_url)
            .collect();
        urls.sort();
        urls.dedup();
        urls.truncate(limit);
        urls
    })
    .await
    .map_err(|error| error.to_string())?;
    let mut rows = Vec::new();
    let mut failed_urls = 0;
    let mut first_error = None;
    for url in &urls {
        match provider.fetch_url(url).await {
            Ok(metrics) => rows.push(BacklinkMetricRow {
                url: url.clone(),
                backlinks: metrics.backlinks,
                referring_domains: metrics.referring_domains,
                authority_score: metrics.authority_score,
            }),
            Err(error) => {
                failed_urls += 1;
                first_error.get_or_insert_with(|| format!("{url}: {error}"));
                if failed_urls >= 10 && rows.is_empty() {
                    break;
                }
            }
        }
    }
    let matched_rows =
        tauri::async_runtime::spawn_blocking(move || store.merge_backlink_metrics(rows))
            .await
            .map_err(|error| error.to_string())?;
    drop(task);
    Ok(BacklinkMergeResult {
        requested_urls: urls.len(),
        matched_rows,
        failed_urls,
        first_error,
    })
}

#[tauri::command]
fn get_analytics_property(app: AppHandle) -> Result<Option<String>, String> {
    get_integration_setting(&app, GA4_PROPERTY_SETTING)
}

#[tauri::command]
async fn merge_analytics_metrics(
    app: AppHandle,
    state: State<'_, AppState>,
    request: MergeAnalyticsMetricsRequest,
) -> Result<AnalyticsMergeResult, String> {
    let task = state.crawl_task.lock().await;
    ensure_idle(&state, &task)?;
    let property_id = request
        .property_id
        .trim()
        .trim_start_matches("properties/")
        .to_string();
    if property_id.is_empty() || !property_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Enter the numeric Google Analytics 4 property ID".into());
    }
    set_integration_setting(&app, GA4_PROPERTY_SETTING, &property_id)?;
    let access_token = google_oauth::access_token()
        .await?
        .ok_or("Connect a Google account with Analytics access in Settings > Integrations")?;
    let provider = GoogleAnalyticsProvider::new(AnalyticsConfig {
        property_id: property_id.clone(),
        access_token,
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
    let mut sessions = 0.0;
    let rows = response
        .rows
        .into_iter()
        .filter_map(|row| {
            let metric = row.analytics?;
            sessions += metric.sessions;
            Some(AnalyticsMetricRow {
                url: row.url,
                sessions: metric.sessions,
                engaged_sessions: metric.engaged_sessions,
                conversions: metric.conversions,
                revenue: metric.revenue,
            })
        })
        .collect::<Vec<_>>();
    let fetched_rows = rows.len();
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned".to_string())?
        .clone();
    let matched_rows =
        tauri::async_runtime::spawn_blocking(move || store.merge_analytics_metrics(rows))
            .await
            .map_err(|error| error.to_string())?;
    drop(task);
    Ok(AnalyticsMergeResult {
        property_id,
        fetched_rows,
        matched_rows,
        sessions,
    })
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn export_grid_query() -> GridQuery {
    with_thresholds(GridQuery {
        offset: 0,
        limit: 1_000_000,
        ..GridQuery::default()
    })
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

fn export_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|error| format!("failed to resolve export directory: {error}"))?;
    let dir = base.join("Ferrous Frog").join("exports");
    fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create export directory: {error}"))?;
    Ok(dir)
}

fn export_path(app: &AppHandle, filename: &str) -> Result<PathBuf, String> {
    Ok(export_dir(app)?.join(filename))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state::FLAGS)
                .with_denylist(&["splashscreen"])
                .build(),
        )
        .manage(AppState {
            store: Mutex::new(ActiveStore::memory()),
            control: Mutex::new(None),
            crawl_task: tokio::sync::Mutex::new(None),
            current_session_id: Mutex::new(None),
            comparison: Mutex::new(crate::ComparisonState::default()),
            frontend_ready: AtomicBool::new(false),
            exit_confirmed: AtomicBool::new(false),
        })
        .manage(pagespeed::PageSpeedState::default())
        .manage(google_oauth::GoogleOAuthState::default())
        .manage(ai::AiState::default())
        .manage(audit_reports::AuditReportsState::default())
        .setup(|app| {
            // Start the splash minimum after Tauri has created its windows.
            app.manage(Instant::now());
            let app = app.handle().clone();
            if let Err(error) =
                session_index_connection(&app).and_then(|conn| sessions::mark_interrupted(&conn))
            {
                eprintln!("failed to recover saved crawl history: {error}");
            }
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
            audit_reports::list_audit_reports,
            audit_reports::get_audit_report,
            audit_reports::prepare_audit_report,
            audit_reports::query_audit_report_findings,
            audit_reports::query_audit_report_evidence,
            audit_reports::cancel_audit_report,
            audit_reports::delete_audit_report,
            audit_report_export::export_audit_report,
            audit_report_export::export_audit_report_evidence,
            audit_report_export::export_audit_report_comparison,
            audit_report_ai::preview_audit_report_ai,
            audit_report_ai::get_audit_report_ai,
            audit_report_ai::run_audit_report_ai,
            audit_report_ai::preview_audit_report_comparison_ai,
            audit_report_ai::get_audit_report_comparison_ai,
            audit_report_ai::run_audit_report_comparison_ai,
            audit_report_comparison::prepare_audit_report_comparison,
            audit_report_comparison::list_audit_report_comparisons,
            audit_report_comparison::get_audit_report_comparison,
            audit_report_comparison::query_audit_report_comparison_findings,
            audit_report_comparison::query_audit_report_comparison_evidence,
            audit_report_comparison::delete_audit_report_comparison,
            content::preview_content_area,
            content::preview_custom_extractor,
            updates::check_for_updates,
            get_rendering_status,
            serp::measure_serp_snippet,
            serp::import_serp_snippets,
            serp::export_serp_snippets,
            complete_startup,
            quit_app,
            start_crawl,
            validate_crawl_configuration,
            pause_crawl,
            resume_crawl,
            stop_crawl,
            test_robots_txt,
            test_robots_txt_batch,
            download_robots_txt,
            get_rows,
            set_audit_thresholds,
            validate_result_filters,
            get_url_tree,
            get_issues,
            get_link_edges,
            get_image_assets,
            page_references,
            page_captures::get_page_capture,
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
            http_auth::get_http_auth_status,
            http_auth::save_http_auth_credentials,
            http_auth::clear_http_auth_credentials,
            http_auth::get_form_login_status,
            http_auth::save_form_login_credentials,
            http_auth::clear_form_login_credentials,
            pagespeed::get_page_speed_credential_status,
            pagespeed::save_page_speed_api_key,
            pagespeed::clear_page_speed_api_key,
            pagespeed::run_page_speed,
            pagespeed::run_page_speed_bulk,
            pagespeed::run_field_vitals,
            pagespeed::cancel_page_speed,
            save_search_console_credentials,
            clear_search_console_credentials,
            test_search_console_credentials,
            merge_search_console_metrics,
            get_analytics_property,
            merge_analytics_metrics,
            get_backlink_settings,
            save_backlink_settings,
            merge_backlink_metrics,
            google_oauth::get_google_oauth_status,
            google_oauth::save_google_oauth_client,
            google_oauth::disconnect_google_account,
            google_oauth::connect_google_account,
            ai::get_ai_status,
            ai::save_ai_settings,
            ai::save_ai_api_key,
            ai::clear_ai_api_key,
            ai::run_ai_task,
            export_csv,
            export_selected_csv,
            export_xlsx,
            export_sitemap,
            export_link_edges_csv,
            export_redirect_chains_csv,
            export_html_report,
            export_file,
            import_crawl_archive,
            compare_crawl_archive,
            compare_crawl_sessions,
            open_crawl_comparison,
            query_crawl_comparison,
            get_crawl_comparison_detail,
            export_crawl_comparison,
            close_crawl_comparison,
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

    #[test]
    fn history_exposes_legacy_session_metadata_without_config() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE crawl_sessions (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, start_url TEXT NOT NULL,
                database_path TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
             );
             INSERT INTO crawl_sessions VALUES (
                'old', 'Old crawl', 'https://example.test/', '/missing.sqlite3', 1, 2
             );",
        )
        .unwrap();
        sessions::initialize_index(&conn).unwrap();
        let sessions = query_sessions(&conn, None).unwrap();
        let value = serde_json::to_value(&sessions[0]).unwrap();
        assert_eq!(value["mode"], "spider");
        assert_eq!(value["status"], "unavailable");
        assert_eq!(
            conn.query_row("SELECT status FROM crawl_sessions", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "unknown"
        );
        assert!(value["crawled"].is_null());
        assert!(value.get("config").is_none());
    }

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
            comparison: Mutex::new(crate::ComparisonState::default()),
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
