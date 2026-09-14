//! Portable offline packages for comparisons of frozen audit reports.
use crate::audit_report::{
    AUDIT_REPORT_HTML_PAGE_SIZE, AuditReportExportProgress, localized_finding_title,
};
use csv::Writer;
use ferrous_frog_storage::*;
use minijinja::{Environment, context};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparisonManifestFinding {
    pub finding_id: String,
    pub status: AuditComparisonStatus,
    pub added: usize,
    pub persisting: usize,
    pub resolved: usize,
    pub not_observed: usize,
    pub newly_observed_current: usize,
    pub unverified_current: usize,
    pub count_unit: String,
    pub compatibility_reasons: Vec<String>,
    pub html_pages: Vec<String>,
    pub csv_file: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparisonManifest {
    pub schema_version: u32,
    pub status: String,
    pub comparison_status: String,
    pub baseline_report_id: String,
    pub current_report_id: String,
    pub finding_count: usize,
    pub evidence_rows: usize,
    #[serde(default)]
    pub ai_annotated_findings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_overview: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_generation_version: Option<String>,
    pub compatibility_reasons: Vec<String>,
    pub findings: Vec<AuditComparisonManifestFinding>,
    pub files: Vec<String>,
    pub summary: AuditComparisonSummary,
}

fn comparison_labels(language: &AuditReportLanguage) -> serde_json::Value {
    let t = |english, turkish| {
        if *language == AuditReportLanguage::Turkish {
            turkish
        } else {
            english
        }
    };
    let mut labels = serde_json::json!({
        "language": t("en", "tr"),
        "brandNote": t("AUDIT REPORT FOLLOW-UP", "DENETİM RAPORU KARŞILAŞTIRMASI"),
        "eyebrow": t("FROZEN REPORT COMPARISON", "DONDURULMUŞ RAPOR KARŞILAŞTIRMASI"),
        "lead": t("Measurements compare saved evidence only. Missing current evidence is not a verified fix.",
            "Ölçümler yalnızca kaydedilmiş kanıtları karşılaştırır. Güncel kanıtın olmaması doğrulanmış bir düzeltme değildir."),
        "baseline": t("Baseline", "Önceki rapor"), "current": t("Current", "Güncel rapor"),
        "status": t("Status", "Durum"), "ready": t("Ready", "Hazır"),
        "findings": t("Findings", "Bulgular"), "evidenceRows": t("Evidence rows", "Kanıt satırları"),
        "compatibilityNotes": t("Compatibility notes", "Karşılaştırılabilirlik notları"),
        "computedChanges": t("Computed finding changes", "Hesaplanan bulgu değişimleri"),
        "added": t("added", "eklendi"), "persisting": t("persisting", "sürüyor"),
        "resolved": t("resolved", "giderildi"), "notObserved": t("not observed", "gözlemlenmedi"),
        "notObservedWarning": t("Not observed does not verify a fix.",
            "Gözlemlenmemesi düzeltmenin doğrulandığı anlamına gelmez."),
        "viewEvidence": t("View evidence", "Kanıtları görüntüle"),
        "downloadCsv": t("Download complete CSV", "Eksiksiz CSV dosyasını indir"),
        "frozenMetadata": t("Frozen comparison metadata", "Dondurulmuş karşılaştırma üstverisi"),
        "footer": t("Prepared by Ferrous Frog · Offline report comparison · No external resources",
            "Ferrous Frog ile hazırlandı · Çevrimdışı rapor karşılaştırması · Harici kaynak yok"),
        "overview": t("Comparison overview", "Karşılaştırma özeti"),
        "previous": t("Previous", "Önceki"), "next": t("Next", "Sonraki"),
        "comparisonEvidenceRows": t("comparison evidence rows", "karşılaştırma kanıtı satırı"),
        "originalUrl": t("Original URL", "Orijinal URL"), "state": t("State", "Durum"),
        "before": t("Before", "Önce"), "after": t("After", "Sonra"),
        "capturedRow": t("Captured comparison row", "Kaydedilmiş karşılaştırma satırı"),
        "occurrence": t("Occurrence", "Oluşum"), "fullEvidence": t("Full evidence", "Kanıtın tamamı")
    });
    let ai_labels = serde_json::json!({
        "aiOverview": t("AI comparison overview", "Yapay zekâ karşılaştırma özeti"),
        "aiExplanation": t("AI explanation", "Yapay zekâ açıklaması"),
        "aiDisclosure": t("AI interpretations are unverified. Computed counts and comparison states remain the source of truth.",
            "Yapay zekâ yorumları doğrulanmamıştır. Hesaplanan sayılar ve karşılaştırma durumları esas alınmalıdır."),
        "aiGeneration": t("AI generation", "Yapay zekâ üretim sürümü"),
        "aiPriorities": t("Suggested priorities", "Önerilen öncelikler"),
        "aiLimitations": t("Limitations", "Sınırlamalar"),
        "aiCoverage": t("Included findings", "Dahil edilen bulgular"),
        "aiSamples": t("Sampled evidence", "Örneklenen kanıt"),
        "aiPartial": t("Partial coverage", "Kısmi kapsam"),
        "aiModel": t("Model", "Model"),
        "aiReferences": t("Cited evidence IDs", "Atıf yapılan kanıt kimlikleri"),
        "proposedCause": t("Proposed cause — unverified", "Önerilen neden — doğrulanmamış"),
        "recommendation": t("Recommended action", "Önerilen işlem"),
        "verification": t("Verify the change", "Değişikliği doğrulayın")
    });
    labels
        .as_object_mut()
        .unwrap()
        .extend(ai_labels.as_object().unwrap().clone());
    labels
}

fn status_label(status: &AuditComparisonStatus, language: &AuditReportLanguage) -> &'static str {
    let turkish = *language == AuditReportLanguage::Turkish;
    match (status, turkish) {
        (AuditComparisonStatus::New, false) => "New",
        (AuditComparisonStatus::New, true) => "Yeni",
        (AuditComparisonStatus::Resolved, false) => "Resolved",
        (AuditComparisonStatus::Resolved, true) => "Giderildi",
        (AuditComparisonStatus::Improved, false) => "Improved",
        (AuditComparisonStatus::Improved, true) => "İyileşti",
        (AuditComparisonStatus::Unchanged, false) => "Unchanged",
        (AuditComparisonStatus::Unchanged, true) => "Değişmedi",
        (AuditComparisonStatus::Worsened, false) => "Worsened",
        (AuditComparisonStatus::Worsened, true) => "Kötüleşti",
        (AuditComparisonStatus::MixedChanges, false) => "Mixed changes",
        (AuditComparisonStatus::MixedChanges, true) => "Karışık değişiklikler",
        (AuditComparisonStatus::NotComparable, false) => "Not comparable",
        (AuditComparisonStatus::NotComparable, true) => "Karşılaştırılamaz",
    }
}

