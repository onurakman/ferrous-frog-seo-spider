use ferrous_frog_storage::{CrawlRecord, Issue, IssueView, Severity};
use std::collections::HashMap;

const TITLE_MIN: usize = 30;
const TITLE_MAX: usize = 60;
const META_MIN: usize = 70;
const META_MAX: usize = 160;
const H1_MAX: usize = 70;
const H2_MAX: usize = 70;
const IMAGE_ALT_MAX: usize = 125;

pub fn analyze_records(records: &[CrawlRecord]) -> Vec<Issue> {
    let title_counts =
        duplicate_counts(records.iter().filter_map(|record| record.title.as_deref()));
    let meta_counts = duplicate_counts(
        records
            .iter()
            .filter_map(|record| record.meta_description.as_deref()),
    );
    let h1_counts = duplicate_counts(records.iter().filter_map(|record| record.h1.as_deref()));
    let h2_counts = duplicate_counts(records.iter().filter_map(|record| record.h2.as_deref()));
    let near_duplicate_counts = cluster_counts(
        records
            .iter()
            .filter_map(|record| record.near_duplicate_cluster_id),
    );
    let mut issues = Vec::new();

    for record in records {
        response_issues(record, &mut issues);
        title_issues(record, &title_counts, &mut issues);
        meta_issues(record, &meta_counts, &mut issues);
        h1_issues(record, &h1_counts, &mut issues);
        h2_issues(record, &h2_counts, &mut issues);
        canonical_issues(record, &mut issues);
        directive_issues(record, &mut issues);
        image_issues(record, &mut issues);
        security_issues(record, &mut issues);
        mobile_issues(record, &mut issues);
        hreflang_issues(record, &mut issues);
        structured_data_issues(record, &mut issues);
        near_duplicate_issues(record, &near_duplicate_counts, &mut issues);
        sitemap_issues(record, &mut issues);
    }

    issues
}

fn response_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    match record.status_code {
        Some(code) if (400..500).contains(&code) => issues.push(issue(
            "response.client_error",
            IssueView::BrokenLinks,
            Severity::Error,
            record,
            format!("Client error response: {code}"),
        )),
        Some(code) if code >= 500 => issues.push(issue(
            "response.server_error",
            IssueView::BrokenLinks,
            Severity::Error,
            record,
            format!("Server error response: {code}"),
        )),
        None if record.error.is_some() => issues.push(issue(
            "response.no_response",
            IssueView::NoResponse,
            Severity::Error,
            record,
            record
                .error
                .clone()
                .unwrap_or_else(|| "No response".to_string()),
        )),
        _ => {}
    }
}

fn title_issues(record: &CrawlRecord, counts: &HashMap<String, usize>, issues: &mut Vec<Issue>) {
    let title = record.title.as_deref().unwrap_or("").trim();
    if title.is_empty() {
        issues.push(issue(
            "title.missing",
            IssueView::TitleMissing,
            Severity::Warning,
            record,
            "Missing page title".to_string(),
        ));
        return;
    }

    let key = normalize_text_key(title);
    if counts.get(&key).copied().unwrap_or(0) > 1 {
        issues.push(issue(
            "title.duplicate",
            IssueView::TitleDuplicate,
            Severity::Warning,
            record,
            "Duplicate page title".to_string(),
        ));
    }

    if record.title_len < TITLE_MIN {
        issues.push(issue(
            "title.too_short",
            IssueView::TitleTooShort,
            Severity::Info,
            record,
            format!("Page title is shorter than {TITLE_MIN} characters"),
        ));
    }

    if record.title_len > TITLE_MAX {
        issues.push(issue(
            "title.too_long",
            IssueView::TitleTooLong,
            Severity::Warning,
            record,
            format!("Page title is longer than {TITLE_MAX} characters"),
        ));
    }

    if let Some(h1) = record.h1.as_deref() {
        if title.eq_ignore_ascii_case(h1.trim()) {
            issues.push(issue(
                "title.same_as_h1",
                IssueView::TitleSameAsH1,
                Severity::Info,
                record,
                "Page title is the same as H1".to_string(),
            ));
        }
    }
}

