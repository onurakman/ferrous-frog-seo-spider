use csv::Writer;
use ferrous_frog_storage::{CrawlRecord, LinkEdge, summarize};
use minijinja::{Environment, context};
use rust_xlsxwriter::{Workbook, XlsxError};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;

const HEADERS: [&str; 65] = [
    "id",
    "url",
    "final_url",
    "classification",
    "status_code",
    "status_text",
    "content_type",
    "indexability",
    "indexability_status",
    "response_time_ms",
    "dns_lookup_time_ms",
    "ttfb_ms",
    "download_time_ms",
    "total_network_time_ms",
    "transfer_rate_bytes_per_sec",
    "resolved_ip_count",
    "size_bytes",
    "response_hash",
    "word_count",
    "text_to_code_ratio",
    "simhash",
    "near_duplicate_cluster_id",
    "depth",
    "redirect_target",
    "title",
    "title_len",
    "meta_description",
    "meta_description_len",
    "meta_robots",
    "x_robots_tag",
    "h1",
    "h1_len",
    "h1_count",
    "h2",
    "h2_len",
    "h2_count",
    "canonical",
    "canonical_count",
    "image_count",
    "images_missing_alt",
    "images_alt_too_long",
    "mixed_content_count",
    "insecure_form_count",
    "hsts_header",
    "content_security_policy_header",
    "x_frame_options_header",
    "x_content_type_options_header",
    "viewport",
    "amphtml",
    "rel_next",
    "rel_prev",
    "hreflang_count",
    "hreflang_invalid_count",
    "hreflang_missing_self_reference",
    "json_ld_count",
    "json_ld_invalid_count",
    "open_graph_count",
    "twitter_card_count",
    "inlink_count",
    "outlink_count",
    "internal_outlink_count",
    "external_outlink_count",
    "custom_extractions",
    "error",
    "in_sitemap",
];

const LINK_EDGE_HEADERS: [&str; 13] = [
    "id",
    "source_url",
    "target_url",
    "anchor_text",
    "rel",
    "rel_nofollow",
    "link_type",
    "source_status_code",
    "target_status_code",
    "source_depth",
    "target_depth",
    "source_position",
    "discovery_order",
];

const REDIRECT_CHAIN_HEADERS: [&str; 10] = [
    "record_id",
    "source_url",
    "final_url",
    "hop_index",
    "hop_url",
    "status_code",
    "location",
    "dns_lookup_time_ms",
    "ttfb_ms",
    "elapsed_ms",
];

const HTML_REPORT_ROW_LIMIT: usize = 50;
const TITLE_MIN_LENGTH: usize = 30;
const TITLE_MAX_LENGTH: usize = 60;
const META_MIN_LENGTH: usize = 70;
const META_MAX_LENGTH: usize = 160;
const H1_MAX_LENGTH: usize = 70;
const LARGE_IMAGE_BYTES: usize = 200 * 1024;
const LARGE_PAGE_BYTES: usize = 1_000_000;
const SLOW_TTFB_MS: u64 = 800;
const SLOW_RESPONSE_MS: u64 = 3_000;
const HTML_REPORT_TEMPLATE: &str = include_str!("../templates/seo_report.html.j2");

pub fn records_to_csv<W: Write>(records: &[CrawlRecord], writer: W) -> csv::Result<()> {
    let mut writer = Writer::from_writer(writer);
    writer.write_record(HEADERS)?;

    for record in records {
        writer.write_record([
            record.id.to_string(),
            record.url.clone(),
            record.final_url.clone(),
            format!("{:?}", record.classification),
            record
                .status_code
                .map(|code| code.to_string())
                .unwrap_or_default(),
            record.status_text.clone(),
            record.content_type.clone().unwrap_or_default(),
            record.indexability.clone(),
            record.indexability_status.clone(),
            record.response_time_ms.to_string(),
            record
                .dns_lookup_time_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record
                .ttfb_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record
                .download_time_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record
                .total_network_time_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record
                .transfer_rate_bytes_per_sec
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.resolved_ip_count.to_string(),
            record.size_bytes.to_string(),
            record.response_hash.clone().unwrap_or_default(),
            record.word_count.to_string(),
            format!("{:.4}", record.text_to_code_ratio),
            record
                .simhash
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record
                .near_duplicate_cluster_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.depth.to_string(),
            record.redirect_target.clone().unwrap_or_default(),
            record.title.clone().unwrap_or_default(),
            record.title_len.to_string(),
            record.meta_description.clone().unwrap_or_default(),
            record.meta_description_len.to_string(),
            record.meta_robots.clone().unwrap_or_default(),
            record.x_robots_tag.clone().unwrap_or_default(),
            record.h1.clone().unwrap_or_default(),
            record.h1_len.to_string(),
            record.h1_count.to_string(),
            record.h2.clone().unwrap_or_default(),
            record.h2_len.to_string(),
            record.h2_count.to_string(),
            record.canonical.clone().unwrap_or_default(),
            record.canonical_count.to_string(),
            record.image_count.to_string(),
            record.images_missing_alt.to_string(),
            record.images_alt_too_long.to_string(),
            record.mixed_content_count.to_string(),
            record.insecure_form_count.to_string(),
            record.hsts_header.to_string(),
            record.content_security_policy_header.to_string(),
            record.x_frame_options_header.to_string(),
            record.x_content_type_options_header.to_string(),
            record.viewport.to_string(),
            record.amphtml.clone().unwrap_or_default(),
            record.rel_next.clone().unwrap_or_default(),
            record.rel_prev.clone().unwrap_or_default(),
            record.hreflang_count.to_string(),
            record.hreflang_invalid_count.to_string(),
            record.hreflang_missing_self_reference.to_string(),
            record.json_ld_count.to_string(),
            record.json_ld_invalid_count.to_string(),
            record.open_graph_count.to_string(),
            record.twitter_card_count.to_string(),
            record.inlink_count.to_string(),
            record.outlink_count.to_string(),
            record.internal_outlink_count.to_string(),
            record.external_outlink_count.to_string(),
            serde_json::to_string(&record.custom_extractions).unwrap_or_default(),
            record.error.clone().unwrap_or_default(),
            record.in_sitemap.to_string(),
        ])?;
    }

    writer.flush()?;
    Ok(())
}

