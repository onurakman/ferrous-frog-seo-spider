use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::sync::{Mutex, MutexGuard};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UrlClassification {
    Internal,
    External,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LinkType {
    Internal,
    External,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LinkEdgeView {
    #[default]
    All,
    Internal,
    External,
    Broken,
    Nofollow,
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
    H1Missing,
    H1Duplicate,
    H1TooLong,
    H2Missing,
    H2Duplicate,
    H2TooLong,
    TitleSameAsH1,
    CanonicalMissing,
    CanonicalMultiple,
    DirectivesNoindex,
    ImagesMissingAlt,
    ImagesAltTooLong,
    SecurityMixedContent,
    SecurityInsecureForms,
    SecurityMissingHsts,
    SecurityMissingCsp,
    SecurityMissingXFrameOptions,
    SecurityMissingContentTypeOptions,
    MobileMissingViewport,
    HreflangInvalid,
    HreflangMissingSelfReference,
    StructuredDataInvalid,
    NearDuplicate,
    BrokenLinks,
    SitemapOrphan,
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
    pub dns_lookup_time_ms: Option<u64>,
    pub ttfb_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomExtractionValue {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlRecord {
    pub id: u64,
    pub url: String,
    pub final_url: String,
    pub classification: UrlClassification,
    pub in_sitemap: bool,
    pub status_code: Option<u16>,
    pub status_text: String,
    pub content_type: Option<String>,
    pub indexability: String,
    pub indexability_status: String,
    pub response_time_ms: u64,
    pub dns_lookup_time_ms: Option<u64>,
    pub ttfb_ms: Option<u64>,
    pub download_time_ms: Option<u64>,
    pub total_network_time_ms: Option<u64>,
    pub transfer_rate_bytes_per_sec: Option<u64>,
    pub resolved_ip_count: u32,
    pub size_bytes: usize,
    pub response_hash: Option<String>,
    pub depth: usize,
    pub redirect_target: Option<String>,
    pub redirect_type: Option<String>,
    pub redirect_chain: Vec<RedirectHop>,
    pub title: Option<String>,
    pub title_len: usize,
    pub meta_description: Option<String>,
    pub meta_description_len: usize,
    pub meta_robots: Option<String>,
    pub x_robots_tag: Option<String>,
    pub h1: Option<String>,
    pub h1_len: usize,
    pub h1_count: usize,
    pub h2: Option<String>,
    pub h2_len: usize,
    pub h2_count: usize,
    pub canonical: Option<String>,
    pub canonical_count: usize,
    pub simhash: Option<u64>,
    pub word_count: usize,
    pub text_to_code_ratio: f64,
    pub image_count: u32,
    pub images_missing_alt: u32,
    pub images_alt_too_long: u32,
    pub mixed_content_count: u32,
    pub insecure_form_count: u32,
    pub hsts_header: bool,
    pub content_security_policy_header: bool,
    pub x_frame_options_header: bool,
    pub x_content_type_options_header: bool,
    pub viewport: bool,
    pub amphtml: Option<String>,
    pub rel_next: Option<String>,
    pub rel_prev: Option<String>,
    pub hreflang_count: u32,
    pub hreflang_invalid_count: u32,
    pub hreflang_missing_self_reference: bool,
    pub json_ld_count: u32,
    pub json_ld_invalid_count: u32,
    pub open_graph_count: u32,
    pub twitter_card_count: u32,
    pub near_duplicate_cluster_id: Option<u64>,
    pub inlink_count: u32,
    pub outlink_count: u32,
    pub internal_outlink_count: u32,
    pub external_outlink_count: u32,
    pub custom_extractions: Vec<CustomExtractionValue>,
    pub error: Option<String>,
}

impl CrawlRecord {
    pub fn pending(url: String, depth: usize) -> Self {
        Self {
            id: 0,
            final_url: url.clone(),
            url,
            classification: UrlClassification::Internal,
            in_sitemap: false,
            status_code: None,
            status_text: "Pending".to_string(),
            content_type: None,
            indexability: "Unknown".to_string(),
            indexability_status: "Not fetched".to_string(),
            response_time_ms: 0,
            dns_lookup_time_ms: None,
            ttfb_ms: None,
            download_time_ms: None,
            total_network_time_ms: None,
            transfer_rate_bytes_per_sec: None,
            resolved_ip_count: 0,
            size_bytes: 0,
            response_hash: None,
            depth,
            redirect_target: None,
            redirect_type: None,
            redirect_chain: Vec::new(),
            title: None,
            title_len: 0,
            meta_description: None,
            meta_description_len: 0,
            meta_robots: None,
            x_robots_tag: None,
            h1: None,
            h1_len: 0,
            h1_count: 0,
            h2: None,
            h2_len: 0,
            h2_count: 0,
            canonical: None,
            canonical_count: 0,
            simhash: None,
            word_count: 0,
            text_to_code_ratio: 0.0,
            image_count: 0,
            images_missing_alt: 0,
            images_alt_too_long: 0,
            mixed_content_count: 0,
            insecure_form_count: 0,
            hsts_header: false,
            content_security_policy_header: false,
            x_frame_options_header: false,
            x_content_type_options_header: false,
            viewport: false,
            amphtml: None,
            rel_next: None,
            rel_prev: None,
            hreflang_count: 0,
            hreflang_invalid_count: 0,
            hreflang_missing_self_reference: false,
            json_ld_count: 0,
            json_ld_invalid_count: 0,
            open_graph_count: 0,
            twitter_card_count: 0,
            near_duplicate_cluster_id: None,
            inlink_count: 0,
            outlink_count: 0,
            internal_outlink_count: 0,
            external_outlink_count: 0,
            custom_extractions: Vec::new(),
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
    pub near_duplicates: usize,
    pub indexable: usize,
    pub non_indexable: usize,
    pub title_missing: usize,
    pub title_duplicate: usize,
    pub meta_missing: usize,
    pub meta_duplicate: usize,
    pub h1_missing: usize,
    pub h1_duplicate: usize,
    pub h2_missing: usize,
    pub h2_duplicate: usize,
    pub canonical_missing: usize,
    pub canonical_multiple: usize,
    pub noindex: usize,
    pub images_missing_alt: usize,
    pub images_alt_too_long: usize,
    pub mixed_content: usize,
    pub insecure_forms: usize,
    pub hreflang_invalid: usize,
    pub structured_data_invalid: usize,
    pub missing_viewport: usize,
    pub missing_hsts: usize,
    pub sitemap_orphans: usize,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinkEdge {
    pub id: u64,
    pub source_url: String,
    pub target_url: String,
    pub anchor_text: String,
    pub rel: String,
    pub rel_nofollow: bool,
    pub link_type: LinkType,
    pub source_status_code: Option<u16>,
    pub target_status_code: Option<u16>,
    pub source_depth: usize,
    pub target_depth: Option<usize>,
    pub source_position: u32,
    pub discovery_order: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkEdgeQuery {
    pub offset: usize,
    pub limit: usize,
    #[serde(default)]
    pub global_search: Option<String>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub sort_dir: SortDirection,
    #[serde(default)]
    pub view: LinkEdgeView,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub internal_only: bool,
}

impl Default for LinkEdgeQuery {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 1_000,
            global_search: None,
            sort_by: None,
            sort_dir: SortDirection::Asc,
            view: LinkEdgeView::All,
            source_url: None,
            target_url: None,
            internal_only: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkEdgeResponse {
    pub edges: Vec<LinkEdge>,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnchorTextRow {
    pub anchor_text: String,
    pub target_url: String,
    pub link_type: LinkType,
    pub link_count: usize,
    pub source_count: usize,
    pub nofollow_count: usize,
    pub first_source_url: String,
    pub target_status_code: Option<u16>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorTextResponse {
    pub rows: Vec<AnchorTextRow>,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlGraphQuery {
    pub max_nodes: usize,
    pub max_edges: usize,
    #[serde(default)]
    pub internal_only: bool,
}

impl Default for CrawlGraphQuery {
    fn default() -> Self {
        Self {
            max_nodes: 2_000,
            max_edges: 5_000,
            internal_only: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub url: String,
    pub label: String,
    pub crawled: bool,
    pub classification: Option<UrlClassification>,
    pub status_code: Option<u16>,
    pub depth: Option<usize>,
    pub indexability: Option<String>,
    pub inlink_count: u32,
    pub outlink_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<LinkEdge>,
    pub total_nodes: usize,
    pub total_edges: usize,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("storage lock poisoned")]
    LockPoisoned,
}

pub trait CrawlStore: Clone + Send + Sync + 'static {
    fn clear(&self);
    fn upsert(&self, record: CrawlRecord) -> CrawlRecord;
    fn add_inlink(&self, target_url: &str);
    fn add_link_edge(&self, edge: LinkEdge) -> LinkEdge;
    fn records(&self) -> Vec<CrawlRecord>;
    fn query(&self, query: GridQuery) -> GridResponse;
    fn link_edges(&self, query: LinkEdgeQuery) -> LinkEdgeResponse;
    fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse;

    fn summary(&self) -> CrawlSummary {
        summarize(&self.records())
    }

    fn crawl_graph(&self, query: CrawlGraphQuery) -> CrawlGraph {
        let max_edges = query.max_edges.max(1);
        let edges = self.link_edges(LinkEdgeQuery {
            limit: max_edges,
            internal_only: query.internal_only,
            ..LinkEdgeQuery::default()
        });
        build_crawl_graph(self.records(), edges.edges, edges.total, query)
    }
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
    link_edges: Vec<LinkEdge>,
    next_id: u64,
    next_edge_id: u64,
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
            update_memory_edge_statuses(&mut inner.link_edges, &record);
            return record;
        }

        inner.next_id += 1;
        record.id = inner.next_id;
        let index = inner.records.len();
        inner.url_to_index.insert(key, index);
        inner.records.push(record.clone());
        update_memory_edge_statuses(&mut inner.link_edges, &record);
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

    pub fn add_link_edge(&self, mut edge: LinkEdge) -> LinkEdge {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        inner.next_edge_id += 1;
        edge.id = inner.next_edge_id;
        edge.discovery_order = edge.id;

        if let Some(index) = inner.url_to_index.get(&edge.source_url).copied() {
            let source = &inner.records[index];
            edge.source_status_code = source.status_code;
            edge.source_depth = source.depth;
        }

        if let Some(index) = inner.url_to_index.get(&edge.target_url).copied() {
            let target = &inner.records[index];
            edge.target_status_code = target.status_code;
            edge.target_depth = Some(target.depth);
        }

        inner.link_edges.push(edge.clone());
        edge
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
        let h1_counts = duplicate_counts(rows.iter().filter_map(|row| row.h1.as_deref()));
        let h2_counts = duplicate_counts(rows.iter().filter_map(|row| row.h2.as_deref()));
        let near_duplicate_counts =
            cluster_counts(rows.iter().filter_map(|row| row.near_duplicate_cluster_id));

        rows.retain(|row| {
            matches_view(
                row,
                &query.view,
                &title_counts,
                &meta_counts,
                &h1_counts,
                &h2_counts,
                &near_duplicate_counts,
            )
        });

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

    pub fn link_edges(&self, query: LinkEdgeQuery) -> LinkEdgeResponse {
        let inner = self.inner.read().expect("memory store lock poisoned");
        let mut edges = inner.link_edges.clone();
        filter_link_edges(&mut edges, &query);
        if let Some(search) = query
            .global_search
            .as_ref()
            .map(|value| value.trim().to_lowercase())
        {
            if !search.is_empty() {
                edges.retain(|edge| link_edge_matches_search(edge, &search));
            }
        }
        if let Some(sort_by) = query.sort_by.as_deref() {
            sort_link_edges(&mut edges, sort_by, &query.sort_dir);
        }
        let total = edges.len();
        let limit = query.limit.min(10_000);
        let rows = edges.into_iter().skip(query.offset).take(limit).collect();

        LinkEdgeResponse { edges: rows, total }
    }

    pub fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse {
        let inner = self.inner.read().expect("memory store lock poisoned");
        let mut edges = inner.link_edges.clone();
        filter_link_edges(&mut edges, &query);
        if let Some(search) = query
            .global_search
            .as_ref()
            .map(|value| value.trim().to_lowercase())
        {
            if !search.is_empty() {
                edges.retain(|edge| link_edge_matches_search(edge, &search));
            }
        }
        let mut rows = aggregate_anchor_texts(edges);
        if let Some(sort_by) = query.sort_by.as_deref() {
            sort_anchor_text_rows(&mut rows, sort_by, &query.sort_dir);
        } else {
            sort_anchor_text_rows(&mut rows, "linkCount", &SortDirection::Desc);
        }
        let total = rows.len();
        let limit = query.limit.min(10_000);
        let rows = rows.into_iter().skip(query.offset).take(limit).collect();

        AnchorTextResponse { rows, total }
    }
}

impl CrawlStore for MemoryStore {
    fn clear(&self) {
        Self::clear(self);
    }

    fn upsert(&self, record: CrawlRecord) -> CrawlRecord {
        Self::upsert(self, record)
    }

    fn add_inlink(&self, target_url: &str) {
        Self::add_inlink(self, target_url);
    }

    fn add_link_edge(&self, edge: LinkEdge) -> LinkEdge {
        Self::add_link_edge(self, edge)
    }

    fn records(&self) -> Vec<CrawlRecord> {
        Self::records(self)
    }

    fn query(&self, query: GridQuery) -> GridResponse {
        Self::query(self, query)
    }

    fn link_edges(&self, query: LinkEdgeQuery) -> LinkEdgeResponse {
        Self::link_edges(self, query)
    }

    fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse {
        Self::anchor_texts(self, query)
    }

    fn summary(&self) -> CrawlSummary {
        Self::summary(self)
    }
}

#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn try_clear(&self) -> Result<(), StorageError> {
        let conn = self.connection()?;
        conn.execute("DELETE FROM crawl_records", [])?;
        conn.execute("DELETE FROM inlink_counts", [])?;
        conn.execute("DELETE FROM link_edges", [])?;
        Ok(())
    }

    pub fn try_upsert(&self, mut record: CrawlRecord) -> Result<CrawlRecord, StorageError> {
        let conn = self.connection()?;
        let existing_id = conn
            .query_row(
                "SELECT id FROM crawl_records WHERE final_url = ?1",
                [&record.final_url],
                |row| row.get::<_, i64>(0).map(|id| id as u64),
            )
            .optional()?;
        let inlink_count = conn
            .query_row(
                "SELECT count FROM inlink_counts WHERE url = ?1",
                [&record.final_url],
                |row| row.get::<_, u32>(0),
            )
            .optional()?
            .unwrap_or(record.inlink_count);
        record.inlink_count = inlink_count;
        let redirect_chain = serde_json::to_string(&record.redirect_chain)?;
        let custom_extractions = serde_json::to_string(&record.custom_extractions)?;
        let classification = classification_to_str(&record.classification);
        let simhash = record.simhash.map(|value| value.to_string());
        let near_duplicate_cluster_id = record.near_duplicate_cluster_id.map(|value| value as i64);
        let dns_lookup_time_ms = record.dns_lookup_time_ms.map(|value| value as i64);
        let ttfb_ms = record.ttfb_ms.map(|value| value as i64);
        let download_time_ms = record.download_time_ms.map(|value| value as i64);
        let total_network_time_ms = record.total_network_time_ms.map(|value| value as i64);
        let transfer_rate_bytes_per_sec =
            record.transfer_rate_bytes_per_sec.map(|value| value as i64);

        if let Some(id) = existing_id {
            record.id = id;
            conn.execute(
                "UPDATE crawl_records SET
                    url = ?1,
                    final_url = ?2,
                    classification = ?3,
                    status_code = ?4,
                    status_text = ?5,
                    content_type = ?6,
                    indexability = ?7,
                    indexability_status = ?8,
                    response_time_ms = ?9,
                    size_bytes = ?10,
                    response_hash = ?11,
                    depth = ?12,
                    redirect_target = ?13,
                    redirect_type = ?14,
                    redirect_chain = ?15,
                    title = ?16,
                    title_len = ?17,
                    meta_description = ?18,
                    meta_description_len = ?19,
                    meta_robots = ?20,
                    x_robots_tag = ?21,
                    h1 = ?22,
                    h1_len = ?23,
                    h1_count = ?24,
                    h2 = ?25,
                    h2_len = ?26,
                    h2_count = ?27,
                    canonical = ?28,
                    canonical_count = ?29,
                    simhash = ?30,
                    word_count = ?31,
                    text_to_code_ratio = ?32,
                    image_count = ?33,
                    images_missing_alt = ?34,
                    images_alt_too_long = ?35,
                    mixed_content_count = ?36,
                    insecure_form_count = ?37,
                    hsts_header = ?38,
                    content_security_policy_header = ?39,
                    x_frame_options_header = ?40,
                    x_content_type_options_header = ?41,
                    viewport = ?42,
                    amphtml = ?43,
                    rel_next = ?44,
                    rel_prev = ?45,
                    hreflang_count = ?46,
                    hreflang_invalid_count = ?47,
                    hreflang_missing_self_reference = ?48,
                    json_ld_count = ?49,
                    json_ld_invalid_count = ?50,
                    open_graph_count = ?51,
                    twitter_card_count = ?52,
                    near_duplicate_cluster_id = ?53,
                    inlink_count = ?54,
                    outlink_count = ?55,
                    internal_outlink_count = ?56,
                    external_outlink_count = ?57,
                    custom_extractions = ?58,
                    error = ?59,
                    dns_lookup_time_ms = ?60,
                    ttfb_ms = ?61,
                    download_time_ms = ?62,
                    total_network_time_ms = ?63,
                    transfer_rate_bytes_per_sec = ?64,
                    resolved_ip_count = ?65,
                    in_sitemap = ?66
                 WHERE id = ?67",
                params![
                    record.url,
                    record.final_url,
                    classification,
                    record.status_code,
                    record.status_text,
                    record.content_type,
                    record.indexability,
                    record.indexability_status,
                    record.response_time_ms as i64,
                    record.size_bytes as i64,
                    record.response_hash,
                    record.depth as i64,
                    record.redirect_target,
                    record.redirect_type,
                    redirect_chain,
                    record.title,
                    record.title_len as i64,
                    record.meta_description,
                    record.meta_description_len as i64,
                    record.meta_robots,
                    record.x_robots_tag,
                    record.h1,
                    record.h1_len as i64,
                    record.h1_count as i64,
                    record.h2,
                    record.h2_len as i64,
                    record.h2_count as i64,
                    record.canonical,
                    record.canonical_count as i64,
                    simhash,
                    record.word_count as i64,
                    record.text_to_code_ratio,
                    record.image_count,
                    record.images_missing_alt,
                    record.images_alt_too_long,
                    record.mixed_content_count,
                    record.insecure_form_count,
                    record.hsts_header,
                    record.content_security_policy_header,
                    record.x_frame_options_header,
                    record.x_content_type_options_header,
                    record.viewport,
                    record.amphtml,
                    record.rel_next,
                    record.rel_prev,
                    record.hreflang_count,
                    record.hreflang_invalid_count,
                    record.hreflang_missing_self_reference,
                    record.json_ld_count,
                    record.json_ld_invalid_count,
                    record.open_graph_count,
                    record.twitter_card_count,
                    near_duplicate_cluster_id,
                    record.inlink_count,
                    record.outlink_count,
                    record.internal_outlink_count,
                    record.external_outlink_count,
                    custom_extractions,
                    record.error,
                    dns_lookup_time_ms,
                    ttfb_ms,
                    download_time_ms,
                    total_network_time_ms,
                    transfer_rate_bytes_per_sec,
                    record.resolved_ip_count,
                    record.in_sitemap,
                    record.id as i64
                ],
            )?;
            update_sqlite_edge_statuses(&conn, &record)?;
            Ok(record)
        } else {
            conn.execute(
                "INSERT INTO crawl_records (
                    url,
                    final_url,
                    classification,
                    status_code,
                    status_text,
                    content_type,
                    indexability,
                    indexability_status,
                    response_time_ms,
                    size_bytes,
                    response_hash,
                    depth,
                    redirect_target,
                    redirect_type,
                    redirect_chain,
                    title,
                    title_len,
                    meta_description,
                    meta_description_len,
                    meta_robots,
                    x_robots_tag,
                    h1,
                    h1_len,
                    h1_count,
                    h2,
                    h2_len,
                    h2_count,
                    canonical,
                    canonical_count,
                    simhash,
                    word_count,
                    text_to_code_ratio,
                    image_count,
                    images_missing_alt,
                    images_alt_too_long,
                    mixed_content_count,
                    insecure_form_count,
                    hsts_header,
                    content_security_policy_header,
                    x_frame_options_header,
                    x_content_type_options_header,
                    viewport,
                    amphtml,
                    rel_next,
                    rel_prev,
                    hreflang_count,
                    hreflang_invalid_count,
                    hreflang_missing_self_reference,
                    json_ld_count,
                    json_ld_invalid_count,
                    open_graph_count,
                    twitter_card_count,
                    near_duplicate_cluster_id,
                    inlink_count,
                    outlink_count,
                    internal_outlink_count,
                    external_outlink_count,
                    custom_extractions,
                    error,
                    dns_lookup_time_ms,
                    ttfb_ms,
                    download_time_ms,
                    total_network_time_ms,
                    transfer_rate_bytes_per_sec,
                    resolved_ip_count,
                    in_sitemap
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                    ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35,
                    ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46,
                    ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54, ?55, ?56, ?57,
                    ?58, ?59, ?60, ?61, ?62, ?63, ?64, ?65, ?66
                 )",
                params![
                    record.url,
                    record.final_url,
                    classification,
                    record.status_code,
                    record.status_text,
                    record.content_type,
                    record.indexability,
                    record.indexability_status,
                    record.response_time_ms as i64,
                    record.size_bytes as i64,
                    record.response_hash,
                    record.depth as i64,
                    record.redirect_target,
                    record.redirect_type,
                    redirect_chain,
                    record.title,
                    record.title_len as i64,
                    record.meta_description,
                    record.meta_description_len as i64,
                    record.meta_robots,
                    record.x_robots_tag,
                    record.h1,
                    record.h1_len as i64,
                    record.h1_count as i64,
                    record.h2,
                    record.h2_len as i64,
                    record.h2_count as i64,
                    record.canonical,
                    record.canonical_count as i64,
                    simhash,
                    record.word_count as i64,
                    record.text_to_code_ratio,
                    record.image_count,
                    record.images_missing_alt,
                    record.images_alt_too_long,
                    record.mixed_content_count,
                    record.insecure_form_count,
                    record.hsts_header,
                    record.content_security_policy_header,
                    record.x_frame_options_header,
                    record.x_content_type_options_header,
                    record.viewport,
                    record.amphtml,
                    record.rel_next,
                    record.rel_prev,
                    record.hreflang_count,
                    record.hreflang_invalid_count,
                    record.hreflang_missing_self_reference,
                    record.json_ld_count,
                    record.json_ld_invalid_count,
                    record.open_graph_count,
                    record.twitter_card_count,
                    near_duplicate_cluster_id,
                    record.inlink_count,
                    record.outlink_count,
                    record.internal_outlink_count,
                    record.external_outlink_count,
                    custom_extractions,
                    record.error,
                    dns_lookup_time_ms,
                    ttfb_ms,
                    download_time_ms,
                    total_network_time_ms,
                    transfer_rate_bytes_per_sec,
                    record.resolved_ip_count,
                    record.in_sitemap
                ],
            )?;
            record.id = conn.last_insert_rowid() as u64;
            update_sqlite_edge_statuses(&conn, &record)?;
            Ok(record)
        }
    }

    pub fn try_add_inlink(&self, target_url: &str) -> Result<(), StorageError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO inlink_counts (url, count) VALUES (?1, 1)
             ON CONFLICT(url) DO UPDATE SET count = count + 1",
            [target_url],
        )?;
        conn.execute(
            "UPDATE crawl_records
             SET inlink_count = COALESCE((SELECT count FROM inlink_counts WHERE url = ?1), inlink_count)
             WHERE final_url = ?1",
            [target_url],
        )?;
        Ok(())
    }

    pub fn try_add_link_edge(&self, mut edge: LinkEdge) -> Result<LinkEdge, StorageError> {
        let conn = self.connection()?;
        if let Some((status_code, depth)) = sqlite_record_status(&conn, &edge.source_url)? {
            edge.source_status_code = status_code;
            edge.source_depth = depth;
        }
        if let Some((status_code, depth)) = sqlite_record_status(&conn, &edge.target_url)? {
            edge.target_status_code = status_code;
            edge.target_depth = Some(depth);
        }

        conn.execute(
            "INSERT INTO link_edges (
                source_url,
                target_url,
                anchor_text,
                rel,
                rel_nofollow,
                link_type,
                source_status_code,
                target_status_code,
                source_depth,
                target_depth,
                source_position,
                discovery_order
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                &edge.source_url,
                &edge.target_url,
                &edge.anchor_text,
                &edge.rel,
                edge.rel_nofollow,
                link_type_to_str(&edge.link_type),
                edge.source_status_code,
                edge.target_status_code,
                edge.source_depth as i64,
                edge.target_depth.map(|value| value as i64),
                i64::from(edge.source_position),
                edge.discovery_order as i64
            ],
        )?;
        edge.id = conn.last_insert_rowid() as u64;
        edge.discovery_order = edge.id;
        conn.execute(
            "UPDATE link_edges SET discovery_order = ?1 WHERE id = ?1",
            [edge.id as i64],
        )?;
        Ok(edge)
    }

    pub fn try_records(&self) -> Result<Vec<CrawlRecord>, StorageError> {
        self.query_records("SELECT * FROM crawl_records ORDER BY id ASC", [])
    }

    pub fn try_link_edges(&self, query: LinkEdgeQuery) -> Result<LinkEdgeResponse, StorageError> {
        let (where_clause, args) = link_edge_filter_sql(&query);
        let order_by = link_edge_sort_column(query.sort_by.as_deref())
            .map(|column| {
                let direction = match query.sort_dir {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                };
                format!(" ORDER BY {column} {direction}")
            })
            .unwrap_or_else(|| " ORDER BY id ASC".to_string());
        let limit = query.limit.min(10_000);
        let total_sql = format!("SELECT COUNT(*) FROM link_edges{where_clause}");
        let select_sql = format!(
            "SELECT * FROM link_edges{where_clause}{order_by} LIMIT {limit} OFFSET {}",
            query.offset
        );
        let conn = self.connection()?;
        let total = count_query(&conn, &total_sql, &args)?;
        let edges = query_link_edges_with_args(&conn, &select_sql, &args)?;
        Ok(LinkEdgeResponse { edges, total })
    }

    pub fn try_anchor_texts(
        &self,
        query: LinkEdgeQuery,
    ) -> Result<AnchorTextResponse, StorageError> {
        let (where_clause, args) = link_edge_filter_sql(&query);
        let order_by = anchor_text_sort_column(query.sort_by.as_deref())
            .map(|column| {
                let direction = match query.sort_dir {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                };
                format!(" ORDER BY {column} {direction}, anchor_text ASC, target_url ASC")
            })
            .unwrap_or_else(|| {
                " ORDER BY link_count DESC, anchor_text ASC, target_url ASC".to_string()
            });
        let limit = query.limit.min(10_000);
        let total_sql = format!(
            "SELECT COUNT(*) FROM (
                SELECT 1
                FROM link_edges{where_clause}
                GROUP BY lower(trim(anchor_text)), target_url, link_type
            )"
        );
        let select_sql = format!(
            "SELECT
                MIN(trim(anchor_text)) AS anchor_text,
                target_url,
                link_type,
                COUNT(*) AS link_count,
                COUNT(DISTINCT source_url) AS source_count,
                SUM(CASE WHEN rel_nofollow != 0 THEN 1 ELSE 0 END) AS nofollow_count,
                MIN(source_url) AS first_source_url,
                MAX(target_status_code) AS target_status_code
             FROM link_edges{where_clause}
             GROUP BY lower(trim(anchor_text)), target_url, link_type
             {order_by}
             LIMIT {limit} OFFSET {}",
            query.offset
        );
        let conn = self.connection()?;
        let total = count_query(&conn, &total_sql, &args)?;
        let rows = query_anchor_texts_with_args(&conn, &select_sql, &args)?;
        Ok(AnchorTextResponse { rows, total })
    }

    pub fn try_query(&self, query: GridQuery) -> Result<GridResponse, StorageError> {
        if needs_duplicate_filter(&query.view) {
            let memory = MemoryStore::new();
            for record in self.try_records()? {
                memory.upsert(record);
            }
            return Ok(memory.query(query));
        }

        let summary = self.try_summary()?;
        let (where_clause, args) = query_filter_sql(&query);
        let order_by = sort_column(query.sort_by.as_deref())
            .map(|column| {
                let direction = match query.sort_dir {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                };
                format!(" ORDER BY {column} {direction}")
            })
            .unwrap_or_else(|| " ORDER BY id ASC".to_string());
        let limit = query.limit.min(10_000);
        let total_sql = format!("SELECT COUNT(*) FROM crawl_records{where_clause}");
        let select_sql = format!(
            "SELECT * FROM crawl_records{where_clause}{order_by} LIMIT {limit} OFFSET {}",
            query.offset
        );
        let conn = self.connection()?;
        let total = count_query(&conn, &total_sql, &args)?;
        let rows = query_records_with_args(&conn, &select_sql, &args)?;

        Ok(GridResponse {
            rows,
            total,
            summary,
        })
    }

    pub fn try_summary(&self) -> Result<CrawlSummary, StorageError> {
        let conn = self.connection()?;
        let mut summary = CrawlSummary::default();
        summary.total = conn.query_row("SELECT COUNT(*) FROM crawl_records", [], |row| {
            row.get::<_, i64>(0).map(|count| count as usize)
        })?;
        summary.internal = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE classification = 'internal'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.external = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE classification = 'external'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.success = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE status_code >= 200 AND status_code < 300",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.redirects = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE status_code >= 300 AND status_code < 400 OR redirect_chain != '[]'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.client_errors = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE status_code >= 400 AND status_code < 500",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.server_errors = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE status_code >= 500",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.no_response = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE status_code IS NULL",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.broken = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE error IS NOT NULL OR status_code IS NULL OR status_code >= 400",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.near_duplicates = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records
             WHERE near_duplicate_cluster_id IS NOT NULL
             AND near_duplicate_cluster_id IN (
                SELECT near_duplicate_cluster_id
                FROM crawl_records
                WHERE near_duplicate_cluster_id IS NOT NULL
                GROUP BY near_duplicate_cluster_id
                HAVING COUNT(*) > 1
             )",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.indexable = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE indexability = 'Indexable'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.non_indexable = conn.query_row(
            "SELECT COUNT(*) FROM crawl_records WHERE indexability = 'Non-indexable'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count as usize),
        )?;
        summary.title_missing = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE title IS NULL OR trim(title) = ''",
        )?;
        summary.title_duplicate = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records
             WHERE title IS NOT NULL AND trim(title) != ''
             AND lower(trim(title)) IN (
                SELECT lower(trim(title))
                FROM crawl_records
                WHERE title IS NOT NULL AND trim(title) != ''
                GROUP BY lower(trim(title))
                HAVING COUNT(*) > 1
             )",
        )?;
        summary.meta_missing = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records
             WHERE meta_description IS NULL OR trim(meta_description) = ''",
        )?;
        summary.meta_duplicate = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records
             WHERE meta_description IS NOT NULL AND trim(meta_description) != ''
             AND lower(trim(meta_description)) IN (
                SELECT lower(trim(meta_description))
                FROM crawl_records
                WHERE meta_description IS NOT NULL AND trim(meta_description) != ''
                GROUP BY lower(trim(meta_description))
                HAVING COUNT(*) > 1
             )",
        )?;
        summary.h1_missing = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE h1 IS NULL OR trim(h1) = ''",
        )?;
        summary.h1_duplicate = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records
             WHERE h1 IS NOT NULL AND trim(h1) != ''
             AND lower(trim(h1)) IN (
                SELECT lower(trim(h1))
                FROM crawl_records
                WHERE h1 IS NOT NULL AND trim(h1) != ''
                GROUP BY lower(trim(h1))
                HAVING COUNT(*) > 1
             )",
        )?;
        summary.h2_missing = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE h2 IS NULL OR trim(h2) = ''",
        )?;
        summary.h2_duplicate = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records
             WHERE h2 IS NOT NULL AND trim(h2) != ''
             AND lower(trim(h2)) IN (
                SELECT lower(trim(h2))
                FROM crawl_records
                WHERE h2 IS NOT NULL AND trim(h2) != ''
                GROUP BY lower(trim(h2))
                HAVING COUNT(*) > 1
             )",
        )?;
        summary.canonical_missing = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE canonical IS NULL OR trim(canonical) = ''",
        )?;
        summary.canonical_multiple = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE canonical_count > 1",
        )?;
        summary.noindex = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE lower(indexability_status) LIKE '%noindex%'",
        )?;
        summary.images_missing_alt = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE images_missing_alt > 0",
        )?;
        summary.images_alt_too_long = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE images_alt_too_long > 0",
        )?;
        summary.mixed_content = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE mixed_content_count > 0",
        )?;
        summary.insecure_forms = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE insecure_form_count > 0",
        )?;
        summary.hreflang_invalid = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE hreflang_invalid_count > 0",
        )?;
        summary.structured_data_invalid = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records WHERE json_ld_invalid_count > 0",
        )?;
        summary.missing_viewport = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records
             WHERE status_code >= 200 AND status_code < 300
             AND lower(content_type) LIKE '%text/html%'
             AND viewport = 0",
        )?;
        summary.missing_hsts = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records
             WHERE status_code >= 200 AND status_code < 300
             AND final_url LIKE 'https://%'
             AND hsts_header = 0",
        )?;
        summary.sitemap_orphans = summary_count(
            &conn,
            "SELECT COUNT(*) FROM crawl_records
             WHERE in_sitemap != 0 AND inlink_count = 0 AND classification = 'internal'",
        )?;
        Ok(summary)
    }

    fn initialize(&self) -> Result<(), StorageError> {
        let conn = self.connection()?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS crawl_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL,
                final_url TEXT NOT NULL UNIQUE,
                classification TEXT NOT NULL,
                status_code INTEGER,
                status_text TEXT NOT NULL,
                content_type TEXT,
                indexability TEXT NOT NULL,
                indexability_status TEXT NOT NULL,
                response_time_ms INTEGER NOT NULL,
                dns_lookup_time_ms INTEGER,
                ttfb_ms INTEGER,
                download_time_ms INTEGER,
                total_network_time_ms INTEGER,
                transfer_rate_bytes_per_sec INTEGER,
                resolved_ip_count INTEGER NOT NULL DEFAULT 0,
                size_bytes INTEGER NOT NULL,
                response_hash TEXT,
                depth INTEGER NOT NULL,
                redirect_target TEXT,
                redirect_type TEXT,
                redirect_chain TEXT NOT NULL,
                title TEXT,
                title_len INTEGER NOT NULL,
                meta_description TEXT,
                meta_description_len INTEGER NOT NULL,
                meta_robots TEXT,
                x_robots_tag TEXT,
                h1 TEXT,
                h1_len INTEGER NOT NULL,
                h1_count INTEGER NOT NULL DEFAULT 0,
                h2 TEXT,
                h2_len INTEGER NOT NULL DEFAULT 0,
                h2_count INTEGER NOT NULL DEFAULT 0,
                canonical TEXT,
                canonical_count INTEGER NOT NULL DEFAULT 0,
                simhash TEXT,
                word_count INTEGER NOT NULL DEFAULT 0,
                text_to_code_ratio REAL NOT NULL DEFAULT 0,
                image_count INTEGER NOT NULL DEFAULT 0,
                images_missing_alt INTEGER NOT NULL DEFAULT 0,
                images_alt_too_long INTEGER NOT NULL DEFAULT 0,
                mixed_content_count INTEGER NOT NULL DEFAULT 0,
                insecure_form_count INTEGER NOT NULL DEFAULT 0,
                hsts_header INTEGER NOT NULL DEFAULT 0,
                content_security_policy_header INTEGER NOT NULL DEFAULT 0,
                x_frame_options_header INTEGER NOT NULL DEFAULT 0,
                x_content_type_options_header INTEGER NOT NULL DEFAULT 0,
                viewport INTEGER NOT NULL DEFAULT 0,
                amphtml TEXT,
                rel_next TEXT,
                rel_prev TEXT,
                hreflang_count INTEGER NOT NULL DEFAULT 0,
                hreflang_invalid_count INTEGER NOT NULL DEFAULT 0,
                hreflang_missing_self_reference INTEGER NOT NULL DEFAULT 0,
                json_ld_count INTEGER NOT NULL DEFAULT 0,
                json_ld_invalid_count INTEGER NOT NULL DEFAULT 0,
                open_graph_count INTEGER NOT NULL DEFAULT 0,
                twitter_card_count INTEGER NOT NULL DEFAULT 0,
                near_duplicate_cluster_id INTEGER,
                inlink_count INTEGER NOT NULL,
                outlink_count INTEGER NOT NULL,
                internal_outlink_count INTEGER NOT NULL,
                external_outlink_count INTEGER NOT NULL,
                custom_extractions TEXT NOT NULL DEFAULT '[]',
                error TEXT,
                in_sitemap INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS inlink_counts (
                url TEXT PRIMARY KEY,
                count INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS link_edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_url TEXT NOT NULL,
                target_url TEXT NOT NULL,
                anchor_text TEXT NOT NULL DEFAULT '',
                rel TEXT NOT NULL DEFAULT '',
                rel_nofollow INTEGER NOT NULL DEFAULT 0,
                link_type TEXT NOT NULL,
                source_status_code INTEGER,
                target_status_code INTEGER,
                source_depth INTEGER NOT NULL,
                target_depth INTEGER,
                source_position INTEGER NOT NULL DEFAULT 0,
                discovery_order INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_crawl_records_status_code ON crawl_records(status_code);
            CREATE INDEX IF NOT EXISTS idx_crawl_records_classification ON crawl_records(classification);
            CREATE INDEX IF NOT EXISTS idx_crawl_records_depth ON crawl_records(depth);
            CREATE INDEX IF NOT EXISTS idx_crawl_records_title ON crawl_records(title);
            CREATE INDEX IF NOT EXISTS idx_crawl_records_meta_description ON crawl_records(meta_description);
            CREATE INDEX IF NOT EXISTS idx_crawl_records_near_duplicate_cluster_id ON crawl_records(near_duplicate_cluster_id);
            CREATE INDEX IF NOT EXISTS idx_link_edges_source_url ON link_edges(source_url);
            CREATE INDEX IF NOT EXISTS idx_link_edges_target_url ON link_edges(target_url);
            CREATE INDEX IF NOT EXISTS idx_link_edges_link_type ON link_edges(link_type);
            ",
        )?;
        if !column_exists(&conn, "crawl_records", "response_hash")? {
            conn.execute(
                "ALTER TABLE crawl_records ADD COLUMN response_hash TEXT",
                [],
            )?;
        }
        if !column_exists(&conn, "crawl_records", "simhash")? {
            conn.execute("ALTER TABLE crawl_records ADD COLUMN simhash TEXT", [])?;
        }
        if !column_exists(&conn, "crawl_records", "word_count")? {
            conn.execute(
                "ALTER TABLE crawl_records ADD COLUMN word_count INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !column_exists(&conn, "crawl_records", "text_to_code_ratio")? {
            conn.execute(
                "ALTER TABLE crawl_records ADD COLUMN text_to_code_ratio REAL NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !column_exists(&conn, "crawl_records", "near_duplicate_cluster_id")? {
            conn.execute(
                "ALTER TABLE crawl_records ADD COLUMN near_duplicate_cluster_id INTEGER",
                [],
            )?;
        }
        if !column_exists(&conn, "crawl_records", "custom_extractions")? {
            conn.execute(
                "ALTER TABLE crawl_records ADD COLUMN custom_extractions TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }
        add_column_if_missing(&conn, "meta_robots", "TEXT")?;
        add_column_if_missing(&conn, "x_robots_tag", "TEXT")?;
        add_column_if_missing(&conn, "h1_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "h2", "TEXT")?;
        add_column_if_missing(&conn, "h2_len", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "h2_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "canonical_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "image_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "images_missing_alt", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "images_alt_too_long", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "mixed_content_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "insecure_form_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "dns_lookup_time_ms", "INTEGER")?;
        add_column_if_missing(&conn, "ttfb_ms", "INTEGER")?;
        add_column_if_missing(&conn, "download_time_ms", "INTEGER")?;
        add_column_if_missing(&conn, "total_network_time_ms", "INTEGER")?;
        add_column_if_missing(&conn, "transfer_rate_bytes_per_sec", "INTEGER")?;
        add_column_if_missing(&conn, "resolved_ip_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "hsts_header", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(
            &conn,
            "content_security_policy_header",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(
            &conn,
            "x_frame_options_header",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(
            &conn,
            "x_content_type_options_header",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(&conn, "viewport", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "amphtml", "TEXT")?;
        add_column_if_missing(&conn, "rel_next", "TEXT")?;
        add_column_if_missing(&conn, "rel_prev", "TEXT")?;
        add_column_if_missing(&conn, "hreflang_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(
            &conn,
            "hreflang_invalid_count",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(
            &conn,
            "hreflang_missing_self_reference",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(&conn, "json_ld_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "json_ld_invalid_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "open_graph_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "twitter_card_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "in_sitemap", "INTEGER NOT NULL DEFAULT 0")?;
        add_table_column_if_missing(
            &conn,
            "link_edges",
            "source_position",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.conn.lock().map_err(|_| StorageError::LockPoisoned)
    }

    fn query_records<P>(&self, sql: &str, params: P) -> Result<Vec<CrawlRecord>, StorageError>
    where
        P: rusqlite::Params,
    {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params, record_from_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }
}

impl CrawlStore for SqliteStore {
    fn clear(&self) {
        self.try_clear().expect("sqlite clear failed");
    }

    fn upsert(&self, record: CrawlRecord) -> CrawlRecord {
        self.try_upsert(record).expect("sqlite upsert failed")
    }

    fn add_inlink(&self, target_url: &str) {
        self.try_add_inlink(target_url)
            .expect("sqlite inlink update failed");
    }

    fn add_link_edge(&self, edge: LinkEdge) -> LinkEdge {
        self.try_add_link_edge(edge)
            .expect("sqlite link edge insert failed")
    }

    fn records(&self) -> Vec<CrawlRecord> {
        self.try_records().expect("sqlite record query failed")
    }

    fn query(&self, query: GridQuery) -> GridResponse {
        self.try_query(query).expect("sqlite grid query failed")
    }

    fn link_edges(&self, query: LinkEdgeQuery) -> LinkEdgeResponse {
        self.try_link_edges(query)
            .expect("sqlite link edge query failed")
    }

    fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse {
        self.try_anchor_texts(query)
            .expect("sqlite anchor text query failed")
    }

    fn summary(&self) -> CrawlSummary {
        self.try_summary().expect("sqlite summary query failed")
    }
}

#[derive(Clone)]
pub enum ActiveStore {
    Memory(MemoryStore),
    Sqlite(SqliteStore),
}

impl ActiveStore {
    pub fn memory() -> Self {
        Self::Memory(MemoryStore::new())
    }

    pub fn sqlite(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Ok(Self::Sqlite(SqliteStore::open(path)?))
    }
}

impl CrawlStore for ActiveStore {
    fn clear(&self) {
        match self {
            ActiveStore::Memory(store) => store.clear(),
            ActiveStore::Sqlite(store) => store.clear(),
        }
    }

    fn upsert(&self, record: CrawlRecord) -> CrawlRecord {
        match self {
            ActiveStore::Memory(store) => store.upsert(record),
            ActiveStore::Sqlite(store) => store.upsert(record),
        }
    }

    fn add_inlink(&self, target_url: &str) {
        match self {
            ActiveStore::Memory(store) => store.add_inlink(target_url),
            ActiveStore::Sqlite(store) => store.add_inlink(target_url),
        }
    }

    fn add_link_edge(&self, edge: LinkEdge) -> LinkEdge {
        match self {
            ActiveStore::Memory(store) => store.add_link_edge(edge),
            ActiveStore::Sqlite(store) => store.add_link_edge(edge),
        }
    }

    fn records(&self) -> Vec<CrawlRecord> {
        match self {
            ActiveStore::Memory(store) => store.records(),
            ActiveStore::Sqlite(store) => store.records(),
        }
    }

    fn query(&self, query: GridQuery) -> GridResponse {
        match self {
            ActiveStore::Memory(store) => store.query(query),
            ActiveStore::Sqlite(store) => store.query(query),
        }
    }

    fn link_edges(&self, query: LinkEdgeQuery) -> LinkEdgeResponse {
        match self {
            ActiveStore::Memory(store) => store.link_edges(query),
            ActiveStore::Sqlite(store) => store.link_edges(query),
        }
    }

    fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse {
        match self {
            ActiveStore::Memory(store) => store.anchor_texts(query),
            ActiveStore::Sqlite(store) => store.anchor_texts(query),
        }
    }

    fn summary(&self) -> CrawlSummary {
        match self {
            ActiveStore::Memory(store) => store.summary(),
            ActiveStore::Sqlite(store) => store.summary(),
        }
    }
}

pub fn summarize(records: &[CrawlRecord]) -> CrawlSummary {
    let mut summary = CrawlSummary {
        total: records.len(),
        ..CrawlSummary::default()
    };
    let near_duplicate_counts = cluster_counts(
        records
            .iter()
            .filter_map(|record| record.near_duplicate_cluster_id),
    );
    let title_counts =
        duplicate_counts(records.iter().filter_map(|record| record.title.as_deref()));
    let meta_counts = duplicate_counts(
        records
            .iter()
            .filter_map(|record| record.meta_description.as_deref()),
    );
    let h1_counts = duplicate_counts(records.iter().filter_map(|record| record.h1.as_deref()));
    let h2_counts = duplicate_counts(records.iter().filter_map(|record| record.h2.as_deref()));

    for record in records {
        match record.classification {
            UrlClassification::Internal => summary.internal += 1,
            UrlClassification::External => summary.external += 1,
        }

        match record.indexability.as_str() {
            "Indexable" => summary.indexable += 1,
            "Non-indexable" => summary.non_indexable += 1,
            _ => {}
        }

        if record.title.as_deref().unwrap_or("").trim().is_empty() {
            summary.title_missing += 1;
        } else if record
            .title
            .as_deref()
            .map(normalize_text_key)
            .and_then(|key| title_counts.get(&key).copied())
            .unwrap_or(0)
            > 1
        {
            summary.title_duplicate += 1;
        }

        if record
            .meta_description
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            summary.meta_missing += 1;
        } else if record
            .meta_description
            .as_deref()
            .map(normalize_text_key)
            .and_then(|key| meta_counts.get(&key).copied())
            .unwrap_or(0)
            > 1
        {
            summary.meta_duplicate += 1;
        }

        if record.h1.as_deref().unwrap_or("").trim().is_empty() {
            summary.h1_missing += 1;
        } else if record
            .h1
            .as_deref()
            .map(normalize_text_key)
            .and_then(|key| h1_counts.get(&key).copied())
            .unwrap_or(0)
            > 1
        {
            summary.h1_duplicate += 1;
        }

        if record.h2.as_deref().unwrap_or("").trim().is_empty() {
            summary.h2_missing += 1;
        } else if record
            .h2
            .as_deref()
            .map(normalize_text_key)
            .and_then(|key| h2_counts.get(&key).copied())
            .unwrap_or(0)
            > 1
        {
            summary.h2_duplicate += 1;
        }

        if record.canonical.as_deref().unwrap_or("").trim().is_empty() {
            summary.canonical_missing += 1;
        }
        if record.canonical_count > 1 {
            summary.canonical_multiple += 1;
        }
        if record
            .indexability_status
            .to_lowercase()
            .contains("noindex")
        {
            summary.noindex += 1;
        }
        if record.images_missing_alt > 0 {
            summary.images_missing_alt += 1;
        }
        if record.images_alt_too_long > 0 {
            summary.images_alt_too_long += 1;
        }
        if record.mixed_content_count > 0 {
            summary.mixed_content += 1;
        }
        if record.insecure_form_count > 0 {
            summary.insecure_forms += 1;
        }
        if record.hreflang_invalid_count > 0 {
            summary.hreflang_invalid += 1;
        }
        if record.json_ld_invalid_count > 0 {
            summary.structured_data_invalid += 1;
        }
        if is_success_html_record(record) && !record.viewport {
            summary.missing_viewport += 1;
        }
        if is_success_record(record)
            && record.final_url.starts_with("https://")
            && !record.hsts_header
        {
            summary.missing_hsts += 1;
        }
        if record.in_sitemap
            && record.inlink_count == 0
            && record.classification == UrlClassification::Internal
        {
            summary.sitemap_orphans += 1;
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

        if record
            .near_duplicate_cluster_id
            .and_then(|cluster_id| near_duplicate_counts.get(&cluster_id).copied())
            .unwrap_or(0)
            > 1
        {
            summary.near_duplicates += 1;
        }
    }

    summary
}

fn classification_to_str(classification: &UrlClassification) -> &'static str {
    match classification {
        UrlClassification::Internal => "internal",
        UrlClassification::External => "external",
    }
}

fn classification_from_str(value: &str) -> UrlClassification {
    match value {
        "external" => UrlClassification::External,
        _ => UrlClassification::Internal,
    }
}

fn link_type_to_str(link_type: &LinkType) -> &'static str {
    match link_type {
        LinkType::Internal => "internal",
        LinkType::External => "external",
    }
}

fn link_type_from_str(value: &str) -> LinkType {
    match value {
        "external" => LinkType::External,
        _ => LinkType::Internal,
    }
}

fn update_memory_edge_statuses(edges: &mut [LinkEdge], record: &CrawlRecord) {
    for edge in edges {
        if edge.source_url == record.final_url {
            edge.source_status_code = record.status_code;
            edge.source_depth = record.depth;
        }
        if edge.target_url == record.final_url {
            edge.target_status_code = record.status_code;
            edge.target_depth = Some(record.depth);
        }
    }
}

fn filter_link_edges(edges: &mut Vec<LinkEdge>, query: &LinkEdgeQuery) {
    match query.view {
        LinkEdgeView::All => {}
        LinkEdgeView::Internal => edges.retain(|edge| edge.link_type == LinkType::Internal),
        LinkEdgeView::External => edges.retain(|edge| edge.link_type == LinkType::External),
        LinkEdgeView::Broken => edges.retain(|edge| {
            edge.target_status_code
                .map(|status| status >= 400)
                .unwrap_or(true)
        }),
        LinkEdgeView::Nofollow => edges.retain(|edge| edge.rel_nofollow),
    }
    if query.internal_only {
        edges.retain(|edge| edge.link_type == LinkType::Internal);
    }
    if let Some(source_url) = query.source_url.as_ref().map(|value| value.trim()) {
        if !source_url.is_empty() {
            edges.retain(|edge| edge.source_url == source_url);
        }
    }
    if let Some(target_url) = query.target_url.as_ref().map(|value| value.trim()) {
        if !target_url.is_empty() {
            edges.retain(|edge| edge.target_url == target_url);
        }
    }
}

struct AnchorTextAccumulator {
    row: AnchorTextRow,
    sources: HashSet<String>,
}

fn aggregate_anchor_texts(edges: Vec<LinkEdge>) -> Vec<AnchorTextRow> {
    let mut groups: HashMap<(String, String, String), AnchorTextAccumulator> = HashMap::new();

    for edge in edges {
        let anchor_text = compact_text(&edge.anchor_text);
        let anchor_key = anchor_text.to_lowercase();
        let link_type = link_type_to_str(&edge.link_type).to_string();
        let key = (anchor_key, edge.target_url.clone(), link_type);
        let entry = groups.entry(key).or_insert_with(|| {
            let mut sources = HashSet::new();
            sources.insert(edge.source_url.clone());
            AnchorTextAccumulator {
                row: AnchorTextRow {
                    anchor_text: anchor_text.clone(),
                    target_url: edge.target_url.clone(),
                    link_type: edge.link_type.clone(),
                    link_count: 0,
                    source_count: 0,
                    nofollow_count: 0,
                    first_source_url: edge.source_url.clone(),
                    target_status_code: edge.target_status_code,
                },
                sources,
            }
        });
        entry.row.link_count = entry.row.link_count.saturating_add(1);
        if edge.rel_nofollow {
            entry.row.nofollow_count = entry.row.nofollow_count.saturating_add(1);
        }
        if entry.row.first_source_url.is_empty() || edge.source_url < entry.row.first_source_url {
            entry.row.first_source_url = edge.source_url.clone();
        }
        if entry.row.target_status_code.is_none() {
            entry.row.target_status_code = edge.target_status_code;
        }
        entry.sources.insert(edge.source_url);
    }

    groups
        .into_values()
        .map(|mut entry| {
            entry.row.source_count = entry.sources.len();
            entry.row
        })
        .collect()
}

fn build_crawl_graph(
    records: Vec<CrawlRecord>,
    edges: Vec<LinkEdge>,
    total_edges: usize,
    query: CrawlGraphQuery,
) -> CrawlGraph {
    let max_nodes = query.max_nodes.max(1);
    let mut nodes = HashMap::new();

    for record in records.into_iter().filter(|record| {
        !query.internal_only || record.classification == UrlClassification::Internal
    }) {
        if nodes.len() >= max_nodes && !nodes.contains_key(&record.final_url) {
            continue;
        }
        nodes.insert(
            record.final_url.clone(),
            GraphNode {
                label: graph_label(&record.final_url),
                url: record.final_url,
                crawled: true,
                classification: Some(record.classification),
                status_code: record.status_code,
                depth: Some(record.depth),
                indexability: Some(record.indexability),
                inlink_count: record.inlink_count,
                outlink_count: record.outlink_count,
            },
        );
    }

    for edge in &edges {
        if nodes.len() >= max_nodes {
            break;
        }
        nodes
            .entry(edge.source_url.clone())
            .or_insert_with(|| placeholder_graph_node(&edge.source_url, &LinkType::Internal));
        if nodes.len() >= max_nodes {
            break;
        }
        nodes
            .entry(edge.target_url.clone())
            .or_insert_with(|| placeholder_graph_node(&edge.target_url, &edge.link_type));
    }

    let edges = edges
        .into_iter()
        .filter(|edge| nodes.contains_key(&edge.source_url) && nodes.contains_key(&edge.target_url))
        .collect::<Vec<_>>();
    let total_nodes = nodes.len();
    let mut nodes = nodes.into_values().collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        left.depth
            .cmp(&right.depth)
            .then_with(|| left.url.cmp(&right.url))
    });

    CrawlGraph {
        nodes,
        edges,
        total_nodes,
        total_edges,
    }
}

fn placeholder_graph_node(url: &str, link_type: &LinkType) -> GraphNode {
    GraphNode {
        url: url.to_string(),
        label: graph_label(url),
        crawled: false,
        classification: Some(match link_type {
            LinkType::Internal => UrlClassification::Internal,
            LinkType::External => UrlClassification::External,
        }),
        status_code: None,
        depth: None,
        indexability: None,
        inlink_count: 0,
        outlink_count: 0,
    }
}

fn graph_label(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        let mut label = parsed.path().trim_matches('/').to_string();
        if label.is_empty() {
            label = parsed.host_str().unwrap_or(url).to_string();
        }
        if let Some(query) = parsed.query() {
            label.push('?');
            label.push_str(query);
        }
        return label;
    }
    url.to_string()
}

fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CrawlRecord> {
    let redirect_chain_json: String = row.get("redirect_chain")?;
    let redirect_chain = serde_json::from_str(&redirect_chain_json).unwrap_or_default();
    let custom_extractions_json: String = row.get("custom_extractions")?;
    let custom_extractions = serde_json::from_str(&custom_extractions_json).unwrap_or_default();
    let classification: String = row.get("classification")?;
    let id: i64 = row.get("id")?;
    let response_time_ms: i64 = row.get("response_time_ms")?;
    let dns_lookup_time_ms: Option<i64> = row.get("dns_lookup_time_ms")?;
    let ttfb_ms: Option<i64> = row.get("ttfb_ms")?;
    let download_time_ms: Option<i64> = row.get("download_time_ms")?;
    let total_network_time_ms: Option<i64> = row.get("total_network_time_ms")?;
    let transfer_rate_bytes_per_sec: Option<i64> = row.get("transfer_rate_bytes_per_sec")?;
    let size_bytes: i64 = row.get("size_bytes")?;
    let depth: i64 = row.get("depth")?;
    let title_len: i64 = row.get("title_len")?;
    let meta_description_len: i64 = row.get("meta_description_len")?;
    let h1_len: i64 = row.get("h1_len")?;
    let h1_count: i64 = row.get("h1_count")?;
    let h2_len: i64 = row.get("h2_len")?;
    let h2_count: i64 = row.get("h2_count")?;
    let canonical_count: i64 = row.get("canonical_count")?;
    let word_count: i64 = row.get("word_count")?;
    let simhash: Option<String> = row.get("simhash")?;
    let near_duplicate_cluster_id: Option<i64> = row.get("near_duplicate_cluster_id")?;

    Ok(CrawlRecord {
        id: id as u64,
        url: row.get("url")?,
        final_url: row.get("final_url")?,
        classification: classification_from_str(&classification),
        in_sitemap: row.get("in_sitemap")?,
        status_code: row.get("status_code")?,
        status_text: row.get("status_text")?,
        content_type: row.get("content_type")?,
        indexability: row.get("indexability")?,
        indexability_status: row.get("indexability_status")?,
        response_time_ms: response_time_ms as u64,
        dns_lookup_time_ms: dns_lookup_time_ms.map(|value| value as u64),
        ttfb_ms: ttfb_ms.map(|value| value as u64),
        download_time_ms: download_time_ms.map(|value| value as u64),
        total_network_time_ms: total_network_time_ms.map(|value| value as u64),
        transfer_rate_bytes_per_sec: transfer_rate_bytes_per_sec.map(|value| value as u64),
        resolved_ip_count: row.get("resolved_ip_count")?,
        size_bytes: size_bytes as usize,
        response_hash: row.get("response_hash")?,
        depth: depth as usize,
        redirect_target: row.get("redirect_target")?,
        redirect_type: row.get("redirect_type")?,
        redirect_chain,
        title: row.get("title")?,
        title_len: title_len as usize,
        meta_description: row.get("meta_description")?,
        meta_description_len: meta_description_len as usize,
        meta_robots: row.get("meta_robots")?,
        x_robots_tag: row.get("x_robots_tag")?,
        h1: row.get("h1")?,
        h1_len: h1_len as usize,
        h1_count: h1_count as usize,
        h2: row.get("h2")?,
        h2_len: h2_len as usize,
        h2_count: h2_count as usize,
        canonical: row.get("canonical")?,
        canonical_count: canonical_count as usize,
        simhash: simhash.and_then(|value| value.parse::<u64>().ok()),
        word_count: word_count as usize,
        text_to_code_ratio: row.get("text_to_code_ratio")?,
        image_count: row.get("image_count")?,
        images_missing_alt: row.get("images_missing_alt")?,
        images_alt_too_long: row.get("images_alt_too_long")?,
        mixed_content_count: row.get("mixed_content_count")?,
        insecure_form_count: row.get("insecure_form_count")?,
        hsts_header: row.get("hsts_header")?,
        content_security_policy_header: row.get("content_security_policy_header")?,
        x_frame_options_header: row.get("x_frame_options_header")?,
        x_content_type_options_header: row.get("x_content_type_options_header")?,
        viewport: row.get("viewport")?,
        amphtml: row.get("amphtml")?,
        rel_next: row.get("rel_next")?,
        rel_prev: row.get("rel_prev")?,
        hreflang_count: row.get("hreflang_count")?,
        hreflang_invalid_count: row.get("hreflang_invalid_count")?,
        hreflang_missing_self_reference: row.get("hreflang_missing_self_reference")?,
        json_ld_count: row.get("json_ld_count")?,
        json_ld_invalid_count: row.get("json_ld_invalid_count")?,
        open_graph_count: row.get("open_graph_count")?,
        twitter_card_count: row.get("twitter_card_count")?,
        near_duplicate_cluster_id: near_duplicate_cluster_id.map(|value| value as u64),
        inlink_count: row.get("inlink_count")?,
        outlink_count: row.get("outlink_count")?,
        internal_outlink_count: row.get("internal_outlink_count")?,
        external_outlink_count: row.get("external_outlink_count")?,
        custom_extractions,
        error: row.get("error")?,
    })
}

fn link_edge_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LinkEdge> {
    let id: i64 = row.get("id")?;
    let link_type: String = row.get("link_type")?;
    let source_depth: i64 = row.get("source_depth")?;
    let target_depth: Option<i64> = row.get("target_depth")?;
    let source_position: i64 = row.get("source_position")?;
    let discovery_order: i64 = row.get("discovery_order")?;

    Ok(LinkEdge {
        id: id as u64,
        source_url: row.get("source_url")?,
        target_url: row.get("target_url")?,
        anchor_text: row.get("anchor_text")?,
        rel: row.get("rel")?,
        rel_nofollow: row.get("rel_nofollow")?,
        link_type: link_type_from_str(&link_type),
        source_status_code: row.get("source_status_code")?,
        target_status_code: row.get("target_status_code")?,
        source_depth: source_depth as usize,
        target_depth: target_depth.map(|value| value as usize),
        source_position: source_position.max(0) as u32,
        discovery_order: discovery_order as u64,
    })
}

fn anchor_text_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnchorTextRow> {
    let link_type: String = row.get("link_type")?;
    let link_count: i64 = row.get("link_count")?;
    let source_count: i64 = row.get("source_count")?;
    let nofollow_count: i64 = row.get("nofollow_count")?;

    Ok(AnchorTextRow {
        anchor_text: row.get("anchor_text")?,
        target_url: row.get("target_url")?,
        link_type: link_type_from_str(&link_type),
        link_count: link_count.max(0) as usize,
        source_count: source_count.max(0) as usize,
        nofollow_count: nofollow_count.max(0) as usize,
        first_source_url: row.get("first_source_url")?,
        target_status_code: row.get("target_status_code")?,
    })
}

fn sqlite_record_status(
    conn: &Connection,
    url: &str,
) -> Result<Option<(Option<u16>, usize)>, StorageError> {
    let value = conn
        .query_row(
            "SELECT status_code, depth FROM crawl_records WHERE final_url = ?1",
            [url],
            |row| {
                let status_code: Option<u16> = row.get(0)?;
                let depth: i64 = row.get(1)?;
                Ok((status_code, depth as usize))
            },
        )
        .optional()?;
    Ok(value)
}

fn update_sqlite_edge_statuses(
    conn: &Connection,
    record: &CrawlRecord,
) -> Result<(), StorageError> {
    conn.execute(
        "UPDATE link_edges
         SET source_status_code = ?1, source_depth = ?2
         WHERE source_url = ?3",
        params![record.status_code, record.depth as i64, &record.final_url],
    )?;
    conn.execute(
        "UPDATE link_edges
         SET target_status_code = ?1, target_depth = ?2
         WHERE target_url = ?3",
        params![record.status_code, record.depth as i64, &record.final_url],
    )?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, StorageError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn add_column_if_missing(
    conn: &Connection,
    column: &str,
    definition: &str,
) -> Result<(), StorageError> {
    add_table_column_if_missing(conn, "crawl_records", column, definition)
}

fn add_table_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), StorageError> {
    if !column_exists(conn, table, column)? {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

fn needs_duplicate_filter(view: &IssueView) -> bool {
    matches!(
        view,
        IssueView::TitleDuplicate
            | IssueView::MetaDuplicate
            | IssueView::H1Duplicate
            | IssueView::H2Duplicate
    )
}

fn query_filter_sql(query: &GridQuery) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut args = Vec::new();

    match query.view {
        IssueView::All => {}
        IssueView::Internal => clauses.push("classification = 'internal'".to_string()),
        IssueView::External => clauses.push("classification = 'external'".to_string()),
        IssueView::Status2xx => clauses.push("status_code >= 200 AND status_code < 300".to_string()),
        IssueView::Status3xx => clauses.push("(status_code >= 300 AND status_code < 400 OR redirect_chain != '[]')".to_string()),
        IssueView::Status4xx => clauses.push("status_code >= 400 AND status_code < 500".to_string()),
        IssueView::Status5xx => clauses.push("status_code >= 500".to_string()),
        IssueView::NoResponse => clauses.push("status_code IS NULL".to_string()),
        IssueView::TitleMissing => clauses.push("(title IS NULL OR trim(title) = '')".to_string()),
        IssueView::TitleDuplicate => {}
        IssueView::TitleTooShort => clauses.push("(title IS NOT NULL AND trim(title) != '' AND title_len < 30)".to_string()),
        IssueView::TitleTooLong => clauses.push("title_len > 60".to_string()),
        IssueView::MetaMissing => clauses.push("(meta_description IS NULL OR trim(meta_description) = '')".to_string()),
        IssueView::MetaDuplicate => {}
        IssueView::MetaTooShort => clauses.push("(meta_description IS NOT NULL AND trim(meta_description) != '' AND meta_description_len < 70)".to_string()),
        IssueView::MetaTooLong => clauses.push("meta_description_len > 160".to_string()),
        IssueView::H1Missing => clauses.push("(h1 IS NULL OR trim(h1) = '')".to_string()),
        IssueView::H1Duplicate => {}
        IssueView::H1TooLong => clauses.push("h1_len > 70".to_string()),
        IssueView::H2Missing => clauses.push("(h2 IS NULL OR trim(h2) = '')".to_string()),
        IssueView::H2Duplicate => {}
        IssueView::H2TooLong => clauses.push("h2_len > 70".to_string()),
        IssueView::TitleSameAsH1 => clauses.push(
            "(title IS NOT NULL AND h1 IS NOT NULL AND trim(title) != '' AND lower(trim(title)) = lower(trim(h1)))"
                .to_string(),
        ),
        IssueView::CanonicalMissing => clauses.push("(canonical IS NULL OR trim(canonical) = '')".to_string()),
        IssueView::CanonicalMultiple => clauses.push("canonical_count > 1".to_string()),
        IssueView::DirectivesNoindex => clauses.push("lower(indexability_status) LIKE '%noindex%'".to_string()),
        IssueView::ImagesMissingAlt => clauses.push("images_missing_alt > 0".to_string()),
        IssueView::ImagesAltTooLong => clauses.push("images_alt_too_long > 0".to_string()),
        IssueView::SecurityMixedContent => clauses.push("mixed_content_count > 0".to_string()),
        IssueView::SecurityInsecureForms => clauses.push("insecure_form_count > 0".to_string()),
        IssueView::SecurityMissingHsts => clauses
            .push("status_code >= 200 AND status_code < 300 AND final_url LIKE 'https://%' AND hsts_header = 0".to_string()),
        IssueView::SecurityMissingCsp => clauses
            .push("status_code >= 200 AND status_code < 300 AND lower(content_type) LIKE '%text/html%' AND content_security_policy_header = 0".to_string()),
        IssueView::SecurityMissingXFrameOptions => clauses
            .push("status_code >= 200 AND status_code < 300 AND lower(content_type) LIKE '%text/html%' AND x_frame_options_header = 0".to_string()),
        IssueView::SecurityMissingContentTypeOptions => clauses
            .push("status_code >= 200 AND status_code < 300 AND x_content_type_options_header = 0".to_string()),
        IssueView::MobileMissingViewport => clauses
            .push("status_code >= 200 AND status_code < 300 AND lower(content_type) LIKE '%text/html%' AND viewport = 0".to_string()),
        IssueView::HreflangInvalid => clauses.push("hreflang_invalid_count > 0".to_string()),
        IssueView::HreflangMissingSelfReference => {
            clauses.push("hreflang_missing_self_reference != 0".to_string())
        }
        IssueView::StructuredDataInvalid => clauses.push("json_ld_invalid_count > 0".to_string()),
        IssueView::NearDuplicate => clauses.push(
            "near_duplicate_cluster_id IS NOT NULL AND near_duplicate_cluster_id IN (
                SELECT near_duplicate_cluster_id
                FROM crawl_records
                WHERE near_duplicate_cluster_id IS NOT NULL
                GROUP BY near_duplicate_cluster_id
                HAVING COUNT(*) > 1
            )"
            .to_string(),
        ),
        IssueView::BrokenLinks => clauses.push("(error IS NOT NULL OR status_code IS NULL OR status_code >= 400)".to_string()),
        IssueView::SitemapOrphan => clauses
            .push("in_sitemap != 0 AND inlink_count = 0 AND classification = 'internal'".to_string()),
    }

    if let Some(search) = query.global_search.as_ref().map(|value| value.trim()) {
        if !search.is_empty() {
            clauses.push(
                "(lower(url) LIKE ? OR lower(final_url) LIKE ? OR lower(title) LIKE ? OR lower(meta_description) LIKE ? OR lower(meta_robots) LIKE ? OR lower(x_robots_tag) LIKE ? OR lower(h1) LIKE ? OR lower(h2) LIKE ? OR lower(canonical) LIKE ? OR lower(amphtml) LIKE ? OR lower(rel_next) LIKE ? OR lower(rel_prev) LIKE ? OR lower(response_hash) LIKE ? OR CAST(status_code AS TEXT) LIKE ? OR CAST(near_duplicate_cluster_id AS TEXT) LIKE ?)"
                    .to_string(),
            );
            let pattern = format!("%{}%", search.to_lowercase());
            for _ in 0..15 {
                args.push(pattern.clone());
            }
        }
    }

    if clauses.is_empty() {
        (String::new(), args)
    } else {
        (format!(" WHERE {}", clauses.join(" AND ")), args)
    }
}

fn link_edge_filter_sql(query: &LinkEdgeQuery) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut args = Vec::new();

    if query.internal_only {
        clauses.push("link_type = 'internal'".to_string());
    }
    match query.view {
        LinkEdgeView::All => {}
        LinkEdgeView::Internal => clauses.push("link_type = 'internal'".to_string()),
        LinkEdgeView::External => clauses.push("link_type = 'external'".to_string()),
        LinkEdgeView::Broken => {
            clauses.push("(target_status_code IS NULL OR target_status_code >= 400)".to_string())
        }
        LinkEdgeView::Nofollow => clauses.push("rel_nofollow != 0".to_string()),
    }
    if let Some(source_url) = query.source_url.as_ref().map(|value| value.trim()) {
        if !source_url.is_empty() {
            clauses.push("source_url = ?".to_string());
            args.push(source_url.to_string());
        }
    }
    if let Some(target_url) = query.target_url.as_ref().map(|value| value.trim()) {
        if !target_url.is_empty() {
            clauses.push("target_url = ?".to_string());
            args.push(target_url.to_string());
        }
    }
    if let Some(search) = query.global_search.as_ref().map(|value| value.trim()) {
        if !search.is_empty() {
            clauses.push(
                "(lower(source_url) LIKE ? OR lower(target_url) LIKE ? OR lower(anchor_text) LIKE ? OR lower(rel) LIKE ? OR lower(link_type) LIKE ? OR CAST(source_status_code AS TEXT) LIKE ? OR CAST(target_status_code AS TEXT) LIKE ? OR CAST(source_depth AS TEXT) LIKE ? OR CAST(target_depth AS TEXT) LIKE ? OR CAST(source_position AS TEXT) LIKE ?)"
                    .to_string(),
            );
            let pattern = format!("%{}%", search.to_lowercase());
            for _ in 0..10 {
                args.push(pattern.clone());
            }
        }
    }

    if clauses.is_empty() {
        (String::new(), args)
    } else {
        (format!(" WHERE {}", clauses.join(" AND ")), args)
    }
}

fn link_edge_sort_column(sort_by: Option<&str>) -> Option<&'static str> {
    match sort_by {
        Some("sourceUrl") => Some("source_url"),
        Some("targetUrl") => Some("target_url"),
        Some("anchorText") => Some("anchor_text"),
        Some("rel") => Some("rel"),
        Some("relNofollow") => Some("rel_nofollow"),
        Some("linkType") => Some("link_type"),
        Some("sourceStatusCode") => Some("source_status_code"),
        Some("targetStatusCode") => Some("target_status_code"),
        Some("sourceDepth") => Some("source_depth"),
        Some("targetDepth") => Some("target_depth"),
        Some("sourcePosition") => Some("source_position"),
        Some("discoveryOrder") => Some("discovery_order"),
        _ => None,
    }
}

fn anchor_text_sort_column(sort_by: Option<&str>) -> Option<&'static str> {
    match sort_by {
        Some("anchorText") => Some("anchor_text"),
        Some("targetUrl") => Some("target_url"),
        Some("linkType") => Some("link_type"),
        Some("linkCount") => Some("link_count"),
        Some("sourceCount") => Some("source_count"),
        Some("nofollowCount") => Some("nofollow_count"),
        Some("firstSourceUrl") => Some("first_source_url"),
        Some("targetStatusCode") => Some("target_status_code"),
        _ => None,
    }
}

fn sort_column(sort_by: Option<&str>) -> Option<&'static str> {
    match sort_by {
        Some("statusCode") => Some("status_code"),
        Some("responseTimeMs") => Some("response_time_ms"),
        Some("dnsLookupTimeMs") => Some("dns_lookup_time_ms"),
        Some("ttfbMs") => Some("ttfb_ms"),
        Some("downloadTimeMs") => Some("download_time_ms"),
        Some("totalNetworkTimeMs") => Some("total_network_time_ms"),
        Some("transferRateBytesPerSec") => Some("transfer_rate_bytes_per_sec"),
        Some("resolvedIpCount") => Some("resolved_ip_count"),
        Some("inSitemap") => Some("in_sitemap"),
        Some("sizeBytes") => Some("size_bytes"),
        Some("depth") => Some("depth"),
        Some("titleLen") => Some("title_len"),
        Some("metaDescription") => Some("meta_description"),
        Some("metaDescriptionLen") => Some("meta_description_len"),
        Some("h1") => Some("h1"),
        Some("h1Len") => Some("h1_len"),
        Some("h1Count") => Some("h1_count"),
        Some("h2") => Some("h2"),
        Some("h2Len") => Some("h2_len"),
        Some("h2Count") => Some("h2_count"),
        Some("canonicalCount") => Some("canonical_count"),
        Some("wordCount") => Some("word_count"),
        Some("textToCodeRatio") => Some("text_to_code_ratio"),
        Some("imageCount") => Some("image_count"),
        Some("imagesMissingAlt") => Some("images_missing_alt"),
        Some("imagesAltTooLong") => Some("images_alt_too_long"),
        Some("mixedContentCount") => Some("mixed_content_count"),
        Some("insecureFormCount") => Some("insecure_form_count"),
        Some("hreflangCount") => Some("hreflang_count"),
        Some("hreflangInvalidCount") => Some("hreflang_invalid_count"),
        Some("jsonLdCount") => Some("json_ld_count"),
        Some("jsonLdInvalidCount") => Some("json_ld_invalid_count"),
        Some("openGraphCount") => Some("open_graph_count"),
        Some("twitterCardCount") => Some("twitter_card_count"),
        Some("nearDuplicateClusterId") => Some("near_duplicate_cluster_id"),
        Some("inlinkCount") => Some("inlink_count"),
        Some("outlinkCount") => Some("outlink_count"),
        Some("title") => Some("title"),
        Some("finalUrl") => Some("final_url"),
        _ => None,
    }
}

fn count_query(conn: &Connection, sql: &str, args: &[String]) -> Result<usize, StorageError> {
    let mut stmt = conn.prepare(sql)?;
    let value = stmt.query_row(rusqlite::params_from_iter(args), |row| {
        row.get::<_, i64>(0).map(|count| count as usize)
    })?;
    Ok(value)
}

fn summary_count(conn: &Connection, sql: &str) -> Result<usize, StorageError> {
    let value = conn.query_row(sql, [], |row| {
        row.get::<_, i64>(0).map(|count| count as usize)
    })?;
    Ok(value)
}

fn query_records_with_args(
    conn: &Connection,
    sql: &str,
    args: &[String],
) -> Result<Vec<CrawlRecord>, StorageError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), record_from_row)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn query_link_edges_with_args(
    conn: &Connection,
    sql: &str,
    args: &[String],
) -> Result<Vec<LinkEdge>, StorageError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), link_edge_from_row)?;
    let mut edges = Vec::new();
    for row in rows {
        edges.push(row?);
    }
    Ok(edges)
}

