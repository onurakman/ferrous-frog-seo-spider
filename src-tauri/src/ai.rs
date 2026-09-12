//! AI assistance for the selected URL: a configurable provider, a user-managed API key in the OS
//! credential store, per-minute rate limiting and bounded page text sent with each prompt.

use crate::{
    AppState, KEYRING_SERVICE, get_integration_setting, now_ms, selected_records,
    set_integration_setting,
};
use ferrous_frog_integrations::llm::{
    AiTask, DEFAULT_ANTHROPIC_MODEL, LlmClient, LlmCompletion, LlmConfig, LlmProvider, PageContext,
    parse_intent, parse_meta_description, parse_spelling, prompt_for,
};
use ferrous_frog_storage::{
    AiInsights, AiIntent, AiLanguageIssue, AiMetaDescription, AiSpelling, CrawlRecord,
    is_success_html_record,
};
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Mutex, atomic::Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};

const KEYRING_ACCOUNT: &str = "ai-api-key";
const PROVIDER_SETTING: &str = "ai_provider";
const MODEL_SETTING: &str = "ai_model";
const BASE_URL_SETTING: &str = "ai_base_url";
const RPM_SETTING: &str = "ai_requests_per_minute";
const MAX_CHARS_SETTING: &str = "ai_max_input_chars";
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub struct AiState {
    // ponytail: per-process sliding window; persist it if several windows ever share one key.
    recent: Mutex<VecDeque<Instant>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub provider: LlmProvider,
    pub model: String,
    pub base_url: String,
    pub requests_per_minute: u32,
    pub max_input_chars: usize,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: LlmProvider::Anthropic,
            model: DEFAULT_ANTHROPIC_MODEL.to_string(),
            base_url: String::new(),
            requests_per_minute: 20,
            max_input_chars: 12_000,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    #[serde(flatten)]
    pub settings: AiSettings,
    pub key_saved: bool,
    pub keyring_available: bool,
    pub message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveAiKeyRequest {
    api_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunAiTaskRequest {
    record_id: u64,
    task: AiTask,
}

fn validate_settings(settings: &AiSettings) -> Result<AiSettings, String> {
    let model = settings.model.trim();
    if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err("Choose a model name of at most 200 characters".into());
    }
    let base_url = settings.base_url.trim();
    if !base_url.is_empty()
        && !url::Url::parse(base_url)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
    {
        return Err("The AI base URL must be an absolute HTTP or HTTPS URL".into());
    }
    if settings.provider == LlmProvider::OpenAiCompatible && base_url.is_empty() {
        return Err("OpenAI-compatible providers need a base URL such as https://host/v1".into());
    }
    if !(1..=600).contains(&settings.requests_per_minute) {
        return Err("Requests per minute must be between 1 and 600".into());
    }
    if !(1_000..=200_000).contains(&settings.max_input_chars) {
        return Err("Page text per prompt must be between 1,000 and 200,000 characters".into());
    }
    Ok(AiSettings {
        provider: settings.provider,
        model: model.to_string(),
        base_url: base_url.to_string(),
        requests_per_minute: settings.requests_per_minute,
        max_input_chars: settings.max_input_chars,
    })
}

fn load_settings(app: &AppHandle) -> Result<AiSettings, String> {
    let defaults = AiSettings::default();
    let provider = match get_integration_setting(app, PROVIDER_SETTING)?.as_deref() {
        Some("openAiCompatible") => LlmProvider::OpenAiCompatible,
        _ => LlmProvider::Anthropic,
    };
    Ok(AiSettings {
        provider,
        model: get_integration_setting(app, MODEL_SETTING)?.unwrap_or(defaults.model),
        base_url: get_integration_setting(app, BASE_URL_SETTING)?.unwrap_or_default(),
        requests_per_minute: get_integration_setting(app, RPM_SETTING)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(defaults.requests_per_minute),
        max_input_chars: get_integration_setting(app, MAX_CHARS_SETTING)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(defaults.max_input_chars),
    })
}

fn store_settings(app: &AppHandle, settings: &AiSettings) -> Result<(), String> {
    set_integration_setting(
        app,
        PROVIDER_SETTING,
        match settings.provider {
            LlmProvider::Anthropic => "anthropic",
            LlmProvider::OpenAiCompatible => "openAiCompatible",
        },
    )?;
    set_integration_setting(app, MODEL_SETTING, &settings.model)?;
    set_integration_setting(app, BASE_URL_SETTING, &settings.base_url)?;
    set_integration_setting(app, RPM_SETTING, &settings.requests_per_minute.to_string())?;
    set_integration_setting(
        app,
        MAX_CHARS_SETTING,
        &settings.max_input_chars.to_string(),
    )
}

fn keyring_entry() -> Result<Entry, KeyringError> {
    Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
}

fn validate_key(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 1024
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("The AI API key must contain 1–1024 bytes without whitespace".into());
    }
    Ok(value)
}

