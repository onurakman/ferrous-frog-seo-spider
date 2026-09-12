use crate::{AppState, KEYRING_SERVICE, now_ms, selected_records};
use ferrous_frog_integrations::{
    FieldFormFactor as ProviderFormFactor, FieldVitalsConfig, FieldVitalsMetrics,
    FieldVitalsProvider, MetricRequest, PageSpeedCategory, PageSpeedConfig, PageSpeedMetrics,
    PageSpeedProvider, PageSpeedStrategy as ProviderStrategy, UrlMetricProvider,
};
use ferrous_frog_storage::{
    CrawlRecord, FieldFormFactor, FieldVitalsSnapshot, PageSpeedSnapshot, PageSpeedStrategy,
    is_success_html_record,
};
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Mutex, atomic::Ordering};
use tauri::State;
use tokio::sync::watch;

const CANCELLED: &str = "PageSpeed measurement cancelled";
const PRE_CANCEL_LIMIT: usize = 32;
const KEYRING_ACCOUNT: &str = "page-speed-api-key";
const KEYRING_UNAVAILABLE: &str =
    "The OS credential store is unavailable. Unlock it and try again.";

#[derive(Default)]
pub struct PageSpeedState {
    inner: Mutex<PageSpeedInner>,
}

#[derive(Default)]
struct PageSpeedInner {
    active: Option<ActiveRequest>,
    pre_cancelled: VecDeque<String>,
    closing: bool,
}

pub struct PageSpeedShutdown<'a> {
    state: &'a PageSpeedState,
    committed: bool,
}

impl PageSpeedShutdown<'_> {
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for PageSpeedShutdown<'_> {
    fn drop(&mut self) {
        if !self.committed
            && let Ok(mut inner) = self.state.inner.lock()
        {
            inner.closing = false;
        }
    }
}

struct ActiveRequest {
    request_id: String,
    cancel: watch::Sender<bool>,
}

struct ActiveRun<'a> {
    state: &'a PageSpeedState,
    request_id: String,
}

impl Drop for ActiveRun<'_> {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.state.inner.lock()
            && inner
                .active
                .as_ref()
                .is_some_and(|run| run.request_id == self.request_id)
        {
            inner.active = None;
        }
    }
}

impl PageSpeedState {
    pub fn begin_shutdown(&self) -> Result<PageSpeedShutdown<'_>, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "PageSpeed state lock poisoned")?;
        if inner.closing {
            return Err("the application is closing".into());
        }
        // Closing admission and cancelling the current future must be one operation.
        inner.closing = true;
        if let Some(run) = &inner.active {
            run.cancel.send_replace(true);
        }
        Ok(PageSpeedShutdown {
            state: self,
            committed: false,
        })
    }

    fn begin(&self, request_id: &str) -> Result<(ActiveRun<'_>, watch::Receiver<bool>), String> {
        validate_request_id(request_id)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "PageSpeed state lock poisoned")?;
        if inner.closing {
            return Err("the application is closing".into());
        }
        if let Some(index) = inner.pre_cancelled.iter().position(|id| id == request_id) {
            inner.pre_cancelled.remove(index);
            return Err(CANCELLED.into());
        }
        if inner.active.is_some() {
            return Err(
                "A PageSpeed measurement is already running; cancel it before starting another"
                    .into(),
            );
        }
        let (cancel, receiver) = watch::channel(false);
        inner.active = Some(ActiveRequest {
            request_id: request_id.into(),
            cancel,
        });
        Ok((
            ActiveRun {
                state: self,
                request_id: request_id.into(),
            },
            receiver,
        ))
    }

    fn cancel(&self, request_id: &str) -> Result<bool, String> {
        validate_request_id(request_id)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "PageSpeed state lock poisoned")?;
        if let Some(run) = inner
            .active
            .as_ref()
            .filter(|run| run.request_id == request_id)
        {
            run.cancel.send_replace(true);
            return Ok(true);
        }
        if let Some(index) = inner.pre_cancelled.iter().position(|id| id == request_id) {
            inner.pre_cancelled.remove(index);
        }
        // IPC can deliver Cancel before the async Run is polled. Retain only the
        // latest 32 unique IDs (at most 4 KiB of ID bytes); older IDs are evicted.
        if inner.pre_cancelled.len() == PRE_CANCEL_LIMIT {
            inner.pre_cancelled.pop_front();
        }
        inner.pre_cancelled.push_back(request_id.into());
        Ok(false)
    }
}