fn query_anchor_texts_with_args(
    conn: &Connection,
    sql: &str,
    args: &[String],
) -> Result<Vec<AnchorTextRow>, StorageError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), anchor_text_from_row)?;
    let mut anchor_rows = Vec::new();
    for row in rows {
        anchor_rows.push(row?);
    }
    Ok(anchor_rows)
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

fn cluster_counts(values: impl Iterator<Item = u64>) -> HashMap<u64, usize> {
    let mut counts = HashMap::new();
    for value in values {
        *counts.entry(value).or_insert(0) += 1;
    }
    counts
}

fn normalize_text_key(value: &str) -> String {
    compact_text(value).to_lowercase()
}

fn compact_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn matches_view(
    row: &CrawlRecord,
    view: &IssueView,
    title_counts: &HashMap<String, usize>,
    meta_counts: &HashMap<String, usize>,
    h1_counts: &HashMap<String, usize>,
    h2_counts: &HashMap<String, usize>,
    near_duplicate_counts: &HashMap<u64, usize>,
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
        IssueView::H1Missing => row.h1.as_deref().unwrap_or("").trim().is_empty(),
        IssueView::H1Duplicate => {
            row.h1
                .as_deref()
                .map(normalize_text_key)
                .and_then(|key| h1_counts.get(&key).copied())
                .unwrap_or(0)
                > 1
        }
        IssueView::H1TooLong => row.h1_len > 70,
        IssueView::H2Missing => row.h2.as_deref().unwrap_or("").trim().is_empty(),
        IssueView::H2Duplicate => {
            row.h2
                .as_deref()
                .map(normalize_text_key)
                .and_then(|key| h2_counts.get(&key).copied())
                .unwrap_or(0)
                > 1
        }
        IssueView::H2TooLong => row.h2_len > 70,
        IssueView::TitleSameAsH1 => {
            let title = row.title.as_deref().unwrap_or("").trim();
            let h1 = row.h1.as_deref().unwrap_or("").trim();
            !title.is_empty() && title.eq_ignore_ascii_case(h1)
        }
        IssueView::CanonicalMissing => row.canonical.as_deref().unwrap_or("").trim().is_empty(),
        IssueView::CanonicalMultiple => row.canonical_count > 1,
        IssueView::DirectivesNoindex => row.indexability_status.to_lowercase().contains("noindex"),
        IssueView::ImagesMissingAlt => row.images_missing_alt > 0,
        IssueView::ImagesAltTooLong => row.images_alt_too_long > 0,
        IssueView::SecurityMixedContent => row.mixed_content_count > 0,
        IssueView::SecurityInsecureForms => row.insecure_form_count > 0,
        IssueView::SecurityMissingHsts => {
            is_success_record(row) && row.final_url.starts_with("https://") && !row.hsts_header
        }
        IssueView::SecurityMissingCsp => {
            is_success_html_record(row) && !row.content_security_policy_header
        }
        IssueView::SecurityMissingXFrameOptions => {
            is_success_html_record(row) && !row.x_frame_options_header
        }
        IssueView::SecurityMissingContentTypeOptions => {
            is_success_record(row) && !row.x_content_type_options_header
        }
        IssueView::MobileMissingViewport => is_success_html_record(row) && !row.viewport,
        IssueView::HreflangInvalid => row.hreflang_invalid_count > 0,
        IssueView::HreflangMissingSelfReference => row.hreflang_missing_self_reference,
        IssueView::StructuredDataInvalid => row.json_ld_invalid_count > 0,
        IssueView::NearDuplicate => {
            row.near_duplicate_cluster_id
                .and_then(|cluster_id| near_duplicate_counts.get(&cluster_id).copied())
                .unwrap_or(0)
                > 1
        }
        IssueView::BrokenLinks => {
            row.error.is_some()
                || matches!(row.status_code, Some(code) if code >= 400)
                || row.status_code.is_none()
        }
        IssueView::SitemapOrphan => {
            row.in_sitemap
                && row.inlink_count == 0
                && row.classification == UrlClassification::Internal
        }
    }
}