fn state_label(
    state: &AuditComparisonEvidenceState,
    language: &AuditReportLanguage,
) -> &'static str {
    let turkish = *language == AuditReportLanguage::Turkish;
    match (state, turkish) {
        (AuditComparisonEvidenceState::Added, false) => "Added",
        (AuditComparisonEvidenceState::Added, true) => "Eklendi",
        (AuditComparisonEvidenceState::Persisting, false) => "Persisting",
        (AuditComparisonEvidenceState::Persisting, true) => "Sürüyor",
        (AuditComparisonEvidenceState::Resolved, false) => "Resolved",
        (AuditComparisonEvidenceState::Resolved, true) => "Giderildi",
        (AuditComparisonEvidenceState::NotObserved, false) => "Not observed",
        (AuditComparisonEvidenceState::NotObserved, true) => "Gözlemlenmedi",
    }
}

fn localized_reason(reason: &str, language: &AuditReportLanguage) -> String {
    if *language == AuditReportLanguage::English {
        return reason.into();
    }
    // Reasons are deterministic engine copy. Replace each clause so joined compatibility
    // reasons remain readable without changing raw metadata, CSV or captured observations.
    let replacements = [
        (
            "Report schema or rule versions differ.",
            "Rapor şeması veya kural sürümleri farklı.",
        ),
        (
            "Saved scope, filters, or audit thresholds differ.",
            "Kaydedilmiş kapsam, filtreler veya denetim eşikleri farklı.",
        ),
        (
            "Recorded scope exclusions differ.",
            "Kaydedilmiş kapsam dışlamaları farklı.",
        ),
        (
            "Link edges lack exact source occurrence attribution; retained reference sets are not comparable.",
            "Bağlantı kenarlarında tam kaynak oluşumu atfı yok; kaydedilmiş başvuru kümeleri karşılaştırılamaz.",
        ),
        (
            "The rule was not measured with compatible coverage in both reports.",
            "Kural her iki raporda karşılaştırılabilir kapsamla ölçülmedi.",
        ),
        (
            "Not observed in the baseline; a current finding is not a verified regression.",
            "Önceki raporda gözlemlenmedi; güncel bulgu doğrulanmış bir kötüleşme değildir.",
        ),
        (
            "Not observed in the current report; absence does not verify a fix.",
            "Güncel raporda gözlemlenmedi; yokluğu düzeltmeyi doğrulamaz.",
        ),
        (
            "The matching occurrence is outside one report's saved scope.",
            "Eşleşen oluşum raporlardan birinin kaydedilmiş kapsamı dışında.",
        ),
        (
            "Legacy duplicate List occurrences cannot be matched reliably.",
            "Eski yinelenen Liste oluşumları güvenilir biçimde eşleştirilemiyor.",
        ),
        (
            "HTTP and rendered capture modes differ.",
            "HTTP ve işlenmiş sayfa yakalama modları farklı.",
        ),
        (
            "The same versioned rule remains detected for this request occurrence.",
            "Aynı sürümlü kural bu istek oluşumunda hâlâ tespit ediliyor.",
        ),
        (
            "The current occurrence is failed, blocked, incomplete, or ineligible; a fix is unverified.",
            "Güncel oluşum başarısız, engellenmiş, eksik veya uygun değil; düzeltme doğrulanmadı.",
        ),
        (
            "Global duplicate/reference context from the baseline was not completely observed in the current report.",
            "Önceki rapordaki genel yineleme veya başvuru bağlamı güncel raporda tam gözlemlenmedi.",
        ),
        (
            "Eligible current evidence verifies that the previous rule failure is absent.",
            "Uygun güncel kanıt, önceki kural ihlalinin artık bulunmadığını doğruluyor.",
        ),
        (
            "The baseline occurrence is failed, blocked, incomplete, or ineligible; a regression is unverified.",
            "Önceki oluşum başarısız, engellenmiş, eksik veya uygun değil; kötüleşme doğrulanmadı.",
        ),
        (
            "Global duplicate/reference context from the current report was not completely observed in the baseline.",
            "Güncel rapordaki genel yineleme veya başvuru bağlamı önceki raporda tam gözlemlenmedi.",
        ),
        (
            "Eligible baseline evidence was unaffected; the current occurrence now fails this rule.",
            "Uygun önceki kanıtta sorun yoktu; güncel oluşum artık bu kuralı ihlal ediyor.",
        ),
        (
            "Exact legacy link occurrence correspondence and capture completeness are unavailable.",
            "Eski bağlantı oluşumlarının tam eşleşmesi ve yakalama bütünlüğü mevcut değil.",
        ),
    ];
    replacements
        .into_iter()
        .fold(reason.to_owned(), |text, (english, turkish)| {
            text.replace(english, turkish)
        })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonFindingDisplay {
    title: String,
    status: &'static str,
    count_unit: String,
    compatibility_reasons: Vec<String>,
}
fn finding_display(
    finding: &AuditComparisonFinding,
    language: &AuditReportLanguage,
) -> ComparisonFindingDisplay {
    ComparisonFindingDisplay {
        title: localized_finding_title(&finding.finding_id, &finding.title, language),
        status: status_label(&finding.status, language),
        count_unit: if *language == AuditReportLanguage::Turkish {
            match finding.count_unit.as_str() {
                "retained reference occurrences" => "Kaydedilmiş başvuru oluşumları".into(),
                "request URL / List occurrences" => "İstek URL'si / Liste oluşumları".into(),
                _ => finding.count_unit.clone(),
            }
        } else {
            finding.count_unit.clone()
        },
        compatibility_reasons: finding
            .compatibility_reasons
            .iter()
            .map(|reason| localized_reason(reason, language))
            .collect(),
    }
}

#[derive(Serialize)]
struct ComparisonEvidenceDisplay {
    state: &'static str,
    reason: String,
}
fn evidence_display(
    row: &AuditComparisonEvidence,
    language: &AuditReportLanguage,
) -> ComparisonEvidenceDisplay {
    ComparisonEvidenceDisplay {
        state: state_label(&row.state, language),
        reason: localized_reason(&row.reason, language),
    }
}

/// Write into an existing empty private directory. The comparison database embeds the observations,
/// so this keeps working after either source report is deleted.
pub fn write_audit_report_comparison_package(
    comparison: &AuditReportComparisonStore,
    directory: &Path,
    progress: impl FnMut(AuditReportExportProgress) -> bool,
) -> Result<AuditComparisonManifest, String> {
    write_audit_report_comparison_package_with_annotations(
        comparison,
        directory,
        |_| Ok(None::<()>),
        None,
        None,
        progress,
    )
}

/// Include only previously validated optional commentary from one selected saved generation.
/// AI text is rendered separately and never replaces computed comparison states or evidence.
pub fn write_audit_report_comparison_package_with_annotations<A: Serialize>(
    comparison: &AuditReportComparisonStore,
    directory: &Path,
    mut annotation: impl FnMut(&str) -> Result<Option<A>, String>,
    overview: Option<serde_json::Value>,
    ai_generation_version: Option<&str>,
    mut progress: impl FnMut(AuditReportExportProgress) -> bool,
) -> Result<AuditComparisonManifest, String> {
    if std::fs::read_dir(directory)
        .map_err(error)?
        .next()
        .is_some()
    {
        return Err("audit comparison destination must be an empty private directory".into());
    }
    let summary = comparison.summary().map_err(error)?;
    let language = &summary.current.request.language;
    let labels = comparison_labels(language);
    let compatibility_reasons = summary
        .compatibility_reasons
        .iter()
        .map(|reason| localized_reason(reason, language))
        .collect::<Vec<_>>();
    let mut files = PackageFiles::new(directory);
    let mut environment = Environment::new();
    environment.add_global(
        "turkish",
        summary.current.request.language == AuditReportLanguage::Turkish,
    );
    environment
        .add_template(
            "comparison.html",
            include_str!("../templates/audit_report_comparison.html.j2"),
        )
        .map_err(error)?;
    let template = environment.get_template("comparison.html").map_err(error)?;
    files.write(
        "report.css",
        include_bytes!("../templates/audit_report.css"),
    )?;
    files.write("report.js", include_bytes!("../templates/audit_report.js"))?;
    let mut findings = Vec::new();
    loop {
        let page = comparison
            .query_findings(AuditComparisonFindingQuery {
                offset: findings.len(),
                limit: AUDIT_REPORT_PAGE_SIZE,
                ..Default::default()
            })
            .map_err(error)?;
        if page.rows.is_empty() {
            if findings.len() == page.total {
                break;
            }
            return Err("comparison finding count changed during export".into());
        }
        findings.extend(page.rows);
    }
    if findings.len() != summary.finding_count {
        return Err("comparison finding count changed during export".into());
    }
    let total = summary.evidence_rows;
    check(&mut progress, "preparingComparison", 0, total)?;
    let mut completed = 0;
    let mut ai_annotated_findings = 0;
    let mut manifest_findings = Vec::new();
    let mut cards = Vec::new();
    for (ordinal, finding) in findings.iter().enumerate() {
        let display = finding_display(finding, language);
        let prefix = format!("comparison-{:04}", ordinal + 1);
        let csv_file = format!("{prefix}.csv");
        let first = comparison
            .query_evidence(AuditComparisonEvidenceQuery {
                finding_id: finding.finding_id.clone(),
                limit: AUDIT_REPORT_PAGE_SIZE,
                preview: false,
                ..Default::default()
            })
            .map_err(error)?;
        let page_count = first.total.div_ceil(AUDIT_REPORT_HTML_PAGE_SIZE);
        let html_pages = (1..=page_count)
            .map(|number| format!("{prefix}-page-{number:04}.html"))
            .collect::<Vec<_>>();
        let mut csv = Writer::from_writer(files.create(&csv_file)?);
        csv.write_record([
            "id",
            "findingId",
            "identityKey",
            "requestUrl",
            "occurrence",
            "state",
            "reason",
            "baselineOriginalUrl",
            "currentOriginalUrl",
            "baselineObserved",
            "currentObserved",
            "previewTruncated",
            "evidenceJson",
        ])
        .map_err(error)?;
        let mut written = 0;
        let mut cursor = None;
        for (page_index, html_file) in html_pages.iter().enumerate() {
            let start = page_index * AUDIT_REPORT_HTML_PAGE_SIZE;
            let end = (start + AUDIT_REPORT_HTML_PAGE_SIZE).min(first.total);
            let mut html = BufWriter::new(files.create(html_file)?);
            template.render_captured_to(context! { section => "header", title => format!("{} → {}", summary.baseline.request.title, summary.current.request.title), labels => labels }, &mut html).map_err(error)?;
            template.render_captured_to(context! { section => "evidenceStart", finding => finding, display => display, labels => labels, first => start + 1, last => end, total => first.total, csvFile => csv_file, previous => page_index.checked_sub(1).map(|index| &html_pages[index]), next => html_pages.get(page_index + 1) }, &mut html).map_err(error)?;
            let mut page_rows = start;
            while page_rows < end {
                check(&mut progress, "comparisonEvidence", completed, total)?;
                let limit = (end - page_rows).min(AUDIT_REPORT_PAGE_SIZE);
                let page = comparison
                    .export_evidence_window(&finding.finding_id, cursor.as_ref(), limit)
                    .map_err(error)?;
                if page.rows.len() != limit {
                    return Err("comparison evidence totals changed during export".into());
                }
                cursor = page.next_cursor;
                for row in page.rows {
                    let row_display = evidence_display(&row, language);
                    let cells = comparison_cells(&row)?;
                    csv.write_record(cells.iter().map(|value| safe_csv(value)))
                        .map_err(error)?;
                    template.render_captured_to(context! { section => "evidenceRow", row => row, display => row_display, labels => labels, baseline => observed_json(row.baseline.as_ref())?, current => observed_json(row.current.as_ref())?, payload => serde_json::to_string_pretty(&row).map_err(error)? }, &mut html).map_err(error)?;
                    completed += 1;
                    written += 1;
                }
                page_rows += limit;
            }
            template.render_captured_to(context! { section => "evidenceEnd", labels => labels, previous => page_index.checked_sub(1).map(|index| &html_pages[index]), next => html_pages.get(page_index + 1) }, &mut html).map_err(error)?;
            html.flush().map_err(error)?;
            html.get_ref().sync_all().map_err(error)?;
        }
        csv.flush().map_err(error)?;
        csv.get_ref().sync_all().map_err(error)?;
        if written != first.total {
            return Err("comparison evidence count does not match the finding".into());
        }
        if !comparison
            .export_evidence_window(&finding.finding_id, cursor.as_ref(), 1)
            .map_err(error)?
            .rows
            .is_empty()
        {
            return Err("comparison evidence exceeds the frozen finding total".into());
        }
        let annotation = annotation(&finding.finding_id)?;
        ai_annotated_findings += usize::from(annotation.is_some());
        cards.push(
            context! { finding => finding, display => display, number => ordinal + 1, htmlPage => html_pages.first(), csvFile => csv_file, annotation => annotation },
        );
        manifest_findings.push(AuditComparisonManifestFinding {
            finding_id: finding.finding_id.clone(),
            status: finding.status.clone(),
            added: finding.added,
            persisting: finding.persisting,
            resolved: finding.resolved,
            not_observed: finding.not_observed,
            newly_observed_current: finding.newly_observed_current,
            unverified_current: finding.unverified_current,
            count_unit: finding.count_unit.clone(),
            compatibility_reasons: finding.compatibility_reasons.clone(),
            html_pages,
            csv_file,
        });
    }
    if completed != total {
        return Err("exported comparison totals do not reconcile".into());
    }
    check(&mut progress, "comparisonIndex", completed, total)?;
    let mut index = BufWriter::new(files.create("index.html")?);
    template.render_captured_to(context! { section => "header", title => format!("{} → {}", summary.baseline.request.title, summary.current.request.title), labels => labels }, &mut index).map_err(error)?;
    template.render_captured_to(context! { section => "index", summary => summary, cards => cards, labels => labels, compatibilityReasons => compatibility_reasons, aiAnnotatedFindings => ai_annotated_findings, overview => overview, aiGenerationVersion => ai_generation_version, metadata => serde_json::to_string_pretty(&summary).map_err(error)? }, &mut index).map_err(error)?;
    index.flush().map_err(error)?;
    index.get_ref().sync_all().map_err(error)?;
    let mut names = files.names();
    names.push("manifest.json".into());
    let manifest = AuditComparisonManifest {
        schema_version: summary.schema_version,
        status: "complete".into(),
        comparison_status: summary.status.clone(),
        baseline_report_id: summary.baseline_report_id.clone(),
        current_report_id: summary.current_report_id.clone(),
        finding_count: findings.len(),
        evidence_rows: completed,
        ai_annotated_findings,
        ai_overview: overview,
        ai_generation_version: ai_generation_version.map(str::to_owned),
        compatibility_reasons: summary.compatibility_reasons.clone(),
        findings: manifest_findings,
        files: names,
        summary,
    };
    files.write(
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest).map_err(error)?,
    )?;
    files.complete = true;
    Ok(manifest)
}