fn key_state(result: Result<String, KeyringError>) -> (bool, bool, Option<String>) {
    match result {
        Ok(key) => (validate_key(&key).is_ok(), true, None),
        Err(KeyringError::NoEntry) => (false, true, None),
        Err(_) => (
            false,
            false,
            Some("The OS credential store is unavailable. Unlock it and try again.".into()),
        ),
    }
}

fn status(app: &AppHandle) -> Result<AiStatus, String> {
    let settings = load_settings(app)?;
    let (key_saved, keyring_available, message) =
        key_state(keyring_entry().and_then(|entry| entry.get_password()));
    Ok(AiStatus {
        settings,
        key_saved,
        keyring_available,
        message,
    })
}

/// Sliding one-minute window; returns the seconds to wait when the window is full.
fn admit(recent: &mut VecDeque<Instant>, limit: u32, now: Instant) -> Result<(), u64> {
    while recent
        .front()
        .is_some_and(|first| now.duration_since(*first) >= Duration::from_secs(60))
    {
        recent.pop_front();
    }
    if recent.len() >= limit as usize {
        let wait = Duration::from_secs(60).saturating_sub(now.duration_since(recent[0]));
        return Err(wait.as_secs().max(1));
    }
    recent.push_back(now);
    Ok(())
}

/// Keeps the first `max_chars` characters on a char boundary.
fn bounded_text(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(max_chars).collect()
}

fn page_context(record: &CrawlRecord, html: &str, max_chars: usize) -> Result<PageContext, String> {
    let url = url::Url::parse(&record.final_url).map_err(|_| "invalid final URL".to_string())?;
    // Explicit exclusions switch the parser to its visible-text walk, which drops scripts and styles.
    let content =
        ferrous_frog_parser::ContentSelectors::compile(&ferrous_frog_parser::ContentConfig {
            include_selectors: Vec::new(),
            exclude_selectors: vec!["script".into(), "style".into(), "noscript".into()],
        })
        .map_err(|error| error.to_string())?;
    let signals = ferrous_frog_parser::parse_html_with_content(&url, html, &content);
    Ok(PageContext {
        url: record.final_url.clone(),
        title: signals.title.or_else(|| record.title.clone()),
        meta_description: signals
            .meta_description
            .or_else(|| record.meta_description.clone()),
        h1: signals.h1.or_else(|| record.h1.clone()),
        text: bounded_text(&signals.visible_text, max_chars),
    })
}