fn meta_issues(record: &CrawlRecord, counts: &HashMap<String, usize>, issues: &mut Vec<Issue>) {
    let meta = record.meta_description.as_deref().unwrap_or("").trim();
    if meta.is_empty() {
        issues.push(issue(
            "meta_description.missing",
            IssueView::MetaMissing,
            Severity::Warning,
            record,
            "Missing meta description".to_string(),
        ));
        return;
    }

    let key = normalize_text_key(meta);
    if counts.get(&key).copied().unwrap_or(0) > 1 {
        issues.push(issue(
            "meta_description.duplicate",
            IssueView::MetaDuplicate,
            Severity::Warning,
            record,
            "Duplicate meta description".to_string(),
        ));
    }

    if record.meta_description_len < META_MIN {
        issues.push(issue(
            "meta_description.too_short",
            IssueView::MetaTooShort,
            Severity::Info,
            record,
            format!("Meta description is shorter than {META_MIN} characters"),
        ));
    }

    if record.meta_description_len > META_MAX {
        issues.push(issue(
            "meta_description.too_long",
            IssueView::MetaTooLong,
            Severity::Warning,
            record,
            format!("Meta description is longer than {META_MAX} characters"),
        ));
    }
}

fn h1_issues(record: &CrawlRecord, counts: &HashMap<String, usize>, issues: &mut Vec<Issue>) {
    let h1 = record.h1.as_deref().unwrap_or("").trim();
    if h1.is_empty() {
        issues.push(issue(
            "h1.missing",
            IssueView::H1Missing,
            Severity::Warning,
            record,
            "Missing H1".to_string(),
        ));
        return;
    }

    let key = normalize_text_key(h1);
    if counts.get(&key).copied().unwrap_or(0) > 1 {
        issues.push(issue(
            "h1.duplicate",
            IssueView::H1Duplicate,
            Severity::Warning,
            record,
            "Duplicate H1".to_string(),
        ));
    }

    if record.h1_len > H1_MAX {
        issues.push(issue(
            "h1.too_long",
            IssueView::H1TooLong,
            Severity::Info,
            record,
            format!("H1 is longer than {H1_MAX} characters"),
        ));
    }
}

fn h2_issues(record: &CrawlRecord, counts: &HashMap<String, usize>, issues: &mut Vec<Issue>) {
    let h2 = record.h2.as_deref().unwrap_or("").trim();
    if h2.is_empty() {
        issues.push(issue(
            "h2.missing",
            IssueView::H2Missing,
            Severity::Info,
            record,
            "Missing H2".to_string(),
        ));
        return;
    }

    let key = normalize_text_key(h2);
    if counts.get(&key).copied().unwrap_or(0) > 1 {
        issues.push(issue(
            "h2.duplicate",
            IssueView::H2Duplicate,
            Severity::Info,
            record,
            "Duplicate H2".to_string(),
        ));
    }

    if record.h2_len > H2_MAX {
        issues.push(issue(
            "h2.too_long",
            IssueView::H2TooLong,
            Severity::Info,
            record,
            format!("H2 is longer than {H2_MAX} characters"),
        ));
    }
}

fn canonical_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record.canonical.as_deref().unwrap_or("").trim().is_empty() {
        issues.push(issue(
            "canonical.missing",
            IssueView::CanonicalMissing,
            Severity::Info,
            record,
            "Missing canonical URL".to_string(),
        ));
    }

    if record.canonical_count > 1 {
        issues.push(issue(
            "canonical.multiple",
            IssueView::CanonicalMultiple,
            Severity::Warning,
            record,
            "Multiple canonical link elements".to_string(),
        ));
    }
}

