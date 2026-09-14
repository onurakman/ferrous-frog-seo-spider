//! Portable, offline audit packages from frozen native evidence.
use csv::Writer;
use ferrous_frog_storage::*;
use minijinja::{Environment, context};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

pub const AUDIT_REPORT_HTML_PAGE_SIZE: usize = 1_000;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReportExportProgress {
    pub phase: String,
    pub completed: usize,
    pub total: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReportManifestFinding {
    pub finding_id: String,
    pub counts: AuditFindingCounts,
    pub html_pages: Vec<String>,
    pub csv_file: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReportManifest {
    pub schema_version: u32,
    pub report_id: String,
    pub language: AuditReportLanguage,
    /// Package completion; measurement coverage is described separately below.
    pub status: String,
    pub finding_count: usize,
    pub evidence_rows: usize,
    pub ai_annotated_findings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_overview: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_generation_version: Option<String>,
    pub findings: Vec<AuditReportManifestFinding>,
    pub coverage: Vec<AuditRuleCoverage>,
    pub files: Vec<String>,
    pub summary: AuditReportSummary,
}

/// Write into an existing empty private directory. The caller atomically publishes the directory.
/// Every full evidence row is streamed to CSV and HTML; no network or website requests occur.
/// Returning false from progress cancels and removes files created by this invocation.
pub fn write_audit_report_package(
    report: &AuditReportStore,
    directory: &Path,
    progress: impl FnMut(AuditReportExportProgress) -> bool,
) -> Result<AuditReportManifest, String> {
    write_audit_report_package_with_annotations(
        report,
        directory,
        |_| Ok(None::<serde_json::Value>),
        None,
        None,
        progress,
    )
}

/// The native caller supplies only previously validated annotations from one frozen generation.
/// Annotation text remains separate from the measured finding, count and evidence fields.
pub fn write_audit_report_package_with_annotations<A: Serialize>(
    report: &AuditReportStore,
    directory: &Path,
    mut annotation: impl FnMut(&str) -> Result<Option<A>, String>,
    overview: Option<serde_json::Value>,
    ai_generation_version: Option<&str>,
    mut progress: impl FnMut(AuditReportExportProgress) -> bool,
) -> Result<AuditReportManifest, String> {
    if std::fs::read_dir(directory)
        .map_err(error)?
        .next()
        .is_some()
    {
        return Err("audit report destination must be an empty private directory".into());
    }
    let mut output = PackageFiles {
        directory,
        files: Vec::new(),
        complete: false,
    };
    let summary = report.summary().map_err(error)?;
    let language = &summary.request.language;
    let labels = labels(language);
    let mut environment = Environment::new();
    environment
        .add_template(
            "audit_report.html",
            include_str!("../templates/audit_report.html.j2"),
        )
        .map_err(error)?;
    let template = environment
        .get_template("audit_report.html")
        .map_err(error)?;
    let mut findings = Vec::new();
    while findings.len() < summary.finding_count {
        let page = report
            .query_findings(AuditFindingQuery {
                offset: findings.len(),
                ..Default::default()
            })
            .map_err(error)?;
        if page.total != summary.finding_count || page.rows.is_empty() {
            return Err("report finding count changed during export".into());
        }
        findings.extend(page.rows);
    }
    findings.sort_by_key(|finding| {
        (
            match finding.severity {
                Severity::Error => 0,
                Severity::Warning => 1,
                Severity::Info => 2,
            },
            finding.id.clone(),
        )
    });
    let total: usize = findings
        .iter()
        .map(|finding| finding.counts.occurrences)
        .sum();
    check_progress(&mut progress, "preparing", 0, total)?;
    output.write(
        "report.css",
        include_bytes!("../templates/audit_report.css"),
    )?;
    output.write("report.js", include_bytes!("../templates/audit_report.js"))?;
    let mut completed = 0;
    let mut manifest_findings = Vec::new();
    let mut cards = Vec::new();
    let mut ai_annotated_findings = 0;
    for (ordinal, finding) in findings.iter().enumerate() {
        check_progress(&mut progress, "finding", completed, total)?;
        let prefix = format!("finding-{:04}", ordinal + 1);
        let csv_file = format!("{prefix}.csv");
        let page_count = finding
            .counts
            .occurrences
            .div_ceil(AUDIT_REPORT_HTML_PAGE_SIZE);
        let html_pages = (1..=page_count)
            .map(|page| format!("{prefix}-page-{page:04}.html"))
            .collect::<Vec<_>>();
        let mut csv = Writer::from_writer(output.create(&csv_file)?);
        csv.write_record(CSV_HEADERS).map_err(error)?;
        let mut finding_rows = 0;
        let mut cursor = None;
        for (page_index, html_file) in html_pages.iter().enumerate() {
            let start = page_index * AUDIT_REPORT_HTML_PAGE_SIZE;
            let end = (start + AUDIT_REPORT_HTML_PAGE_SIZE).min(finding.counts.occurrences);
            let presentation = presentation(finding, language);
            let mut html = BufWriter::new(output.create(html_file)?);
            template
                .render_captured_to(
                    context! { section => "header", labels => labels, title => presentation.title },
                    &mut html,
                )
                .map_err(error)?;
            template.render_captured_to(context! { section => "evidenceStart", labels => labels, finding => finding, presentation => presentation, first => start+1, last => end, previous => page_index.checked_sub(1).map(|index| &html_pages[index]), next => html_pages.get(page_index+1), csvFile => csv_file }, &mut html).map_err(error)?;
            // Read 100 full rows at a time; each HTML page contains at most 1,000 rows.
            let mut page_rows = start;
            while page_rows < end {
                check_progress(&mut progress, "evidence", completed, total)?;
                let limit = (end - page_rows).min(AUDIT_REPORT_PAGE_SIZE);
                let page = report
                    .export_evidence_window(&finding.id, cursor, limit)
                    .map_err(error)?;
                if page.rows.len() != limit {
                    return Err("report evidence totals changed during export".into());
                }
                cursor = page.next_sequence;
                for row in &page.rows {
                    let cells = evidence_cells(row)?;
                    let cells = cells
                        .iter()
                        .map(|value| safe_csv_cell(value))
                        .collect::<Vec<_>>();
                    csv.write_record(cells.iter().map(|value| value.as_bytes()))
                        .map_err(error)?;
                    let payload = serde_json::to_string_pretty(row).map_err(error)?;
                    template.render_captured_to(context! { section => "evidenceRow", labels => labels, row => row, payload => payload }, &mut html).map_err(error)?;
                    finding_rows += 1;
                    completed += 1;
                }
                page_rows += page.rows.len();
            }
            template.render_captured_to(context! { section => "evidenceEnd", labels => labels, previous => page_index.checked_sub(1).map(|index| &html_pages[index]), next => html_pages.get(page_index+1) }, &mut html).map_err(error)?;
            html.flush().map_err(error)?;
            html.get_ref().sync_all().map_err(error)?;
        }
        csv.flush().map_err(error)?;
        csv.get_ref().sync_all().map_err(error)?;
        if finding_rows != finding.counts.occurrences {
            return Err("exported evidence count does not match the finding".into());
        }
        if !report
            .export_evidence_window(&finding.id, cursor, 1)
            .map_err(error)?
            .rows
            .is_empty()
        {
            return Err("report evidence exceeds the frozen finding total".into());
        }
        let annotation = annotation(&finding.id)?;
        ai_annotated_findings += usize::from(annotation.is_some());
        cards.push(context! { finding => finding, presentation => presentation(finding, language), number => ordinal+1, firstPage => html_pages.first(), csvFile => csv_file, annotation => annotation });
        manifest_findings.push(AuditReportManifestFinding {
            finding_id: finding.id.clone(),
            counts: finding.counts.clone(),
            html_pages,
            csv_file,
        });
    }
    check_progress(&mut progress, "finalizing", completed, total)?;
    if completed != total {
        return Err("exported report totals do not reconcile".into());
    }
    let coverage = summary.coverage.iter().map(|item| context! { ruleId => item.rule_id, state => item.state, eligibleRecords => item.eligible_records, reason => coverage_reason(item,language) }).collect::<Vec<_>>();
    let mut index = BufWriter::new(output.create("index.html")?);
    template
        .render_captured_to(
            context! { section => "header", labels => labels, title => summary.request.title },
            &mut index,
        )
        .map_err(error)?;
    template.render_captured_to(context! { section => "index", labels => labels, summary => summary, cards => cards, coverage => coverage, aiAnnotatedFindings => ai_annotated_findings, overview => overview, aiGenerationVersion => ai_generation_version,
        errors => findings.iter().filter(|finding| finding.severity == Severity::Error).count(),
        warnings => findings.iter().filter(|finding| finding.severity == Severity::Warning).count(),
        infos => findings.iter().filter(|finding| finding.severity == Severity::Info).count(),
        metadata => serde_json::to_string_pretty(&summary).map_err(error)? }, &mut index).map_err(error)?;
    index.flush().map_err(error)?;
    index.get_ref().sync_all().map_err(error)?;
    drop(index);
    check_progress(&mut progress, "manifest", completed, total)?;
    let mut files = output
        .files
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    files.push("manifest.json".into());
    let manifest = AuditReportManifest {
        schema_version: 1,
        report_id: summary.request.id.clone(),
        language: summary.request.language.clone(),
        status: "complete".into(),
        finding_count: findings.len(),
        evidence_rows: completed,
        ai_annotated_findings,
        ai_overview: overview,
        ai_generation_version: ai_generation_version.map(str::to_owned),
        findings: manifest_findings,
        coverage: summary.coverage.clone(),
        files,
        summary,
    };
    output.write(
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest).map_err(error)?,
    )?;
    output.complete = true;
    Ok(manifest)
}

struct PackageFiles<'a> {
    directory: &'a Path,
    files: Vec<PathBuf>,
    complete: bool,
}

