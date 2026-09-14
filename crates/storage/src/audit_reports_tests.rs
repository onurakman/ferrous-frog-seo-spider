use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "ff-audit-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn request() -> AuditReportRequest {
    AuditReportRequest {
        id: "synthetic-report".into(),
        title: "Synthetic audit".into(),
        language: AuditReportLanguage::English,
        source_session_id: "session-1".into(),
        source_revision: "revision-1".into(),
        source_status: AuditSourceStatus::Completed,
        created_at: "2026-09-14T12:00:00Z".into(),
        scope: GridQuery::default(),
        exclusions: vec![],
        crawl_limits: vec![],
    }
}
fn stores() -> [ActiveStore; 2] {
    [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ]
}
fn page(url: &str) -> CrawlRecord {
    let mut row = CrawlRecord::pending(url.into(), 0);
    row.status_code = Some(200);
    row.content_type = Some("text/html".into());
    row.indexability_status = "Indexable".into();
    row
}
fn evidence(report: &AuditReportStore, finding: &str, offset: usize) -> AuditEvidenceResponse {
    report
        .query_evidence(AuditEvidenceQuery {
            finding_id: finding.into(),
            offset,
            ..Default::default()
        })
        .unwrap()
}
#[test]
fn report_preserves_all_1205_records_identity_paging_and_survives_source_deletion() {
    for source in stores() {
        let mut keys = Vec::new();
        for i in 0..1205 {
            let mut row = page(&format!("https://example.test/{i:04}"));
            if i >= 1203 {
                row.url = "https://example.test/repeated".into();
                row.final_url = "https://example.test/final".into();
            }
            row.storage_key = format!("list:{i}:{}", row.url);
            row.list_position = Some(i + 1);
            row.list_duplicate_index = u32::from(i == 1204);
            keys.push(source.upsert(row).storage_key);
        }
        let report_path = path();
        let report = AuditReportStore::prepare(&report_path, &source, request()).unwrap();
        let finding = report
            .query_findings(Default::default())
            .unwrap()
            .rows
            .into_iter()
            .find(|f| f.id == "title.missing")
            .unwrap();
        assert_eq!(finding.counts.source_records, Some(1205));
        assert_eq!(finding.counts.unique_urls, 1204);
        assert_eq!(finding.counts.occurrences, 1205);
        for (offset, expected) in [(0, 100), (600, 100), (1200, 5), (1205, 0)] {
            let page = evidence(&report, "title.missing", offset);
            assert_eq!(page.total, 1205);
            assert_eq!(page.rows.len(), expected);
        }
        let all = [0, 1000]
            .into_iter()
            .flat_map(|offset| {
                report
                    .query_evidence(AuditEvidenceQuery {
                        finding_id: "title.missing".into(),
                        offset,
                        limit: 1000,
                        ..Default::default()
                    })
                    .unwrap()
                    .rows
            })
            .collect::<Vec<_>>();
        assert_eq!(
            all.iter()
                .filter_map(|e| e.source_storage_key.clone())
                .collect::<HashSet<_>>(),
            keys.into_iter().collect()
        );
        let last = report
            .query_evidence(AuditEvidenceQuery {
                finding_id: "title.missing".into(),
                search: Some("list:1204:".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(last.total, 1);
        assert_eq!(last.rows[0].list_position, Some(1205));
        assert_eq!(last.rows[0].original_url, "https://example.test/repeated");
        assert_eq!(
            last.rows[0].final_url.as_deref(),
            Some("https://example.test/final")
        );
        let tied = report
            .query_evidence(AuditEvidenceQuery {
                finding_id: "title.missing".into(),
                sort_by: AuditEvidenceSort::OriginalUrl,
                sort_dir: SortDirection::Desc,
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            tied.rows
                .iter()
                .map(|row| row.source_record_id)
                .collect::<Vec<_>>(),
            [Some(1204), Some(1205)]
        );
        source.clear();
        drop(report);
        let reopened = AuditReportStore::open(&report_path).unwrap();
        assert_eq!(evidence(&reopened, "title.missing", 1200).rows.len(), 5);
        assert_eq!(reopened.summary().unwrap().scope_records, 1205);
        drop(reopened);
        std::fs::remove_file(&report_path).unwrap();
    }
}
#[test]
fn report_freezes_thresholds_and_scope_but_uses_global_duplicate_context() {
    for source in stores() {
        for (path, title) in [
            ("inside", "shared title"),
            ("outside", "shared title"),
            ("missing", ""),
        ] {
            let mut row = page(&format!("https://example.test/{path}"));
            row.title = Some(title.into());
            row.title_len = title.len();
            source.upsert(row);
        }
        let mut input = request();
        input.scope.view = IssueView::TitleDuplicate;
        input.scope.segment_pattern = Some("/inside".into());
        input.scope.thresholds.title_max_chars = 5;
        input.scope.thresholds.title_min_chars = 1;
        let report_path = path();
        let report = AuditReportStore::prepare(&report_path, &source, input).unwrap();
        assert_eq!(evidence(&report, "title.duplicate", 0).total, 1);
        assert_eq!(evidence(&report, "title.tooLong", 0).total, 1);
        assert_eq!(evidence(&report, "title.missing", 0).total, 0);
        let mut row = page("https://example.test/inside");
        row.title = Some("new".into());
        row.title_len = 3;
        source.upsert(row);
        assert_eq!(evidence(&report, "title.duplicate", 0).total, 1);
        assert_eq!(
            report
                .summary()
                .unwrap()
                .request
                .scope
                .thresholds
                .title_max_chars,
            5
        );
        drop(report);
        std::fs::remove_file(&report_path).unwrap();
    }
}
#[test]
fn broken_references_keep_target_page_and_occurrence_units_and_legacy_attribution() {
    for source in stores() {
        let target = "https://example.test/broken";
        let mut broken = page(target);
        broken.status_code = Some(404);
        source.upsert(broken);
        for i in 0..40 {
            let url = format!("https://example.test/source-{i}");
            source.upsert(page(&url));
            for position in 0..15 {
                source.add_link_edge(LinkEdge {
                    id: 0,
                    source_url: url.clone(),
                    target_url: target.into(),
                    anchor_text: format!("reference {position}"),
                    rel: String::new(),
                    rel_nofollow: false,
                    link_type: LinkType::Internal,
                    source_status_code: Some(200),
                    target_status_code: Some(404),
                    source_depth: 0,
                    target_depth: Some(0),
                    source_position: position,
                    discovery_order: 0,
                });
            }
        }
        let report_path = path();
        let report = AuditReportStore::prepare(&report_path, &source, request()).unwrap();
        let finding = report
            .query_findings(Default::default())
            .unwrap()
            .rows
            .into_iter()
            .find(|f| f.id == "links.broken")
            .unwrap();
        assert_eq!(finding.counts.unique_urls, 1);
        assert_eq!(finding.counts.targets, Some(1));
        assert_eq!(finding.counts.source_pages, 40);
        assert_eq!(finding.counts.source_records, None);
        assert_eq!(finding.counts.occurrences, 600);
        let page = evidence(&report, "links.broken", 500);
        assert_eq!(page.rows.len(), 100);
        assert_eq!(page.total, 600);
        assert!(
            page.rows
                .iter()
                .all(|e| e.source_storage_key.is_none()
                    && e.attribution == AuditAttribution::UrlOnly)
        );
        assert_eq!(finding.coverage, AuditCoverageState::Incomplete);
        drop(report);
        std::fs::remove_file(&report_path).unwrap();
    }
}
#[test]
fn report_rejects_active_sources_bad_queries_and_existing_destinations() {
    let source = ActiveStore::memory();
    let report_path = path();
    let mut input = request();
    input.source_status = AuditSourceStatus::Running;
    assert!(AuditReportStore::prepare(&report_path, &source, input).is_err());
    assert!(!report_path.exists());
    let report = AuditReportStore::prepare(&report_path, &source, request()).unwrap();
    assert!(AuditReportStore::prepare(&report_path, &source, request()).is_err());
    assert!(
        report
            .query_evidence(AuditEvidenceQuery {
                finding_id: "title.missing".into(),
                limit: 1001,
                ..Default::default()
            })
            .is_err()
    );
    assert!(
        report
            .query_evidence(AuditEvidenceQuery {
                finding_id: "invented".into(),
                ..Default::default()
            })
            .is_err()
    );
    drop(report);
    std::fs::remove_file(&report_path).unwrap();
    std::fs::write(&report_path, b"not a report").unwrap();
    assert!(AuditReportStore::open(&report_path).is_err());
    std::fs::remove_file(&report_path).unwrap();
}

#[test]
fn report_cancellation_never_publishes_and_queries_preserve_full_unicode_evidence() {
    let source = ActiveStore::memory();
    let mut row = page("https://example.test/long");
    row.title = Some("ç".repeat(40_000));
    row.title_len = 40_000;
    source.upsert(row);
    let report_path = path();
    assert!(matches!(
        AuditReportStore::prepare_with_progress(&report_path, &source, request(), |progress| {
            progress.phase != "findings"
        }),
        Err(AuditReportError::Cancelled)
    ));
    assert!(!report_path.exists());
    let mut input = request();
    input.source_status = AuditSourceStatus::Stopped;
    let report = AuditReportStore::prepare(&report_path, &source, input).unwrap();
    assert!(report.summary().unwrap().partial);
    let preview = evidence(&report, "title.tooLong", 0);
    assert!(preview.rows[0].preview_truncated);
    assert_eq!(
        preview.rows[0].observed.title.as_ref().unwrap().len(),
        65_536
    );
    let full = report
        .query_evidence(AuditEvidenceQuery {
            finding_id: "title.tooLong".into(),
            preview: false,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(full.rows[0].observed.title.as_ref().unwrap().len(), 80_000);
    drop(report);
    std::fs::remove_file(&report_path).unwrap();
}

#[test]
fn report_validates_thresholds_regex_and_retains_negative_capture_evidence() {
    let source = ActiveStore::memory();
    let report_path = path();
    let mut input = request();
    input.scope.thresholds.title_max_chars = 0;
    assert!(AuditReportStore::prepare(&report_path, &source, input).is_err());
    let mut input = request();
    input.scope.segment_regex = true;
    input.scope.segment_pattern = Some("[".into());
    assert!(AuditReportStore::prepare(&report_path, &source, input).is_err());
    let mut complete = page("https://example.test/eligible");
    complete.title = Some("Valid title".into());
    complete.title_len = 11;
    source.upsert(complete);
    let mut incomplete = page("https://example.test/incomplete");
    incomplete.indexability_status = "Response body incomplete".into();
    source.upsert(incomplete);
    let mut non_html = page("https://example.test/image");
    non_html.content_type = Some("image/png".into());
    source.upsert(non_html);
    let report = AuditReportStore::prepare(&report_path, &source, request()).unwrap();
    assert_eq!(evidence(&report, "title.missing", 0).total, 0);
    let summary = report.summary().unwrap();
    assert_eq!(summary.scope_records, 3);
    assert_eq!(summary.eligible_html_records, 1);
    assert_eq!(summary.unavailable_html_records, 1);
    let conn = rusqlite::Connection::open(&report_path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM crawl_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    drop(conn);
    drop(report);
    std::fs::remove_file(&report_path).unwrap();
}

#[test]
fn sqlite_snapshot_is_consistent_during_external_writes_and_preserves_source_id_gaps() {
    let source_path = path();
    let source = SqliteStore::open(&source_path).unwrap();
    for i in 0..205 {
        source.upsert(page(&format!("https://example.test/{i:03}")));
    }
    source
        .connection()
        .unwrap()
        .execute("DELETE FROM crawl_records WHERE id=1", [])
        .unwrap();
    let external = SqliteStore::open(&source_path).unwrap();
    let report_path = path();
    let report = AuditReportStore::prepare_with_progress(
        &report_path,
        &ActiveStore::Sqlite(source.clone()),
        request(),
        |progress| {
            if progress.phase == "snapshot" && progress.completed == 100 {
                let mut changed = page("https://example.test/204");
                changed.title = Some("Later title".into());
                changed.title_len = 11;
                external.upsert(changed);
            }
            true
        },
    )
    .unwrap();
    let last = report
        .query_evidence(AuditEvidenceQuery {
            finding_id: "title.missing".into(),
            search: Some("/204".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(last.total, 1);
    assert_eq!(last.rows[0].source_record_id, Some(205));
    assert!(last.rows[0].observed.title.is_none());
    assert_eq!(evidence(&report, "title.missing", 0).total, 204);
    drop(report);
    drop(external);
    drop(source);
    std::fs::remove_file(report_path).unwrap();
    std::fs::remove_file(source_path).unwrap();
}

#[test]
fn broken_source_read_fails_without_publishing_or_replacing_files() {
    let source = SqliteStore::in_memory().unwrap();
    source
        .connection()
        .unwrap()
        .execute("DROP TABLE link_edges", [])
        .unwrap();
    let report_path = path();
    assert!(
        AuditReportStore::prepare(&report_path, &ActiveStore::Sqlite(source), request()).is_err()
    );
    assert!(!report_path.exists());
}

#[test]
fn saved_source_preparation_is_read_only_and_rejects_legacy_without_migration() {
    let source_path = path();
    let source = SqliteStore::open(&source_path).unwrap();
    source.upsert(page("https://example.test/saved"));
    drop(source);
    let before = std::fs::read(&source_path).unwrap();
    let report_path = path();
    let report = AuditReportStore::prepare_saved_with_progress(
        &report_path,
        &source_path,
        request(),
        |_| true,
    )
    .unwrap();
    assert_eq!(evidence(&report, "title.missing", 0).total, 1);
    assert_eq!(std::fs::read(&source_path).unwrap(), before);
    drop(report);
    std::fs::remove_file(&report_path).unwrap();
    std::fs::remove_file(&source_path).unwrap();
    let legacy = rusqlite::Connection::open(&source_path).unwrap();
    legacy
        .execute_batch("CREATE TABLE crawl_records(id INTEGER PRIMARY KEY,url TEXT)")
        .unwrap();
    drop(legacy);
    let before = std::fs::read(&source_path).unwrap();
    assert!(
        AuditReportStore::prepare_saved_with_progress(
            &report_path,
            &source_path,
            request(),
            |_| true
        )
        .is_err()
    );
    assert!(!report_path.exists());
    assert_eq!(std::fs::read(&source_path).unwrap(), before);
    std::fs::remove_file(source_path).unwrap();
}

#[test]
fn report_applies_scope_before_pruning_unrelated_payloads_from_the_snapshot() {
    let source = ActiveStore::memory();
    let mut row = page("https://example.test/selected");
    row.custom_extractions = vec![CustomExtractionValue {
        name: "segment".into(),
        values: vec!["selected from extraction".into()],
    }];
    row.custom_searches = vec![CustomSearchValue {
        name: "unrelated".into(),
        source: CustomSearchSource::RawHtml,
        matched: true,
        match_count: 1,
        snippets: vec!["PRIVATE_UNUSED_SEARCH_TEXT".into()],
    }];
    row.structured_data_issues = vec![StructuredDataIssue {
        severity: "warning".into(),
        message: "PRIVATE_UNUSED_STRUCTURED_DATA".into(),
        path: "$.unused".into(),
    }];
    source.upsert(row);
    source.upsert(page("https://example.test/outside"));
    let mut input = request();
    input.scope.global_search = Some("selected from extraction".into());
    let report_path = path();
    let report = AuditReportStore::prepare(&report_path, &source, input).unwrap();
    assert_eq!(report.summary().unwrap().scope_records, 1);
    assert_eq!(evidence(&report, "title.missing", 0).total, 1);
    let conn = rusqlite::Connection::open(&report_path).unwrap();
    let payloads:(String,String,String)=conn.query_row("SELECT custom_extractions,custom_searches,structured_data_issues FROM crawl_records WHERE url='https://example.test/selected'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(payloads, ("[]".into(), "[]".into(), "[]".into()));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM crawl_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(conn);
    drop(report);
    let bytes = std::fs::read(&report_path).unwrap();
    assert!(
        !bytes
            .windows(b"PRIVATE_UNUSED_SEARCH_TEXT".len())
            .any(|w| w == b"PRIVATE_UNUSED_SEARCH_TEXT")
    );
    assert!(
        !bytes
            .windows(b"PRIVATE_UNUSED_STRUCTURED_DATA".len())
            .any(|w| w == b"PRIVATE_UNUSED_STRUCTURED_DATA")
    );
    std::fs::remove_file(report_path).unwrap();
}
