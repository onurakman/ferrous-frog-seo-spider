use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params, types::Value};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::sync::{Mutex, MutexGuard};
use thiserror::Error;

const IMAGE_ASSET_OVERSIZE_BYTES: u64 = 200 * 1024;
const SUCCESS_HTML_SQL: &str =
    "status_code >= 200 AND status_code < 300 AND lower(content_type) LIKE '%text/html%'";
const NO_RESPONSE_SQL: &str = "status_code IS NULL AND error IS NOT NULL AND status_text != 'Blocked by robots.txt' AND error != 'Blocked by robots.txt'";

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
#[derive(Default)]
pub enum IssueView {
    #[default]
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
    TitlePixelTooNarrow,
    TitlePixelTooWide,
    MetaMissing,
    MetaDuplicate,
    MetaTooShort,
    MetaTooLong,
    MetaPixelTooNarrow,
    MetaPixelTooWide,
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
    HreflangMissingReturnLink,
    HreflangNonCanonicalTarget,
    StructuredDataInvalid,
    StructuredDataWarning,
    HtmlDeprecatedTags,
    HtmlDuplicateIds,
    RenderedDomChanged,
    NearDuplicate,
    BrokenLinks,
    SitemapOrphan,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedirectHop {
    pub url: String,
    pub status_code: u16,
    pub location: Option<String>,
    pub dns_lookup_time_ms: Option<u64>,
    pub tcp_connect_time_ms: Option<u64>,
    pub tls_handshake_time_ms: Option<u64>,
    pub ttfb_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomExtractionValue {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CustomSearchSource {
    RawHtml,
    RenderedHtml,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomSearchValue {
    pub name: String,
    pub source: CustomSearchSource,
    pub matched: bool,
    pub match_count: usize,
    pub snippets: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HreflangLink {
    pub hreflang: String,
    pub url: String,
    pub valid: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StructuredDataIssue {
    pub severity: String,
    pub message: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlRecord {
    pub id: u64,
    pub storage_key: String,
    pub url: String,
    pub final_url: String,
    pub list_position: Option<u32>,
    pub list_duplicate_index: u32,
    pub classification: UrlClassification,
    pub in_sitemap: bool,
    pub status_code: Option<u16>,
    pub status_text: String,
    pub content_type: Option<String>,
    pub indexability: String,
    pub indexability_status: String,
    pub response_time_ms: u64,
    pub dns_lookup_time_ms: Option<u64>,
    pub tcp_connect_time_ms: Option<u64>,
    pub tls_handshake_time_ms: Option<u64>,
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
    pub title_pixel_width: u32,
    pub meta_description: Option<String>,
    pub meta_description_len: usize,
    pub meta_description_pixel_width: u32,
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
    pub hreflang_links: Vec<HreflangLink>,
    pub json_ld_count: u32,
    pub json_ld_invalid_count: u32,
    pub structured_data_error_count: u32,
    pub structured_data_warning_count: u32,
    pub structured_data_issues: Vec<StructuredDataIssue>,
    pub open_graph_count: u32,
    pub twitter_card_count: u32,
    #[serde(default)]
    pub deprecated_html_tag_count: u32,
    #[serde(default)]
    pub duplicate_id_count: u32,
    #[serde(default)]
    pub js_rendered: bool,
    #[serde(default)]
    pub rendered_dom_changed: bool,
    #[serde(default)]
    pub rendered_word_count_delta: i32,
    #[serde(default)]
    pub rendered_link_count_delta: i32,
    pub near_duplicate_cluster_id: Option<u64>,
    pub inlink_count: u32,
    pub first_inlink_source_url: Option<String>,
    pub first_inlink_anchor_text: Option<String>,
    pub first_inlink_source_position: Option<u32>,
    pub outlink_count: u32,
    pub internal_outlink_count: u32,
    pub external_outlink_count: u32,
    pub custom_extractions: Vec<CustomExtractionValue>,
    #[serde(default)]
    pub custom_searches: Vec<CustomSearchValue>,
    #[serde(default)]
    pub search_console_clicks: Option<f64>,
    #[serde(default)]
    pub search_console_impressions: Option<f64>,
    #[serde(default)]
    pub search_console_ctr: Option<f64>,
    #[serde(default)]
    pub search_console_average_position: Option<f64>,
    pub error: Option<String>,
}

impl CrawlRecord {
    pub fn pending(url: String, depth: usize) -> Self {
        Self {
            id: 0,
            storage_key: url.clone(),
            final_url: url.clone(),
            url,
            list_position: None,
            list_duplicate_index: 0,
            classification: UrlClassification::Internal,
            in_sitemap: false,
            status_code: None,
            status_text: "Pending".to_string(),
            content_type: None,
            indexability: "Unknown".to_string(),
            indexability_status: "Not fetched".to_string(),
            response_time_ms: 0,
            dns_lookup_time_ms: None,
            tcp_connect_time_ms: None,
            tls_handshake_time_ms: None,
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
            title_pixel_width: 0,
            meta_description: None,
            meta_description_len: 0,
            meta_description_pixel_width: 0,
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
            hreflang_links: Vec::new(),
            json_ld_count: 0,
            json_ld_invalid_count: 0,
            structured_data_error_count: 0,
            structured_data_warning_count: 0,
            structured_data_issues: Vec::new(),
            open_graph_count: 0,
            twitter_card_count: 0,
            deprecated_html_tag_count: 0,
            duplicate_id_count: 0,
            js_rendered: false,
            rendered_dom_changed: false,
            rendered_word_count_delta: 0,
            rendered_link_count_delta: 0,
            near_duplicate_cluster_id: None,
            inlink_count: 0,
            first_inlink_source_url: None,
            first_inlink_anchor_text: None,
            first_inlink_source_position: None,
            outlink_count: 0,
            internal_outlink_count: 0,
            external_outlink_count: 0,
            custom_extractions: Vec::new(),
            custom_searches: Vec::new(),
            search_console_clicks: None,
            search_console_impressions: None,
            search_console_ctr: None,
            search_console_average_position: None,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchConsoleMetricRow {
    pub url: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub average_position: f64,
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
    pub structured_data_warnings: usize,
    pub deprecated_html_tags: usize,
    pub duplicate_ids: usize,
    pub rendered_dom_changed: usize,
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
    #[serde(default)]
    pub segment_pattern: Option<String>,
    #[serde(default)]
    pub segment_regex: bool,
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
            segment_pattern: None,
            segment_regex: false,
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
pub struct ImageAsset {
    pub id: u64,
    pub page_url: String,
    pub image_url: String,
    pub alt_text: Option<String>,
    pub alt_len: u32,
    pub missing_alt: bool,
    pub alt_too_long: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source_position: u32,
    pub size_bytes: Option<u64>,
    pub oversized: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAssetQuery {
    pub offset: usize,
    pub limit: usize,
    #[serde(default)]
    pub global_search: Option<String>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub sort_dir: SortDirection,
    #[serde(default)]
    pub page_url: Option<String>,
    #[serde(default)]
    pub oversized_only: bool,
    #[serde(default)]
    pub missing_alt_only: bool,
}

impl Default for ImageAssetQuery {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 1_000,
            global_search: None,
            sort_by: None,
            sort_dir: SortDirection::Asc,
            page_url: None,
            oversized_only: false,
            missing_alt_only: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAssetResponse {
    pub images: Vec<ImageAsset>,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CrawlFrontierItem {
    pub url: String,
    pub depth: usize,
    pub from_sitemap: bool,
    pub storage_key: String,
    pub list_position: Option<u32>,
    pub list_duplicate_index: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CrawlFrontierState {
    pub queued: Vec<CrawlFrontierItem>,
    pub seen: Vec<String>,
    pub crawled: usize,
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
pub struct SitemapValidationQuery {
    pub offset: usize,
    pub limit: usize,
    #[serde(default)]
    pub global_search: Option<String>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub sort_dir: SortDirection,
}

impl Default for SitemapValidationQuery {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 1_000,
            global_search: None,
            sort_by: None,
            sort_dir: SortDirection::Desc,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SitemapValidationRow {
    pub url: String,
    pub final_url: String,
    pub status_code: Option<u16>,
    pub status_text: String,
    pub indexability: String,
    pub indexability_status: String,
    pub inlink_count: u32,
    pub redirect_target: Option<String>,
    pub canonical: Option<String>,
    pub issue_count: usize,
    pub severity: Severity,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SitemapValidationResponse {
    pub rows: Vec<SitemapValidationRow>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlPathQuery {
    pub target_url: String,
    #[serde(default = "default_crawl_path_max_edges")]
    pub max_edges: usize,
    #[serde(default = "default_true")]
    pub internal_only: bool,
}

impl Default for CrawlPathQuery {
    fn default() -> Self {
        Self {
            target_url: String::new(),
            max_edges: default_crawl_path_max_edges(),
            internal_only: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlPathResponse {
    pub target_url: String,
    pub found: bool,
    pub truncated: bool,
    pub explored_edges: usize,
    pub steps: Vec<LinkEdge>,
}

fn default_crawl_path_max_edges() -> usize {
    100_000
}

fn default_true() -> bool {
    true
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
    fn add_image_assets(&self, page_url: &str, images: Vec<ImageAsset>);
    fn merge_search_console_metrics(&self, metrics: Vec<SearchConsoleMetricRow>) -> usize;
    fn records(&self) -> Vec<CrawlRecord>;
    fn query(&self, query: GridQuery) -> GridResponse;
    fn link_edges(&self, query: LinkEdgeQuery) -> LinkEdgeResponse;
    fn image_assets(&self, query: ImageAssetQuery) -> ImageAssetResponse;
    fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse;
    fn save_frontier_state(&self, state: CrawlFrontierState);
    fn load_frontier_state(&self) -> Option<CrawlFrontierState>;
    fn clear_frontier_state(&self);

    fn sitemap_validation(&self, query: SitemapValidationQuery) -> SitemapValidationResponse {
        build_sitemap_validation_report(&self.records(), query)
    }

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

    fn crawl_path(&self, query: CrawlPathQuery) -> CrawlPathResponse {
        let max_edges = query.max_edges.max(1);
        let edges = self.link_edges(LinkEdgeQuery {
            limit: max_edges,
            sort_by: Some("discoveryOrder".to_string()),
            sort_dir: SortDirection::Asc,
            internal_only: query.internal_only,
            ..LinkEdgeQuery::default()
        });
        build_crawl_path(
            edges.edges,
            query.target_url,
            edges.total,
            max_edges,
            query.internal_only,
        )
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
    image_assets: Vec<ImageAsset>,
    frontier_state: Option<CrawlFrontierState>,
    next_id: u64,
    next_edge_id: u64,
    next_image_asset_id: u64,
}

#[derive(Clone, Debug)]
struct FirstInlinkSource {
    source_url: String,
    anchor_text: String,
    source_position: u32,
    discovery_order: u64,
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
        if record.storage_key.trim().is_empty() {
            record.storage_key = record.final_url.clone();
        }
        let key = record.storage_key.clone();
        record.inlink_count =
            memory_inlink_count_for_record(&inner, &record).unwrap_or(record.inlink_count);

        if let Some(index) = inner.url_to_index.get(&key).copied() {
            record.id = inner.records[index].id;
            inner.records[index] = record.clone();
            update_memory_edge_statuses(&mut inner.link_edges, &record);
            let mut returned = record;
            apply_first_inlink_sources(std::slice::from_mut(&mut returned), &inner.link_edges);
            return returned;
        }

        inner.next_id += 1;
        record.id = inner.next_id;
        let index = inner.records.len();
        inner.url_to_index.insert(key, index);
        inner.records.push(record.clone());
        update_memory_edge_statuses(&mut inner.link_edges, &record);
        apply_first_inlink_sources(std::slice::from_mut(&mut record), &inner.link_edges);
        record
    }

    pub fn add_inlink(&self, target_url: &str) {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        let target_aliases = url_aliases(target_url);
        {
            let count = inner
                .inlink_counts
                .entry(target_url.to_string())
                .or_insert(0);
            *count = count.saturating_add(1);
        }

        let inlink_counts = inner.inlink_counts.clone();
        for record in &mut inner.records {
            let record_aliases = record_url_aliases(record);
            if target_aliases
                .iter()
                .any(|alias| record_aliases.contains(alias))
            {
                record.inlink_count =
                    memory_inlink_count_for_aliases(&inlink_counts, &record_aliases);
            }
        }
    }

    pub fn add_link_edge(&self, mut edge: LinkEdge) -> LinkEdge {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        inner.next_edge_id += 1;
        edge.id = inner.next_edge_id;
        edge.discovery_order = edge.id;

        if let Some(index) = memory_record_index_by_url(&inner, &edge.source_url) {
            let source = &inner.records[index];
            edge.source_status_code = source.status_code;
            edge.source_depth = source.depth;
        }

        if let Some(index) = memory_record_index_by_url(&inner, &edge.target_url) {
            let target = &inner.records[index];
            edge.target_status_code = target.status_code;
            edge.target_depth = Some(target.depth);
        }

        inner.link_edges.push(edge.clone());
        edge
    }

    pub fn add_image_assets(&self, page_url: &str, images: Vec<ImageAsset>) {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        let page_aliases = url_aliases(page_url);
        inner.image_assets.retain(|image| {
            let image_page_aliases = url_aliases(&image.page_url);
            !page_aliases
                .iter()
                .any(|alias| image_page_aliases.contains(alias))
        });

        for mut image in images {
            inner.next_image_asset_id = inner.next_image_asset_id.saturating_add(1);
            image.id = inner.next_image_asset_id;
            image.size_bytes = None;
            image.oversized = false;
            inner.image_assets.push(image);
        }
    }

    pub fn merge_search_console_metrics(&self, metrics: Vec<SearchConsoleMetricRow>) -> usize {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        let metrics_by_alias = search_console_metrics_by_alias(metrics);
        if metrics_by_alias.is_empty() {
            return 0;
        }

        let mut updated = 0usize;
        for record in &mut inner.records {
            let aliases = sorted_aliases(record_url_aliases(record));
            if let Some(metric) = aliases.iter().find_map(|alias| metrics_by_alias.get(alias)) {
                record.search_console_clicks = Some(metric.clicks);
                record.search_console_impressions = Some(metric.impressions);
                record.search_console_ctr = Some(metric.ctr);
                record.search_console_average_position = Some(metric.average_position);
                updated += 1;
            }
        }
        updated
    }

    pub fn records(&self) -> Vec<CrawlRecord> {
        let inner = self.inner.read().expect("memory store lock poisoned");
        let mut records = inner.records.clone();
        apply_first_inlink_sources(&mut records, &inner.link_edges);
        records
    }

    pub fn summary(&self) -> CrawlSummary {
        let records = self.records();
        summarize(&records)
    }

    pub fn query(&self, query: GridQuery) -> GridResponse {
        let mut rows = self.records();
        let summary = summarize(&rows);
        let html_rows = rows.iter().filter(|row| is_success_html_record(row));
        let title_counts =
            duplicate_counts(html_rows.clone().filter_map(|row| row.title.as_deref()));
        let meta_counts = duplicate_counts(
            html_rows
                .clone()
                .filter_map(|row| row.meta_description.as_deref()),
        );
        let h1_counts = duplicate_counts(html_rows.clone().filter_map(|row| row.h1.as_deref()));
        let h2_counts = duplicate_counts(html_rows.clone().filter_map(|row| row.h2.as_deref()));
        let near_duplicate_counts =
            cluster_counts(html_rows.filter_map(|row| row.near_duplicate_cluster_id));
        let hreflang_index = HreflangAuditIndex::from_records(&rows);

        rows.retain(|row| {
            matches_view(
                row,
                &query.view,
                &title_counts,
                &meta_counts,
                &h1_counts,
                &h2_counts,
                &near_duplicate_counts,
                &hreflang_index,
            )
        });
        if let Some(segment_matcher) = SegmentMatcher::from_query(&query) {
            rows.retain(|row| segment_matcher.matches(row));
        }

        if let Some(search) = query
            .global_search
            .as_ref()
            .map(|value| value.trim().to_lowercase())
            && !search.is_empty()
        {
            rows.retain(|row| row_matches_search(row, &search));
        }

        if let Some(sort_by) = query.sort_by.as_deref() {
            sort_rows(&mut rows, sort_by, &query.sort_dir);
        } else {
            rows.sort_by(compare_default_row_order);
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
        filter_link_edges(&mut edges, &query, &inner.records);
        if let Some(search) = query
            .global_search
            .as_ref()
            .map(|value| value.trim().to_lowercase())
            && !search.is_empty()
        {
            edges.retain(|edge| link_edge_matches_search(edge, &search));
        }
        if let Some(sort_by) = query.sort_by.as_deref() {
            sort_link_edges(&mut edges, sort_by, &query.sort_dir);
        }
        let total = edges.len();
        let limit = query.limit.min(1_000_000);
        let rows = edges.into_iter().skip(query.offset).take(limit).collect();

        LinkEdgeResponse { edges: rows, total }
    }

    pub fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse {
        let inner = self.inner.read().expect("memory store lock poisoned");
        let mut edges = inner.link_edges.clone();
        filter_link_edges(&mut edges, &query, &inner.records);
        if let Some(search) = query
            .global_search
            .as_ref()
            .map(|value| value.trim().to_lowercase())
            && !search.is_empty()
        {
            edges.retain(|edge| link_edge_matches_search(edge, &search));
        }
        let mut rows = aggregate_anchor_texts(edges);
        if let Some(sort_by) = query.sort_by.as_deref() {
            sort_anchor_text_rows(&mut rows, sort_by, &query.sort_dir);
        } else {
            sort_anchor_text_rows(&mut rows, "linkCount", &SortDirection::Desc);
        }
        let total = rows.len();
        let limit = query.limit.min(1_000_000);
        let rows = rows.into_iter().skip(query.offset).take(limit).collect();

        AnchorTextResponse { rows, total }
    }

    pub fn image_assets(&self, query: ImageAssetQuery) -> ImageAssetResponse {
        let inner = self.inner.read().expect("memory store lock poisoned");
        let mut images = annotate_memory_image_assets(&inner);
        filter_image_assets(&mut images, &query);
        if let Some(sort_by) = query.sort_by.as_deref() {
            sort_image_assets(&mut images, sort_by, &query.sort_dir);
        } else {
            sort_image_assets(&mut images, "pageUrl", &SortDirection::Asc);
        }
        let total = images.len();
        let limit = query.limit.min(1_000_000);
        let images = images.into_iter().skip(query.offset).take(limit).collect();
        ImageAssetResponse { images, total }
    }

    pub fn save_frontier_state(&self, state: CrawlFrontierState) {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        inner.frontier_state = Some(state);
    }

    pub fn load_frontier_state(&self) -> Option<CrawlFrontierState> {
        let inner = self.inner.read().expect("memory store lock poisoned");
        inner.frontier_state.clone()
    }

    pub fn clear_frontier_state(&self) {
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        inner.frontier_state = None;
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

    fn add_image_assets(&self, page_url: &str, images: Vec<ImageAsset>) {
        Self::add_image_assets(self, page_url, images);
    }

    fn merge_search_console_metrics(&self, metrics: Vec<SearchConsoleMetricRow>) -> usize {
        Self::merge_search_console_metrics(self, metrics)
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

    fn image_assets(&self, query: ImageAssetQuery) -> ImageAssetResponse {
        Self::image_assets(self, query)
    }

    fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse {
        Self::anchor_texts(self, query)
    }

    fn save_frontier_state(&self, state: CrawlFrontierState) {
        Self::save_frontier_state(self, state);
    }

    fn load_frontier_state(&self) -> Option<CrawlFrontierState> {
        Self::load_frontier_state(self)
    }

    fn clear_frontier_state(&self) {
        Self::clear_frontier_state(self);
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
        conn.execute("DELETE FROM image_assets", [])?;
        conn.execute("DELETE FROM crawl_frontier_queue", [])?;
        conn.execute("DELETE FROM crawl_frontier_seen", [])?;
        conn.execute("DELETE FROM crawl_frontier_meta", [])?;
        Ok(())
    }

    pub fn try_upsert(&self, mut record: CrawlRecord) -> Result<CrawlRecord, StorageError> {
        let conn = self.connection()?;
        if record.storage_key.trim().is_empty() {
            record.storage_key = record.final_url.clone();
        }
        let existing_id = conn
            .query_row(
                "SELECT id FROM crawl_records WHERE storage_key = ?1",
                [&record.storage_key],
                |row| row.get::<_, i64>(0).map(|id| id as u64),
            )
            .optional()?;
        record.inlink_count =
            sqlite_inlink_count_for_record(&conn, &record)?.unwrap_or(record.inlink_count);
        let redirect_chain = serde_json::to_string(&record.redirect_chain)?;
        let hreflang_links = serde_json::to_string(&record.hreflang_links)?;
        let structured_data_issues = serde_json::to_string(&record.structured_data_issues)?;
        let custom_extractions = serde_json::to_string(&record.custom_extractions)?;
        let custom_searches = serde_json::to_string(&record.custom_searches)?;
        let classification = classification_to_str(&record.classification);
        let simhash = record.simhash.map(|value| value.to_string());
        let near_duplicate_cluster_id = record.near_duplicate_cluster_id.map(|value| value as i64);
        let dns_lookup_time_ms = record.dns_lookup_time_ms.map(|value| value as i64);
        let tcp_connect_time_ms = record.tcp_connect_time_ms.map(|value| value as i64);
        let tls_handshake_time_ms = record.tls_handshake_time_ms.map(|value| value as i64);
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
                    deprecated_html_tag_count = ?53,
                    duplicate_id_count = ?54,
                    js_rendered = ?55,
                    rendered_dom_changed = ?56,
                    rendered_word_count_delta = ?57,
                    rendered_link_count_delta = ?58,
                    near_duplicate_cluster_id = ?59,
                    inlink_count = ?60,
                    outlink_count = ?61,
                    internal_outlink_count = ?62,
                    external_outlink_count = ?63,
                    custom_extractions = ?64,
                    custom_searches = ?65,
                    search_console_clicks = ?66,
                    search_console_impressions = ?67,
                    search_console_ctr = ?68,
                    search_console_average_position = ?69,
                    error = ?70,
                    dns_lookup_time_ms = ?71,
                    tcp_connect_time_ms = ?72,
                    tls_handshake_time_ms = ?73,
                    ttfb_ms = ?74,
                    download_time_ms = ?75,
                    total_network_time_ms = ?76,
                    transfer_rate_bytes_per_sec = ?77,
                    resolved_ip_count = ?78,
                    in_sitemap = ?79,
                    storage_key = ?80,
                    list_position = ?81,
                    list_duplicate_index = ?82,
                    hreflang_links = ?83,
                    structured_data_error_count = ?84,
                    structured_data_warning_count = ?85,
                    structured_data_issues = ?86,
                    title_pixel_width = ?87,
                    meta_description_pixel_width = ?88
                 WHERE id = ?89",
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
                    record.deprecated_html_tag_count,
                    record.duplicate_id_count,
                    record.js_rendered,
                    record.rendered_dom_changed,
                    record.rendered_word_count_delta,
                    record.rendered_link_count_delta,
                    near_duplicate_cluster_id,
                    record.inlink_count,
                    record.outlink_count,
                    record.internal_outlink_count,
                    record.external_outlink_count,
                    custom_extractions,
                    custom_searches,
                    record.search_console_clicks,
                    record.search_console_impressions,
                    record.search_console_ctr,
                    record.search_console_average_position,
                    record.error,
                    dns_lookup_time_ms,
                    tcp_connect_time_ms,
                    tls_handshake_time_ms,
                    ttfb_ms,
                    download_time_ms,
                    total_network_time_ms,
                    transfer_rate_bytes_per_sec,
                    record.resolved_ip_count,
                    record.in_sitemap,
                    record.storage_key,
                    record.list_position,
                    record.list_duplicate_index,
                    hreflang_links,
                    record.structured_data_error_count,
                    record.structured_data_warning_count,
                    structured_data_issues,
                    record.title_pixel_width,
                    record.meta_description_pixel_width,
                    record.id as i64
                ],
            )?;
            update_sqlite_edge_statuses(&conn, &record)?;
            annotate_sqlite_first_inlink_sources(&conn, std::slice::from_mut(&mut record))?;
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
                    deprecated_html_tag_count,
                    duplicate_id_count,
                    js_rendered,
                    rendered_dom_changed,
                    rendered_word_count_delta,
                    rendered_link_count_delta,
                    near_duplicate_cluster_id,
                    inlink_count,
                    outlink_count,
                    internal_outlink_count,
                    external_outlink_count,
                    custom_extractions,
                    custom_searches,
                    search_console_clicks,
                    search_console_impressions,
                    search_console_ctr,
                    search_console_average_position,
                    error,
                    dns_lookup_time_ms,
                    tcp_connect_time_ms,
                    tls_handshake_time_ms,
                    ttfb_ms,
                    download_time_ms,
                    total_network_time_ms,
                    transfer_rate_bytes_per_sec,
                    resolved_ip_count,
                    in_sitemap,
                    storage_key,
                    list_position,
                    list_duplicate_index,
                    hreflang_links,
                    structured_data_error_count,
                    structured_data_warning_count,
                    structured_data_issues,
                    title_pixel_width,
                    meta_description_pixel_width
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                    ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35,
                    ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46,
                    ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54, ?55, ?56, ?57,
                    ?58, ?59, ?60, ?61, ?62, ?63, ?64, ?65, ?66, ?67, ?68, ?69,
                    ?70, ?71, ?72, ?73, ?74, ?75, ?76, ?77, ?78, ?79, ?80,
                    ?81, ?82, ?83, ?84, ?85, ?86, ?87, ?88
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
                    record.deprecated_html_tag_count,
                    record.duplicate_id_count,
                    record.js_rendered,
                    record.rendered_dom_changed,
                    record.rendered_word_count_delta,
                    record.rendered_link_count_delta,
                    near_duplicate_cluster_id,
                    record.inlink_count,
                    record.outlink_count,
                    record.internal_outlink_count,
                    record.external_outlink_count,
                    custom_extractions,
                    custom_searches,
                    record.search_console_clicks,
                    record.search_console_impressions,
                    record.search_console_ctr,
                    record.search_console_average_position,
                    record.error,
                    dns_lookup_time_ms,
                    tcp_connect_time_ms,
                    tls_handshake_time_ms,
                    ttfb_ms,
                    download_time_ms,
                    total_network_time_ms,
                    transfer_rate_bytes_per_sec,
                    record.resolved_ip_count,
                    record.in_sitemap,
                    record.storage_key,
                    record.list_position,
                    record.list_duplicate_index,
                    hreflang_links,
                    record.structured_data_error_count,
                    record.structured_data_warning_count,
                    structured_data_issues,
                    record.title_pixel_width,
                    record.meta_description_pixel_width
                ],
            )?;
            record.id = conn.last_insert_rowid() as u64;
            update_sqlite_edge_statuses(&conn, &record)?;
            annotate_sqlite_first_inlink_sources(&conn, std::slice::from_mut(&mut record))?;
            Ok(record)
        }
    }

    pub fn try_add_inlink(&self, target_url: &str) -> Result<(), StorageError> {
        let conn = self.connection()?;
        let target_aliases = sorted_aliases(url_aliases(target_url));
        conn.execute(
            "INSERT INTO inlink_counts (url, count) VALUES (?1, 1)
             ON CONFLICT(url) DO UPDATE SET count = count + 1",
            [target_url],
        )?;
        if !target_aliases.is_empty() {
            let placeholders = sql_placeholders(target_aliases.len());
            let sql = format!(
                "UPDATE crawl_records
                 SET inlink_count = inlink_count + 1
                 WHERE final_url IN ({placeholders})
                    OR url IN ({placeholders})
                    OR storage_key IN ({placeholders})"
            );
            let args = repeat_args(&target_aliases, 3);
            conn.execute(&sql, rusqlite::params_from_iter(args.iter()))?;
        }
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

    pub fn try_add_image_assets(
        &self,
        page_url: &str,
        images: Vec<ImageAsset>,
    ) -> Result<(), StorageError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM image_assets WHERE page_url = ?1", [page_url])?;
        for image in images {
            tx.execute(
                "INSERT INTO image_assets (
                    page_url,
                    image_url,
                    alt_text,
                    alt_len,
                    missing_alt,
                    alt_too_long,
                    width,
                    height,
                    source_position
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    page_url,
                    image.image_url,
                    image.alt_text,
                    i64::from(image.alt_len),
                    image.missing_alt,
                    image.alt_too_long,
                    image.width.map(i64::from),
                    image.height.map(i64::from),
                    i64::from(image.source_position),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn try_merge_search_console_metrics(
        &self,
        metrics: Vec<SearchConsoleMetricRow>,
    ) -> Result<usize, StorageError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        let mut updated_ids = HashSet::new();

        for metric in metrics {
            let aliases = sorted_aliases(url_aliases(&metric.url));
            if aliases.is_empty() {
                continue;
            }

            let placeholders = sql_placeholders(aliases.len());
            let select_sql = format!(
                "SELECT id FROM crawl_records
                 WHERE storage_key IN ({placeholders})
                    OR url IN ({placeholders})
                    OR final_url IN ({placeholders})"
            );
            let args = repeat_args(&aliases, 3);
            let ids = {
                let mut stmt = tx.prepare(&select_sql)?;
                let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
                    row.get::<_, i64>(0)
                })?;
                let mut ids = Vec::new();
                for row in rows {
                    ids.push(row?);
                }
                ids
            };
            if ids.is_empty() {
                continue;
            }

            let id_placeholders = sql_placeholders(ids.len());
            let update_sql = format!(
                "UPDATE crawl_records
                 SET search_console_clicks = ?,
                     search_console_impressions = ?,
                     search_console_ctr = ?,
                     search_console_average_position = ?
                 WHERE id IN ({id_placeholders})"
            );
            let mut update_args = vec![
                Value::Real(metric.clicks),
                Value::Real(metric.impressions),
                Value::Real(metric.ctr),
                Value::Real(metric.average_position),
            ];
            update_args.extend(ids.iter().copied().map(Value::Integer));
            tx.execute(&update_sql, rusqlite::params_from_iter(update_args.iter()))?;
            updated_ids.extend(ids.into_iter().map(|id| id as u64));
        }

        tx.commit()?;
        Ok(updated_ids.len())
    }

    pub fn try_records(&self) -> Result<Vec<CrawlRecord>, StorageError> {
        self.query_records(
            "SELECT * FROM crawl_records ORDER BY COALESCE(list_position, id) ASC, id ASC",
            [],
        )
    }

    pub fn try_link_edges(&self, query: LinkEdgeQuery) -> Result<LinkEdgeResponse, StorageError> {
        let conn = self.connection()?;
        let (where_clause, args) = link_edge_filter_sql(&query, &conn)?;
        let order_by = link_edge_sort_column(query.sort_by.as_deref())
            .map(|column| {
                let direction = match query.sort_dir {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                };
                format!(" ORDER BY {column} {direction}")
            })
            .unwrap_or_else(|| " ORDER BY id ASC".to_string());
        let limit = query.limit.min(1_000_000);
        let total_sql = format!("SELECT COUNT(*) FROM link_edges{where_clause}");
        let select_sql = format!(
            "SELECT * FROM link_edges{where_clause}{order_by} LIMIT {limit} OFFSET {}",
            query.offset
        );
        let total = count_query(&conn, &total_sql, &args)?;
        let edges = query_link_edges_with_args(&conn, &select_sql, &args)?;
        Ok(LinkEdgeResponse { edges, total })
    }

    pub fn try_anchor_texts(
        &self,
        query: LinkEdgeQuery,
    ) -> Result<AnchorTextResponse, StorageError> {
        let conn = self.connection()?;
        let (where_clause, args) = link_edge_filter_sql(&query, &conn)?;
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
        let limit = query.limit.min(1_000_000);
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
        let total = count_query(&conn, &total_sql, &args)?;
        let rows = query_anchor_texts_with_args(&conn, &select_sql, &args)?;
        Ok(AnchorTextResponse { rows, total })
    }

    pub fn try_image_assets(
        &self,
        query: ImageAssetQuery,
    ) -> Result<ImageAssetResponse, StorageError> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT
                ia.id,
                ia.page_url,
                ia.image_url,
                ia.alt_text,
                ia.alt_len,
                ia.missing_alt,
                ia.alt_too_long,
                ia.width,
                ia.height,
                ia.source_position,
                (
                    SELECT cr.size_bytes
                    FROM crawl_records cr
                    WHERE cr.final_url = ia.image_url
                       OR cr.url = ia.image_url
                       OR cr.storage_key = ia.image_url
                    ORDER BY cr.id ASC
                    LIMIT 1
                ) AS size_bytes
             FROM image_assets ia
             ORDER BY ia.id ASC",
        )?;
        let rows = stmt.query_map([], image_asset_from_row)?;
        let mut images = Vec::new();
        for row in rows {
            images.push(row?);
        }

        filter_image_assets(&mut images, &query);
        if let Some(sort_by) = query.sort_by.as_deref() {
            sort_image_assets(&mut images, sort_by, &query.sort_dir);
        } else {
            sort_image_assets(&mut images, "pageUrl", &SortDirection::Asc);
        }
        let total = images.len();
        let limit = query.limit.min(1_000_000);
        let images = images.into_iter().skip(query.offset).take(limit).collect();

        Ok(ImageAssetResponse { images, total })
    }

    pub fn try_save_frontier_state(&self, state: CrawlFrontierState) -> Result<(), StorageError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM crawl_frontier_queue", [])?;
        tx.execute("DELETE FROM crawl_frontier_seen", [])?;
        tx.execute("DELETE FROM crawl_frontier_meta", [])?;

        for (position, item) in state.queued.into_iter().enumerate() {
            tx.execute(
                "INSERT INTO crawl_frontier_queue (
                    position,
                    url,
                    depth,
                    from_sitemap,
                    storage_key,
                    list_position,
                    list_duplicate_index
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    position as i64,
                    item.url,
                    item.depth as i64,
                    item.from_sitemap,
                    item.storage_key,
                    item.list_position.map(i64::from),
                    i64::from(item.list_duplicate_index),
                ],
            )?;
        }

        for url in state.seen {
            tx.execute(
                "INSERT OR IGNORE INTO crawl_frontier_seen (url) VALUES (?1)",
                [url],
            )?;
        }

        tx.execute(
            "INSERT INTO crawl_frontier_meta (key, value) VALUES ('crawled', ?1)",
            [state.crawled.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn try_load_frontier_state(&self) -> Result<Option<CrawlFrontierState>, StorageError> {
        let conn = self.connection()?;
        let mut queue_stmt = conn.prepare(
            "SELECT
                url,
                depth,
                from_sitemap,
                storage_key,
                list_position,
                list_duplicate_index
             FROM crawl_frontier_queue
             ORDER BY position ASC",
        )?;
        let queue_rows = queue_stmt.query_map([], frontier_item_from_row)?;
        let mut queued = Vec::new();
        for row in queue_rows {
            queued.push(row?);
        }
        drop(queue_stmt);

        let mut seen_stmt = conn.prepare("SELECT url FROM crawl_frontier_seen ORDER BY url ASC")?;
        let seen_rows = seen_stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut seen = Vec::new();
        for row in seen_rows {
            seen.push(row?);
        }
        drop(seen_stmt);

        if queued.is_empty() && seen.is_empty() {
            return Ok(None);
        }

        let crawled = conn
            .query_row(
                "SELECT value FROM crawl_frontier_meta WHERE key = 'crawled'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);

        Ok(Some(CrawlFrontierState {
            queued,
            seen,
            crawled,
        }))
    }

    pub fn try_clear_frontier_state(&self) -> Result<(), StorageError> {
        let conn = self.connection()?;
        conn.execute("DELETE FROM crawl_frontier_queue", [])?;
        conn.execute("DELETE FROM crawl_frontier_seen", [])?;
        conn.execute("DELETE FROM crawl_frontier_meta", [])?;
        Ok(())
    }

    pub fn try_query(&self, query: GridQuery) -> Result<GridResponse, StorageError> {
        if needs_duplicate_filter(&query.view) || needs_regex_segment_filter(&query) {
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
        let limit = query.limit.min(1_000_000);
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
        let count_view = |view| {
            let (where_clause, args) = query_filter_sql(&GridQuery {
                view,
                ..GridQuery::default()
            });
            count_query(
                &conn,
                &format!("SELECT COUNT(*) FROM crawl_records{where_clause}"),
                &args,
            )
        };
        Ok(CrawlSummary {
            total: count_view(IssueView::All)?,
            internal: count_view(IssueView::Internal)?,
            external: count_view(IssueView::External)?,
            success: count_view(IssueView::Status2xx)?,
            redirects: count_view(IssueView::Status3xx)?,
            client_errors: count_view(IssueView::Status4xx)?,
            server_errors: count_view(IssueView::Status5xx)?,
            no_response: count_view(IssueView::NoResponse)?,
            broken: count_view(IssueView::BrokenLinks)?,
            near_duplicates: count_view(IssueView::NearDuplicate)?,
            indexable: summary_count(
                &conn,
                "SELECT COUNT(*) FROM crawl_records WHERE indexability = 'Indexable'",
            )?,
            non_indexable: summary_count(
                &conn,
                "SELECT COUNT(*) FROM crawl_records WHERE indexability = 'Non-indexable'",
            )?,
            title_missing: count_view(IssueView::TitleMissing)?,
            title_duplicate: sqlite_duplicate_count(&conn, "title")?,
            meta_missing: count_view(IssueView::MetaMissing)?,
            meta_duplicate: sqlite_duplicate_count(&conn, "meta_description")?,
            h1_missing: count_view(IssueView::H1Missing)?,
            h1_duplicate: sqlite_duplicate_count(&conn, "h1")?,
            h2_missing: count_view(IssueView::H2Missing)?,
            h2_duplicate: sqlite_duplicate_count(&conn, "h2")?,
            canonical_missing: count_view(IssueView::CanonicalMissing)?,
            canonical_multiple: count_view(IssueView::CanonicalMultiple)?,
            noindex: count_view(IssueView::DirectivesNoindex)?,
            images_missing_alt: count_view(IssueView::ImagesMissingAlt)?,
            images_alt_too_long: count_view(IssueView::ImagesAltTooLong)?,
            mixed_content: count_view(IssueView::SecurityMixedContent)?,
            insecure_forms: count_view(IssueView::SecurityInsecureForms)?,
            hreflang_invalid: count_view(IssueView::HreflangInvalid)?,
            structured_data_invalid: count_view(IssueView::StructuredDataInvalid)?,
            structured_data_warnings: count_view(IssueView::StructuredDataWarning)?,
            deprecated_html_tags: count_view(IssueView::HtmlDeprecatedTags)?,
            duplicate_ids: count_view(IssueView::HtmlDuplicateIds)?,
            rendered_dom_changed: count_view(IssueView::RenderedDomChanged)?,
            missing_viewport: count_view(IssueView::MobileMissingViewport)?,
            missing_hsts: count_view(IssueView::SecurityMissingHsts)?,
            sitemap_orphans: count_view(IssueView::SitemapOrphan)?,
        })
    }

    fn initialize(&self) -> Result<(), StorageError> {
        let conn = self.connection()?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS crawl_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                storage_key TEXT NOT NULL UNIQUE,
                url TEXT NOT NULL,
                final_url TEXT NOT NULL,
                list_position INTEGER,
                list_duplicate_index INTEGER NOT NULL DEFAULT 0,
                classification TEXT NOT NULL,
                status_code INTEGER,
                status_text TEXT NOT NULL,
                content_type TEXT,
                indexability TEXT NOT NULL,
                indexability_status TEXT NOT NULL,
                response_time_ms INTEGER NOT NULL,
                dns_lookup_time_ms INTEGER,
                tcp_connect_time_ms INTEGER,
                tls_handshake_time_ms INTEGER,
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
                title_pixel_width INTEGER NOT NULL DEFAULT 0,
                meta_description TEXT,
                meta_description_len INTEGER NOT NULL,
                meta_description_pixel_width INTEGER NOT NULL DEFAULT 0,
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
                hreflang_links TEXT NOT NULL DEFAULT '[]',
                json_ld_count INTEGER NOT NULL DEFAULT 0,
                json_ld_invalid_count INTEGER NOT NULL DEFAULT 0,
                structured_data_error_count INTEGER NOT NULL DEFAULT 0,
                structured_data_warning_count INTEGER NOT NULL DEFAULT 0,
                structured_data_issues TEXT NOT NULL DEFAULT '[]',
                open_graph_count INTEGER NOT NULL DEFAULT 0,
                twitter_card_count INTEGER NOT NULL DEFAULT 0,
                deprecated_html_tag_count INTEGER NOT NULL DEFAULT 0,
                duplicate_id_count INTEGER NOT NULL DEFAULT 0,
                js_rendered INTEGER NOT NULL DEFAULT 0,
                rendered_dom_changed INTEGER NOT NULL DEFAULT 0,
                rendered_word_count_delta INTEGER NOT NULL DEFAULT 0,
                rendered_link_count_delta INTEGER NOT NULL DEFAULT 0,
                near_duplicate_cluster_id INTEGER,
                inlink_count INTEGER NOT NULL,
                outlink_count INTEGER NOT NULL,
                internal_outlink_count INTEGER NOT NULL,
                external_outlink_count INTEGER NOT NULL,
                custom_extractions TEXT NOT NULL DEFAULT '[]',
                custom_searches TEXT NOT NULL DEFAULT '[]',
                search_console_clicks REAL,
                search_console_impressions REAL,
                search_console_ctr REAL,
                search_console_average_position REAL,
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

            CREATE INDEX IF NOT EXISTS idx_link_edges_source_url ON link_edges(source_url);
            CREATE INDEX IF NOT EXISTS idx_link_edges_target_url ON link_edges(target_url);
            CREATE INDEX IF NOT EXISTS idx_link_edges_link_type ON link_edges(link_type);

            CREATE TABLE IF NOT EXISTS image_assets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                page_url TEXT NOT NULL,
                image_url TEXT NOT NULL,
                alt_text TEXT,
                alt_len INTEGER NOT NULL DEFAULT 0,
                missing_alt INTEGER NOT NULL DEFAULT 0,
                alt_too_long INTEGER NOT NULL DEFAULT 0,
                width INTEGER,
                height INTEGER,
                source_position INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_image_assets_page_url ON image_assets(page_url);
            CREATE INDEX IF NOT EXISTS idx_image_assets_image_url ON image_assets(image_url);

            CREATE TABLE IF NOT EXISTS crawl_frontier_queue (
                position INTEGER PRIMARY KEY,
                url TEXT NOT NULL,
                depth INTEGER NOT NULL DEFAULT 0,
                from_sitemap INTEGER NOT NULL DEFAULT 0,
                storage_key TEXT NOT NULL,
                list_position INTEGER,
                list_duplicate_index INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS crawl_frontier_seen (
                url TEXT PRIMARY KEY
            );

            CREATE TABLE IF NOT EXISTS crawl_frontier_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_crawl_frontier_queue_storage_key
                ON crawl_frontier_queue(storage_key);
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
        add_column_if_missing(&conn, "custom_searches", "TEXT NOT NULL DEFAULT '[]'")?;
        add_column_if_missing(&conn, "meta_robots", "TEXT")?;
        add_column_if_missing(&conn, "x_robots_tag", "TEXT")?;
        add_column_if_missing(&conn, "title_pixel_width", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(
            &conn,
            "meta_description_pixel_width",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
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
        add_column_if_missing(&conn, "tcp_connect_time_ms", "INTEGER")?;
        add_column_if_missing(&conn, "tls_handshake_time_ms", "INTEGER")?;
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
        add_column_if_missing(&conn, "hreflang_links", "TEXT NOT NULL DEFAULT '[]'")?;
        add_column_if_missing(&conn, "json_ld_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "json_ld_invalid_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(
            &conn,
            "structured_data_error_count",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(
            &conn,
            "structured_data_warning_count",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(
            &conn,
            "structured_data_issues",
            "TEXT NOT NULL DEFAULT '[]'",
        )?;
        add_column_if_missing(&conn, "open_graph_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "twitter_card_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(
            &conn,
            "deprecated_html_tag_count",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(&conn, "duplicate_id_count", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "js_rendered", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "rendered_dom_changed", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(
            &conn,
            "rendered_word_count_delta",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(
            &conn,
            "rendered_link_count_delta",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(&conn, "search_console_clicks", "REAL")?;
        add_column_if_missing(&conn, "search_console_impressions", "REAL")?;
        add_column_if_missing(&conn, "search_console_ctr", "REAL")?;
        add_column_if_missing(&conn, "search_console_average_position", "REAL")?;
        add_column_if_missing(&conn, "in_sitemap", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(&conn, "storage_key", "TEXT")?;
        conn.execute(
            "UPDATE crawl_records
             SET storage_key = final_url
             WHERE storage_key IS NULL OR trim(storage_key) = ''",
            [],
        )?;
        add_column_if_missing(&conn, "list_position", "INTEGER")?;
        add_column_if_missing(&conn, "list_duplicate_index", "INTEGER NOT NULL DEFAULT 0")?;
        migrate_final_url_unique_constraint(&conn)?;
        create_crawl_record_indexes(&conn)?;
        add_table_column_if_missing(
            &conn,
            "link_edges",
            "source_position",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_table_column_if_missing(
            &conn,
            "image_assets",
            "source_position",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_table_column_if_missing(&conn, "crawl_frontier_queue", "list_position", "INTEGER")?;
        add_table_column_if_missing(
            &conn,
            "crawl_frontier_queue",
            "list_duplicate_index",
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
        annotate_sqlite_first_inlink_sources(&conn, &mut records)?;
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

    fn add_image_assets(&self, page_url: &str, images: Vec<ImageAsset>) {
        self.try_add_image_assets(page_url, images)
            .expect("sqlite image asset update failed");
    }

    fn merge_search_console_metrics(&self, metrics: Vec<SearchConsoleMetricRow>) -> usize {
        self.try_merge_search_console_metrics(metrics)
            .expect("sqlite Search Console metric merge failed")
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

    fn image_assets(&self, query: ImageAssetQuery) -> ImageAssetResponse {
        self.try_image_assets(query)
            .expect("sqlite image asset query failed")
    }

    fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse {
        self.try_anchor_texts(query)
            .expect("sqlite anchor text query failed")
    }

    fn save_frontier_state(&self, state: CrawlFrontierState) {
        self.try_save_frontier_state(state)
            .expect("sqlite frontier save failed");
    }

    fn load_frontier_state(&self) -> Option<CrawlFrontierState> {
        self.try_load_frontier_state()
            .expect("sqlite frontier load failed")
    }

    fn clear_frontier_state(&self) {
        self.try_clear_frontier_state()
            .expect("sqlite frontier clear failed");
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

    fn add_image_assets(&self, page_url: &str, images: Vec<ImageAsset>) {
        match self {
            ActiveStore::Memory(store) => store.add_image_assets(page_url, images),
            ActiveStore::Sqlite(store) => store.add_image_assets(page_url, images),
        }
    }

    fn merge_search_console_metrics(&self, metrics: Vec<SearchConsoleMetricRow>) -> usize {
        match self {
            ActiveStore::Memory(store) => store.merge_search_console_metrics(metrics),
            ActiveStore::Sqlite(store) => store.merge_search_console_metrics(metrics),
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

    fn image_assets(&self, query: ImageAssetQuery) -> ImageAssetResponse {
        match self {
            ActiveStore::Memory(store) => store.image_assets(query),
            ActiveStore::Sqlite(store) => store.image_assets(query),
        }
    }

    fn anchor_texts(&self, query: LinkEdgeQuery) -> AnchorTextResponse {
        match self {
            ActiveStore::Memory(store) => store.anchor_texts(query),
            ActiveStore::Sqlite(store) => store.anchor_texts(query),
        }
    }

    fn save_frontier_state(&self, state: CrawlFrontierState) {
        match self {
            ActiveStore::Memory(store) => store.save_frontier_state(state),
            ActiveStore::Sqlite(store) => store.save_frontier_state(state),
        }
    }

    fn load_frontier_state(&self) -> Option<CrawlFrontierState> {
        match self {
            ActiveStore::Memory(store) => store.load_frontier_state(),
            ActiveStore::Sqlite(store) => store.load_frontier_state(),
        }
    }

    fn clear_frontier_state(&self) {
        match self {
            ActiveStore::Memory(store) => store.clear_frontier_state(),
            ActiveStore::Sqlite(store) => store.clear_frontier_state(),
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
    let html_records = records
        .iter()
        .filter(|record| is_success_html_record(record));
    let near_duplicate_counts = cluster_counts(
        html_records
            .clone()
            .filter_map(|record| record.near_duplicate_cluster_id),
    );
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
    let h2_counts = duplicate_counts(html_records.filter_map(|record| record.h2.as_deref()));

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

        summary.no_response += usize::from(is_no_response_record(record));
        summary.broken += usize::from(is_broken_record(record));
        summary.redirects += usize::from(
            !record.redirect_chain.is_empty() || matches!(record.status_code, Some(300..=399)),
        );
        match record.status_code {
            Some(200..=299) => summary.success += 1,
            Some(400..=499) => summary.client_errors += 1,
            Some(500..) => summary.server_errors += 1,
            _ => {}
        }
        if record
            .indexability_status
            .to_lowercase()
            .contains("noindex")
        {
            summary.noindex += 1;
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
        if !is_success_html_record(record) {
            continue;
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
        if record.structured_data_error_count > 0 || record.json_ld_invalid_count > 0 {
            summary.structured_data_invalid += 1;
        }
        if record.structured_data_warning_count > 0 {
            summary.structured_data_warnings += 1;
        }
        if record.deprecated_html_tag_count > 0 {
            summary.deprecated_html_tags += 1;
        }
        if record.duplicate_id_count > 0 {
            summary.duplicate_ids += 1;
        }
        if record.rendered_dom_changed {
            summary.rendered_dom_changed += 1;
        }
        if !record.viewport {
            summary.missing_viewport += 1;
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

pub fn build_sitemap_validation_report(
    records: &[CrawlRecord],
    query: SitemapValidationQuery,
) -> SitemapValidationResponse {
    let mut rows = records
        .iter()
        .filter(|record| record.in_sitemap)
        .map(sitemap_validation_row)
        .collect::<Vec<_>>();

    if let Some(search) = query
        .global_search
        .as_ref()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
    {
        rows.retain(|row| sitemap_validation_matches_search(row, &search));
    }

    if let Some(sort_by) = query.sort_by.as_deref() {
        sort_sitemap_validation_rows(&mut rows, sort_by, &query.sort_dir);
    } else {
        rows.sort_by(compare_default_sitemap_validation_order);
    }

    let total = rows.len();
    let limit = query.limit.min(1_000_000);
    let rows = rows.into_iter().skip(query.offset).take(limit).collect();
    SitemapValidationResponse { rows, total }
}

fn sitemap_validation_row(record: &CrawlRecord) -> SitemapValidationRow {
    let (severity, issues) = sitemap_validation_issues(record);
    SitemapValidationRow {
        url: record.url.clone(),
        final_url: record.final_url.clone(),
        status_code: record.status_code,
        status_text: record.status_text.clone(),
        indexability: record.indexability.clone(),
        indexability_status: record.indexability_status.clone(),
        inlink_count: record.inlink_count,
        redirect_target: record.redirect_target.clone(),
        canonical: record.canonical.clone(),
        issue_count: issues.len(),
        severity,
        issues,
    }
}

fn sitemap_validation_issues(record: &CrawlRecord) -> (Severity, Vec<String>) {
    let mut severity = Severity::Info;
    let mut issues = Vec::new();

    match record.status_code {
        None if is_robots_blocked_record(record) => push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Warning,
            "Robots-blocked URL in sitemap",
        ),
        None if is_no_response_record(record) => push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Error,
            "No response URL in sitemap",
        ),
        Some(300..=399) => push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Warning,
            "Redirecting URL in sitemap",
        ),
        Some(400..=499) => push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Error,
            "4xx URL in sitemap",
        ),
        Some(500..=599) => push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Error,
            "5xx URL in sitemap",
        ),
        _ => {}
    }

    if record.indexability != "Indexable" || record.indexability_status != "Indexable" {
        push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Warning,
            "Non-indexable URL in sitemap",
        );
    }

    if let Some(canonical) = record.canonical.as_deref().map(str::trim)
        && !canonical.is_empty()
        && canonical != record.final_url
    {
        push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Warning,
            "Canonical points to a different URL",
        );
    }

    if record
        .redirect_target
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Warning,
            "Sitemap URL has a redirect target",
        );
    }

    if record.classification == UrlClassification::External {
        push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Warning,
            "External URL in sitemap",
        );
    }

    if record.inlink_count == 0 && record.classification == UrlClassification::Internal {
        push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Warning,
            "Orphan URL in sitemap",
        );
    }

    if !is_robots_blocked_record(record)
        && record
            .error
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        push_sitemap_issue(
            &mut issues,
            &mut severity,
            Severity::Error,
            "Fetch error for sitemap URL",
        );
    }

    if issues.is_empty() {
        issues.push("OK".to_string());
    }

    (severity, issues)
}

fn push_sitemap_issue(
    issues: &mut Vec<String>,
    severity: &mut Severity,
    next_severity: Severity,
    issue: &str,
) {
    if severity_rank(&next_severity) > severity_rank(severity) {
        *severity = next_severity;
    }
    issues.push(issue.to_string());
}

fn severity_rank(severity: &Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

fn sitemap_validation_matches_search(row: &SitemapValidationRow, search: &str) -> bool {
    [
        row.url.as_str(),
        row.final_url.as_str(),
        row.status_text.as_str(),
        row.indexability.as_str(),
        row.indexability_status.as_str(),
        row.redirect_target.as_deref().unwrap_or_default(),
        row.canonical.as_deref().unwrap_or_default(),
    ]
    .iter()
    .any(|value| value.to_lowercase().contains(search))
        || row
            .status_code
            .map(|value| value.to_string().contains(search))
            .unwrap_or(false)
        || row
            .issues
            .iter()
            .any(|issue| issue.to_lowercase().contains(search))
}

fn sort_sitemap_validation_rows(
    rows: &mut [SitemapValidationRow],
    sort_by: &str,
    sort_dir: &SortDirection,
) {
    rows.sort_by(|left, right| {
        let ordering = compare_sitemap_validation_rows(left, right, sort_by)
            .then_with(|| compare_default_sitemap_validation_order(left, right));
        match sort_dir {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
}

fn compare_default_sitemap_validation_order(
    left: &SitemapValidationRow,
    right: &SitemapValidationRow,
) -> Ordering {
    severity_rank(&right.severity)
        .cmp(&severity_rank(&left.severity))
        .then_with(|| right.issue_count.cmp(&left.issue_count))
        .then_with(|| left.final_url.cmp(&right.final_url))
}

fn compare_sitemap_validation_rows(
    left: &SitemapValidationRow,
    right: &SitemapValidationRow,
    sort_by: &str,
) -> Ordering {
    match sort_by {
        "url" => left.url.cmp(&right.url),
        "finalUrl" => left.final_url.cmp(&right.final_url),
        "statusCode" => left.status_code.cmp(&right.status_code),
        "statusText" => left.status_text.cmp(&right.status_text),
        "indexability" => left.indexability.cmp(&right.indexability),
        "indexabilityStatus" => left.indexability_status.cmp(&right.indexability_status),
        "inlinkCount" => left.inlink_count.cmp(&right.inlink_count),
        "redirectTarget" => left.redirect_target.cmp(&right.redirect_target),
        "canonical" => left.canonical.cmp(&right.canonical),
        "issueCount" => left.issue_count.cmp(&right.issue_count),
        "severity" => severity_rank(&left.severity).cmp(&severity_rank(&right.severity)),
        "issues" => left.issues.join("; ").cmp(&right.issues.join("; ")),
        _ => compare_default_sitemap_validation_order(left, right),
    }
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

fn record_url_aliases(record: &CrawlRecord) -> HashSet<String> {
    url_aliases_many([
        record.storage_key.as_str(),
        record.url.as_str(),
        record.final_url.as_str(),
    ])
}

fn url_aliases_many<'a>(values: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    let mut aliases = HashSet::new();
    for value in values {
        add_url_aliases(&mut aliases, value);
    }
    aliases
}

fn url_aliases(value: &str) -> HashSet<String> {
    url_aliases_many([value])
}

fn search_console_metrics_by_alias(
    metrics: Vec<SearchConsoleMetricRow>,
) -> HashMap<String, SearchConsoleMetricRow> {
    let mut metrics_by_alias = HashMap::new();
    for metric in metrics {
        for alias in url_aliases(&metric.url) {
            metrics_by_alias.insert(alias, metric.clone());
        }
    }
    metrics_by_alias
}

fn add_url_aliases(aliases: &mut HashSet<String>, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    aliases.insert(trimmed.to_string());
    if let Ok(parsed) = url::Url::parse(trimmed) {
        aliases.insert(parsed.to_string());

        let mut without_fragment = parsed.clone();
        without_fragment.set_fragment(None);
        aliases.insert(without_fragment.to_string());

        if without_fragment.path() == "/" {
            let host_alias = match without_fragment.host_str() {
                Some(host) => {
                    let mut value = format!("{}://{}", without_fragment.scheme(), host);
                    if let Some(port) = without_fragment.port() {
                        value.push(':');
                        value.push_str(&port.to_string());
                    }
                    if let Some(query) = without_fragment.query() {
                        value.push('?');
                        value.push_str(query);
                    }
                    value
                }
                None => without_fragment.to_string(),
            };
            aliases.insert(host_alias);
        }
    }
}

fn sorted_aliases(aliases: HashSet<String>) -> Vec<String> {
    let mut aliases = aliases.into_iter().collect::<Vec<_>>();
    aliases.sort();
    aliases
}

fn sql_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(",")
}

fn repeat_args(values: &[String], times: usize) -> Vec<String> {
    let mut args = Vec::with_capacity(values.len() * times);
    for _ in 0..times {
        args.extend(values.iter().cloned());
    }
    args
}

fn optional_u16_value(value: Option<u16>) -> Value {
    value
        .map(|value| Value::Integer(i64::from(value)))
        .unwrap_or(Value::Null)
}

fn record_matches_url(record: &CrawlRecord, url: &str) -> bool {
    let aliases = record_url_aliases(record);
    url_aliases(url).iter().any(|alias| aliases.contains(alias))
}

fn memory_record_index_by_url(inner: &MemoryStoreInner, url: &str) -> Option<usize> {
    inner
        .records
        .iter()
        .position(|record| record_matches_url(record, url))
}

fn update_memory_edge_statuses(edges: &mut [LinkEdge], record: &CrawlRecord) {
    let aliases = record_url_aliases(record);
    for edge in edges {
        if url_aliases(&edge.source_url)
            .iter()
            .any(|alias| aliases.contains(alias))
        {
            edge.source_status_code = record.status_code;
            edge.source_depth = record.depth;
        }
        if url_aliases(&edge.target_url)
            .iter()
            .any(|alias| aliases.contains(alias))
        {
            edge.target_status_code = record.status_code;
            edge.target_depth = Some(record.depth);
        }
    }
}

fn memory_inlink_count_for_record(inner: &MemoryStoreInner, record: &CrawlRecord) -> Option<u32> {
    let aliases = record_url_aliases(record);
    let count = memory_inlink_count_for_aliases(&inner.inlink_counts, &aliases);
    (count > 0).then_some(count)
}

fn memory_inlink_count_for_aliases(
    inlink_counts: &HashMap<String, u32>,
    aliases: &HashSet<String>,
) -> u32 {
    aliases
        .iter()
        .filter_map(|alias| inlink_counts.get(alias))
        .copied()
        .sum()
}

fn filter_link_edges(edges: &mut Vec<LinkEdge>, query: &LinkEdgeQuery, records: &[CrawlRecord]) {
    match query.view {
        LinkEdgeView::All => {}
        LinkEdgeView::Internal => edges.retain(|edge| edge.link_type == LinkType::Internal),
        LinkEdgeView::External => edges.retain(|edge| edge.link_type == LinkType::External),
        LinkEdgeView::Broken => {
            let failed_targets = records
                .iter()
                .filter(|record| is_broken_record(record))
                .flat_map(record_url_aliases)
                .collect::<HashSet<_>>();
            edges.retain(|edge| {
                edge.target_status_code.is_some_and(|status| status >= 400)
                    || failed_targets.contains(&edge.target_url)
            });
        }
        LinkEdgeView::Nofollow => edges.retain(|edge| edge.rel_nofollow),
    }
    if query.internal_only {
        edges.retain(|edge| edge.link_type == LinkType::Internal);
    }
    if let Some(source_url) = query.source_url.as_ref().map(|value| value.trim())
        && !source_url.is_empty()
    {
        edges.retain(|edge| edge.source_url == source_url);
    }
    if let Some(target_url) = query.target_url.as_ref().map(|value| value.trim())
        && !target_url.is_empty()
    {
        edges.retain(|edge| edge.target_url == target_url);
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
        let crawled = record.status_code.is_some() || is_no_response_record(&record);
        nodes.insert(
            record.final_url.clone(),
            GraphNode {
                label: graph_label(&record.final_url),
                url: record.final_url,
                crawled,
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

fn build_crawl_path(
    edges: Vec<LinkEdge>,
    target_url: String,
    total_edges: usize,
    max_edges: usize,
    internal_only: bool,
) -> CrawlPathResponse {
    let target_aliases = url_aliases(&target_url);
    if target_aliases.is_empty() || edges.is_empty() {
        return CrawlPathResponse {
            target_url,
            found: false,
            truncated: total_edges > max_edges,
            explored_edges: edges.len(),
            steps: Vec::new(),
        };
    }

    let mut adjacency: HashMap<String, Vec<LinkEdge>> = HashMap::new();
    for edge in edges
        .iter()
        .filter(|edge| !internal_only || matches!(edge.link_type, LinkType::Internal))
    {
        for alias in url_aliases(&edge.source_url) {
            adjacency.entry(alias).or_default().push(edge.clone());
        }
    }
    for outgoing_edges in adjacency.values_mut() {
        outgoing_edges.sort_by(|left, right| {
            left.discovery_order
                .cmp(&right.discovery_order)
                .then_with(|| left.source_position.cmp(&right.source_position))
                .then_with(|| left.target_url.cmp(&right.target_url))
        });
    }

    let min_source_depth = edges
        .iter()
        .map(|edge| edge.source_depth)
        .min()
        .unwrap_or(0);
    let mut starts = edges
        .iter()
        .filter(|edge| edge.source_depth == 0)
        .map(|edge| edge.source_url.clone())
        .collect::<Vec<_>>();
    if starts.is_empty() {
        starts = edges
            .iter()
            .filter(|edge| edge.source_depth == min_source_depth)
            .map(|edge| edge.source_url.clone())
            .collect();
    }
    starts.sort();
    starts.dedup();

    let mut queue = VecDeque::new();
    let mut visited_aliases = HashSet::new();
    let mut parent_by_url: HashMap<String, (String, LinkEdge)> = HashMap::new();

    for start in starts {
        if aliases_overlap(&url_aliases(&start), &target_aliases) {
            return CrawlPathResponse {
                target_url,
                found: true,
                truncated: total_edges > max_edges,
                explored_edges: edges.len(),
                steps: Vec::new(),
            };
        }
        if mark_url_visited(&mut visited_aliases, &start) {
            queue.push_back(start);
        }
    }

    let mut found_url = None;
    while let Some(current_url) = queue.pop_front() {
        let current_aliases = url_aliases(&current_url);
        let mut outgoing = current_aliases
            .iter()
            .filter_map(|alias| adjacency.get(alias))
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        outgoing.sort_by(|left, right| {
            left.discovery_order
                .cmp(&right.discovery_order)
                .then_with(|| left.source_position.cmp(&right.source_position))
                .then_with(|| left.target_url.cmp(&right.target_url))
        });
        outgoing.dedup_by_key(|edge| edge.id);

        for edge in outgoing {
            let child_url = edge.target_url.clone();
            let child_aliases = url_aliases(&child_url);
            if aliases_overlap(&child_aliases, &target_aliases) {
                parent_by_url.insert(child_url.clone(), (current_url.clone(), edge));
                found_url = Some(child_url);
                break;
            }
            if mark_aliases_visited(&mut visited_aliases, &child_aliases) {
                parent_by_url.insert(child_url.clone(), (current_url.clone(), edge));
                queue.push_back(child_url);
            }
        }

        if found_url.is_some() {
            break;
        }
    }

    let mut steps = Vec::new();
    if let Some(mut cursor) = found_url {
        let mut guard = 0;
        while let Some((source_url, edge)) = parent_by_url.get(&cursor).cloned() {
            steps.push(edge);
            cursor = source_url;
            guard += 1;
            if guard > max_edges {
                break;
            }
        }
        steps.reverse();
    }

    CrawlPathResponse {
        target_url,
        found: !steps.is_empty(),
        truncated: total_edges > max_edges,
        explored_edges: edges.len(),
        steps,
    }
}

fn mark_url_visited(visited_aliases: &mut HashSet<String>, url: &str) -> bool {
    mark_aliases_visited(visited_aliases, &url_aliases(url))
}

fn mark_aliases_visited(visited_aliases: &mut HashSet<String>, aliases: &HashSet<String>) -> bool {
    if aliases.iter().any(|alias| visited_aliases.contains(alias)) {
        return false;
    }
    visited_aliases.extend(aliases.iter().cloned());
    true
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
    let hreflang_links_json: String = row.get("hreflang_links")?;
    let hreflang_links = serde_json::from_str(&hreflang_links_json).unwrap_or_default();
    let structured_data_issues_json: String = row.get("structured_data_issues")?;
    let structured_data_issues =
        serde_json::from_str(&structured_data_issues_json).unwrap_or_default();
    let custom_extractions_json: String = row.get("custom_extractions")?;
    let custom_extractions = serde_json::from_str(&custom_extractions_json).unwrap_or_default();
    let custom_searches_json: String = row.get("custom_searches")?;
    let custom_searches = serde_json::from_str(&custom_searches_json).unwrap_or_default();
    let classification: String = row.get("classification")?;
    let id: i64 = row.get("id")?;
    let response_time_ms: i64 = row.get("response_time_ms")?;
    let dns_lookup_time_ms: Option<i64> = row.get("dns_lookup_time_ms")?;
    let tcp_connect_time_ms: Option<i64> = row.get("tcp_connect_time_ms")?;
    let tls_handshake_time_ms: Option<i64> = row.get("tls_handshake_time_ms")?;
    let ttfb_ms: Option<i64> = row.get("ttfb_ms")?;
    let download_time_ms: Option<i64> = row.get("download_time_ms")?;
    let total_network_time_ms: Option<i64> = row.get("total_network_time_ms")?;
    let transfer_rate_bytes_per_sec: Option<i64> = row.get("transfer_rate_bytes_per_sec")?;
    let size_bytes: i64 = row.get("size_bytes")?;
    let depth: i64 = row.get("depth")?;
    let list_position: Option<i64> = row.get("list_position")?;
    let list_duplicate_index: i64 = row.get("list_duplicate_index")?;
    let title_len: i64 = row.get("title_len")?;
    let meta_description_len: i64 = row.get("meta_description_len")?;
    let title_pixel_width: i64 = row.get("title_pixel_width")?;
    let meta_description_pixel_width: i64 = row.get("meta_description_pixel_width")?;
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
        storage_key: row.get("storage_key")?,
        url: row.get("url")?,
        final_url: row.get("final_url")?,
        list_position: list_position.map(|value| value as u32),
        list_duplicate_index: list_duplicate_index.max(0) as u32,
        classification: classification_from_str(&classification),
        in_sitemap: row.get("in_sitemap")?,
        status_code: row.get("status_code")?,
        status_text: row.get("status_text")?,
        content_type: row.get("content_type")?,
        indexability: row.get("indexability")?,
        indexability_status: row.get("indexability_status")?,
        response_time_ms: response_time_ms as u64,
        dns_lookup_time_ms: dns_lookup_time_ms.map(|value| value as u64),
        tcp_connect_time_ms: tcp_connect_time_ms.map(|value| value as u64),
        tls_handshake_time_ms: tls_handshake_time_ms.map(|value| value as u64),
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
        title_pixel_width: title_pixel_width.max(0) as u32,
        meta_description: row.get("meta_description")?,
        meta_description_len: meta_description_len as usize,
        meta_description_pixel_width: meta_description_pixel_width.max(0) as u32,
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
        hreflang_links,
        json_ld_count: row.get("json_ld_count")?,
        json_ld_invalid_count: row.get("json_ld_invalid_count")?,
        structured_data_error_count: row.get("structured_data_error_count")?,
        structured_data_warning_count: row.get("structured_data_warning_count")?,
        structured_data_issues,
        open_graph_count: row.get("open_graph_count")?,
        twitter_card_count: row.get("twitter_card_count")?,
        deprecated_html_tag_count: row.get("deprecated_html_tag_count")?,
        duplicate_id_count: row.get("duplicate_id_count")?,
        js_rendered: row.get("js_rendered")?,
        rendered_dom_changed: row.get("rendered_dom_changed")?,
        rendered_word_count_delta: row.get("rendered_word_count_delta")?,
        rendered_link_count_delta: row.get("rendered_link_count_delta")?,
        near_duplicate_cluster_id: near_duplicate_cluster_id.map(|value| value as u64),
        inlink_count: row.get("inlink_count")?,
        first_inlink_source_url: None,
        first_inlink_anchor_text: None,
        first_inlink_source_position: None,
        outlink_count: row.get("outlink_count")?,
        internal_outlink_count: row.get("internal_outlink_count")?,
        external_outlink_count: row.get("external_outlink_count")?,
        custom_extractions,
        custom_searches,
        search_console_clicks: row.get("search_console_clicks")?,
        search_console_impressions: row.get("search_console_impressions")?,
        search_console_ctr: row.get("search_console_ctr")?,
        search_console_average_position: row.get("search_console_average_position")?,
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

fn image_asset_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImageAsset> {
    let id: i64 = row.get("id")?;
    let alt_len: i64 = row.get("alt_len")?;
    let width: Option<i64> = row.get("width")?;
    let height: Option<i64> = row.get("height")?;
    let source_position: i64 = row.get("source_position")?;
    let size_bytes: Option<i64> = row.get("size_bytes")?;
    let size_bytes = size_bytes
        .filter(|value| *value >= 0)
        .map(|value| value as u64);

    Ok(ImageAsset {
        id: id.max(0) as u64,
        page_url: row.get("page_url")?,
        image_url: row.get("image_url")?,
        alt_text: row.get("alt_text")?,
        alt_len: alt_len.max(0) as u32,
        missing_alt: row.get("missing_alt")?,
        alt_too_long: row.get("alt_too_long")?,
        width: width.filter(|value| *value >= 0).map(|value| value as u32),
        height: height.filter(|value| *value >= 0).map(|value| value as u32),
        source_position: source_position.max(0) as u32,
        size_bytes,
        oversized: size_bytes
            .map(|value| value > IMAGE_ASSET_OVERSIZE_BYTES)
            .unwrap_or(false),
    })
}

fn frontier_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CrawlFrontierItem> {
    let depth: i64 = row.get("depth")?;
    let list_position: Option<i64> = row.get("list_position")?;
    let list_duplicate_index: i64 = row.get("list_duplicate_index")?;

    Ok(CrawlFrontierItem {
        url: row.get("url")?,
        depth: depth.max(0) as usize,
        from_sitemap: row.get("from_sitemap")?,
        storage_key: row.get("storage_key")?,
        list_position: list_position.map(|value| value.max(0) as u32),
        list_duplicate_index: list_duplicate_index.max(0) as u32,
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

fn sqlite_inlink_count_for_record(
    conn: &Connection,
    record: &CrawlRecord,
) -> Result<Option<u32>, StorageError> {
    let aliases = sorted_aliases(record_url_aliases(record));
    if aliases.is_empty() {
        return Ok(None);
    }
    let placeholders = sql_placeholders(aliases.len());
    let sql = format!("SELECT SUM(count) FROM inlink_counts WHERE url IN ({placeholders})");
    let count = conn.query_row(&sql, rusqlite::params_from_iter(aliases.iter()), |row| {
        row.get::<_, Option<i64>>(0)
    })?;
    Ok(count.and_then(|value| (value > 0).then_some(value as u32)))
}

fn sqlite_record_status(
    conn: &Connection,
    url: &str,
) -> Result<Option<(Option<u16>, usize)>, StorageError> {
    let aliases = sorted_aliases(url_aliases(url));
    if aliases.is_empty() {
        return Ok(None);
    }
    let placeholders = sql_placeholders(aliases.len());
    let sql = format!(
        "SELECT status_code, depth
         FROM crawl_records
         WHERE final_url IN ({placeholders})
            OR url IN ({placeholders})
            OR storage_key IN ({placeholders})
         ORDER BY id ASC
         LIMIT 1"
    );
    let args = repeat_args(&aliases, 3);
    let value = conn
        .query_row(&sql, rusqlite::params_from_iter(args.iter()), |row| {
            let status_code: Option<u16> = row.get(0)?;
            let depth: i64 = row.get(1)?;
            Ok((status_code, depth as usize))
        })
        .optional()?;
    Ok(value)
}

fn update_sqlite_edge_statuses(
    conn: &Connection,
    record: &CrawlRecord,
) -> Result<(), StorageError> {
    let aliases = sorted_aliases(record_url_aliases(record));
    if aliases.is_empty() {
        return Ok(());
    }
    let placeholders = sql_placeholders(aliases.len());
    let source_sql = format!(
        "UPDATE link_edges
         SET source_status_code = ?1, source_depth = ?2
         WHERE source_url IN ({placeholders})"
    );
    let target_sql = format!(
        "UPDATE link_edges
         SET target_status_code = ?1, target_depth = ?2
         WHERE target_url IN ({placeholders})"
    );
    let mut params = vec![
        optional_u16_value(record.status_code),
        Value::Integer(record.depth as i64),
    ];
    params.extend(aliases.iter().cloned().map(Value::Text));
    conn.execute(&source_sql, rusqlite::params_from_iter(params.iter()))?;
    conn.execute(&target_sql, rusqlite::params_from_iter(params.iter()))?;
    Ok(())
}

fn apply_first_inlink_sources(records: &mut [CrawlRecord], edges: &[LinkEdge]) {
    let mut first_sources: HashMap<String, FirstInlinkSource> = HashMap::new();
    for edge in edges {
        let candidate = FirstInlinkSource {
            source_url: edge.source_url.clone(),
            anchor_text: edge.anchor_text.clone(),
            source_position: edge.source_position,
            discovery_order: edge.discovery_order,
        };
        for alias in url_aliases(&edge.target_url) {
            first_sources
                .entry(alias)
                .and_modify(|current| {
                    if first_source_order(&candidate, current) == Ordering::Less {
                        *current = candidate.clone();
                    }
                })
                .or_insert_with(|| candidate.clone());
        }
    }

    for record in records {
        record.first_inlink_source_url = None;
        record.first_inlink_anchor_text = None;
        record.first_inlink_source_position = None;
        let record_aliases = record_url_aliases(record);
        if let Some(first_source) = record_aliases
            .iter()
            .filter_map(|alias| first_sources.get(alias))
            .min_by(|left, right| first_source_order(left, right))
        {
            record.first_inlink_source_url = Some(first_source.source_url.clone());
            record.first_inlink_anchor_text = Some(first_source.anchor_text.clone());
            record.first_inlink_source_position = Some(first_source.source_position);
        }
    }
}

fn first_source_order(left: &FirstInlinkSource, right: &FirstInlinkSource) -> Ordering {
    left.discovery_order
        .cmp(&right.discovery_order)
        .then_with(|| left.source_position.cmp(&right.source_position))
        .then_with(|| left.source_url.cmp(&right.source_url))
}

fn annotate_sqlite_first_inlink_sources(
    conn: &Connection,
    records: &mut [CrawlRecord],
) -> Result<(), StorageError> {
    for record in records.iter_mut() {
        record.first_inlink_source_url = None;
        record.first_inlink_anchor_text = None;
        record.first_inlink_source_position = None;
    }
    if records.is_empty() {
        return Ok(());
    }

    let mut target_urls = records
        .iter()
        .flat_map(record_url_aliases)
        .collect::<Vec<_>>();
    target_urls.sort();
    target_urls.dedup();

    let mut first_sources: HashMap<String, FirstInlinkSource> = HashMap::new();
    for chunk in target_urls.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT source_url, target_url, anchor_text, source_position, discovery_order
             FROM link_edges
             WHERE target_url IN ({placeholders})
             ORDER BY target_url ASC, discovery_order ASC, source_position ASC, id ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
            let source_position: i64 = row.get("source_position")?;
            let discovery_order: i64 = row.get("discovery_order")?;
            Ok((
                row.get::<_, String>("target_url")?,
                FirstInlinkSource {
                    source_url: row.get("source_url")?,
                    anchor_text: row.get("anchor_text")?,
                    source_position: source_position.max(0) as u32,
                    discovery_order: discovery_order.max(0) as u64,
                },
            ))
        })?;
        for row in rows {
            let (target_url, source) = row?;
            for alias in url_aliases(&target_url) {
                first_sources
                    .entry(alias)
                    .and_modify(|current| {
                        if first_source_order(&source, current) == Ordering::Less {
                            *current = source.clone();
                        }
                    })
                    .or_insert_with(|| source.clone());
            }
        }
    }

    for record in records {
        let record_aliases = record_url_aliases(record);
        if let Some(first_source) = record_aliases
            .iter()
            .filter_map(|alias| first_sources.get(alias))
            .min_by(|left, right| first_source_order(left, right))
        {
            record.first_inlink_source_url = Some(first_source.source_url.clone());
            record.first_inlink_anchor_text = Some(first_source.anchor_text.clone());
            record.first_inlink_source_position = Some(first_source.source_position);
        }
    }

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

fn migrate_final_url_unique_constraint(conn: &Connection) -> Result<(), StorageError> {
    if !has_unique_index_on_columns(conn, "crawl_records", &["final_url"])? {
        return Ok(());
    }

    conn.execute_batch(
        "
        BEGIN IMMEDIATE;

        ALTER TABLE crawl_records RENAME TO crawl_records_old;

        CREATE TABLE crawl_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            storage_key TEXT NOT NULL UNIQUE,
            url TEXT NOT NULL,
            final_url TEXT NOT NULL,
            list_position INTEGER,
            list_duplicate_index INTEGER NOT NULL DEFAULT 0,
            classification TEXT NOT NULL,
            status_code INTEGER,
            status_text TEXT NOT NULL,
            content_type TEXT,
            indexability TEXT NOT NULL,
            indexability_status TEXT NOT NULL,
            response_time_ms INTEGER NOT NULL,
            dns_lookup_time_ms INTEGER,
            tcp_connect_time_ms INTEGER,
            tls_handshake_time_ms INTEGER,
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
            title_pixel_width INTEGER NOT NULL DEFAULT 0,
            meta_description TEXT,
            meta_description_len INTEGER NOT NULL,
            meta_description_pixel_width INTEGER NOT NULL DEFAULT 0,
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
            hreflang_links TEXT NOT NULL DEFAULT '[]',
            json_ld_count INTEGER NOT NULL DEFAULT 0,
            json_ld_invalid_count INTEGER NOT NULL DEFAULT 0,
            structured_data_error_count INTEGER NOT NULL DEFAULT 0,
            structured_data_warning_count INTEGER NOT NULL DEFAULT 0,
            structured_data_issues TEXT NOT NULL DEFAULT '[]',
            open_graph_count INTEGER NOT NULL DEFAULT 0,
            twitter_card_count INTEGER NOT NULL DEFAULT 0,
            deprecated_html_tag_count INTEGER NOT NULL DEFAULT 0,
            duplicate_id_count INTEGER NOT NULL DEFAULT 0,
            js_rendered INTEGER NOT NULL DEFAULT 0,
            rendered_dom_changed INTEGER NOT NULL DEFAULT 0,
            rendered_word_count_delta INTEGER NOT NULL DEFAULT 0,
            rendered_link_count_delta INTEGER NOT NULL DEFAULT 0,
            near_duplicate_cluster_id INTEGER,
            inlink_count INTEGER NOT NULL,
            outlink_count INTEGER NOT NULL,
            internal_outlink_count INTEGER NOT NULL,
            external_outlink_count INTEGER NOT NULL,
            custom_extractions TEXT NOT NULL DEFAULT '[]',
            custom_searches TEXT NOT NULL DEFAULT '[]',
            search_console_clicks REAL,
            search_console_impressions REAL,
            search_console_ctr REAL,
            search_console_average_position REAL,
            error TEXT,
            in_sitemap INTEGER NOT NULL DEFAULT 0
        );

        INSERT INTO crawl_records (
            id,
            storage_key,
            url,
            final_url,
            list_position,
            list_duplicate_index,
            classification,
            status_code,
            status_text,
            content_type,
            indexability,
            indexability_status,
            response_time_ms,
            dns_lookup_time_ms,
            tcp_connect_time_ms,
            tls_handshake_time_ms,
            ttfb_ms,
            download_time_ms,
            total_network_time_ms,
            transfer_rate_bytes_per_sec,
            resolved_ip_count,
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
            hreflang_links,
            json_ld_count,
            json_ld_invalid_count,
            structured_data_error_count,
            structured_data_warning_count,
            structured_data_issues,
            open_graph_count,
            twitter_card_count,
            near_duplicate_cluster_id,
            inlink_count,
            outlink_count,
            internal_outlink_count,
            external_outlink_count,
            custom_extractions,
            error,
            in_sitemap
        )
        SELECT
            id,
            storage_key,
            url,
            final_url,
            list_position,
            list_duplicate_index,
            classification,
            status_code,
            status_text,
            content_type,
            indexability,
            indexability_status,
            response_time_ms,
            dns_lookup_time_ms,
            NULL,
            NULL,
            ttfb_ms,
            download_time_ms,
            total_network_time_ms,
            transfer_rate_bytes_per_sec,
            resolved_ip_count,
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
            hreflang_links,
            json_ld_count,
            json_ld_invalid_count,
            structured_data_error_count,
            structured_data_warning_count,
            structured_data_issues,
            open_graph_count,
            twitter_card_count,
            near_duplicate_cluster_id,
            inlink_count,
            outlink_count,
            internal_outlink_count,
            external_outlink_count,
            custom_extractions,
            error,
            in_sitemap
        FROM crawl_records_old;

        DROP TABLE crawl_records_old;

        COMMIT;
        ",
    )?;

    Ok(())
}

fn has_unique_index_on_columns(
    conn: &Connection,
    table: &str,
    expected_columns: &[&str],
) -> Result<bool, StorageError> {
    let pragma = format!("PRAGMA index_list({})", sqlite_identifier(table));
    let mut stmt = conn.prepare(&pragma)?;
    let indexes = stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        let unique: i64 = row.get(2)?;
        Ok((name, unique != 0))
    })?;

    for index in indexes {
        let (name, unique) = index?;
        if !unique {
            continue;
        }
        let columns = index_columns(conn, &name)?;
        let matches_expected = columns.len() == expected_columns.len()
            && columns
                .iter()
                .zip(expected_columns.iter())
                .all(|(left, right)| left == right);
        if matches_expected {
            return Ok(true);
        }
    }

    Ok(false)
}

fn index_columns(conn: &Connection, index_name: &str) -> Result<Vec<String>, StorageError> {
    let pragma = format!("PRAGMA index_info({})", sqlite_identifier(index_name));
    let mut stmt = conn.prepare(&pragma)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(2))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }
    Ok(columns)
}

fn sqlite_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn create_crawl_record_indexes(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_crawl_records_status_code ON crawl_records(status_code);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_storage_key ON crawl_records(storage_key);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_final_url ON crawl_records(final_url);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_list_position ON crawl_records(list_position);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_classification ON crawl_records(classification);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_depth ON crawl_records(depth);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_title ON crawl_records(title);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_meta_description ON crawl_records(meta_description);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_near_duplicate_cluster_id ON crawl_records(near_duplicate_cluster_id);
        ",
    )?;
    Ok(())
}

fn needs_duplicate_filter(view: &IssueView) -> bool {
    matches!(
        view,
        IssueView::TitleDuplicate
            | IssueView::MetaDuplicate
            | IssueView::H1Duplicate
            | IssueView::H2Duplicate
            | IssueView::HreflangMissingReturnLink
            | IssueView::HreflangNonCanonicalTarget
    )
}

fn needs_regex_segment_filter(query: &GridQuery) -> bool {
    query.segment_regex
        && query
            .segment_pattern
            .as_ref()
            .map(|pattern| !pattern.trim().is_empty())
            .unwrap_or(false)
}

enum SegmentMatcher {
    Contains(String),
    Regex(Option<Regex>),
}

impl SegmentMatcher {
    fn from_query(query: &GridQuery) -> Option<Self> {
        let pattern = query.segment_pattern.as_ref()?.trim();
        if pattern.is_empty() {
            return None;
        }
        if query.segment_regex {
            return Some(Self::Regex(Regex::new(pattern).ok()));
        }
        Some(Self::Contains(pattern.to_lowercase()))
    }

    fn matches(&self, record: &CrawlRecord) -> bool {
        match self {
            Self::Contains(pattern) => {
                record.url.to_lowercase().contains(pattern)
                    || record.final_url.to_lowercase().contains(pattern)
            }
            Self::Regex(Some(regex)) => {
                regex.is_match(&record.url) || regex.is_match(&record.final_url)
            }
            Self::Regex(None) => false,
        }
    }
}

fn query_filter_sql(query: &GridQuery) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut args = Vec::new();

    if is_html_audit_view(&query.view) {
        clauses.push(SUCCESS_HTML_SQL.to_string());
    }

    match query.view {
        IssueView::All => {}
        IssueView::Internal => clauses.push("classification = 'internal'".to_string()),
        IssueView::External => clauses.push("classification = 'external'".to_string()),
        IssueView::Status2xx => clauses.push("status_code >= 200 AND status_code < 300".to_string()),
        IssueView::Status3xx => clauses.push("(status_code >= 300 AND status_code < 400 OR redirect_chain != '[]')".to_string()),
        IssueView::Status4xx => clauses.push("status_code >= 400 AND status_code < 500".to_string()),
        IssueView::Status5xx => clauses.push("status_code >= 500".to_string()),
        IssueView::NoResponse => clauses.push(NO_RESPONSE_SQL.to_string()),
        IssueView::TitleMissing => clauses.push("(title IS NULL OR trim(title) = '')".to_string()),
        IssueView::TitleDuplicate => {}
        IssueView::TitleTooShort => clauses.push("(title IS NOT NULL AND trim(title) != '' AND title_len < 30)".to_string()),
        IssueView::TitleTooLong => clauses.push("title_len > 60".to_string()),
        IssueView::TitlePixelTooNarrow => clauses.push("(title IS NOT NULL AND trim(title) != '' AND title_pixel_width < 200)".to_string()),
        IssueView::TitlePixelTooWide => clauses.push("title_pixel_width > 580".to_string()),
        IssueView::MetaMissing => clauses.push("(meta_description IS NULL OR trim(meta_description) = '')".to_string()),
        IssueView::MetaDuplicate => {}
        IssueView::MetaTooShort => clauses.push("(meta_description IS NOT NULL AND trim(meta_description) != '' AND meta_description_len < 70)".to_string()),
        IssueView::MetaTooLong => clauses.push("meta_description_len > 160".to_string()),
        IssueView::MetaPixelTooNarrow => clauses.push("(meta_description IS NOT NULL AND trim(meta_description) != '' AND meta_description_pixel_width < 400)".to_string()),
        IssueView::MetaPixelTooWide => clauses.push("meta_description_pixel_width > 920".to_string()),
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
        IssueView::HreflangMissingReturnLink => {}
        IssueView::HreflangNonCanonicalTarget => {}
        IssueView::StructuredDataInvalid => clauses.push(
            "(structured_data_error_count > 0 OR json_ld_invalid_count > 0)".to_string(),
        ),
        IssueView::StructuredDataWarning => {
            clauses.push("structured_data_warning_count > 0".to_string())
        }
        IssueView::HtmlDeprecatedTags => clauses.push("deprecated_html_tag_count > 0".to_string()),
        IssueView::HtmlDuplicateIds => clauses.push("duplicate_id_count > 0".to_string()),
        IssueView::RenderedDomChanged => clauses.push("rendered_dom_changed != 0".to_string()),
        IssueView::NearDuplicate => clauses.push(format!(
            "near_duplicate_cluster_id IS NOT NULL AND near_duplicate_cluster_id IN (
                SELECT near_duplicate_cluster_id
                FROM crawl_records
                WHERE near_duplicate_cluster_id IS NOT NULL AND {SUCCESS_HTML_SQL}
                GROUP BY near_duplicate_cluster_id
                HAVING COUNT(*) > 1
            )"
        )),
        IssueView::BrokenLinks => clauses.push(broken_record_sql()),
        IssueView::SitemapOrphan => clauses
            .push("in_sitemap != 0 AND inlink_count = 0 AND classification = 'internal'".to_string()),
    }

    if let Some(search) = query.global_search.as_ref().map(|value| value.trim())
        && !search.is_empty()
    {
        clauses.push(
                "(lower(url) LIKE ? OR lower(final_url) LIKE ? OR lower(title) LIKE ? OR lower(meta_description) LIKE ? OR lower(meta_robots) LIKE ? OR lower(x_robots_tag) LIKE ? OR lower(h1) LIKE ? OR lower(h2) LIKE ? OR lower(canonical) LIKE ? OR lower(amphtml) LIKE ? OR lower(rel_next) LIKE ? OR lower(rel_prev) LIKE ? OR lower(response_hash) LIKE ? OR lower(custom_extractions) LIKE ? OR lower(custom_searches) LIKE ? OR lower(structured_data_issues) LIKE ? OR CAST(status_code AS TEXT) LIKE ? OR CAST(near_duplicate_cluster_id AS TEXT) LIKE ? OR CAST(list_position AS TEXT) LIKE ? OR CAST(deprecated_html_tag_count AS TEXT) LIKE ? OR CAST(duplicate_id_count AS TEXT) LIKE ? OR CAST(js_rendered AS TEXT) LIKE ? OR CAST(rendered_dom_changed AS TEXT) LIKE ? OR CAST(rendered_word_count_delta AS TEXT) LIKE ? OR CAST(rendered_link_count_delta AS TEXT) LIKE ? OR CAST(search_console_clicks AS TEXT) LIKE ? OR CAST(search_console_impressions AS TEXT) LIKE ? OR CAST(search_console_ctr AS TEXT) LIKE ? OR CAST(search_console_average_position AS TEXT) LIKE ?)"
                    .to_string(),
            );
        let pattern = format!("%{}%", search.to_lowercase());
        for _ in 0..29 {
            args.push(pattern.clone());
        }
    }

    if let Some(segment) = query.segment_pattern.as_ref().map(|value| value.trim())
        && !segment.is_empty()
        && !query.segment_regex
    {
        clauses.push("(lower(url) LIKE ? OR lower(final_url) LIKE ?)".to_string());
        let pattern = format!("%{}%", segment.to_lowercase());
        args.push(pattern.clone());
        args.push(pattern);
    }

    if clauses.is_empty() {
        (String::new(), args)
    } else {
        (format!(" WHERE {}", clauses.join(" AND ")), args)
    }
}

fn link_edge_filter_sql(
    query: &LinkEdgeQuery,
    conn: &Connection,
) -> Result<(String, Vec<String>), StorageError> {
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
            let mut stmt = conn.prepare(&format!(
                "SELECT storage_key, url, final_url FROM crawl_records WHERE {}",
                broken_record_sql()
            ))?;
            let records = stmt.query_map([], |row| {
                Ok([
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ])
            })?;
            let mut failed_targets = HashSet::new();
            for record in records {
                for url in record? {
                    add_url_aliases(&mut failed_targets, &url);
                }
            }
            clauses.push(
                "(target_status_code >= 400 OR target_url IN (SELECT value FROM json_each(?)))"
                    .to_string(),
            );
            args.push(serde_json::to_string(&failed_targets)?);
        }
        LinkEdgeView::Nofollow => clauses.push("rel_nofollow != 0".to_string()),
    }
    if let Some(source_url) = query.source_url.as_ref().map(|value| value.trim())
        && !source_url.is_empty()
    {
        clauses.push("source_url = ?".to_string());
        args.push(source_url.to_string());
    }
    if let Some(target_url) = query.target_url.as_ref().map(|value| value.trim())
        && !target_url.is_empty()
    {
        clauses.push("target_url = ?".to_string());
        args.push(target_url.to_string());
    }
    if let Some(search) = query.global_search.as_ref().map(|value| value.trim())
        && !search.is_empty()
    {
        clauses.push(
                "(lower(source_url) LIKE ? OR lower(target_url) LIKE ? OR lower(anchor_text) LIKE ? OR lower(rel) LIKE ? OR lower(link_type) LIKE ? OR CAST(source_status_code AS TEXT) LIKE ? OR CAST(target_status_code AS TEXT) LIKE ? OR CAST(source_depth AS TEXT) LIKE ? OR CAST(target_depth AS TEXT) LIKE ? OR CAST(source_position AS TEXT) LIKE ?)"
                    .to_string(),
            );
        let pattern = format!("%{}%", search.to_lowercase());
        for _ in 0..10 {
            args.push(pattern.clone());
        }
    }

    if clauses.is_empty() {
        Ok((String::new(), args))
    } else {
        Ok((format!(" WHERE {}", clauses.join(" AND ")), args))
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

fn custom_sort_name(sort_by: &str) -> Option<String> {
    let value = sort_by.strip_prefix("custom:")?;
    let (name, _) = value.rsplit_once(':')?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn custom_search_sort_name(sort_by: &str) -> Option<String> {
    let value = sort_by.strip_prefix("search:")?;
    let (name, _) = value.rsplit_once(':')?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn sqlite_custom_extraction_sort_expression(name: &str) -> String {
    let escaped_name = name.replace('\'', "''");
    format!(
        "(SELECT lower(COALESCE(json_extract(value, '$.values[0]'), ''))
          FROM json_each(crawl_records.custom_extractions)
          WHERE json_extract(value, '$.name') = '{escaped_name}'
          LIMIT 1)"
    )
}

fn sqlite_custom_search_sort_expression(name: &str) -> String {
    let escaped_name = name.replace('\'', "''");
    format!(
        "(SELECT COALESCE(SUM(COALESCE(json_extract(value, '$.matchCount'), 0)), 0)
          FROM json_each(crawl_records.custom_searches)
          WHERE json_extract(value, '$.name') = '{escaped_name}')"
    )
}

fn sort_column(sort_by: Option<&str>) -> Option<String> {
    match sort_by {
        Some(value) if value.starts_with("custom:") => {
            custom_sort_name(value).map(|name| sqlite_custom_extraction_sort_expression(&name))
        }
        Some(value) if value.starts_with("search:") => {
            custom_search_sort_name(value).map(|name| sqlite_custom_search_sort_expression(&name))
        }
        Some("statusCode") => Some("status_code".to_string()),
        Some("responseTimeMs") => Some("response_time_ms".to_string()),
        Some("dnsLookupTimeMs") => Some("dns_lookup_time_ms".to_string()),
        Some("tcpConnectTimeMs") => Some("tcp_connect_time_ms".to_string()),
        Some("tlsHandshakeTimeMs") => Some("tls_handshake_time_ms".to_string()),
        Some("ttfbMs") => Some("ttfb_ms".to_string()),
        Some("downloadTimeMs") => Some("download_time_ms".to_string()),
        Some("totalNetworkTimeMs") => Some("total_network_time_ms".to_string()),
        Some("transferRateBytesPerSec") => Some("transfer_rate_bytes_per_sec".to_string()),
        Some("resolvedIpCount") => Some("resolved_ip_count".to_string()),
        Some("inSitemap") => Some("in_sitemap".to_string()),
        Some("listPosition") => Some("list_position".to_string()),
        Some("listDuplicateIndex") => Some("list_duplicate_index".to_string()),
        Some("sizeBytes") => Some("size_bytes".to_string()),
        Some("depth") => Some("depth".to_string()),
        Some("titleLen") => Some("title_len".to_string()),
        Some("titlePixelWidth") => Some("title_pixel_width".to_string()),
        Some("metaDescription") => Some("meta_description".to_string()),
        Some("metaDescriptionLen") => Some("meta_description_len".to_string()),
        Some("metaDescriptionPixelWidth") => Some("meta_description_pixel_width".to_string()),
        Some("h1") => Some("h1".to_string()),
        Some("h1Len") => Some("h1_len".to_string()),
        Some("h1Count") => Some("h1_count".to_string()),
        Some("h2") => Some("h2".to_string()),
        Some("h2Len") => Some("h2_len".to_string()),
        Some("h2Count") => Some("h2_count".to_string()),
        Some("canonicalCount") => Some("canonical_count".to_string()),
        Some("wordCount") => Some("word_count".to_string()),
        Some("textToCodeRatio") => Some("text_to_code_ratio".to_string()),
        Some("imageCount") => Some("image_count".to_string()),
        Some("imagesMissingAlt") => Some("images_missing_alt".to_string()),
        Some("imagesAltTooLong") => Some("images_alt_too_long".to_string()),
        Some("mixedContentCount") => Some("mixed_content_count".to_string()),
        Some("insecureFormCount") => Some("insecure_form_count".to_string()),
        Some("hreflangCount") => Some("hreflang_count".to_string()),
        Some("hreflangInvalidCount") => Some("hreflang_invalid_count".to_string()),
        Some("jsonLdCount") => Some("json_ld_count".to_string()),
        Some("jsonLdInvalidCount") => Some("json_ld_invalid_count".to_string()),
        Some("structuredDataErrorCount") => Some("structured_data_error_count".to_string()),
        Some("structuredDataWarningCount") => Some("structured_data_warning_count".to_string()),
        Some("openGraphCount") => Some("open_graph_count".to_string()),
        Some("twitterCardCount") => Some("twitter_card_count".to_string()),
        Some("deprecatedHtmlTagCount") => Some("deprecated_html_tag_count".to_string()),
        Some("duplicateIdCount") => Some("duplicate_id_count".to_string()),
        Some("jsRendered") => Some("js_rendered".to_string()),
        Some("renderedDomChanged") => Some("rendered_dom_changed".to_string()),
        Some("renderedWordCountDelta") => Some("rendered_word_count_delta".to_string()),
        Some("renderedLinkCountDelta") => Some("rendered_link_count_delta".to_string()),
        Some("searchConsoleClicks") => Some("search_console_clicks".to_string()),
        Some("searchConsoleImpressions") => Some("search_console_impressions".to_string()),
        Some("searchConsoleCtr") => Some("search_console_ctr".to_string()),
        Some("searchConsoleAveragePosition") => Some("search_console_average_position".to_string()),
        Some("nearDuplicateClusterId") => Some("near_duplicate_cluster_id".to_string()),
        Some("inlinkCount") => Some("inlink_count".to_string()),
        Some("outlinkCount") => Some("outlink_count".to_string()),
        Some("title") => Some("title".to_string()),
        Some("url") => Some("url".to_string()),
        Some("finalUrl") => Some("final_url".to_string()),
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

fn sqlite_duplicate_count(conn: &Connection, column: &str) -> Result<usize, StorageError> {
    let column = sqlite_identifier(column);
    let mut stmt = conn.prepare(&format!(
        "SELECT {column} FROM crawl_records WHERE {SUCCESS_HTML_SQL} AND {column} IS NOT NULL"
    ))?;
    let values = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut counts = HashMap::<String, usize>::new();
    for value in values {
        let key = normalize_text_key(&value?);
        if !key.is_empty() {
            *counts.entry(key).or_default() += 1;
        }
    }
    Ok(counts.values().filter(|count| **count > 1).sum())
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
    annotate_sqlite_first_inlink_sources(conn, &mut records)?;
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

#[derive(Clone, Debug)]
struct HreflangAuditRecord {
    aliases: HashSet<String>,
    canonical: Option<String>,
    hreflang_links: Vec<HreflangLink>,
}

#[derive(Clone, Debug, Default)]
struct HreflangAuditIndex {
    records_by_alias: HashMap<String, HreflangAuditRecord>,
}

impl HreflangAuditIndex {
    fn from_records(records: &[CrawlRecord]) -> Self {
        let mut records_by_alias = HashMap::new();
        for record in records {
            let aliases = record_url_aliases(record);
            let audit_record = HreflangAuditRecord {
                aliases: aliases.clone(),
                canonical: record.canonical.clone(),
                hreflang_links: record.hreflang_links.clone(),
            };
            for alias in aliases {
                records_by_alias
                    .entry(alias)
                    .or_insert_with(|| audit_record.clone());
            }
        }
        Self { records_by_alias }
    }

    fn find(&self, url: &str) -> Option<&HreflangAuditRecord> {
        url_aliases(url)
            .into_iter()
            .find_map(|alias| self.records_by_alias.get(&alias))
    }
}

fn aliases_overlap(left: &HashSet<String>, right: &HashSet<String>) -> bool {
    left.iter().any(|alias| right.contains(alias))
}

fn canonical_points_outside_aliases(canonical: Option<&str>, aliases: &HashSet<String>) -> bool {
    let Some(canonical) = canonical.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    !aliases_overlap(&url_aliases(canonical), aliases)
}

fn record_has_hreflang_missing_return_link(
    record: &CrawlRecord,
    hreflang_index: &HreflangAuditIndex,
) -> bool {
    let source_aliases = record_url_aliases(record);

    record
        .hreflang_links
        .iter()
        .filter(|link| link.valid)
        .any(|link| {
            let target_aliases = url_aliases(&link.url);
            if aliases_overlap(&source_aliases, &target_aliases) {
                return false;
            }

            hreflang_index.find(&link.url).is_some_and(|target| {
                !target
                    .hreflang_links
                    .iter()
                    .filter(|link| link.valid)
                    .any(|return_link| {
                        aliases_overlap(&source_aliases, &url_aliases(&return_link.url))
                    })
            })
        })
}

fn record_has_hreflang_non_canonical_target(
    record: &CrawlRecord,
    hreflang_index: &HreflangAuditIndex,
) -> bool {
    record
        .hreflang_links
        .iter()
        .filter(|link| link.valid)
        .any(|link| {
            hreflang_index.find(&link.url).is_some_and(|target| {
                canonical_points_outside_aliases(target.canonical.as_deref(), &target.aliases)
            })
        })
}

fn normalize_text_key(value: &str) -> String {
    compact_text(value).to_lowercase()
}

fn compact_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[expect(
    clippy::too_many_arguments,
    reason = "Borrows precomputed audit indexes without rebuilding them per row"
)]
fn matches_view(
    row: &CrawlRecord,
    view: &IssueView,
    title_counts: &HashMap<String, usize>,
    meta_counts: &HashMap<String, usize>,
    h1_counts: &HashMap<String, usize>,
    h2_counts: &HashMap<String, usize>,
    near_duplicate_counts: &HashMap<u64, usize>,
    hreflang_index: &HreflangAuditIndex,
) -> bool {
    if is_html_audit_view(view) && !is_success_html_record(row) {
        return false;
    }
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
        IssueView::NoResponse => is_no_response_record(row),
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
        IssueView::TitlePixelTooNarrow => {
            let title = row.title.as_deref().unwrap_or("").trim();
            !title.is_empty() && row.title_pixel_width < 200
        }
        IssueView::TitlePixelTooWide => row.title_pixel_width > 580,
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
        IssueView::MetaPixelTooNarrow => {
            let meta = row.meta_description.as_deref().unwrap_or("").trim();
            !meta.is_empty() && row.meta_description_pixel_width < 400
        }
        IssueView::MetaPixelTooWide => row.meta_description_pixel_width > 920,
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
        IssueView::HreflangMissingReturnLink => {
            record_has_hreflang_missing_return_link(row, hreflang_index)
        }
        IssueView::HreflangNonCanonicalTarget => {
            record_has_hreflang_non_canonical_target(row, hreflang_index)
        }
        IssueView::StructuredDataInvalid => {
            row.structured_data_error_count > 0 || row.json_ld_invalid_count > 0
        }
        IssueView::StructuredDataWarning => row.structured_data_warning_count > 0,
        IssueView::HtmlDeprecatedTags => row.deprecated_html_tag_count > 0,
        IssueView::HtmlDuplicateIds => row.duplicate_id_count > 0,
        IssueView::RenderedDomChanged => row.rendered_dom_changed,
        IssueView::NearDuplicate => {
            row.near_duplicate_cluster_id
                .and_then(|cluster_id| near_duplicate_counts.get(&cluster_id).copied())
                .unwrap_or(0)
                > 1
        }
        IssueView::BrokenLinks => is_broken_record(row),
        IssueView::SitemapOrphan => {
            row.in_sitemap
                && row.inlink_count == 0
                && row.classification == UrlClassification::Internal
        }
    }
}

fn is_html_audit_view(view: &IssueView) -> bool {
    matches!(
        view,
        IssueView::TitleMissing
            | IssueView::TitleDuplicate
            | IssueView::TitleTooShort
            | IssueView::TitleTooLong
            | IssueView::TitlePixelTooNarrow
            | IssueView::TitlePixelTooWide
            | IssueView::MetaMissing
            | IssueView::MetaDuplicate
            | IssueView::MetaTooShort
            | IssueView::MetaTooLong
            | IssueView::MetaPixelTooNarrow
            | IssueView::MetaPixelTooWide
            | IssueView::H1Missing
            | IssueView::H1Duplicate
            | IssueView::H1TooLong
            | IssueView::H2Missing
            | IssueView::H2Duplicate
            | IssueView::H2TooLong
            | IssueView::TitleSameAsH1
            | IssueView::CanonicalMissing
            | IssueView::CanonicalMultiple
            | IssueView::ImagesMissingAlt
            | IssueView::ImagesAltTooLong
            | IssueView::SecurityMixedContent
            | IssueView::SecurityInsecureForms
            | IssueView::SecurityMissingCsp
            | IssueView::SecurityMissingXFrameOptions
            | IssueView::MobileMissingViewport
            | IssueView::HreflangInvalid
            | IssueView::HreflangMissingSelfReference
            | IssueView::HreflangMissingReturnLink
            | IssueView::HreflangNonCanonicalTarget
            | IssueView::StructuredDataInvalid
            | IssueView::StructuredDataWarning
            | IssueView::HtmlDeprecatedTags
            | IssueView::HtmlDuplicateIds
            | IssueView::RenderedDomChanged
            | IssueView::NearDuplicate
    )
}

pub fn is_robots_blocked_record(row: &CrawlRecord) -> bool {
    row.status_code.is_none()
        && (row.status_text == "Blocked by robots.txt"
            || row.error.as_deref() == Some("Blocked by robots.txt"))
}

pub fn is_no_response_record(row: &CrawlRecord) -> bool {
    row.status_code.is_none() && row.error.is_some() && !is_robots_blocked_record(row)
}

pub fn is_broken_record(row: &CrawlRecord) -> bool {
    is_no_response_record(row)
        || matches!(row.status_code, Some(code) if code >= 400
            || ((300..400).contains(&code) && row.error.is_some()))
}

fn broken_record_sql() -> String {
    format!(
        "(status_code >= 400 OR (status_code >= 300 AND status_code < 400 AND error IS NOT NULL) OR ({NO_RESPONSE_SQL}))"
    )
}

pub fn is_success_record(row: &CrawlRecord) -> bool {
    matches!(row.status_code, Some(code) if (200..300).contains(&code))
}

pub fn is_success_html_record(row: &CrawlRecord) -> bool {
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
        || row
            .list_position
            .map(|position| position.to_string().contains(search))
            .unwrap_or(false)
        || row.deprecated_html_tag_count.to_string().contains(search)
        || row.duplicate_id_count.to_string().contains(search)
        || row.js_rendered.to_string().contains(search)
        || row.rendered_dom_changed.to_string().contains(search)
        || row.rendered_word_count_delta.to_string().contains(search)
        || row.rendered_link_count_delta.to_string().contains(search)
        || row
            .search_console_clicks
            .map(|value| value.to_string().contains(search))
            .unwrap_or(false)
        || row
            .search_console_impressions
            .map(|value| value.to_string().contains(search))
            .unwrap_or(false)
        || row
            .search_console_ctr
            .map(|value| value.to_string().contains(search))
            .unwrap_or(false)
        || row
            .search_console_average_position
            .map(|value| value.to_string().contains(search))
            .unwrap_or(false)
        || row
            .first_inlink_source_url
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .first_inlink_anchor_text
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(search)
        || row
            .first_inlink_source_position
            .map(|position| position.to_string().contains(search))
            .unwrap_or(false)
        || row.custom_extractions.iter().any(|extraction| {
            extraction.name.to_lowercase().contains(search)
                || extraction
                    .values
                    .iter()
                    .any(|value| value.to_lowercase().contains(search))
        })
        || row.custom_searches.iter().any(|custom_search| {
            custom_search.name.to_lowercase().contains(search)
                || custom_search.match_count.to_string().contains(search)
                || custom_search
                    .snippets
                    .iter()
                    .any(|value| value.to_lowercase().contains(search))
        })
        || row.structured_data_issues.iter().any(|issue| {
            issue.severity.to_lowercase().contains(search)
                || issue.message.to_lowercase().contains(search)
                || issue.path.to_lowercase().contains(search)
        })
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

fn compare_default_row_order(left: &CrawlRecord, right: &CrawlRecord) -> Ordering {
    left.list_position
        .map(u64::from)
        .unwrap_or(left.id)
        .cmp(&right.list_position.map(u64::from).unwrap_or(right.id))
        .then_with(|| left.id.cmp(&right.id))
}

fn compare_rows(left: &CrawlRecord, right: &CrawlRecord, sort_by: &str) -> Ordering {
    if let Some(name) = custom_sort_name(sort_by) {
        return custom_extraction_sort_value(left, &name)
            .cmp(&custom_extraction_sort_value(right, &name));
    }
    if let Some(name) = custom_search_sort_name(sort_by) {
        return custom_search_sort_value(left, &name).cmp(&custom_search_sort_value(right, &name));
    }

    match sort_by {
        "statusCode" => left.status_code.cmp(&right.status_code),
        "responseTimeMs" => left.response_time_ms.cmp(&right.response_time_ms),
        "dnsLookupTimeMs" => left.dns_lookup_time_ms.cmp(&right.dns_lookup_time_ms),
        "tcpConnectTimeMs" => left.tcp_connect_time_ms.cmp(&right.tcp_connect_time_ms),
        "tlsHandshakeTimeMs" => left.tls_handshake_time_ms.cmp(&right.tls_handshake_time_ms),
        "ttfbMs" => left.ttfb_ms.cmp(&right.ttfb_ms),
        "downloadTimeMs" => left.download_time_ms.cmp(&right.download_time_ms),
        "totalNetworkTimeMs" => left.total_network_time_ms.cmp(&right.total_network_time_ms),
        "transferRateBytesPerSec" => left
            .transfer_rate_bytes_per_sec
            .cmp(&right.transfer_rate_bytes_per_sec),
        "resolvedIpCount" => left.resolved_ip_count.cmp(&right.resolved_ip_count),
        "inSitemap" => left.in_sitemap.cmp(&right.in_sitemap),
        "listPosition" => left.list_position.cmp(&right.list_position),
        "listDuplicateIndex" => left.list_duplicate_index.cmp(&right.list_duplicate_index),
        "sizeBytes" => left.size_bytes.cmp(&right.size_bytes),
        "depth" => left.depth.cmp(&right.depth),
        "titleLen" => left.title_len.cmp(&right.title_len),
        "titlePixelWidth" => left.title_pixel_width.cmp(&right.title_pixel_width),
        "metaDescription" => left.meta_description.cmp(&right.meta_description),
        "metaDescriptionLen" => left.meta_description_len.cmp(&right.meta_description_len),
        "metaDescriptionPixelWidth" => left
            .meta_description_pixel_width
            .cmp(&right.meta_description_pixel_width),
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
        "structuredDataErrorCount" => left
            .structured_data_error_count
            .cmp(&right.structured_data_error_count),
        "structuredDataWarningCount" => left
            .structured_data_warning_count
            .cmp(&right.structured_data_warning_count),
        "openGraphCount" => left.open_graph_count.cmp(&right.open_graph_count),
        "twitterCardCount" => left.twitter_card_count.cmp(&right.twitter_card_count),
        "deprecatedHtmlTagCount" => left
            .deprecated_html_tag_count
            .cmp(&right.deprecated_html_tag_count),
        "duplicateIdCount" => left.duplicate_id_count.cmp(&right.duplicate_id_count),
        "jsRendered" => left.js_rendered.cmp(&right.js_rendered),
        "renderedDomChanged" => left.rendered_dom_changed.cmp(&right.rendered_dom_changed),
        "renderedWordCountDelta" => left
            .rendered_word_count_delta
            .cmp(&right.rendered_word_count_delta),
        "renderedLinkCountDelta" => left
            .rendered_link_count_delta
            .cmp(&right.rendered_link_count_delta),
        "searchConsoleClicks" => {
            compare_optional_f64(left.search_console_clicks, right.search_console_clicks)
        }
        "searchConsoleImpressions" => compare_optional_f64(
            left.search_console_impressions,
            right.search_console_impressions,
        ),
        "searchConsoleCtr" => {
            compare_optional_f64(left.search_console_ctr, right.search_console_ctr)
        }
        "searchConsoleAveragePosition" => compare_optional_f64(
            left.search_console_average_position,
            right.search_console_average_position,
        ),
        "nearDuplicateClusterId" => left
            .near_duplicate_cluster_id
            .cmp(&right.near_duplicate_cluster_id),
        "inlinkCount" => left.inlink_count.cmp(&right.inlink_count),
        "firstInlinkSourceUrl" => left
            .first_inlink_source_url
            .cmp(&right.first_inlink_source_url),
        "firstInlinkSourcePosition" => left
            .first_inlink_source_position
            .cmp(&right.first_inlink_source_position),
        "outlinkCount" => left.outlink_count.cmp(&right.outlink_count),
        "title" => left.title.cmp(&right.title),
        "url" => left.url.cmp(&right.url),
        "finalUrl" => left.final_url.cmp(&right.final_url),
        _ => left.id.cmp(&right.id),
    }
}

fn compare_optional_f64(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

fn custom_extraction_sort_value(record: &CrawlRecord, name: &str) -> String {
    record
        .custom_extractions
        .iter()
        .find(|extraction| extraction.name == name)
        .map(|extraction| extraction.values.join(", ").to_lowercase())
        .unwrap_or_default()
}

fn custom_search_sort_value(record: &CrawlRecord, name: &str) -> usize {
    record
        .custom_searches
        .iter()
        .filter(|custom_search| custom_search.name == name)
        .map(|custom_search| custom_search.match_count)
        .sum::<usize>()
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

fn annotate_memory_image_assets(inner: &MemoryStoreInner) -> Vec<ImageAsset> {
    let size_by_alias = image_size_by_alias(&inner.records);
    inner
        .image_assets
        .iter()
        .cloned()
        .map(|mut image| {
            if let Some(size_bytes) = url_aliases(&image.image_url)
                .iter()
                .find_map(|alias| size_by_alias.get(alias).copied())
            {
                image.size_bytes = Some(size_bytes);
                image.oversized = size_bytes > IMAGE_ASSET_OVERSIZE_BYTES;
            }
            image
        })
        .collect()
}

fn image_size_by_alias(records: &[CrawlRecord]) -> HashMap<String, u64> {
    let mut sizes = HashMap::new();
    for record in records {
        let is_image = record
            .content_type
            .as_deref()
            .map(|value| value.to_ascii_lowercase().starts_with("image/"))
            .unwrap_or(false);
        if !is_image {
            continue;
        }
        for alias in record_url_aliases(record) {
            sizes.insert(alias, record.size_bytes as u64);
        }
    }
    sizes
}

fn filter_image_assets(images: &mut Vec<ImageAsset>, query: &ImageAssetQuery) {
    if let Some(page_url) = query
        .page_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let page_aliases = url_aliases(page_url);
        images.retain(|image| {
            let image_page_aliases = url_aliases(&image.page_url);
            page_aliases
                .iter()
                .any(|alias| image_page_aliases.contains(alias))
        });
    }
    if query.oversized_only {
        images.retain(|image| image.oversized);
    }
    if query.missing_alt_only {
        images.retain(|image| image.missing_alt);
    }
    if let Some(search) = query
        .global_search
        .as_ref()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
    {
        images.retain(|image| image_asset_matches_search(image, &search));
    }
}

fn image_asset_matches_search(image: &ImageAsset, search: &str) -> bool {
    image.page_url.to_lowercase().contains(search)
        || image.image_url.to_lowercase().contains(search)
        || image
            .alt_text
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(search)
}

fn sort_image_assets(images: &mut [ImageAsset], sort_by: &str, sort_dir: &SortDirection) {
    images.sort_by(|left, right| {
        let ordering = compare_image_assets(left, right, sort_by)
            .then_with(|| left.page_url.cmp(&right.page_url))
            .then_with(|| left.source_position.cmp(&right.source_position));
        match sort_dir {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
}

fn compare_image_assets(left: &ImageAsset, right: &ImageAsset, sort_by: &str) -> Ordering {
    match sort_by {
        "pageUrl" => left.page_url.cmp(&right.page_url),
        "imageUrl" => left.image_url.cmp(&right.image_url),
        "altText" => left.alt_text.cmp(&right.alt_text),
        "altLen" => left.alt_len.cmp(&right.alt_len),
        "missingAlt" => left.missing_alt.cmp(&right.missing_alt),
        "altTooLong" => left.alt_too_long.cmp(&right.alt_too_long),
        "width" => left.width.cmp(&right.width),
        "height" => left.height.cmp(&right.height),
        "sourcePosition" => left.source_position.cmp(&right.source_position),
        "sizeBytes" => left.size_bytes.cmp(&right.size_bytes),
        "oversized" => left.oversized.cmp(&right.oversized),
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
    fn memory_on_page_audits_require_successful_html() {
        assert_on_page_audits_require_successful_html(MemoryStore::new());
    }

    #[test]
    fn sqlite_on_page_audits_require_successful_html() {
        assert_on_page_audits_require_successful_html(SqliteStore::in_memory().unwrap());
    }

    fn assert_on_page_audits_require_successful_html(store: impl CrawlStore) {
        let empty = hreflang_record("https://example.com/empty", Vec::new(), None);
        store.upsert(empty.clone());
        for (path, status, content_type) in [
            ("image.png", Some(200), Some("image/png")),
            ("missing", Some(404), Some("text/html")),
            ("failed", Some(500), Some("text/html")),
            ("redirect", Some(301), Some("text/html")),
            ("pending", None, None),
        ] {
            let mut record = empty.clone();
            record.url = format!("https://example.com/{path}");
            record.final_url = record.url.clone();
            record.storage_key = record.url.clone();
            record.status_code = status;
            record.content_type = content_type.map(str::to_string);
            store.upsert(record);
        }

        let mut page = hreflang_record(
            "https://example.com/page",
            Vec::new(),
            Some("https://example.com/page"),
        );
        page.title = Some("A".repeat(80));
        page.title_len = 80;
        page.title_pixel_width = 640;
        page.meta_description = Some("B".repeat(180));
        page.meta_description_len = 180;
        page.meta_description_pixel_width = 980;
        page.h1 = page.title.clone();
        page.h1_len = 80;
        page.h2 = Some("C".repeat(80));
        page.h2_len = 80;
        page.canonical_count = 2;
        page.images_missing_alt = 1;
        page.images_alt_too_long = 1;
        page.mixed_content_count = 1;
        page.insecure_form_count = 1;
        page.hreflang_invalid_count = 1;
        page.hreflang_missing_self_reference = true;
        page.structured_data_error_count = 1;
        page.structured_data_warning_count = 1;
        page.deprecated_html_tag_count = 1;
        page.duplicate_id_count = 1;
        page.rendered_dom_changed = true;
        page.near_duplicate_cluster_id = Some(7);
        store.upsert(page.clone());
        for (path, status, content_type) in [
            ("error-copy", Some(404), "text/html"),
            ("image-copy", Some(200), "image/png"),
        ] {
            let mut excluded = page.clone();
            excluded.url = format!("https://example.com/{path}");
            excluded.final_url = excluded.url.clone();
            excluded.storage_key = excluded.url.clone();
            excluded.status_code = status;
            excluded.content_type = Some(content_type.to_string());
            store.upsert(excluded);
        }

        for view in [
            IssueView::TitleMissing,
            IssueView::MetaMissing,
            IssueView::H1Missing,
            IssueView::H2Missing,
            IssueView::CanonicalMissing,
            IssueView::TitleTooLong,
            IssueView::TitlePixelTooWide,
            IssueView::MetaTooLong,
            IssueView::MetaPixelTooWide,
            IssueView::H1TooLong,
            IssueView::H2TooLong,
            IssueView::TitleSameAsH1,
            IssueView::CanonicalMultiple,
            IssueView::ImagesMissingAlt,
            IssueView::ImagesAltTooLong,
            IssueView::SecurityMixedContent,
            IssueView::SecurityInsecureForms,
            IssueView::HreflangInvalid,
            IssueView::HreflangMissingSelfReference,
            IssueView::StructuredDataInvalid,
            IssueView::StructuredDataWarning,
            IssueView::HtmlDeprecatedTags,
            IssueView::HtmlDuplicateIds,
            IssueView::RenderedDomChanged,
        ] {
            let response = store.query(GridQuery {
                view: view.clone(),
                ..GridQuery::default()
            });
            assert_eq!(response.total, 1, "{view:?}");
            assert!(
                matches!(
                    response.rows[0].final_url.as_str(),
                    "https://example.com/empty" | "https://example.com/page"
                ),
                "{view:?}"
            );
        }

        for view in [
            IssueView::TitleDuplicate,
            IssueView::MetaDuplicate,
            IssueView::H1Duplicate,
            IssueView::H2Duplicate,
            IssueView::NearDuplicate,
        ] {
            assert_eq!(
                store
                    .query(GridQuery {
                        view: view.clone(),
                        ..GridQuery::default()
                    })
                    .total,
                0,
                "{view:?}"
            );
        }
        let summary = store.summary();
        assert_eq!(
            (
                summary.title_missing,
                summary.meta_missing,
                summary.h1_missing,
                summary.h2_missing,
                summary.canonical_missing
            ),
            (1, 1, 1, 1, 1)
        );
        assert_eq!(
            (
                summary.title_duplicate,
                summary.meta_duplicate,
                summary.h1_duplicate,
                summary.h2_duplicate,
                summary.near_duplicates
            ),
            (0, 0, 0, 0, 0)
        );
        assert_eq!(
            (
                summary.canonical_multiple,
                summary.images_missing_alt,
                summary.mixed_content,
                summary.structured_data_invalid,
                summary.deprecated_html_tags,
                summary.duplicate_ids,
                summary.rendered_dom_changed
            ),
            (1, 1, 1, 1, 1, 1, 1)
        );

        page.url = "https://example.com/duplicate".to_string();
        page.final_url = page.url.clone();
        page.storage_key = page.url.clone();
        store.upsert(page);
        for view in [
            IssueView::TitleDuplicate,
            IssueView::MetaDuplicate,
            IssueView::H1Duplicate,
            IssueView::H2Duplicate,
            IssueView::NearDuplicate,
        ] {
            assert_eq!(
                store
                    .query(GridQuery {
                        view: view.clone(),
                        ..GridQuery::default()
                    })
                    .total,
                2,
                "{view:?}"
            );
        }
        let summary = store.summary();
        assert_eq!(
            (
                summary.title_duplicate,
                summary.meta_duplicate,
                summary.h1_duplicate,
                summary.h2_duplicate,
                summary.near_duplicates
            ),
            (2, 2, 2, 2, 2)
        );
    }

    #[test]
    fn memory_broken_reports_require_known_fetch_failures() {
        assert_broken_reports_require_known_fetch_failures(MemoryStore::new());
    }

    #[test]
    fn sqlite_broken_reports_require_known_fetch_failures() {
        assert_broken_reports_require_known_fetch_failures(SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn memory_sorts_by_original_url() {
        assert_sorts_by_original_url(MemoryStore::new());
    }

    #[test]
    fn sqlite_sorts_by_original_url() {
        assert_sorts_by_original_url(SqliteStore::in_memory().unwrap());
    }

    fn assert_sorts_by_original_url(store: impl CrawlStore) {
        for (original, destination) in [("z", "a"), ("a", "z")] {
            let mut record = CrawlRecord::pending(format!("https://example.com/{original}"), 0);
            record.final_url = format!("https://example.com/{destination}");
            store.upsert(record);
        }
        for (direction, expected) in [(SortDirection::Asc, "a"), (SortDirection::Desc, "z")] {
            let rows = store.query(GridQuery {
                sort_by: Some("url".to_string()),
                sort_dir: direction,
                ..GridQuery::default()
            });
            assert_eq!(rows.rows[0].url, format!("https://example.com/{expected}"));
        }
    }

    fn assert_broken_reports_require_known_fetch_failures(store: impl CrawlStore) {
        for (path, status, error) in [
            ("blocked", None, Some("Blocked by robots.txt")),
            ("pending", None, None),
            ("timeout", None, Some("Connection timed out")),
            ("missing", Some(404), None),
            (
                "redirect",
                Some(302),
                Some("Redirect response missing Location header"),
            ),
            (
                "rendering",
                Some(200),
                Some("JavaScript rendering failed: timeout"),
            ),
        ] {
            let url = format!("https://example.com/{path}");
            store.add_link_edge(test_edge(
                "https://example.com/source",
                &url,
                LinkType::Internal,
            ));
            let mut record = CrawlRecord::pending(url.clone(), 1);
            record.status_code = status;
            record.error = error.map(str::to_string);
            if path == "blocked" {
                record.status_text = "Blocked by robots.txt".to_string();
                record.in_sitemap = true;
            }
            store.upsert(record);
        }
        store.add_link_edge(test_edge(
            "https://example.com/source",
            "https://example.com/uncrawled",
            LinkType::Internal,
        ));

        let summary = store.summary();
        assert_eq!(summary.no_response, 1);
        assert_eq!(summary.broken, 3);
        let no_response = store.query(GridQuery {
            view: IssueView::NoResponse,
            ..GridQuery::default()
        });
        assert_eq!(no_response.total, 1);
        assert_eq!(no_response.rows[0].final_url, "https://example.com/timeout");
        let broken = store.query(GridQuery {
            view: IssueView::BrokenLinks,
            ..GridQuery::default()
        });
        assert_eq!(broken.total, 3);
        assert_eq!(broken.summary.broken, 3);
        let edges = store.link_edges(LinkEdgeQuery {
            view: LinkEdgeView::Broken,
            ..LinkEdgeQuery::default()
        });
        assert_eq!(edges.total, 3);
        assert!(edges.edges.iter().all(|edge| {
            [
                "https://example.com/timeout",
                "https://example.com/missing",
                "https://example.com/redirect",
            ]
            .contains(&edge.target_url.as_str())
        }));
        assert_eq!(
            store
                .anchor_texts(LinkEdgeQuery {
                    view: LinkEdgeView::Broken,
                    ..LinkEdgeQuery::default()
                })
                .total,
            3
        );

        let graph = store.crawl_graph(CrawlGraphQuery::default());
        for (path, crawled) in [
            ("blocked", false),
            ("pending", false),
            ("uncrawled", false),
            ("timeout", true),
            ("missing", true),
        ] {
            let url = format!("https://example.com/{path}");
            assert_eq!(
                graph
                    .nodes
                    .iter()
                    .find(|node| node.url == url)
                    .unwrap()
                    .crawled,
                crawled,
                "{path}"
            );
        }
        let sitemap = store.sitemap_validation(SitemapValidationQuery::default());
        assert_eq!(sitemap.rows.len(), 1);
        assert_eq!(sitemap.rows[0].severity, Severity::Warning);
        assert!(
            sitemap.rows[0]
                .issues
                .iter()
                .any(|issue| issue == "Robots-blocked URL in sitemap")
        );

        let mut recovered = CrawlRecord::pending("https://example.com/timeout".to_string(), 1);
        recovered.status_code = Some(200);
        store.upsert(recovered);
        assert_eq!(store.summary().no_response, 0);
        assert_eq!(
            store
                .link_edges(LinkEdgeQuery {
                    view: LinkEdgeView::Broken,
                    ..LinkEdgeQuery::default()
                })
                .total,
            2
        );
    }

    #[test]
    fn query_filters_duplicate_titles() {
        let store = MemoryStore::new();
        for url in ["https://example.com/a", "https://example.com/b"] {
            let mut record = CrawlRecord::pending(url.to_string(), 0);
            record.status_code = Some(200);
            record.content_type = Some("text/html".to_string());
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
    fn query_filters_pixel_width_issues() {
        let store = MemoryStore::new();
        let mut narrow = CrawlRecord::pending("https://example.com/narrow".to_string(), 0);
        narrow.status_code = Some(200);
        narrow.content_type = Some("text/html".to_string());
        narrow.title = Some("Short".to_string());
        narrow.title_pixel_width = 120;
        narrow.meta_description = Some("Brief".to_string());
        narrow.meta_description_pixel_width = 220;
        store.upsert(narrow);

        let mut wide = CrawlRecord::pending("https://example.com/wide".to_string(), 0);
        wide.status_code = Some(200);
        wide.content_type = Some("text/html".to_string());
        wide.title = Some("A very wide title fixture".to_string());
        wide.title_pixel_width = 640;
        wide.meta_description = Some("A very wide meta description fixture".to_string());
        wide.meta_description_pixel_width = 980;
        store.upsert(wide);

        assert_eq!(
            store
                .query(GridQuery {
                    view: IssueView::TitlePixelTooNarrow,
                    ..GridQuery::default()
                })
                .total,
            1
        );
        assert_eq!(
            store
                .query(GridQuery {
                    view: IssueView::TitlePixelTooWide,
                    ..GridQuery::default()
                })
                .total,
            1
        );
        assert_eq!(
            store
                .query(GridQuery {
                    view: IssueView::MetaPixelTooNarrow,
                    ..GridQuery::default()
                })
                .total,
            1
        );
        assert_eq!(
            store
                .query(GridQuery {
                    view: IssueView::MetaPixelTooWide,
                    ..GridQuery::default()
                })
                .total,
            1
        );
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
    fn memory_hreflang_flags_missing_return_links() {
        let store = MemoryStore::new();
        store.upsert(hreflang_record(
            "https://example.com/en",
            vec![
                hreflang_link("en", "https://example.com/en"),
                hreflang_link("fr", "https://example.com/fr"),
            ],
            None,
        ));
        store.upsert(hreflang_record(
            "https://example.com/fr",
            vec![hreflang_link("fr", "https://example.com/fr")],
            None,
        ));

        let response = store.query(GridQuery {
            view: IssueView::HreflangMissingReturnLink,
            ..GridQuery::default()
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].final_url, "https://example.com/en");
    }

    #[test]
    fn memory_hreflang_flags_non_canonical_targets() {
        let store = MemoryStore::new();
        store.upsert(hreflang_record(
            "https://example.com/en",
            vec![hreflang_link("fr", "https://example.com/fr")],
            None,
        ));
        store.upsert(hreflang_record(
            "https://example.com/fr",
            Vec::new(),
            Some("https://example.com/preferred-fr"),
        ));

        let response = store.query(GridQuery {
            view: IssueView::HreflangNonCanonicalTarget,
            ..GridQuery::default()
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].final_url, "https://example.com/en");
    }

    #[test]
    fn memory_structured_data_filters_semantic_errors_and_warnings() {
        let store = MemoryStore::new();
        let mut record = CrawlRecord::pending("https://example.com/article".to_string(), 0);
        record.status_code = Some(200);
        record.content_type = Some("text/html".to_string());
        record.structured_data_error_count = 2;
        record.structured_data_warning_count = 1;
        record.structured_data_issues = vec![StructuredDataIssue {
            severity: "error".to_string(),
            message: "Article is missing required field author".to_string(),
            path: "script[1].author".to_string(),
        }];
        store.upsert(record);

        let errors = store.query(GridQuery {
            view: IssueView::StructuredDataInvalid,
            ..GridQuery::default()
        });
        let warnings = store.query(GridQuery {
            view: IssueView::StructuredDataWarning,
            ..GridQuery::default()
        });

        assert_eq!(errors.total, 1);
        assert_eq!(warnings.total, 1);
        assert_eq!(errors.rows[0].structured_data_issues.len(), 1);
    }

    #[test]
    fn memory_custom_extractions_support_search_and_sort() {
        let store = MemoryStore::new();
        store.upsert(custom_extraction_record(
            "https://example.com/z",
            "heading",
            "Zebra",
        ));
        store.upsert(custom_extraction_record(
            "https://example.com/a",
            "heading",
            "Alpha",
        ));

        let sorted = store.query(GridQuery {
            sort_by: Some("custom:heading:0".to_string()),
            ..GridQuery::default()
        });
        let filtered = store.query(GridQuery {
            global_search: Some("zebra".to_string()),
            ..GridQuery::default()
        });

        assert_eq!(sorted.total, 2);
        assert_eq!(sorted.rows[0].final_url, "https://example.com/a");
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.rows[0].final_url, "https://example.com/z");
    }

    #[test]
    fn memory_custom_searches_support_search_and_sort() {
        assert_custom_searches_support_search_and_sort(MemoryStore::new());
    }

    #[test]
    fn memory_search_console_metrics_merge_into_records() {
        assert_search_console_metrics_merge_into_records(MemoryStore::new());
    }

    #[test]
    fn memory_segments_filter_rows() {
        assert_segments_filter_rows(MemoryStore::new());
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

        let rows = store.query(GridQuery {
            view: IssueView::BrokenLinks,
            ..GridQuery::default()
        });
        assert_eq!(
            rows.rows[0].first_inlink_source_url.as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(rows.rows[0].first_inlink_source_position, Some(1));
    }

    #[test]
    fn memory_crawl_path_finds_internal_chain() {
        assert_crawl_path_finds_internal_chain(MemoryStore::new());
    }

    #[test]
    fn memory_sitemap_validation_flags_problem_urls() {
        let store = MemoryStore::new();
        let mut ok = CrawlRecord::pending("https://example.com/ok".to_string(), 0);
        ok.in_sitemap = true;
        ok.status_code = Some(200);
        ok.status_text = "OK".to_string();
        ok.indexability = "Indexable".to_string();
        ok.indexability_status = "Indexable".to_string();
        ok.inlink_count = 1;
        store.upsert(ok);

        let mut broken = CrawlRecord::pending("https://example.com/missing".to_string(), 0);
        broken.in_sitemap = true;
        broken.status_code = Some(404);
        broken.status_text = "Not Found".to_string();
        broken.indexability = "Non-indexable".to_string();
        broken.indexability_status = "HTTP 404".to_string();
        store.upsert(broken);

        let response = store.sitemap_validation(SitemapValidationQuery::default());
        assert_eq!(response.total, 2);
        let broken_row = response
            .rows
            .iter()
            .find(|row| row.final_url.ends_with("/missing"))
            .unwrap();
        assert_eq!(broken_row.severity, Severity::Error);
        assert!(
            broken_row
                .issues
                .iter()
                .any(|issue| issue == "4xx URL in sitemap")
        );
        assert!(
            broken_row
                .issues
                .iter()
                .any(|issue| issue == "Orphan URL in sitemap")
        );
    }

    #[test]
    fn memory_first_source_matches_original_url_when_final_url_changes() {
        let store = MemoryStore::new();
        let mut edge = test_edge(
            "https://example.com/source",
            "https://example.com/original",
            LinkType::Internal,
        );
        edge.anchor_text = "Moved target".to_string();
        edge.source_position = 9;
        store.add_link_edge(edge);

        let mut target = CrawlRecord::pending("https://example.com/original".to_string(), 1);
        target.final_url = "https://example.com/final".to_string();
        target.status_code = Some(500);
        store.upsert(target);

        let rows = store.query(GridQuery {
            view: IssueView::BrokenLinks,
            ..GridQuery::default()
        });
        assert_eq!(
            rows.rows[0].first_inlink_source_url.as_deref(),
            Some("https://example.com/source")
        );
        assert_eq!(
            rows.rows[0].first_inlink_anchor_text.as_deref(),
            Some("Moved target")
        );
        assert_eq!(rows.rows[0].first_inlink_source_position, Some(9));
    }

    #[test]
    fn memory_first_source_matches_url_aliases() {
        let store = MemoryStore::new();
        let mut edge = test_edge(
            "https://example.com/source",
            "https://example.com",
            LinkType::Internal,
        );
        edge.anchor_text = "Root without slash".to_string();
        edge.source_position = 4;
        store.add_link_edge(edge);
        store.add_inlink("https://example.com");

        let mut target = CrawlRecord::pending("https://example.com/".to_string(), 1);
        target.status_code = Some(404);
        store.upsert(target);

        let rows = store.query(GridQuery {
            view: IssueView::BrokenLinks,
            ..GridQuery::default()
        });
        assert_eq!(
            rows.rows[0].first_inlink_source_url.as_deref(),
            Some("https://example.com/source")
        );
        assert_eq!(
            rows.rows[0].first_inlink_anchor_text.as_deref(),
            Some("Root without slash")
        );
        assert_eq!(rows.rows[0].first_inlink_source_position, Some(4));
        assert_eq!(rows.rows[0].inlink_count, 1);
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
    fn memory_store_tracks_image_assets() {
        assert_image_asset_store(MemoryStore::new());
    }

    #[test]
    fn memory_store_filters_html_validation_issues() {
        assert_html_validation_issue_views(MemoryStore::new());
    }

    #[test]
    fn memory_store_filters_rendered_dom_changes() {
        assert_rendered_dom_change_view(MemoryStore::new());
    }

    #[test]
    fn memory_store_round_trips_frontier_state() {
        assert_frontier_state_store(MemoryStore::new());
    }

    #[test]
    fn query_filters_near_duplicate_clusters() {
        let store = MemoryStore::new();
        for url in ["https://example.com/a", "https://example.com/b"] {
            let mut record = CrawlRecord::pending(url.to_string(), 0);
            record.status_code = Some(200);
            record.content_type = Some("text/html".to_string());
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
    fn memory_store_preserves_duplicate_list_urls() {
        let store = MemoryStore::new();
        for position in [1_u32, 2] {
            let mut record = CrawlRecord::pending("https://example.com/a".to_string(), 0);
            record.storage_key = format!("list:{position}:https://example.com/a");
            record.list_position = Some(position);
            record.list_duplicate_index = position;
            record.status_code = Some(200);
            store.upsert(record);
        }

        let response = store.query(GridQuery::default());

        assert_eq!(response.total, 2);
        assert_eq!(response.rows[0].list_position, Some(1));
        assert_eq!(response.rows[1].list_position, Some(2));
        assert_eq!(response.rows[1].list_duplicate_index, 2);
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
    fn sqlite_store_preserves_duplicate_list_urls() {
        let store = SqliteStore::in_memory().unwrap();
        for position in [1_u32, 2] {
            let mut record = CrawlRecord::pending("https://example.com/a".to_string(), 0);
            record.storage_key = format!("list:{position}:https://example.com/a");
            record.list_position = Some(position);
            record.list_duplicate_index = position;
            record.status_code = Some(200);
            store.upsert(record);
        }

        let response = store.query(GridQuery::default());

        assert_eq!(response.total, 2);
        assert_eq!(response.rows[0].list_position, Some(1));
        assert_eq!(response.rows[1].list_position, Some(2));
        assert_eq!(response.rows[1].list_duplicate_index, 2);
    }

    #[test]
    fn sqlite_store_migrates_final_url_unique_schema_for_duplicate_list_urls() {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "ferrous_frog_storage_migration_{}_{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        path.push(unique);

        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "
                CREATE TABLE crawl_records (
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
                    tcp_connect_time_ms INTEGER,
                    tls_handshake_time_ms INTEGER,
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
                    title_pixel_width INTEGER NOT NULL DEFAULT 0,
                    meta_description TEXT,
                    meta_description_len INTEGER NOT NULL,
                    meta_description_pixel_width INTEGER NOT NULL DEFAULT 0,
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
                INSERT INTO crawl_records (
                    url,
                    final_url,
                    classification,
                    status_code,
                    status_text,
                    indexability,
                    indexability_status,
                    response_time_ms,
                    size_bytes,
                    depth,
                    redirect_chain,
                    title_len,
                    meta_description_len,
                    h1_len,
                    inlink_count,
                    outlink_count,
                    internal_outlink_count,
                    external_outlink_count
                ) VALUES (
                    'https://example.com/a',
                    'https://example.com/a',
                    'internal',
                    200,
                    'OK',
                    'Indexable',
                    'Indexable',
                    10,
                    100,
                    0,
                    '[]',
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0
                );
                ",
            )
            .unwrap();
        }

        let store = SqliteStore::open(&path).unwrap();
        for position in [1_u32, 2] {
            let mut record = CrawlRecord::pending("https://example.com/a".to_string(), 0);
            record.storage_key = format!("list:{position}:https://example.com/a");
            record.list_position = Some(position);
            record.list_duplicate_index = position;
            record.status_code = Some(200);
            store.upsert(record);
        }

        let records = store.records();
        let matching_count = records
            .iter()
            .filter(|record| record.final_url == "https://example.com/a")
            .count();
        assert_eq!(matching_count, 3);
        assert!(
            records
                .iter()
                .any(|record| record.storage_key == "https://example.com/a")
        );
        assert!(
            records
                .iter()
                .any(|record| record.storage_key == "list:2:https://example.com/a")
        );

        drop(store);
        let _ = std::fs::remove_file(path);
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
    fn sqlite_hreflang_flags_missing_return_links() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert(hreflang_record(
            "https://example.com/en",
            vec![
                hreflang_link("en", "https://example.com/en"),
                hreflang_link("fr", "https://example.com/fr"),
            ],
            None,
        ));
        store.upsert(hreflang_record(
            "https://example.com/fr",
            vec![hreflang_link("fr", "https://example.com/fr")],
            None,
        ));

        let response = store.query(GridQuery {
            view: IssueView::HreflangMissingReturnLink,
            ..GridQuery::default()
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].final_url, "https://example.com/en");
    }

    #[test]
    fn sqlite_hreflang_flags_non_canonical_targets() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert(hreflang_record(
            "https://example.com/en",
            vec![hreflang_link("fr", "https://example.com/fr")],
            None,
        ));
        store.upsert(hreflang_record(
            "https://example.com/fr",
            Vec::new(),
            Some("https://example.com/preferred-fr"),
        ));

        let response = store.query(GridQuery {
            view: IssueView::HreflangNonCanonicalTarget,
            ..GridQuery::default()
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].final_url, "https://example.com/en");
    }

    #[test]
    fn sqlite_structured_data_filters_semantic_errors_and_warnings() {
        let store = SqliteStore::in_memory().unwrap();
        let mut record = CrawlRecord::pending("https://example.com/article".to_string(), 0);
        record.status_code = Some(200);
        record.content_type = Some("text/html".to_string());
        record.structured_data_error_count = 2;
        record.structured_data_warning_count = 1;
        record.structured_data_issues = vec![StructuredDataIssue {
            severity: "error".to_string(),
            message: "Article is missing required field author".to_string(),
            path: "script[1].author".to_string(),
        }];
        store.upsert(record);

        let errors = store.query(GridQuery {
            view: IssueView::StructuredDataInvalid,
            ..GridQuery::default()
        });
        let warnings = store.query(GridQuery {
            view: IssueView::StructuredDataWarning,
            ..GridQuery::default()
        });

        assert_eq!(errors.total, 1);
        assert_eq!(warnings.total, 1);
        assert_eq!(errors.rows[0].structured_data_issues.len(), 1);
    }

    #[test]
    fn sqlite_custom_extractions_support_search_and_sort() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert(custom_extraction_record(
            "https://example.com/z",
            "heading",
            "Zebra",
        ));
        store.upsert(custom_extraction_record(
            "https://example.com/a",
            "heading",
            "Alpha",
        ));

        let sorted = store.query(GridQuery {
            sort_by: Some("custom:heading:0".to_string()),
            ..GridQuery::default()
        });
        let filtered = store.query(GridQuery {
            global_search: Some("zebra".to_string()),
            ..GridQuery::default()
        });

        assert_eq!(sorted.total, 2);
        assert_eq!(sorted.rows[0].final_url, "https://example.com/a");
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.rows[0].final_url, "https://example.com/z");
    }

    #[test]
    fn sqlite_custom_searches_support_search_and_sort() {
        assert_custom_searches_support_search_and_sort(SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn sqlite_search_console_metrics_merge_into_records() {
        assert_search_console_metrics_merge_into_records(SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn sqlite_segments_filter_rows() {
        assert_segments_filter_rows(SqliteStore::in_memory().unwrap());
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

        let rows = store.query(GridQuery {
            view: IssueView::BrokenLinks,
            ..GridQuery::default()
        });
        assert_eq!(
            rows.rows[0].first_inlink_source_url.as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(rows.rows[0].first_inlink_source_position, Some(1));
    }

    #[test]
    fn sqlite_crawl_path_finds_internal_chain() {
        assert_crawl_path_finds_internal_chain(SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn sqlite_sitemap_validation_flags_problem_urls() {
        let store = SqliteStore::in_memory().unwrap();
        let mut canonicalized =
            CrawlRecord::pending("https://example.com/canonicalized".to_string(), 0);
        canonicalized.in_sitemap = true;
        canonicalized.status_code = Some(200);
        canonicalized.status_text = "OK".to_string();
        canonicalized.indexability = "Non-indexable".to_string();
        canonicalized.indexability_status = "Canonicalized".to_string();
        canonicalized.canonical = Some("https://example.com/preferred".to_string());
        store.upsert(canonicalized);

        let response = store.sitemap_validation(SitemapValidationQuery {
            global_search: Some("canonical".to_string()),
            ..SitemapValidationQuery::default()
        });
        assert_eq!(response.total, 1);
        assert_eq!(response.rows[0].severity, Severity::Warning);
        assert!(
            response.rows[0]
                .issues
                .iter()
                .any(|issue| issue == "Canonical points to a different URL")
        );
    }

    #[test]
    fn sqlite_store_tracks_image_assets() {
        assert_image_asset_store(SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn sqlite_store_filters_html_validation_issues() {
        assert_html_validation_issue_views(SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn sqlite_store_filters_rendered_dom_changes() {
        assert_rendered_dom_change_view(SqliteStore::in_memory().unwrap());
    }

    #[test]
    #[ignore = "runs a configurable large SQLite synthetic URL benchmark"]
    fn sqlite_large_synthetic_storage_benchmark() {
        let url_count = std::env::var("BENCH_URLS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1_000_000);
        let path = std::env::temp_dir().join(format!(
            "ferrous-frog-synthetic-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteStore::open(&path).unwrap();
        let started = std::time::Instant::now();

        for index in 0..url_count {
            let mut record = CrawlRecord::pending(
                format!("https://synthetic.example.com/page/{index:08}"),
                (index % 12) + 1,
            );
            record.status_code = Some(if index % 97 == 0 { 404 } else { 200 });
            record.status_text = if record.status_code == Some(404) {
                "Not Found".to_string()
            } else {
                "OK".to_string()
            };
            record.content_type = Some("text/html; charset=utf-8".to_string());
            record.title = Some(format!("Synthetic page {index}"));
            record.title_len = record.title.as_deref().unwrap_or_default().len();
            record.meta_description =
                Some(format!("Synthetic benchmark description for page {index}."));
            record.meta_description_len =
                record.meta_description.as_deref().unwrap_or_default().len();
            record.h1 = Some(format!("Synthetic page {index}"));
            record.h1_len = record.h1.as_deref().unwrap_or_default().len();
            record.outlink_count = 12;
            record.internal_outlink_count = 11;
            record.external_outlink_count = 1;
            record.inlink_count = if index == 0 { 0 } else { 1 };
            record.size_bytes = 24_000 + (index % 4096);
            store.upsert(record);
        }

        let elapsed = started.elapsed();
        let summary = store.summary();
        let tail = store.query(GridQuery {
            offset: url_count.saturating_sub(10),
            limit: 10,
            sort_by: Some("finalUrl".to_string()),
            ..GridQuery::default()
        });

        assert_eq!(summary.total, url_count);
        assert!(!tail.rows.is_empty() || url_count == 0);
        eprintln!(
            "Inserted and queried {url_count} synthetic URLs in {:.2?} ({:.2} URLs/sec)",
            elapsed,
            url_count as f64 / elapsed.as_secs_f64().max(0.001)
        );

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sqlite_store_round_trips_frontier_state() {
        assert_frontier_state_store(SqliteStore::in_memory().unwrap());
    }

    #[test]
    fn sqlite_first_source_matches_original_url_when_final_url_changes() {
        let store = SqliteStore::in_memory().unwrap();
        let mut edge = test_edge(
            "https://example.com/source",
            "https://example.com/original",
            LinkType::Internal,
        );
        edge.anchor_text = "Moved target".to_string();
        edge.source_position = 9;
        store.add_link_edge(edge);

        let mut target = CrawlRecord::pending("https://example.com/original".to_string(), 1);
        target.final_url = "https://example.com/final".to_string();
        target.status_code = Some(500);
        store.upsert(target);

        let rows = store.query(GridQuery {
            view: IssueView::BrokenLinks,
            ..GridQuery::default()
        });
        assert_eq!(
            rows.rows[0].first_inlink_source_url.as_deref(),
            Some("https://example.com/source")
        );
        assert_eq!(
            rows.rows[0].first_inlink_anchor_text.as_deref(),
            Some("Moved target")
        );
        assert_eq!(rows.rows[0].first_inlink_source_position, Some(9));
    }

    #[test]
    fn sqlite_first_source_matches_url_aliases() {
        let store = SqliteStore::in_memory().unwrap();
        let mut edge = test_edge(
            "https://example.com/source",
            "https://example.com",
            LinkType::Internal,
        );
        edge.anchor_text = "Root without slash".to_string();
        edge.source_position = 4;
        store.add_link_edge(edge);
        store.add_inlink("https://example.com");

        let mut target = CrawlRecord::pending("https://example.com/".to_string(), 1);
        target.status_code = Some(404);
        store.upsert(target);

        let rows = store.query(GridQuery {
            view: IssueView::BrokenLinks,
            ..GridQuery::default()
        });
        assert_eq!(
            rows.rows[0].first_inlink_source_url.as_deref(),
            Some("https://example.com/source")
        );
        assert_eq!(
            rows.rows[0].first_inlink_anchor_text.as_deref(),
            Some("Root without slash")
        );
        assert_eq!(rows.rows[0].first_inlink_source_position, Some(4));
        assert_eq!(rows.rows[0].inlink_count, 1);
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

    fn hreflang_record(
        url: &str,
        hreflang_links: Vec<HreflangLink>,
        canonical: Option<&str>,
    ) -> CrawlRecord {
        let mut record = CrawlRecord::pending(url.to_string(), 0);
        record.status_code = Some(200);
        record.status_text = "OK".to_string();
        record.content_type = Some("text/html".to_string());
        record.indexability = "Indexable".to_string();
        record.indexability_status = "Indexable".to_string();
        record.hreflang_count = hreflang_links.len() as u32;
        record.hreflang_links = hreflang_links;
        record.canonical = canonical.map(str::to_string);
        record
    }

    fn hreflang_link(hreflang: &str, url: &str) -> HreflangLink {
        HreflangLink {
            hreflang: hreflang.to_string(),
            url: url.to_string(),
            valid: true,
        }
    }

    fn custom_extraction_record(url: &str, name: &str, value: &str) -> CrawlRecord {
        let mut record = CrawlRecord::pending(url.to_string(), 0);
        record.status_code = Some(200);
        record.custom_extractions = vec![CustomExtractionValue {
            name: name.to_string(),
            values: vec![value.to_string()],
        }];
        record
    }

    fn custom_search_record(url: &str, count: usize, snippet: &str) -> CrawlRecord {
        let mut record = CrawlRecord::pending(url.to_string(), 0);
        record.status_code = Some(200);
        record.custom_searches = vec![CustomSearchValue {
            name: "analytics".to_string(),
            source: CustomSearchSource::RawHtml,
            matched: count > 0,
            match_count: count,
            snippets: vec![snippet.to_string()],
        }];
        record
    }

    fn assert_custom_searches_support_search_and_sort<S: CrawlStore>(store: S) {
        store.upsert(custom_search_record(
            "https://example.com/low",
            1,
            "analytics once",
        ));
        store.upsert(custom_search_record(
            "https://example.com/high",
            3,
            "analytics snippet zebra",
        ));

        let sorted = store.query(GridQuery {
            sort_by: Some("search:analytics:0".to_string()),
            sort_dir: SortDirection::Desc,
            ..GridQuery::default()
        });
        let filtered = store.query(GridQuery {
            global_search: Some("zebra".to_string()),
            ..GridQuery::default()
        });

        assert_eq!(sorted.total, 2);
        assert_eq!(sorted.rows[0].final_url, "https://example.com/high");
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.rows[0].final_url, "https://example.com/high");
    }

    fn assert_search_console_metrics_merge_into_records<S: CrawlStore>(store: S) {
        store.upsert(CrawlRecord::pending(
            "https://example.com/page".to_string(),
            0,
        ));
        store.upsert(CrawlRecord::pending(
            "https://example.com/other".to_string(),
            0,
        ));

        let matched = store.merge_search_console_metrics(vec![SearchConsoleMetricRow {
            url: "https://example.com/page#fragment".to_string(),
            clicks: 42.0,
            impressions: 420.0,
            ctr: 0.1,
            average_position: 3.7,
        }]);

        assert_eq!(matched, 1);
        let sorted = store.query(GridQuery {
            sort_by: Some("searchConsoleClicks".to_string()),
            sort_dir: SortDirection::Desc,
            ..GridQuery::default()
        });
        assert_eq!(sorted.rows[0].final_url, "https://example.com/page");
        assert_eq!(sorted.rows[0].search_console_clicks, Some(42.0));
        assert_eq!(sorted.rows[0].search_console_impressions, Some(420.0));
        assert_eq!(sorted.rows[0].search_console_ctr, Some(0.1));
        assert_eq!(sorted.rows[0].search_console_average_position, Some(3.7));
    }

    fn assert_html_validation_issue_views<S: CrawlStore>(store: S) {
        let mut deprecated = CrawlRecord::pending("https://example.com/legacy".to_string(), 0);
        deprecated.status_code = Some(200);
        deprecated.content_type = Some("text/html".to_string());
        deprecated.deprecated_html_tag_count = 2;
        store.upsert(deprecated);

        let mut duplicate_ids =
            CrawlRecord::pending("https://example.com/duplicate".to_string(), 0);
        duplicate_ids.status_code = Some(200);
        duplicate_ids.content_type = Some("text/html".to_string());
        duplicate_ids.duplicate_id_count = 1;
        store.upsert(duplicate_ids);

        let deprecated_rows = store.query(GridQuery {
            view: IssueView::HtmlDeprecatedTags,
            sort_by: Some("deprecatedHtmlTagCount".to_string()),
            sort_dir: SortDirection::Desc,
            ..GridQuery::default()
        });
        assert_eq!(deprecated_rows.total, 1);
        assert_eq!(deprecated_rows.summary.deprecated_html_tags, 1);
        assert_eq!(deprecated_rows.summary.duplicate_ids, 1);
        assert_eq!(deprecated_rows.rows[0].deprecated_html_tag_count, 2);

        let duplicate_rows = store.query(GridQuery {
            view: IssueView::HtmlDuplicateIds,
            sort_by: Some("duplicateIdCount".to_string()),
            sort_dir: SortDirection::Desc,
            ..GridQuery::default()
        });
        assert_eq!(duplicate_rows.total, 1);
        assert_eq!(duplicate_rows.rows[0].duplicate_id_count, 1);
    }

    fn assert_rendered_dom_change_view<S: CrawlStore>(store: S) {
        let mut rendered = CrawlRecord::pending("https://example.com/rendered".to_string(), 0);
        rendered.status_code = Some(200);
        rendered.content_type = Some("text/html; charset=utf-8".to_string());
        rendered.js_rendered = true;
        rendered.rendered_dom_changed = true;
        rendered.rendered_word_count_delta = 15;
        rendered.rendered_link_count_delta = 3;
        store.upsert(rendered);

        let mut stable = CrawlRecord::pending("https://example.com/stable".to_string(), 0);
        stable.status_code = Some(200);
        stable.content_type = Some("text/html; charset=utf-8".to_string());
        stable.js_rendered = true;
        store.upsert(stable);

        let response = store.query(GridQuery {
            view: IssueView::RenderedDomChanged,
            sort_by: Some("renderedWordCountDelta".to_string()),
            sort_dir: SortDirection::Desc,
            ..GridQuery::default()
        });

        assert_eq!(response.total, 1);
        assert_eq!(response.summary.rendered_dom_changed, 1);
        assert_eq!(response.rows[0].rendered_word_count_delta, 15);
        assert_eq!(response.rows[0].rendered_link_count_delta, 3);
    }

    fn assert_image_asset_store<S: CrawlStore>(store: S) {
        let mut page = CrawlRecord::pending("https://example.com/page".to_string(), 0);
        page.status_code = Some(200);
        page.content_type = Some("text/html; charset=utf-8".to_string());
        store.upsert(page);

        let mut hero = CrawlRecord::pending("https://example.com/hero.png".to_string(), 1);
        hero.status_code = Some(200);
        hero.content_type = Some("image/png".to_string());
        hero.size_bytes = IMAGE_ASSET_OVERSIZE_BYTES as usize + 1;
        store.upsert(hero);

        store.add_image_assets(
            "https://example.com/page",
            vec![
                test_image_asset(
                    "https://example.com/page",
                    "https://example.com/hero.png",
                    None,
                    true,
                    false,
                    Some(1600),
                    Some(900),
                    3,
                ),
                test_image_asset(
                    "https://example.com/page",
                    "https://example.com/logo.svg",
                    Some("Brand logo"),
                    false,
                    false,
                    Some(120),
                    Some(40),
                    8,
                ),
            ],
        );

        let all = store.image_assets(ImageAssetQuery {
            page_url: Some("https://example.com/page".to_string()),
            sort_by: Some("sourcePosition".to_string()),
            ..ImageAssetQuery::default()
        });
        assert_eq!(all.total, 2);
        assert_eq!(all.images[0].image_url, "https://example.com/hero.png");
        assert_eq!(all.images[0].width, Some(1600));
        assert_eq!(all.images[0].height, Some(900));
        assert_eq!(
            all.images[0].size_bytes,
            Some(IMAGE_ASSET_OVERSIZE_BYTES + 1)
        );
        assert!(all.images[0].oversized);

        let missing_alt = store.image_assets(ImageAssetQuery {
            page_url: Some("https://example.com/page".to_string()),
            missing_alt_only: true,
            ..ImageAssetQuery::default()
        });
        assert_eq!(missing_alt.total, 1);
        assert_eq!(
            missing_alt.images[0].image_url,
            "https://example.com/hero.png"
        );

        let oversized = store.image_assets(ImageAssetQuery {
            page_url: Some("https://example.com/page".to_string()),
            oversized_only: true,
            ..ImageAssetQuery::default()
        });
        assert_eq!(oversized.total, 1);

        store.add_image_assets(
            "https://example.com/page",
            vec![test_image_asset(
                "https://example.com/page",
                "https://example.com/replaced.webp",
                Some("Replacement"),
                false,
                false,
                Some(640),
                Some(360),
                1,
            )],
        );
        let replaced = store.image_assets(ImageAssetQuery {
            page_url: Some("https://example.com/page".to_string()),
            ..ImageAssetQuery::default()
        });
        assert_eq!(replaced.total, 1);
        assert_eq!(
            replaced.images[0].image_url,
            "https://example.com/replaced.webp"
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn test_image_asset(
        page_url: &str,
        image_url: &str,
        alt_text: Option<&str>,
        missing_alt: bool,
        alt_too_long: bool,
        width: Option<u32>,
        height: Option<u32>,
        source_position: u32,
    ) -> ImageAsset {
        ImageAsset {
            id: 0,
            page_url: page_url.to_string(),
            image_url: image_url.to_string(),
            alt_text: alt_text.map(ToString::to_string),
            alt_len: alt_text.map(str::len).unwrap_or(0) as u32,
            missing_alt,
            alt_too_long,
            width,
            height,
            source_position,
            size_bytes: None,
            oversized: false,
        }
    }

    fn assert_frontier_state_store<S: CrawlStore>(store: S) {
        let state = CrawlFrontierState {
            queued: vec![
                CrawlFrontierItem {
                    url: "https://example.com/a".to_string(),
                    depth: 1,
                    from_sitemap: false,
                    storage_key: "https://example.com/a".to_string(),
                    list_position: None,
                    list_duplicate_index: 0,
                },
                CrawlFrontierItem {
                    url: "https://example.com/list".to_string(),
                    depth: 0,
                    from_sitemap: true,
                    storage_key: "list:2:https://example.com/list".to_string(),
                    list_position: Some(2),
                    list_duplicate_index: 1,
                },
            ],
            seen: vec![
                "https://example.com/a".to_string(),
                "list:2:https://example.com/list".to_string(),
            ],
            crawled: 42,
        };

        store.save_frontier_state(state.clone());
        assert_eq!(store.load_frontier_state(), Some(state));
        store.clear_frontier_state();
        assert_eq!(store.load_frontier_state(), None);
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

    fn assert_crawl_path_finds_internal_chain(store: impl CrawlStore) {
        let mut home = CrawlRecord::pending("https://example.com/".to_string(), 0);
        home.status_code = Some(200);
        store.upsert(home);
        let mut category = CrawlRecord::pending("https://example.com/category".to_string(), 1);
        category.status_code = Some(200);
        store.upsert(category);
        let mut product = CrawlRecord::pending("https://example.com/product".to_string(), 2);
        product.status_code = Some(200);
        store.upsert(product);

        let mut first = test_edge(
            "https://example.com/",
            "https://example.com/category",
            LinkType::Internal,
        );
        first.anchor_text = "Category".to_string();
        first.target_depth = Some(1);
        first.discovery_order = 1;
        store.add_link_edge(first);

        let mut second = test_edge(
            "https://example.com/category",
            "https://example.com/product",
            LinkType::Internal,
        );
        second.anchor_text = "Product".to_string();
        second.source_depth = 1;
        second.target_depth = Some(2);
        second.source_position = 3;
        second.discovery_order = 2;
        store.add_link_edge(second);

        let response = store.crawl_path(CrawlPathQuery {
            target_url: "https://example.com/product".to_string(),
            ..CrawlPathQuery::default()
        });

        assert!(response.found);
        assert_eq!(response.steps.len(), 2);
        assert_eq!(response.steps[0].anchor_text, "Category");
        assert_eq!(response.steps[1].anchor_text, "Product");
        assert_eq!(response.steps[1].source_position, 3);
    }

    fn assert_segments_filter_rows(store: impl CrawlStore) {
        let mut blog = CrawlRecord::pending("https://example.com/blog/post".to_string(), 1);
        blog.status_code = Some(200);
        store.upsert(blog);
        let mut product =
            CrawlRecord::pending("https://example.com/products/widget".to_string(), 1);
        product.status_code = Some(200);
        store.upsert(product);

        let contains = store.query(GridQuery {
            segment_pattern: Some("/blog".to_string()),
            ..GridQuery::default()
        });
        let regex = store.query(GridQuery {
            segment_pattern: Some(r"/products/.+".to_string()),
            segment_regex: true,
            ..GridQuery::default()
        });
        let invalid_regex = store.query(GridQuery {
            segment_pattern: Some("(".to_string()),
            segment_regex: true,
            ..GridQuery::default()
        });

        assert_eq!(contains.total, 1);
        assert_eq!(contains.rows[0].final_url, "https://example.com/blog/post");
        assert_eq!(regex.total, 1);
        assert_eq!(
            regex.rows[0].final_url,
            "https://example.com/products/widget"
        );
        assert_eq!(invalid_regex.total, 0);
    }
}
