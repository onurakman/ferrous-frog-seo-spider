use super::*;
use ferrous_frog_storage::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    root: PathBuf,
    report: AuditReportStore,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn fixture(count: usize, language: AuditReportLanguage) -> Fixture {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "ff-portable-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let source = ActiveStore::memory();
    for i in 0..count {
        let mut row = CrawlRecord::pending(format!("https://example.test/{i:04}"), 0);
        row.status_code = Some(200);
        row.content_type = Some("text/html".into());
        row.indexability_status = "Indexable".into();
        row.meta_description = Some(format!("Useful unique description of page {i}"));
        row.meta_description_len = 40;
        row.h1 = Some("Primary heading".into());
        row.canonical = Some(row.url.clone());
        if i + 1 == count {
            row.h1 = Some(format!(
                " =HYPERLINK(\"javascript:evil()\") <script>window.INJECTED=true</script> İstanbul 🐸\nlast evidence {}",
                "x".repeat(70_000)
            ));
        }
        source.upsert(row);
    }
    let report = AuditReportStore::prepare(
        root.join("report.sqlite"),
        &source,
        AuditReportRequest {
            id: "synthetic".into(),
            title: "Website audit <img src=x onerror=alert(1)>".into(),
            language,
            source_session_id: "saved".into(),
            source_revision: "rev1".into(),
            source_status: AuditSourceStatus::Completed,
            created_at: "2026-09-14T12:00:00Z".into(),
            scope: GridQuery::default(),
            exclusions: vec![],
            crawl_limits: vec![],
        },
    )
    .unwrap();
    Fixture { root, report }
}

