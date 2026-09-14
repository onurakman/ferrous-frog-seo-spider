use super::*;
use ferrous_frog_storage::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    root: PathBuf,
    comparison: AuditReportComparisonStore,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn fixture(count: usize, current_eligible: bool) -> Fixture {
    fixture_language(count, current_eligible, AuditReportLanguage::English)
}
fn fixture_language(
    count: usize,
    current_eligible: bool,
    language: AuditReportLanguage,
) -> Fixture {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "ff-comparison-export-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let baseline_source = ActiveStore::memory();
    let current_source = ActiveStore::memory();
    for index in 0..count {
        let mut baseline = CrawlRecord::pending(format!("https://example.test/{index:04}"), 0);
        baseline.status_code = Some(200);
        baseline.content_type = Some("text/html".into());
        baseline.indexability_status = "Indexable".into();
        baseline.h1 = Some("=HYPERLINK(\"evil\") <script>bad()</script>".into());
        baseline_source.upsert(baseline);
        let mut current = CrawlRecord::pending(format!("https://example.test/{index:04}"), 0);
        current.status_code = current_eligible.then_some(200);
        current.content_type = Some("text/html".into());
        current.indexability_status = "Indexable".into();
        current.title = Some("Fixed title".into());
        current.title_len = 11;
        current_source.upsert(current);
    }
    let request = |id: &str| AuditReportRequest {
        id: id.into(),
        title: format!("{id} <img src=x>"),
        language: language.clone(),
        source_session_id: id.into(),
        source_revision: "v1".into(),
        source_status: AuditSourceStatus::Completed,
        created_at: "2026-09-14T00:00:00Z".into(),
        scope: GridQuery::default(),
        exclusions: vec![],
        crawl_limits: vec![],
    };
    let baseline = AuditReportStore::prepare(
        root.join("baseline.sqlite"),
        &baseline_source,
        request("baseline"),
    )
    .unwrap();
    let current = AuditReportStore::prepare(
        root.join("current.sqlite"),
        &current_source,
        request("current"),
    )
    .unwrap();
    let comparison = AuditReportComparisonStore::prepare(
        root.join("comparison.sqlite"),
        &baseline,
        &current,
        |_| true,
    )
    .unwrap();
    drop(baseline);
    drop(current);
    std::fs::remove_file(root.join("baseline.sqlite")).unwrap();
    std::fs::remove_file(root.join("current.sqlite")).unwrap();
    Fixture { root, comparison }
}

#[test]
fn comparison_package_streams_1205_resolved_rows_to_two_html_pages_and_full_csv() {
    let fixture = fixture(1205, true);
    let output = fixture.root.join("package");
    std::fs::create_dir(&output).unwrap();
    let manifest =
        write_audit_report_comparison_package(&fixture.comparison, &output, |_| true).unwrap();
    let finding = manifest
        .findings
        .iter()
        .find(|item| item.finding_id == "title.missing")
        .unwrap();
    assert_eq!(finding.status, AuditComparisonStatus::Resolved);
    assert_eq!(finding.resolved, 1205);
    assert_eq!(finding.html_pages.len(), 2);
    let rows = csv::Reader::from_path(output.join(&finding.csv_file))
        .unwrap()
        .records()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1205);
    assert!(
        rows[1204]
            .iter()
            .any(|cell| cell.contains("https://example.test/1204"))
    );
    let last = std::fs::read_to_string(output.join(&finding.html_pages[1])).unwrap();
    assert!(
        last.replace("&#x2f;", "/")
            .contains("https://example.test/1204")
    );
    assert!(!last.contains("<script>bad"));
    assert!(last.contains("&lt;script&gt;bad"));
    let index = std::fs::read_to_string(output.join("index.html")).unwrap();
    assert!(!index.contains("<img src=x>"));
    assert!(index.contains("&lt;img src=x&gt;"));
    assert_eq!(
        manifest.evidence_rows,
        manifest
            .findings
            .iter()
            .map(|item| item.added + item.persisting + item.resolved + item.not_observed)
            .sum::<usize>()
    );
    if let Some(destination) = std::env::var_os("FF_AUDIT_COMPARISON_OFFLINE_FIXTURE_DIR") {
        let destination = PathBuf::from(destination);
        std::fs::create_dir_all(&destination).unwrap();
        for file in &manifest.files {
            std::fs::copy(output.join(file), destination.join(file)).unwrap();
        }
    }
}