async fn fetch_page(url: String) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(ferrous_frog_crawler_core::CrawlConfig::default().user_agent)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("could not fetch the page: {}", error.without_url()))?;
    if !response.status().is_success() {
        return Err(format!("the page returned HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PAGE_BYTES as u64)
    {
        return Err("the page exceeds the 2 MiB limit for AI prompts".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| error.without_url().to_string())?;
    if bytes.len() > MAX_PAGE_BYTES {
        return Err("the page exceeds the 2 MiB limit for AI prompts".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn apply_task(
    insights: &mut AiInsights,
    task: AiTask,
    completion: &LlmCompletion,
) -> Result<(), String> {
    match task {
        AiTask::Intent => {
            let result = parse_intent(&completion.text).map_err(|error| error.to_string())?;
            insights.intent = Some(AiIntent {
                intent: result.intent,
                confidence: result.confidence,
                rationale: result.rationale,
            });
        }
        AiTask::MetaDescription => {
            let result =
                parse_meta_description(&completion.text).map_err(|error| error.to_string())?;
            insights.meta_description = Some(AiMetaDescription {
                draft: result.draft,
                alternatives: result.alternatives,
            });
        }
        AiTask::Spelling => {
            let result = parse_spelling(&completion.text).map_err(|error| error.to_string())?;
            insights.spelling = Some(AiSpelling {
                language: result.language,
                issues: result
                    .issues
                    .into_iter()
                    .map(|issue| AiLanguageIssue {
                        text: issue.text,
                        suggestion: issue.suggestion,
                        kind: issue.kind,
                    })
                    .collect(),
            });
        }
    }
    insights.model = if completion.model.is_empty() {
        insights.model.clone()
    } else {
        completion.model.clone()
    };
    insights.updated_at_ms = now_ms();
    Ok(())
}

async fn run_task_with<F, Fut, G, GFut>(
    state: &AppState,
    ai: &AiState,
    settings: &AiSettings,
    request: RunAiTaskRequest,
    fetch: F,
    complete: G,
) -> Result<AiInsights, String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
    G: FnOnce(String, String) -> GFut,
    GFut: Future<Output = Result<LlmCompletion, String>>,
{
    let task = state
        .crawl_task
        .try_lock()
        .map_err(|_| "The crawl is busy; try the AI task again after the current operation")?;
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("the application is closing".into());
    }
    if task
        .as_ref()
        .is_some_and(|task| !task.inner().is_finished())
    {
        return Err("Stop or complete the active crawl before running AI tasks".into());
    }
    {
        let mut recent = ai
            .recent
            .lock()
            .map_err(|_| "AI rate limiter lock poisoned")?;
        if let Err(wait) = admit(&mut recent, settings.requests_per_minute, Instant::now()) {
            return Err(format!(
                "AI rate limit reached ({} per minute); wait {wait} s",
                settings.requests_per_minute
            ));
        }
    }
    let store = state
        .store
        .lock()
        .map_err(|_| "store lock poisoned")?
        .clone();
    let read_store = store.clone();
    let record_id = request.record_id;
    let record = tauri::async_runtime::spawn_blocking(move || {
        selected_records(&read_store, &[record_id]).map(|mut rows| rows.remove(0))
    })
    .await
    .map_err(|_| "AI record worker failed")??;
    if !is_success_html_record(&record) {
        return Err("AI tasks need a successfully crawled HTML page".into());
    }
    let html = fetch(record.final_url.clone()).await?;
    let context = page_context(&record, &html, settings.max_input_chars)?;
    if context.text.trim().is_empty() {
        return Err("The page has no visible text to analyze".into());
    }
    let (system, user) = prompt_for(request.task, &context);
    let completion = complete(system, user).await?;
    let mut insights = record.ai_insights.clone().unwrap_or_default();
    apply_task(&mut insights, request.task, &completion)?;
    let saved = insights.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .try_save_ai_insights(record_id, saved)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "AI snapshot worker failed")??;
    drop(task);
    Ok(insights)
}

#[tauri::command]
pub async fn get_ai_status(app: AppHandle) -> Result<AiStatus, String> {
    tauri::async_runtime::spawn_blocking(move || status(&app))
        .await
        .map_err(|_| "AI settings worker failed")?
}

#[tauri::command]
pub async fn save_ai_settings(app: AppHandle, settings: AiSettings) -> Result<AiStatus, String> {
    let settings = validate_settings(&settings)?;
    tauri::async_runtime::spawn_blocking(move || {
        store_settings(&app, &settings)?;
        status(&app)
    })
    .await
    .map_err(|_| "AI settings worker failed")?
}

#[tauri::command]
pub async fn save_ai_api_key(
    app: AppHandle,
    request: SaveAiKeyRequest,
) -> Result<AiStatus, String> {
    let key = validate_key(&request.api_key)?.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        keyring_entry()
            .and_then(|entry| entry.set_password(&key))
            .map_err(|_| "Could not save the AI API key in the OS credential store".to_string())?;
        status(&app)
    })
    .await
    .map_err(|_| "AI settings worker failed")?
}

#[tauri::command]
pub async fn clear_ai_api_key(app: AppHandle) -> Result<AiStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match keyring_entry().and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(KeyringError::NoEntry) => status(&app),
            Err(_) => Err("Could not clear the AI API key from the OS credential store".into()),
        }
    })
    .await
    .map_err(|_| "AI settings worker failed")?
}