pub fn records_to_csv_string(records: &[CrawlRecord]) -> csv::Result<String> {
    let mut bytes = Vec::new();
    records_to_csv(records, &mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn link_edges_to_csv<W: Write>(edges: &[LinkEdge], writer: W) -> csv::Result<()> {
    let mut writer = Writer::from_writer(writer);
    writer.write_record(LINK_EDGE_HEADERS)?;

    for edge in edges {
        writer.write_record([
            edge.id.to_string(),
            edge.source_url.clone(),
            edge.target_url.clone(),
            edge.anchor_text.clone(),
            edge.rel.clone(),
            edge.rel_nofollow.to_string(),
            format!("{:?}", edge.link_type),
            edge.source_status_code
                .map(|code| code.to_string())
                .unwrap_or_default(),
            edge.target_status_code
                .map(|code| code.to_string())
                .unwrap_or_default(),
            edge.source_depth.to_string(),
            edge.target_depth
                .map(|depth| depth.to_string())
                .unwrap_or_default(),
            edge.source_position.to_string(),
            edge.discovery_order.to_string(),
        ])?;
    }

    writer.flush()?;
    Ok(())
}

pub fn link_edges_to_csv_string(edges: &[LinkEdge]) -> csv::Result<String> {
    let mut bytes = Vec::new();
    link_edges_to_csv(edges, &mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn redirect_chains_to_csv<W: Write>(records: &[CrawlRecord], writer: W) -> csv::Result<()> {
    let mut writer = Writer::from_writer(writer);
    writer.write_record(REDIRECT_CHAIN_HEADERS)?;

    for record in records {
        for (index, hop) in record.redirect_chain.iter().enumerate() {
            writer.write_record([
                record.id.to_string(),
                record.url.clone(),
                record.final_url.clone(),
                (index + 1).to_string(),
                hop.url.clone(),
                hop.status_code.to_string(),
                hop.location.clone().unwrap_or_default(),
                hop.dns_lookup_time_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                hop.ttfb_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                hop.elapsed_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ])?;
        }
    }

    writer.flush()?;
    Ok(())
}

pub fn redirect_chains_to_csv_string(records: &[CrawlRecord]) -> csv::Result<String> {
    let mut bytes = Vec::new();
    redirect_chains_to_csv(records, &mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn records_to_xlsx_bytes(records: &[CrawlRecord]) -> Result<Vec<u8>, XlsxError> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("Crawl Results")?;

    for (column, header) in HEADERS.iter().enumerate() {
        worksheet.write_string(0, column as u16, *header)?;
    }

    for (index, record) in records.iter().enumerate() {
        let row = (index + 1) as u32;
        worksheet.write_number(row, 0, record.id as f64)?;
        worksheet.write_string(row, 1, &record.url)?;
        worksheet.write_string(row, 2, &record.final_url)?;
        worksheet.write_string(row, 3, format!("{:?}", record.classification))?;
        if let Some(status_code) = record.status_code {
            worksheet.write_number(row, 4, f64::from(status_code))?;
        }
        worksheet.write_string(row, 5, &record.status_text)?;
        worksheet.write_string(row, 6, record.content_type.as_deref().unwrap_or_default())?;
        worksheet.write_string(row, 7, &record.indexability)?;
        worksheet.write_string(row, 8, &record.indexability_status)?;
        worksheet.write_number(row, 9, record.response_time_ms as f64)?;
        if let Some(value) = record.dns_lookup_time_ms {
            worksheet.write_number(row, 10, value as f64)?;
        }
        if let Some(value) = record.ttfb_ms {
            worksheet.write_number(row, 11, value as f64)?;
        }
        if let Some(value) = record.download_time_ms {
            worksheet.write_number(row, 12, value as f64)?;
        }
        if let Some(value) = record.total_network_time_ms {
            worksheet.write_number(row, 13, value as f64)?;
        }
        if let Some(value) = record.transfer_rate_bytes_per_sec {
            worksheet.write_number(row, 14, value as f64)?;
        }
        worksheet.write_number(row, 15, f64::from(record.resolved_ip_count))?;
        worksheet.write_number(row, 16, record.size_bytes as f64)?;
        worksheet.write_string(row, 17, record.response_hash.as_deref().unwrap_or_default())?;
        worksheet.write_number(row, 18, record.word_count as f64)?;
        worksheet.write_number(row, 19, record.text_to_code_ratio)?;
        worksheet.write_string(
            row,
            20,
            record
                .simhash
                .map(|value| value.to_string())
                .unwrap_or_default(),
        )?;
        if let Some(cluster_id) = record.near_duplicate_cluster_id {
            worksheet.write_number(row, 21, cluster_id as f64)?;
        }
        worksheet.write_number(row, 22, record.depth as f64)?;
        worksheet.write_string(
            row,
            23,
            record.redirect_target.as_deref().unwrap_or_default(),
        )?;
        worksheet.write_string(row, 24, record.title.as_deref().unwrap_or_default())?;
        worksheet.write_number(row, 25, record.title_len as f64)?;
        worksheet.write_string(
            row,
            26,
            record.meta_description.as_deref().unwrap_or_default(),
        )?;
        worksheet.write_number(row, 27, record.meta_description_len as f64)?;
        worksheet.write_string(row, 28, record.meta_robots.as_deref().unwrap_or_default())?;
        worksheet.write_string(row, 29, record.x_robots_tag.as_deref().unwrap_or_default())?;
        worksheet.write_string(row, 30, record.h1.as_deref().unwrap_or_default())?;
        worksheet.write_number(row, 31, record.h1_len as f64)?;
        worksheet.write_number(row, 32, record.h1_count as f64)?;
        worksheet.write_string(row, 33, record.h2.as_deref().unwrap_or_default())?;
        worksheet.write_number(row, 34, record.h2_len as f64)?;
        worksheet.write_number(row, 35, record.h2_count as f64)?;
        worksheet.write_string(row, 36, record.canonical.as_deref().unwrap_or_default())?;
        worksheet.write_number(row, 37, record.canonical_count as f64)?;
        worksheet.write_number(row, 38, f64::from(record.image_count))?;
        worksheet.write_number(row, 39, f64::from(record.images_missing_alt))?;
        worksheet.write_number(row, 40, f64::from(record.images_alt_too_long))?;
        worksheet.write_number(row, 41, f64::from(record.mixed_content_count))?;
        worksheet.write_number(row, 42, f64::from(record.insecure_form_count))?;
        worksheet.write_string(row, 43, record.hsts_header.to_string())?;
        worksheet.write_string(row, 44, record.content_security_policy_header.to_string())?;
        worksheet.write_string(row, 45, record.x_frame_options_header.to_string())?;
        worksheet.write_string(row, 46, record.x_content_type_options_header.to_string())?;
        worksheet.write_string(row, 47, record.viewport.to_string())?;
        worksheet.write_string(row, 48, record.amphtml.as_deref().unwrap_or_default())?;
        worksheet.write_string(row, 49, record.rel_next.as_deref().unwrap_or_default())?;
        worksheet.write_string(row, 50, record.rel_prev.as_deref().unwrap_or_default())?;
        worksheet.write_number(row, 51, f64::from(record.hreflang_count))?;
        worksheet.write_number(row, 52, f64::from(record.hreflang_invalid_count))?;
        worksheet.write_string(row, 53, record.hreflang_missing_self_reference.to_string())?;
        worksheet.write_number(row, 54, f64::from(record.json_ld_count))?;
        worksheet.write_number(row, 55, f64::from(record.json_ld_invalid_count))?;
        worksheet.write_number(row, 56, f64::from(record.open_graph_count))?;
        worksheet.write_number(row, 57, f64::from(record.twitter_card_count))?;
        worksheet.write_number(row, 58, f64::from(record.inlink_count))?;
        worksheet.write_number(row, 59, f64::from(record.outlink_count))?;
        worksheet.write_number(row, 60, f64::from(record.internal_outlink_count))?;
        worksheet.write_number(row, 61, f64::from(record.external_outlink_count))?;
        worksheet.write_string(
            row,
            62,
            serde_json::to_string(&record.custom_extractions).unwrap_or_default(),
        )?;
        worksheet.write_string(row, 63, record.error.as_deref().unwrap_or_default())?;
        worksheet.write_boolean(row, 64, record.in_sitemap)?;
    }

    workbook.save_to_buffer()
}

pub fn records_to_sitemap_xml(records: &[CrawlRecord]) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
"#,
    );

    for record in records.iter().filter(|record| sitemap_eligible(record)) {
        xml.push_str("  <url>\n");
        xml.push_str("    <loc>");
        xml.push_str(&escape_xml(&record.final_url));
        xml.push_str("</loc>\n");
        xml.push_str("  </url>\n");
    }

    xml.push_str("</urlset>\n");
    xml
}

pub fn records_to_html_report(
    records: &[CrawlRecord],
    edges: &[LinkEdge],
) -> Result<String, minijinja::Error> {
    let report = build_html_report(records, edges);
    let mut env = Environment::new();
    env.add_template("seo_report.html", HTML_REPORT_TEMPLATE)?;
    env.get_template("seo_report.html")?
        .render(context! { report => report })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HtmlReport {
    title: String,
    subtitle: String,
    row_limit: usize,
    kpis: Vec<ReportKpi>,
    facts: Vec<ReportFact>,
    routing: Vec<RouteNote>,
    sections: Vec<ReportSection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportKpi {
    label: String,
    value: String,
    description: String,
    tone: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportFact {
    label: String,
    value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RouteNote {
    team: String,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportSection {
    title: String,
    team: String,
    severity: String,
    summary: String,
    count: usize,
    count_label: String,
    columns: Vec<String>,
    rows: Vec<ReportRow>,
    has_rows: bool,
    empty_message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportRow {
    cells: Vec<String>,
}

fn build_html_report(records: &[CrawlRecord], edges: &[LinkEdge]) -> HtmlReport {
    let summary = summarize(records);
    let broken_url_count = records
        .iter()
        .filter(|record| broken_record(record))
        .count();
    let broken_edge_count = edges.iter().filter(|edge| broken_edge(edge)).count();
    let image_issue_count = records
        .iter()
        .filter(|record| {
            record.images_missing_alt > 0
                || record.images_alt_too_long > 0
                || large_image_record(record)
        })
        .count();
    let security_issue_count = records
        .iter()
        .filter(|record| has_security_issue(record))
        .count();

    HtmlReport {
        title: "Technical SEO Report".to_string(),
        subtitle: "Shareable audit summary for engineering, content, SEO, performance, and security teams.".to_string(),
        row_limit: HTML_REPORT_ROW_LIMIT,
        kpis: vec![
            kpi("URLs crawled", summary.total, "Total stored URL records", "default"),
            kpi("Broken URLs", broken_url_count, "No response, 4xx, 5xx, or fetch error", "danger"),
            kpi("Broken link edges", broken_edge_count, "Source pages linking to known broken targets", "danger"),
            kpi("Non-indexable", summary.non_indexable, "Pages currently marked non-indexable", "warning"),
            kpi("Image issues", image_issue_count, "Missing alt, long alt, or large image assets where crawled", "warning"),
            kpi("Security issues", security_issue_count, "Mixed content, insecure forms, or missing headers", "danger"),
        ],
        facts: vec![
            fact("2xx success", summary.success),
            fact("3xx redirects", summary.redirects),
            fact("4xx client errors", summary.client_errors),
            fact("5xx server errors", summary.server_errors),
            fact("No response", summary.no_response),
        ],
        routing: vec![
            route("Engineering", "Broken URLs, redirects, canonical chains, crawlability, rendering, and templates."),
            route("Content", "Missing or duplicate titles, descriptions, headings, and image alt text."),
            route("Performance", "Large pages, slow TTFB, slow responses, and large image assets where crawled."),
            route("Security", "Mixed content, insecure forms, HSTS, CSP, X-Frame-Options, and content-type headers."),
        ],
        sections: vec![
            broken_url_section(records),
            broken_link_section(edges),
            metadata_section(records),
            headings_and_canonicals_section(records),
            image_section(records),
            performance_section(records),
            security_section(records),
            structured_data_section(records),
        ],
    }
}

fn kpi(label: &str, value: usize, description: &str, tone: &str) -> ReportKpi {
    ReportKpi {
        label: label.to_string(),
        value: value.to_string(),
        description: description.to_string(),
        tone: tone.to_string(),
    }
}

fn fact(label: &str, value: usize) -> ReportFact {
    ReportFact {
        label: label.to_string(),
        value: value.to_string(),
    }
}

fn route(team: &str, text: &str) -> RouteNote {
    RouteNote {
        team: team.to_string(),
        text: text.to_string(),
    }
}

fn make_section(
    title: &str,
    team: &str,
    severity: &str,
    summary: &str,
    columns: &[&str],
    rows: Vec<ReportRow>,
    total_count: usize,
    empty_message: &str,
) -> ReportSection {
    let has_rows = !rows.is_empty();
    ReportSection {
        title: title.to_string(),
        team: team.to_string(),
        severity: severity.to_string(),
        summary: summary.to_string(),
        count: total_count,
        count_label: if total_count == 1 {
            "issue".to_string()
        } else {
            "issues".to_string()
        },
        columns: columns.iter().map(|column| (*column).to_string()).collect(),
        rows,
        has_rows,
        empty_message: empty_message.to_string(),
    }
}

fn row(cells: Vec<String>) -> ReportRow {
    ReportRow { cells }
}

fn broken_url_section(records: &[CrawlRecord]) -> ReportSection {
    let matching = records.iter().filter(|record| broken_record(record));
    let total_count = matching.clone().count();
    let rows = matching
        .take(HTML_REPORT_ROW_LIMIT)
        .map(|record| {
            row(vec![
                safe_text(&record.final_url),
                safe_text(&status_label(record)),
                safe_text(record.error.as_deref().unwrap_or("")),
                "Restore a 2xx response, redirect intentionally, or remove incoming links."
                    .to_string(),
            ])
        })
        .collect();

    make_section(
        "Broken URLs",
        "Engineering",
        "High",
        "URLs that failed, returned no response, or returned 4xx/5xx status codes.",
        &["URL", "Status", "Error", "Recommended action"],
        rows,
        total_count,
        "No broken URL records were found.",
    )
}

fn broken_link_section(edges: &[LinkEdge]) -> ReportSection {
    let matching = edges.iter().filter(|edge| broken_edge(edge));
    let total_count = matching.clone().count();
    let rows = matching
        .take(HTML_REPORT_ROW_LIMIT)
        .map(|edge| {
            row(vec![
                safe_text(&edge.source_url),
                safe_text(&edge.target_url),
                safe_text(&edge.anchor_text),
                safe_text(&edge_status_label(edge)),
                "Update, redirect, or remove this link from the source page.".to_string(),
            ])
        })
        .collect();

    make_section(
        "Broken Links",
        "Engineering",
        "High",
        "Source pages that link to known broken or unreachable targets.",
        &[
            "Source",
            "Target",
            "Anchor",
            "Target status",
            "Recommended action",
        ],
        rows,
        total_count,
        "No broken link edges were found.",
    )
}

fn metadata_section(records: &[CrawlRecord]) -> ReportSection {
    let title_counts = text_counts(records.iter().filter_map(|record| record.title.as_deref()));
    let meta_counts = text_counts(
        records
            .iter()
            .filter_map(|record| record.meta_description.as_deref()),
    );
    let mut rows = Vec::new();
    let mut total_count = 0;

    for record in records {
        let issues = metadata_issues(record, &title_counts, &meta_counts);
        if issues.is_empty() {
            continue;
        }
        total_count += 1;
        if rows.len() < HTML_REPORT_ROW_LIMIT {
            rows.push(row(vec![
                safe_text(&record.final_url),
                safe_text(&issues.join("; ")),
                safe_text(record.title.as_deref().unwrap_or("")),
                safe_text(record.meta_description.as_deref().unwrap_or("")),
                "Rewrite unique titles and descriptions for search result snippets.".to_string(),
            ]));
        }
    }

    make_section(
        "Titles And Meta Descriptions",
        "Content",
        "Medium",
        "Missing, duplicate, short, or long search snippet metadata.",
        &[
            "URL",
            "Issue",
            "Title",
            "Meta description",
            "Recommended action",
        ],
        rows,
        total_count,
        "No title or meta description issues were found.",
    )
}

fn headings_and_canonicals_section(records: &[CrawlRecord]) -> ReportSection {
    let h1_counts = text_counts(records.iter().filter_map(|record| record.h1.as_deref()));
    let mut rows = Vec::new();
    let mut total_count = 0;

    for record in records {
        let issues = heading_canonical_issues(record, &h1_counts);
        if issues.is_empty() {
            continue;
        }
        total_count += 1;
        if rows.len() < HTML_REPORT_ROW_LIMIT {
            rows.push(row(vec![
                safe_text(&record.final_url),
                safe_text(&issues.join("; ")),
                safe_text(record.h1.as_deref().unwrap_or("")),
                safe_text(record.canonical.as_deref().unwrap_or("")),
                "Fix templates so each indexable page has one clear H1 and one intended canonical."
                    .to_string(),
            ]));
        }
    }

    make_section(
        "Headings And Canonicals",
        "Engineering",
        "Medium",
        "Heading structure and canonical tag problems that affect page understanding and consolidation.",
        &["URL", "Issue", "H1", "Canonical", "Recommended action"],
        rows,
        total_count,
        "No heading or canonical issues were found.",
    )
}

fn image_section(records: &[CrawlRecord]) -> ReportSection {
    let mut rows = Vec::new();
    let mut total_count = 0;

    for record in records {
        let issues = image_issues(record);
        if issues.is_empty() {
            continue;
        }
        total_count += 1;
        if rows.len() < HTML_REPORT_ROW_LIMIT {
            rows.push(row(vec![
                safe_text(&record.final_url),
                safe_text(&issues.join("; ")),
                record.image_count.to_string(),
                record.images_missing_alt.to_string(),
                format_bytes(record.size_bytes),
                "Add useful alt text and compress oversized image assets where asset URLs were crawled.".to_string(),
            ]));
        }
    }

    make_section(
        "Image Issues",
        "Content / Performance",
        "Medium",
        "Pages with missing image alt text, long alt text, or large image asset responses where image URLs were crawled.",
        &[
            "URL",
            "Issue",
            "Images",
            "Missing alt",
            "Response size",
            "Recommended action",
        ],
        rows,
        total_count,
        "No image issues were found in the captured data.",
    )
}

fn performance_section(records: &[CrawlRecord]) -> ReportSection {
    let mut rows = Vec::new();
    let mut total_count = 0;

    for record in records {
        let issues = performance_issues(record);
        if issues.is_empty() {
            continue;
        }
        total_count += 1;
        if rows.len() < HTML_REPORT_ROW_LIMIT {
            rows.push(row(vec![
                safe_text(&record.final_url),
                safe_text(&issues.join("; ")),
                format_bytes(record.size_bytes),
                record
                    .ttfb_ms
                    .map(format_ms)
                    .unwrap_or_else(|| "Unknown".to_string()),
                format_ms(record.response_time_ms),
                "Reduce payload size, optimize server latency, and review render-blocking resources.".to_string(),
            ]));
        }
    }

    make_section(
        "Performance Signals",
        "Performance",
        "Medium",
        "Large responses, slow TTFB, and slow total response times.",
        &[
            "URL",
            "Issue",
            "Size",
            "TTFB",
            "Response",
            "Recommended action",
        ],
        rows,
        total_count,
        "No performance threshold issues were found.",
    )
}

fn security_section(records: &[CrawlRecord]) -> ReportSection {
    let mut rows = Vec::new();
    let mut total_count = 0;

    for record in records {
        let issues = security_issues(record);
        if issues.is_empty() {
            continue;
        }
        total_count += 1;
        if rows.len() < HTML_REPORT_ROW_LIMIT {
            rows.push(row(vec![
                safe_text(&record.final_url),
                safe_text(&issues.join("; ")),
                status_label(record),
                safe_text(record.content_type.as_deref().unwrap_or("")),
                "Fix protocol usage, form actions, and missing response security headers."
                    .to_string(),
            ]));
        }
    }

    make_section(
        "Security And Protocol",
        "Security / Engineering",
        "High",
        "Mixed content, insecure forms, and missing baseline security headers.",
        &[
            "URL",
            "Issue",
            "Status",
            "Content type",
            "Recommended action",
        ],
        rows,
        total_count,
        "No security issues were found in the captured data.",
    )
}

fn structured_data_section(records: &[CrawlRecord]) -> ReportSection {
    let mut rows = Vec::new();
    let mut total_count = 0;

    for record in records {
        let issues = structured_data_issues(record);
        if issues.is_empty() {
            continue;
        }
        total_count += 1;
        if rows.len() < HTML_REPORT_ROW_LIMIT {
            rows.push(row(vec![
                safe_text(&record.final_url),
                safe_text(&issues.join("; ")),
                record.json_ld_count.to_string(),
                record.hreflang_count.to_string(),
                "Validate JSON-LD and hreflang annotations before release.".to_string(),
            ]));
        }
    }

    make_section(
        "Structured Data And Hreflang",
        "SEO / Engineering",
        "Medium",
        "Invalid JSON-LD and hreflang signals that can block rich-result or localization workflows.",
        &[
            "URL",
            "Issue",
            "JSON-LD blocks",
            "Hreflang tags",
            "Recommended action",
        ],
        rows,
        total_count,
        "No structured data or hreflang issues were found.",
    )
}

fn broken_record(record: &CrawlRecord) -> bool {
    record.error.is_some()
        || record.status_code.is_none()
        || matches!(record.status_code, Some(code) if code >= 400)
}

fn broken_edge(edge: &LinkEdge) -> bool {
    edge.target_status_code.is_none()
        || matches!(edge.target_status_code, Some(code) if code >= 400)
}

fn metadata_issues(
    record: &CrawlRecord,
    title_counts: &HashMap<String, usize>,
    meta_counts: &HashMap<String, usize>,
) -> Vec<String> {
    let mut issues = Vec::new();
    let title = record.title.as_deref().unwrap_or("").trim();
    let meta = record.meta_description.as_deref().unwrap_or("").trim();

    if title.is_empty() {
        issues.push("Missing title".to_string());
    } else {
        if record.title_len < TITLE_MIN_LENGTH {
            issues.push(format!("Title under {TITLE_MIN_LENGTH} chars"));
        }
        if record.title_len > TITLE_MAX_LENGTH {
            issues.push(format!("Title over {TITLE_MAX_LENGTH} chars"));
        }
        if title_counts
            .get(&normalize_text_key(title))
            .copied()
            .unwrap_or(0)
            > 1
        {
            issues.push("Duplicate title".to_string());
        }
    }

    if meta.is_empty() {
        issues.push("Missing meta description".to_string());
    } else {
        if record.meta_description_len < META_MIN_LENGTH {
            issues.push(format!("Meta description under {META_MIN_LENGTH} chars"));
        }
        if record.meta_description_len > META_MAX_LENGTH {
            issues.push(format!("Meta description over {META_MAX_LENGTH} chars"));
        }
        if meta_counts
            .get(&normalize_text_key(meta))
            .copied()
            .unwrap_or(0)
            > 1
        {
            issues.push("Duplicate meta description".to_string());
        }
    }

    if !title.is_empty()
        && record.h1.as_deref().map(normalize_text_key) == Some(normalize_text_key(title))
    {
        issues.push("Title same as H1".to_string());
    }

    issues
}

fn heading_canonical_issues(
    record: &CrawlRecord,
    h1_counts: &HashMap<String, usize>,
) -> Vec<String> {
    let mut issues = Vec::new();
    let h1 = record.h1.as_deref().unwrap_or("").trim();

    if h1.is_empty() {
        issues.push("Missing H1".to_string());
    } else {
        if record.h1_count > 1 {
            issues.push("Multiple H1 tags".to_string());
        }
        if record.h1_len > H1_MAX_LENGTH {
            issues.push(format!("H1 over {H1_MAX_LENGTH} chars"));
        }
        if h1_counts.get(&normalize_text_key(h1)).copied().unwrap_or(0) > 1 {
            issues.push("Duplicate H1".to_string());
        }
    }

    if record.canonical.as_deref().unwrap_or("").trim().is_empty() {
        issues.push("Missing canonical".to_string());
    }
    if record.canonical_count > 1 {
        issues.push("Multiple canonical tags".to_string());
    }
    if record.indexability == "Indexable"
        && record
            .canonical
            .as_deref()
            .map(|canonical| canonical != record.final_url)
            .unwrap_or(false)
    {
        issues.push("Canonical points away from indexable URL".to_string());
    }

    issues
}

fn image_issues(record: &CrawlRecord) -> Vec<String> {
    let mut issues = Vec::new();
    if record.images_missing_alt > 0 {
        issues.push(format!("{} images missing alt", record.images_missing_alt));
    }
    if record.images_alt_too_long > 0 {
        issues.push(format!(
            "{} image alt values too long",
            record.images_alt_too_long
        ));
    }
    if large_image_record(record) {
        issues.push(format!(
            "Image asset response over {}",
            format_bytes(LARGE_IMAGE_BYTES)
        ));
    }
    issues
}

fn performance_issues(record: &CrawlRecord) -> Vec<String> {
    let mut issues = Vec::new();
    if record.size_bytes > LARGE_PAGE_BYTES {
        issues.push(format!("Response over {}", format_bytes(LARGE_PAGE_BYTES)));
    }
    if record.ttfb_ms.unwrap_or(0) > SLOW_TTFB_MS {
        issues.push(format!("TTFB over {}", format_ms(SLOW_TTFB_MS)));
    }
    if record.response_time_ms > SLOW_RESPONSE_MS {
        issues.push(format!("Response over {}", format_ms(SLOW_RESPONSE_MS)));
    }
    issues
}

fn has_security_issue(record: &CrawlRecord) -> bool {
    !security_issues(record).is_empty()
}

fn security_issues(record: &CrawlRecord) -> Vec<String> {
    let mut issues = Vec::new();
    let is_success = matches!(record.status_code, Some(code) if (200..300).contains(&code));
    let is_html = record
        .content_type
        .as_deref()
        .map(|value| value.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false);

    if record.final_url.starts_with("http://") {
        issues.push("HTTP URL".to_string());
    }
    if record.mixed_content_count > 0 {
        issues.push(format!(
            "{} mixed-content references",
            record.mixed_content_count
        ));
    }
    if record.insecure_form_count > 0 {
        issues.push(format!("{} insecure forms", record.insecure_form_count));
    }
    if is_success && record.final_url.starts_with("https://") && !record.hsts_header {
        issues.push("Missing HSTS".to_string());
    }
    if is_success && is_html && !record.content_security_policy_header {
        issues.push("Missing CSP".to_string());
    }
    if is_success && is_html && !record.x_frame_options_header {
        issues.push("Missing X-Frame-Options".to_string());
    }
    if is_success && !record.x_content_type_options_header {
        issues.push("Missing X-Content-Type-Options".to_string());
    }
    if is_success && is_html && !record.viewport {
        issues.push("Missing viewport".to_string());
    }

    issues
}

fn structured_data_issues(record: &CrawlRecord) -> Vec<String> {
    let mut issues = Vec::new();
    if record.json_ld_invalid_count > 0 {
        issues.push(format!(
            "{} invalid JSON-LD blocks",
            record.json_ld_invalid_count
        ));
    }
    if record.hreflang_invalid_count > 0 {
        issues.push(format!(
            "{} invalid hreflang tags",
            record.hreflang_invalid_count
        ));
    }
    if record.hreflang_missing_self_reference {
        issues.push("Missing hreflang self-reference".to_string());
    }
    issues
}

fn large_image_record(record: &CrawlRecord) -> bool {
    record
        .content_type
        .as_deref()
        .map(|value| value.to_ascii_lowercase().starts_with("image/"))
        .unwrap_or(false)
        && record.size_bytes > LARGE_IMAGE_BYTES
}

fn text_counts<'a>(values: impl Iterator<Item = &'a str>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for value in values {
        let key = normalize_text_key(value);
        if key.is_empty() {
            continue;
        }
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

fn normalize_text_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

fn status_label(record: &CrawlRecord) -> String {
    match record.status_code {
        Some(code) if record.status_text.trim().is_empty() => code.to_string(),
        Some(code) => format!("{code} {}", record.status_text),
        None => "No response".to_string(),
    }
}

fn edge_status_label(edge: &LinkEdge) -> String {
    edge.target_status_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "Unknown or not crawled".to_string())
}

fn safe_text(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 240 {
        return normalized;
    }
    let mut shortened = normalized.chars().take(237).collect::<String>();
    shortened.push_str("...");
    shortened
}

fn format_ms(value: u64) -> String {
    format!("{value} ms")
}

fn format_bytes(value: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if value as f64 >= MB {
        format!("{:.1} MB", value as f64 / MB)
    } else if value as f64 >= KB {
        format!("{:.1} KB", value as f64 / KB)
    } else {
        format!("{value} B")
    }
}

fn sitemap_eligible(record: &CrawlRecord) -> bool {
    matches!(record.status_code, Some(code) if (200..300).contains(&code))
        && record.indexability == "Indexable"
        && record.error.is_none()
        && record.final_url.starts_with("http")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{
        CrawlRecord, CustomExtractionValue, LinkEdge, LinkType, RedirectHop,
    };

    #[test]
    fn writes_csv_headers_and_rows() {
        let mut record = CrawlRecord::pending("https://example.com/".to_string(), 0);
        record.status_code = Some(200);
        record.title = Some("Home".to_string());
        record.custom_extractions = vec![CustomExtractionValue {
            name: "heading".to_string(),
            values: vec!["Home".to_string()],
        }];

        let csv = records_to_csv_string(&[record]).unwrap();

        assert!(csv.contains("final_url"));
        assert!(csv.contains("https://example.com/"));
        assert!(csv.contains("heading"));
    }

    #[test]
    fn writes_xlsx_bytes() {
        let mut record = CrawlRecord::pending("https://example.com/".to_string(), 0);
        record.status_code = Some(200);
        record.title = Some("Home".to_string());

        let bytes = records_to_xlsx_bytes(&[record]).unwrap();

        assert!(bytes.starts_with(b"PK"));
    }

    #[test]
    fn writes_sitemap_xml_for_indexable_success_urls() {
        let mut ok = CrawlRecord::pending("https://example.com/a?x=1&y=2".to_string(), 0);
        ok.status_code = Some(200);
        ok.indexability = "Indexable".to_string();

        let mut broken = CrawlRecord::pending("https://example.com/b".to_string(), 0);
        broken.status_code = Some(404);

        let xml = records_to_sitemap_xml(&[ok, broken]);

        assert!(xml.contains("<urlset"));
        assert!(xml.contains("https://example.com/a?x=1&amp;y=2"));
        assert!(!xml.contains("https://example.com/b"));
    }

    #[test]
    fn writes_html_report_with_issue_sections() {
        let mut broken = CrawlRecord::pending("https://example.com/missing".to_string(), 1);
        broken.status_code = Some(404);
        broken.status_text = "Not Found".to_string();

        let mut page = CrawlRecord::pending("https://example.com/page".to_string(), 0);
        page.status_code = Some(200);
        page.status_text = "OK".to_string();
        page.content_type = Some("text/html".to_string());
        page.title = Some("Short".to_string());
        page.title_len = 5;
        page.meta_description = None;
        page.h1 = None;
        page.images_missing_alt = 2;
        page.response_time_ms = 3_500;
        page.ttfb_ms = Some(900);

        let edge = LinkEdge {
            id: 1,
            source_url: page.final_url.clone(),
            target_url: broken.final_url.clone(),
            anchor_text: "Missing page".to_string(),
            rel: String::new(),
            rel_nofollow: false,
            link_type: LinkType::Internal,
            source_status_code: Some(200),
            target_status_code: Some(404),
            source_depth: 0,
            target_depth: Some(1),
            source_position: 1,
            discovery_order: 1,
        };

        let html = records_to_html_report(&[page, broken], &[edge]).unwrap();

        assert!(html.contains("Technical SEO Report"));
        assert!(html.contains("Broken Links"));
        assert!(html.contains("Missing meta description"));
        assert!(html.contains("Toggle theme"));
    }

    #[test]
    fn writes_link_edge_csv() {
        let edge = LinkEdge {
            id: 7,
            source_url: "https://example.com/a".to_string(),
            target_url: "https://example.com/b".to_string(),
            anchor_text: "Read more".to_string(),
            rel: "nofollow".to_string(),
            rel_nofollow: true,
            link_type: LinkType::Internal,
            source_status_code: Some(200),
            target_status_code: Some(404),
            source_depth: 0,
            target_depth: Some(1),
            source_position: 3,
            discovery_order: 7,
        };

        let csv = link_edges_to_csv_string(&[edge]).unwrap();

        assert!(csv.contains("source_url"));
        assert!(csv.contains("source_position"));
        assert!(csv.contains("https://example.com/b"));
        assert!(csv.contains("Read more"));
    }

    #[test]
    fn writes_redirect_chain_csv() {
        let mut record = CrawlRecord::pending("https://example.com/old".to_string(), 0);
        record.id = 3;
        record.final_url = "https://example.com/new".to_string();
        record.redirect_chain = vec![RedirectHop {
            url: "https://example.com/old".to_string(),
            status_code: 301,
            location: Some("https://example.com/new".to_string()),
            dns_lookup_time_ms: Some(4),
            ttfb_ms: Some(70),
            elapsed_ms: Some(74),
        }];

        let csv = redirect_chains_to_csv_string(&[record]).unwrap();

        assert!(csv.contains("hop_url"));
        assert!(csv.contains("https://example.com/old"));
        assert!(csv.contains("301"));
    }
}