fn validate_request_id(request_id: &str) -> Result<(), String> {
    if request_id.trim().is_empty()
        || request_id.len() > 128
        || request_id.chars().any(char::is_control)
    {
        return Err(
            "PageSpeed request ID must contain 1–128 bytes without control characters".into(),
        );
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunPageSpeedRequest {
    record_id: u64,
    strategy: PageSpeedStrategy,
    request_id: String,
    #[serde(default)]
    categories: Vec<PageSpeedCategory>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunPageSpeedBulkRequest {
    record_ids: Vec<u64>,
    strategy: PageSpeedStrategy,
    request_id: String,
    #[serde(default)]
    categories: Vec<PageSpeedCategory>,
    /// Skip rows that already hold a snapshot for the same strategy.
    #[serde(default)]
    resume: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedBulkFailure {
    record_id: u64,
    error: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedBulkResult {
    measured: usize,
    skipped: usize,
    failed: Vec<PageSpeedBulkFailure>,
    cancelled: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedBulkProgress {
    request_id: String,
    completed: usize,
    total: usize,
    record_id: u64,
    error: Option<String>,
}

const BULK_LIMIT: usize = 500;
const QUOTA_BACKOFF_SECS: [u64; 3] = [2, 8, 30];

fn quota_limited(error: &str) -> bool {
    error.contains("HTTP 429") || error.contains("HTTP 503")
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedCredentialStatus {
    key_saved: bool,
    keyring_available: bool,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveApiKeyRequest {
    api_key: String,
}

fn validate_key(value: &str) -> Result<&str, String> {
    if value.chars().any(char::is_control) {
        return Err("PageSpeed API key must not contain control characters".into());
    }
    let value = value.trim();
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_whitespace) {
        return Err(
            "PageSpeed API key must contain 1–512 bytes without interior whitespace".into(),
        );
    }
    Ok(value)
}

fn key_status(result: Result<String, KeyringError>) -> PageSpeedCredentialStatus {
    match result {
        Ok(key) => {
            let valid = validate_key(&key).is_ok();
            PageSpeedCredentialStatus {
                key_saved: valid,
                keyring_available: true,
                message: (!valid)
                    .then(|| "The saved PageSpeed API key is invalid; replace or clear it.".into()),
            }
        }
        Err(KeyringError::NoEntry) => PageSpeedCredentialStatus {
            key_saved: false,
            keyring_available: true,
            message: None,
        },
        Err(_) => PageSpeedCredentialStatus {
            key_saved: false,
            keyring_available: false,
            message: Some(KEYRING_UNAVAILABLE.into()),
        },
    }
}

fn save_key(
    value: &str,
    write: impl FnOnce(&str) -> Result<(), KeyringError>,
) -> Result<PageSpeedCredentialStatus, String> {
    write(validate_key(value)?)
        .map_err(|_| "Could not save the PageSpeed API key in the OS credential store")?;
    Ok(PageSpeedCredentialStatus {
        key_saved: true,
        keyring_available: true,
        message: None,
    })
}

fn clear_key(result: Result<(), KeyringError>) -> Result<PageSpeedCredentialStatus, String> {
    match result {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(PageSpeedCredentialStatus {
            key_saved: false,
            keyring_available: true,
            message: None,
        }),
        Err(_) => Err("Could not clear the PageSpeed API key from the OS credential store".into()),
    }
}

fn keyring_entry() -> Result<Entry, KeyringError> {
    Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
}

fn saved_key(result: Result<String, KeyringError>) -> Result<Option<String>, String> {
    match result {
        Ok(key) => validate_key(&key).map(|key| Some(key.to_string())),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(_) => Err(KEYRING_UNAVAILABLE.into()),
    }
}

#[tauri::command]
pub async fn get_page_speed_credential_status() -> Result<PageSpeedCredentialStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        key_status(keyring_entry().and_then(|entry| entry.get_password()))
    })
    .await
    .map_err(|_| "PageSpeed credential worker failed".into())
}

#[tauri::command]
pub async fn save_page_speed_api_key(
    request: SaveApiKeyRequest,
) -> Result<PageSpeedCredentialStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        save_key(&request.api_key, |key| keyring_entry()?.set_password(key))
    })
    .await
    .map_err(|_| "PageSpeed credential worker failed")?
}

#[tauri::command]
pub async fn clear_page_speed_api_key() -> Result<PageSpeedCredentialStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        clear_key(keyring_entry().and_then(|entry| entry.delete_credential()))
    })
    .await
    .map_err(|_| "PageSpeed credential worker failed")?
}

fn page_speed_url(row: &CrawlRecord) -> Result<String, String> {
    if !is_success_html_record(row)
        || row.error.is_some()
        || row.status_text == "Blocked by robots.txt"
    {
        return Err(
            "PageSpeed requires a complete successful HTML response without crawl errors".into(),
        );
    }
    let mut url = url::Url::parse(&row.final_url)
        .map_err(|_| "The selected row has no valid final HTTP or HTTPS URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(
            "PageSpeed requires an absolute HTTP or HTTPS URL without embedded credentials".into(),
        );
    }
    url.set_fragment(None);
    Ok(url.into())
}

async fn run_with_fetcher<F, Fut>(
    state: &AppState,
    page_speed: &PageSpeedState,
    request: RunPageSpeedRequest,
    fetch: F,
) -> Result<PageSpeedSnapshot, String>
where
    F: FnOnce(String, PageSpeedStrategy) -> Fut,
    Fut: Future<Output = Result<PageSpeedMetrics, String>>,
{
    let (_active, mut cancel) = page_speed.begin(&request.request_id)?;
    let task = state
        .crawl_task
        .try_lock()
        .map_err(|_| "The crawl is busy; try PageSpeed again after the current operation")?;
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("the application is closing".into());
    }
    if task
        .as_ref()
        .is_some_and(|task| !task.inner().is_finished())
    {
        return Err("Stop or complete the active crawl before running PageSpeed".into());
    }
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned")?
        .clone();
    let read_store = store.clone();
    let record_id = request.record_id;
    let url = tauri::async_runtime::spawn_blocking(move || {
        let rows = selected_records(&read_store, &[record_id])?;
        page_speed_url(&rows[0])
    })
    .await
    .map_err(|_| "PageSpeed record worker failed")??;
    let metrics = tokio::select! {
        biased;
        _ = cancel.changed() => return Err(CANCELLED.into()),
        result = fetch(url.clone(), request.strategy) => result?,
    };
    let snapshot = PageSpeedSnapshot {
        strategy: request.strategy,
        requested_url: url,
        completed_at_ms: now_ms(),
        final_url: metrics.final_url,
        fetched_at: metrics.fetched_at,
        lighthouse_version: metrics.lighthouse_version,
        performance_score: metrics.performance_score,
        accessibility_score: metrics.accessibility_score,
        best_practices_score: metrics.best_practices_score,
        seo_score: metrics.seo_score,
        lcp_ms: metrics.lcp_ms,
        cls: metrics.cls,
        tbt_ms: metrics.tbt_ms,
    };
    // Let a started atomic save finish before releasing the crawl lifecycle guard.
    let result = tauri::async_runtime::spawn_blocking(move || {
        if *cancel.borrow() {
            return Err(CANCELLED.into());
        }
        store
            .try_save_page_speed(record_id, snapshot.clone())
            .map_err(|error| error.to_string())?;
        Ok(snapshot)
    })
    .await
    .map_err(|_| "PageSpeed snapshot worker failed")?;
    drop(task);
    result
}

