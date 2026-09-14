//! Saved report commands. Report files remain independent of crawl/session lifecycle.

use crate::{AppState, app_data_dir, audit_thresholds, get_session};
use ferrous_frog_storage::{
    ActiveStore, AuditEvidenceQuery, AuditEvidenceResponse, AuditFindingQuery,
    AuditFindingResponse, AuditPreparationProgress, AuditReportLanguage, AuditReportRequest,
    AuditReportStore, AuditReportSummary, AuditSourceStatus, AuditThresholds, GridQuery,
};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
pub(super) struct AuditReportsState {
    active: Mutex<Option<(String, Arc<AtomicBool>)>>,
    closing: AtomicBool,
    idle: tokio::sync::Notify,
    // Report file operations serialize separately from the active crawl.
    pub(super) files: tokio::sync::Mutex<()>,
}

pub(super) struct ReportJob<'a> {
    state: &'a AuditReportsState,
    pub(super) cancelled: Arc<AtomicBool>,
}

impl AuditReportsState {
    pub(super) fn begin(&self, request_id: &str) -> Result<ReportJob<'_>, String> {
        validate_id(request_id)?;
        let mut active = self.active.lock().map_err(|_| "report job lock poisoned")?;
        if self.closing.load(Ordering::SeqCst) {
            return Err("The application is closing".into());
        }
        if active.is_some() {
            return Err(
                "An audit report operation is still running; cancel it or wait for completion"
                    .into(),
            );
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        *active = Some((request_id.into(), cancelled.clone()));
        Ok(ReportJob {
            state: self,
            cancelled,
        })
    }

    fn cancel(&self, request_id: &str) -> Result<(), String> {
        validate_id(request_id)?;
        if let Some((id, cancelled)) = self
            .active
            .lock()
            .map_err(|_| "report job lock poisoned")?
            .as_ref()
            && id == request_id
        {
            cancelled.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub(super) async fn cancel_for_shutdown(&self) -> Result<(), String> {
        self.closing.store(true, Ordering::SeqCst);
        loop {
            let idle = self.idle.notified();
            let busy = {
                let active = self.active.lock().map_err(|_| {
                    self.closing.store(false, Ordering::SeqCst);
                    "Report shutdown lock poisoned"
                })?;
                if let Some((_, cancelled)) = active.as_ref() {
                    cancelled.store(true, Ordering::SeqCst);
                    true
                } else {
                    false
                }
            };
            if !busy {
                return Ok(());
            }
            idle.await;
        }
    }
}

impl Drop for ReportJob<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            *active = None;
        }
        self.state.idle.notify_one();
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PrepareAuditReportRequest {
    request_id: String,
    session_id: String,
    title: String,
    language: AuditReportLanguage,
    #[serde(default)]
    scope: GridQuery,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReportProgress {
    request_id: String,
    #[serde(flatten)]
    progress: AuditPreparationProgress,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReportList {
    rows: Vec<AuditReportSummary>,
    total: usize,
}

pub(super) fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(
            "Report identifiers must contain 1–128 letters, digits, underscores or hyphens".into(),
        );
    }
    Ok(())
}

pub(super) fn report_path(directory: &Path, id: &str) -> Result<PathBuf, String> {
    validate_id(id)?;
    Ok(directory
        .join("audit-reports")
        .join(format!("{id}.sqlite3")))
}

pub(super) fn delete_saved_file_with_sidecar(
    database: &Path,
    annotations: &Path,
) -> Result<(), String> {
    match fs::symlink_metadata(annotations) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return fs::remove_file(database)
                .map_err(|error| format!("Could not delete saved audit file: {error}"));
        }
        Err(error) => return Err(format!("Could not inspect saved annotations: {error}")),
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err("Saved annotations are not a regular file".into());
        }
        Ok(_) => (),
    }

    let parent = annotations
        .parent()
        .ok_or("Invalid saved annotations path")?;
    let staging_directory = tempfile::Builder::new()
        .prefix(".audit-delete-")
        .tempdir_in(parent)
        .map_err(|error| format!("Could not stage saved annotations: {error}"))?
        .keep();
    let staged = staging_directory.join("annotations.ai");
    if let Err(error) = fs::rename(annotations, &staged) {
        let _ = fs::remove_dir(&staging_directory);
        return Err(format!("Could not stage saved annotations: {error}"));
    }

    if let Err(delete_error) = fs::remove_file(database) {
        if let Err(restore_error) = fs::rename(&staged, annotations) {
            return Err(format!(
                "Could not delete saved audit file: {delete_error}; could not restore annotations: {restore_error}; annotations remain at {}",
                staged.display()
            ));
        }
        let _ = fs::remove_dir(&staging_directory);
        return Err(format!("Could not delete saved audit file: {delete_error}"));
    }

    fs::remove_file(&staged).map_err(|error| {
        format!(
            "Saved audit file was deleted, but staged annotations remain at {}: {error}",
            staged.display()
        )
    })?;
    fs::remove_dir(&staging_directory).map_err(|error| {
        format!(
            "Saved audit file was deleted, but the staging directory remains at {}: {error}",
            staging_directory.display()
        )
    })
}

