//! Complete report packages publish only after their evidence, CSV and manifest succeed.

use crate::audit_reports::{AuditReportsState, open_report};
use crate::{AppState, ExportFileResult, app_data_dir, export_dir};
use ferrous_frog_export::{AuditReportExportProgress, write_audit_report_package_with_annotations};
use ferrous_frog_storage::{AuditEvidenceQuery, AuditReportStore};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportProgress {
    request_id: String,
    #[serde(flatten)]
    progress: AuditReportExportProgress,
}

fn publish_package<A: Serialize>(
    report: &AuditReportStore,
    exports: &Path,
    annotations: impl FnMut(&str) -> Result<Option<A>, String>,
    overview: Option<serde_json::Value>,
    ai_generation_version: Option<&str>,
    progress: impl FnMut(AuditReportExportProgress) -> bool,
) -> Result<ExportFileResult, String> {
    publish_directory(exports, progress, |pending, progress| {
        write_audit_report_package_with_annotations(
            report,
            pending,
            annotations,
            overview,
            ai_generation_version,
            progress,
        )
        .map(|manifest| manifest.evidence_rows)
    })
}

fn publish_directory(
    exports: &Path,
    mut progress: impl FnMut(AuditReportExportProgress) -> bool,
    write: impl FnOnce(
        &Path,
        &mut dyn FnMut(AuditReportExportProgress) -> bool,
    ) -> Result<usize, String>,
) -> Result<ExportFileResult, String> {
    // Reserve a unique parent so the final rename cannot replace an earlier user's report.
    // The only published artifact is `report`; the sibling preparation directory is private.
    let reservation = tempfile::Builder::new()
        .prefix("ferrous-frog-audit-")
        .tempdir_in(exports)
        .map_err(|error| format!("Could not reserve an export directory: {error}"))?;
    let pending = reservation.path().join(".preparing");
    std::fs::create_dir(&pending).map_err(|error| error.to_string())?;
    let row_count = write(&pending, &mut progress)?;
    if !progress(AuditReportExportProgress {
        phase: "publishing".into(),
        completed: row_count,
        total: row_count,
    }) {
        return Err("Audit report export cancelled".into());
    }
    let published = reservation.path().join("report");
    std::fs::rename(&pending, &published)
        .map_err(|error| format!("Could not publish the complete report: {error}"))?;
    let _published_directory = reservation.keep();
    Ok(ExportFileResult {
        path: published.join("index.html").to_string_lossy().into_owned(),
        row_count,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn export_audit_report(
    app: AppHandle,
    state: State<'_, AppState>,
    reports: State<'_, AuditReportsState>,
    report_id: String,
    request_id: String,
) -> Result<ExportFileResult, String> {
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("The application is closing".into());
    }
    let job = reports.begin(&request_id)?;
    let cancelled = job.cancelled.clone();
    let directory = app_data_dir(&app)?;
    let exports = export_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        let report = open_report(&directory, &report_id)?;
        let annotation_path = crate::audit_report_ai::annotation_path(&directory, &report_id)?;
        let ai_generation_version =
            crate::audit_report_ai::selected_generation_version(&annotation_path)?;
        let annotations = crate::audit_report_ai::export_annotation_reader(&annotation_path)?;
        let overview = crate::audit_report_ai::export_overview(&annotation_path)?
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| error.to_string())?;
        publish_package(
            &report,
            &exports,
            annotations,
            overview,
            ai_generation_version.as_deref(),
            |progress| {
                let _ = app.emit(
                    "audit-report-progress",
                    ExportProgress {
                        request_id: request_id.clone(),
                        progress,
                    },
                );
                !cancelled.load(Ordering::SeqCst)
            },
        )
    })
    .await
    .map_err(|error| format!("Report export worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn export_audit_report_evidence(
    app: AppHandle,
    state: State<'_, AppState>,
    reports: State<'_, AuditReportsState>,
    report_id: String,
    request_id: String,
    query: AuditEvidenceQuery,
) -> Result<ExportFileResult, String> {
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("The application is closing".into());
    }
    let job = reports.begin(&request_id)?;
    let cancelled = job.cancelled.clone();
    let directory = app_data_dir(&app)?;
    let path = crate::export_path(
        &app,
        &format!(
            "ferrous-frog-report-evidence-{}-{}.csv",
            crate::now_ms(),
            request_id
        ),
    )?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        let report = open_report(&directory, &report_id)?;
        crate::write_atomic_export(&path, |file| {
            ferrous_frog_export::write_audit_report_evidence_csv(&report, query, file, |progress| {
                let _ = app.emit(
                    "audit-report-progress",
                    ExportProgress {
                        request_id: request_id.clone(),
                        progress,
                    },
                );
                !cancelled.load(Ordering::SeqCst)
            })
        })
    })
    .await
    .map_err(|error| format!("Report evidence export worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn export_audit_report_comparison(
    app: AppHandle,
    state: State<'_, AppState>,
    reports: State<'_, AuditReportsState>,
    comparison_id: String,
    request_id: String,
) -> Result<ExportFileResult, String> {
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("The application is closing".into());
    }
    let job = reports.begin(&request_id)?;
    let cancelled = job.cancelled.clone();
    let directory = app_data_dir(&app)?;
    let exports = export_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        let comparison =
            crate::audit_report_comparison::open_comparison(&directory, &comparison_id)?;
        let annotation_path =
            crate::audit_report_ai::comparison_annotation_path(&directory, &comparison_id)?;
        let ai_generation_version =
            crate::audit_report_ai::selected_generation_version(&annotation_path)?;
        let annotations = crate::audit_report_ai::export_annotation_reader(&annotation_path)?;
        let overview = crate::audit_report_ai::export_overview(&annotation_path)?
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| error.to_string())?;
        publish_directory(
            &exports,
            |progress| {
                let _ = app.emit(
                    "audit-report-progress",
                    ExportProgress {
                        request_id: request_id.clone(),
                        progress,
                    },
                );
                !cancelled.load(Ordering::SeqCst)
            },
            |pending, progress| {
                ferrous_frog_export::audit_report_comparison::write_audit_report_comparison_package_with_annotations(
                    &comparison,
                    pending,
                    annotations,
                    overview,
                    ai_generation_version.as_deref(),
                    progress,
                )
                .map(|manifest| manifest.evidence_rows)
            },
        )
    })
    .await
    .map_err(|error| format!("Report comparison export worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{
        ActiveStore, AuditReportLanguage, AuditReportRequest, AuditSourceStatus, CrawlRecord,
        CrawlStore, GridQuery,
    };

    #[test]
    fn cancelled_packages_leave_no_partial_export_and_completed_packages_never_replace_earlier_ones()
     {
        let directory = tempfile::tempdir().unwrap();
        let exports = directory.path().join("exports");
        std::fs::create_dir(&exports).unwrap();
        let source = ActiveStore::memory();
        let mut row = CrawlRecord::pending("https://example.test/page".into(), 0);
        row.status_code = Some(200);
        row.content_type = Some("text/html".into());
        source.upsert(row);
        let report = AuditReportStore::prepare(
            directory.path().join("report.sqlite3"),
            &source,
            AuditReportRequest {
                id: "export-test".into(),
                title: "Complete audit".into(),
                language: AuditReportLanguage::English,
                source_session_id: "source".into(),
                source_revision: "1".into(),
                source_status: AuditSourceStatus::Completed,
                created_at: "2026-09-14T00:00:00Z".into(),
                scope: GridQuery::default(),
                exclusions: vec![],
                crawl_limits: vec![],
            },
        )
        .unwrap();
        for phase in ["preparing", "evidence", "publishing"] {
            assert!(
                publish_package(
                    &report,
                    &exports,
                    |_| Ok(None::<()>),
                    None,
                    None,
                    |event| event.phase != phase
                )
                .is_err()
            );
            assert_eq!(std::fs::read_dir(&exports).unwrap().count(), 0);
        }
        let first =
            publish_package(&report, &exports, |_| Ok(None::<()>), None, None, |_| true).unwrap();
        let first_bytes = std::fs::read(&first.path).unwrap();
        let second =
            publish_package(&report, &exports, |_| Ok(None::<()>), None, None, |_| true).unwrap();
        assert_ne!(first.path, second.path);
        assert!(first.row_count > 0);
        assert_eq!(std::fs::read(&first.path).unwrap(), first_bytes);
        for result in [first, second] {
            let package = Path::new(&result.path).parent().unwrap();
            assert!(package.join("manifest.json").is_file());
            assert!(!package.parent().unwrap().join(".preparing").exists());
        }
    }
}
