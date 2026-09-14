//! Optional, resumable explanations over immutable findings. Provider output never changes evidence.

use crate::ai::{AiSettings, AiState, load_api_key, load_settings};
use crate::audit_report_comparison::open_comparison;
use crate::audit_reports::{AuditReportsState, open_report, validate_id};
use crate::{AppState, app_data_dir, now_ms};
use ferrous_frog_integrations::IntegrationError;
use ferrous_frog_integrations::llm::{LlmClient, LlmCompletion, LlmConfig};
use ferrous_frog_integrations::report_ai::{
    REPORT_PROMPT_VERSION, ReportAnnotation, ReportComparisonContext,
    ReportComparisonEvidenceState, ReportComparisonObservation, ReportComparisonSample,
    ReportComparisonStatus, ReportEvidenceSample, ReportFindingContext, ReportOverview,
    ReportOverviewItem, parse_report_annotations, parse_report_overview, report_input_digest,
    report_overview_prompt, report_prompt,
};
use ferrous_frog_storage::{
    AuditComparisonEvidenceQuery, AuditComparisonEvidenceState, AuditComparisonFinding,
    AuditComparisonFindingQuery, AuditComparisonObservation, AuditComparisonStatus,
    AuditEvidenceQuery, AuditFinding, AuditFindingQuery, AuditReportComparisonStore,
    AuditReportLanguage, AuditReportStore,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReportAiOptions {
    pub max_requests: usize,
    pub max_input_chars: usize,
    pub samples_per_finding: usize,
}

impl ReportAiOptions {
    fn validate(&self) -> Result<(), String> {
        if !(1..=100).contains(&self.max_requests)
            || !(2_000..=2_000_000).contains(&self.max_input_chars)
            || !(1..=10).contains(&self.samples_per_finding)
        {
            return Err("Choose 1–100 requests, 2,000–2,000,000 total input characters and 1–10 samples per finding".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReportAiPreview {
    pub version: String,
    #[serde(default)]
    pub preview_digest: String,
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub finding_count: usize,
    pub completed_findings: usize,
    pub pending_findings: usize,
    pub estimated_requests: usize,
    pub estimated_input_chars: usize,
    pub sampled_evidence: usize,
    #[serde(default)]
    pub overview_pending: bool,
    #[serde(default)]
    pub overview_planned: bool,
    #[serde(default)]
    pub sampling_policy: String,
    pub data_categories: Vec<String>,
    pub options: ReportAiOptions,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SavedReportAnnotation {
    #[serde(flatten)]
    pub annotation: ReportAnnotation,
    pub input_digest: String,
    pub model: String,
    pub generated_at_ms: i64,
    pub sample_count: usize,
    pub evidence_total: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SavedReportOverview {
    #[serde(flatten)]
    pub overview: ReportOverview,
    pub input_digest: String,
    pub model: String,
    pub generated_at_ms: i64,
    pub included_finding_count: usize,
    pub total_finding_count: usize,
    pub sampled_evidence_count: usize,
    pub partial_coverage: bool,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReportAiStatus {
    pub version: String,
    pub provider: String,
    pub model: String,
    pub status: String,
    pub finding_count: usize,
    pub completed_findings: usize,
    pub requests: usize,
    pub input_chars: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub next_retry_at_ms: Option<i64>,
    pub overview: Option<SavedReportOverview>,
    pub error: Option<String>,
    pub rows: Vec<SavedReportAnnotation>,
    pub export_generation_version: Option<String>,
    pub preserved_generation: Option<Box<ReportAiStatus>>,
}

struct PromptBatch {
    finding: ReportFindingContext,
    system: String,
    user: String,
    digest: String,
    input_chars: usize,
}

struct OverviewBatch {
    system: String,
    user: String,
    digest: String,
    input_chars: usize,
    known_ids: Vec<String>,
    included_count: usize,
    total_count: usize,
    sampled_evidence_count: usize,
}

struct PreparedReportAi {
    preview: ReportAiPreview,
    batches: Vec<PromptBatch>,
    language: String,
    max_prompt_chars: usize,
}

pub(super) fn annotation_path(directory: &Path, report_id: &str) -> Result<PathBuf, String> {
    validate_id(report_id)?;
    // Same lifecycle as the report, separate file so evidence stays read-only.
    Ok(directory
        .join("audit-reports")
        .join(format!("{report_id}.ai")))
}

pub(super) fn comparison_annotation_path(
    directory: &Path,
    comparison_id: &str,
) -> Result<PathBuf, String> {
    validate_id(comparison_id)?;
    Ok(directory
        .join("audit-comparisons")
        .join(format!("{comparison_id}.ai")))
}

fn database(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open(path).map_err(|error| error.to_string())?;
    connection.execute_batch("PRAGMA busy_timeout=5000;
        CREATE TABLE IF NOT EXISTS generations (
            version TEXT PRIMARY KEY, updated_at INTEGER NOT NULL, preview TEXT NOT NULL,
            status TEXT NOT NULL, requests INTEGER NOT NULL DEFAULT 0, input_chars INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER, output_tokens INTEGER, usage_pending INTEGER NOT NULL DEFAULT 0,
            next_retry_at_ms INTEGER,
            run_instance TEXT, overview TEXT, error TEXT);
        CREATE TABLE IF NOT EXISTS annotations (
            version TEXT NOT NULL, finding_id TEXT NOT NULL, payload TEXT NOT NULL,
            PRIMARY KEY(version, finding_id));").map_err(|error| error.to_string())?;
    for (name, definition) in [
        ("usage_pending", "INTEGER NOT NULL DEFAULT 0"),
        ("next_retry_at_ms", "INTEGER"),
        ("run_instance", "TEXT"),
        ("overview", "TEXT"),
    ] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('generations') WHERE name=?1)",
                [name],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !exists {
            connection
                .execute(
                    &format!("ALTER TABLE generations ADD COLUMN {name} {definition}"),
                    [],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(connection)
}

fn run_instance() -> &'static str {
    static INSTANCE: OnceLock<String> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{}-{nanos}", std::process::id())
    })
}

fn readonly_database(path: &Path) -> Result<Option<Connection>, String> {
    if !path.exists() {
        return Ok(None);
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn annotation_exists(
    connection: Option<&Connection>,
    version: &str,
    finding: &str,
) -> Result<bool, String> {
    let Some(connection) = connection else {
        return Ok(false);
    };
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM annotations WHERE version=?1 AND finding_id=?2)",
            params![version, finding],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn overview_exists(connection: Option<&Connection>, version: &str) -> Result<bool, String> {
    let Some(connection) = connection else {
        return Ok(false);
    };
    connection
        .query_row(
            "SELECT overview IS NOT NULL FROM generations WHERE version=?1",
            [version],
            |row| row.get(0),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
        .map_err(|error| error.to_string())
}

fn safe_url(value: &str) -> String {
    let Ok(mut url) = url::Url::parse(value) else {
        return "Unavailable URL".into();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string().chars().take(512).collect()
}

fn excerpt(value: Option<&str>) -> Option<String> {
    value.map(|text| {
        text.chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .take(256)
            .collect()
    })
}

fn sample_offsets(total: usize, requested: usize) -> Vec<usize> {
    let count = total.min(requested);
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![0];
    }
    (0..count)
        .map(|index| ((index as u128 * (total - 1) as u128) / (count - 1) as u128) as usize)
        .collect()
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if result.len() + character.len_utf8() > max_bytes {
            break;
        }
        result.push(character);
    }
    result
}

fn overview_batch(
    connection: &Connection,
    version: &str,
    total_count: usize,
    language: &str,
    max_chars: usize,
) -> Result<Option<OverviewBatch>, String> {
    if total_count == 0 {
        return Ok(None);
    }
    let stored_count: usize = connection
        .query_row(
            "SELECT count(*) FROM annotations WHERE version=?1",
            [version],
            |row| sql_count(row, 0),
        )
        .map_err(|error| error.to_string())?;
    if stored_count != total_count {
        return Err("The saved finding annotations are inconsistent with the frozen report".into());
    }
    let mut items = Vec::new();
    for offset in sample_offsets(total_count, 20) {
        let payload: String = connection.query_row(
            "SELECT payload FROM annotations WHERE version=?1 ORDER BY finding_id LIMIT 1 OFFSET ?2",
            params![version, offset as i64], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        let saved: SavedReportAnnotation =
            serde_json::from_str(&payload).map_err(|error| error.to_string())?;
        items.push(ReportOverviewItem {
            finding_id: saved.annotation.finding_id,
            explanation_excerpt: bounded_text(&saved.annotation.explanation, 256),
            recommendation_excerpt: bounded_text(&saved.annotation.recommendation, 256),
            sample_count: saved.sample_count,
            evidence_total: saved.evidence_total,
        });
    }
    loop {
        match report_overview_prompt(&items, total_count, language, max_chars) {
            Ok((system, user)) => {
                let input_chars = system.chars().count() + user.chars().count();
                let digest = report_input_digest(&format!("{system}\n{user}"));
                let known_ids = items.iter().map(|item| item.finding_id.clone()).collect();
                let sampled_evidence_count = items.iter().map(|item| item.sample_count).sum();
                return Ok(Some(OverviewBatch {
                    system,
                    user,
                    digest,
                    input_chars,
                    known_ids,
                    included_count: items.len(),
                    total_count,
                    sampled_evidence_count,
                }));
            }
            Err(_) if items.len() > 1 => {
                items.remove(items.len() / 2);
            }
            Err(_) => return Ok(None),
        }
    }
}

fn finding_prompt(
    report: &AuditReportStore,
    finding: AuditFinding,
    samples: usize,
    language: &str,
    max_chars: usize,
) -> Result<PromptBatch, String> {
    let first = report
        .query_evidence(AuditEvidenceQuery {
            finding_id: finding.id.clone(),
            limit: 1,
            ..Default::default()
        })
        .map_err(|error| error.to_string())?;
    let evidence_total = first.total;
    let mut first = Some(first);
    let mut sample_rows = Vec::new();
    for offset in sample_offsets(evidence_total, samples) {
        let page = if offset == 0 {
            first.take().expect("first sample")
        } else {
            report
                .query_evidence(AuditEvidenceQuery {
                    finding_id: finding.id.clone(),
                    offset,
                    limit: 1,
                    ..Default::default()
                })
                .map_err(|error| error.to_string())?
        };
        sample_rows.push(
            page.rows
                .into_iter()
                .next()
                .ok_or("Report evidence changed during sampling")?,
        );
    }
    let mut context = ReportFindingContext {
        finding_id: finding.id,
        title: finding.title,
        affected_urls: finding.counts.unique_urls,
        affected_records: finding.counts.source_records,
        eligible_records: finding.eligible_records,
        evidence_total,
        coverage: format!(
            "{:?}; {} source pages; {} occurrences; {:?} distinct targets",
            finding.coverage,
            finding.counts.source_pages,
            finding.counts.occurrences,
            finding.counts.targets
        ),
        samples: sample_rows
            .iter()
            .map(|row| ReportEvidenceSample {
                id: row.id.clone(),
                text: serde_json::json!({
                    "urlWithoutQueryOrCredentials": safe_url(&row.original_url),
                    "statusCode": row.observed.status_code,
                    "titleExcerpt": excerpt(row.observed.title.as_deref()),
                    "metaDescriptionExcerpt": excerpt(row.observed.meta_description.as_deref()),
                    "h1Excerpt": excerpt(row.observed.h1.as_deref()),
                    "anchorExcerpt": excerpt(row.observed.anchor_text.as_deref()),
                    "targetWithoutQueryOrCredentials": row.target_url.as_deref().map(safe_url),
                    "rendered": row.observed.rendered, "previewTruncated": row.preview_truncated
                })
                .to_string(),
                comparison: None,
            })
            .collect(),
        comparison: None,
    };
    loop {
        match report_prompt(std::slice::from_ref(&context), language, max_chars) {
            Ok((system, user)) => {
                let input_chars = system.chars().count() + user.chars().count();
                let digest = report_input_digest(&format!("{system}\n{user}"));
                return Ok(PromptBatch {
                    finding: context,
                    system,
                    user,
                    digest,
                    input_chars,
                });
            }
            Err(_) if !context.samples.is_empty() => {
                let remove_at = if context.samples.len() > 2 {
                    context.samples.len() / 2
                } else {
                    context.samples.len() - 1
                };
                context.samples.remove(remove_at);
            }
            Err(error) => {
                return Err(format!(
                    "Increase the AI input limit in Settings > AI: {error}"
                ));
            }
        }
    }
}

fn comparison_observation(observation: &AuditComparisonObservation) -> ReportComparisonObservation {
    let short = |value: Option<&str>| {
        value.filter(|value| !value.trim().is_empty()).map(|value| {
            bounded_text(
                &value
                    .chars()
                    .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
                    .collect::<String>(),
                256,
            )
        })
    };
    ReportComparisonObservation {
        url_without_query_or_credentials: bounded_text(&safe_url(&observation.original_url), 512),
        final_url_without_query_or_credentials: observation
            .final_url
            .as_deref()
            .map(safe_url)
            .map(|value| bounded_text(&value, 512)),
        target_without_query_or_credentials: observation
            .target_url
            .as_deref()
            .map(safe_url)
            .map(|value| bounded_text(&value, 512)),
        status_code: observation.observed.status_code,
        title_excerpt: short(observation.observed.title.as_deref()),
        meta_description_excerpt: short(observation.observed.meta_description.as_deref()),
        h1_excerpt: short(observation.observed.h1.as_deref()),
        anchor_excerpt: short(observation.observed.anchor_text.as_deref()),
    }
}

fn comparison_finding_prompt(
    comparison: &AuditReportComparisonStore,
    finding: AuditComparisonFinding,
    samples: usize,
    language: &str,
    max_chars: usize,
) -> Result<PromptBatch, String> {
    let first = comparison
        .query_evidence(AuditComparisonEvidenceQuery {
            finding_id: finding.finding_id.clone(),
            limit: 1,
            ..Default::default()
        })
        .map_err(|error| error.to_string())?;
    let evidence_total = first.total;
    let mut first = Some(first);
    let mut sample_rows = Vec::new();
    for offset in sample_offsets(evidence_total, samples) {
        let page = if offset == 0 {
            first.take().expect("first sample")
        } else {
            comparison
                .query_evidence(AuditComparisonEvidenceQuery {
                    finding_id: finding.finding_id.clone(),
                    offset,
                    limit: 1,
                    ..Default::default()
                })
                .map_err(|error| error.to_string())?
        };
        sample_rows.push(
            page.rows
                .into_iter()
                .next()
                .ok_or("Comparison evidence changed during sampling")?,
        );
    }
    let status = match finding.status {
        AuditComparisonStatus::New => ReportComparisonStatus::New,
        AuditComparisonStatus::Resolved => ReportComparisonStatus::Resolved,
        AuditComparisonStatus::Improved => ReportComparisonStatus::Improved,
        AuditComparisonStatus::Unchanged => ReportComparisonStatus::Unchanged,
        AuditComparisonStatus::Worsened => ReportComparisonStatus::Worsened,
        AuditComparisonStatus::MixedChanges => ReportComparisonStatus::MixedChanges,
        AuditComparisonStatus::NotComparable => ReportComparisonStatus::NotComparable,
    };
    let mut context = ReportFindingContext {
        finding_id: finding.finding_id,
        title: finding.title,
        affected_urls: finding
            .current_counts
            .as_ref()
            .map_or(0, |counts| counts.unique_urls),
        affected_records: finding
            .current_counts
            .as_ref()
            .and_then(|counts| counts.source_records),
        eligible_records: None,
        evidence_total,
        coverage: "Frozen comparison; missing current evidence is not a verified fix".into(),
        samples: sample_rows
            .iter()
            .map(|row| ReportEvidenceSample {
                id: format!("comparison-ev-{}", row.id),
                text: "Bounded before/after captured fields".into(),
                comparison: Some(ReportComparisonSample {
                    state: match row.state {
                        AuditComparisonEvidenceState::Added => ReportComparisonEvidenceState::Added,
                        AuditComparisonEvidenceState::Persisting => {
                            ReportComparisonEvidenceState::Persisting
                        }
                        AuditComparisonEvidenceState::Resolved => {
                            ReportComparisonEvidenceState::Resolved
                        }
                        AuditComparisonEvidenceState::NotObserved => {
                            ReportComparisonEvidenceState::NotObserved
                        }
                    },
                    reason: bounded_text(&row.reason, 512),
                    before: row.baseline.as_ref().map(comparison_observation),
                    after: row.current.as_ref().map(comparison_observation),
                }),
            })
            .collect(),
        comparison: Some(ReportComparisonContext {
            status,
            count_unit: bounded_text(&finding.count_unit, 80),
            baseline_occurrences: finding.baseline_counts.map(|counts| counts.occurrences),
            current_occurrences: finding.current_counts.map(|counts| counts.occurrences),
            added: finding.added,
            persisting: finding.persisting,
            resolved: finding.resolved,
            not_observed: finding.not_observed,
            compatibility_reasons: finding
                .compatibility_reasons
                .iter()
                .take(10)
                .map(|reason| bounded_text(reason, 512))
                .filter(|reason| !reason.trim().is_empty())
                .collect(),
        }),
    };
    loop {
        match report_prompt(std::slice::from_ref(&context), language, max_chars) {
            Ok((system, user)) => {
                let input_chars = system.chars().count() + user.chars().count();
                let digest = report_input_digest(&format!("{system}\n{user}"));
                return Ok(PromptBatch {
                    finding: context,
                    system,
                    user,
                    digest,
                    input_chars,
                });
            }
            Err(_) if !context.samples.is_empty() => {
                let remove_at = if context.samples.len() > 2 {
                    context.samples.len() / 2
                } else {
                    context.samples.len() - 1
                };
                context.samples.remove(remove_at);
            }
            Err(error) => {
                return Err(format!(
                    "Increase the AI input limit in Settings > AI: {error}"
                ));
            }
        }
    }
}

fn prepare_ai(
    report: &AuditReportStore,
    path: &Path,
    settings: &AiSettings,
    options: ReportAiOptions,
) -> Result<PreparedReportAi, String> {
    options.validate()?;
    if !(1..=600).contains(&settings.requests_per_minute) {
        return Err("Invalid AI request rate in Settings > AI".into());
    }
    let summary = report.summary().map_err(|error| error.to_string())?;
    let language = match summary.request.language {
        AuditReportLanguage::English => "en",
        AuditReportLanguage::Turkish => "tr",
    };
    let version = report_input_digest(
        &serde_json::json!({
            "reportId": summary.request.id, "ruleVersion": summary.rule_version,
            "promptVersion": REPORT_PROMPT_VERSION, "sampleFormatVersion": 2,
            "language": language, "provider": settings.provider, "model": settings.model,
            "endpoint": settings.base_url, "maxPromptChars": settings.max_input_chars,
            "samplesPerFinding": options.samples_per_finding
        })
        .to_string(),
    );
    let saved = readonly_database(path)?;
    let overview_pending = !overview_exists(saved.as_ref(), &version)?;
    let mut completed_findings = 0;
    let mut input_chars = 0;
    let mut sampled_evidence = 0;
    let mut batches = Vec::new();
    let mut offset = 0;
    while offset < summary.finding_count {
        let page = report
            .query_findings(AuditFindingQuery {
                offset,
                ..Default::default()
            })
            .map_err(|error| error.to_string())?;
        if page.rows.is_empty() {
            return Err("The report finding population is inconsistent".into());
        }
        offset += page.rows.len();
        for finding in page.rows {
            if annotation_exists(saved.as_ref(), &version, &finding.id)? {
                completed_findings += 1;
                continue;
            }
            if batches.len() >= options.max_requests {
                continue;
            }
            let prompt = finding_prompt(
                report,
                finding,
                options.samples_per_finding,
                language,
                settings.max_input_chars,
            )?;
            if prompt.input_chars > options.max_input_chars.saturating_sub(input_chars) {
                continue;
            }
            sampled_evidence += prompt.finding.samples.len();
            input_chars += prompt.input_chars;
            batches.push(prompt);
        }
    }
    let overview_possible = overview_pending
        && completed_findings + batches.len() == summary.finding_count
        && batches.len() < options.max_requests;
    let remaining_input = options.max_input_chars.saturating_sub(input_chars);
    let overview_estimate = if !overview_possible {
        None
    } else if completed_findings == summary.finding_count {
        saved
            .as_ref()
            .map(|connection| {
                overview_batch(
                    connection,
                    &version,
                    summary.finding_count,
                    language,
                    settings.max_input_chars,
                )
            })
            .transpose()?
            .flatten()
            .map(|batch| batch.input_chars)
    } else {
        Some(remaining_input.min(settings.max_input_chars))
    };
    let overview_planned =
        overview_estimate.is_some_and(|estimate| estimate > 0 && estimate <= remaining_input);
    let overview_input_estimate = if overview_planned {
        overview_estimate.unwrap_or(0)
    } else {
        0
    };
    let mut preview = ReportAiPreview {
        version, preview_digest: String::new(), provider: serde_json::to_value(settings.provider).map_err(|error| error.to_string())?.as_str().unwrap_or_default().into(),
        model: settings.model.clone(), endpoint: if settings.base_url.is_empty() { "Default provider endpoint".into() } else { safe_url(&settings.base_url) },
        finding_count: summary.finding_count, completed_findings,
        pending_findings: summary.finding_count.saturating_sub(completed_findings),
        estimated_requests: batches.len() + usize::from(overview_planned),
        estimated_input_chars: input_chars + overview_input_estimate, sampled_evidence,
        overview_pending, overview_planned,
        sampling_policy: "Evenly spaced first/middle/last evidence by stable report order; up to 10 samples per finding. Sampled URLs are limited to 512 characters and captured excerpts to 256 characters. If the per-prompt cap requires fewer samples, middle examples are removed first, then the last example. The overview uses up to 20 evenly spaced saved finding annotations; it remains labelled partial when fewer than all findings are included.".into(),
        data_categories: vec!["Measured finding titles, counts, eligibility and coverage".into(), "Bounded URL paths without credentials, query strings or fragments".into(), "Short captured title, description, H1, status and anchor excerpts".into(), "Previously validated finding annotations for an optional executive overview".into()],
        options,
    };
    preview.preview_digest =
        report_input_digest(&serde_json::to_string(&preview).map_err(|error| error.to_string())?);
    Ok(PreparedReportAi {
        preview,
        batches,
        language: language.into(),
        max_prompt_chars: settings.max_input_chars,
    })
}

fn prepare_comparison_ai(
    comparison: &AuditReportComparisonStore,
    path: &Path,
    settings: &AiSettings,
    options: ReportAiOptions,
) -> Result<PreparedReportAi, String> {
    options.validate()?;
    if !(1..=600).contains(&settings.requests_per_minute) {
        return Err("Invalid AI request rate in Settings > AI".into());
    }
    let summary = comparison.summary().map_err(|error| error.to_string())?;
    let language = match summary.current.request.language {
        AuditReportLanguage::English => "en",
        AuditReportLanguage::Turkish => "tr",
    };
    let version = report_input_digest(&serde_json::json!({
        "comparisonId": summary.baseline_report_id.to_owned() + ":" + &summary.current_report_id,
        "schemaVersion": summary.schema_version, "identityVersion": summary.identity_version,
        "promptVersion": REPORT_PROMPT_VERSION, "comparisonContextVersion": 1,
        "language": language, "provider": settings.provider, "model": settings.model,
        "endpoint": settings.base_url, "maxPromptChars": settings.max_input_chars,
        "samplesPerFinding": options.samples_per_finding
    }).to_string());
    let saved = readonly_database(path)?;
    let overview_pending = !overview_exists(saved.as_ref(), &version)?;
    let mut completed_findings = 0;
    let mut input_chars = 0;
    let mut sampled_evidence = 0;
    let mut batches = Vec::new();
    let mut offset = 0;
    while offset < summary.finding_count {
        let page = comparison
            .query_findings(AuditComparisonFindingQuery {
                offset,
                ..Default::default()
            })
            .map_err(|error| error.to_string())?;
        if page.rows.is_empty() {
            return Err("The comparison finding population is inconsistent".into());
        }
        offset += page.rows.len();
        for finding in page.rows {
            if annotation_exists(saved.as_ref(), &version, &finding.finding_id)? {
                completed_findings += 1;
                continue;
            }
            if batches.len() >= options.max_requests {
                continue;
            }
            let prompt = comparison_finding_prompt(
                comparison,
                finding,
                options.samples_per_finding,
                language,
                settings.max_input_chars,
            )?;
            if prompt.input_chars > options.max_input_chars.saturating_sub(input_chars) {
                continue;
            }
            sampled_evidence += prompt.finding.samples.len();
            input_chars += prompt.input_chars;
            batches.push(prompt);
        }
    }
    let overview_possible = overview_pending
        && completed_findings + batches.len() == summary.finding_count
        && batches.len() < options.max_requests;
    let remaining_input = options.max_input_chars.saturating_sub(input_chars);
    let overview_estimate = if !overview_possible {
        None
    } else if completed_findings == summary.finding_count {
        saved
            .as_ref()
            .map(|connection| {
                overview_batch(
                    connection,
                    &version,
                    summary.finding_count,
                    language,
                    settings.max_input_chars,
                )
            })
            .transpose()?
            .flatten()
            .map(|batch| batch.input_chars)
    } else {
        Some(remaining_input.min(settings.max_input_chars))
    };
    let overview_planned =
        overview_estimate.is_some_and(|estimate| estimate > 0 && estimate <= remaining_input);
    let mut preview = ReportAiPreview {
        version, preview_digest: String::new(),
        provider: serde_json::to_value(settings.provider).map_err(|error| error.to_string())?.as_str().unwrap_or_default().into(),
        model: settings.model.clone(),
        endpoint: if settings.base_url.is_empty() { "Default provider endpoint".into() } else { safe_url(&settings.base_url) },
        finding_count: summary.finding_count, completed_findings,
        pending_findings: summary.finding_count.saturating_sub(completed_findings),
        estimated_requests: batches.len() + usize::from(overview_planned),
        estimated_input_chars: input_chars + if overview_planned { overview_estimate.unwrap_or(0) } else { 0 },
        sampled_evidence, overview_pending, overview_planned,
        sampling_policy: "Evenly spaced first/middle/last comparison evidence by stable request URL and occurrence; up to 10 samples per finding. Before/after URLs omit credentials, query strings and fragments; captured text excerpts are bounded. The overview includes at most 20 saved annotations and labels partial coverage.".into(),
        data_categories: vec!["Computed finding state, before/after counts, count unit and compatibility reasons".into(), "Bounded before/after captured status, title, description, H1 and anchor excerpts".into(), "Sanitized URL paths without credentials, query strings or fragments".into(), "Previously validated annotations for an optional executive overview".into()],
        options,
    };
    preview.preview_digest =
        report_input_digest(&serde_json::to_string(&preview).map_err(|error| error.to_string())?);
    Ok(PreparedReportAi {
        preview,
        batches,
        language: language.into(),
        max_prompt_chars: settings.max_input_chars,
    })
}

fn read_status(
    path: &Path,
    version: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<Option<ReportAiStatus>, String> {
    if limit == 0 || limit > 100 {
        return Err("Choose an annotation page size between 1 and 100".into());
    }
    let offset = i64::try_from(offset).map_err(|_| "Annotation offset is too large")?;
    let Some(connection) = readonly_database(path)? else {
        return Ok(None);
    };
    let mut result = read_status_row(&connection, version, offset, limit)?;
    if let Some(status) = result.as_mut() {
        status.export_generation_version = export_generation_version(&connection)?;
        if let Some(export_version) = status.export_generation_version.as_deref()
            && export_version != status.version
        {
            status.preserved_generation =
                read_status_row(&connection, Some(export_version), offset, limit)?.map(Box::new);
        }
    }
    Ok(result)
}

fn read_status_row(
    connection: &Connection,
    version: Option<&str>,
    offset: i64,
    limit: usize,
) -> Result<Option<ReportAiStatus>, String> {
    let row = connection.query_row("SELECT version,preview,status,requests,input_chars,input_tokens,output_tokens,usage_pending,next_retry_at_ms,run_instance,overview,error FROM generations
        WHERE (?1 IS NULL OR version=?1) ORDER BY updated_at DESC,version LIMIT 1", [version], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, sql_count(row, 3)?, sql_count(row, 4)?, sql_tokens(row, 5)?, sql_tokens(row, 6)?, row.get::<_, bool>(7)?, row.get::<_, Option<i64>>(8)?, row.get::<_, Option<String>>(9)?, row.get::<_, Option<String>>(10)?, row.get::<_, Option<String>>(11)?))
    }).optional().map_err(|error| error.to_string())?;
    let Some((
        version,
        preview,
        status,
        requests,
        input_chars,
        input_tokens,
        output_tokens,
        usage_pending,
        next_retry_at_ms,
        saved_instance,
        overview,
        error,
    )) = row
    else {
        return Ok(None);
    };
    let status = if status == "running" && saved_instance.as_deref() != Some(run_instance()) {
        "interrupted".to_string()
    } else {
        status
    };
    let preview: ReportAiPreview =
        serde_json::from_str(&preview).map_err(|error| error.to_string())?;
    let completed_findings = connection
        .query_row(
            "SELECT count(*) FROM annotations WHERE version=?1",
            [&version],
            |row| sql_count(row, 0),
        )
        .map_err(|error| error.to_string())?;
    let mut statement = connection.prepare("SELECT payload FROM annotations WHERE version=?1 ORDER BY finding_id LIMIT ?2 OFFSET ?3").map_err(|error| error.to_string())?;
    let payloads = statement
        .query_map(params![version, limit as i64, offset], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    let rows = payloads
        .map(|payload| {
            serde_json::from_str(&payload.map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Some(ReportAiStatus {
        version,
        provider: preview.provider,
        model: preview.model,
        status,
        finding_count: preview.finding_count,
        completed_findings,
        requests,
        input_chars,
        input_tokens: if usage_pending { None } else { input_tokens },
        output_tokens: if usage_pending { None } else { output_tokens },
        next_retry_at_ms,
        overview: overview
            .map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
            .transpose()?,
        error,
        rows,
        export_generation_version: None,
        preserved_generation: None,
    }))
}

fn export_generation_version(connection: &Connection) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT g.version FROM generations g
             WHERE g.overview IS NOT NULL OR EXISTS (
                 SELECT 1 FROM annotations a WHERE a.version=g.version)
             ORDER BY CASE WHEN g.status='complete' THEN 0 ELSE 1 END,
                      g.updated_at DESC,g.version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub(super) fn selected_generation_version(path: &Path) -> Result<Option<String>, String> {
    let Some(connection) = readonly_database(path)? else {
        return Ok(None);
    };
    export_generation_version(&connection)
}

pub(super) fn export_annotation_reader(
    path: &Path,
) -> Result<impl FnMut(&str) -> Result<Option<SavedReportAnnotation>, String> + use<>, String> {
    let connection = readonly_database(path)?;
    let version = connection
        .as_ref()
        .map(export_generation_version)
        .transpose()?
        .flatten();
    Ok(move |finding_id: &str| {
        let (Some(connection), Some(version)) = (connection.as_ref(), version.as_ref()) else {
            return Ok(None);
        };
        let payload = connection
            .query_row(
                "SELECT payload FROM annotations WHERE version=?1 AND finding_id=?2",
                params![version, finding_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        payload
            .map(|payload| serde_json::from_str(&payload).map_err(|error| error.to_string()))
            .transpose()
    })
}

pub(super) fn export_overview(path: &Path) -> Result<Option<SavedReportOverview>, String> {
    let Some(connection) = readonly_database(path)? else {
        return Ok(None);
    };
    let Some(version) = export_generation_version(&connection)? else {
        return Ok(None);
    };
    let payload = connection
        .query_row(
            "SELECT overview FROM generations WHERE version=?1",
            [&version],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    payload
        .map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn sql_count(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<usize> {
    let value: i64 = row.get(index)?;
    usize::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn sql_tokens(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

async fn wait_cancelled(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_delay(seconds: u64, cancelled: &AtomicBool) -> bool {
    tokio::select! {
        biased;
        _ = wait_cancelled(cancelled) => false,
        _ = tokio::time::sleep(Duration::from_secs(seconds)) => true,
    }
}

fn mark_unknown_usage(connection: &Connection, version: &str) -> Result<(), String> {
    connection
        .execute(
            "UPDATE generations SET input_tokens=NULL,output_tokens=NULL,usage_pending=0 WHERE version=?1",
            [version],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

enum RequestOutcome {
    Completed(LlmCompletion),
    BudgetLimited,
    Cancelled,
    RetryPending(String),
    Failed(String),
}

struct RequestBudget<'a> {
    connection: &'a mut Connection,
    version: &'a str,
    ai: &'a AiState,
    settings: &'a AiSettings,
    cancelled: &'a AtomicBool,
    options: &'a ReportAiOptions,
    requests: &'a mut usize,
    input_chars: &'a mut usize,
}

struct RequestPrompt<'a> {
    chars: usize,
    system: &'a str,
    user: &'a str,
    phase: &'a str,
    completed: usize,
    total: usize,
}

async fn request_with_budget<F, Fut>(
    budget: RequestBudget<'_>,
    prompt: RequestPrompt<'_>,
    progress: &mut impl FnMut(&str, usize, usize),
    complete: &mut F,
) -> Result<RequestOutcome, String>
where
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<LlmCompletion, IntegrationError>>,
{
    let RequestBudget {
        connection,
        version,
        ai,
        settings,
        cancelled,
        options,
        requests,
        input_chars,
    } = budget;
    let RequestPrompt {
        chars: prompt_chars,
        system,
        user,
        phase,
        completed,
        total,
    } = prompt;
    let mut retries = 0_u32;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Ok(RequestOutcome::Cancelled);
        }
        if *requests >= options.max_requests
            || prompt_chars > options.max_input_chars.saturating_sub(*input_chars)
        {
            return Ok(RequestOutcome::BudgetLimited);
        }
        while let Err(wait) = ai.admit_request(settings.requests_per_minute) {
            progress("waitingForRateLimit", completed, total);
            if !wait_delay(wait, cancelled).await {
                return Ok(RequestOutcome::Cancelled);
            }
        }
        *requests += 1;
        *input_chars += prompt_chars;
        connection
            .execute(
                "UPDATE generations SET requests=requests+1,input_chars=input_chars+?2,
            usage_pending=1,updated_at=?3 WHERE version=?1",
                params![version, prompt_chars as i64, now_ms()],
            )
            .map_err(|error| error.to_string())?;
        progress(phase, completed, total);
        let response = tokio::select! {
            biased;
            _ = wait_cancelled(cancelled) => {
                mark_unknown_usage(connection, version)?;
                return Ok(RequestOutcome::Cancelled);
            },
            result = complete(system.to_owned(), user.to_owned()) => result,
        };
        match response {
            Ok(completion) => {
                connection.execute("UPDATE generations SET input_tokens=CASE WHEN requests=1 THEN ?2 ELSE input_tokens+?2 END,
                    output_tokens=CASE WHEN requests=1 THEN ?3 ELSE output_tokens+?3 END,usage_pending=0 WHERE version=?1",
                    params![version, completion.input_tokens.and_then(|value| i64::try_from(value).ok()),
                        completion.output_tokens.and_then(|value| i64::try_from(value).ok())])
                    .map_err(|error| error.to_string())?;
                return Ok(RequestOutcome::Completed(completion));
            }
            Err(IntegrationError::Retryable {
                status,
                retry_after_secs,
            }) => {
                mark_unknown_usage(connection, version)?;
                let delay = retry_after_secs.unwrap_or(2_u64.pow(retries));
                let delay_ms = i64::try_from(delay.saturating_mul(1000)).unwrap_or(i64::MAX);
                connection
                    .execute(
                        "UPDATE generations SET next_retry_at_ms=?2,updated_at=?3 WHERE version=?1",
                        params![version, now_ms().saturating_add(delay_ms), now_ms()],
                    )
                    .map_err(|error| error.to_string())?;
                if retries >= 2
                    || delay > 120
                    || *requests >= options.max_requests
                    || prompt_chars > options.max_input_chars.saturating_sub(*input_chars)
                {
                    return Ok(RequestOutcome::RetryPending(format!(
                        "Provider HTTP {status}; retry this generation later{}",
                        retry_after_secs
                            .map(|seconds| format!(" after at least {seconds} seconds"))
                            .unwrap_or_default()
                    )));
                }
                retries += 1;
                progress("waitingForProvider", completed, total);
                if !wait_delay(delay, cancelled).await {
                    return Ok(RequestOutcome::Cancelled);
                }
                connection
                    .execute(
                        "UPDATE generations SET next_retry_at_ms=NULL WHERE version=?1",
                        [version],
                    )
                    .map_err(|error| error.to_string())?;
            }
            Err(error) => {
                mark_unknown_usage(connection, version)?;
                return Ok(RequestOutcome::Failed(match error {
                    IntegrationError::InvalidData(message) => message,
                    _ => "The AI request failed. Check the provider settings and retry the unfinished work.".into(),
                }));
            }
        }
    }
}

async fn generate_with<F, Fut>(
    path: &Path,
    prepared: PreparedReportAi,
    ai: &AiState,
    settings: &AiSettings,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(&str, usize, usize),
    mut complete: F,
) -> Result<ReportAiStatus, String>
where
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<LlmCompletion, IntegrationError>>,
{
    let mut connection = database(path)?;
    let preview = &prepared.preview;
    let version = &preview.version;
    let retry_at = connection
        .query_row(
            "SELECT next_retry_at_ms FROM generations WHERE version=?1",
            [version],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    if retry_at.is_some_and(|deadline| deadline > now_ms()) {
        return read_status(path, Some(version), 0, 100)?
            .ok_or("The pending AI generation could not be reopened".into());
    }
    connection.execute("INSERT INTO generations(version,updated_at,preview,status,next_retry_at_ms,run_instance)
        VALUES(?1,?2,?3,'running',NULL,?4)
        ON CONFLICT(version) DO UPDATE SET updated_at=excluded.updated_at,preview=excluded.preview,
        status='running',next_retry_at_ms=NULL,run_instance=excluded.run_instance,error=NULL,
        input_tokens=CASE WHEN usage_pending=1 THEN NULL ELSE input_tokens END,
        output_tokens=CASE WHEN usage_pending=1 THEN NULL ELSE output_tokens END,
        usage_pending=0",
        params![version, now_ms(), serde_json::to_string(preview).map_err(|error| error.to_string())?, run_instance()]).map_err(|error| error.to_string())?;
    let mut requests = 0;
    let mut input_chars = 0;
    let mut completed = preview.completed_findings;
    let mut status = "budgetLimited";
    let mut failure = None;
    let mut continue_to_overview = true;
    for batch in prepared.batches {
        let outcome = request_with_budget(
            RequestBudget {
                connection: &mut connection,
                version,
                ai,
                settings,
                cancelled,
                options: &preview.options,
                requests: &mut requests,
                input_chars: &mut input_chars,
            },
            RequestPrompt {
                chars: batch.input_chars,
                system: &batch.system,
                user: &batch.user,
                phase: "generatingExplanations",
                completed,
                total: preview.finding_count,
            },
            &mut progress,
            &mut complete,
        )
        .await?;
        let completion = match outcome {
            RequestOutcome::Completed(completion) => completion,
            RequestOutcome::BudgetLimited => {
                continue_to_overview = false;
                break;
            }
            RequestOutcome::Cancelled => {
                status = "cancelled";
                continue_to_overview = false;
                break;
            }
            RequestOutcome::RetryPending(message) => {
                status = "retryPending";
                failure = Some(message);
                continue_to_overview = false;
                break;
            }
            RequestOutcome::Failed(message) => {
                status = "failed";
                failure = Some(message);
                continue_to_overview = false;
                break;
            }
        };
        let mut annotations = match parse_report_annotations(
            &completion.text,
            std::slice::from_ref(&batch.finding),
        ) {
            Ok(annotations) => annotations,
            Err(error) => {
                status = "failed";
                failure = Some(error.to_string());
                continue_to_overview = false;
                break;
            }
        };
        if cancelled.load(Ordering::SeqCst) {
            status = "cancelled";
            continue_to_overview = false;
            break;
        }
        let saved = SavedReportAnnotation {
            annotation: annotations.remove(0),
            input_digest: batch.digest,
            model: if completion.model.is_empty() {
                settings.model.clone()
            } else {
                completion.model
            },
            generated_at_ms: now_ms(),
            sample_count: batch.finding.samples.len(),
            evidence_total: batch.finding.evidence_total,
            input_tokens: completion.input_tokens,
            output_tokens: completion.output_tokens,
        };
        connection
            .execute(
                "INSERT INTO annotations(version,finding_id,payload) VALUES(?1,?2,?3)",
                params![
                    version,
                    saved.annotation.finding_id,
                    serde_json::to_string(&saved).map_err(|error| error.to_string())?
                ],
            )
            .map_err(|error| error.to_string())?;
        completed += 1;
        progress("savedExplanation", completed, preview.finding_count);
    }
    if continue_to_overview
        && completed == preview.finding_count
        && preview.overview_pending
        && preview.overview_planned
    {
        if let Some(batch) = overview_batch(
            &connection,
            version,
            completed,
            &prepared.language,
            prepared.max_prompt_chars,
        )? {
            let outcome = request_with_budget(
                RequestBudget {
                    connection: &mut connection,
                    version,
                    ai,
                    settings,
                    cancelled,
                    options: &preview.options,
                    requests: &mut requests,
                    input_chars: &mut input_chars,
                },
                RequestPrompt {
                    chars: batch.input_chars,
                    system: &batch.system,
                    user: &batch.user,
                    phase: "generatingOverview",
                    completed,
                    total: preview.finding_count,
                },
                &mut progress,
                &mut complete,
            )
            .await?;
            match outcome {
                RequestOutcome::Completed(completion) => {
                    match parse_report_overview(&completion.text, &batch.known_ids) {
                        Ok(overview) => {
                            if cancelled.load(Ordering::SeqCst) {
                                status = "cancelled";
                            } else {
                                let saved = SavedReportOverview {
                                    overview,
                                    input_digest: batch.digest,
                                    model: if completion.model.is_empty() {
                                        settings.model.clone()
                                    } else {
                                        completion.model
                                    },
                                    generated_at_ms: now_ms(),
                                    included_finding_count: batch.included_count,
                                    total_finding_count: batch.total_count,
                                    sampled_evidence_count: batch.sampled_evidence_count,
                                    partial_coverage: batch.included_count < batch.total_count,
                                    input_tokens: completion.input_tokens,
                                    output_tokens: completion.output_tokens,
                                };
                                connection
                                    .execute(
                                        "UPDATE generations SET overview=?2 WHERE version=?1",
                                        params![
                                            version,
                                            serde_json::to_string(&saved)
                                                .map_err(|error| error.to_string())?
                                        ],
                                    )
                                    .map_err(|error| error.to_string())?;
                                progress("savedOverview", completed, preview.finding_count);
                            }
                        }
                        Err(error) => {
                            status = "failed";
                            failure = Some(error.to_string());
                        }
                    }
                }
                RequestOutcome::BudgetLimited => {}
                RequestOutcome::Cancelled => {
                    status = "cancelled";
                }
                RequestOutcome::RetryPending(message) => {
                    status = "retryPending";
                    failure = Some(message);
                }
                RequestOutcome::Failed(message) => {
                    status = "failed";
                    failure = Some(message);
                }
            }
        } else {
            failure = Some("The overview input cannot fit the per-request AI input limit; increase it in Settings > AI".into());
        }
    }
    if completed == preview.finding_count && overview_exists(Some(&connection), version)? {
        status = "complete";
    }
    if cancelled.load(Ordering::SeqCst) {
        status = "cancelled";
    }
    connection
        .execute(
            "UPDATE generations SET status=?2,error=?3,updated_at=?4 WHERE version=?1",
            params![version, status, failure, now_ms()],
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    read_status(path, Some(version), 0, 100)?
        .ok_or("The AI generation could not be reopened".into())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RunReportAiRequest {
    request_id: String,
    report_id: String,
    expected_version: String,
    expected_preview_digest: String,
    options: ReportAiOptions,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RunComparisonAiRequest {
    request_id: String,
    comparison_id: String,
    expected_version: String,
    expected_preview_digest: String,
    options: ReportAiOptions,
}

async fn run_prepared_ai(
    app: &AppHandle,
    ai: &AiState,
    request_id: &str,
    path: &Path,
    prepared: PreparedReportAi,
    settings_and_key: (&AiSettings, String),
    cancelled: &AtomicBool,
) -> Result<ReportAiStatus, String> {
    let (settings, key) = settings_and_key;
    let client = LlmClient::new(LlmConfig {
        provider: settings.provider,
        api_key: key,
        model: settings.model.clone(),
        base_url: (!settings.base_url.is_empty()).then(|| settings.base_url.clone()),
        max_output_tokens: 4096,
    })
    .map_err(|error| error.to_string())?;
    generate_with(
        path,
        prepared,
        ai,
        settings,
        cancelled,
        |phase, completed, total| {
            let _ = app.emit(
                "audit-report-progress",
                serde_json::json!({
                    "requestId": request_id, "phase": phase, "completed": completed, "total": total
                }),
            );
        },
        |system, user| {
            let client = &client;
            async move { client.complete(&system, &user).await }
        },
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn run_audit_report_ai(
    app: AppHandle,
    state: State<'_, AppState>,
    reports: State<'_, AuditReportsState>,
    ai: State<'_, AiState>,
    request: RunReportAiRequest,
) -> Result<ReportAiStatus, String> {
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("The application is closing".into());
    }
    let job = reports.begin(&request.request_id)?;
    let directory = app_data_dir(&app)?;
    let settings_app = app.clone();
    let cancelled = job.cancelled.clone();
    let files = reports.files.lock().await;
    let (path, settings, key, prepared) = tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(&settings_app)?;
        let report = open_report(&directory, &request.report_id)?;
        let path = annotation_path(&directory, &request.report_id)?;
        let prepared = prepare_ai(&report, &path, &settings, request.options)?;
        if prepared.preview.version != request.expected_version
            || prepared.preview.preview_digest != request.expected_preview_digest {
            return Err("AI settings, data or budget changed. Review a fresh preview before sending.".into());
        }
        if prepared.preview.pending_findings > 0 && prepared.batches.is_empty() { return Err("The input budget cannot fit a finding. Increase it before generating explanations.".into()); }
        let key = load_api_key()?;
        Ok::<_, String>((path, settings, key, prepared))
    }).await.map_err(|error| error.to_string())??;
    // The frozen report and bounded prompts own all needed evidence. No crawl/file lock crosses a provider await.
    drop(files);
    run_prepared_ai(
        &app,
        &ai,
        &request.request_id,
        &path,
        prepared,
        (&settings, key),
        &cancelled,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn preview_audit_report_ai(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    report_id: String,
    options: ReportAiOptions,
) -> Result<ReportAiPreview, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(&app)?;
        let report = open_report(&directory, &report_id)?;
        Ok(prepare_ai(
            &report,
            &annotation_path(&directory, &report_id)?,
            &settings,
            options,
        )?
        .preview)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn get_audit_report_ai(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    report_id: String,
    offset: usize,
    limit: usize,
) -> Result<Option<ReportAiStatus>, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        drop(open_report(&directory, &report_id)?);
        read_status(
            &annotation_path(&directory, &report_id)?,
            None,
            offset,
            limit,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn preview_audit_report_comparison_ai(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    comparison_id: String,
    options: ReportAiOptions,
) -> Result<ReportAiPreview, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(&app)?;
        let comparison = open_comparison(&directory, &comparison_id)?;
        Ok(prepare_comparison_ai(
            &comparison,
            &comparison_annotation_path(&directory, &comparison_id)?,
            &settings,
            options,
        )?
        .preview)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn get_audit_report_comparison_ai(
    app: AppHandle,
    reports: State<'_, AuditReportsState>,
    comparison_id: String,
    offset: usize,
    limit: usize,
) -> Result<Option<ReportAiStatus>, String> {
    let directory = app_data_dir(&app)?;
    let _files = reports.files.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        drop(open_comparison(&directory, &comparison_id)?);
        read_status(
            &comparison_annotation_path(&directory, &comparison_id)?,
            None,
            offset,
            limit,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn run_audit_report_comparison_ai(
    app: AppHandle,
    state: State<'_, AppState>,
    reports: State<'_, AuditReportsState>,
    ai: State<'_, AiState>,
    request: RunComparisonAiRequest,
) -> Result<ReportAiStatus, String> {
    if state.exit_confirmed.load(Ordering::SeqCst) {
        return Err("The application is closing".into());
    }
    let job = reports.begin(&request.request_id)?;
    let directory = app_data_dir(&app)?;
    let settings_app = app.clone();
    let cancelled = job.cancelled.clone();
    let files = reports.files.lock().await;
    let (path, settings, key, prepared) = tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(&settings_app)?;
        let comparison = open_comparison(&directory, &request.comparison_id)?;
        let path = comparison_annotation_path(&directory, &request.comparison_id)?;
        let prepared = prepare_comparison_ai(&comparison, &path, &settings, request.options)?;
        if prepared.preview.version != request.expected_version
            || prepared.preview.preview_digest != request.expected_preview_digest {
            return Err("AI settings, data or budget changed. Review a fresh preview before sending.".into());
        }
        if prepared.preview.pending_findings > 0 && prepared.batches.is_empty() {
            return Err("The input budget cannot fit a finding. Increase it before generating explanations.".into());
        }
        let key = load_api_key()?;
        Ok::<_, String>((path, settings, key, prepared))
    }).await.map_err(|error| error.to_string())??;
    drop(files);
    run_prepared_ai(
        &app,
        &ai,
        &request.request_id,
        &path,
        prepared,
        (&settings, key),
        &cancelled,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{
        ActiveStore, AuditReportRequest, AuditSourceStatus, CrawlRecord, CrawlStore, GridQuery,
    };

    fn fixture_with_records(directory: &Path, records: usize) -> AuditReportStore {
        let source = ActiveStore::memory();
        for index in 0..records {
            let mut row = CrawlRecord::pending(
                format!("https://user:password@example.test/{index}?token=secret#private"),
                0,
            );
            row.status_code = Some(200);
            row.content_type = Some("text/html".into());
            source.upsert(row);
        }
        AuditReportStore::prepare(
            directory.join("source.sqlite3"),
            &source,
            AuditReportRequest {
                id: "report-ai".into(),
                title: "AI evidence".into(),
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
        .unwrap()
    }

    fn fixture(directory: &Path) -> AuditReportStore {
        fixture_with_records(directory, 3)
    }

    fn comparison_fixture(directory: &Path) -> AuditReportComparisonStore {
        let baseline_dir = directory.join("baseline");
        let current_dir = directory.join("current");
        std::fs::create_dir(&baseline_dir).unwrap();
        std::fs::create_dir(&current_dir).unwrap();
        let baseline = fixture(&baseline_dir);
        let current = fixture(&current_dir);
        AuditReportComparisonStore::prepare(
            directory.join("comparison.sqlite3"),
            &baseline,
            &current,
            |_| true,
        )
        .unwrap()
    }

    fn options(requests: usize) -> ReportAiOptions {
        ReportAiOptions {
            max_requests: requests,
            max_input_chars: 200_000,
            samples_per_finding: 2,
        }
    }

    fn completion(user: &str) -> LlmCompletion {
        let value: serde_json::Value = serde_json::from_str(user).unwrap();
        if value.get("includedFindingCount").is_some() {
            return LlmCompletion { model: "mock-model".into(), input_tokens: Some(11), output_tokens: Some(7),
                text: serde_json::json!({
                    "summary": "Review the captured issues and their recommended changes.",
                    "prioritizedFindingIds": [value["findings"][0]["findingId"]],
                    "limitations": ["The overview reflects only validated, selected finding annotations."]
                }).to_string() };
        }
        let finding = &value["findings"][0];
        LlmCompletion { model: "mock-model".into(), input_tokens: Some(11), output_tokens: Some(7), text: serde_json::json!({
            "annotations": [{"findingId": finding["findingId"], "evidenceIds": [finding["samples"][0]["id"]],
                "explanation": "The saved evidence contains this measured issue.", "proposedCause": null,
                "recommendation": "Update the affected pages.", "verification": "Recrawl the same scope.", "suggestedTeam": "Content"}]
        }).to_string() }
    }

    #[test]
    fn comparison_input_uses_bounded_before_after_and_known_sample_ids_without_changing_status() {
        let directory = tempfile::tempdir().unwrap();
        let comparison = comparison_fixture(directory.path());
        let measured = comparison
            .query_findings(AuditComparisonFindingQuery::default())
            .unwrap()
            .rows
            .into_iter()
            .find(|finding| finding.finding_id == "title.missing")
            .unwrap();
        let status = serde_json::to_value(&measured.status).unwrap();
        let batch = comparison_finding_prompt(&comparison, measured, 3, "en", 20_000).unwrap();
        let payload: serde_json::Value = serde_json::from_str(&batch.user).unwrap();
        assert_eq!(payload["findings"][0]["comparison"]["status"], status);
        assert_eq!(batch.finding.samples.len(), 3);
        assert!(batch.user.contains("comparison-ev-"));
        for secret in ["password", "token=secret", "#private"] {
            assert!(!batch.user.contains(secret));
        }
        let response = serde_json::json!({"annotations":[{
            "findingId": batch.finding.finding_id,
            "evidenceIds": ["comparison-ev-unknown"],
            "explanation": "The captured comparison changed.", "proposedCause": null,
            "recommendation": "Review the affected pages.", "verification": "Recrawl the same scope.",
            "suggestedTeam": "Content"
        }]});
        assert!(parse_report_annotations(&response.to_string(), &[batch.finding]).is_err());
        assert_eq!(
            serde_json::to_value(
                comparison
                    .query_findings(AuditComparisonFindingQuery::default())
                    .unwrap()
                    .rows
                    .into_iter()
                    .find(|finding| finding.finding_id == "title.missing")
                    .unwrap()
                    .status
            )
            .unwrap(),
            status
        );
    }

    #[tokio::test]
    async fn comparison_generation_resumes_its_own_sidecar_and_preserves_partial_annotations_on_new_failure()
     {
        let directory = tempfile::tempdir().unwrap();
        let comparison = comparison_fixture(directory.path());
        let path = comparison_annotation_path(directory.path(), "comparison-ai").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        assert_ne!(
            path,
            annotation_path(directory.path(), "comparison-ai").unwrap()
        );
        assert!(comparison_annotation_path(directory.path(), "../escape").is_err());
        let settings = AiSettings::default();
        let ai = AiState::default();
        let cancelled = AtomicBool::new(false);
        let prepared = prepare_comparison_ai(&comparison, &path, &settings, options(1)).unwrap();
        assert!(!path.exists());
        assert!(prepared.preview.finding_count > 1);
        assert_eq!(prepared.preview.estimated_requests, 1);
        let version = prepared.preview.version.clone();
        let partial = generate_with(&path, prepared, &ai, &settings, &cancelled, |_, _, _| {},
            |_, user| async move {
                let payload: serde_json::Value = serde_json::from_str(&user).unwrap();
                let finding = &payload["findings"][0];
                let ids = finding["samples"].as_array().unwrap().first()
                    .map(|sample| vec![sample["id"].clone()]).unwrap_or_default();
                Ok(LlmCompletion { model: "comparison-mock".into(), input_tokens: Some(9), output_tokens: Some(5),
                    text: serde_json::json!({"annotations":[{
                        "findingId": finding["findingId"], "evidenceIds": ids,
                        "explanation": "The measured state is preserved.", "proposedCause": null,
                        "recommendation": "Review the saved evidence.", "verification": "Recrawl the same scope.",
                        "suggestedTeam": "Engineering"
                    }]}).to_string() })
            }).await.unwrap();
        assert_eq!(partial.status, "budgetLimited");
        assert_eq!(partial.completed_findings, 1);
        assert_eq!(
            prepare_comparison_ai(&comparison, &path, &settings, options(100))
                .unwrap()
                .preview
                .version,
            version
        );
        assert_eq!(
            prepare_comparison_ai(&comparison, &path, &settings, options(100))
                .unwrap()
                .preview
                .completed_findings,
            1
        );
        let mut changed = settings.clone();
        changed.model = "changed-model".into();
        let failed = generate_with(
            &path,
            prepare_comparison_ai(&comparison, &path, &changed, options(1)).unwrap(),
            &ai,
            &changed,
            &cancelled,
            |_, _, _| {},
            |_, _| async {
                Ok(LlmCompletion {
                    model: "changed-model".into(),
                    text: "invalid response".into(),
                    input_tokens: None,
                    output_tokens: None,
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.completed_findings, 0);
        let status = read_status(&path, None, 0, 100).unwrap().unwrap();
        assert_eq!(status.version, failed.version);
        assert_eq!(
            status.export_generation_version.as_deref(),
            Some(version.as_str())
        );
        assert_eq!(status.preserved_generation.unwrap().completed_findings, 1);
        assert!(
            !annotation_path(directory.path(), "comparison-ai")
                .unwrap()
                .exists()
        );
    }

    #[tokio::test]
    async fn budgeted_generations_resume_only_unfinished_findings_and_reject_unknown_references() {
        let directory = tempfile::tempdir().unwrap();
        let report = fixture(directory.path());
        let path = directory.path().join("annotations.ai");
        let settings = AiSettings::default();
        let ai = AiState::default();
        let cancelled = AtomicBool::new(false);
        let prepared = prepare_ai(&report, &path, &settings, options(1)).unwrap();
        assert!(
            !path.exists(),
            "Preflight must not create annotations or contact a provider"
        );
        assert!(prepared.preview.finding_count > 1);
        assert_eq!(prepared.preview.estimated_requests, 1);
        let original_version = prepared.preview.version.clone();
        for prompt in &prepared.batches {
            for secret in ["password", "token=secret", "#private"] {
                assert!(!prompt.user.contains(secret));
            }
        }
        let first = generate_with(
            &path,
            prepared,
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move { Ok(completion(&user)) },
        )
        .await
        .unwrap();
        assert_eq!(first.status, "budgetLimited");
        assert_eq!(first.completed_findings, 1);
        assert_eq!(first.input_tokens, Some(11));
        let saved_first = serde_json::to_string(&first.rows[0]).unwrap();
        let larger_budget = prepare_ai(&report, &path, &settings, options(100)).unwrap();
        assert_eq!(larger_budget.preview.version, original_version);
        assert_ne!(
            larger_budget.preview.preview_digest,
            prepare_ai(&report, &path, &settings, options(1))
                .unwrap()
                .preview
                .preview_digest
        );
        assert_eq!(larger_budget.preview.completed_findings, 1);
        let prepared = larger_budget;
        assert_eq!(prepared.preview.version, original_version);
        assert_eq!(prepared.preview.completed_findings, 1);
        assert!(
            prepared
                .batches
                .iter()
                .all(|batch| batch.finding.finding_id != first.rows[0].annotation.finding_id)
        );
        let failed = generate_with(
            &path,
            prepared,
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move {
                let mut reply = completion(&user);
                let mut payload: serde_json::Value = serde_json::from_str(&reply.text).unwrap();
                payload["annotations"][0]["evidenceIds"] = serde_json::json!(["invented-evidence"]);
                reply.text = payload.to_string();
                Ok(reply)
            },
        )
        .await
        .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.completed_findings, 1);
        assert_eq!(serde_json::to_string(&failed.rows[0]).unwrap(), saved_first);
        let prepared = prepare_ai(&report, &path, &settings, options(100)).unwrap();
        let completed = generate_with(
            &path,
            prepared,
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move { Ok(completion(&user)) },
        )
        .await
        .unwrap();
        assert_eq!(completed.status, "complete");
        assert_eq!(completed.completed_findings, completed.finding_count);
        assert!(completed.overview.is_some());
        assert_eq!(report.summary().unwrap().scope_records, 3);
        let evidence = report
            .query_evidence(AuditEvidenceQuery {
                finding_id: "title.missing".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            evidence.total, 3,
            "Provider failures must never remove affected URLs"
        );
        assert_eq!(
            read_status(&path, None, 0, 1).unwrap().unwrap().rows.len(),
            1
        );
        assert!(read_status(&path, None, 0, 101).is_err());
        let mut changed = settings.clone();
        changed.model = "another-model".into();
        let preview = prepare_ai(&report, &path, &changed, options(1))
            .unwrap()
            .preview;
        assert_ne!(preview.version, original_version);
        assert_eq!(preview.completed_findings, 0);
    }

    #[tokio::test]
    async fn completed_generation_remains_the_export_source_after_new_versions_fail_or_run_out_of_budget()
     {
        let directory = tempfile::tempdir().unwrap();
        let report = fixture(directory.path());
        let path = directory.path().join("annotations.ai");
        let ai = AiState::default();
        let cancelled = AtomicBool::new(false);
        let settings = AiSettings::default();
        let complete = generate_with(
            &path,
            prepare_ai(&report, &path, &settings, options(100)).unwrap(),
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move { Ok(completion(&user)) },
        )
        .await
        .unwrap();
        assert_eq!(complete.status, "complete");
        let original_version = complete.version;
        let original_overview = complete.overview.unwrap();
        let original_finding_id = complete.rows[0].annotation.finding_id.clone();

        let mut failed_settings = settings.clone();
        failed_settings.model = "replacement-model".into();
        let failed = generate_with(
            &path,
            prepare_ai(&report, &path, &failed_settings, options(100)).unwrap(),
            &ai,
            &failed_settings,
            &cancelled,
            |_, _, _| {},
            |_, _| async {
                Ok(LlmCompletion {
                    model: "replacement-model".into(),
                    text: "invalid response".into(),
                    input_tokens: None,
                    output_tokens: None,
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(failed.status, "failed");
        let latest = read_status(&path, None, 0, 100).unwrap().unwrap();
        assert_eq!(latest.version, failed.version);
        assert_eq!(
            latest.export_generation_version.as_deref(),
            Some(original_version.as_str())
        );
        let preserved = latest.preserved_generation.unwrap();
        assert_eq!(preserved.version, original_version);
        assert_eq!(preserved.model, settings.model);
        assert_eq!(preserved.completed_findings, preserved.finding_count);
        assert_eq!(
            preserved.overview.unwrap().input_digest,
            original_overview.input_digest
        );
        assert_eq!(
            export_overview(&path).unwrap().unwrap().input_digest,
            original_overview.input_digest
        );
        assert_eq!(
            export_annotation_reader(&path).unwrap()(&original_finding_id)
                .unwrap()
                .unwrap()
                .model,
            original_overview.model
        );

        let mut limited_settings = settings.clone();
        limited_settings.model = "budget-model".into();
        let limited = generate_with(
            &path,
            prepare_ai(&report, &path, &limited_settings, options(1)).unwrap(),
            &ai,
            &limited_settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move { Ok(completion(&user)) },
        )
        .await
        .unwrap();
        assert_eq!(limited.status, "budgetLimited");
        assert_ne!(limited.version, original_version);
        let latest = read_status(&path, None, 0, 100).unwrap().unwrap();
        assert_eq!(latest.version, limited.version);
        assert_eq!(
            latest.export_generation_version.as_deref(),
            Some(original_version.as_str())
        );
        assert_eq!(
            latest.preserved_generation.unwrap().version,
            original_version
        );
        assert_eq!(
            export_overview(&path).unwrap().unwrap().input_digest,
            original_overview.input_digest
        );
        assert_eq!(
            export_annotation_reader(&path).unwrap()(&original_finding_id)
                .unwrap()
                .unwrap()
                .model,
            original_overview.model
        );
        let package = directory.path().join("package");
        std::fs::create_dir(&package).unwrap();
        let selected_version = selected_generation_version(&path).unwrap();
        let manifest = ferrous_frog_export::write_audit_report_package_with_annotations(
            &report,
            &package,
            export_annotation_reader(&path).unwrap(),
            export_overview(&path)
                .unwrap()
                .map(serde_json::to_value)
                .transpose()
                .unwrap(),
            selected_version.as_deref(),
            |_| true,
        )
        .unwrap();
        assert_eq!(
            manifest.ai_generation_version.as_deref(),
            Some(original_version.as_str())
        );
        assert_eq!(manifest.ai_annotated_findings, manifest.finding_count);
        assert!(manifest.ai_overview.is_some());
        assert!(
            std::fs::read_to_string(package.join("index.html"))
                .unwrap()
                .contains(&format!("AI generation: {original_version}"))
        );
    }

    #[tokio::test]
    async fn partial_generation_remains_available_when_new_version_saves_no_annotations() {
        let directory = tempfile::tempdir().unwrap();
        let report = fixture(directory.path());
        let path = directory.path().join("annotations.ai");
        let ai = AiState::default();
        let cancelled = AtomicBool::new(false);
        let settings = AiSettings::default();
        let partial = generate_with(
            &path,
            prepare_ai(&report, &path, &settings, options(1)).unwrap(),
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move { Ok(completion(&user)) },
        )
        .await
        .unwrap();
        assert_eq!(partial.status, "budgetLimited");
        assert_eq!(partial.completed_findings, 1);
        let original_finding_id = partial.rows[0].annotation.finding_id.clone();

        let mut changed = settings.clone();
        changed.model = "failing-model".into();
        let failed = generate_with(
            &path,
            prepare_ai(&report, &path, &changed, options(1)).unwrap(),
            &ai,
            &changed,
            &cancelled,
            |_, _, _| {},
            |_, _| async {
                Ok(LlmCompletion {
                    model: "failing-model".into(),
                    text: "invalid response".into(),
                    input_tokens: None,
                    output_tokens: None,
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.completed_findings, 0);
        let latest = read_status(&path, None, 0, 100).unwrap().unwrap();
        assert_eq!(latest.version, failed.version);
        assert_eq!(
            latest.export_generation_version.as_deref(),
            Some(partial.version.as_str())
        );
        assert_eq!(latest.preserved_generation.unwrap().completed_findings, 1);
        assert_eq!(
            selected_generation_version(&path).unwrap().as_deref(),
            Some(partial.version.as_str())
        );
        assert!(
            export_annotation_reader(&path).unwrap()(&original_finding_id)
                .unwrap()
                .is_some()
        );
        assert!(export_overview(&path).unwrap().is_none());
    }

    #[tokio::test]
    async fn overview_waits_for_a_separate_budget_slot_without_changing_finding_completion() {
        let directory = tempfile::tempdir().unwrap();
        let report = fixture(directory.path());
        let path = directory.path().join("annotations.ai");
        let settings = AiSettings::default();
        let finding_count = report.summary().unwrap().finding_count;
        let prepared = prepare_ai(&report, &path, &settings, options(finding_count)).unwrap();
        assert!(!prepared.preview.overview_planned);
        assert_eq!(prepared.preview.estimated_requests, finding_count);
        let ai = AiState::default();
        let cancelled = AtomicBool::new(false);
        let findings_only = generate_with(
            &path,
            prepared,
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move { Ok(completion(&user)) },
        )
        .await
        .unwrap();
        assert_eq!(findings_only.status, "budgetLimited");
        assert_eq!(findings_only.completed_findings, finding_count);
        assert!(findings_only.overview.is_none());
        assert!(export_overview(&path).unwrap().is_none());
        let prepared = prepare_ai(&report, &path, &settings, options(1)).unwrap();
        assert_eq!(prepared.preview.pending_findings, 0);
        assert!(prepared.preview.overview_planned);
        assert_eq!(prepared.preview.estimated_requests, 1);
        let rejected = generate_with(
            &path,
            prepared,
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move {
                let mut reply = completion(&user);
                reply.text = serde_json::json!({
                    "summary": "Review the saved findings.",
                    "prioritizedFindingIds": ["invented.rule"],
                    "limitations": ["Only validated annotations were supplied."]
                })
                .to_string();
                Ok(reply)
            },
        )
        .await
        .unwrap();
        assert_eq!(rejected.status, "failed");
        assert_eq!(rejected.completed_findings, finding_count);
        assert!(rejected.overview.is_none());
        assert!(export_overview(&path).unwrap().is_none());
        let prepared = prepare_ai(&report, &path, &settings, options(1)).unwrap();
        let complete = generate_with(
            &path,
            prepared,
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, user| async move { Ok(completion(&user)) },
        )
        .await
        .unwrap();
        assert_eq!(complete.status, "complete");
        assert_eq!(complete.completed_findings, finding_count);
        assert_eq!(complete.requests, finding_count + 2);
        let overview = complete.overview.unwrap();
        assert_eq!(overview.total_finding_count, finding_count);
        assert!(overview.included_finding_count <= 20);
        assert_eq!(
            export_overview(&path).unwrap().unwrap().input_digest,
            overview.input_digest
        );
        assert_eq!(
            read_status(&path, None, 0, 1).unwrap().unwrap().rows.len(),
            1
        );
    }

    #[tokio::test]
    async fn provider_delays_and_cancellation_preserve_local_evidence_without_unknown_usage_becoming_zero()
     {
        let directory = tempfile::tempdir().unwrap();
        let report = fixture(directory.path());
        let path = directory.path().join("annotations.ai");
        let settings = AiSettings::default();
        let ai = AiState::default();
        let cancelled = AtomicBool::new(false);
        let prepared = prepare_ai(&report, &path, &settings, options(2)).unwrap();
        let delayed = generate_with(
            &path,
            prepared,
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, _| async {
                Err(IntegrationError::Retryable {
                    status: 429,
                    retry_after_secs: Some(180),
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(delayed.status, "retryPending");
        assert_eq!(delayed.requests, 1);
        assert!(delayed.next_retry_at_ms.unwrap() >= now_ms() + 179_000);
        assert_eq!(delayed.input_tokens, None);
        assert!(delayed.error.unwrap().contains("180"));
        let prepared = prepare_ai(&report, &path, &settings, options(2)).unwrap();
        let blocked = generate_with(
            &path,
            prepared,
            &ai,
            &settings,
            &cancelled,
            |_, _, _| {},
            |_, _| async { panic!("Retry-After must prevent an immediate provider call") },
        )
        .await
        .unwrap();
        assert_eq!(blocked.status, "retryPending");
        assert_eq!(blocked.requests, 1);
        database(&path)
            .unwrap()
            .execute("UPDATE generations SET next_retry_at_ms=NULL", [])
            .unwrap();
        let prepared = prepare_ai(&report, &path, &settings, options(2)).unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            generate_with(
                &path,
                prepared,
                &ai,
                &settings,
                &cancelled,
                |phase, _, _| {
                    if phase == "generatingExplanations" {
                        cancelled.store(true, Ordering::SeqCst);
                    }
                },
                |_, _| std::future::pending::<Result<LlmCompletion, IntegrationError>>(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.status, "cancelled");
        assert_eq!(result.completed_findings, 0);
        assert_eq!(result.input_tokens, None);
        assert_eq!(report.summary().unwrap().scope_records, 3);
    }

    #[test]
    fn evidence_sampling_spans_the_full_population_and_exposes_actual_count() {
        assert_eq!(sample_offsets(21, 3), [0, 10, 20]);
        assert_eq!(sample_offsets(3, 10), [0, 1, 2]);
        let directory = tempfile::tempdir().unwrap();
        let report = fixture_with_records(directory.path(), 21);
        let finding = report
            .query_findings(Default::default())
            .unwrap()
            .rows
            .into_iter()
            .find(|finding| finding.id == "title.missing")
            .unwrap();
        let prompt = finding_prompt(&report, finding, 3, "en", 200_000).unwrap();
        let actual = prompt
            .finding
            .samples
            .iter()
            .map(|sample| sample.id.as_str())
            .collect::<Vec<_>>();
        let expected = [0, 10, 20].map(|offset| {
            report
                .query_evidence(AuditEvidenceQuery {
                    finding_id: "title.missing".into(),
                    offset,
                    limit: 1,
                    ..Default::default()
                })
                .unwrap()
                .rows
                .remove(0)
                .id
        });
        assert_eq!(
            actual,
            expected.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(prompt.finding.evidence_total, 21);
        let prepared = prepare_ai(
            &report,
            &directory.path().join("annotations.ai"),
            &AiSettings::default(),
            ReportAiOptions {
                max_requests: 100,
                max_input_chars: 200_000,
                samples_per_finding: 3,
            },
        )
        .unwrap();
        assert!(prepared.preview.sampled_evidence >= 3);
        assert!(prepared.preview.sampling_policy.contains("Evenly spaced"));
    }

    #[tokio::test]
    async fn failed_attempt_after_known_usage_makes_generation_usage_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let report = fixture(directory.path());
        let path = directory.path().join("annotations.ai");
        let settings = AiSettings::default();
        let prepared = prepare_ai(&report, &path, &settings, options(2)).unwrap();
        let mut calls = 0;
        let result = generate_with(
            &path,
            prepared,
            &AiState::default(),
            &settings,
            &AtomicBool::new(false),
            |_, _, _| {},
            |_, user| {
                calls += 1;
                let result = if calls == 1 {
                    Ok(completion(&user))
                } else {
                    Err(IntegrationError::InvalidData("Bad response".into()))
                };
                async move { result }
            },
        )
        .await
        .unwrap();
        assert_eq!(result.status, "failed");
        assert_eq!(result.requests, 2);
        assert_eq!(result.completed_findings, 1);
        assert_eq!(result.input_tokens, None);
        assert_eq!(result.output_tokens, None);
        assert_eq!(result.rows[0].input_tokens, Some(11));
    }

    #[test]
    fn interrupted_generation_is_visible_after_reopen_and_annotations_export_one_at_a_time() {
        let directory = tempfile::tempdir().unwrap();
        let report = fixture(directory.path());
        let path = directory.path().join("annotations.ai");
        let settings = AiSettings::default();
        let prepared = prepare_ai(&report, &path, &settings, options(1)).unwrap();
        let version = prepared.preview.version.clone();
        let mut reader = export_annotation_reader(&path).unwrap();
        assert!(reader("title.missing").unwrap().is_none());
        drop(reader);
        let connection = database(&path).unwrap();
        connection
            .execute(
                "INSERT INTO generations(version,updated_at,preview,status,run_instance,requests,input_tokens,output_tokens,usage_pending)
            VALUES(?1,?2,?3,'running','another-process',2,18,9,1)",
                params![
                    version,
                    now_ms(),
                    serde_json::to_string(&prepared.preview).unwrap()
                ],
            )
            .unwrap();
        let interrupted = read_status(&path, Some(&version), 0, 1).unwrap().unwrap();
        assert_eq!(interrupted.status, "interrupted");
        assert_eq!(interrupted.input_tokens, None);
        assert_eq!(interrupted.output_tokens, None);
        let saved = SavedReportAnnotation {
            annotation: ReportAnnotation {
                finding_id: "title.missing".into(),
                evidence_ids: vec!["sample".into()],
                explanation: "Stored".into(),
                proposed_cause: None,
                recommendation: "Fix".into(),
                verification: "Recrawl".into(),
                suggested_team: "Content".into(),
            },
            input_digest: "digest".into(),
            model: "mock".into(),
            generated_at_ms: now_ms(),
            sample_count: 1,
            evidence_total: 3,
            input_tokens: Some(1),
            output_tokens: Some(2),
        };
        connection
            .execute(
                "INSERT INTO annotations(version,finding_id,payload) VALUES(?1,?2,?3)",
                params![
                    version,
                    saved.annotation.finding_id,
                    serde_json::to_string(&saved).unwrap()
                ],
            )
            .unwrap();
        let mut reader = export_annotation_reader(&path).unwrap();
        assert_eq!(
            reader("title.missing")
                .unwrap()
                .unwrap()
                .annotation
                .explanation,
            "Stored"
        );
        assert!(reader("meta.missing").unwrap().is_none());
    }
}