pub(super) fn open_report(directory: &Path, id: &str) -> Result<AuditReportStore, String> {
    let report = AuditReportStore::open(report_path(directory, id)?)
        .map_err(|error| format!("Could not open the audit report: {error}"))?;
    if report
        .summary()
        .map_err(|error| error.to_string())?
        .request
        .id
        != id
    {
        return Err("The saved report identifier does not match its filename".into());
    }
    Ok(report)
}

fn list_reports(directory: &Path, offset: usize, limit: usize) -> Result<ReportList, String> {
    if limit == 0 || limit > 100 {
        return Err("Choose a report library page size between 1 and 100".into());
    }
    let directory = directory.join("audit-reports");
    if !directory.exists() {
        return Ok(ReportList {
            rows: Vec::new(),
            total: 0,
        });
    }
    // ponytail: enumerate report filenames for ordering; add a metadata index if report libraries grow large.
    let mut paths = fs::read_dir(&directory)
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "sqlite3")
            && path.is_file()
    });
    paths.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    let total = paths.len();
    let rows = paths
        .iter()
        .skip(offset)
        .take(limit)
        .map(|path| {
            let id = path
                .file_stem()
                .and_then(|id| id.to_str())
                .ok_or("Invalid audit report filename")?;
            open_report(
                directory.parent().ok_or("Invalid audit report directory")?,
                id,
            )?
            .summary()
            .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ReportList { rows, total })
}

fn source_revision(path: &Path) -> Result<i64, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| error.to_string())?
        .query_row(
            "SELECT revision FROM crawl_audit_revision WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

async fn active_crawl_source(
    state: &AppState,
) -> Result<(Option<String>, Option<PathBuf>), String> {
    let lifecycle = state.crawl_task.lock().await;
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("The application is closing".into());
    }
    if lifecycle
        .as_ref()
        .is_some_and(|task| !task.inner().is_finished())
    {
        let id = state
            .current_session_id
            .lock()
            .map_err(|_| "session lock poisoned")?
            .clone();
        let store = state.store.lock().map_err(|_| "store lock poisoned")?;
        let path = match &*store {
            ActiveStore::Sqlite(store) => store
                .try_database_path()
                .map_err(|error| error.to_string())?,
            ActiveStore::Memory(_) => None,
        };
        Ok((id, path))
    } else {
        Ok((None, None))
    }
}