/// Measures the selected rows one after another with one cancellable request ID.
/// Quota responses (429/503) are retried with backoff; other failures move on to the next row.
async fn run_page_speed_bulk_with_fetcher<F, Fut>(
    state: &AppState,
    page_speed: &PageSpeedState,
    request: RunPageSpeedBulkRequest,
    fetch: F,
    progress: impl Fn(PageSpeedBulkProgress),
) -> Result<PageSpeedBulkResult, String>
where
    F: Fn(String, PageSpeedStrategy, Vec<PageSpeedCategory>) -> Fut,
    Fut: Future<Output = Result<PageSpeedMetrics, String>>,
{
    if request.record_ids.is_empty() || request.record_ids.len() > BULK_LIMIT {
        return Err(format!("select between 1 and {BULK_LIMIT} rows to measure"));
    }
    let (_active, mut cancel) = page_speed.begin(&request.request_id)?;
    let task = state
        .crawl_task
        .try_lock()
        .map_err(|_| "The crawl is busy; try PageSpeed again after the current operation")?;
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("the application is closing".into());
    }
    if task
        .as_ref()
        .is_some_and(|task| !task.inner().is_finished())
    {
        return Err("Stop or complete the active crawl before running PageSpeed".into());
    }
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned")?
        .clone();
    let read_store = store.clone();
    let ids = request.record_ids.clone();
    let rows = tauri::async_runtime::spawn_blocking(move || selected_records(&read_store, &ids))
        .await
        .map_err(|_| "PageSpeed record worker failed")??;
    let total = rows.len();
    let mut result = PageSpeedBulkResult {
        measured: 0,
        skipped: 0,
        failed: Vec::new(),
        cancelled: false,
    };
    for (index, row) in rows.into_iter().enumerate() {
        if *cancel.borrow() {
            result.cancelled = true;
            break;
        }
        let record_id = row.id;
        let report = |error: Option<String>| {
            progress(PageSpeedBulkProgress {
                request_id: request.request_id.clone(),
                completed: index + 1,
                total,
                record_id,
                error,
            })
        };
        if request.resume
            && row
                .page_speed
                .as_ref()
                .is_some_and(|snapshot| snapshot.strategy == request.strategy)
        {
            result.skipped += 1;
            report(None);
            continue;
        }
        let url = match page_speed_url(&row) {
            Ok(url) => url,
            Err(error) => {
                report(Some(error.clone()));
                result
                    .failed
                    .push(PageSpeedBulkFailure { record_id, error });
                continue;
            }
        };
        let mut attempt = 0;
        let outcome = loop {
            let fetched = tokio::select! {
                biased;
                _ = cancel.changed() => Err(CANCELLED.to_string()),
                result = fetch(url.clone(), request.strategy, request.categories.clone()) => result,
            };
            match fetched {
                Err(error) if quota_limited(&error) && attempt < QUOTA_BACKOFF_SECS.len() => {
                    let wait = std::time::Duration::from_secs(QUOTA_BACKOFF_SECS[attempt]);
                    attempt += 1;
                    tokio::select! {
                        biased;
                        _ = cancel.changed() => break Err(CANCELLED.to_string()),
                        _ = tokio::time::sleep(wait) => {}
                    }
                }
                other => break other,
            }
        };
        match outcome {
            Ok(metrics) => {
                let snapshot = PageSpeedSnapshot {
                    strategy: request.strategy,
                    requested_url: url,
                    completed_at_ms: now_ms(),
                    final_url: metrics.final_url,
                    fetched_at: metrics.fetched_at,
                    lighthouse_version: metrics.lighthouse_version,
                    performance_score: metrics.performance_score,
                    accessibility_score: metrics.accessibility_score,
                    best_practices_score: metrics.best_practices_score,
                    seo_score: metrics.seo_score,
                    lcp_ms: metrics.lcp_ms,
                    cls: metrics.cls,
                    tbt_ms: metrics.tbt_ms,
                };
                let save_store = store.clone();
                let saved = tauri::async_runtime::spawn_blocking(move || {
                    save_store
                        .try_save_page_speed(record_id, snapshot)
                        .map_err(|error| error.to_string())
                })
                .await
                .map_err(|_| "PageSpeed snapshot worker failed".to_string())
                .and_then(|inner| inner);
                match saved {
                    Ok(()) => {
                        result.measured += 1;
                        report(None);
                    }
                    Err(error) => {
                        report(Some(error.clone()));
                        result
                            .failed
                            .push(PageSpeedBulkFailure { record_id, error });
                    }
                }
            }
            Err(error) if error == CANCELLED => {
                result.cancelled = true;
                break;
            }
            Err(error) => {
                report(Some(error.clone()));
                result
                    .failed
                    .push(PageSpeedBulkFailure { record_id, error });
            }
        }
    }
    drop(task);
    Ok(result)
}

#[tauri::command]
pub async fn run_page_speed_bulk(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    page_speed: State<'_, PageSpeedState>,
    request: RunPageSpeedBulkRequest,
) -> Result<PageSpeedBulkResult, String> {
    use tauri::Emitter;
    run_page_speed_bulk_with_fetcher(&state, &page_speed, request, fetch_page_speed, |event| {
        let _ = app.emit("page-speed-progress", event);
    })
    .await
}