/// Export all matching evidence, irrespective of the current UI page or preview limit.
/// Unfiltered ID order uses a sequence cursor; filtered or alternate-sort exports retain
/// offset paging and may be slower at large offsets.
pub fn write_audit_report_evidence_csv(
    report: &AuditReportStore,
    mut query: AuditEvidenceQuery,
    output: impl Write,
    mut progress: impl FnMut(AuditReportExportProgress) -> bool,
) -> Result<usize, String> {
    query.offset = 0;
    query.limit = AUDIT_REPORT_PAGE_SIZE;
    query.preview = false;
    let mut page = report.query_evidence(query.clone()).map_err(error)?;
    let total = page.total;
    check_progress(&mut progress, "preparingEvidenceCsv", 0, total)?;
    let mut writer = Writer::from_writer(output);
    writer.write_record(CSV_HEADERS).map_err(error)?;
    let unfiltered_id_order = query.search.as_deref().is_none_or(str::is_empty)
        && query.status_code.is_none()
        && matches!(query.sort_by, AuditEvidenceSort::Id)
        && query.sort_dir == SortDirection::Asc;
    if unfiltered_id_order {
        let mut cursor = None;
        let mut completed = 0;
        while completed < total {
            check_progress(&mut progress, "exportingEvidenceCsv", completed, total)?;
            let limit = (total - completed).min(AUDIT_REPORT_PAGE_SIZE);
            let window = report
                .export_evidence_window(&query.finding_id, cursor, limit)
                .map_err(error)?;
            if window.rows.len() != limit {
                return Err("Frozen evidence totals changed during CSV export".into());
            }
            write_evidence_csv_rows(&mut writer, &window.rows)?;
            completed += window.rows.len();
            cursor = window.next_sequence;
        }
        if !report
            .export_evidence_window(&query.finding_id, cursor, 1)
            .map_err(error)?
            .rows
            .is_empty()
        {
            return Err("Frozen evidence exceeds the finding total".into());
        }
    } else {
        // Filtered and arbitrary-sort exports keep the public query semantics. These may be
        // slower at very large offsets because no ordered keyset exists for every sort mode.
        while query.offset < total {
            check_progress(&mut progress, "exportingEvidenceCsv", query.offset, total)?;
            if page.total != total || page.rows.len() != query.limit.min(total - query.offset) {
                return Err("Filtered evidence totals changed during export".into());
            }
            write_evidence_csv_rows(&mut writer, &page.rows)?;
            query.offset += page.rows.len();
            if query.offset < total {
                page = report.query_evidence(query.clone()).map_err(error)?;
            }
        }
    }
    writer.flush().map_err(error)?;
    check_progress(&mut progress, "publishingEvidenceCsv", total, total)?;
    Ok(total)
}
fn write_evidence_csv_rows<W: Write>(
    writer: &mut Writer<W>,
    rows: &[AuditEvidence],
) -> Result<(), String> {
    for row in rows {
        let values = evidence_cells(row)?;
        let cells = values
            .iter()
            .map(|value| safe_csv_cell(value))
            .collect::<Vec<_>>();
        writer
            .write_record(cells.iter().map(|value| value.as_bytes()))
            .map_err(error)?;
    }
    Ok(())
}
impl PackageFiles<'_> {
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
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn check_progress(
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
        Err("audit report export cancelled".into())
    }
}
const CSV_HEADERS: [&str; 31] = [
    "id",
    "findingId",
    "sourceStorageKey",
    "sourceRecordId",
    "listPosition",
    "listDuplicateIndex",
    "originalUrl",
    "finalUrl",
    "kind",
    "attribution",
    "sourceUrl",
    "targetUrl",
    "sourcePosition",
    "sourceEdgeId",
    "statusCode",
    "contentType",
    "indexabilityStatus",
    "title",
    "titleLength",
    "metaDescription",
    "metaDescriptionLength",
    "h1",
    "canonical",
    "anchorText",
    "rel",
    "rendered",
    "error",
    "previewTruncated",
    "evidenceJson",
    "countUnit",
    "captureAttribution",
];
fn evidence_cells(row: &AuditEvidence) -> Result<Vec<String>, String> {
    let object = serde_json::to_value(row).map_err(error)?;
    let observed = &object["observed"];
    let payload = serde_json::to_string(row).map_err(error)?;
    Ok(CSV_HEADERS
        .iter()
        .map(|key| match *key {
            "evidenceJson" => payload.clone(),
            "countUnit" => "evidence occurrence".into(),
            "captureAttribution" => if row.attribution == AuditAttribution::ExactRecord {
                "exact source record"
            } else {
                "source URL only; exact List occurrence unavailable"
            }
            .into(),
            _ => match object.get(*key).or_else(|| observed.get(*key)) {
                Some(serde_json::Value::String(value)) => value.clone(),
                Some(serde_json::Value::Null) | None => String::new(),
                Some(value) => value.to_string(),
            },
        })
        .collect())
}
fn safe_csv_cell(value: &str) -> std::borrow::Cow<'_, str> {
    if value.trim_start().starts_with(['=', '+', '-', '@']) || value.starts_with(['\t', '\r', '\n'])
    {
        format!("'{value}").into()
    } else {
        value.into()
    }
}
fn text<'a>(language: &AuditReportLanguage, english: &'a str, turkish: &'a str) -> &'a str {
    if *language == AuditReportLanguage::Turkish {
        turkish
    } else {
        english
    }
}
fn labels(language: &AuditReportLanguage) -> serde_json::Value {
    let t = |english, turkish| text(language, english, turkish);
    serde_json::Value::Object([
        ("aiOverview",t("AI executive summary","AI yönetici özeti")),("aiPriorities",t("Suggested priorities","Önerilen öncelikler")),("aiLimitations",t("Limitations","Sınırlamalar")),("aiCoverage",t("Included findings","Dahil edilen bulgular")),
        ("aiDisclosure",t("AI interpretation is optional and unverified. Measurements and full evidence remain authoritative","AI yorumu isteğe bağlıdır ve doğrulanmamıştır. Ölçümler ve eksiksiz kanıtlar esas alınır")),
        ("aiExplanation",t("AI explanation","AI açıklaması")),("proposedCause",t("Proposed cause — unverified","Olası neden — doğrulanmadı")),
        ("aiSamples",t("Sampled evidence","Örneklenen kanıt")),("aiModel",t("Model","Model")),("aiGeneration",t("AI generation","AI üretim sürümü")),("aiReferences",t("Cited evidence IDs","Atıf yapılan kanıt kimlikleri")),
        ("language",t("en","tr")),("eyebrow",t("TECHNICAL SEO AUDIT","TEKNİK SEO DENETİMİ")),("subtitle",t("A clear view of your site's captured evidence, priorities, and next steps.","Sitenizin kaydedilmiş kanıtlarına, önceliklerine ve sonraki adımlarına açık bir bakış.")),
        ("captured",t("Captured records","Kaydedilen kayıtlar")),("inScope",t("Records in scope","Kapsamdaki kayıtlar")),("eligible",t("Eligible HTML records","Uygun HTML kayıtları")),("findings",t("Findings","Bulgular")),("errors",t("Errors","Hatalar")),("warnings",t("Warnings","Uyarılar")),("infos",t("Information","Bilgiler")),("scope",t("Scope & measurement coverage","Kapsam ve ölçüm durumu")),("source",t("Source crawl","Kaynak tarama")),("created",t("Report created","Rapor oluşturulma zamanı")),("partial",t("Partial crawl — unobserved pages have not been verified.","Kısmi tarama — gözlemlenmeyen sayfalar doğrulanmadı.")),
        ("measured",t("Measured","Ölçüldü")),("incomplete",t("Incomplete evidence","Eksik kanıt")),("notMeasured",t("Not measured","Ölçülmedi")),("limits",t("Coverage is limited to captured evidence and the rules listed below. Unmeasured checks are not successful checks.","Kapsam, kaydedilen kanıtlarla ve aşağıdaki kurallarla sınırlıdır. Ölçülmeyen kontroller başarılı sayılmaz.")),
        ("summary",t("Audit overview","Denetime genel bakış")),("overview",t("Prioritize the findings below using their measured severity. Counts are computed from the saved crawl; evidence pages and CSV files contain every matching stored occurrence.","Aşağıdaki bulguları ölçülen önem düzeyine göre önceliklendirin. Sayılar kayıtlı taramadan hesaplanır; kanıt sayfaları ve CSV dosyaları eşleşen tüm kayıtlı oluşumları içerir.")),
        ("filter",t("Filter findings","Bulguları filtrele")),("allSeverities",t("All severities","Tüm önem düzeyleri")),("allTeams",t("All teams","Tüm ekipler")),("content",t("Content","İçerik")),("engineering",t("Engineering","Mühendislik")),("visible",t("Visible findings","Görünen bulgular")),
        ("problem",t("Observed problem","Gözlemlenen sorun")),("recommendation",t("Recommended action","Önerilen işlem")),("verification",t("Verify the change","Değişikliği doğrulayın")),("allEvidence",t("View all evidence","Tüm kanıtları görüntüle")),("csv",t("Download complete CSV","Tam CSV dosyasını indir")),("pages",t("source pages","kaynak sayfa")),("records",t("source records","kaynak kayıt")),("uniqueUrls",t("unique URLs","benzersiz URL")),("targets",t("targets","hedef")),("occurrences",t("evidence occurrences","kanıt oluşumu")),("unknown",t("Unavailable","Mevcut değil")),
        ("noFindings",t("No findings were detected in the eligible captured population. Review coverage before drawing broader conclusions.","Uygun kaydedilmiş veri kümesinde bulgu tespit edilmedi. Daha geniş sonuçlara varmadan önce kapsamı inceleyin.")),("metadata",t("Frozen report configuration","Dondurulmuş rapor yapılandırması")),("back",t("Report overview","Rapor özeti")),("previous",t("Previous page","Önceki sayfa")),("next",t("Next page","Sonraki sayfa")),("of",t("of","/")),("pageSearch",t("Search this page only","Yalnızca bu sayfada ara")),("pageSearchHelp",t("This search covers the current HTML page. Use the complete CSV to search across every evidence row.","Bu arama yalnızca mevcut HTML sayfasını kapsar. Tüm kanıt satırlarında aramak için tam CSV dosyasını kullanın.")),("originalUrl",t("Original / source URL","Orijinal / kaynak URL")),("finalUrl",t("Final URL","Son URL")),("status",t("Status","Durum")),("target",t("Target URL","Hedef URL")),("fullEvidence",t("Full captured evidence","Kaydedilen kanıtın tamamı")),("legacy",t("Source URL attribution only; the exact List occurrence is unavailable.","Yalnızca kaynak URL biliniyor; tam Liste oluşumu mevcut değil.")),("footer",t("Prepared by Ferrous Frog · Offline report · No external resources","Ferrous Frog ile hazırlandı · Çevrimdışı rapor · Harici kaynak yok"))
    ].into_iter().map(|(key,value)| (key.into(), serde_json::Value::String(value.into()))).collect())
}
#[derive(Serialize)]
struct Presentation {
    title: String,
    explanation: String,
    recommendation: String,
    verification: String,
    team: String,
    category: String,
}
pub(crate) fn localized_finding_title(
    id: &str,
    fallback: &str,
    language: &AuditReportLanguage,
) -> String {
    if *language == AuditReportLanguage::English {
        return fallback.into();
    }
    match id {
        "title.missing" => "Sayfa başlığı eksik",
        "title.duplicate" => "Yinelenen sayfa başlığı",
        "title.tooShort" => "Kısa sayfa başlığı",
        "title.tooLong" => "Uzun sayfa başlığı",
        "meta.missing" => "Meta açıklaması eksik",
        "meta.duplicate" => "Yinelenen meta açıklaması",
        "meta.tooShort" => "Kısa meta açıklaması",
        "meta.tooLong" => "Uzun meta açıklaması",
        "h1.missing" => "H1 başlığı eksik",
        "canonical.missing" => "Kanonik URL eksik",
        "canonical.toError" => "Kanonik URL hatalı bir hedefe işaret ediyor",
        "canonical.toRedirect" => "Kanonik URL yönlendirmeye işaret ediyor",
        "canonical.loop" => "Kanonik URL döngüsü",
        "response.clientError" => "İstemci hatası yanıtı",
        "response.serverError" => "Sunucu hatası yanıtı",
        "response.noResponse" => "Yanıt alınamadı",
        "links.broken" => "Bozuk bağlantı hedefleri",
        _ => fallback,
    }
    .into()
}
fn presentation(finding: &AuditFinding, language: &AuditReportLanguage) -> Presentation {
    if *language == AuditReportLanguage::English {
        return Presentation {
            title: finding.title.clone(),
            explanation: finding.explanation.clone(),
            recommendation: finding.recommendation.clone(),
            verification: finding.verification.clone(),
            team: finding.suggested_team.clone(),
            category: finding.category.clone(),
        };
    }
    let title = localized_finding_title(&finding.id, &finding.title, language);
    let (category, recommendation) = match finding.category.as_str() {
        "Titles" => (
            "Başlıklar",
            "Etkilenen başlıkları inceleyin; kaydedilmiş eşiklere uygun, açıklayıcı ve benzersiz başlıklar oluşturun.",
        ),
        "Descriptions" => (
            "Açıklamalar",
            "Etkilenen açıklamaları inceleyin; kaydedilmiş eşiklere uygun, yararlı ve benzersiz açıklamalar oluşturun.",
        ),
        "Headings" => (
            "Sayfa başlıkları",
            "Etkilenen sayfaya açıklayıcı bir ana başlık ekleyin.",
        ),
        "Canonicals" => (
            "Kanonik URL'ler",
            "Kanonik bildirimi inceleyin ve amaçlanan, erişilebilir, dizine eklenebilir hedefe yönlendirin.",
        ),
        "Links" => (
            "Bağlantılar",
            "Hatalı URL'yi ve ona verilen bağlantıları inceleyin; hedefi düzeltin veya kullanılmayan bağlantıyı kaldırın.",
        ),
        _ => (
            "Yanıt kodları",
            "Hatalı URL'yi ve ona verilen bağlantıları inceleyin; hedefi düzeltin veya kullanılmayan bağlantıyı kaldırın.",
        ),
    };
    Presentation {title,explanation:"Bu durum dondurulmuş tarama kanıtlarında tespit edildi. Sayılar kayıtlı rapor kapsamındaki gözlemleri ifade eder.".into(),recommendation:recommendation.into(),verification:"Etkilenen URL'leri karşılaştırılabilir kapsam ve eşiklerle yeniden tarayın; ardından yeni kaydedilen kanıtları inceleyin.".into(),team:if finding.suggested_team=="Content" {"İçerik"} else {"Mühendislik"}.into(),category:category.into()}
}
fn coverage_reason<'a>(coverage: &'a AuditRuleCoverage, language: &AuditReportLanguage) -> &'a str {
    if *language == AuditReportLanguage::English {
        return &coverage.reason;
    }
    match coverage.rule_id.as_str() {
        "links.broken" => {
            "Eşleşen tüm kayıtlı bağlantılar mevcuttur. Eski bağlantı kayıtları tam Liste oluşumunu belirtmez; kaynak kayıt sayısı ve toplama bütünlüğü doğrulanamaz."
        }
        "otherIssueViews" => {
            "Yalnızca açıkça listelenen rapor kuralları değerlendirildi; diğer denetim görünümleri bu sürümde ölçülmedi."
        }
        "imageOccurrences" => "Görsel oluşumu kanıtları bu rapor sürümüne dahil değildir.",
        "browserInteractions" => {
            "Formlar, analiz olayları, erişilebilirlik etkileşimleri ve yasal uyumluluk test edilmedi."
        }
        "retainedBodies" => {
            "Ham HTML, oluşturulmuş HTML, başlıklar ve saklanan metinler rapora kopyalanmadı; yalnızca kaydedilmiş kayıt alanları ve bağlantı kanıtları mevcuttur."
        }
        _ => {
            "Dondurulmuş kayıtlar mevcut denetim kuralıyla değerlendirildi; genel bağlam kapsam dışındaki kayıtları da içerir."
        }
    }
}