#[tauri::command]
pub async fn run_ai_task(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    request: RunAiTaskRequest,
) -> Result<AiInsights, String> {
    let settings_app = app.clone();
    let (settings, api_key) = tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(&settings_app)?;
        let api_key = match keyring_entry().and_then(|entry| entry.get_password()) {
            Ok(key) => validate_key(&key).map(str::to_string)?,
            Err(KeyringError::NoEntry) => {
                return Err("Save an API key for the AI provider in Settings > AI".to_string());
            }
            Err(_) => {
                return Err(
                    "The OS credential store is unavailable. Unlock it and try again".to_string(),
                );
            }
        };
        Ok((settings, api_key))
    })
    .await
    .map_err(|_| "AI settings worker failed")??;
    let client = LlmClient::new(LlmConfig {
        provider: settings.provider,
        api_key,
        model: settings.model.clone(),
        base_url: (!settings.base_url.is_empty()).then(|| settings.base_url.clone()),
        max_output_tokens: 2048,
    })
    .map_err(|error| error.to_string())?;
    run_task_with(
        &state,
        &ai,
        &settings,
        request,
        fetch_page,
        |system, user| async move {
            client
                .complete(&system, &user)
                .await
                .map_err(|error| error.to_string())
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{ActiveStore, CrawlStore, SqliteStore};
    use std::sync::atomic::AtomicBool;

    fn app_state(store: ActiveStore) -> AppState {
        AppState {
            store: Mutex::new(store),
            control: Mutex::new(None),
            crawl_task: tokio::sync::Mutex::new(None),
            current_session_id: Mutex::new(None),
            frontend_ready: AtomicBool::new(true),
            exit_confirmed: AtomicBool::new(false),
        }
    }

    #[test]
    fn settings_keys_and_rate_window_are_validated() {
        assert!(validate_settings(&AiSettings::default()).is_ok());
        assert!(
            validate_settings(&AiSettings {
                provider: LlmProvider::OpenAiCompatible,
                ..AiSettings::default()
            })
            .is_err()
        );
        assert!(
            validate_settings(&AiSettings {
                base_url: "ftp://x".into(),
                ..AiSettings::default()
            })
            .is_err()
        );
        assert!(
            validate_settings(&AiSettings {
                requests_per_minute: 0,
                ..AiSettings::default()
            })
            .is_err()
        );
        assert_eq!(
            validate_settings(&AiSettings {
                model: " claude-opus-5 ".into(),
                ..AiSettings::default()
            })
            .unwrap()
            .model,
            "claude-opus-5"
        );
        assert!(validate_key(" sk-ant-1 ").is_ok());
        assert!(validate_key("bad key").is_err());
        let mut recent = VecDeque::new();
        let start = Instant::now();
        assert!(admit(&mut recent, 2, start).is_ok());
        assert!(admit(&mut recent, 2, start + Duration::from_secs(1)).is_ok());
        assert_eq!(
            admit(&mut recent, 2, start + Duration::from_secs(2)),
            Err(58)
        );
        assert!(admit(&mut recent, 2, start + Duration::from_secs(61)).is_ok());
        assert_eq!(bounded_text("  a\n\nb   c ", 3), "a b");
    }

    #[tokio::test]
    async fn tasks_fetch_bounded_text_and_store_results_per_row() {
        let store = ActiveStore::Sqlite(SqliteStore::in_memory().unwrap());
        let mut record = CrawlRecord::pending("https://example.test/guide".into(), 1);
        record.final_url = "https://example.test/guide".into();
        record.status_code = Some(200);
        record.content_type = Some("text/html".into());
        record.indexability_status = "Indexable".into();
        let saved = store.upsert(record);
        let state = app_state(store.clone());
        let ai = AiState::default();
        let settings = AiSettings {
            max_input_chars: 1_000,
            ..AiSettings::default()
        };
        let insights = run_task_with(
            &state,
            &ai,
            &settings,
            RunAiTaskRequest {
                record_id: saved.id,
                task: AiTask::Intent,
            },
            |url| async move {
                assert_eq!(url, "https://example.test/guide");
                Ok("<html><head><title>Guide</title></head><body><h1>The guide</h1><p>Learn how it works.</p><script>ignored()</script></body></html>".into())
            },
            |system, user| async move {
                assert!(system.contains("JSON"));
                assert!(user.contains("Learn how it works.") && !user.contains("ignored()"), "{user}");
                Ok(LlmCompletion {
                    text: r#"{"intent":"informational","confidence":0.8,"rationale":"How-to content."}"#.into(),
                    model: "claude-opus-5".into(),
                    input_tokens: 10,
                    output_tokens: 5,
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(insights.intent.as_ref().unwrap().intent, "informational");
        assert_eq!(insights.model, "claude-opus-5");
        assert_eq!(
            store.records()[0].ai_insights.as_ref().unwrap().intent,
            insights.intent
        );
        // A second task keeps the first result and adds its own.
        let insights = run_task_with(
            &state,
            &ai,
            &settings,
            RunAiTaskRequest {
                record_id: saved.id,
                task: AiTask::MetaDescription,
            },
            |_| async move { Ok("<html><body><p>Learn how it works.</p></body></html>".into()) },
            |_, _| async move {
                Ok(LlmCompletion {
                    text: r#"{"draft":"Learn how it works in five minutes.","alternatives":[]}"#
                        .into(),
                    model: String::new(),
                    input_tokens: 0,
                    output_tokens: 0,
                })
            },
        )
        .await
        .unwrap();
        assert!(insights.intent.is_some() && insights.meta_description.is_some());
        let failed = run_task_with(
            &state,
            &ai,
            &settings,
            RunAiTaskRequest {
                record_id: saved.id,
                task: AiTask::Spelling,
            },
            |_| async move { Err("could not fetch the page: timeout".to_string()) },
            |_, _| async move { unreachable!("no completion without page text") },
        )
        .await
        .unwrap_err();
        assert!(failed.contains("timeout"));
        let limited = AiSettings {
            requests_per_minute: 1,
            ..settings.clone()
        };
        ai.recent.lock().unwrap().clear();
        ai.recent.lock().unwrap().push_back(Instant::now());
        let rate = run_task_with(
            &state,
            &ai,
            &limited,
            RunAiTaskRequest {
                record_id: saved.id,
                task: AiTask::Spelling,
            },
            |_| async move { unreachable!() },
            |_, _| async move { unreachable!() },
        )
        .await
        .unwrap_err();
        assert!(rate.contains("rate limit"), "{rate}");
    }
}