fn directive_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record
        .indexability_status
        .to_ascii_lowercase()
        .contains("noindex")
    {
        issues.push(issue(
            "directives.noindex",
            IssueView::DirectivesNoindex,
            Severity::Info,
            record,
            "URL is marked noindex".to_string(),
        ));
    }
}

fn image_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record.images_missing_alt > 0 {
        issues.push(issue(
            "images.missing_alt",
            IssueView::ImagesMissingAlt,
            Severity::Warning,
            record,
            format!("{} image(s) missing alt text", record.images_missing_alt),
        ));
    }

    if record.images_alt_too_long > 0 {
        issues.push(issue(
            "images.alt_too_long",
            IssueView::ImagesAltTooLong,
            Severity::Info,
            record,
            format!(
                "{} image alt attribute(s) longer than {IMAGE_ALT_MAX} characters",
                record.images_alt_too_long
            ),
        ));
    }
}

fn security_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record.mixed_content_count > 0 {
        issues.push(issue(
            "security.mixed_content",
            IssueView::SecurityMixedContent,
            Severity::Warning,
            record,
            format!("{} mixed content resource(s)", record.mixed_content_count),
        ));
    }

    if record.insecure_form_count > 0 {
        issues.push(issue(
            "security.insecure_forms",
            IssueView::SecurityInsecureForms,
            Severity::Warning,
            record,
            format!("{} insecure form(s)", record.insecure_form_count),
        ));
    }

    if is_success_record(record) && record.final_url.starts_with("https://") && !record.hsts_header
    {
        issues.push(issue(
            "security.missing_hsts",
            IssueView::SecurityMissingHsts,
            Severity::Warning,
            record,
            "HTTPS response is missing Strict-Transport-Security".to_string(),
        ));
    }

    if is_success_html_record(record) && !record.content_security_policy_header {
        issues.push(issue(
            "security.missing_csp",
            IssueView::SecurityMissingCsp,
            Severity::Info,
            record,
            "HTML response is missing Content-Security-Policy".to_string(),
        ));
    }

    if is_success_html_record(record) && !record.x_frame_options_header {
        issues.push(issue(
            "security.missing_x_frame_options",
            IssueView::SecurityMissingXFrameOptions,
            Severity::Info,
            record,
            "HTML response is missing X-Frame-Options".to_string(),
        ));
    }

    if is_success_record(record) && !record.x_content_type_options_header {
        issues.push(issue(
            "security.missing_x_content_type_options",
            IssueView::SecurityMissingContentTypeOptions,
            Severity::Info,
            record,
            "Response is missing X-Content-Type-Options".to_string(),
        ));
    }
}

fn mobile_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if is_success_html_record(record) && !record.viewport {
        issues.push(issue(
            "mobile.missing_viewport",
            IssueView::MobileMissingViewport,
            Severity::Warning,
            record,
            "HTML page is missing a viewport meta tag".to_string(),
        ));
    }
}

fn hreflang_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record.hreflang_invalid_count > 0 {
        issues.push(issue(
            "hreflang.invalid",
            IssueView::HreflangInvalid,
            Severity::Warning,
            record,
            format!(
                "Page has {} invalid hreflang value(s)",
                record.hreflang_invalid_count
            ),
        ));
    }

    if record.hreflang_missing_self_reference {
        issues.push(issue(
            "hreflang.missing_self_reference",
            IssueView::HreflangMissingSelfReference,
            Severity::Warning,
            record,
            "Page has hreflang alternates but no self-reference".to_string(),
        ));
    }
}

fn structured_data_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record.json_ld_invalid_count > 0 {
        issues.push(issue(
            "structured_data.invalid_json_ld",
            IssueView::StructuredDataInvalid,
            Severity::Warning,
            record,
            format!(
                "Page has {} invalid JSON-LD block(s)",
                record.json_ld_invalid_count
            ),
        ));
    }
}

fn is_success_record(record: &CrawlRecord) -> bool {
    matches!(record.status_code, Some(code) if (200..300).contains(&code))
}

