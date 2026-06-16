use ferrous_frog_analysis::analyze_records;
use ferrous_frog_crawler_core::{CrawlConfig, CrawlControl, CrawlerEvent, crawl};
use ferrous_frog_export::records_to_csv_string;
use ferrous_frog_storage::{GridQuery, GridResponse, Issue, MemoryStore};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

struct AppState {
    store: MemoryStore,
    control: Mutex<Option<CrawlControl>>,
}

#[tauri::command]
async fn start_crawl(
    app: AppHandle,
    state: State<'_, AppState>,
    config: CrawlConfig,
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

    state.store.clear();
    let store = state.store.clone();
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
fn get_rows(state: State<'_, AppState>, query: GridQuery) -> GridResponse {
    state.store.query(query)
}

#[tauri::command]
fn get_issues(state: State<'_, AppState>) -> Vec<Issue> {
    analyze_records(&state.store.records())
}

#[tauri::command]
fn export_csv(state: State<'_, AppState>, query: GridQuery) -> Result<String, String> {
    let records = state.store.query(query).rows;
    records_to_csv_string(&records).map_err(|error| error.to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            store: MemoryStore::new(),
            control: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            start_crawl,
            pause_crawl,
            resume_crawl,
            stop_crawl,
            get_rows,
            get_issues,
            export_csv
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Ferrous Frog");
}
