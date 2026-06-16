use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UrlClassification {
    Internal,
    External,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IssueView {
    All,
    Internal,
    External,
    Status2xx,
    Status3xx,
    Status4xx,
    Status5xx,
    NoResponse,
    TitleMissing,
    TitleDuplicate,
    TitleTooShort,
    TitleTooLong,
    MetaMissing,
    MetaDuplicate,
    MetaTooShort,
    MetaTooLong,
    BrokenLinks,
}

impl Default for IssueView {
    fn default() -> Self {
        Self::All
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Asc,
    Desc,
}

impl Default for SortDirection {
    fn default() -> Self {
        Self::Asc
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedirectHop {
    pub url: String,
    pub status_code: u16,
    pub location: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlRecord {
    pub id: u64,
    pub url: String,
    pub final_url: String,
    pub classification: UrlClassification,
    pub status_code: Option<u16>,
    pub status_text: String,
    pub content_type: Option<String>,
    pub indexability: String,
    pub indexability_status: String,
    pub response_time_ms: u64,
    pub size_bytes: usize,
    pub depth: usize,
    pub redirect_target: Option<String>,
    pub redirect_type: Option<String>,
    pub redirect_chain: Vec<RedirectHop>,
    pub title: Option<String>,
    pub title_len: usize,
    pub meta_description: Option<String>,
    pub meta_description_len: usize,
    pub h1: Option<String>,
    pub h1_len: usize,
    pub canonical: Option<String>,
    pub inlink_count: u32,
    pub outlink_count: u32,
    pub internal_outlink_count: u32,
    pub external_outlink_count: u32,
    pub error: Option<String>,
}

impl CrawlRecord {
    pub fn pending(url: String, depth: usize) -> Self {
        Self {
            id: 0,
            final_url: url.clone(),
            url,
            classification: UrlClassification::Internal,
            status_code: None,
            status_text: "Pending".to_string(),
            content_type: None,
            indexability: "Unknown".to_string(),
            indexability_status: "Not fetched".to_string(),
            response_time_ms: 0,
            size_bytes: 0,
            depth,
            redirect_target: None,
            redirect_type: None,
            redirect_chain: Vec::new(),
            title: None,
            title_len: 0,
            meta_description: None,
            meta_description_len: 0,
            h1: None,
            h1_len: 0,
            canonical: None,
            inlink_count: 0,
            outlink_count: 0,
            internal_outlink_count: 0,
            external_outlink_count: 0,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub rule_id: String,
    pub view: IssueView,
    pub severity: Severity,
    pub url: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlSummary {
    pub total: usize,
    pub internal: usize,
    pub external: usize,
    pub success: usize,
    pub redirects: usize,
    pub client_errors: usize,
    pub server_errors: usize,
    pub no_response: usize,
    pub broken: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridQuery {
    pub offset: usize,
    pub limit: usize,
    pub global_search: Option<String>,
    pub sort_by: Option<String>,
    pub sort_dir: SortDirection,
    pub view: IssueView,
}

impl Default for GridQuery {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 200,
            global_search: None,
            sort_by: None,
            sort_dir: SortDirection::Asc,
            view: IssueView::All,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridResponse {
    pub rows: Vec<CrawlRecord>,
    pub total: usize,
    pub summary: CrawlSummary,
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    inner: Arc<RwLock<MemoryStoreInner>>,
}

#[derive(Default)]
struct MemoryStoreInner {
    records: Vec<CrawlRecord>,
    url_to_index: HashMap<String, usize>,
    inlink_counts: HashMap<String, u32>,
    next_id: u64,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        *inner = MemoryStoreInner::default();
    }

    pub fn upsert(&self, mut record: CrawlRecord) -> CrawlRecord {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        let key = record.final_url.clone();
        record.inlink_count = *inner
            .inlink_counts
            .get(&key)
            .unwrap_or(&record.inlink_count);

        if let Some(index) = inner.url_to_index.get(&key).copied() {
            record.id = inner.records[index].id;
            inner.records[index] = record.clone();
            return record;
        }

        inner.next_id += 1;
        record.id = inner.next_id;
        let index = inner.records.len();
        inner.url_to_index.insert(key, index);
        inner.records.push(record.clone());
        record
    }

    pub fn add_inlink(&self, target_url: &str) {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        let new_count = {
            let count = inner
                .inlink_counts
                .entry(target_url.to_string())
                .or_insert(0);
            *count = count.saturating_add(1);
            *count
        };

        if let Some(index) = inner.url_to_index.get(target_url).copied() {
            inner.records[index].inlink_count = new_count;
        }
    }

    pub fn records(&self) -> Vec<CrawlRecord> {
        self.inner
            .read()
            .expect("memory store lock poisoned")
            .records
            .clone()
    }

    pub fn summary(&self) -> CrawlSummary {
        let records = self.records();
        summarize(&records)
    }

    pub fn query(&self, query: GridQuery) -> GridResponse {
        let mut rows = self.records();
        let summary = summarize(&rows);
        let title_counts = duplicate_counts(rows.iter().filter_map(|row| row.title.as_deref()));
        let meta_counts = duplicate_counts(
            rows.iter()
                .filter_map(|row| row.meta_description.as_deref()),
        );

        rows.retain(|row| matches_view(row, &query.view, &title_counts, &meta_counts));

        if let Some(search) = query
            .global_search
            .as_ref()
            .map(|value| value.trim().to_lowercase())
        {
            if !search.is_empty() {
                rows.retain(|row| row_matches_search(row, &search));
            }
        }

        if let Some(sort_by) = query.sort_by.as_deref() {
            sort_rows(&mut rows, sort_by, &query.sort_dir);
        }

        let total = rows.len();
        let start = query.offset.min(total);
        let end = start.saturating_add(query.limit).min(total);
        let rows = rows[start..end].to_vec();

        GridResponse {
            rows,
            total,
            summary,
        }
    }
}

pub fn summarize(records: &[CrawlRecord]) -> CrawlSummary {
    let mut summary = CrawlSummary {
        total: records.len(),
        ..CrawlSummary::default()
    };

    for record in records {
        match record.classification {
            UrlClassification::Internal => summary.internal += 1,
            UrlClassification::External => summary.external += 1,
        }

        match record.status_code {
            Some(code) if (200..300).contains(&code) => summary.success += 1,
            Some(code) if (300..400).contains(&code) => summary.redirects += 1,
            Some(code) if (400..500).contains(&code) => {
                summary.client_errors += 1;
                summary.broken += 1;
            }
            Some(code) if code >= 500 => {
                summary.server_errors += 1;
                summary.broken += 1;
            }
            None => {
                summary.no_response += 1;
                if record.error.is_some() {
                    summary.broken += 1;
                }
            }
            _ => {}
        }
    }

    summary
}

fn duplicate_counts<'a>(values: impl Iterator<Item = &'a str>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for value in values {
        let normalized = normalize_text_key(value);
        if !normalized.is_empty() {
            *counts.entry(normalized).or_insert(0) += 1;
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

fn matches_view(
    row: &CrawlRecord,
    view: &IssueView,
    title_counts: &HashMap<String, usize>,
    meta_counts: &HashMap<String, usize>,
) -> bool {
    match view {
        IssueView::All => true,
        IssueView::Internal => row.classification == UrlClassification::Internal,
        IssueView::External => row.classification == UrlClassification::External,
        IssueView::Status2xx => matches!(row.status_code, Some(code) if (200..300).contains(&code)),
        IssueView::Status3xx => {
            !row.redirect_chain.is_empty()
                || matches!(row.status_code, Some(code) if (300..400).contains(&code))
        }
        IssueView::Status4xx => matches!(row.status_code, Some(code) if (400..500).contains(&code)),
        IssueView::Status5xx => matches!(row.status_code, Some(code) if code >= 500),
        IssueView::NoResponse => row.status_code.is_none(),
        IssueView::TitleMissing => row.title.as_deref().unwrap_or("").trim().is_empty(),
        IssueView::TitleDuplicate => {
            row.title
                .as_deref()
                .map(normalize_text_key)
                .and_then(|key| title_counts.get(&key).copied())
                .unwrap_or(0)
                > 1
        }
        IssueView::TitleTooShort => {
            let title = row.title.as_deref().unwrap_or("").trim();
            !title.is_empty() && row.title_len < 30
        }
        IssueView::TitleTooLong => row.title_len > 60,
        IssueView::MetaMissing => row
            .meta_description
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty(),
        IssueView::MetaDuplicate => {
            row.meta_description
                .as_deref()
                .map(normalize_text_key)
                .and_then(|key| meta_counts.get(&key).copied())
                .unwrap_or(0)
                > 1
        }
        IssueView::MetaTooShort => {
            let meta = row.meta_description.as_deref().unwrap_or("").trim();
            !meta.is_empty() && row.meta_description_len < 70
        }
        IssueView::MetaTooLong => row.meta_description_len > 160,
        IssueView::BrokenLinks => {
            row.error.is_some()
                || matches!(row.status_code, Some(code) if code >= 400)
                || row.status_code.is_none()
        }
    }
}

fn row_matches_search(row: &CrawlRecord, search: &str) -> bool {
    row.url.to_lowercase().contains(search)
        || row.final_url.to_lowercase().contains(search)
        || row
            .title
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .meta_description
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .h1
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .status_code
            .map(|code| code.to_string().contains(search))
            .unwrap_or(false)
}

fn sort_rows(rows: &mut [CrawlRecord], sort_by: &str, sort_dir: &SortDirection) {
    rows.sort_by(|left, right| {
        let ordering = compare_rows(left, right, sort_by);
        match sort_dir {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
}

fn compare_rows(left: &CrawlRecord, right: &CrawlRecord, sort_by: &str) -> Ordering {
    match sort_by {
        "statusCode" => left.status_code.cmp(&right.status_code),
        "responseTimeMs" => left.response_time_ms.cmp(&right.response_time_ms),
        "sizeBytes" => left.size_bytes.cmp(&right.size_bytes),
        "depth" => left.depth.cmp(&right.depth),
        "titleLen" => left.title_len.cmp(&right.title_len),
        "metaDescriptionLen" => left.meta_description_len.cmp(&right.meta_description_len),
        "inlinkCount" => left.inlink_count.cmp(&right.inlink_count),
        "outlinkCount" => left.outlink_count.cmp(&right.outlink_count),
        "title" => left.title.cmp(&right.title),
        "finalUrl" => left.final_url.cmp(&right.final_url),
        _ => left.id.cmp(&right.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_filters_duplicate_titles() {
        let store = MemoryStore::new();
        for url in ["https://example.com/a", "https://example.com/b"] {
            let mut record = CrawlRecord::pending(url.to_string(), 0);
            record.status_code = Some(200);
            record.title = Some("Repeated title".to_string());
            record.title_len = 14;
            store.upsert(record);
        }

        let response = store.query(GridQuery {
            view: IssueView::TitleDuplicate,
            ..GridQuery::default()
        });

        assert_eq!(response.total, 2);
    }

    #[test]
    fn inlinks_update_existing_record() {
        let store = MemoryStore::new();
        let record = CrawlRecord::pending("https://example.com/a".to_string(), 0);
        store.upsert(record);
        store.add_inlink("https://example.com/a");

        let rows = store.records();
        assert_eq!(rows[0].inlink_count, 1);
    }
}