fn is_success_html_record(record: &CrawlRecord) -> bool {
    is_success_record(record)
        && record
            .content_type
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains("text/html"))
            .unwrap_or(false)
}

fn sitemap_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record.in_sitemap
        && record.inlink_count == 0
        && record.classification == ferrous_frog_storage::UrlClassification::Internal
    {
        issues.push(issue(
            "sitemap.orphan",
            IssueView::SitemapOrphan,
            Severity::Warning,
            record,
            "URL is present in the sitemap but has no discovered in-links".to_string(),
        ));
    }
}

fn near_duplicate_issues(
    record: &CrawlRecord,
    counts: &HashMap<u64, usize>,
    issues: &mut Vec<Issue>,
) {
    let Some(cluster_id) = record.near_duplicate_cluster_id else {
        return;
    };

    if counts.get(&cluster_id).copied().unwrap_or(0) > 1 {
        issues.push(issue(
            "content.near_duplicate",
            IssueView::NearDuplicate,
            Severity::Info,
            record,
            format!("Near-duplicate content cluster {cluster_id}"),
        ));
    }
}

fn issue(
    rule_id: &str,
    view: IssueView,
    severity: Severity,
    record: &CrawlRecord,
    message: String,
) -> Issue {
    Issue {
        rule_id: rule_id.to_string(),
        view,
        severity,
        url: record.final_url.clone(),
        message,
    }
}

fn duplicate_counts<'a>(values: impl Iterator<Item = &'a str>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for value in values {
        let key = normalize_text_key(value);
        if !key.is_empty() {
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    counts
}

fn cluster_counts(values: impl Iterator<Item = u64>) -> HashMap<u64, usize> {
    let mut counts = HashMap::new();
    for value in values {
        *counts.entry(value).or_insert(0) += 1;
    }
    counts
}

fn normalize_text_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::CrawlRecord;

    #[test]
    fn emits_duplicate_title_issues() {
        let mut first = CrawlRecord::pending("https://example.com/a".to_string(), 0);
        first.status_code = Some(200);
        first.content_type = Some("text/html; charset=utf-8".to_string());
        first.title = Some("Repeated title for testing".to_string());
        first.title_len = 26;
        first.h1 = Some("Repeated heading".to_string());
        first.h1_len = 16;
        first.h2 = Some("Repeated subheading".to_string());
        first.h2_len = 19;
        first.canonical = Some("https://example.com/a".to_string());
        first.canonical_count = 2;
        first.images_missing_alt = 1;
        first.mixed_content_count = 1;
        first.hreflang_invalid_count = 1;
        first.hreflang_missing_self_reference = true;
        first.json_ld_invalid_count = 1;
        first.meta_description = Some(
            "This description is long enough to avoid a missing issue in this unit test."
                .to_string(),
        );
        first.meta_description_len = 73;

        let mut second = CrawlRecord::pending("https://example.com/b".to_string(), 0);
        second.status_code = Some(200);
        second.content_type = Some("text/html; charset=utf-8".to_string());
        second.title = first.title.clone();
        second.title_len = first.title_len;
        second.h1 = first.h1.clone();
        second.h1_len = first.h1_len;
        second.h2 = first.h2.clone();
        second.h2_len = first.h2_len;
        second.canonical = Some("https://example.com/b".to_string());
        second.meta_description = first.meta_description.clone();
        second.meta_description_len = first.meta_description_len;
        first.near_duplicate_cluster_id = Some(1);
        second.near_duplicate_cluster_id = Some(1);

        let issues = analyze_records(&[first, second]);

        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "title.duplicate")
        );
        assert!(issues.iter().any(|issue| issue.rule_id == "h1.duplicate"));
        assert!(issues.iter().any(|issue| issue.rule_id == "h2.duplicate"));
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "canonical.multiple")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "images.missing_alt")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "security.mixed_content")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "security.missing_hsts")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "mobile.missing_viewport")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "hreflang.invalid")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "structured_data.invalid_json_ld")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "content.near_duplicate")
        );
    }
}