fn prepare_saved_report(
    directory: &Path,
    mut request: PrepareAuditReportRequest,
    thresholds: AuditThresholds,
    active_session_id: Option<&str>,
    active_source_path: Option<&Path>,
    mut progress: impl FnMut(AuditPreparationProgress) -> bool,
) -> Result<AuditReportSummary, String> {
    validate_id(&request.request_id)?;
    if request.title.trim().is_empty()
        || request.title.len() > 256
        || request.title.chars().any(char::is_control)
    {
        return Err(
            "Choose a report title containing 1–256 bytes without control characters".into(),
        );
    }
    let index = Connection::open_with_flags(
        directory.join("ferrous-frog-sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|error| format!("Could not read the saved crawl library: {error}"))?;
    let mut session = get_session(&index, &request.session_id, None)?;
    crate::sessions::load_config(&index, &mut session)?;
    let source_status = match session.status.as_str() {
        "finished" | "completed" => AuditSourceStatus::Completed,
        "imported" => AuditSourceStatus::Imported,
        "stopped" => AuditSourceStatus::Stopped,
        "failed" => AuditSourceStatus::Failed,
        _ => {
            return Err("Stop or complete the selected crawl before preparing its report".into());
        }
    };
    let source_path = Path::new(&session.database_path);
    let canonical_source = fs::canonicalize(source_path).map_err(|error| error.to_string())?;
    if active_session_id == Some(request.session_id.as_str())
        || active_source_path
            .and_then(|path| fs::canonicalize(path).ok())
            .is_some_and(|path| path == canonical_source)
    {
        return Err("Stop or complete the selected crawl before preparing its report".into());
    }
    let initial_revision = source_revision(source_path)?;
    let initial_updated_at = session.updated_at_ms;
    let session_id = session.id.clone();
    let (created_at, suffix): (String, String) = index
        .query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), lower(hex(randomblob(8)))",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let id = format!("report-{}-{suffix}", crate::now_ms());
    let mut exclusions = Vec::new();
    let mut crawl_limits = Vec::new();
    if let Some(config) = &session.config {
        // Explicit allowlist: authentication, headers and automation destinations never enter reports.
        exclusions.push(format!("Discovery settings: {}", serde_json::json!({
            "mode": config.mode, "respectRobots": config.respect_robots,
            "robotsOverrideEnabled": config.use_robots_txt_override,
            "includeUrlPatterns": config.include_url_patterns, "excludeUrlPatterns": config.exclude_url_patterns,
            "subdomainScope": config.subdomain_scope, "folderScope": config.folder_scope,
            "cdnHosts": config.cdn_hosts, "checkLinksOutsideStartFolder": config.check_links_outside_start_folder,
            "followInternalNofollow": config.follow_internal_nofollow.unwrap_or(config.follow_nofollow),
            "followExternalNofollow": config.follow_external_nofollow.unwrap_or(config.follow_nofollow),
            "resourceTypes": config.resource_types, "referenceLinks": config.reference_links,
            "querySettings": config.query_settings, "contentSelectors": config.content,
            "storedEvidence": config.store, "capture": config.capture, "renderingEnabled": config.rendering.enabled
        })));
        crawl_limits.push(format!("Crawl limits: {}", serde_json::json!({
            "maxUrls": config.max_urls, "maxDepth": config.max_depth,
            "maxFolderDepth": config.max_folder_depth, "maxUrlLength": config.max_url_length,
            "maxLinksPerPage": config.max_links_per_page, "maxResponseBytes": config.max_response_bytes,
            "timeoutSeconds": config.timeout_secs, "maxRedirects": config.max_redirects
        })));
    } else {
        exclusions.push(
            "Saved crawl configuration is unavailable; discovery coverage cannot be reconstructed"
                .into(),
        );
    }
    if session.status == "imported" {
        exclusions.push(
            "Imported crawl: capture coverage depends on fields available in the imported data"
                .into(),
        );
    }
    request.scope.thresholds = thresholds;
    let input = AuditReportRequest {
        id: id.clone(),
        title: request.title.trim().into(),
        language: request.language,
        source_session_id: session.id,
        source_revision: session.updated_at_ms.to_string(),
        source_status,
        created_at,
        scope: request.scope,
        exclusions,
        crawl_limits,
    };
    let path = report_path(directory, &id)?;
    fs::create_dir_all(path.parent().ok_or("Invalid report directory")?)
        .map_err(|error| error.to_string())?;
    let mut source_changed = false;
    let report = AuditReportStore::prepare_saved_with_progress(&path, source_path, input, |step| {
        if !progress(step.clone()) {
            return false;
        }
        if step.phase == "publishing" {
            source_changed = get_session(&index, &session_id, None)
                .ok()
                .is_none_or(|current| {
                    current.status != session.status
                        || current.updated_at_ms != initial_updated_at
                        || fs::canonicalize(&current.database_path).ok().as_ref()
                            != Some(&canonical_source)
                        || source_revision(source_path).ok() != Some(initial_revision)
                });
            return !source_changed;
        }
        true
    })
    .map_err(|error| {
        if source_changed {
            "The saved crawl changed during report preparation; create the report again".into()
        } else {
            error.to_string()
        }
    })?;
    report.summary().map_err(|error| error.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn prepare_audit_report(
    app: AppHandle,
    state: State<'_, AppState>,
    reports: State<'_, AuditReportsState>,
    request: PrepareAuditReportRequest,
) -> Result<AuditReportSummary, String> {
    let job = reports.begin(&request.request_id)?;
    let directory = app_data_dir(&app)?;
    let request_id = request.request_id.clone();
    let (active, active_path) = active_crawl_source(&state).await?;
    let cancelled = job.cancelled.clone();
    let thresholds = audit_thresholds();
    let files = reports.files.lock().await;
    let result = tauri::async_runtime::spawn_blocking(move || {
        prepare_saved_report(
            &directory,
            request,
            thresholds,
            active.as_deref(),
            active_path.as_deref(),
            |progress| {
                let _ = app.emit(
                    "audit-report-progress",
                    ReportProgress {
                        request_id: request_id.clone(),
                        progress,
                    },
                );
                !cancelled.load(Ordering::SeqCst)
            },
        )
    })
    .await
    .map_err(|error| format!("Report preparation worker failed: {error}"))?;
    drop(files);
    result
}

#[tauri::command(rename_all = "camelCase")]
pub(super) fn cancel_audit_report(
    reports: State<'_, AuditReportsState>,
    request_id: String,
) -> Result<(), String> {
    reports.cancel(&request_id)
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn list_audit_reports(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    offset: usize,
    limit: usize,
) -> Result<ReportList, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || list_reports(&directory, offset, limit))
        .await
        .map_err(|error| format!("Report library worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn get_audit_report(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    report_id: String,
) -> Result<AuditReportSummary, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        open_report(&directory, &report_id)?
            .summary()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Report worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn query_audit_report_findings(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    report_id: String,
    query: AuditFindingQuery,
) -> Result<AuditFindingResponse, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        open_report(&directory, &report_id)?
            .query_findings(query)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Report findings worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn query_audit_report_evidence(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    report_id: String,
    mut query: AuditEvidenceQuery,
) -> Result<AuditEvidenceResponse, String> {
    // Frontend callers cannot opt out of bounded text previews. Full values belong to native exports.
    query.preview = true;
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        open_report(&directory, &report_id)?
            .query_evidence(query)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Report evidence worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn delete_audit_report(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    report_id: String,
) -> Result<(), String> {
    let _job = reports.begin(&format!("delete-{}", crate::now_ms()))?;
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        drop(open_report(&directory, &report_id)?);
        let annotations = crate::audit_report_ai::annotation_path(&directory, &report_id)?;
        delete_saved_file_with_sidecar(&report_path(&directory, &report_id)?, &annotations)
    })
    .await
    .map_err(|error| format!("Report deletion worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{CrawlRecord, CrawlStore};

    #[test]
    fn failed_database_deletion_restores_report_and_comparison_annotations() {
        for library in ["audit-reports", "audit-comparisons"] {
            let directory = tempfile::tempdir().unwrap();
            let parent = directory.path().join(library);
            fs::create_dir(&parent).unwrap();
            let database = parent.join("saved.sqlite3");
            let annotations = parent.join("saved.ai");
            fs::create_dir(&database).unwrap(); // remove_file fails on a directory on every platform.
            fs::write(&annotations, b"previous generated commentary").unwrap();

            assert!(delete_saved_file_with_sidecar(&database, &annotations).is_err());
            assert!(database.is_dir());
            assert_eq!(
                fs::read(&annotations).unwrap(),
                b"previous generated commentary"
            );

            fs::remove_dir(&database).unwrap();
            fs::write(&database, b"saved database").unwrap();
            delete_saved_file_with_sidecar(&database, &annotations).unwrap();
            assert!(!database.exists());
            assert!(!annotations.exists());
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_annotations_do_not_delete_saved_audit_file() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("saved.sqlite3");
        let target = directory.path().join("other.ai");
        let annotations = directory.path().join("saved.ai");
        fs::write(&database, b"saved database").unwrap();
        fs::write(&target, b"other commentary").unwrap();
        std::os::unix::fs::symlink(&target, &annotations).unwrap();

        assert!(delete_saved_file_with_sidecar(&database, &annotations).is_err());
        assert_eq!(fs::read(&database).unwrap(), b"saved database");
        assert_eq!(fs::read(&target).unwrap(), b"other commentary");
    }

    fn request(session_id: &str) -> PrepareAuditReportRequest {
        PrepareAuditReportRequest {
            request_id: "request-1".into(),
            session_id: session_id.into(),
            title: "Saved evidence".into(),
            language: AuditReportLanguage::English,
            scope: GridQuery::default(),
        }
    }

    #[test]
    fn saved_reports_survive_source_deletion_and_keep_private_paths_and_thresholds() {
        let dir = tempfile::tempdir().unwrap();
        let index = crate::sessions::index_connection(dir.path()).unwrap();
        let (session, store) = crate::sessions::create_session(
            &index,
            dir.path(),
            "Source",
            "https://example.test",
            None,
            "finished",
        )
        .unwrap();
        let mut record = CrawlRecord::pending("https://example.test/page".into(), 0);
        record.status_code = Some(200);
        record.content_type = Some("text/html".into());
        store.upsert(record);
        let summary = prepare_saved_report(
            dir.path(),
            request(&session.id),
            AuditThresholds::default(),
            None,
            None,
            |_| true,
        )
        .unwrap();
        assert_eq!(summary.scope_records, 1);
        assert_eq!(summary.status, "ready");
        assert_eq!(summary.request.source_status, AuditSourceStatus::Completed);
        drop(store);
        fs::remove_file(session.database_path).unwrap();
        let listed = list_reports(dir.path(), 0, 10).unwrap();
        assert_eq!(listed.total, 1);
        assert_eq!(listed.rows[0].request.id, summary.request.id);
        assert_eq!(
            open_report(dir.path(), &summary.request.id)
                .unwrap()
                .query_evidence(AuditEvidenceQuery {
                    finding_id: "title.missing".into(),
                    ..Default::default()
                })
                .unwrap()
                .total,
            1
        );
        for id in ["../source", "", "a/b", "a\\b"] {
            assert!(report_path(dir.path(), id).is_err());
        }
        assert!(list_reports(dir.path(), 0, 101).is_err());
    }

    #[test]
    fn cancellation_and_active_sources_never_publish_a_report() {
        let dir = tempfile::tempdir().unwrap();
        let index = crate::sessions::index_connection(dir.path()).unwrap();
        let (session, _store) = crate::sessions::create_session(
            &index,
            dir.path(),
            "Source",
            "https://example.test",
            None,
            "finished",
        )
        .unwrap();
        assert!(
            prepare_saved_report(
                dir.path(),
                request(&session.id),
                AuditThresholds::default(),
                Some(&session.id),
                None,
                |_| true
            )
            .is_err()
        );
        assert!(
            prepare_saved_report(
                dir.path(),
                request(&session.id),
                AuditThresholds::default(),
                None,
                None,
                |_| false
            )
            .is_err()
        );
        assert_eq!(list_reports(dir.path(), 0, 10).unwrap().total, 0);
        let state = AuditReportsState::default();
        let job = state.begin("request-1").unwrap();
        assert!(state.begin("request-2").is_err());
        state.cancel("request-2").unwrap();
        assert!(!job.cancelled.load(Ordering::SeqCst));
        state.cancel("request-1").unwrap();
        assert!(job.cancelled.load(Ordering::SeqCst));
        drop(job);
        assert!(state.begin("request-2").is_ok());
    }

    #[tokio::test]
    async fn shutdown_cancels_and_waits_for_report_cleanup_before_allowing_exit() {
        let reports = AuditReportsState::default();
        let job = reports.begin("pending-report").unwrap();
        let cleanup = async {
            while !job.cancelled.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            assert!(reports.begin("late-report").is_err());
            drop(job);
        };
        let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::join!(reports.cancel_for_shutdown(), cleanup)
        })
        .await
        .unwrap();
        result.unwrap();
        assert!(reports.active.lock().unwrap().is_none());
    }

    #[test]
    fn report_rejects_ready_and_changed_sources_before_publication() {
        let dir = tempfile::tempdir().unwrap();
        let index = crate::sessions::index_connection(dir.path()).unwrap();
        let (session, store) = crate::sessions::create_session(
            &index,
            dir.path(),
            "Source",
            "https://example.test",
            None,
            "ready",
        )
        .unwrap();
        assert!(
            prepare_saved_report(
                dir.path(),
                request(&session.id),
                AuditThresholds::default(),
                None,
                None,
                |_| true,
            )
            .is_err()
        );
        crate::sessions::save_status(&index, &session.id, "finished").unwrap();
        let mut changed = false;
        let error = prepare_saved_report(
            dir.path(),
            request(&session.id),
            AuditThresholds::default(),
            None,
            None,
            |step| {
                if step.phase == "publishing" && !changed {
                    changed = true;
                    store.upsert(CrawlRecord::pending(
                        "https://example.test/changed".into(),
                        0,
                    ));
                }
                true
            },
        )
        .unwrap_err();
        assert!(
            error.contains("changed during report preparation"),
            "{error}"
        );
        assert_eq!(list_reports(dir.path(), 0, 10).unwrap().total, 0);
        crate::sessions::save_status(&index, &session.id, "imported").unwrap();
        let imported = prepare_saved_report(
            dir.path(),
            request(&session.id),
            AuditThresholds::default(),
            None,
            None,
            |_| true,
        )
        .unwrap();
        assert!(imported.partial);
        assert_eq!(imported.request.source_status, AuditSourceStatus::Imported);
        assert!(
            imported
                .request
                .exclusions
                .iter()
                .any(|item| item.contains("Imported crawl"))
        );
    }

    #[test]
    fn active_database_alias_cannot_be_reported_under_another_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let index = crate::sessions::index_connection(dir.path()).unwrap();
        let (active, _store) = crate::sessions::create_session(
            &index,
            dir.path(),
            "Active",
            "https://example.test",
            None,
            "finished",
        )
        .unwrap();
        let (alias, _store) = crate::sessions::create_session(
            &index,
            dir.path(),
            "Alias",
            "https://example.test",
            None,
            "finished",
        )
        .unwrap();
        index
            .execute(
                "UPDATE crawl_sessions SET database_path=?1 WHERE id=?2",
                rusqlite::params![active.database_path, alias.id],
            )
            .unwrap();
        assert!(
            prepare_saved_report(
                dir.path(),
                request(&alias.id),
                AuditThresholds::default(),
                Some(&active.id),
                Some(Path::new(&active.database_path)),
                |_| true,
            )
            .is_err()
        );
        assert_eq!(list_reports(dir.path(), 0, 10).unwrap().total, 0);
    }

    #[tokio::test]
    async fn active_crawl_check_releases_lifecycle_lock_before_report_work() {
        let task = tauri::async_runtime::spawn(async {
            std::future::pending::<()>().await;
        });
        let state = AppState {
            store: Mutex::new(ActiveStore::memory()),
            control: Mutex::new(None),
            crawl_task: tokio::sync::Mutex::new(Some(task)),
            current_session_id: Mutex::new(Some("active".into())),
            comparison: Mutex::new(crate::ComparisonState::default()),
            frontend_ready: AtomicBool::new(true),
            exit_confirmed: AtomicBool::new(false),
        };
        let (id, path) = active_crawl_source(&state).await.unwrap();
        assert_eq!(id.as_deref(), Some("active"));
        assert!(path.is_none());
        assert!(state.crawl_task.try_lock().is_ok());
    }
}