async fn fetch_page_speed(
    url: String,
    strategy: PageSpeedStrategy,
    categories: Vec<PageSpeedCategory>,
) -> Result<PageSpeedMetrics, String> {
    let api_key = tauri::async_runtime::spawn_blocking(|| {
        saved_key(keyring_entry().and_then(|entry| entry.get_password()))
    })
    .await
    .map_err(|_| "PageSpeed credential worker failed")??;
    let provider = PageSpeedProvider::new(PageSpeedConfig {
        api_key,
        strategy: match strategy {
            PageSpeedStrategy::Mobile => ProviderStrategy::Mobile,
            PageSpeedStrategy::Desktop => ProviderStrategy::Desktop,
        },
        locale: None,
        categories,
    })
    .map_err(|error| error.to_string())?;
    let mut response = provider
        .fetch_metrics(MetricRequest {
            urls: vec![url.clone()],
            date_range: None,
        })
        .await
        .map_err(|error| error.to_string())?;
    if response.rows.len() != 1 || response.rows[0].url != url {
        return Err("PageSpeed returned an unexpected result for the selected URL".into());
    }
    response
        .rows
        .pop()
        .and_then(|row| row.page_speed)
        .ok_or_else(|| "PageSpeed returned no Lighthouse measurement".into())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunFieldVitalsRequest {
    record_id: u64,
    form_factor: FieldFormFactor,
}

/// Fetches Chrome UX Report field data for the selected row and stores the latest snapshot.
async fn run_field_vitals_with_fetcher<F, Fut>(
    state: &AppState,
    request: RunFieldVitalsRequest,
    fetch: F,
) -> Result<FieldVitalsSnapshot, String>
where
    F: FnOnce(String, FieldFormFactor) -> Fut,
    Fut: Future<Output = Result<FieldVitalsMetrics, String>>,
{
    let task = state
        .crawl_task
        .try_lock()
        .map_err(|_| "The crawl is busy; try field data again after the current operation")?;
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("the application is closing".into());
    }
    if task
        .as_ref()
        .is_some_and(|task| !task.inner().is_finished())
    {
        return Err("Stop or complete the active crawl before fetching field data".into());
    }
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned")?
        .clone();
    let read_store = store.clone();
    let record_id = request.record_id;
    let url = tauri::async_runtime::spawn_blocking(move || {
        let rows = selected_records(&read_store, &[record_id])?;
        page_speed_url(&rows[0])
    })
    .await
    .map_err(|_| "Field data record worker failed")??;
    let metrics = fetch(url.clone(), request.form_factor).await?;
    let snapshot = FieldVitalsSnapshot {
        form_factor: request.form_factor,
        requested_url: url,
        completed_at_ms: now_ms(),
        has_data: metrics.has_data,
        lcp_ms_p75: metrics.lcp_ms_p75,
        cls_p75: metrics.cls_p75,
        inp_ms_p75: metrics.inp_ms_p75,
        fcp_ms_p75: metrics.fcp_ms_p75,
        ttfb_ms_p75: metrics.ttfb_ms_p75,
        collection_period_start: metrics.collection_period_start,
        collection_period_end: metrics.collection_period_end,
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        store
            .try_save_field_vitals(record_id, snapshot.clone())
            .map_err(|error| error.to_string())?;
        Ok(snapshot)
    })
    .await
    .map_err(|_| "Field data snapshot worker failed")?;
    drop(task);
    result
}

