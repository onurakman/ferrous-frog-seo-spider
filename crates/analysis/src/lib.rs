use ferrous_frog_storage::{CrawlRecord, Issue, IssueView, Severity};
use std::collections::HashMap;

const TITLE_MIN: usize = 30;
const TITLE_MAX: usize = 60;
const META_MIN: usize = 70;
const META_MAX: usize = 160;

pub fn analyze_records(records: &[CrawlRecord]) -> Vec<Issue> {
    let title_counts =
        duplicate_counts(records.iter().filter_map(|record| record.title.as_deref()));
    let meta_counts = duplicate_counts(
        records
            .iter()
            .filter_map(|record| record.meta_description.as_deref()),
    );
    let mut issues = Vec::new();

    for record in records {
        response_issues(record, &mut issues);
        title_issues(record, &title_counts, &mut issues);
        meta_issues(record, &meta_counts, &mut issues);
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
        first.title = Some("Repeated title for testing".to_string());
        first.title_len = 26;
        first.meta_description = Some(
            "This description is long enough to avoid a missing issue in this unit test."
                .to_string(),
        );
        first.meta_description_len = 73;

        let mut second = CrawlRecord::pending("https://example.com/b".to_string(), 0);
        second.title = first.title.clone();
        second.title_len = first.title_len;
        second.meta_description = first.meta_description.clone();
        second.meta_description_len = first.meta_description_len;

        let issues = analyze_records(&[first, second]);

        assert!(
            issues
                .iter()
                .any(|issue| issue.rule_id == "title.duplicate")
        );
    }
}