#[test]
fn comparison_package_marks_ineligible_current_rows_not_observed_and_cleans_cancelled_writes() {
    let fixture = fixture(2, false);
    let output = fixture.root.join("package");
    std::fs::create_dir(&output).unwrap();
    let result = write_audit_report_comparison_package(&fixture.comparison, &output, |progress| {
        progress.phase != "comparisonEvidence"
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);
    let manifest =
        write_audit_report_comparison_package(&fixture.comparison, &output, |_| true).unwrap();
    let finding = manifest
        .findings
        .iter()
        .find(|item| item.finding_id == "title.missing")
        .unwrap();
    assert_eq!(finding.resolved, 0);
    assert_eq!(finding.not_observed, 2);
    let page = std::fs::read_to_string(output.join(&finding.html_pages[0])).unwrap();
    assert!(page.contains("Not observed does not verify a fix."));
    assert!(page.contains("notObserved"));
}

#[test]
fn turkish_comparison_localizes_visible_labels_and_warning_but_retains_raw_metadata() {
    let fixture = fixture_language(2, false, AuditReportLanguage::Turkish);
    let output = fixture.root.join("turkish-package");
    std::fs::create_dir(&output).unwrap();
    let manifest =
        write_audit_report_comparison_package(&fixture.comparison, &output, |_| true).unwrap();
    let finding = manifest
        .findings
        .iter()
        .find(|item| item.finding_id == "title.missing")
        .unwrap();
    assert_eq!(finding.not_observed, 2);
    let index = std::fs::read_to_string(output.join("index.html")).unwrap();
    let page = std::fs::read_to_string(output.join(&finding.html_pages[0])).unwrap();
    for html in [&index, &page] {
        assert!(html.contains("lang=\"tr\""));
        assert!(html.contains("Gözlemlenmemesi düzeltmenin doğrulandığı anlamına gelmez."));
        assert!(!html.contains("Not observed does not verify a fix."));
    }
    assert!(index.contains("Sayfa başlığı eksik"));
    assert!(index.contains("Karşılaştırılamaz"));
    assert!(page.contains("Gözlemlenmedi"));
    assert!(!page.contains("<script>bad()"));
    assert!(page.contains("&lt;script&gt;"));
    assert!(
        page.contains("notObserved"),
        "raw evidence JSON keeps its original state code"
    );
    let raw: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(output.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        raw["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["findingId"] == "title.missing")
            .unwrap()["status"],
        "notComparable"
    );
}

#[test]
fn comparison_ai_commentary_is_optional_escaped_and_keeps_measured_states_and_csv() {
    for language in [AuditReportLanguage::English, AuditReportLanguage::Turkish] {
        let fixture = fixture_language(2, false, language.clone());
        let plain = fixture.root.join("plain");
        let annotated = fixture.root.join("annotated");
        std::fs::create_dir(&plain).unwrap();
        std::fs::create_dir(&annotated).unwrap();
        let measured =
            write_audit_report_comparison_package(&fixture.comparison, &plain, |_| true).unwrap();
        assert_eq!(measured.ai_annotated_findings, 0);
        assert!(measured.ai_overview.is_none());
        assert!(measured.ai_generation_version.is_none());
        let overview = serde_json::json!({
            "summary": "Saved comparison commentary <script>bad()</script>",
            "prioritizedFindingIds": ["title.missing"],
            "limitations": ["Only captured evidence <img src=x>"],
            "includedFindingCount": 1, "totalFindingCount": measured.finding_count,
            "sampledEvidenceCount": 1, "partialCoverage": true, "model": "fixture-model"
        });
        let manifest =
            crate::audit_report_comparison::write_audit_report_comparison_package_with_annotations(
                &fixture.comparison,
                &annotated,
                |id| {
                    Ok((id == "title.missing").then(|| serde_json::json!({
                "findingId": id, "explanation": "Captured changes <script>bad()</script>",
                "proposedCause": "Possible cause <img src=x>", "recommendation": "Inspect & verify",
                "verification": "Crawl again", "suggestedTeam": "Engineering",
                "model": "fixture-model <svg onload=bad()>", "sampleCount": 1, "evidenceTotal": 2,
                "evidenceIds": ["comparison:1"], "resolved": 999, "status": "resolved"
            })))
                },
                Some(overview.clone()),
                Some("previous-usable-generation<1>"),
                |_| true,
            )
            .unwrap();
        assert_eq!(manifest.ai_annotated_findings, 1);
        assert_eq!(manifest.ai_overview, Some(overview));
        assert_eq!(
            manifest.ai_generation_version.as_deref(),
            Some("previous-usable-generation<1>")
        );
        assert_eq!(manifest.evidence_rows, measured.evidence_rows);
        assert_eq!(
            serde_json::to_value(&manifest.findings).unwrap(),
            serde_json::to_value(&measured.findings).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&manifest.summary).unwrap(),
            serde_json::to_value(&measured.summary).unwrap()
        );
        for finding in &manifest.findings {
            assert_eq!(
                std::fs::read(annotated.join(&finding.csv_file)).unwrap(),
                std::fs::read(plain.join(&finding.csv_file)).unwrap()
            );
        }
        let index = std::fs::read_to_string(annotated.join("index.html")).unwrap();
        for hostile in ["<script>bad()", "<img src=x>", "<svg onload=bad()>"] {
            assert!(!index.contains(hostile));
        }
        assert!(index.contains("&lt;script&gt;bad()"));
        assert!(index.contains("previous-usable-generation&lt;1&gt;"));
        assert!(index.contains("comparison:1"));
        assert!(index.contains("fixture-model &lt;svg"));
        assert!(!index.contains("<b>999</b>"));
        assert!(index.contains(if language == AuditReportLanguage::Turkish {
            "Yapay zekâ yorumları doğrulanmamıştır"
        } else {
            "AI interpretations are unverified"
        }));
        assert!(index.contains("href=\"#comparison-finding-"));
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(annotated.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            stored["aiGenerationVersion"],
            "previous-usable-generation<1>"
        );
    }
}

#[test]
fn comparison_annotation_read_failure_removes_partial_package() {
    let fixture = fixture(2, true);
    let output = fixture.root.join("package");
    std::fs::create_dir(&output).unwrap();
    let result =
        crate::audit_report_comparison::write_audit_report_comparison_package_with_annotations(
            &fixture.comparison,
            &output,
            |_| Err::<Option<()>, _>("annotation read failed".into()),
            None,
            None,
            |_| true,
        );
    assert!(result.unwrap_err().contains("annotation read failed"));
    assert_eq!(std::fs::read_dir(output).unwrap().count(), 0);
}