async fn fetch_field_vitals(
    url: String,
    form_factor: FieldFormFactor,
) -> Result<FieldVitalsMetrics, String> {
    let api_key = tauri::async_runtime::spawn_blocking(|| {
        saved_key(keyring_entry().and_then(|entry| entry.get_password()))
    })
    .await
    .map_err(|_| "PageSpeed credential worker failed")??
    .ok_or_else(|| {
        "Chrome UX Report needs the Google API key saved in Settings > Integrations (with the Chrome UX Report API enabled)".to_string()
    })?;
    let provider = FieldVitalsProvider::new(FieldVitalsConfig {
        api_key,
        form_factor: match form_factor {
            FieldFormFactor::Phone => ProviderFormFactor::Phone,
            FieldFormFactor::Desktop => ProviderFormFactor::Desktop,
            FieldFormFactor::Tablet => ProviderFormFactor::Tablet,
        },
    })
    .map_err(|error| error.to_string())?;
    provider
        .fetch(&url)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn run_field_vitals(
    state: State<'_, AppState>,
    request: RunFieldVitalsRequest,
) -> Result<FieldVitalsSnapshot, String> {
    run_field_vitals_with_fetcher(&state, request, fetch_field_vitals).await
}

#[tauri::command]
pub async fn run_page_speed(
    state: State<'_, AppState>,
    page_speed: State<'_, PageSpeedState>,
    request: RunPageSpeedRequest,
) -> Result<PageSpeedSnapshot, String> {
    let categories = request.categories.clone();
    run_with_fetcher(&state, &page_speed, request, move |url, strategy| {
        fetch_page_speed(url, strategy, categories)
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub fn cancel_page_speed(
    page_speed: State<'_, PageSpeedState>,
    request_id: String,
) -> Result<bool, String> {
    page_speed.cancel(&request_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CrawlControl, CrawlStore};
    use ferrous_frog_storage::{ActiveStore, SqliteStore};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;
    use tokio::sync::oneshot;

    fn state(store: ActiveStore) -> AppState {
        AppState {
            store: Mutex::new(store),
            control: Mutex::new(None),
            crawl_task: tokio::sync::Mutex::new(None),
            current_session_id: Mutex::new(None),
            frontend_ready: AtomicBool::new(true),
            exit_confirmed: AtomicBool::new(false),
        }
    }

    fn page() -> CrawlRecord {
        let mut row = CrawlRecord::pending("https://example.test/original".into(), 1);
        row.final_url = "https://example.test/final#section".into();
        row.status_code = Some(200);
        row.content_type = Some("TEXT/HTML; charset=utf-8".into());
        row.indexability = "Indexable".into();
        row.indexability_status = "Indexable".into();
        row
    }

    fn request(record_id: u64, request_id: &str) -> RunPageSpeedRequest {
        RunPageSpeedRequest {
            record_id,
            strategy: PageSpeedStrategy::Desktop,
            request_id: request_id.into(),
            categories: Vec::new(),
        }
    }

    fn metrics() -> PageSpeedMetrics {
        PageSpeedMetrics {
            performance_score: Some(0.0),
            lcp_ms: Some(0.0),
            tbt_ms: Some(42.0),
            final_url: Some("https://example.test/lighthouse-final".into()),
            fetched_at: Some("2026-09-09T12:34:56.000Z".into()),
            lighthouse_version: Some("13.1.0".into()),
            ..PageSpeedMetrics::default()
        }
    }

    async fn success(
        _url: String,
        _strategy: PageSpeedStrategy,
    ) -> Result<PageSpeedMetrics, String> {
        Ok(metrics())
    }

    #[test]
    fn pagespeed_credentials_validate_before_writing_and_never_echo_keys_or_platform_errors() {
        const SECRET: &str = "PSI_SENTINEL_SECRET";
        let mut saved = String::new();
        let status = save_key(&format!("  {SECRET}  "), |key| {
            saved = key.to_string();
            Ok(())
        })
        .unwrap();
        assert_eq!(saved, SECRET);
        assert!(status.key_saved && status.keyring_available);
        assert!(!serde_json::to_string(&status).unwrap().contains(SECRET));
        for value in [
            String::new(),
            " ".into(),
            format!("{SECRET}\n"),
            format!("{SECRET}\0"),
            format!("{SECRET}\tmore"),
            "x".repeat(513),
        ] {
            let error = save_key(&value, |_| panic!("invalid input reached keyring")).unwrap_err();
            assert!(!error.contains(SECRET));
        }
        save_key(&"x".repeat(512), |_| Ok(())).unwrap();
        let error = save_key(SECRET, |_| {
            Err(KeyringError::Invalid(SECRET.into(), SECRET.into()))
        })
        .unwrap_err();
        assert!(!error.contains(SECRET));
        let status = key_status(Ok(SECRET.into()));
        assert!(status.key_saved && status.keyring_available);
        assert_eq!(status.message, None);
        let status = key_status(Err(KeyringError::NoEntry));
        assert!(!status.key_saved && status.keyring_available);
        assert_eq!(saved_key(Err(KeyringError::NoEntry)).unwrap(), None);
        assert_eq!(
            saved_key(Ok(format!(" {SECRET} "))).unwrap().as_deref(),
            Some(SECRET)
        );
        let status = key_status(Ok(String::new()));
        assert!(!status.key_saved && status.keyring_available && status.message.is_some());
        let status = key_status(Err(KeyringError::Invalid(SECRET.into(), SECRET.into())));
        assert!(!status.key_saved && !status.keyring_available);
        assert!(status.message.is_some());
        assert!(!serde_json::to_string(&status).unwrap().contains(SECRET));
        assert!(
            !saved_key(Err(KeyringError::Invalid(SECRET.into(), SECRET.into())))
                .unwrap_err()
                .contains(SECRET)
        );
        for result in [Ok(()), Err(KeyringError::NoEntry)] {
            let status = clear_key(result).unwrap();
            assert!(!status.key_saved && status.keyring_available);
        }
        assert!(
            !clear_key(Err(KeyringError::Invalid(SECRET.into(), SECRET.into())))
                .unwrap_err()
                .contains(SECRET)
        );
    }

    #[test]
    fn pagespeed_eligibility_uses_complete_successful_html_and_safe_final_url() {
        assert_eq!(
            page_speed_url(&page()).unwrap(),
            "https://example.test/final"
        );
        let mut row = page();
        row.indexability = "Non-indexable".into();
        row.indexability_status = "Meta robots noindex".into();
        row.classification = ferrous_frog_storage::UrlClassification::External;
        assert!(page_speed_url(&row).is_ok());
        for status in [None, Some(301), Some(404), Some(503)] {
            let mut row = page();
            row.status_code = status;
            assert!(page_speed_url(&row).is_err());
        }
        for kind in ["incomplete", "image", "unknown-type", "error", "robots"] {
            let mut row = page();
            match kind {
                "incomplete" => row.indexability_status = "Response body incomplete".into(),
                "image" => row.content_type = Some("image/png".into()),
                "unknown-type" => row.content_type = None,
                "error" => row.error = Some("Rendering failed".into()),
                "robots" => row.status_text = "Blocked by robots.txt".into(),
                _ => unreachable!(),
            }
            assert!(page_speed_url(&row).is_err(), "{kind}");
        }
        for url in [
            "file:///private",
            "data:text/html,test",
            "relative",
            "https://name:PSI_SENTINEL_SECRET@example.test/",
        ] {
            let mut row = page();
            row.final_url = url.into();
            let error = page_speed_url(&row).unwrap_err();
            assert!(!error.contains("PSI_SENTINEL_SECRET"));
        }
        assert!(
            serde_json::from_value::<RunPageSpeedRequest>(
                serde_json::json!({"recordId":1,"strategy":"phone","requestId":"one"})
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn pagespeed_success_and_failed_refresh_preserve_exact_list_occurrence() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            for position in [0, 1] {
                let mut row = page();
                row.storage_key = format!("list:{position}");
                row.list_position = Some(position);
                row.list_duplicate_index = position;
                store.upsert(row);
            }
            let state = state(store.clone());
            let page_speed = PageSpeedState::default();
            let started = now_ms();
            let snapshot = run_with_fetcher(
                &state,
                &page_speed,
                request(2, "success"),
                |url, strategy| async move {
                    assert_eq!(url, "https://example.test/final");
                    assert_eq!(strategy, PageSpeedStrategy::Desktop);
                    Ok(metrics())
                },
            )
            .await
            .unwrap();
            assert_eq!(snapshot.requested_url, "https://example.test/final");
            assert_eq!(snapshot.strategy, PageSpeedStrategy::Desktop);
            assert!(snapshot.completed_at_ms >= started);
            assert_eq!(snapshot.performance_score, Some(0.0));
            assert_eq!(snapshot.tbt_ms, Some(42.0));
            assert_eq!(
                snapshot.final_url.as_deref(),
                Some("https://example.test/lighthouse-final")
            );
            assert_eq!(
                snapshot.fetched_at.as_deref(),
                Some("2026-09-09T12:34:56.000Z")
            );
            assert_eq!(snapshot.lighthouse_version.as_deref(), Some("13.1.0"));
            assert_eq!(snapshot.accessibility_score, None);
            let rows = store.try_records_by_ids(&[1, 2]).unwrap();
            assert!(rows[0].page_speed.is_none());
            assert_eq!(rows[1].page_speed.as_ref(), Some(&snapshot));
            assert_eq!(rows[1].final_url, "https://example.test/final#section");
            let error = run_with_fetcher(
                &state,
                &page_speed,
                request(2, "failed-refresh"),
                |_, _| async { Err("Provider rejected the request".into()) },
            )
            .await
            .unwrap_err();
            assert!(error.contains("Provider rejected"));
            assert_eq!(
                store.try_records_by_ids(&[2]).unwrap()[0]
                    .page_speed
                    .as_ref(),
                Some(&snapshot)
            );
            assert!(state.crawl_task.try_lock().is_ok());
            let mut mobile = request(2, "mobile-refresh");
            mobile.strategy = PageSpeedStrategy::Mobile;
            let snapshot =
                run_with_fetcher(&state, &page_speed, mobile, |_, strategy| async move {
                    assert_eq!(strategy, PageSpeedStrategy::Mobile);
                    Ok(PageSpeedMetrics::default())
                })
                .await
                .unwrap();
            assert_eq!(snapshot.strategy, PageSpeedStrategy::Mobile);
            assert_eq!(snapshot.performance_score, None);
            assert_eq!(snapshot.fetched_at, None);
            assert_eq!(
                store.try_records_by_ids(&[2]).unwrap()[0]
                    .page_speed
                    .as_ref(),
                Some(&snapshot)
            );
        }
    }

    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn pagespeed_early_cancellation_prevents_fetch_and_is_consumed() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            store.upsert(page());
            let state = state(store.clone());
            let page_speed = PageSpeedState::default();
            let previous = run_with_fetcher(&state, &page_speed, request(1, "initial"), success)
                .await
                .unwrap();
            assert!(!page_speed.cancel("before-run").unwrap());
            assert!(!page_speed.cancel("before-run").unwrap());
            let fetched = AtomicBool::new(false);
            let error = run_with_fetcher(
                &state,
                &page_speed,
                request(1, "before-run"),
                |_, _| async {
                    fetched.store(true, Ordering::SeqCst);
                    Ok(metrics())
                },
            )
            .await
            .unwrap_err();
            assert_eq!(error, CANCELLED);
            assert!(!fetched.load(Ordering::SeqCst));
            assert!(state.crawl_task.try_lock().is_ok());
            assert_eq!(
                store.try_records_by_ids(&[1]).unwrap()[0]
                    .page_speed
                    .as_ref(),
                Some(&previous)
            );
            // Duplicate cancellation is one tombstone, consumed by the rejected invocation.
            run_with_fetcher(&state, &page_speed, request(1, "before-run"), success)
                .await
                .unwrap();
        }
    }

    #[test]
    fn pagespeed_pre_cancel_ids_are_validated_unique_and_bounded() {
        let page_speed = PageSpeedState::default();
        for id in [
            String::new(),
            "  ".into(),
            "bad\nrequest".into(),
            "x".repeat(129),
        ] {
            assert!(page_speed.cancel(&id).is_err());
        }
        for index in 0..32 {
            assert!(!page_speed.cancel(&format!("id-{index}")).unwrap());
        }
        // Refreshing a duplicate keeps it recent without consuming another slot.
        assert!(!page_speed.cancel("id-0").unwrap());
        assert!(!page_speed.cancel("id-32").unwrap());
        {
            let _evicted = page_speed.begin("id-1").unwrap();
        }
        assert_eq!(page_speed.begin("id-0").err().unwrap(), CANCELLED);
        for index in 2..=32 {
            assert_eq!(
                page_speed.begin(&format!("id-{index}")).err().unwrap(),
                CANCELLED
            );
        }
        assert!(page_speed.begin("id-0").is_ok());
    }

    #[tokio::test]
    async fn pagespeed_shutdown_gates_new_runs_and_reopens_after_cleanup_failure() {
        let store = ActiveStore::memory();
        store.upsert(page());
        let state = state(store);
        let page_speed = PageSpeedState::default();
        let shutdown = page_speed.begin_shutdown().unwrap();
        assert!(page_speed.begin_shutdown().is_err());
        let error = run_with_fetcher(&state, &page_speed, request(1, "during-quit"), success)
            .await
            .unwrap_err();
        assert!(error.contains("closing"), "{error}");
        assert!(!state.exit_confirmed.load(Ordering::SeqCst));
        drop(shutdown);
        run_with_fetcher(&state, &page_speed, request(1, "abandoned-quit"), success)
            .await
            .unwrap();

        let task = tauri::async_runtime::spawn(std::future::pending());
        task.abort();
        *state.crawl_task.lock().await = Some(task);
        let cleanup: Result<(), String> = async {
            let _shutdown = page_speed.begin_shutdown()?;
            let mut task = state.crawl_task.lock().await;
            crate::stop_active_crawl(&state, &mut task).await?;
            Ok(())
        }
        .await;
        assert!(cleanup.unwrap_err().contains("cleanup"));
        assert!(!state.exit_confirmed.load(Ordering::SeqCst));
        run_with_fetcher(
            &state,
            &page_speed,
            request(1, "after-failed-quit"),
            success,
        )
        .await
        .unwrap();
        page_speed.begin_shutdown().unwrap().commit();
        assert!(
            page_speed
                .begin("after-quit")
                .err()
                .unwrap()
                .contains("closing")
        );
    }

    #[tokio::test]
    async fn pagespeed_cancel_drops_held_provider_and_releases_lifecycle_without_a_second_run() {
        let store = ActiveStore::memory();
        store.upsert(page());
        let state = Arc::new(state(store.clone()));
        let page_speed = Arc::new(PageSpeedState::default());
        for (request_id, cancel_for_quit) in [("first", false), ("second", true)] {
            let (started, ready) = oneshot::channel();
            let dropped = Arc::new(AtomicBool::new(false));
            let worker_state = state.clone();
            let worker_speed = page_speed.clone();
            let marker = dropped.clone();
            let task = tokio::spawn(async move {
                run_with_fetcher(
                    &worker_state,
                    &worker_speed,
                    request(1, request_id),
                    |_, _| async move {
                        let _dropped = Dropped(marker);
                        let _ = started.send(());
                        std::future::pending().await
                    },
                )
                .await
            });
            tokio::time::timeout(Duration::from_secs(1), ready)
                .await
                .unwrap()
                .unwrap();
            assert!(state.crawl_task.try_lock().is_err());
            assert!(state.store.try_lock().is_ok());
            let error = tokio::time::timeout(
                Duration::from_millis(200),
                run_with_fetcher(&state, &page_speed, request(1, "busy"), success),
            )
            .await
            .unwrap()
            .unwrap_err();
            assert!(error.contains("already running"), "{error}");
            assert!(!page_speed.cancel("stale-request").unwrap());
            if request_id == "second" {
                assert!(!page_speed.cancel("first").unwrap());
            }
            assert!(!task.is_finished());
            let shutdown = if cancel_for_quit {
                let shutdown = page_speed.begin_shutdown().unwrap();
                assert!(
                    page_speed
                        .begin("late-quit-run")
                        .err()
                        .unwrap()
                        .contains("closing")
                );
                Some(shutdown)
            } else {
                assert!(page_speed.cancel(request_id).unwrap());
                None
            };
            let error = tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(error, CANCELLED);
            assert!(dropped.load(Ordering::SeqCst));
            assert!(state.crawl_task.try_lock().is_ok());
            if shutdown.is_some() {
                assert!(
                    page_speed
                        .begin("after-provider-cancel")
                        .err()
                        .unwrap()
                        .contains("closing")
                );
            }
            drop(shutdown);
            assert!(!page_speed.cancel(request_id).unwrap());
            assert!(
                store.try_records_by_ids(&[1]).unwrap()[0]
                    .page_speed
                    .is_none()
            );
        }
        run_with_fetcher(&state, &page_speed, request(1, "after-cancel"), success)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn pagespeed_rejects_active_paused_closing_busy_and_missing_rows_before_fetching() {
        let store = ActiveStore::memory();
        store.upsert(page());
        let state = state(store);
        let page_speed = PageSpeedState::default();
        for paused in [false, true] {
            let control = CrawlControl::default();
            if paused {
                control.pause();
            }
            *state.control.lock().unwrap() = Some(control);
            *state.crawl_task.lock().await =
                Some(tauri::async_runtime::spawn(std::future::pending()));
            let error = run_with_fetcher(&state, &page_speed, request(1, "active"), |_, _| async {
                panic!("active crawl reached provider")
            })
            .await
            .unwrap_err();
            assert!(error.contains("active crawl"), "{error}");
            state.crawl_task.lock().await.take().unwrap().abort();
        }
        for (record_id, request_id) in [
            (0, "invalid-id"),
            (2, "missing-id"),
            (u64::MAX, "large-id"),
            (1, ""),
        ] {
            assert!(
                run_with_fetcher(
                    &state,
                    &page_speed,
                    request(record_id, request_id),
                    |_, _| async { panic!("invalid request reached provider") }
                )
                .await
                .is_err()
            );
        }
        let guard = state.crawl_task.lock().await;
        let error = tokio::time::timeout(
            Duration::from_millis(200),
            run_with_fetcher(&state, &page_speed, request(1, "lifecycle-busy"), success),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.contains("busy"), "{error}");
        drop(guard);
        state.exit_confirmed.store(true, Ordering::SeqCst);
        let error = run_with_fetcher(&state, &page_speed, request(1, "closing"), success)
            .await
            .unwrap_err();
        assert!(error.contains("closing"), "{error}");
    }

    #[tokio::test]
    async fn pagespeed_save_failure_preserves_snapshot_and_read_stays_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("crawl.sqlite3");
        let store = ActiveStore::sqlite(&path).unwrap();
        store.upsert(page());
        let mut other = page();
        other.storage_key = "other".into();
        store.upsert(other);
        let external = rusqlite::Connection::open(&path).unwrap();
        external
            .execute(
                "UPDATE crawl_records SET custom_extractions = '{invalid' WHERE id = 2",
                [],
            )
            .unwrap();
        let state = state(store.clone());
        let page_speed = PageSpeedState::default();
        let snapshot = run_with_fetcher(&state, &page_speed, request(1, "initial"), success)
            .await
            .unwrap();
        external.execute_batch("CREATE TRIGGER reject_page_speed BEFORE UPDATE OF page_speed ON crawl_records BEGIN SELECT RAISE(ABORT, 'snapshot save denied'); END;").unwrap();
        let error = run_with_fetcher(&state, &page_speed, request(1, "failed-save"), success)
            .await
            .unwrap_err();
        assert!(error.contains("snapshot save denied"), "{error}");
        assert_eq!(
            store.try_records_by_ids(&[1]).unwrap()[0]
                .page_speed
                .as_ref(),
            Some(&snapshot)
        );
        assert!(state.crawl_task.try_lock().is_ok());
    }
    #[tokio::test]
    async fn field_vitals_snapshots_are_saved_for_the_selected_row() {
        let store = ActiveStore::Sqlite(SqliteStore::in_memory().unwrap());
        let row = store.upsert(page());
        let state = state(store.clone());
        let snapshot = run_field_vitals_with_fetcher(
            &state,
            RunFieldVitalsRequest {
                record_id: row.id,
                form_factor: FieldFormFactor::Desktop,
            },
            |url, form_factor| async move {
                assert_eq!(url, "https://example.test/final");
                assert_eq!(form_factor, FieldFormFactor::Desktop);
                Ok(FieldVitalsMetrics {
                    has_data: true,
                    lcp_ms_p75: Some(1800.0),
                    cls_p75: Some(0.02),
                    inp_ms_p75: Some(120.0),
                    fcp_ms_p75: Some(900.0),
                    ttfb_ms_p75: Some(300.0),
                    collection_period_start: Some("2026-08-15".into()),
                    collection_period_end: Some("2026-09-11".into()),
                    normalized_url: None,
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(snapshot.form_factor, FieldFormFactor::Desktop);
        assert_eq!(store.records()[0].field_vitals, Some(snapshot));
        let missing = run_field_vitals_with_fetcher(
            &state,
            RunFieldVitalsRequest {
                record_id: row.id + 1,
                form_factor: FieldFormFactor::Phone,
            },
            |_, _| async move { Ok(FieldVitalsMetrics::default()) },
        )
        .await;
        assert!(missing.is_err());
        let failed = run_field_vitals_with_fetcher(
            &state,
            RunFieldVitalsRequest {
                record_id: row.id,
                form_factor: FieldFormFactor::Phone,
            },
            |_, _| async move { Err("Chrome UX Report returned HTTP 429".to_string()) },
        )
        .await
        .unwrap_err();
        assert!(failed.contains("429"));
        assert_eq!(
            store.records()[0]
                .field_vitals
                .as_ref()
                .map(|s| s.form_factor),
            Some(FieldFormFactor::Desktop),
            "failures keep the previous snapshot"
        );
    }
    #[tokio::test]
    async fn bulk_measurements_skip_resume_rows_retry_quota_errors_and_report_progress() {
        let store = ActiveStore::Sqlite(SqliteStore::in_memory().unwrap());
        let first = store.upsert(page());
        let mut second_row = page();
        second_row.url = "https://example.test/second".into();
        second_row.final_url = "https://example.test/second".into();
        second_row.storage_key = "https://example.test/second".into();
        let second = store.upsert(second_row);
        let mut third_row = page();
        third_row.url = "https://example.test/third".into();
        third_row.final_url = "https://example.test/third".into();
        third_row.storage_key = "https://example.test/third".into();
        third_row.status_code = Some(404);
        let third = store.upsert(third_row);
        store
            .try_save_page_speed(
                first.id,
                PageSpeedSnapshot {
                    strategy: PageSpeedStrategy::Mobile,
                    requested_url: first.final_url.clone(),
                    completed_at_ms: 1,
                    final_url: None,
                    fetched_at: None,
                    lighthouse_version: None,
                    performance_score: Some(0.5),
                    accessibility_score: None,
                    best_practices_score: None,
                    seo_score: None,
                    lcp_ms: None,
                    cls: None,
                    tbt_ms: None,
                },
            )
            .unwrap();
        let state = state(store.clone());
        let page_speed = PageSpeedState::default();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let progress = Arc::new(Mutex::new(Vec::new()));
        let seen_attempts = attempts.clone();
        let seen_progress = progress.clone();
        let result = run_page_speed_bulk_with_fetcher(
            &state,
            &page_speed,
            RunPageSpeedBulkRequest {
                record_ids: vec![first.id, second.id, third.id],
                strategy: PageSpeedStrategy::Mobile,
                request_id: "bulk-1".into(),
                categories: vec![PageSpeedCategory::Performance],
                resume: true,
            },
            move |url, _, categories| {
                let attempts = seen_attempts.clone();
                async move {
                    assert_eq!(categories, vec![PageSpeedCategory::Performance]);
                    let count = {
                        let mut attempts = attempts.lock().unwrap();
                        attempts.push(url.clone());
                        attempts.iter().filter(|seen| **seen == url).count()
                    };
                    if count == 1 {
                        Err("PageSpeed Insights returned HTTP 429".to_string())
                    } else {
                        Ok(metrics())
                    }
                }
            },
            move |event| seen_progress.lock().unwrap().push(event),
        )
        .await
        .unwrap();
        assert_eq!(result.measured, 1);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].record_id, third.id);
        assert!(!result.cancelled);
        assert_eq!(
            attempts.lock().unwrap().len(),
            2,
            "one quota retry for the measured row"
        );
        let records = store.records();
        assert_eq!(
            records
                .iter()
                .find(|row| row.id == first.id)
                .unwrap()
                .page_speed
                .as_ref()
                .unwrap()
                .performance_score,
            Some(0.5),
            "resume keeps the existing snapshot"
        );
        assert!(
            records
                .iter()
                .find(|row| row.id == second.id)
                .unwrap()
                .page_speed
                .is_some()
        );
        {
            let progress = progress.lock().unwrap();
            assert_eq!(progress.len(), 3);
            assert_eq!(progress[2].completed, 3);
            assert!(progress[2].error.is_some());
        }
        assert!(
            run_page_speed_bulk_with_fetcher(
                &state,
                &page_speed,
                RunPageSpeedBulkRequest {
                    record_ids: Vec::new(),
                    strategy: PageSpeedStrategy::Mobile,
                    request_id: "bulk-2".into(),
                    categories: Vec::new(),
                    resume: false,
                },
                |_, _, _| async { Ok(metrics()) },
                |_| {},
            )
            .await
            .is_err()
        );
    }
}
