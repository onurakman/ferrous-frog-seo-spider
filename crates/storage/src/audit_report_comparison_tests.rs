use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ff-report-comparison-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        Self { directory }
    }
    fn report(&self, id: &str, records: Vec<CrawlRecord>, scope: GridQuery) -> AuditReportStore {
        self.report_with_edges(id, records, scope, vec![])
    }
    fn report_with_edges(
        &self,
        id: &str,
        records: Vec<CrawlRecord>,
        scope: GridQuery,
        edges: Vec<LinkEdge>,
    ) -> AuditReportStore {
        let source = ActiveStore::memory();
        for record in records {
            source.upsert(record);
        }
        for edge in edges {
            source.add_link_edge(edge);
        }
        AuditReportStore::prepare(
            self.directory.join(format!("{id}.sqlite")),
            &source,
            AuditReportRequest {
                id: id.into(),
                title: id.into(),
                language: AuditReportLanguage::English,
                source_session_id: id.into(),
                source_revision: "1".into(),
                source_status: AuditSourceStatus::Completed,
                created_at: "2026-09-14T12:00:00Z".into(),
                scope,
                exclusions: vec![],
                crawl_limits: vec![],
            },
        )
        .unwrap()
    }
    fn compare(
        &self,
        baseline: &AuditReportStore,
        current: &AuditReportStore,
    ) -> AuditReportComparisonStore {
        AuditReportComparisonStore::prepare(
            self.directory.join("comparison.sqlite"),
            baseline,
            current,
            |_| true,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn page(path: &str, missing: bool) -> CrawlRecord {
    let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
    row.status_code = Some(200);
    row.content_type = Some("text/html".into());
    row.indexability_status = "Indexable".into();
    if !missing {
        row.title = Some(format!("Useful title {path}"));
        row.title_len = 20;
    }
    row
}
fn title_finding(comparison: &AuditReportComparisonStore) -> AuditComparisonFinding {
    comparison
        .query_findings(Default::default())
        .unwrap()
        .rows
        .into_iter()
        .find(|finding| finding.finding_id == "title.missing")
        .unwrap()
}
#[test]
fn comparison_computes_measured_states_and_same_net_mixed_changes() {
    for (before, after, expected, counts) in [
        (
            [false, false],
            [true, false],
            AuditComparisonStatus::New,
            (1, 0, 0),
        ),
        (
            [true, false],
            [false, false],
            AuditComparisonStatus::Resolved,
            (0, 0, 1),
        ),
        (
            [true, true],
            [true, false],
            AuditComparisonStatus::Improved,
            (0, 1, 1),
        ),
        (
            [true, false],
            [true, false],
            AuditComparisonStatus::Unchanged,
            (0, 1, 0),
        ),
        (
            [true, false],
            [true, true],
            AuditComparisonStatus::Worsened,
            (1, 1, 0),
        ),
        (
            [true, false],
            [false, true],
            AuditComparisonStatus::MixedChanges,
            (1, 0, 1),
        ),
    ] {
        let fixture = Fixture::new();
        let baseline = fixture.report(
            "baseline",
            vec![page("a", before[0]), page("b", before[1])],
            Default::default(),
        );
        let current = fixture.report(
            "current",
            vec![page("a", after[0]), page("b", after[1])],
            Default::default(),
        );
        let comparison = fixture.compare(&baseline, &current);
        let finding = title_finding(&comparison);
        assert_eq!(finding.status, expected);
        assert_eq!(
            (finding.added, finding.persisting, finding.resolved),
            counts
        );
    }
}
#[test]
fn comparison_never_resolves_missing_failed_blocked_incomplete_or_different_capture_pages() {
    for variant in 0..5 {
        let fixture = Fixture::new();
        let baseline = fixture.report("baseline", vec![page("a", true)], Default::default());
        let mut changed = page("a", false);
        match variant {
            1 => changed.status_code = Some(404),
            2 => {
                changed.status_code = None;
                changed.indexability_status = "Blocked by robots.txt".into();
            }
            3 => changed.indexability_status = "Response body incomplete".into(),
            4 => changed.js_rendered = true,
            _ => {}
        }
        let current = fixture.report(
            "current",
            if variant == 0 { vec![] } else { vec![changed] },
            Default::default(),
        );
        let comparison = fixture.compare(&baseline, &current);
        let finding = title_finding(&comparison);
        assert_eq!(finding.status, AuditComparisonStatus::NotComparable);
        assert_eq!(finding.resolved, 0);
        assert_eq!(finding.not_observed, 1);
    }
}
#[test]
fn comparison_keeps_current_only_findings_visible_without_claiming_verified_regression() {
    let fixture = Fixture::new();
    let baseline = fixture.report("baseline", vec![], Default::default());
    let current = fixture.report("current", vec![page("new", true)], Default::default());
    let comparison = fixture.compare(&baseline, &current);
    let finding = title_finding(&comparison);
    assert_eq!(finding.status, AuditComparisonStatus::NotComparable);
    assert_eq!(finding.newly_observed_current, 1);
    assert_eq!(finding.added, 0);
    let rows = comparison
        .query_evidence(AuditComparisonEvidenceQuery {
            finding_id: "title.missing".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.total, 1);
    assert!(rows.rows[0].baseline.is_none());
    assert!(rows.rows[0].current.as_ref().unwrap().evidence_id.is_some());
}
#[test]
fn comparison_pages_every_1205_fix_with_negative_values_after_source_reports_are_deleted() {
    let fixture = Fixture::new();
    let baseline = fixture.report(
        "baseline",
        (0..1205).map(|i| page(&format!("{i:04}"), true)).collect(),
        Default::default(),
    );
    let current = fixture.report(
        "current",
        (0..1205).map(|i| page(&format!("{i:04}"), false)).collect(),
        Default::default(),
    );
    let comparison = fixture.compare(&baseline, &current);
    let finding = title_finding(&comparison);
    assert_eq!(finding.resolved, 1205);
    assert_eq!(finding.status, AuditComparisonStatus::Resolved);
    for (offset, len) in [(0, 100), (600, 100), (1200, 5)] {
        let rows = comparison
            .query_evidence(AuditComparisonEvidenceQuery {
                finding_id: "title.missing".into(),
                state: Some(AuditComparisonEvidenceState::Resolved),
                offset,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.total, 1205);
        assert_eq!(rows.rows.len(), len);
    }
    let last = comparison
        .query_evidence(AuditComparisonEvidenceQuery {
            finding_id: "title.missing".into(),
            search: Some("/1204".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(last.total, 1);
    assert_eq!(
        last.rows[0]
            .current
            .as_ref()
            .unwrap()
            .observed
            .title
            .as_deref(),
        Some("Useful title 1204")
    );
    drop(baseline);
    drop(current);
    std::fs::remove_file(fixture.directory.join("baseline.sqlite")).unwrap();
    std::fs::remove_file(fixture.directory.join("current.sqlite")).unwrap();
    drop(comparison);
    let reopened =
        AuditReportComparisonStore::open(fixture.directory.join("comparison.sqlite")).unwrap();
    assert_eq!(title_finding(&reopened).resolved, 1205);
}
#[test]
fn comparison_freezes_scope_threshold_and_duplicate_context_compatibility() {
    let fixture = Fixture::new();
    let baseline = fixture.report("baseline", vec![page("a", true)], Default::default());
    let mut scope = GridQuery::default();
    scope.thresholds.title_max_chars += 1;
    let current = fixture.report("current", vec![page("a", false)], scope);
    let comparison = fixture.compare(&baseline, &current);
    assert_eq!(
        title_finding(&comparison).status,
        AuditComparisonStatus::NotComparable
    );
    assert!(
        !comparison
            .summary()
            .unwrap()
            .compatibility_reasons
            .is_empty()
    );
    let fixture = Fixture::new();
    let mut a = page("a", false);
    a.title = Some("Shared title".into());
    let mut b = page("b", false);
    b.title = a.title.clone();
    let baseline = fixture.report("baseline", vec![a.clone(), b], Default::default());
    let current = fixture.report("current", vec![a], Default::default());
    let comparison = fixture.compare(&baseline, &current);
    let duplicate = comparison
        .query_findings(Default::default())
        .unwrap()
        .rows
        .into_iter()
        .find(|f| f.finding_id == "title.duplicate")
        .unwrap();
    assert_eq!(duplicate.resolved, 0);
    assert_eq!(duplicate.status, AuditComparisonStatus::NotComparable);
}
#[test]
fn comparison_uses_request_urls_and_known_list_occurrences_across_shuffled_positions() {
    let fixture = Fixture::new();
    let occurrence = |index, position, missing| {
        let mut row = page("request", missing);
        row.final_url = "https://example.test/shared-final".into();
        row.list_position = Some(position);
        row.list_duplicate_index = index;
        row.storage_key = format!("list:{position}:{}", row.url);
        row
    };
    let baseline = fixture.report(
        "baseline",
        vec![
            occurrence(3, 10, true),
            occurrence(1, 20, true),
            occurrence(2, 30, false),
        ],
        Default::default(),
    );
    let current = fixture.report(
        "current",
        vec![
            occurrence(1, 12, false),
            occurrence(3, 44, true),
            occurrence(2, 18, true),
        ],
        Default::default(),
    );
    let comparison = fixture.compare(&baseline, &current);
    let finding = title_finding(&comparison);
    assert_eq!(finding.status, AuditComparisonStatus::MixedChanges);
    assert_eq!(
        (finding.added, finding.persisting, finding.resolved),
        (1, 1, 1)
    );
    let rows = comparison
        .query_evidence(AuditComparisonEvidenceQuery {
            finding_id: "title.missing".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        rows.rows
            .iter()
            .map(|row| (row.occurrence, row.state.clone()))
            .collect::<Vec<_>>(),
        vec![
            (1, AuditComparisonEvidenceState::Resolved),
            (2, AuditComparisonEvidenceState::Added),
            (3, AuditComparisonEvidenceState::Persisting)
        ]
    );
}

#[test]
fn comparison_requires_observed_canonical_targets_and_ignores_response_only_noise() {
    for target_present in [false, true] {
        let fixture = Fixture::new();
        let mut source = page("source", true);
        source.canonical = Some("https://example.test/target".into());
        let mut failed = page("target", false);
        failed.status_code = Some(404);
        let baseline = fixture.report("baseline", vec![source.clone(), failed], Default::default());
        source.response_time_ms = 9_999;
        source.response_hash = Some("new response nonce".into());
        let current = fixture.report(
            "current",
            if target_present {
                vec![source, page("target", false)]
            } else {
                vec![source]
            },
            Default::default(),
        );
        let comparison = fixture.compare(&baseline, &current);
        assert_eq!(
            title_finding(&comparison).status,
            AuditComparisonStatus::Unchanged
        );
        let canonical = comparison
            .query_findings(Default::default())
            .unwrap()
            .rows
            .into_iter()
            .find(|f| f.finding_id == "canonical.toError")
            .unwrap();
        assert_eq!(
            canonical.status,
            if target_present {
                AuditComparisonStatus::Resolved
            } else {
                AuditComparisonStatus::NotComparable
            }
        );
    }
}

#[test]
fn comparison_labels_legacy_duplicate_identities_and_changed_scopes_or_rule_versions() {
    for case in 0..3 {
        let fixture = Fixture::new();
        let mut a = page("a", true);
        a.list_position = Some(1);
        a.storage_key = "list:1:https://example.test/a".into();
        let mut b = a.clone();
        b.list_position = Some(2);
        b.storage_key = "list:2:https://example.test/a".into();
        let baseline = fixture.report("baseline", vec![a.clone(), b.clone()], Default::default());
        a.title = Some("Fixed".into());
        let mut scope = GridQuery::default();
        if case == 1 {
            scope.global_search = Some("/a".into());
        }
        let current = fixture.report("current", vec![a, b], scope);
        if case == 2 {
            let conn =
                rusqlite::Connection::open(fixture.directory.join("current.sqlite")).unwrap();
            let mut summary = current.summary().unwrap();
            summary.rule_version = "future-v2".into();
            conn.execute(
                "UPDATE audit_report SET payload=?1",
                [serde_json::to_string(&summary).unwrap()],
            )
            .unwrap();
        }
        let comparison = fixture.compare(&baseline, &current);
        let finding = title_finding(&comparison);
        assert_eq!(finding.status, AuditComparisonStatus::NotComparable);
        assert_eq!(finding.resolved, 0);
        assert!(!finding.compatibility_reasons.is_empty());
    }
}

#[test]
fn comparison_cancellation_queries_and_existing_destinations_are_validated() {
    let fixture = Fixture::new();
    let baseline = fixture.report("baseline", vec![page("a", true)], Default::default());
    let current = fixture.report("current", vec![page("a", false)], Default::default());
    let path = fixture.directory.join("comparison.sqlite");
    assert!(matches!(
        AuditReportComparisonStore::prepare(&path, &baseline, &current, |p| p.phase != "evidence"),
        Err(AuditReportError::Cancelled)
    ));
    assert!(!path.exists());
    let comparison = fixture.compare(&baseline, &current);
    assert!(
        comparison
            .query_evidence(AuditComparisonEvidenceQuery {
                finding_id: "title.missing".into(),
                limit: 1001,
                ..Default::default()
            })
            .is_err()
    );
    assert!(
        comparison
            .query_evidence(AuditComparisonEvidenceQuery {
                finding_id: "invented".into(),
                ..Default::default()
            })
            .is_err()
    );
    assert!(AuditReportComparisonStore::prepare(&path, &baseline, &current, |_| true).is_err());
    assert_eq!(title_finding(&comparison).resolved, 1);
}

#[test]
fn comparison_keeps_both_complete_legacy_reference_sets_without_inventing_correspondence() {
    let fixture = Fixture::new();
    let source = page("source", false);
    let mut target = page("target", false);
    target.status_code = Some(404);
    let edges = (0..2)
        .map(|position| LinkEdge {
            id: 0,
            source_url: source.url.clone(),
            target_url: target.url.clone(),
            anchor_text: format!("Reference {position}"),
            rel: "nofollow".into(),
            rel_nofollow: true,
            link_type: LinkType::Internal,
            source_status_code: Some(200),
            target_status_code: Some(404),
            source_depth: 0,
            target_depth: Some(0),
            source_position: position,
            discovery_order: 0,
        })
        .collect::<Vec<_>>();
    let baseline = fixture.report_with_edges(
        "baseline",
        vec![source.clone(), target.clone()],
        Default::default(),
        edges.clone(),
    );
    let current =
        fixture.report_with_edges("current", vec![source, target], Default::default(), edges);
    let comparison = fixture.compare(&baseline, &current);
    let finding = comparison
        .query_findings(Default::default())
        .unwrap()
        .rows
        .into_iter()
        .find(|f| f.finding_id == "links.broken")
        .unwrap();
    assert_eq!(finding.status, AuditComparisonStatus::NotComparable);
    assert_eq!(finding.not_observed, 4);
    assert_eq!(finding.unverified_current, 2);
    assert_eq!(finding.newly_observed_current, 0);
    assert_eq!(finding.baseline_counts.unwrap().occurrences, 2);
    assert_eq!(finding.current_counts.unwrap().occurrences, 2);
    let evidence = comparison
        .query_evidence(AuditComparisonEvidenceQuery {
            finding_id: "links.broken".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(evidence.total, 4);
    for row in evidence.rows {
        let side = row.baseline.or(row.current).unwrap();
        assert!(side.source_edge_id.is_some());
        assert_eq!(side.attribution, AuditAttribution::UrlOnly);
        assert_eq!(side.kind, AuditEvidenceKind::Link);
        assert!(side.source_record_id.is_none());
    }
}