fn is_success_record(row: &CrawlRecord) -> bool {
    matches!(row.status_code, Some(code) if (200..300).contains(&code))
}

fn is_success_html_record(row: &CrawlRecord) -> bool {
    is_success_record(row)
        && row
            .content_type
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains("text/html"))
            .unwrap_or(false)
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
            .h2
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .meta_robots
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .x_robots_tag
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .canonical
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .amphtml
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .rel_next
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .rel_prev
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .response_hash
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .status_code
            .map(|code| code.to_string().contains(search))
            .unwrap_or(false)
        || row
            .near_duplicate_cluster_id
            .map(|cluster_id| cluster_id.to_string().contains(search))
            .unwrap_or(false)
}

fn link_edge_matches_search(edge: &LinkEdge, search: &str) -> bool {
    edge.source_url.to_lowercase().contains(search)
        || edge.target_url.to_lowercase().contains(search)
        || edge.anchor_text.to_lowercase().contains(search)
        || edge.rel.to_lowercase().contains(search)
        || link_type_to_str(&edge.link_type).contains(search)
        || edge
            .source_status_code
            .map(|code| code.to_string().contains(search))
            .unwrap_or(false)
        || edge
            .target_status_code
            .map(|code| code.to_string().contains(search))
            .unwrap_or(false)
        || edge.source_depth.to_string().contains(search)
        || edge
            .target_depth
            .map(|depth| depth.to_string().contains(search))
            .unwrap_or(false)
        || edge.source_position.to_string().contains(search)
        || edge.discovery_order.to_string().contains(search)
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
        "dnsLookupTimeMs" => left.dns_lookup_time_ms.cmp(&right.dns_lookup_time_ms),
        "ttfbMs" => left.ttfb_ms.cmp(&right.ttfb_ms),
        "downloadTimeMs" => left.download_time_ms.cmp(&right.download_time_ms),
        "totalNetworkTimeMs" => left.total_network_time_ms.cmp(&right.total_network_time_ms),
        "transferRateBytesPerSec" => left
            .transfer_rate_bytes_per_sec
            .cmp(&right.transfer_rate_bytes_per_sec),
        "resolvedIpCount" => left.resolved_ip_count.cmp(&right.resolved_ip_count),
        "inSitemap" => left.in_sitemap.cmp(&right.in_sitemap),
        "sizeBytes" => left.size_bytes.cmp(&right.size_bytes),
        "depth" => left.depth.cmp(&right.depth),
        "titleLen" => left.title_len.cmp(&right.title_len),
        "metaDescription" => left.meta_description.cmp(&right.meta_description),
        "metaDescriptionLen" => left.meta_description_len.cmp(&right.meta_description_len),
        "h1" => left.h1.cmp(&right.h1),
        "h1Len" => left.h1_len.cmp(&right.h1_len),
        "h1Count" => left.h1_count.cmp(&right.h1_count),
        "h2" => left.h2.cmp(&right.h2),
        "h2Len" => left.h2_len.cmp(&right.h2_len),
        "h2Count" => left.h2_count.cmp(&right.h2_count),
        "canonicalCount" => left.canonical_count.cmp(&right.canonical_count),
        "wordCount" => left.word_count.cmp(&right.word_count),
        "textToCodeRatio" => left
            .text_to_code_ratio
            .partial_cmp(&right.text_to_code_ratio)
            .unwrap_or(Ordering::Equal),
        "imageCount" => left.image_count.cmp(&right.image_count),
        "imagesMissingAlt" => left.images_missing_alt.cmp(&right.images_missing_alt),
        "imagesAltTooLong" => left.images_alt_too_long.cmp(&right.images_alt_too_long),
        "mixedContentCount" => left.mixed_content_count.cmp(&right.mixed_content_count),
        "insecureFormCount" => left.insecure_form_count.cmp(&right.insecure_form_count),
        "hreflangCount" => left.hreflang_count.cmp(&right.hreflang_count),
        "hreflangInvalidCount" => left
            .hreflang_invalid_count
            .cmp(&right.hreflang_invalid_count),
        "jsonLdCount" => left.json_ld_count.cmp(&right.json_ld_count),
        "jsonLdInvalidCount" => left.json_ld_invalid_count.cmp(&right.json_ld_invalid_count),
        "openGraphCount" => left.open_graph_count.cmp(&right.open_graph_count),
        "twitterCardCount" => left.twitter_card_count.cmp(&right.twitter_card_count),
        "nearDuplicateClusterId" => left
            .near_duplicate_cluster_id
            .cmp(&right.near_duplicate_cluster_id),
        "inlinkCount" => left.inlink_count.cmp(&right.inlink_count),
        "outlinkCount" => left.outlink_count.cmp(&right.outlink_count),
        "title" => left.title.cmp(&right.title),
        "finalUrl" => left.final_url.cmp(&right.final_url),
        _ => left.id.cmp(&right.id),
    }
}