#[cfg(test)]
mod cursor_csv_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn unfiltered_id_csv_walks_all_windows_with_full_first_middle_last_values() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ff-audit-csv-cursor-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let source = ActiveStore::memory();
        for index in 0..205 {
            let mut row = CrawlRecord::pending(format!("https://example.test/{index:03}"), 0);
            row.status_code = Some(200);
            row.content_type = Some("text/html".into());
            row.indexability_status = "Indexable".into();
            if index == 204 {
                row.h1 = Some("x".repeat(70_000));
            }
            source.upsert(row);
        }
        let report = AuditReportStore::prepare(
            &path,
            &source,
            AuditReportRequest {
                id: "csv-cursor-test".into(),
                title: "CSV cursor".into(),
                language: AuditReportLanguage::English,
                source_session_id: "source".into(),
                source_revision: "revision".into(),
                source_status: AuditSourceStatus::Completed,
                created_at: "2026-09-14T00:00:00Z".into(),
                scope: GridQuery::default(),
                exclusions: vec![],
                crawl_limits: vec![],
            },
        )
        .unwrap();
        let mut output = Vec::new();
        let rows = write_audit_report_evidence_csv(
            &report,
            AuditEvidenceQuery {
                finding_id: "title.missing".into(),
                offset: 1_000,
                limit: 1,
                preview: true,
                ..Default::default()
            },
            &mut output,
            |_| true,
        )
        .unwrap();
        assert_eq!(rows, 205);
        let mut reader = csv::Reader::from_reader(output.as_slice());
        let headers = reader.headers().unwrap().clone();
        let url = headers
            .iter()
            .position(|value| value == "originalUrl")
            .unwrap();
        let h1 = headers.iter().position(|value| value == "h1").unwrap();
        let records = reader.records().map(Result::unwrap).collect::<Vec<_>>();
        assert_eq!(records.len(), 205);
        for index in [0, 102, 204] {
            assert_eq!(
                &records[index][url],
                format!("https://example.test/{index:03}")
            );
        }
        assert_eq!(records[204][h1].len(), 70_000);
        drop(report);
        std::fs::remove_file(path).unwrap();
    }
}