fn observed_json(observation: Option<&AuditComparisonObservation>) -> Result<String, String> {
    serde_json::to_string_pretty(&observation.map(|item| &item.observed)).map_err(error)
}
fn comparison_cells(row: &AuditComparisonEvidence) -> Result<Vec<String>, String> {
    Ok(vec![
        row.id.to_string(),
        row.finding_id.clone(),
        row.identity_key.clone(),
        row.request_url.clone(),
        row.occurrence.to_string(),
        serde_json::to_string(&row.state).map_err(error)?,
        row.reason.clone(),
        row.baseline
            .as_ref()
            .map(|value| value.original_url.clone())
            .unwrap_or_default(),
        row.current
            .as_ref()
            .map(|value| value.original_url.clone())
            .unwrap_or_default(),
        observed_json(row.baseline.as_ref())?,
        observed_json(row.current.as_ref())?,
        row.preview_truncated.to_string(),
        serde_json::to_string(row).map_err(error)?,
    ])
}
fn safe_csv(value: &str) -> String {
    if value.trim_start().starts_with(['=', '+', '-', '@']) || value.starts_with(['\t', '\r', '\n'])
    {
        format!("'{value}")
    } else {
        value.into()
    }
}
fn check(
    progress: &mut impl FnMut(AuditReportExportProgress) -> bool,
    phase: &str,
    completed: usize,
    total: usize,
) -> Result<(), String> {
    if progress(AuditReportExportProgress {
        phase: phase.into(),
        completed,
        total,
    }) {
        Ok(())
    } else {
        Err("audit comparison export cancelled".into())
    }
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
struct PackageFiles<'a> {
    directory: &'a Path,
    files: Vec<PathBuf>,
    complete: bool,
}
impl<'a> PackageFiles<'a> {
    fn new(directory: &'a Path) -> Self {
        Self {
            directory,
            files: vec![],
            complete: false,
        }
    }
    fn create(&mut self, name: &str) -> Result<File, String> {
        let path = self.directory.join(name);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(error)?;
        self.files.push(path);
        Ok(file)
    }
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let mut file = self.create(name)?;
        file.write_all(bytes).map_err(error)?;
        file.sync_all().map_err(error)
    }
    fn names(&self) -> Vec<String> {
        self.files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }
}
impl Drop for PackageFiles<'_> {
    fn drop(&mut self) {
        if !self.complete {
            for path in &self.files {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