fn sort_link_edges(edges: &mut [LinkEdge], sort_by: &str, sort_dir: &SortDirection) {
    edges.sort_by(|left, right| {
        let ordering = compare_link_edges(left, right, sort_by);
        match sort_dir {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
}

fn compare_link_edges(left: &LinkEdge, right: &LinkEdge, sort_by: &str) -> Ordering {
    match sort_by {
        "sourceUrl" => left.source_url.cmp(&right.source_url),
        "targetUrl" => left.target_url.cmp(&right.target_url),
        "anchorText" => left.anchor_text.cmp(&right.anchor_text),
        "rel" => left.rel.cmp(&right.rel),
        "relNofollow" => left.rel_nofollow.cmp(&right.rel_nofollow),
        "linkType" => link_type_to_str(&left.link_type).cmp(link_type_to_str(&right.link_type)),
        "sourceStatusCode" => left.source_status_code.cmp(&right.source_status_code),
        "targetStatusCode" => left.target_status_code.cmp(&right.target_status_code),
        "sourceDepth" => left.source_depth.cmp(&right.source_depth),
        "targetDepth" => left.target_depth.cmp(&right.target_depth),
        "sourcePosition" => left.source_position.cmp(&right.source_position),
        "discoveryOrder" => left.discovery_order.cmp(&right.discovery_order),
        _ => left.id.cmp(&right.id),
    }
}

fn sort_anchor_text_rows(rows: &mut [AnchorTextRow], sort_by: &str, sort_dir: &SortDirection) {
    rows.sort_by(|left, right| {
        let ordering = compare_anchor_text_rows(left, right, sort_by)
            .then_with(|| left.anchor_text.cmp(&right.anchor_text))
            .then_with(|| left.target_url.cmp(&right.target_url));
        match sort_dir {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
}

fn compare_anchor_text_rows(
    left: &AnchorTextRow,
    right: &AnchorTextRow,
    sort_by: &str,
) -> Ordering {
    match sort_by {
        "anchorText" => left.anchor_text.cmp(&right.anchor_text),
        "targetUrl" => left.target_url.cmp(&right.target_url),
        "linkType" => link_type_to_str(&left.link_type).cmp(link_type_to_str(&right.link_type)),
        "linkCount" => left.link_count.cmp(&right.link_count),
        "sourceCount" => left.source_count.cmp(&right.source_count),
        "nofollowCount" => left.nofollow_count.cmp(&right.nofollow_count),
        "firstSourceUrl" => left.first_source_url.cmp(&right.first_source_url),
        "targetStatusCode" => left.target_status_code.cmp(&right.target_status_code),
        _ => left.link_count.cmp(&right.link_count),
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

    #[test]
    fn memory_store_tracks_link_edges_and_graph_nodes() {
        let store = MemoryStore::new();
        let mut source = CrawlRecord::pending("https://example.com/a".to_string(), 0);
        source.status_code = Some(200);
        store.upsert(source);

        store.add_link_edge(test_edge(
            "https://example.com/a",
            "https://example.com/b",
            LinkType::Internal,
        ));
        let mut target = CrawlRecord::pending("https://example.com/b".to_string(), 1);
        target.status_code = Some(404);
        store.upsert(target);

        let edges = store.link_edges(LinkEdgeQuery::default());
        assert_eq!(edges.total, 1);
        assert_eq!(edges.edges[0].source_status_code, Some(200));
        assert_eq!(edges.edges[0].target_status_code, Some(404));
        assert_eq!(edges.edges[0].target_depth, Some(1));
        assert_eq!(edges.edges[0].source_position, 1);

        let graph = store.crawl_graph(CrawlGraphQuery::default());
        assert_eq!(graph.total_edges, 1);
        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.nodes.iter().all(|node| node.crawled));
    }

    #[test]
    fn memory_link_edges_support_search_and_sort() {
        let store = MemoryStore::new();
        let mut first = test_edge(
            "https://example.com/a",
            "https://example.com/z",
            LinkType::Internal,
        );
        first.anchor_text = "Zeta target".to_string();
        first.source_position = 2;
        store.add_link_edge(first);

        let mut second = test_edge(
            "https://example.com/a",
            "https://example.com/b",
            LinkType::Internal,
        );
        second.anchor_text = "Beta target".to_string();
        second.source_position = 8;
        store.add_link_edge(second);

        let response = store.link_edges(LinkEdgeQuery {
            global_search: Some("target".to_string()),
            sort_by: Some("sourcePosition".to_string()),
            sort_dir: SortDirection::Desc,
            ..LinkEdgeQuery::default()
        });

        assert_eq!(response.total, 2);
        assert_eq!(response.edges[0].source_position, 8);
        assert_eq!(response.edges[1].source_position, 2);
    }

    #[test]
    fn memory_store_aggregates_anchor_texts() {
        let store = MemoryStore::new();
        let mut first = test_edge(
            "https://example.com/a",
            "https://example.com/b",
            LinkType::Internal,
        );
        first.anchor_text = "Read more".to_string();
        first.rel_nofollow = true;
        store.add_link_edge(first);

        let mut second = test_edge(
            "https://example.com/c",
            "https://example.com/b",
            LinkType::Internal,
        );
        second.anchor_text = "Read more".to_string();
        second.rel = String::new();
        second.rel_nofollow = false;
        store.add_link_edge(second);

        let response = store.anchor_texts(LinkEdgeQuery {
            sort_by: Some("linkCount".to_string()),
            sort_dir: SortDirection::Desc,
            ..LinkEdgeQuery::default()
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].anchor_text, "Read more");
        assert_eq!(response.rows[0].link_count, 2);
        assert_eq!(response.rows[0].source_count, 2);
        assert_eq!(response.rows[0].nofollow_count, 1);
    }

    #[test]
    fn query_filters_near_duplicate_clusters() {
        let store = MemoryStore::new();
        for url in ["https://example.com/a", "https://example.com/b"] {
            let mut record = CrawlRecord::pending(url.to_string(), 0);
            record.status_code = Some(200);
            record.near_duplicate_cluster_id = Some(7);
            store.upsert(record);
        }

        let response = store.query(GridQuery {
            view: IssueView::NearDuplicate,
            ..GridQuery::default()
        });

        assert_eq!(response.total, 2);
        assert_eq!(response.summary.near_duplicates, 2);
    }

    #[test]
    fn sqlite_store_persists_and_queries_records() {
        let store = SqliteStore::in_memory().unwrap();
        let mut record = CrawlRecord::pending("https://example.com/a".to_string(), 0);
        record.status_code = Some(200);
        record.status_text = "OK".to_string();
        record.title = Some("A useful title for sqlite storage".to_string());
        record.title_len = 33;
        record.meta_description = Some(
            "A useful description long enough to behave like a normal result row.".to_string(),
        );
        record.meta_description_len = 68;
        record.custom_extractions = vec![CustomExtractionValue {
            name: "heading".to_string(),
            values: vec!["Example".to_string()],
        }];
        record.response_hash = Some("abc123".to_string());
        record.simhash = Some(u64::MAX);
        record.word_count = 250;
        record.text_to_code_ratio = 0.42;
        record.near_duplicate_cluster_id = Some(3);
        store.upsert(record);

        let response = store.query(GridQuery {
            view: IssueView::Status2xx,
            sort_by: Some("finalUrl".to_string()),
            ..GridQuery::default()
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].final_url, "https://example.com/a");
        assert_eq!(response.rows[0].custom_extractions[0].name, "heading");
        assert_eq!(response.rows[0].simhash, Some(u64::MAX));
        assert_eq!(response.rows[0].word_count, 250);
    }

    #[test]
    fn sqlite_inlinks_update_existing_record() {
        let store = SqliteStore::in_memory().unwrap();
        let record = CrawlRecord::pending("https://example.com/a".to_string(), 0);
        store.upsert(record);
        store.add_inlink("https://example.com/a");

        let rows = store.records();
        assert_eq!(rows[0].inlink_count, 1);
    }

    #[test]
    fn sqlite_store_tracks_link_edges_and_graph_nodes() {
        let store = SqliteStore::in_memory().unwrap();
        let mut source = CrawlRecord::pending("https://example.com/a".to_string(), 0);
        source.status_code = Some(200);
        store.upsert(source);

        store.add_link_edge(test_edge(
            "https://example.com/a",
            "https://example.com/b",
            LinkType::Internal,
        ));
        let mut target = CrawlRecord::pending("https://example.com/b".to_string(), 1);
        target.status_code = Some(404);
        store.upsert(target);

        let edges = store.link_edges(LinkEdgeQuery::default());
        assert_eq!(edges.total, 1);
        assert_eq!(edges.edges[0].source_status_code, Some(200));
        assert_eq!(edges.edges[0].target_status_code, Some(404));
        assert_eq!(edges.edges[0].target_depth, Some(1));
        assert_eq!(edges.edges[0].source_position, 1);

        let graph = store.crawl_graph(CrawlGraphQuery::default());
        assert_eq!(graph.total_edges, 1);
        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.nodes.iter().all(|node| node.crawled));
    }

    #[test]
    fn sqlite_link_edges_support_search_and_sort() {
        let store = SqliteStore::in_memory().unwrap();
        let mut first = test_edge(
            "https://example.com/a",
            "https://example.com/z",
            LinkType::Internal,
        );
        first.anchor_text = "Zeta target".to_string();
        first.source_position = 2;
        store.add_link_edge(first);

        let mut second = test_edge(
            "https://example.com/a",
            "https://example.com/b",
            LinkType::Internal,
        );
        second.anchor_text = "Beta target".to_string();
        second.source_position = 8;
        store.add_link_edge(second);

        let response = store.link_edges(LinkEdgeQuery {
            global_search: Some("target".to_string()),
            sort_by: Some("sourcePosition".to_string()),
            sort_dir: SortDirection::Desc,
            ..LinkEdgeQuery::default()
        });

        assert_eq!(response.total, 2);
        assert_eq!(response.edges[0].source_position, 8);
        assert_eq!(response.edges[1].source_position, 2);
    }

    #[test]
    fn sqlite_store_aggregates_anchor_texts() {
        let store = SqliteStore::in_memory().unwrap();
        let mut first = test_edge(
            "https://example.com/a",
            "https://example.com/b",
            LinkType::Internal,
        );
        first.anchor_text = "Read more".to_string();
        first.rel_nofollow = true;
        store.add_link_edge(first);

        let mut second = test_edge(
            "https://example.com/c",
            "https://example.com/b",
            LinkType::Internal,
        );
        second.anchor_text = "Read more".to_string();
        second.rel = String::new();
        second.rel_nofollow = false;
        store.add_link_edge(second);

        let response = store.anchor_texts(LinkEdgeQuery {
            sort_by: Some("linkCount".to_string()),
            sort_dir: SortDirection::Desc,
            ..LinkEdgeQuery::default()
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].anchor_text, "Read more");
        assert_eq!(response.rows[0].link_count, 2);
        assert_eq!(response.rows[0].source_count, 2);
        assert_eq!(response.rows[0].nofollow_count, 1);
    }

    fn test_edge(source_url: &str, target_url: &str, link_type: LinkType) -> LinkEdge {
        LinkEdge {
            id: 0,
            source_url: source_url.to_string(),
            target_url: target_url.to_string(),
            anchor_text: "Target".to_string(),
            rel: "nofollow".to_string(),
            rel_nofollow: true,
            link_type,
            source_status_code: None,
            target_status_code: None,
            source_depth: 0,
            target_depth: None,
            source_position: 1,
            discovery_order: 0,
        }
    }
}
