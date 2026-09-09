use ferrous_frog_storage::{
    CanonicalDiagnostics, CrawlRecord, Issue, IssueView, ReferenceDiagnostics, Severity,
    exact_duplicate_hashes, is_exact_duplicate_record, is_no_response_record,
    is_success_html_record, is_success_record, reference_diagnostics,
};
use std::collections::HashMap;

const TITLE_MIN: usize = 30;
const TITLE_MAX: usize = 60;
const META_MIN: usize = 70;
const META_MAX: usize = 160;
const H1_MAX: usize = 70;
const H2_MAX: usize = 70;
const IMAGE_ALT_MAX: usize = 125;

pub fn analyze_records(records: &[CrawlRecord]) -> Vec<Issue> {
    let html_records = records
        .iter()
        .filter(|record| is_success_html_record(record));
    let title_counts = duplicate_counts(
        html_records
            .clone()
            .filter_map(|record| record.title.as_deref()),
    );
    let meta_counts = duplicate_counts(
        html_records
            .clone()
            .filter_map(|record| record.meta_description.as_deref()),
    );
    let h1_counts = duplicate_counts(
        html_records
            .clone()
            .filter_map(|record| record.h1.as_deref()),
    );
    let h2_counts = duplicate_counts(
        html_records
            .clone()
            .filter_map(|record| record.h2.as_deref()),
    );
    let near_duplicate_counts =
        cluster_counts(html_records.filter_map(|record| record.near_duplicate_cluster_id));
    let mut issues = Vec::new();
    let references = reference_diagnostics(records);
    let exact_hashes = exact_duplicate_hashes(records);

    for (record, references) in records.iter().zip(references) {
        response_issues(record, &mut issues);
        directive_issues(record, &mut issues);
        security_issues(record, &mut issues);
        sitemap_issues(record, &mut issues);
        if !is_success_html_record(record) {
            continue;
        }
        title_issues(record, &title_counts, &mut issues);
        meta_issues(record, &meta_counts, &mut issues);
        h1_issues(record, &h1_counts, &mut issues);
        h2_issues(record, &h2_counts, &mut issues);
        canonical_issues(record, references.canonical, &mut issues);
        reference_target_issues(record, references, &mut issues);
        image_issues(record, &mut issues);
        mobile_issues(record, &mut issues);
        hreflang_issues(record, &mut issues);
        structured_data_issues(record, &mut issues);
        html_validation_issues(record, &mut issues);
        rendering_issues(record, &mut issues);
        near_duplicate_issues(record, &near_duplicate_counts, &mut issues);
        if is_exact_duplicate_record(record, &exact_hashes) {
            issues.push(issue(
                "content.exact_duplicate",
                IssueView::ExactDuplicate,
                Severity::Info,
                record,
                format!(
                    "Identical downloaded response body (hash {}) at multiple final URLs",
                    record.response_hash.as_deref().unwrap_or_default()
                ),
            ));
        }
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
        Some(300..=399) if record.error.is_some() => issues.push(issue(
            "response.redirect_error",
            IssueView::BrokenLinks,
            Severity::Error,
            record,
            record.error.clone().unwrap_or_default(),
        )),
        None if is_no_response_record(record) => issues.push(issue(
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
    if let Some(count) = record.title_count.filter(|count| *count > 1) {
        issues.push(issue(
            "title.multiple",
            IssueView::TitleMultiple,
            Severity::Warning,
            record,
            format!("Page contains {count} title elements"),
        ));
    }
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

    if let Some(h1) = record.h1.as_deref()
        && title.eq_ignore_ascii_case(h1.trim())
    {
        issues.push(issue(
            "title.same_as_h1",
            IssueView::TitleSameAsH1,
            Severity::Info,
            record,
            "Page title is the same as H1".to_string(),
        ));
    }
}

fn meta_issues(record: &CrawlRecord, counts: &HashMap<String, usize>, issues: &mut Vec<Issue>) {
    if let Some(count) = record.meta_description_count.filter(|count| *count > 1) {
        issues.push(issue(
            "meta_description.multiple",
            IssueView::MetaMultiple,
            Severity::Warning,
            record,
            format!("Page contains {count} meta description elements"),
        ));
    }
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

fn canonical_issues(
    record: &CrawlRecord,
    diagnostic: CanonicalDiagnostics,
    issues: &mut Vec<Issue>,
) {
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
    let target = record.canonical.as_deref().unwrap_or_default();
    for (matches, rule, view, severity, message) in [
        (
            diagnostic.uncrawled,
            "canonical.uncrawled",
            IssueView::CanonicalUncrawled,
            Severity::Info,
            "Canonical target has not been crawled",
        ),
        (
            diagnostic.to_redirect,
            "canonical.to_redirect",
            IssueView::CanonicalToRedirect,
            Severity::Warning,
            "Canonical target redirects",
        ),
        (
            diagnostic.to_error,
            "canonical.to_error",
            IssueView::CanonicalToError,
            Severity::Error,
            "Canonical target has a response or fetch error",
        ),
        (
            diagnostic.non_indexable,
            "canonical.non_indexable",
            IssueView::CanonicalNonIndexable,
            Severity::Warning,
            "Canonical target is non-indexable or blocked by robots.txt",
        ),
        (
            diagnostic.chain,
            "canonical.chain",
            IssueView::CanonicalChain,
            Severity::Warning,
            "Canonical target declares a further canonical",
        ),
        (
            diagnostic.loop_detected,
            "canonical.loop",
            IssueView::CanonicalLoop,
            Severity::Error,
            "Canonical path enters a loop",
        ),
    ] {
        if matches {
            issues.push(issue(
                rule,
                view,
                severity,
                record,
                format!("{message}: {target}"),
            ));
        }
    }
}

fn reference_target_issues(
    record: &CrawlRecord,
    diagnostic: ReferenceDiagnostics,
    issues: &mut Vec<Issue>,
) {
    for (matches, rule, label, view, target, severity) in [
        (
            diagnostic.pagination_next_to_error,
            "pagination.next_to_error",
            "Pagination next target has a response or fetch error",
            IssueView::PaginationNextToError,
            &record.rel_next,
            Severity::Error,
        ),
        (
            diagnostic.pagination_prev_to_error,
            "pagination.prev_to_error",
            "Pagination prev target has a response or fetch error",
            IssueView::PaginationPrevToError,
            &record.rel_prev,
            Severity::Error,
        ),
        (
            diagnostic.amp_to_error,
            "amp.to_error",
            "AMP target has a response or fetch error",
            IssueView::AmpToError,
            &record.amphtml,
            Severity::Error,
        ),
        (
            diagnostic.pagination_next_loop,
            "pagination.next_loop",
            "Next pagination path enters a loop",
            IssueView::PaginationNextLoop,
            &record.rel_next,
            Severity::Error,
        ),
        (
            diagnostic.pagination_prev_loop,
            "pagination.prev_loop",
            "Previous pagination path enters a loop",
            IssueView::PaginationPrevLoop,
            &record.rel_prev,
            Severity::Error,
        ),
        (
            diagnostic.pagination_next_non_reciprocal,
            "pagination.next_non_reciprocal",
            "Captured next target does not link back through its captured previous relation",
            IssueView::PaginationNextNonReciprocal,
            &record.rel_next,
            Severity::Warning,
        ),
        (
            diagnostic.pagination_prev_non_reciprocal,
            "pagination.prev_non_reciprocal",
            "Captured previous target does not link back through its captured next relation",
            IssueView::PaginationPrevNonReciprocal,
            &record.rel_prev,
            Severity::Warning,
        ),
    ] {
        if matches {
            issues.push(issue(
                rule,
                view,
                severity,
                record,
                format!("{label}: {}", target.as_deref().unwrap_or_default()),
            ));
        }
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
    if is_success_html_record(record) && record.mixed_content_count > 0 {
        issues.push(issue(
            "security.mixed_content",
            IssueView::SecurityMixedContent,
            Severity::Warning,
            record,
            format!("{} mixed content resource(s)", record.mixed_content_count),
        ));
    }

    if is_success_html_record(record) && record.insecure_form_count > 0 {
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
    if record.structured_data_error_count > 0 || record.json_ld_invalid_count > 0 {
        issues.push(issue(
            "structured_data.invalid_json_ld",
            IssueView::StructuredDataInvalid,
            Severity::Error,
            record,
            format!(
                "Page has {} structured data error(s)",
                record
                    .structured_data_error_count
                    .max(record.json_ld_invalid_count)
            ),
        ));
    }

    if record.structured_data_warning_count > 0 {
        issues.push(issue(
            "structured_data.warning",
            IssueView::StructuredDataWarning,
            Severity::Warning,
            record,
            format!(
                "Page has {} structured data warning(s)",
                record.structured_data_warning_count
            ),
        ));
    }
}

fn html_validation_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record.deprecated_html_tag_count > 0 {
        issues.push(issue(
            "html.deprecated_tags",
            IssueView::HtmlDeprecatedTags,
            Severity::Warning,
            record,
            format!(
                "{} deprecated HTML tag instance(s)",
                record.deprecated_html_tag_count
            ),
        ));
    }

    if record.duplicate_id_count > 0 {
        issues.push(issue(
            "html.duplicate_ids",
            IssueView::HtmlDuplicateIds,
            Severity::Warning,
            record,
            format!(
                "{} duplicate HTML id instance(s)",
                record.duplicate_id_count
            ),
        ));
    }
}

fn rendering_issues(record: &CrawlRecord, issues: &mut Vec<Issue>) {
    if record.rendered_dom_changed {
        issues.push(issue(
            "rendering.dom_changed",
            IssueView::RenderedDomChanged,
            Severity::Info,
            record,
            format!(
                "Rendered DOM changed the crawlable content by {} words and {} links",
                record.rendered_word_count_delta, record.rendered_link_count_delta
            ),
        ));
    }
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
    fn multiple_metadata_emits_warnings_even_when_retained_values_are_empty() {
        let mut record = CrawlRecord::pending("https://example.test/multiple".into(), 0);
        record.status_code = Some(200);
        record.content_type = Some("text/html".into());
        record.title_count = Some(2);
        record.meta_description_count = Some(2);
        let mut records = vec![record.clone()];
        record.indexability_status = "Response body incomplete".into();
        records.push(record);
        let issues = analyze_records(&records);
        let rules: Vec<_> = issues
            .iter()
            .filter(|issue| issue.rule_id.ends_with(".multiple"))
            .collect();
        assert_eq!(rules.len(), 2);
        for (issue, (rule, view)) in rules.iter().zip([
            ("title.multiple", IssueView::TitleMultiple),
            ("meta_description.multiple", IssueView::MetaMultiple),
        ]) {
            assert_eq!(issue.rule_id, rule);
            assert_eq!(issue.view, view);
            assert_eq!(issue.severity, Severity::Warning);
            assert!(issue.message.contains('2'));
        }
    }

    #[test]
    fn amp_emits_a_typed_error_with_the_declared_target() {
        let mut source = CrawlRecord::pending("https://example.test/source".into(), 0);
        source.status_code = Some(200);
        source.content_type = Some("text/html".into());
        source.amphtml = Some("https://example.test/amp#fragment".into());
        let mut target = CrawlRecord::pending("https://example.test/amp".into(), 0);
        target.status_code = Some(404);
        let mut incomplete = source.clone();
        incomplete.indexability_status = "Response body incomplete".into();
        let issues = analyze_records(&[source.clone(), target, incomplete]);
        let amp: Vec<_> = issues
            .iter()
            .filter(|issue| issue.rule_id == "amp.to_error")
            .collect();
        assert_eq!(amp.len(), 1);
        assert_eq!(format!("{:?}", amp[0].view), "AmpToError");
        assert_eq!(amp[0].severity, Severity::Error);
        assert_eq!(amp[0].url, source.url);
        assert!(amp[0].message.contains(source.amphtml.as_deref().unwrap()));
    }

    #[test]
    fn pagination_reciprocity_emits_advisory_directional_issues_for_captured_evidence() {
        let page = |path: &str, next: Option<&str>, prev: Option<&str>| {
            let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
            row.status_code = Some(200);
            row.content_type = Some("text/html".into());
            row.rel_next = next.map(|path| format!("https://example.test/{path}"));
            row.rel_prev = prev.map(|path| format!("https://example.test/{path}"));
            row
        };
        let mut incomplete = page("incomplete", Some("missing-return"), None);
        incomplete.indexability_status = "Response body incomplete".into();
        let rows = [
            page("next", Some("missing-return#section"), None),
            page("prev", None, Some("mismatched-return")),
            page("missing-return", None, None),
            page("mismatched-return", Some("other"), None),
            page("other", None, Some("mismatched-return")),
            page("unknown", Some("unknown-target"), None),
            incomplete,
        ];
        let issues = analyze_records(&rows);
        let reciprocity: Vec<_> = issues
            .iter()
            .filter(|issue| issue.rule_id.ends_with("_non_reciprocal"))
            .collect();
        assert_eq!(reciprocity.len(), 2);
        for (issue, row, direction, opposite, view) in [
            (
                reciprocity[0],
                &rows[0],
                "next",
                "previous",
                "PaginationNextNonReciprocal",
            ),
            (
                reciprocity[1],
                &rows[1],
                "prev",
                "next",
                "PaginationPrevNonReciprocal",
            ),
        ] {
            assert_eq!(
                issue.rule_id,
                format!("pagination.{direction}_non_reciprocal")
            );
            assert_eq!(format!("{:?}", issue.view), view);
            assert_eq!(issue.severity, Severity::Warning);
            assert_eq!(issue.url, row.url);
            assert!(
                issue
                    .message
                    .contains(&format!("captured {opposite} relation"))
            );
            let target = if direction == "next" {
                &row.rel_next
            } else {
                &row.rel_prev
            };
            assert!(issue.message.contains(target.as_deref().unwrap()));
        }
    }

    #[test]
    fn pagination_loops_emit_directional_errors_with_declared_targets() {
        let page = |path: &str, next: Option<&str>, prev: Option<&str>| {
            let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
            row.status_code = Some(200);
            row.content_type = Some("text/html".into());
            row.rel_next = next.map(|path| format!("https://example.test/{path}"));
            row.rel_prev = prev.map(|path| format!("https://example.test/{path}"));
            row
        };
        let mut incomplete = page("incomplete", Some("a"), Some("prev#section"));
        incomplete.indexability_status = "Response body incomplete".into();
        let rows = [
            page("a", Some("b#section"), None),
            page("b", Some("a"), None),
            page("enter", Some("a"), None),
            page("prev", None, Some("prev#section")),
            page("linear-a", Some("linear-b"), None),
            page("linear-b", None, Some("linear-a")),
            incomplete,
        ];
        let issues = analyze_records(&rows);
        let loops: Vec<_> = issues
            .iter()
            .filter(|issue| {
                issue.rule_id.starts_with("pagination.") && issue.rule_id.ends_with("_loop")
            })
            .collect();
        assert_eq!(loops.len(), 4);
        for (issue, row) in loops.iter().zip(&rows[..4]) {
            let (direction, view, target) = if row.rel_next.is_some() {
                (
                    "next",
                    "PaginationNextLoop",
                    row.rel_next.as_deref().unwrap(),
                )
            } else {
                (
                    "prev",
                    "PaginationPrevLoop",
                    row.rel_prev.as_deref().unwrap(),
                )
            };
            assert_eq!(issue.rule_id, format!("pagination.{direction}_loop"));
            assert_eq!(format!("{:?}", issue.view), view);
            assert_eq!(issue.severity, Severity::Error);
            assert_eq!(issue.url, row.url);
            assert!(issue.message.contains("path enters a loop"));
            assert!(issue.message.contains(target));
        }
    }

    #[test]
    fn pagination_emits_directional_typed_issues_only_for_known_failed_targets() {
        let mut source = CrawlRecord::pending("https://example.test/source".into(), 0);
        source.status_code = Some(200);
        source.content_type = Some("text/html".into());
        source.rel_next = Some("https://example.test/missing#next".into());
        source.rel_prev = Some("https://example.test/failed".into());
        let mut missing = CrawlRecord::pending("https://example.test/missing".into(), 0);
        missing.status_code = Some(404);
        let mut failed = CrawlRecord::pending("https://example.test/failed".into(), 0);
        failed.error = Some("Connection refused".into());
        let mut unknown = source.clone();
        unknown.url = "https://example.test/unknown-source".into();
        unknown.final_url = unknown.url.clone();
        unknown.rel_next = Some("https://example.test/unknown".into());
        unknown.rel_prev = Some("https://example.test/blocked".into());
        let mut blocked = CrawlRecord::pending("https://example.test/blocked".into(), 0);
        blocked.error = Some("Blocked by robots.txt".into());
        let mut incomplete = source.clone();
        incomplete.indexability_status = "Response body incomplete".into();
        let issues = analyze_records(&[
            source.clone(),
            missing,
            failed,
            unknown,
            blocked,
            incomplete,
        ]);
        let pagination: Vec<_> = issues
            .iter()
            .filter(|issue| issue.rule_id.starts_with("pagination."))
            .collect();
        assert_eq!(pagination.len(), 2);
        for (issue, direction, target) in [
            (pagination[0], "next", source.rel_next),
            (pagination[1], "prev", source.rel_prev),
        ] {
            assert_eq!(issue.rule_id, format!("pagination.{direction}_to_error"));
            assert_eq!(
                format!("{:?}", issue.view),
                if direction == "next" {
                    "PaginationNextToError"
                } else {
                    "PaginationPrevToError"
                }
            );
            assert_eq!(issue.severity, Severity::Error);
            assert_eq!(issue.url, source.url);
            assert!(issue.message.contains(target.as_deref().unwrap()));
        }
    }

    #[test]
    fn canonical_target_diagnostics_emit_typed_source_issues() {
        let page = |path: &str, target: &str| {
            let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
            record.status_code = Some(200);
            record.content_type = Some("text/html".into());
            record.canonical = Some(format!("https://example.test/{target}"));
            record
        };
        let mut error = page("missing", "missing");
        error.status_code = Some(404);
        let mut blocked = CrawlRecord::pending("https://example.test/blocked".into(), 0);
        blocked.error = Some("Blocked by robots.txt".into());
        let records = vec![
            page("unknown-source", "unseen"),
            page("error-source", "missing"),
            error,
            page("blocked-source", "blocked"),
            blocked,
            page("a", "b"),
            page("b", "a"),
            page("self", "self"),
        ];
        let issues = analyze_records(&records);
        for (rule, paths, severity) in [
            (
                "canonical.uncrawled",
                vec!["unknown-source"],
                Severity::Info,
            ),
            ("canonical.to_error", vec!["error-source"], Severity::Error),
            (
                "canonical.non_indexable",
                vec!["blocked-source"],
                Severity::Warning,
            ),
            ("canonical.chain", vec!["a", "b"], Severity::Warning),
            ("canonical.loop", vec!["a", "b"], Severity::Error),
        ] {
            let matching = issues
                .iter()
                .filter(|issue| issue.rule_id == rule)
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), paths.len(), "{rule}");
            for (issue, path) in matching.into_iter().zip(paths) {
                assert_eq!(issue.url, format!("https://example.test/{path}"));
                assert_eq!(issue.severity, severity);
                assert!(
                    issue.message.contains(
                        records
                            .iter()
                            .find(|record| record.final_url == issue.url)
                            .unwrap()
                            .canonical
                            .as_ref()
                            .unwrap()
                    )
                );
            }
        }
    }

    #[test]
    fn on_page_issues_require_successful_html() {
        let mut page = CrawlRecord::pending("https://example.com/page".to_string(), 0);
        page.status_code = Some(200);
        page.content_type = Some("text/html; charset=utf-8".to_string());
        let mut image = CrawlRecord::pending("https://example.com/image.png".to_string(), 0);
        image.status_code = Some(200);
        image.content_type = Some("image/png".to_string());
        let mut missing = CrawlRecord::pending("https://example.com/missing".to_string(), 0);
        missing.status_code = Some(404);
        missing.content_type = Some("text/html".to_string());

        let issues = analyze_records(&[page, image, missing]);
        for view in [
            IssueView::TitleMissing,
            IssueView::MetaMissing,
            IssueView::H1Missing,
            IssueView::H2Missing,
            IssueView::CanonicalMissing,
        ] {
            let matching = issues
                .iter()
                .filter(|issue| issue.view == view)
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1, "{view:?}");
            assert_eq!(matching[0].url, "https://example.com/page");
        }
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "response.client_error")
        );
        assert!(issues.iter().any(|issue| {
            issue.url.ends_with("image.png") && issue.rule_id == "security.missing_hsts"
        }));
    }

    #[test]
    fn failed_pages_do_not_create_duplicate_issues() {
        let mut page = CrawlRecord::pending("https://example.com/page".to_string(), 0);
        page.status_code = Some(200);
        page.content_type = Some("text/html".to_string());
        page.title = Some("A unique page title".to_string());
        page.meta_description = Some("A unique description".to_string());
        page.h1 = Some("A unique heading".to_string());
        page.h2 = Some("A unique subheading".to_string());
        page.near_duplicate_cluster_id = Some(7);
        let mut failed = page.clone();
        failed.final_url = "https://example.com/failed".to_string();
        failed.status_code = Some(500);

        let issues = analyze_records(&[page, failed]);
        for view in [
            IssueView::TitleDuplicate,
            IssueView::MetaDuplicate,
            IssueView::H1Duplicate,
            IssueView::H2Duplicate,
            IssueView::NearDuplicate,
        ] {
            assert!(!issues.iter().any(|issue| issue.view == view), "{view:?}");
        }
    }

    #[test]
    fn exact_body_duplicates_emit_typed_info_without_confusing_list_aliases() {
        let mut first = CrawlRecord::pending("https://example.test/a".into(), 0);
        first.status_code = Some(200);
        first.content_type = Some("text/html".into());
        first.response_hash = Some("identical-body-hash".into());
        let mut repeated = first.clone();
        repeated.final_url.push_str("#section");
        repeated.storage_key = "list:2:https://example.test/a".into();
        assert!(
            analyze_records(&[first.clone(), repeated.clone()])
                .iter()
                .all(|issue| issue.rule_id != "content.exact_duplicate")
        );

        let mut second = first.clone();
        second.url = "https://example.test/b".into();
        second.final_url = second.url.clone();
        second.indexability = "Non-indexable".into();
        second.word_count = 100;
        second.js_rendered = true;
        let mut failed = first.clone();
        failed.final_url = "https://example.test/failed".into();
        failed.status_code = Some(404);
        let mut incomplete = first.clone();
        incomplete.final_url = "https://example.test/incomplete".into();
        incomplete.indexability_status = "Response body incomplete".into();
        let issues = analyze_records(&[first, second, repeated, failed, incomplete]);
        let exact = issues
            .iter()
            .filter(|issue| issue.rule_id == "content.exact_duplicate")
            .collect::<Vec<_>>();
        assert_eq!(exact.len(), 3);
        for issue in exact {
            assert_eq!(issue.severity, Severity::Info);
            assert_eq!(issue.view, IssueView::ExactDuplicate);
            assert!(issue.message.contains("identical-body-hash"));
            assert!(issue.message.contains("response body"));
        }
    }

    #[test]
    fn response_issues_distinguish_robots_blocks_from_fetch_failures() {
        let mut blocked = CrawlRecord::pending("https://example.com/blocked".to_string(), 0);
        blocked.status_text = "Blocked by robots.txt".to_string();
        blocked.error = Some("Blocked by robots.txt".to_string());
        let pending = CrawlRecord::pending("https://example.com/pending".to_string(), 0);
        let mut failed = CrawlRecord::pending("https://example.com/failed".to_string(), 0);
        failed.error = Some("Connection refused".to_string());
        let mut redirect = CrawlRecord::pending("https://example.com/redirect".to_string(), 0);
        redirect.status_code = Some(302);
        redirect.error = Some("Redirect response missing Location header".to_string());

        let issues = analyze_records(&[blocked, pending, failed, redirect]);
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().any(|issue| {
            issue.view == IssueView::NoResponse && issue.url == "https://example.com/failed"
        }));
        assert!(issues.iter().any(|issue| {
            issue.view == IssueView::BrokenLinks && issue.url == "https://example.com/redirect"
        }));
    }

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
        first.structured_data_error_count = 1;
        first.structured_data_warning_count = 1;
        first.deprecated_html_tag_count = 2;
        first.duplicate_id_count = 1;
        first.js_rendered = true;
        first.rendered_dom_changed = true;
        first.rendered_word_count_delta = 10;
        first.rendered_link_count_delta = 2;
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
                .any(|issue| issue.rule_id == "structured_data.warning")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "html.deprecated_tags")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "html.duplicate_ids")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "rendering.dom_changed")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "content.near_duplicate")
        );
    }
}
