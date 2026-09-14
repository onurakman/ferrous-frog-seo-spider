//! Reproducible report acceptance workload; uses synthetic data and makes no network requests.
//! cargo run -p ferrous-frog-export --example audit_report_scale --locked -- /tmp/ff-report-scale 100000 1000001

use ferrous_frog_export::write_audit_report_package;
use ferrous_frog_storage::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn rss_kib() -> Option<usize> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
}

fn metric(phase: &str, started: Instant, extra: serde_json::Value) {
    println!(
        "{}",
        json!({"phase":phase,"elapsedSeconds":started.elapsed().as_secs_f64(),"peakRssKiB":rss_kib(),"details":extra})
    );
}

fn request() -> AuditReportRequest {
    AuditReportRequest {
        id: "scale-report".into(),
        title: "Synthetic scale acceptance".into(),
        language: AuditReportLanguage::English,
        source_session_id: "synthetic".into(),
        source_revision: "fixed".into(),
        source_status: AuditSourceStatus::Completed,
        created_at: "2026-09-14T00:00:00Z".into(),
        scope: GridQuery::default(),
        exclusions: vec![],
        crawl_limits: vec![],
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let directory = PathBuf::from(
        args.get(1)
            .ok_or("Supply a new, empty output directory path")?,
    );
    let pages = args
        .get(2)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(100_000_usize);
    let edges = args
        .get(3)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(1_000_001_usize);
    if pages == 0 {
        return Err("At least one source page is required".into());
    }
    let reuse = args.get(4).is_some_and(|value| value == "--reuse");
    if !reuse {
        std::fs::create_dir(&directory)?;
        let store = SqliteStore::open(directory.join("source.sqlite3"))?;
        let started = Instant::now();
        for index in 0..pages {
            let mut row =
                CrawlRecord::pending(format!("https://scale.example.test/page/{index:08}"), 1);
            row.status_code = Some(200);
            row.content_type = Some("text/html".into());
            row.meta_description = Some(format!(
                "A unique captured description for synthetic page {index:08}. {}",
                "Description content. ".repeat(4)
            ));
            row.meta_description_len = row.meta_description.as_ref().unwrap().len();
            row.h1 = Some("Synthetic page".into());
            row.h1_len = 14;
            row.canonical = Some(row.url.clone());
            row.canonical_count = 1;
            store.try_upsert(row)?;
        }
        let target = "https://scale.example.test/missing";
        let mut broken = CrawlRecord::pending(target.into(), 1);
        broken.status_code = Some(404);
        store.try_upsert(broken)?;
        metric("sourceRecords", started, json!({"records":pages+1}));
        let started = Instant::now();
        for index in 0..edges {
            store.try_add_link_edge(LinkEdge {
                id: 0,
                source_url: format!("https://scale.example.test/page/{:08}", index % pages),
                target_url: target.into(),
                anchor_text: format!("Reference {index}"),
                rel: String::new(),
                rel_nofollow: false,
                link_type: LinkType::Internal,
                source_status_code: Some(200),
                target_status_code: Some(404),
                source_depth: 1,
                target_depth: Some(1),
                source_position: (index / pages + 1) as u32,
                discovery_order: 0,
            })?;
            if index > 0 && index.is_multiple_of(100_000) {
                metric("sourceEdgesProgress", started, json!({"edges":index}));
            }
        }
        metric("sourceEdges", started, json!({"edges":edges}));
        drop(store);
    }
    let started = Instant::now();
    let mut previous_phase = String::new();
    let report = if reuse {
        AuditReportStore::open(directory.join("report.sqlite3"))?
    } else {
        AuditReportStore::prepare_saved_with_progress(
            directory.join("report.sqlite3"),
            directory.join("source.sqlite3"),
            request(),
            |progress| {
                if progress.phase != previous_phase || progress.completed.is_multiple_of(100_000) {
                    metric(
                        "preparationProgress",
                        started,
                        json!({"step":progress.phase,"completed":progress.completed}),
                    );
                    previous_phase = progress.phase;
                }
                true
            },
        )?
    };
    metric(
        if reuse { "reopen" } else { "prepare" },
        started,
        json!({"summary":report.summary()?,"databaseBytes":std::fs::metadata(directory.join("report.sqlite3"))?.len()}),
    );
    for (finding, expected) in [("title.missing", pages), ("links.broken", edges)] {
        for offset in [0, expected / 2, expected.saturating_sub(100)] {
            let started = Instant::now();
            let result = report.query_evidence(AuditEvidenceQuery {
                finding_id: finding.into(),
                offset,
                ..Default::default()
            })?;
            assert_eq!(result.total, expected);
            assert_eq!(result.rows.len(), (expected - offset).min(100));
            let ipc_bytes = serde_json::to_vec(&result)?.len();
            metric(
                "query",
                started,
                json!({"finding":finding,"offset":offset,"rows":result.rows.len(),"total":result.total,"serializedBytes":ipc_bytes,"lastId":result.rows.last().map(|row| &row.id)}),
            );
        }
    }
    let started = Instant::now();
    let package = directory.join(if reuse {
        format!(
            "package-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
        )
    } else {
        "package".into()
    });
    std::fs::create_dir(&package)?;
    let mut logged = 0;
    let manifest = write_audit_report_package(&report, &package, |progress| {
        if progress.completed >= logged + 50_000 {
            logged = progress.completed;
            metric(
                "exportProgress",
                started,
                json!({"completed":progress.completed,"total":progress.total}),
            );
        }
        true
    })?;
    metric(
        "export",
        started,
        json!({"rows":manifest.evidence_rows,"files":manifest.files.len()}),
    );
    for finding in &manifest.findings {
        let mut csv = csv::Reader::from_path(package.join(&finding.csv_file))?;
        let mut total = 0;
        for record in csv.records() {
            record?;
            total += 1;
        }
        assert_eq!(total, finding.counts.occurrences);
        assert_eq!(finding.html_pages.len(), total.div_ceil(1_000));
    }
    assert_eq!(
        manifest
            .findings
            .iter()
            .find(|finding| finding.finding_id == "title.missing")
            .unwrap()
            .counts
            .occurrences,
        pages
    );
    assert_eq!(
        manifest
            .findings
            .iter()
            .find(|finding| finding.finding_id == "links.broken")
            .unwrap()
            .counts
            .occurrences,
        edges
    );
    metric(
        "verified",
        started,
        json!({"rows":manifest.evidence_rows,"outputDirectory":directory,"outputBytes":tree_bytes(&directory)?}),
    );
    Ok(())
}

fn tree_bytes(path: &Path) -> std::io::Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        total += if metadata.is_dir() {
            tree_bytes(&entry.path())?
        } else {
            metadata.len()
        };
    }
    Ok(total)
}