#[test]
fn portable_report_exports_every_1205_row_in_two_linked_pages_and_csv() {
    let fixture = fixture(1205, AuditReportLanguage::English);
    let directory = fixture.root.join("package");
    std::fs::create_dir(&directory).unwrap();
    let manifest = write_audit_report_package(&fixture.report, &directory, |_| true).unwrap();
    let finding = manifest
        .findings
        .iter()
        .find(|f| f.finding_id == "title.missing")
        .unwrap();
    assert_eq!(finding.counts.occurrences, 1205);
    assert_eq!(finding.html_pages.len(), 2);
    let mut reader = csv::Reader::from_path(directory.join(&finding.csv_file)).unwrap();
    let headers = reader.headers().unwrap().clone();
    let records = reader.records().map(Result::unwrap).collect::<Vec<_>>();
    assert_eq!(records.len(), 1205);
    let original = headers.iter().position(|h| h == "originalUrl").unwrap();
    assert_eq!(&records[1204][original], "https://example.test/1204");
    let h1 = headers.iter().position(|h| h == "h1").unwrap();
    assert!(records[1204][h1].starts_with("' =HYPERLINK"));
    assert!(records[1204][h1].len() > 70_000);
    let first = std::fs::read_to_string(directory.join(&finding.html_pages[0])).unwrap();
    let last = std::fs::read_to_string(directory.join(&finding.html_pages[1])).unwrap();
    assert!(first.contains(&finding.html_pages[1]));
    assert!(last.contains(&finding.html_pages[0]));
    assert_eq!(first.matches("class=\"evidence-row\"").count(), 1000);
    assert_eq!(last.matches("class=\"evidence-row\"").count(), 205);
    assert!(
        last.replace("&#x2f;", "/")
            .contains("https://example.test/1204")
    );
    assert!(last.contains("İstanbul 🐸"));
    assert!(!last.contains("<script>window.INJECTED"));
    assert!(last.contains("&lt;script&gt;"));
    let index = std::fs::read_to_string(directory.join("index.html")).unwrap();
    assert!(index.contains("Measured priorities"));
    assert!(index.contains(&finding.html_pages[0]));
    assert!(!index.contains("<img src=x"));
    let saved: AuditReportManifest =
        serde_json::from_reader(std::fs::File::open(directory.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        saved.evidence_rows,
        manifest
            .findings
            .iter()
            .map(|f| f.counts.occurrences)
            .sum::<usize>()
    );
    assert_eq!(saved.finding_count, manifest.findings.len());
    for file in &manifest.files {
        assert!(directory.join(file).is_file(), "missing {file}");
    }
    if let Some(destination) = std::env::var_os("FF_AUDIT_OFFLINE_FIXTURE_DIR") {
        let destination = PathBuf::from(destination);
        std::fs::create_dir_all(&destination).unwrap();
        for file in &manifest.files {
            std::fs::copy(directory.join(file), destination.join(file)).unwrap();
        }
    }
}

#[test]
fn portable_report_handles_empty_turkish_coverage_without_implying_success() {
    let fixture = fixture(0, AuditReportLanguage::Turkish);
    let directory = fixture.root.join("package");
    std::fs::create_dir(&directory).unwrap();
    let manifest = write_audit_report_package(&fixture.report, &directory, |_| true).unwrap();
    assert_eq!(manifest.finding_count, 0);
    assert_eq!(manifest.evidence_rows, 0);
    assert!(
        manifest
            .coverage
            .iter()
            .any(|c| c.state == AuditCoverageState::NotMeasured)
    );
    let index = std::fs::read_to_string(directory.join("index.html")).unwrap();
    assert!(index.contains("lang=\"tr\""));
    assert!(index.contains("Ölçülmedi"));
    assert!(index.contains("Kapsam"));
}

#[test]
fn optional_annotations_are_escaped_and_do_not_replace_measured_evidence() {
    let fixture = fixture(3, AuditReportLanguage::English);
    let directory = fixture.root.join("annotated-package");
    std::fs::create_dir(&directory).unwrap();
    let manifest = write_audit_report_package_with_annotations(&fixture.report, &directory, |finding| {
        Ok((finding == "title.missing").then(|| serde_json::json!({
            "explanation": "AI explanation <script>window.AI_EXECUTED=true</script>",
            "proposedCause": "A template may omit the title.", "recommendation": "Inspect the page template.",
            "verification": "Recrawl and verify captured titles.", "sampleCount": 1, "evidenceTotal": 3,
            "model": "test-model", "evidenceIds": ["ev-1"]
        })))
    }, Some(serde_json::json!({"summary":"Overview <img src=x onerror=alert(1)>","prioritizedFindingIds":["title.missing"],"limitations":"Captured evidence only.","includedFindingCount":1,"totalFindingCount":2,"sampledEvidenceCount":1,"partialCoverage":true})), Some("old-generation<script>"), |_| true).unwrap();
    assert_eq!(manifest.ai_annotated_findings, 1);
    assert!(manifest.ai_overview.is_some());
    assert_eq!(
        manifest.ai_generation_version.as_deref(),
        Some("old-generation<script>")
    );
    assert_eq!(
        manifest
            .findings
            .iter()
            .find(|finding| finding.finding_id == "title.missing")
            .unwrap()
            .counts
            .occurrences,
        3
    );
    let html = std::fs::read_to_string(directory.join("index.html")).unwrap();
    assert!(html.contains("AI interpretation is optional and unverified"));
    assert!(html.contains("Sampled evidence: 1 / 3"));
    assert!(html.contains("test-model"));
    assert!(!html.contains("<script>window.AI_EXECUTED"));
    assert!(html.contains("&lt;script&gt;window.AI_EXECUTED"));
    assert!(html.contains("Overview &lt;img"));
    assert!(!html.contains("<img src=x"));
    assert!(html.contains("href=\"#finding-1\""));
    assert!(html.contains("AI generation: old-generation&lt;script&gt;"));
    assert!(!html.contains("AI generation: old-generation<script>"));
}

#[test]
fn matching_evidence_csv_ignores_ui_page_and_preserves_full_filtered_values() {
    let fixture = fixture(1205, AuditReportLanguage::English);
    let mut csv = Vec::new();
    let rows = write_audit_report_evidence_csv(
        &fixture.report,
        AuditEvidenceQuery {
            finding_id: "title.missing".into(),
            search: Some("https://example.test/12".into()),
            status_code: None,
            offset: 1000,
            limit: 1,
            sort_by: AuditEvidenceSort::OriginalUrl,
            sort_dir: SortDirection::Desc,
            preview: true,
        },
        &mut csv,
        |_| true,
    )
    .unwrap();
    assert_eq!(rows, 5);
    let mut reader = csv::Reader::from_reader(csv.as_slice());
    let headers = reader.headers().unwrap().clone();
    let values = reader.records().map(Result::unwrap).collect::<Vec<_>>();
    assert_eq!(values.len(), 5);
    let url = headers
        .iter()
        .position(|column| column == "originalUrl")
        .unwrap();
    let h1 = headers.iter().position(|column| column == "h1").unwrap();
    assert_eq!(&values[0][url], "https://example.test/1204");
    assert!(values[0][h1].len() > 70_000);
    let mut filtered_csv = Vec::new();
    assert_eq!(
        write_audit_report_evidence_csv(
            &fixture.report,
            AuditEvidenceQuery {
                finding_id: "title.missing".into(),
                status_code: Some(404),
                ..Default::default()
            },
            &mut filtered_csv,
            |_| true,
        )
        .unwrap(),
        0
    );
    assert_eq!(
        csv::Reader::from_reader(filtered_csv.as_slice())
            .records()
            .count(),
        0
    );
}

#[test]
fn portable_report_cancellation_and_destination_errors_never_leave_a_complete_package() {
    let fixture = fixture(1205, AuditReportLanguage::English);
    let directory = fixture.root.join("package");
    std::fs::create_dir(&directory).unwrap();
    let result = write_audit_report_package(&fixture.report, &directory, |progress| {
        progress.phase != "evidence" || progress.completed < 1000
    });
    assert!(result.is_err());
    assert!(!directory.join("manifest.json").exists());
    assert!(!directory.join("index.html").exists());
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    std::fs::write(directory.join("keep.txt"), "earlier export").unwrap();
    assert!(write_audit_report_package(&fixture.report, &directory, |_| true).is_err());
    assert_eq!(
        std::fs::read_to_string(directory.join("keep.txt")).unwrap(),
        "earlier export"
    );
    assert!(
        write_audit_report_package(&fixture.report, &directory.join("missing"), |_| true).is_err()
    );
}

#[test]
fn portable_report_midwrite_failure_removes_only_its_own_partial_files() {
    let fixture = fixture(1, AuditReportLanguage::English);
    let directory = fixture.root.join("package");
    std::fs::create_dir(&directory).unwrap();
    let result = write_audit_report_package(&fixture.report, &directory, |progress| {
        if progress.phase == "finding" {
            std::fs::write(
                directory.join("finding-0001-page-0001.html"),
                "concurrent file",
            )
            .unwrap();
        }
        true
    });
    assert!(result.is_err());
    assert!(!directory.join("manifest.json").exists());
    assert!(!directory.join("index.html").exists());
    assert_eq!(
        std::fs::read_to_string(directory.join("finding-0001-page-0001.html")).unwrap(),
        "concurrent file"
    );
    assert_eq!(std::fs::read_dir(directory).unwrap().count(), 1);
}
