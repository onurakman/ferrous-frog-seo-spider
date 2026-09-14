//! Native lifecycle for saved report comparisons, isolated from crawl/session state.
use crate::app_data_dir;
use crate::audit_reports::{
    AuditReportsState, delete_saved_file_with_sidecar, open_report, validate_id,
};
use ferrous_frog_storage::{
    AuditComparisonEvidenceQuery, AuditComparisonEvidenceResponse, AuditComparisonFindingQuery,
    AuditComparisonFindingResponse, AuditComparisonSummary, AuditPreparationProgress,
    AuditReportComparisonStore,
};
use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SavedAuditReportComparison {
    pub(super) id: String,
    pub(super) summary: AuditComparisonSummary,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AuditReportComparisonList {
    rows: Vec<SavedAuditReportComparison>,
    total: usize,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonProgress {
    request_id: String,
    #[serde(flatten)]
    progress: AuditPreparationProgress,
}

pub(super) fn comparison_path(directory: &Path, id: &str) -> Result<PathBuf, String> {
    validate_id(id)?;
    Ok(directory
        .join("audit-comparisons")
        .join(format!("{id}.sqlite3")))
}

pub(super) fn open_comparison(
    directory: &Path,
    id: &str,
) -> Result<AuditReportComparisonStore, String> {
    AuditReportComparisonStore::open(comparison_path(directory, id)?)
        .map_err(|error| format!("Could not open the saved audit comparison: {error}"))
}

fn saved_comparison(directory: &Path, id: &str) -> Result<SavedAuditReportComparison, String> {
    Ok(SavedAuditReportComparison {
        id: id.into(),
        summary: open_comparison(directory, id)?
            .summary()
            .map_err(|error| error.to_string())?,
    })
}

fn prepare_saved_comparison(
    directory: &Path,
    request_id: &str,
    baseline_report_id: &str,
    current_report_id: &str,
    progress: impl FnMut(AuditPreparationProgress) -> bool,
) -> Result<SavedAuditReportComparison, String> {
    validate_id(request_id)?;
    validate_id(baseline_report_id)?;
    validate_id(current_report_id)?;
    if baseline_report_id == current_report_id {
        return Err("Choose two different saved audit reports to compare".into());
    }
    let baseline = open_report(directory, baseline_report_id)?;
    let current = open_report(directory, current_report_id)?;
    let suffix: String = Connection::open_in_memory()
        .map_err(|error| error.to_string())?
        .query_row("SELECT lower(hex(randomblob(8)))", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let id = format!("comparison-{}-{suffix}", crate::now_ms());
    let path = comparison_path(directory, &id)?;
    fs::create_dir_all(path.parent().ok_or("Invalid comparison directory")?)
        .map_err(|error| error.to_string())?;
    let comparison = AuditReportComparisonStore::prepare(&path, &baseline, &current, progress)
        .map_err(|error| error.to_string())?;
    Ok(SavedAuditReportComparison {
        id,
        summary: comparison.summary().map_err(|error| error.to_string())?,
    })
}

fn list_comparisons(
    directory: &Path,
    offset: usize,
    limit: usize,
) -> Result<AuditReportComparisonList, String> {
    if !(1..=100).contains(&limit) {
        return Err("Choose a comparison library page size between 1 and 100".into());
    }
    let library = directory.join("audit-comparisons");
    if !library.exists() {
        return Ok(AuditReportComparisonList {
            rows: Vec::new(),
            total: 0,
        });
    }
    // ponytail: enumerate filenames like the report library; use a metadata index if libraries grow large.
    let mut paths = fs::read_dir(&library)
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "sqlite3")
            && path.is_file()
    });
    paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    let total = paths.len();
    let rows = paths
        .iter()
        .skip(offset)
        .take(limit)
        .map(|path| {
            let id = path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or("Invalid comparison filename")?;
            saved_comparison(directory, id)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(AuditReportComparisonList { rows, total })
}

fn query_saved_evidence(
    directory: &Path,
    comparison_id: &str,
    mut query: AuditComparisonEvidenceQuery,
) -> Result<AuditComparisonEvidenceResponse, String> {
    // UI input cannot disable bounded previews; native export reads complete values separately.
    query.preview = true;
    open_comparison(directory, comparison_id)?
        .query_evidence(query)
        .map_err(|error| error.to_string())
}

fn delete_saved_comparison(directory: &Path, id: &str) -> Result<(), String> {
    drop(open_comparison(directory, id)?);
    let annotations = crate::audit_report_ai::comparison_annotation_path(directory, id)?;
    delete_saved_file_with_sidecar(&comparison_path(directory, id)?, &annotations)
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn prepare_audit_report_comparison(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    request_id: String,
    baseline_report_id: String,
    current_report_id: String,
) -> Result<SavedAuditReportComparison, String> {
    let job = reports.begin(&request_id)?;
    let cancelled = job.cancelled.clone();
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        prepare_saved_comparison(
            &directory,
            &request_id,
            &baseline_report_id,
            &current_report_id,
            |progress| {
                let _ = app.emit(
                    "audit-report-progress",
                    ComparisonProgress {
                        request_id: request_id.clone(),
                        progress,
                    },
                );
                !cancelled.load(Ordering::SeqCst)
            },
        )
    })
    .await
    .map_err(|error| format!("Audit comparison preparation worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn list_audit_report_comparisons(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<AuditReportComparisonList, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        list_comparisons(&directory, offset.unwrap_or(0), limit.unwrap_or(100))
    })
    .await
    .map_err(|error| format!("Audit comparison library worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn get_audit_report_comparison(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    comparison_id: String,
) -> Result<SavedAuditReportComparison, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || saved_comparison(&directory, &comparison_id))
        .await
        .map_err(|error| format!("Audit comparison worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn query_audit_report_comparison_findings(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    comparison_id: String,
    query: AuditComparisonFindingQuery,
) -> Result<AuditComparisonFindingResponse, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        open_comparison(&directory, &comparison_id)?
            .query_findings(query)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Audit comparison findings worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn query_audit_report_comparison_evidence(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    comparison_id: String,
    query: AuditComparisonEvidenceQuery,
) -> Result<AuditComparisonEvidenceResponse, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        query_saved_evidence(&directory, &comparison_id, query)
    })
    .await
    .map_err(|error| format!("Audit comparison evidence worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn delete_audit_report_comparison(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    comparison_id: String,
) -> Result<(), String> {
    let _job = reports.begin(&format!("delete-comparison-{}", crate::now_ms()))?;
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        delete_saved_comparison(&directory, &comparison_id)
    })
    .await
    .map_err(|error| format!("Audit comparison deletion worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{
        ActiveStore, AuditReportLanguage, AuditReportRequest, AuditReportStore, AuditSourceStatus,
        CrawlRecord, CrawlStore, GridQuery,
    };

    fn source_report(directory: &Path, id: &str, title: Option<&str>) {
        let path = crate::audit_reports::report_path(directory, id).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = ActiveStore::memory();
        let mut record = CrawlRecord::pending("https://example.test/page".into(), 0);
        record.status_code = Some(200);
        record.content_type = Some("text/html".into());
        record.indexability_status = "Indexable".into();
        record.title = title.map(str::to_owned);
        record.title_len = title.map_or(0, str::len);
        source.upsert(record);
        AuditReportStore::prepare(
            &path,
            &source,
            AuditReportRequest {
                id: id.into(),
                title: id.into(),
                language: AuditReportLanguage::English,
                source_session_id: id.into(),
                source_revision: "1".into(),
                source_status: AuditSourceStatus::Completed,
                created_at: "2026-09-14T12:00:00Z".into(),
                scope: GridQuery::default(),
                exclusions: vec![],
                crawl_limits: vec![],
            },
        )
        .unwrap();
    }
    #[test]
    fn saved_comparison_lifecycle_survives_report_deletion_and_keeps_library_windows() {
        let directory = tempfile::tempdir().unwrap();
        source_report(directory.path(), "baseline", None);
        source_report(directory.path(), "current", Some("Fixed title"));
        let saved =
            prepare_saved_comparison(directory.path(), "request-1", "baseline", "current", |_| {
                true
            })
            .unwrap();
        assert!(saved.id.starts_with("comparison-"));
        assert_eq!(saved.summary.baseline_report_id, "baseline");
        let first = list_comparisons(directory.path(), 0, 1).unwrap();
        assert_eq!(first.total, 1);
        assert_eq!(first.rows[0].id, saved.id);
        assert_eq!(
            list_comparisons(directory.path(), 1, 1).unwrap().rows.len(),
            0
        );
        fs::remove_file(crate::audit_reports::report_path(directory.path(), "baseline").unwrap())
            .unwrap();
        fs::remove_file(crate::audit_reports::report_path(directory.path(), "current").unwrap())
            .unwrap();
        let evidence = query_saved_evidence(
            directory.path(),
            &saved.id,
            AuditComparisonEvidenceQuery {
                finding_id: "title.missing".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(evidence.total, 1);
        assert_eq!(
            evidence.rows[0].state,
            ferrous_frog_storage::AuditComparisonEvidenceState::Resolved
        );
        let sidecar = directory
            .path()
            .join("audit-comparisons")
            .join(format!("{}.ai", saved.id));
        fs::create_dir(&sidecar).unwrap();
        assert!(delete_saved_comparison(directory.path(), &saved.id).is_err());
        assert!(
            open_comparison(directory.path(), &saved.id).is_ok(),
            "Sidecar cleanup failure must preserve the comparison"
        );
        fs::remove_dir(&sidecar).unwrap();
        fs::write(&sidecar, "saved optional commentary").unwrap();
        delete_saved_comparison(directory.path(), &saved.id).unwrap();
        assert!(
            !sidecar.exists(),
            "Deleting a comparison must remove its optional AI commentary"
        );
        assert_eq!(list_comparisons(directory.path(), 0, 10).unwrap().total, 0);
    }
    #[test]
    fn comparison_ipc_preview_cannot_be_disabled_by_the_caller() {
        let directory = tempfile::tempdir().unwrap();
        source_report(directory.path(), "baseline", None);
        source_report(directory.path(), "current", Some(&"ç".repeat(40_000)));
        let saved =
            prepare_saved_comparison(directory.path(), "request-1", "baseline", "current", |_| {
                true
            })
            .unwrap();
        let page = query_saved_evidence(
            directory.path(),
            &saved.id,
            AuditComparisonEvidenceQuery {
                finding_id: "title.missing".into(),
                preview: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(page.rows[0].preview_truncated);
        assert_eq!(
            page.rows[0]
                .current
                .as_ref()
                .unwrap()
                .observed
                .title
                .as_ref()
                .unwrap()
                .len(),
            65_536
        );
        delete_saved_comparison(directory.path(), &saved.id).unwrap();
        assert!(
            !comparison_path(directory.path(), &saved.id)
                .unwrap()
                .exists()
        );
    }
    #[test]
    fn comparison_invalid_ids_sources_and_cancellation_leave_no_saved_comparison() {
        let directory = tempfile::tempdir().unwrap();
        source_report(directory.path(), "baseline", None);
        source_report(directory.path(), "current", Some("Fixed title"));
        assert!(
            prepare_saved_comparison(
                directory.path(),
                "request-1",
                "baseline",
                "baseline",
                |_| true
            )
            .is_err()
        );
        assert!(
            prepare_saved_comparison(
                directory.path(),
                "request-1",
                "../baseline",
                "current",
                |_| true
            )
            .is_err()
        );
        assert!(
            prepare_saved_comparison(directory.path(), "request-1", "missing", "current", |_| {
                true
            })
            .is_err()
        );
        assert!(
            prepare_saved_comparison(
                directory.path(),
                "request-1",
                "baseline",
                "current",
                |step| step.phase != "evidence"
            )
            .is_err()
        );
        assert_eq!(list_comparisons(directory.path(), 0, 10).unwrap().total, 0);
        assert!(comparison_path(directory.path(), "../escape").is_err());
        assert!(list_comparisons(directory.path(), 0, 101).is_err());
        assert!(open_comparison(directory.path(), "missing").is_err());
        assert!(crate::audit_reports::open_report(directory.path(), "baseline").is_ok());
    }
}
