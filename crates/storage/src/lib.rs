use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params, types::Value};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::sync::{Mutex, MutexGuard};
use thiserror::Error;

mod native_exports;

#[cfg(test)]
mod frontier_tests;

#[cfg(test)]
mod image_assets_tests;

#[cfg(test)]
mod metadata_whitespace_tests;

#[cfg(test)]
mod summary_tests;

const IMAGE_ASSET_OVERSIZE_BYTES: u64 = 200 * 1024;
pub const SUCCESS_HTML_SQL: &str = "status_code >= 200 AND status_code < 300 AND lower(content_type) LIKE '%text/html%' AND indexability_status != 'Response body incomplete'";
const NO_RESPONSE_SQL: &str = "status_code IS NULL AND error IS NOT NULL AND status_text != 'Blocked by robots.txt' AND error != 'Blocked by robots.txt'";
const MARK_SITEMAP_URLS_SQL: &str = "UPDATE crawl_records SET in_sitemap = 1
    WHERE (url = ?1 OR final_url = ?1) AND in_sitemap = 0";

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
    TitleMultiple,
    TitleTooShort,
    TitleTooLong,
    TitlePixelTooNarrow,
    TitlePixelTooWide,
    MetaMissing,
    MetaDuplicate,
    MetaMultiple,
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
    CanonicalUncrawled,
    CanonicalToRedirect,
    CanonicalToError,
    CanonicalNonIndexable,
    CanonicalChain,
    CanonicalLoop,
    PaginationNextToError,
    PaginationPrevToError,
    PaginationNextLoop,
    PaginationPrevLoop,
    PaginationNextNonReciprocal,
    PaginationPrevNonReciprocal,
    AmpToError,
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
    ExactDuplicate,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageSpeedStrategy {
    Mobile,
    Desktop,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedSnapshot {
    pub strategy: PageSpeedStrategy,
    pub requested_url: String,
    pub completed_at_ms: i64,
    pub final_url: Option<String>,
    pub fetched_at: Option<String>,
    pub lighthouse_version: Option<String>,
    pub performance_score: Option<f64>,
    pub accessibility_score: Option<f64>,
    pub best_practices_score: Option<f64>,
    pub seo_score: Option<f64>,
    pub lcp_ms: Option<f64>,
    pub cls: Option<f64>,
    pub tbt_ms: Option<f64>,
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
    #[serde(default)]
    pub title_count: Option<usize>,
    pub title_len: usize,
    pub title_pixel_width: u32,
    pub meta_description: Option<String>,
    #[serde(default)]
    pub meta_description_count: Option<usize>,
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
    #[serde(default)]
    pub page_speed: Option<PageSpeedSnapshot>,
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
            title_count: None,
            title_len: 0,
            title_pixel_width: 0,
            meta_description: None,
            meta_description_count: None,
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
            page_speed: None,
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
    #[serde(default)]
    pub exact_duplicates: usize,
    pub indexable: usize,
    pub non_indexable: usize,
    pub title_missing: usize,
    pub title_duplicate: usize,
    #[serde(default)]
    pub title_multiple: usize,
    pub meta_missing: usize,
    pub meta_duplicate: usize,
    #[serde(default)]
    pub meta_multiple: usize,
    pub h1_missing: usize,
    pub h1_duplicate: usize,
    pub h2_missing: usize,
    pub h2_duplicate: usize,
    pub canonical_missing: usize,
    pub canonical_multiple: usize,
    #[serde(default)]
    pub canonical_uncrawled: usize,
    #[serde(default)]
    pub canonical_to_redirect: usize,
    #[serde(default)]
    pub canonical_to_error: usize,
    #[serde(default)]
    pub canonical_non_indexable: usize,
    #[serde(default)]
    pub canonical_chain: usize,
    #[serde(default)]
    pub canonical_loop: usize,
    #[serde(default)]
    pub pagination_next_to_error: usize,
    #[serde(default)]
    pub pagination_prev_to_error: usize,
    #[serde(default)]
    pub pagination_next_loop: usize,
    #[serde(default)]
    pub pagination_prev_loop: usize,
    #[serde(default)]
    pub pagination_next_non_reciprocal: usize,
    #[serde(default)]
    pub pagination_prev_non_reciprocal: usize,
    #[serde(default)]
    pub amp_to_error: usize,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<GridFilterGroup>,
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
            filters: None,
            sort_by: None,
            sort_dir: SortDirection::Asc,
            view: IssueView::All,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GridFilterGroup {
    #[serde(rename = "match")]
    pub match_mode: GridFilterMatch,
    pub rules: Vec<GridFilterRule>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GridFilterMatch {
    All,
    Any,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GridFilterRule {
    pub field: GridFilterField,
    pub operator: GridFilterOperator,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GridFilterField {
    Url,
    FinalUrl,
    Title,
    MetaDescription,
    Canonical,
    StatusCode,
    Depth,
    WordCount,
    ResponseTimeMs,
    Indexability,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GridFilterOperator {
    Contains,
    NotContains,
    Equals,
    NotEquals,
    IsEmpty,
    IsNotEmpty,
    LessThan,
    GreaterThan,
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
    #[error("crawl record {0} was not found")]
    RecordNotFound(u64),
    #[error("invalid PageSpeed snapshot: {0}")]
    InvalidPageSpeedSnapshot(String),
    #[error("invalid grid query: {0}")]
    InvalidQuery(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("storage lock poisoned")]
    LockPoisoned,
}

fn validate_page_speed_snapshot(snapshot: &PageSpeedSnapshot) -> Result<(), StorageError> {
    for (name, value) in [
        ("performanceScore", snapshot.performance_score),
        ("accessibilityScore", snapshot.accessibility_score),
        ("bestPracticesScore", snapshot.best_practices_score),
        ("seoScore", snapshot.seo_score),
        ("lcpMs", snapshot.lcp_ms),
        ("cls", snapshot.cls),
        ("tbtMs", snapshot.tbt_ms),
    ] {
        if value.is_some_and(|value| !value.is_finite()) {
            return Err(StorageError::InvalidPageSpeedSnapshot(format!(
                "{name} must be finite"
            )));
        }
    }
    Ok(())
}

pub trait CrawlStore: Clone + Send + Sync + 'static {
    fn clear(&self);
    fn upsert(&self, record: CrawlRecord) -> CrawlRecord;
    fn add_inlink(&self, target_url: &str);
    fn mark_sitemap_urls(&self, urls: &[String]);
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

    /// Frequent crawler progress does not rebuild cross-page canonical diagnostics.
    fn progress_summary(&self) -> CrawlSummary {
        summarize_without_canonicals(&self.records())
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
    alias_to_indices: HashMap<String, BTreeSet<usize>>,
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

        let index = if let Some(index) = inner.url_to_index.get(&key).copied() {
            for alias in record_url_aliases(&inner.records[index]) {
                if let Some(indices) = inner.alias_to_indices.get_mut(&alias) {
                    indices.remove(&index);
                    if indices.is_empty() {
                        inner.alias_to_indices.remove(&alias);
                    }
                }
            }
            record.id = inner.records[index].id;
            if record.page_speed.is_none() {
                record.page_speed = inner.records[index].page_speed.clone();
            }
            inner.records[index] = record.clone();
            index
        } else {
            inner.next_id += 1;
            record.id = inner.next_id;
            let index = inner.records.len();
            inner.url_to_index.insert(key, index);
            inner.records.push(record.clone());
            index
        };
        for alias in record_url_aliases(&record) {
            inner
                .alias_to_indices
                .entry(alias)
                .or_default()
                .insert(index);
        }
        update_memory_edge_statuses(&mut inner.link_edges, &record);
        apply_first_inlink_sources(std::slice::from_mut(&mut record), &inner.link_edges);
        record
    }

    /// Replace only the selected occurrence's latest snapshot; aliases are not merged.
    pub fn try_save_page_speed(
        &self,
        id: u64,
        snapshot: PageSpeedSnapshot,
    ) -> Result<(), StorageError> {
        validate_page_speed_snapshot(&snapshot)?;
        let mut inner = self.inner.write().map_err(|_| StorageError::LockPoisoned)?;
        let record = inner
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(StorageError::RecordNotFound(id))?;
        record.page_speed = Some(snapshot);
        Ok(())
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

        let indices: HashSet<usize> = target_aliases
            .iter()
            .filter_map(|alias| inner.alias_to_indices.get(alias))
            .flatten()
            .copied()
            .collect();
        for index in indices {
            let record_aliases = record_url_aliases(&inner.records[index]);
            inner.records[index].inlink_count =
                memory_inlink_count_for_aliases(&inner.inlink_counts, &record_aliases);
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

    pub fn crawl_graph(&self, query: CrawlGraphQuery) -> CrawlGraph {
        let inner = self.inner.read().expect("memory store lock poisoned");
        let matching_edges = inner
            .link_edges
            .iter()
            .filter(|edge| !query.internal_only || edge.link_type == LinkType::Internal);
        let total_edges = matching_edges.clone().count();
        let edges = matching_edges
            .take(query.max_edges.clamp(1, 1_000_000))
            .cloned()
            .collect();
        let nodes = graph_record_nodes(&inner.records, &query);
        finish_crawl_graph(nodes, edges, total_edges, query.max_nodes.max(1))
    }

    pub fn summary(&self) -> CrawlSummary {
        let records = self.records();
        summarize(&records)
    }

    pub fn query(&self, query: GridQuery) -> GridResponse {
        if validate_grid_query(&query).is_err() {
            return GridResponse {
                rows: Vec::new(),
                total: 0,
                summary: self.summary(),
            };
        }
        let mut rows = self.records();
        let references = reference_diagnostics(&rows);
        let exact = exact_duplicate_hashes(&rows);
        let mut summary = summarize_without_canonicals(&rows);
        add_reference_summary(&mut summary, &references);
        summary.exact_duplicates = rows
            .iter()
            .filter(|row| is_exact_duplicate_record(row, &exact))
            .count();
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
        let mut references = references.iter();

        rows.retain(|row| {
            matches_view(
                row,
                &query.view,
                &title_counts,
                &meta_counts,
                &h1_counts,
                &h2_counts,
                &near_duplicate_counts,
                &exact,
                &hreflang_index,
                references
                    .next()
                    .expect("one reference diagnostic per record"),
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

        if let Some(group) = &query.filters {
            let rules: Vec<_> = group.rules.iter().map(PreparedGridRule::new).collect();
            rows.retain(|row| {
                rules.is_empty()
                    || match group.match_mode {
                        GridFilterMatch::All => rules.iter().all(|rule| rule.matches(row)),
                        GridFilterMatch::Any => rules.iter().any(|rule| rule.matches(row)),
                    }
            });
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
        let sizes = image_size_by_alias(&inner.records);
        let page_aliases = query
            .page_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(url_aliases);
        let search = query
            .global_search
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        // Sorting keeps references; only the returned page clones image strings.
        let mut images = inner
            .image_assets
            .iter()
            .filter(|image| {
                (!query.missing_alt_only || image.missing_alt)
                    && page_aliases
                        .as_ref()
                        .is_none_or(|aliases| !aliases.is_disjoint(&url_aliases(&image.page_url)))
                    && search
                        .as_deref()
                        .is_none_or(|search| image_asset_matches_search(image, search))
            })
            .map(|image| {
                let size = url_aliases(&image.image_url)
                    .iter()
                    .filter_map(|alias| sizes.get(alias))
                    .max_by_key(|(id, _)| *id)
                    .map(|(_, size)| *size);
                (image, size)
            })
            .filter(|(_, size)| {
                !query.oversized_only || size.is_some_and(|size| size > IMAGE_ASSET_OVERSIZE_BYTES)
            })
            .collect::<Vec<_>>();
        let sort_by = query.sort_by.as_deref().unwrap_or("pageUrl");
        images.sort_by(|(left, left_size), (right, right_size)| {
            let ordering = match sort_by {
                "sizeBytes" => left_size.cmp(right_size),
                "oversized" => left_size
                    .is_some_and(|size| size > IMAGE_ASSET_OVERSIZE_BYTES)
                    .cmp(&right_size.is_some_and(|size| size > IMAGE_ASSET_OVERSIZE_BYTES)),
                _ => compare_image_assets(left, right, sort_by),
            }
            .then_with(|| left.page_url.cmp(&right.page_url))
            .then_with(|| left.source_position.cmp(&right.source_position));
            if query.sort_by.is_some() && query.sort_dir == SortDirection::Desc {
                ordering.reverse()
            } else {
                ordering
            }
        });
        let total = images.len();
        let limit = query.limit.min(1_000_000);
        let images = images
            .into_iter()
            .skip(query.offset)
            .take(limit)
            .map(|(image, size_bytes)| ImageAsset {
                size_bytes,
                oversized: size_bytes.is_some_and(|size| size > IMAGE_ASSET_OVERSIZE_BYTES),
                ..image.clone()
            })
            .collect();
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

    fn mark_sitemap_urls(&self, urls: &[String]) {
        if urls.is_empty() {
            return;
        }
        let urls: HashSet<&str> = urls.iter().map(String::as_str).collect();
        let mut inner = self.inner.write().expect("memory store lock poisoned");
        for record in &mut inner.records {
            if urls.contains(record.url.as_str()) || urls.contains(record.final_url.as_str()) {
                record.in_sitemap = true;
            }
        }
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

    fn crawl_graph(&self, query: CrawlGraphQuery) -> CrawlGraph {
        Self::crawl_graph(self, query)
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

    fn progress_summary(&self) -> CrawlSummary {
        let inner = self.inner.read().expect("memory store lock poisoned");
        summarize_without_canonicals(&inner.records)
    }

    fn sitemap_validation(&self, query: SitemapValidationQuery) -> SitemapValidationResponse {
        let inner = self.inner.read().expect("memory store lock poisoned");
        build_sitemap_validation_report(&inner.records, query)
    }
}

#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
    summary_cache: Arc<Mutex<Option<CachedSummary>>>,
    reference_cache: Arc<Mutex<Option<CachedReferences>>>,
    exact_duplicate_cache: Arc<Mutex<Option<CachedExactDuplicates>>>,
    image_alias_revision: Arc<Mutex<Option<i64>>>,
}

struct CachedSummary {
    revision: i64,
    summary: CrawlSummary,
}

struct CachedReferences {
    revision: i64,
    counts: [usize; 13],
}

struct CachedExactDuplicates {
    revision: i64,
    count: usize,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            summary_cache: Default::default(),
            reference_cache: Default::default(),
            exact_duplicate_cache: Default::default(),
            image_alias_revision: Default::default(),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            summary_cache: Default::default(),
            reference_cache: Default::default(),
            exact_duplicate_cache: Default::default(),
            image_alias_revision: Default::default(),
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

    pub fn try_mark_sitemap_urls(&self, urls: &[String]) -> Result<(), StorageError> {
        if urls.is_empty() {
            return Ok(());
        }
        let mut conn = self.connection()?;
        let transaction = conn.transaction()?;
        {
            let mut statement = transaction.prepare_cached(MARK_SITEMAP_URLS_SQL)?;
            for url in urls {
                statement.execute([url])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn try_upsert(&self, mut record: CrawlRecord) -> Result<CrawlRecord, StorageError> {
        let conn = self.connection()?;
        if record.storage_key.trim().is_empty() {
            record.storage_key = record.final_url.clone();
        }
        let existing = conn
            .query_row(
                "SELECT id, page_speed FROM crawl_records WHERE storage_key = ?1",
                [&record.storage_key],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()?;
        if record.page_speed.is_none() {
            record.page_speed = existing
                .as_ref()
                .and_then(|(_, snapshot)| snapshot.as_deref())
                .map(serde_json::from_str)
                .transpose()?;
        }
        let existing_id = existing.map(|(id, _)| id);
        record.inlink_count =
            sqlite_inlink_count_for_record(&conn, &record)?.unwrap_or(record.inlink_count);
        let redirect_chain = serde_json::to_string(&record.redirect_chain)?;
        let hreflang_links = serde_json::to_string(&record.hreflang_links)?;
        let structured_data_issues = serde_json::to_string(&record.structured_data_issues)?;
        let custom_extractions = serde_json::to_string(&record.custom_extractions)?;
        let custom_searches = serde_json::to_string(&record.custom_searches)?;
        let page_speed = record
            .page_speed
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
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
                    meta_description_pixel_width = ?88,
                    title_count = ?89,
                    meta_description_count = ?90,
                    page_speed = ?91
                 WHERE id = ?92",
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
                    record.title_count.map(|count| count as i64),
                    record.meta_description_count.map(|count| count as i64),
                    page_speed,
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
                    meta_description_pixel_width,
                    title_count,
                    meta_description_count,
                    page_speed
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                    ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35,
                    ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46,
                    ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54, ?55, ?56, ?57,
                    ?58, ?59, ?60, ?61, ?62, ?63, ?64, ?65, ?66, ?67, ?68, ?69,
                    ?70, ?71, ?72, ?73, ?74, ?75, ?76, ?77, ?78, ?79, ?80,
                    ?81, ?82, ?83, ?84, ?85, ?86, ?87, ?88, ?89, ?90, ?91
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
                    record.meta_description_pixel_width,
                    record.title_count.map(|count| count as i64),
                    record.meta_description_count.map(|count| count as i64),
                    page_speed
                ],
            )?;
            record.id = conn.last_insert_rowid() as u64;
            update_sqlite_edge_statuses(&conn, &record)?;
            annotate_sqlite_first_inlink_sources(&conn, std::slice::from_mut(&mut record))?;
            Ok(record)
        }
    }

    /// Replace one occurrence's snapshot atomically without rewriting crawl evidence.
    pub fn try_save_page_speed(
        &self,
        id: u64,
        snapshot: PageSpeedSnapshot,
    ) -> Result<(), StorageError> {
        validate_page_speed_snapshot(&snapshot)?;
        let payload = serde_json::to_string(&snapshot)?;
        let sql_id = i64::try_from(id).map_err(|_| StorageError::RecordNotFound(id))?;
        let mut conn = self.connection()?;
        let transaction = conn.transaction()?;
        if transaction.execute(
            "UPDATE crawl_records SET page_speed = ?1 WHERE id = ?2",
            params![payload, sql_id],
        )? == 0
        {
            return Err(StorageError::RecordNotFound(id));
        }
        transaction.commit()?;
        Ok(())
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

    pub fn try_crawl_graph(&self, query: CrawlGraphQuery) -> Result<CrawlGraph, StorageError> {
        let conn = self.connection()?;
        let transaction = conn.unchecked_transaction()?;
        let edge_filter = if query.internal_only {
            " WHERE link_type = 'internal'"
        } else {
            ""
        };
        let total_edges = count_query(
            &transaction,
            &format!("SELECT COUNT(*) FROM link_edges{edge_filter}"),
            &[],
        )?;
        let edge_limit = query.max_edges.clamp(1, 1_000_000);
        let edges = query_link_edges_with_args(
            &transaction,
            &format!("SELECT * FROM link_edges{edge_filter} ORDER BY id ASC LIMIT {edge_limit}"),
            &[],
        )?;
        let filter = if query.internal_only {
            " AND classification != 'external'"
        } else {
            ""
        };
        let max_nodes = query.max_nodes.max(1);
        // Admission follows the first occurrence; metadata follows the last occurrence
        // of that literal final URL. Rank only keys, then decode the capped winners.
        let sql = format!(
            "WITH first_occurrences AS (
                SELECT final_url, COALESCE(list_position, id) AS position, id,
                    ROW_NUMBER() OVER (
                        PARTITION BY final_url ORDER BY COALESCE(list_position, id), id
                    ) AS occurrence
                FROM crawl_records WHERE 1{filter}
            ), selected_urls AS (
                SELECT final_url FROM first_occurrences WHERE occurrence = 1
                ORDER BY position, id LIMIT ?1
            )
            SELECT r.final_url, r.classification, r.status_code, r.depth,
                r.indexability, r.inlink_count, r.outlink_count,
                COALESCE((status_code IS NOT NULL OR ({NO_RESPONSE_SQL})), 0) AS crawled
            FROM selected_urls selected JOIN crawl_records r ON r.id = (
                SELECT id FROM crawl_records WHERE final_url = selected.final_url{filter}
                ORDER BY COALESCE(list_position, id) DESC, id DESC LIMIT 1
            )"
        );
        let mut statement = transaction.prepare(&sql)?;
        let nodes = statement
            .query_map([max_nodes.min(i64::MAX as usize) as i64], |row| {
                let url: String = row.get("final_url")?;
                let classification: String = row.get("classification")?;
                let depth: i64 = row.get("depth")?;
                let node = GraphNode {
                    label: graph_label(&url),
                    url: url.clone(),
                    crawled: row.get("crawled")?,
                    classification: Some(classification_from_str(&classification)),
                    status_code: row.get("status_code")?,
                    depth: Some(depth as usize),
                    indexability: Some(row.get("indexability")?),
                    inlink_count: row.get("inlink_count")?,
                    outlink_count: row.get("outlink_count")?,
                };
                Ok((url, node))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(finish_crawl_graph(nodes, edges, total_edges, max_nodes))
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
                format!(" ORDER BY {column} {direction}, id ASC")
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
        let (where_clause, args) = image_asset_filter_sql(&query)?;
        let total_sql = if query.oversized_only {
            self.ensure_image_record_aliases(&conn)?;
            format!("{IMAGE_ASSET_SIZE_CTE} SELECT COUNT(*) FROM annotated_images{where_clause}")
        } else {
            format!("SELECT COUNT(*) FROM image_assets{where_clause}")
        };
        let total = count_query(&conn, &total_sql, &args)?;
        let limit = query.limit.min(1_000_000);
        if limit == 0 || query.offset >= total {
            return Ok(ImageAssetResponse {
                images: Vec::new(),
                total,
            });
        }
        if !query.oversized_only {
            self.ensure_image_record_aliases(&conn)?;
        }
        let sort_by = image_asset_sort_column(query.sort_by.as_deref());
        let direction = if query.sort_by.is_some() && query.sort_dir == SortDirection::Desc {
            "DESC"
        } else {
            "ASC"
        };
        let mut stmt = conn.prepare(&format!(
            "{IMAGE_ASSET_SIZE_CTE} SELECT * FROM annotated_images{where_clause}
             ORDER BY {sort_by} {direction}, page_url {direction}, source_position {direction}, id ASC
             LIMIT {limit} OFFSET {}", query.offset
        ))?;
        let images = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter()),
                image_asset_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ImageAssetResponse { images, total })
    }

    pub fn try_sitemap_validation(
        &self,
        query: SitemapValidationQuery,
    ) -> Result<SitemapValidationResponse, StorageError> {
        let conn = self.connection()?;
        // Count and page retain one read snapshot if another connection commits meanwhile.
        let transaction = conn.unchecked_transaction()?;
        let cte = sitemap_validation_cte();
        let (filter, args) = sitemap_validation_filter_sql(&query);
        let total = count_query(
            &transaction,
            &format!("{cte} SELECT COUNT(*) FROM sitemap_report{filter}"),
            &args,
        )?;
        let limit = query.limit.min(1_000_000);
        if limit == 0 || query.offset >= total {
            return Ok(SitemapValidationResponse {
                rows: Vec::new(),
                total,
            });
        }
        let order = sitemap_validation_order_sql(&query);
        let mut statement = transaction.prepare(&format!(
            "{cte} SELECT url, final_url, status_code, status_text, indexability,
             indexability_status, inlink_count, redirect_target, canonical,
             issue_count, severity_rank, issues FROM sitemap_report{filter}
             ORDER BY {order} LIMIT {limit} OFFSET {}",
            query.offset
        ))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(args.iter()), |row| {
                let issues: String = row.get("issues")?;
                Ok(SitemapValidationRow {
                    url: row.get("url")?,
                    final_url: row.get("final_url")?,
                    status_code: row.get("status_code")?,
                    status_text: row.get("status_text")?,
                    indexability: row.get("indexability")?,
                    indexability_status: row.get("indexability_status")?,
                    inlink_count: row.get("inlink_count")?,
                    redirect_target: row.get("redirect_target")?,
                    canonical: row.get("canonical")?,
                    issue_count: usize::from(row.get::<_, u8>("issue_count")?),
                    severity: match row.get::<_, u8>("severity_rank")? {
                        2 => Severity::Error,
                        1 => Severity::Warning,
                        _ => Severity::Info,
                    },
                    issues: issues.split("; ").map(str::to_string).collect(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SitemapValidationResponse { rows, total })
    }

    pub fn try_save_frontier_state(&self, state: CrawlFrontierState) -> Result<(), StorageError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM crawl_frontier_queue", [])?;
        tx.execute("DELETE FROM crawl_frontier_seen", [])?;
        tx.execute("DELETE FROM crawl_frontier_meta", [])?;

        if !state.queued.is_empty() {
            let mut insert = tx.prepare(
                "INSERT INTO crawl_frontier_queue (
                    position,
                    url,
                    depth,
                    from_sitemap,
                    storage_key,
                    list_position,
                    list_duplicate_index
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (position, item) in state.queued.into_iter().enumerate() {
                insert.execute(params![
                    position as i64,
                    item.url,
                    item.depth as i64,
                    item.from_sitemap,
                    item.storage_key,
                    item.list_position.map(i64::from),
                    i64::from(item.list_duplicate_index),
                ])?;
            }
        }

        if !state.seen.is_empty() {
            let mut insert =
                tx.prepare("INSERT OR IGNORE INTO crawl_frontier_seen (url) VALUES (?1)")?;
            for url in state.seen {
                insert.execute([url])?;
            }
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
        validate_grid_query(&query)?;
        let ctes = if matches!(
            query.view,
            IssueView::HreflangMissingReturnLink | IssueView::HreflangNonCanonicalTarget
        ) {
            HREFLANG_AUDIT_CTES
        } else {
            ""
        };

        let (where_clause, args) = query_filter_sql(&query);
        let order_by = if query.sort_by.is_some() {
            let column = sort_column(query.sort_by.as_deref()).unwrap_or_else(|| "id".into());
            let direction = match query.sort_dir {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };
            format!(" ORDER BY {column} {direction}, id ASC")
        } else {
            " ORDER BY COALESCE(list_position, id) ASC, id ASC".to_string()
        };
        let limit = query.limit.min(1_000_000);
        let total_sql = format!("{ctes}SELECT COUNT(*) FROM crawl_records{where_clause}");
        let select_sql = format!(
            "{ctes}SELECT * FROM crawl_records{where_clause}{order_by} LIMIT {limit} OFFSET {}",
            query.offset
        );
        let conn = self.connection()?;
        let summary = self.summary_with_connection(&conn)?;
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
        self.summary_with_connection(&conn)
    }

    pub fn try_progress_summary(&self) -> Result<CrawlSummary, StorageError> {
        let conn = self.connection()?;
        self.progress_summary_with_connection(&conn)
    }

    fn summary_with_connection(&self, conn: &Connection) -> Result<CrawlSummary, StorageError> {
        let mut summary = self.progress_summary_with_connection(conn)?;
        set_reference_summary(&mut summary, self.ensure_reference_diagnostics(conn)?);
        summary.exact_duplicates = self.ensure_exact_duplicates(conn)?;
        Ok(summary)
    }

    fn progress_summary_with_connection(
        &self,
        conn: &Connection,
    ) -> Result<CrawlSummary, StorageError> {
        let revision = crawl_audit_revision(conn)?;
        // ponytail: reuse aggregates between writes; incremental counters if active large crawls dominate.
        let mut cache = self
            .summary_cache
            .lock()
            .map_err(|_| StorageError::LockPoisoned)?;
        if let Some(cached) = &*cache
            && cached.revision == revision
        {
            return Ok(cached.summary.clone());
        }
        let mut summary = sqlite_progress_counts(conn)?;
        summary.title_duplicate = sqlite_duplicate_count(conn, "title")?;
        summary.meta_duplicate = sqlite_duplicate_count(conn, "meta_description")?;
        summary.h1_duplicate = sqlite_duplicate_count(conn, "h1")?;
        summary.h2_duplicate = sqlite_duplicate_count(conn, "h2")?;
        *cache = Some(CachedSummary {
            revision,
            summary: summary.clone(),
        });
        Ok(summary)
    }

    fn ensure_reference_diagnostics(&self, conn: &Connection) -> Result<[usize; 13], StorageError> {
        let revision = crawl_audit_revision(conn)?;
        let mut cache = self
            .reference_cache
            .lock()
            .map_err(|_| StorageError::LockPoisoned)?;
        if let Some(cached) = &*cache
            && cached.revision == revision
        {
            return Ok(cached.counts);
        }
        let transaction = conn.unchecked_transaction()?;
        let records = {
            let sql = format!("SELECT id, url, final_url, canonical,
                COALESCE(({SUCCESS_HTML_SQL}), 0), classification = 'internal',
                (status_code IS NOT NULL OR error IS NOT NULL OR status_text = 'Blocked by robots.txt'),
                COALESCE(({}), 0),
                (status_code IS NULL AND (status_text = 'Blocked by robots.txt' OR error = 'Blocked by robots.txt'))
                    OR (indexability = 'Non-indexable' AND NOT COALESCE(({}), 0)
                        AND NOT COALESCE(status_code BETWEEN 300 AND 399, 0)),
                COALESCE(status_code BETWEEN 300 AND 399, 0), redirect_chain,
                rel_next, rel_prev,
                COALESCE((status_code IS NOT NULL OR ({NO_RESPONSE_SQL})), 0),
                COALESCE((status_code BETWEEN 400 AND 599 OR ({NO_RESPONSE_SQL})
                    OR (status_code BETWEEN 300 AND 399 AND error IS NOT NULL)), 0),
                COALESCE(error != 'Redirect limit exceeded', 1), amphtml
                FROM crawl_records ORDER BY id", broken_record_sql(), broken_record_sql());
            let mut statement = transaction.prepare(&sql)?;
            let rows = statement.query_map([], reference_audit_record_from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let diagnostics = build_reference_diagnostics(&records);
        let counts = reference_counts(&diagnostics);
        transaction.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS ff_reference_diagnostics (
            record_id INTEGER PRIMARY KEY, flags INTEGER NOT NULL);
            DELETE FROM ff_reference_diagnostics;",
        )?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO ff_reference_diagnostics (record_id, flags) VALUES (?1, ?2)",
            )?;
            for (record, diagnostic) in records.iter().zip(diagnostics) {
                if diagnostic.flags() != 0 {
                    statement.execute(params![record.id as i64, diagnostic.flags()])?;
                }
            }
        }
        transaction.commit()?;
        // Keep the pre-read evidence revision: a concurrent external record write must still
        // invalidate this snapshot. Derived TEMP writes never change the evidence revision.
        *cache = Some(CachedReferences { revision, counts });
        Ok(counts)
    }

    fn ensure_exact_duplicates(&self, conn: &Connection) -> Result<usize, StorageError> {
        let revision = crawl_audit_revision(conn)?;
        let mut cache = self
            .exact_duplicate_cache
            .lock()
            .map_err(|_| StorageError::LockPoisoned)?;
        if let Some(cached) = &*cache
            && cached.revision == revision
        {
            return Ok(cached.count);
        }
        let transaction = conn.unchecked_transaction()?;
        transaction.execute_batch(&format!(
            "CREATE TEMP TABLE IF NOT EXISTS ff_exact_duplicate_records (record_id INTEGER PRIMARY KEY);
            DELETE FROM ff_exact_duplicate_records;
            INSERT INTO ff_exact_duplicate_records (record_id)
            SELECT id FROM crawl_records
            WHERE {SUCCESS_HTML_SQL} AND ff_final_url_key(final_url) IS NOT NULL
            AND response_hash IN (
                SELECT response_hash FROM crawl_records
                WHERE {SUCCESS_HTML_SQL} AND response_hash IS NOT NULL AND ff_text_key(response_hash) != ''
                GROUP BY response_hash
                HAVING COUNT(DISTINCT ff_final_url_key(final_url)) > 1
            );"
        ))?;
        let count = summary_count(
            &transaction,
            "SELECT COUNT(*) FROM ff_exact_duplicate_records",
        )?;
        transaction.commit()?;
        *cache = Some(CachedExactDuplicates { revision, count });
        Ok(count)
    }

    fn ensure_image_record_aliases(&self, conn: &Connection) -> Result<(), StorageError> {
        let revision = crawl_audit_revision(conn)?;
        let mut cached_revision = self
            .image_alias_revision
            .lock()
            .map_err(|_| StorageError::LockPoisoned)?;
        if *cached_revision == Some(revision) {
            return Ok(());
        }
        let transaction = conn.unchecked_transaction()?;
        transaction.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS ff_image_record_aliases (
                alias TEXT PRIMARY KEY, record_id INTEGER NOT NULL, size_bytes INTEGER NOT NULL
            ) WITHOUT ROWID;
            DELETE FROM ff_image_record_aliases;
            INSERT INTO ff_image_record_aliases (alias, record_id, size_bytes)
            SELECT a.value, cr.id, cr.size_bytes
            FROM crawl_records cr, json_each(ff_url_aliases(cr.storage_key, cr.url, cr.final_url)) a
            WHERE lower(cr.content_type) LIKE 'image/%'
            ON CONFLICT(alias) DO UPDATE SET record_id = excluded.record_id, size_bytes = excluded.size_bytes
            WHERE excluded.record_id > ff_image_record_aliases.record_id;",
        )?;
        transaction.commit()?;
        *cached_revision = Some(revision);
        Ok(())
    }

    fn initialize(&self) -> Result<(), StorageError> {
        let conn = self.connection()?;
        let flags = rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC;
        conn.create_scalar_function("ff_final_url_key", 1, flags, |context| {
            Ok(normalized_final_url(
                context.get_raw(0).as_str().unwrap_or_default(),
            ))
        })?;
        conn.create_scalar_function("ff_text_key", 1, flags, |context| {
            Ok(normalize_text_key(
                context
                    .get::<Option<String>>(0)?
                    .as_deref()
                    .unwrap_or_default(),
            ))
        })?;
        conn.create_scalar_function("ff_trim", 1, flags, |context| {
            Ok(context
                .get::<Option<String>>(0)?
                .unwrap_or_default()
                .trim()
                .to_string())
        })?;
        conn.create_scalar_function("ff_regexp", 2, flags, |context| {
            let regex = match context.get_aux::<Option<Regex>>(0)? {
                Some(regex) => regex,
                None => context.set_aux(0, Regex::new(&context.get::<String>(0)?).ok())?,
            };
            Ok(regex.as_ref().as_ref().is_some_and(|regex| {
                regex.is_match(context.get_raw(1).as_str().unwrap_or_default())
            }))
        })?;
        conn.create_scalar_function("ff_contains", 2, flags, |context| {
            use rusqlite::types::ValueRef;
            let text = match context.get_raw(0) {
                ValueRef::Text(_) => context.get_raw(0).as_str()?.to_lowercase(),
                ValueRef::Integer(value) => value.to_string(),
                ValueRef::Real(value) => value.to_string(),
                _ => return Ok(false),
            };
            Ok(text.contains(context.get_raw(1).as_str()?))
        })?;
        conn.create_scalar_function("ff_custom_contains", 4, flags, |context| {
            Ok(custom_data_matches_search(
                &serde_json::from_str::<Vec<CustomExtractionValue>>(context.get_raw(0).as_str()?)
                    .unwrap_or_default(),
                &serde_json::from_str::<Vec<CustomSearchValue>>(context.get_raw(1).as_str()?)
                    .unwrap_or_default(),
                &serde_json::from_str::<Vec<StructuredDataIssue>>(context.get_raw(2).as_str()?)
                    .unwrap_or_default(),
                context.get_raw(3).as_str()?,
            ))
        })?;
        conn.create_scalar_function("ff_extraction_sort", 2, flags, |context| {
            Ok(custom_extraction_sort_value(
                &serde_json::from_str::<Vec<CustomExtractionValue>>(context.get_raw(0).as_str()?)
                    .unwrap_or_default(),
                context.get_raw(1).as_str()?,
            ))
        })?;
        conn.create_scalar_function("ff_url_aliases", 3, flags, |context| {
            let aliases = sorted_aliases(url_aliases_many([
                context.get_raw(0).as_str()?,
                context.get_raw(1).as_str()?,
                context.get_raw(2).as_str()?,
            ]));
            serde_json::to_string(&aliases)
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
        })?;
        conn.create_scalar_function("ff_hreflang_aliases", 1, flags, |context| {
            let links = serde_json::from_str::<Vec<HreflangLink>>(context.get_raw(0).as_str()?)
                .unwrap_or_default();
            let aliases = links
                .iter()
                .enumerate()
                .filter(|(_, link)| link.valid)
                .flat_map(|(index, link)| {
                    url_aliases(&link.url)
                        .into_iter()
                        .map(move |alias| (index, alias))
                })
                .collect::<Vec<_>>();
            serde_json::to_string(&aliases)
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
        })?;
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
                title_count INTEGER,
                title_len INTEGER NOT NULL,
                title_pixel_width INTEGER NOT NULL DEFAULT 0,
                meta_description TEXT,
                meta_description_count INTEGER,
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
                page_speed TEXT,
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
        add_column_if_missing(&conn, "title_count", "INTEGER")?;
        add_column_if_missing(&conn, "meta_description_count", "INTEGER")?;
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
        add_column_if_missing(&conn, "page_speed", "TEXT")?;
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
        // Persistent triggers also observe writes from other SQLite connections and roll back
        // with the changed records. Frontier/seen/edge/TEMP writes do not invalidate audits.
        // Install after migrations which can replace crawl_records and remove its triggers.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS crawl_audit_revision (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                revision INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO crawl_audit_revision (id, revision) VALUES (1, 0);
            CREATE TRIGGER IF NOT EXISTS crawl_records_audit_insert AFTER INSERT ON crawl_records
            BEGIN
                UPDATE crawl_audit_revision SET revision = revision + 1 WHERE id = 1;
            END;
            CREATE TRIGGER IF NOT EXISTS crawl_records_audit_update AFTER UPDATE ON crawl_records
            BEGIN
                UPDATE crawl_audit_revision SET revision = revision + 1 WHERE id = 1;
            END;
            CREATE TRIGGER IF NOT EXISTS crawl_records_audit_delete AFTER DELETE ON crawl_records
            BEGIN
                UPDATE crawl_audit_revision SET revision = revision + 1 WHERE id = 1;
            END;",
        )?;
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

    fn mark_sitemap_urls(&self, urls: &[String]) {
        self.try_mark_sitemap_urls(urls)
            .expect("sqlite sitemap provenance update failed");
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

    fn crawl_graph(&self, query: CrawlGraphQuery) -> CrawlGraph {
        self.try_crawl_graph(query)
            .expect("sqlite graph query failed")
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

    fn progress_summary(&self) -> CrawlSummary {
        self.try_progress_summary()
            .expect("sqlite progress summary query failed")
    }

    fn sitemap_validation(&self, query: SitemapValidationQuery) -> SitemapValidationResponse {
        self.try_sitemap_validation(query)
            .expect("sqlite sitemap validation query failed")
    }
}

#[derive(Clone)]
pub enum ActiveStore {
    Memory(MemoryStore),
    Sqlite(SqliteStore),
}

impl ActiveStore {
    pub fn try_save_page_speed(
        &self,
        id: u64,
        snapshot: PageSpeedSnapshot,
    ) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => store.try_save_page_speed(id, snapshot),
            Self::Sqlite(store) => store.try_save_page_speed(id, snapshot),
        }
    }

    pub fn memory() -> Self {
        Self::Memory(MemoryStore::new())
    }

    pub fn sqlite(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Ok(Self::Sqlite(SqliteStore::open(path)?))
    }

    pub fn try_crawl_graph(&self, query: CrawlGraphQuery) -> Result<CrawlGraph, StorageError> {
        match self {
            Self::Memory(store) => Ok(store.crawl_graph(query)),
            Self::Sqlite(store) => store.try_crawl_graph(query),
        }
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

    fn mark_sitemap_urls(&self, urls: &[String]) {
        match self {
            ActiveStore::Memory(store) => store.mark_sitemap_urls(urls),
            ActiveStore::Sqlite(store) => store.mark_sitemap_urls(urls),
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

    fn crawl_graph(&self, query: CrawlGraphQuery) -> CrawlGraph {
        self.try_crawl_graph(query).expect("graph query failed")
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

    fn progress_summary(&self) -> CrawlSummary {
        match self {
            ActiveStore::Memory(store) => store.progress_summary(),
            ActiveStore::Sqlite(store) => store.progress_summary(),
        }
    }

    fn sitemap_validation(&self, query: SitemapValidationQuery) -> SitemapValidationResponse {
        match self {
            ActiveStore::Memory(store) => store.sitemap_validation(query),
            ActiveStore::Sqlite(store) => store.sitemap_validation(query),
        }
    }
}

pub fn summarize(records: &[CrawlRecord]) -> CrawlSummary {
    let mut summary = summarize_without_canonicals(records);
    add_reference_summary(&mut summary, &reference_diagnostics(records));
    let hashes = exact_duplicate_hashes(records);
    summary.exact_duplicates = records
        .iter()
        .filter(|row| is_exact_duplicate_record(row, &hashes))
        .count();
    summary
}

fn summarize_without_canonicals(records: &[CrawlRecord]) -> CrawlSummary {
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

        summary.title_multiple += usize::from(record.title_count.is_some_and(|count| count > 1));
        summary.meta_multiple +=
            usize::from(record.meta_description_count.is_some_and(|count| count > 1));

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

// Conditions and issue order mirror sitemap_validation_issues; parity fixtures cover both.
// These are record-local findings: repeated List/redirect aliases retain separate rows.
const SITEMAP_ISSUE_SQL: &[(&str, &str, u8)] = &[
    (
        "status_code IS NULL AND (status_text = 'Blocked by robots.txt' OR error = 'Blocked by robots.txt')",
        "Robots-blocked URL in sitemap",
        1,
    ),
    (NO_RESPONSE_SQL, "No response URL in sitemap", 2),
    (
        "status_code BETWEEN 300 AND 399",
        "Redirecting URL in sitemap",
        1,
    ),
    ("status_code BETWEEN 400 AND 499", "4xx URL in sitemap", 2),
    ("status_code BETWEEN 500 AND 599", "5xx URL in sitemap", 2),
    (
        "indexability != 'Indexable' OR indexability_status != 'Indexable'",
        "Non-indexable URL in sitemap",
        1,
    ),
    (
        "ff_trim(canonical) != '' AND ff_trim(canonical) != final_url",
        "Canonical points to a different URL",
        1,
    ),
    (
        "ff_trim(redirect_target) != ''",
        "Sitemap URL has a redirect target",
        1,
    ),
    ("classification = 'external'", "External URL in sitemap", 1),
    (
        "inlink_count = 0 AND classification != 'external'",
        "Orphan URL in sitemap",
        1,
    ),
    (
        "NOT (status_code IS NULL AND (status_text = 'Blocked by robots.txt' OR error = 'Blocked by robots.txt')) AND ff_trim(error) != ''",
        "Fetch error for sitemap URL",
        2,
    ),
];

fn sitemap_validation_cte() -> String {
    let mut flags = Vec::new();
    let mut counts = Vec::new();
    let mut severities = Vec::new();
    let mut messages = Vec::new();
    for (index, (condition, message, rank)) in SITEMAP_ISSUE_SQL.iter().enumerate() {
        let flag = format!("issue_{index}");
        flags.push(format!("CASE WHEN {condition} THEN 1 ELSE 0 END AS {flag}"));
        counts.push(flag.clone());
        severities.push(format!("{flag} * {rank}"));
        messages.push(format!(
            "CASE WHEN {flag} THEN '; {}' ELSE '' END",
            message.replace('\'', "''")
        ));
    }
    format!(
        "WITH sitemap_flags AS (
            SELECT id, list_position, url, final_url, status_code, status_text,
                indexability, indexability_status, inlink_count, redirect_target, canonical, {}
            FROM crawl_records WHERE in_sitemap != 0
        ), sitemap_report AS (
            SELECT *, MAX(1, {}) AS issue_count, MAX({}) AS severity_rank,
                COALESCE(NULLIF(SUBSTR({}, 3), ''), 'OK') AS issues
            FROM sitemap_flags
        )",
        flags.join(", "),
        counts.join(" + "),
        severities.join(", "),
        messages.join(" || ")
    )
}

fn sitemap_validation_filter_sql(query: &SitemapValidationQuery) -> (String, Vec<String>) {
    let Some(search) = query
        .global_search
        .as_deref()
        .map(str::trim)
        .filter(|search| !search.is_empty())
        .map(str::to_lowercase)
    else {
        return (String::new(), Vec::new());
    };
    let mut conditions = [
        "url",
        "final_url",
        "status_text",
        "indexability",
        "indexability_status",
        "redirect_target",
        "canonical",
        "status_code",
    ]
    .map(|column| format!("ff_contains({column}, ?1)"))
    .to_vec();
    // Match individual issue labels, so a search cannot bridge their display separator.
    for (index, (_, message, _)) in SITEMAP_ISSUE_SQL.iter().enumerate() {
        if message.to_lowercase().contains(&search) {
            conditions.push(format!("issue_{index} = 1"));
        }
    }
    if "ok".contains(&search) {
        conditions.push("severity_rank = 0".into());
    }
    (
        format!(" WHERE ({})", conditions.join(" OR ")),
        vec![search],
    )
}

fn sitemap_validation_order_sql(query: &SitemapValidationQuery) -> String {
    let descending = query.sort_by.is_some() && query.sort_dir == SortDirection::Desc;
    let direction = if descending { "DESC" } else { "ASC" };
    let reverse = if descending { "ASC" } else { "DESC" };
    let column = match query.sort_by.as_deref() {
        Some("url") => Some("url"),
        Some("finalUrl") => Some("final_url"),
        Some("statusCode") => Some("status_code"),
        Some("statusText") => Some("status_text"),
        Some("indexability") => Some("indexability"),
        Some("indexabilityStatus") => Some("indexability_status"),
        Some("inlinkCount") => Some("inlink_count"),
        Some("redirectTarget") => Some("redirect_target"),
        Some("canonical") => Some("canonical"),
        Some("issueCount") => Some("issue_count"),
        Some("severity") => Some("severity_rank"),
        Some("issues") => Some("issues"),
        _ => None,
    };
    let primary = column
        .map(|column| format!("{column} {direction}, "))
        .unwrap_or_default();
    // The old report stably sorted try_records(), whose input follows List position then ID.
    format!(
        "{primary}severity_rank {reverse}, issue_count {reverse}, final_url {direction}, COALESCE(list_position, id) ASC, id ASC"
    )
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
    #[cfg(test)]
    URL_ALIAS_EXPANSIONS.with(|count| count.set(count.get() + 1));

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

#[cfg(test)]
thread_local! {
    static URL_ALIAS_EXPANSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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

fn memory_record_index_by_url(inner: &MemoryStoreInner, url: &str) -> Option<usize> {
    // A redirect or List occurrence can share aliases with later records.
    // Keep the earliest stored record, including when its aliases change on update.
    url_aliases(url)
        .iter()
        .filter_map(|alias| inner.alias_to_indices.get(alias)?.first().copied())
        .min()
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
    let nodes = graph_record_nodes(&records, &query);
    finish_crawl_graph(nodes, edges, total_edges, query.max_nodes.max(1))
}

fn graph_record_nodes<'a>(
    records: impl IntoIterator<Item = &'a CrawlRecord>,
    query: &CrawlGraphQuery,
) -> HashMap<String, GraphNode> {
    let max_nodes = query.max_nodes.max(1);
    let mut selected = HashMap::new();
    for record in records.into_iter().filter(|record| {
        !query.internal_only || record.classification == UrlClassification::Internal
    }) {
        if selected.len() >= max_nodes && !selected.contains_key(record.final_url.as_str()) {
            continue;
        }
        selected.insert(record.final_url.as_str(), record);
    }
    selected
        .into_values()
        .map(|record| {
            (
                record.final_url.clone(),
                GraphNode {
                    label: graph_label(&record.final_url),
                    url: record.final_url.clone(),
                    crawled: record.status_code.is_some() || is_no_response_record(record),
                    classification: Some(record.classification.clone()),
                    status_code: record.status_code,
                    depth: Some(record.depth),
                    indexability: Some(record.indexability.clone()),
                    inlink_count: record.inlink_count,
                    outlink_count: record.outlink_count,
                },
            )
        })
        .collect()
}

fn finish_crawl_graph(
    mut nodes: HashMap<String, GraphNode>,
    edges: Vec<LinkEdge>,
    total_edges: usize,
    max_nodes: usize,
) -> CrawlGraph {
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
    let page_speed_column = row.as_ref().column_index("page_speed")?;
    let page_speed = row
        .get::<_, Option<String>>(page_speed_column)?
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    page_speed_column,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?;
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
        title_count: row
            .get::<_, Option<i64>>("title_count")?
            .map(|count| count.max(0) as usize),
        title_len: title_len as usize,
        title_pixel_width: title_pixel_width.max(0) as u32,
        meta_description: row.get("meta_description")?,
        meta_description_count: row
            .get::<_, Option<i64>>("meta_description_count")?
            .map(|count| count.max(0) as usize),
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
        page_speed,
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
             ORDER BY target_url ASC, discovery_order ASC, source_position ASC, source_url ASC, id ASC"
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
            title_count INTEGER,
            title_len INTEGER NOT NULL,
            title_pixel_width INTEGER NOT NULL DEFAULT 0,
            meta_description TEXT,
            meta_description_count INTEGER,
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
            page_speed TEXT,
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
            title_count,
            title_len,
            meta_description,
            meta_description_count,
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
            page_speed,
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
            title_count,
            title_len,
            meta_description,
            meta_description_count,
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
            page_speed,
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
        CREATE INDEX IF NOT EXISTS idx_crawl_records_url ON crawl_records(url);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_final_url ON crawl_records(final_url);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_list_position ON crawl_records(list_position);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_classification ON crawl_records(classification);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_depth ON crawl_records(depth);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_title ON crawl_records(title);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_meta_description ON crawl_records(meta_description);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_near_duplicate_cluster_id ON crawl_records(near_duplicate_cluster_id);
        CREATE INDEX IF NOT EXISTS idx_crawl_records_response_hash ON crawl_records(response_hash);
        ",
    )?;
    Ok(())
}

/// Validate advanced rules before querying or opening an export destination.
pub fn validate_grid_query(query: &GridQuery) -> Result<(), StorageError> {
    let Some(group) = &query.filters else {
        return Ok(());
    };
    if group.rules.len() > 20 {
        return Err(StorageError::InvalidQuery(
            "Advanced filters support at most 20 rules".into(),
        ));
    }
    for (index, rule) in group.rules.iter().enumerate() {
        let invalid = |message: String| {
            StorageError::InvalidQuery(format!("Filter rule {}: {message}", index + 1))
        };
        if rule.value.chars().take(2_001).count() > 2_000 {
            return Err(invalid("value exceeds 2,000 characters".into()));
        }
        let valid_operator = if rule.field.is_numeric() {
            matches!(
                rule.operator,
                GridFilterOperator::Equals
                    | GridFilterOperator::NotEquals
                    | GridFilterOperator::LessThan
                    | GridFilterOperator::GreaterThan
            )
        } else {
            !matches!(
                rule.operator,
                GridFilterOperator::LessThan | GridFilterOperator::GreaterThan
            )
        };
        if !valid_operator {
            return Err(invalid(format!(
                "{:?} does not support {:?}",
                rule.field, rule.operator
            )));
        }
        if rule.field.is_numeric() && !rule.value.trim().parse::<f64>().is_ok_and(f64::is_finite) {
            return Err(invalid(format!(
                "{:?} requires a finite number",
                rule.field
            )));
        }
    }
    Ok(())
}

impl GridFilterField {
    fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::StatusCode | Self::Depth | Self::WordCount | Self::ResponseTimeMs
        )
    }

    fn column(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::FinalUrl => "final_url",
            Self::Title => "title",
            Self::MetaDescription => "meta_description",
            Self::Canonical => "canonical",
            Self::StatusCode => "status_code",
            Self::Depth => "depth",
            Self::WordCount => "word_count",
            Self::ResponseTimeMs => "response_time_ms",
            Self::Indexability => "indexability",
        }
    }

    fn text(self, row: &CrawlRecord) -> &str {
        match self {
            Self::Url => &row.url,
            Self::FinalUrl => &row.final_url,
            Self::Title => row.title.as_deref().unwrap_or_default(),
            Self::MetaDescription => row.meta_description.as_deref().unwrap_or_default(),
            Self::Canonical => row.canonical.as_deref().unwrap_or_default(),
            Self::Indexability => &row.indexability,
            _ => "",
        }
    }

    fn number(self, row: &CrawlRecord) -> Option<u64> {
        match self {
            Self::StatusCode => row.status_code.map(u64::from),
            Self::Depth => Some(row.depth as u64),
            Self::WordCount => Some(row.word_count as u64),
            Self::ResponseTimeMs => Some(row.response_time_ms),
            _ => None,
        }
    }
}

struct PreparedGridRule<'a> {
    rule: &'a GridFilterRule,
    text: String,
    number: Option<GridFilterNumber>,
}

#[derive(Clone, Copy)]
enum GridFilterNumber {
    Integer(i64),
    Real(f64),
}

impl<'a> PreparedGridRule<'a> {
    fn new(rule: &'a GridFilterRule) -> Self {
        Self {
            rule,
            text: normalize_text_key(&rule.value),
            number: rule
                .value
                .trim()
                .parse()
                .map(GridFilterNumber::Integer)
                .or_else(|_| rule.value.trim().parse().map(GridFilterNumber::Real))
                .ok(),
        }
    }

    fn matches(&self, row: &CrawlRecord) -> bool {
        use GridFilterOperator::*;
        if self.rule.field.is_numeric() {
            let Some(value) = self.rule.field.number(row) else {
                return false;
            };
            let Some(number) = self.number else {
                return false;
            };
            // Compare an integer against a floating-point threshold without rounding the
            // stored integer first, matching SQLite even above f64's exact-integer range.
            let ordering = match number {
                GridFilterNumber::Integer(number) => i128::from(value).cmp(&i128::from(number)),
                GridFilterNumber::Real(number) if number < 0.0 => Ordering::Greater,
                GridFilterNumber::Real(number) if number >= u64::MAX as f64 => Ordering::Less,
                GridFilterNumber::Real(number) => value.cmp(&(number as u64)).then_with(|| {
                    if number.fract() == 0.0 {
                        Ordering::Equal
                    } else {
                        Ordering::Less
                    }
                }),
            };
            match self.rule.operator {
                Equals => ordering == Ordering::Equal,
                NotEquals => ordering != Ordering::Equal,
                LessThan => ordering == Ordering::Less,
                GreaterThan => ordering == Ordering::Greater,
                _ => false,
            }
        } else {
            let text = normalize_text_key(self.rule.field.text(row));
            match self.rule.operator {
                Contains => text.contains(&self.text),
                NotContains => !text.contains(&self.text),
                Equals => text == self.text,
                NotEquals => text != self.text,
                IsEmpty => text.is_empty(),
                IsNotEmpty => !text.is_empty(),
                _ => false,
            }
        }
    }
}

fn grid_filter_group_sql(group: &GridFilterGroup, args: &mut Vec<String>) -> Option<String> {
    use GridFilterOperator::*;
    if group.rules.is_empty() {
        return None;
    }
    let clauses = group
        .rules
        .iter()
        .map(|rule| {
            let column = rule.field.column();
            if matches!(rule.operator, IsEmpty | IsNotEmpty) {
                let operator = if rule.operator == IsEmpty { "=" } else { "!=" };
                return format!("ff_text_key({column}) {operator} ''");
            }
            let parameter = format!("?{}", args.len() + 1);
            args.push(if rule.field.is_numeric() {
                rule.value.trim().to_string()
            } else {
                normalize_text_key(&rule.value)
            });
            let operator = match rule.operator {
                Equals => "=",
                NotEquals => "!=",
                LessThan => "<",
                GreaterThan => ">",
                Contains | NotContains => {
                    let compare = if rule.operator == Contains {
                        "> 0"
                    } else {
                        "= 0"
                    };
                    return format!("instr(ff_text_key({column}), {parameter}) {compare}");
                }
                _ => unreachable!("empty operators are handled above"),
            };
            if rule.field.is_numeric() {
                let kind = if rule.value.trim().parse::<i64>().is_ok() {
                    "INTEGER"
                } else {
                    "REAL"
                };
                format!("{column} {operator} CAST({parameter} AS {kind})")
            } else {
                format!("ff_text_key({column}) {operator} {parameter}")
            }
        })
        .collect::<Vec<_>>();
    let joiner = match group.match_mode {
        GridFilterMatch::All => " AND ",
        GridFilterMatch::Any => " OR ",
    };
    Some(format!("({})", clauses.join(joiner)))
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

// ponytail: build narrow alias joins per query; persist indexed aliases if repeated large audits dominate.
const HREFLANG_AUDIT_CTES: &str = "WITH
    ff_record_aliases AS MATERIALIZED (
        SELECT r.id AS record_id, a.value AS alias
        FROM crawl_records r, json_each(ff_url_aliases(r.storage_key, r.url, r.final_url)) a
    ),
    ff_hreflang_refs AS MATERIALIZED (
        SELECT r.id AS source_id, json_extract(a.value, '$[0]') AS link_id,
            json_extract(a.value, '$[1]') AS alias
        FROM crawl_records r, json_each(ff_hreflang_aliases(r.hreflang_links)) a
    ),
    ff_primary_aliases AS MATERIALIZED (
        SELECT alias, MIN(record_id) AS record_id FROM ff_record_aliases GROUP BY alias
    ),
    ff_hreflang_targets AS (
        SELECT refs.source_id, refs.link_id, MIN(target.record_id) AS target_id,
            MAX(source.record_id IS NOT NULL) AS self_reference
        FROM ff_hreflang_refs refs JOIN ff_primary_aliases target ON target.alias = refs.alias
        LEFT JOIN ff_record_aliases source ON source.record_id = refs.source_id AND source.alias = refs.alias
        GROUP BY refs.source_id, refs.link_id
    ) ";

fn query_filter_sql(query: &GridQuery) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut args = Vec::new();

    if is_html_audit_view(&query.view) {
        clauses.push(SUCCESS_HTML_SQL.to_string());
    }
    let duplicate_column = match query.view {
        IssueView::TitleDuplicate => Some("title"),
        IssueView::MetaDuplicate => Some("meta_description"),
        IssueView::H1Duplicate => Some("h1"),
        IssueView::H2Duplicate => Some("h2"),
        _ => None,
    };
    if let Some(column) = duplicate_column {
        clauses.push(format!(
            "ff_text_key({column}) IN (
            SELECT ff_text_key({column}) FROM crawl_records
            WHERE {SUCCESS_HTML_SQL} AND ff_text_key({column}) != ''
            GROUP BY ff_text_key({column}) HAVING COUNT(*) > 1
        )"
        ));
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
        IssueView::TitleMissing => clauses.push("(title IS NULL OR ff_trim(title) = '')".to_string()),
        IssueView::TitleDuplicate => {}
        IssueView::TitleMultiple => clauses.push("title_count > 1".to_string()),
        IssueView::TitleTooShort => clauses.push("(title IS NOT NULL AND ff_trim(title) != '' AND title_len < 30)".to_string()),
        IssueView::TitleTooLong => clauses.push("title_len > 60".to_string()),
        IssueView::TitlePixelTooNarrow => clauses.push("(title IS NOT NULL AND ff_trim(title) != '' AND title_pixel_width < 200)".to_string()),
        IssueView::TitlePixelTooWide => clauses.push("title_pixel_width > 580".to_string()),
        IssueView::MetaMissing => clauses.push("(meta_description IS NULL OR ff_trim(meta_description) = '')".to_string()),
        IssueView::MetaDuplicate => {}
        IssueView::MetaMultiple => clauses.push("meta_description_count > 1".to_string()),
        IssueView::MetaTooShort => clauses.push("(meta_description IS NOT NULL AND ff_trim(meta_description) != '' AND meta_description_len < 70)".to_string()),
        IssueView::MetaTooLong => clauses.push("meta_description_len > 160".to_string()),
        IssueView::MetaPixelTooNarrow => clauses.push("(meta_description IS NOT NULL AND ff_trim(meta_description) != '' AND meta_description_pixel_width < 400)".to_string()),
        IssueView::MetaPixelTooWide => clauses.push("meta_description_pixel_width > 920".to_string()),
        IssueView::H1Missing => clauses.push("(h1 IS NULL OR ff_trim(h1) = '')".to_string()),
        IssueView::H1Duplicate => {}
        IssueView::H1TooLong => clauses.push("h1_len > 70".to_string()),
        IssueView::H2Missing => clauses.push("(h2 IS NULL OR ff_trim(h2) = '')".to_string()),
        IssueView::H2Duplicate => {}
        IssueView::H2TooLong => clauses.push("h2_len > 70".to_string()),
        IssueView::TitleSameAsH1 => clauses.push(
            "(title IS NOT NULL AND h1 IS NOT NULL AND ff_trim(title) != '' AND lower(ff_trim(title)) = lower(ff_trim(h1)))"
                .to_string(),
        ),
        IssueView::CanonicalMissing => clauses.push("(canonical IS NULL OR ff_trim(canonical) = '')".to_string()),
        IssueView::CanonicalMultiple => clauses.push("canonical_count > 1".to_string()),
        IssueView::CanonicalUncrawled | IssueView::CanonicalToRedirect | IssueView::CanonicalToError
        | IssueView::CanonicalNonIndexable | IssueView::CanonicalChain | IssueView::CanonicalLoop
        | IssueView::PaginationNextToError | IssueView::PaginationPrevToError
        | IssueView::PaginationNextLoop | IssueView::PaginationPrevLoop
        | IssueView::PaginationNextNonReciprocal | IssueView::PaginationPrevNonReciprocal
        | IssueView::AmpToError => {
            let mask = reference_view_mask(&query.view).expect("reference view");
            clauses.push(format!("id IN (SELECT record_id FROM ff_reference_diagnostics WHERE (flags & {mask}) != 0)"));
        }
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
        IssueView::HreflangMissingReturnLink => clauses.push(
            "id IN (SELECT links.source_id FROM ff_hreflang_targets links
            WHERE links.self_reference = 0 AND NOT EXISTS (
                SELECT 1 FROM ff_hreflang_refs back
                JOIN ff_record_aliases source ON source.alias = back.alias
                WHERE back.source_id = links.target_id AND source.record_id = links.source_id
            ))".into(),
        ),
        IssueView::HreflangNonCanonicalTarget => clauses.push(
            "id IN (SELECT links.source_id FROM ff_hreflang_targets links
            JOIN crawl_records target ON target.id = links.target_id
            WHERE target.canonical IS NOT NULL
                AND ff_url_aliases(target.canonical, '', '') != '[]'
                AND NOT EXISTS (
                    SELECT 1 FROM ff_record_aliases a
                    JOIN json_each(ff_url_aliases(target.canonical, '', '')) c ON a.alias = c.value
                    WHERE a.record_id = target.id
                ))".into(),
        ),
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
        IssueView::ExactDuplicate => clauses.push(
            "id IN (SELECT record_id FROM ff_exact_duplicate_records)".into(),
        ),
        IssueView::BrokenLinks => clauses.push(broken_record_sql()),
        IssueView::SitemapOrphan => clauses
            .push("in_sitemap != 0 AND inlink_count = 0 AND classification = 'internal'".to_string()),
    }

    if let Some(search) = query.global_search.as_ref().map(|value| value.trim())
        && !search.is_empty()
    {
        let parameter = format!("?{}", args.len() + 1);
        let mut predicates = [
            "url",
            "final_url",
            "title",
            "meta_description",
            "meta_robots",
            "x_robots_tag",
            "h1",
            "h2",
            "canonical",
            "amphtml",
            "rel_next",
            "rel_prev",
            "response_hash",
            "status_code",
            "near_duplicate_cluster_id",
            "list_position",
            "deprecated_html_tag_count",
            "duplicate_id_count",
            "CASE WHEN js_rendered THEN 'true' ELSE 'false' END",
            "CASE WHEN rendered_dom_changed THEN 'true' ELSE 'false' END",
            "rendered_word_count_delta",
            "rendered_link_count_delta",
            "search_console_clicks",
            "search_console_impressions",
            "search_console_ctr",
            "search_console_average_position",
        ]
        .into_iter()
        .map(|column| format!("ff_contains({column}, {parameter})"))
        .collect::<Vec<_>>();
        predicates.push(format!("ff_custom_contains(custom_extractions, custom_searches, structured_data_issues, {parameter})"));
        predicates.push(sqlite_first_inlink_expression(&format!(
            "ff_contains(source_url, {parameter}) OR ff_contains(anchor_text, {parameter}) OR ff_contains(source_position, {parameter})"
        )));
        clauses.push(format!("({})", predicates.join(" OR ")));
        args.push(search.to_lowercase());
    }

    if let Some(segment) = query.segment_pattern.as_ref().map(|value| value.trim())
        && !segment.is_empty()
    {
        let pattern = if query.segment_regex {
            clauses.push("(ff_regexp(?, url) OR ff_regexp(?, final_url))".to_string());
            segment.to_string()
        } else {
            clauses.push("(ff_contains(url, ?) OR ff_contains(final_url, ?))".to_string());
            segment.to_lowercase()
        };
        args.push(pattern.clone());
        args.push(pattern);
    }

    if let Some(group) = &query.filters
        && let Some(clause) = grid_filter_group_sql(group, &mut args)
    {
        clauses.push(clause);
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
        let parameter = format!("?{}", args.len() + 1);
        let predicates = [
            "source_url",
            "target_url",
            "anchor_text",
            "rel",
            "link_type",
            "source_status_code",
            "target_status_code",
            "source_depth",
            "target_depth",
            "source_position",
            "discovery_order",
        ]
        .into_iter()
        .map(|column| format!("ff_contains({column}, {parameter})"))
        .collect::<Vec<_>>();
        clauses.push(format!("({})", predicates.join(" OR ")));
        args.push(search.to_lowercase());
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
    format!("ff_extraction_sort(custom_extractions, '{escaped_name}')")
}

fn sqlite_first_inlink_expression(column: &str) -> String {
    format!("(SELECT {column} FROM link_edges
        WHERE target_url IN (SELECT value FROM json_each(ff_url_aliases(crawl_records.storage_key, crawl_records.url, crawl_records.final_url)))
        ORDER BY discovery_order, source_position, source_url, id LIMIT 1)")
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
        Some("titleCount") => Some("title_count".to_string()),
        Some("titlePixelWidth") => Some("title_pixel_width".to_string()),
        Some("metaDescription") => Some("meta_description".to_string()),
        Some("metaDescriptionLen") => Some("meta_description_len".to_string()),
        Some("metaDescriptionCount") => Some("meta_description_count".to_string()),
        Some("metaDescriptionPixelWidth") => Some("meta_description_pixel_width".to_string()),
        Some("h1") => Some("h1".to_string()),
        Some("h1Len") => Some("h1_len".to_string()),
        Some("h1Count") => Some("h1_count".to_string()),
        Some("h2") => Some("h2".to_string()),
        Some("h2Len") => Some("h2_len".to_string()),
        Some("h2Count") => Some("h2_count".to_string()),
        Some("canonicalCount") => Some("canonical_count".to_string()),
        Some("relNext") => Some("rel_next".to_string()),
        Some("relPrev") => Some("rel_prev".to_string()),
        Some("amphtml") => Some("amphtml".to_string()),
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
        Some("firstInlinkSourceUrl") => Some(sqlite_first_inlink_expression("source_url")),
        Some("firstInlinkSourcePosition") => {
            Some(sqlite_first_inlink_expression("source_position"))
        }
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

fn crawl_audit_revision(conn: &Connection) -> Result<i64, StorageError> {
    Ok(conn.query_row(
        "SELECT revision FROM crawl_audit_revision WHERE id = 1",
        [],
        |row| row.get(0),
    )?)
}

fn summary_count(conn: &Connection, sql: &str) -> Result<usize, StorageError> {
    let value = conn.query_row(sql, [], |row| {
        row.get::<_, i64>(0).map(|count| count as usize)
    })?;
    Ok(value)
}

fn sqlite_progress_counts(conn: &Connection) -> Result<CrawlSummary, StorageError> {
    let view_filter = |view| {
        let (filter, args) = query_filter_sql(&GridQuery {
            view,
            ..GridQuery::default()
        });
        debug_assert!(args.is_empty());
        filter
    };
    // Pair each SQL projection with its decoded field so column order cannot drift.
    macro_rules! count_fields {
        ($($field:ident: $filter:expr),+ $(,)?) => {{
            let projections = [$({
                let filter = $filter;
                let count = if filter.is_empty() {
                    "COUNT(*)".to_string()
                } else {
                    format!("COUNT(*) FILTER({filter})")
                };
                format!("{count} AS {}", stringify!($field))
            }),+];
            let sql = format!("SELECT {} FROM crawl_records", projections.join(", "));
            Ok(conn.query_row(&sql, [], |row| Ok(CrawlSummary {
                $($field: row.get::<_, i64>(stringify!($field))? as usize,)+
                ..CrawlSummary::default()
            }))?)
        }};
    }
    count_fields! {
        total: view_filter(IssueView::All),
        internal: view_filter(IssueView::Internal),
        external: view_filter(IssueView::External),
        success: view_filter(IssueView::Status2xx),
        redirects: view_filter(IssueView::Status3xx),
        client_errors: view_filter(IssueView::Status4xx),
        server_errors: view_filter(IssueView::Status5xx),
        no_response: view_filter(IssueView::NoResponse),
        broken: view_filter(IssueView::BrokenLinks),
        near_duplicates: view_filter(IssueView::NearDuplicate),
        indexable: " WHERE indexability = 'Indexable'".to_string(),
        non_indexable: " WHERE indexability = 'Non-indexable'".to_string(),
        title_missing: view_filter(IssueView::TitleMissing),
        title_multiple: view_filter(IssueView::TitleMultiple),
        meta_missing: view_filter(IssueView::MetaMissing),
        meta_multiple: view_filter(IssueView::MetaMultiple),
        h1_missing: view_filter(IssueView::H1Missing),
        h2_missing: view_filter(IssueView::H2Missing),
        canonical_missing: view_filter(IssueView::CanonicalMissing),
        canonical_multiple: view_filter(IssueView::CanonicalMultiple),
        noindex: view_filter(IssueView::DirectivesNoindex),
        images_missing_alt: view_filter(IssueView::ImagesMissingAlt),
        images_alt_too_long: view_filter(IssueView::ImagesAltTooLong),
        mixed_content: view_filter(IssueView::SecurityMixedContent),
        insecure_forms: view_filter(IssueView::SecurityInsecureForms),
        hreflang_invalid: view_filter(IssueView::HreflangInvalid),
        structured_data_invalid: view_filter(IssueView::StructuredDataInvalid),
        structured_data_warnings: view_filter(IssueView::StructuredDataWarning),
        deprecated_html_tags: view_filter(IssueView::HtmlDeprecatedTags),
        duplicate_ids: view_filter(IssueView::HtmlDuplicateIds),
        rendered_dom_changed: view_filter(IssueView::RenderedDomChanged),
        missing_viewport: view_filter(IssueView::MobileMissingViewport),
        missing_hsts: view_filter(IssueView::SecurityMissingHsts),
        sitemap_orphans: view_filter(IssueView::SitemapOrphan),
    }
}

fn sqlite_duplicate_count(conn: &Connection, column: &str) -> Result<usize, StorageError> {
    let column = sqlite_identifier(column);
    summary_count(
        conn,
        &format!(
            "SELECT COALESCE(SUM(CASE WHEN text_key != '' THEN matches ELSE 0 END), 0) FROM (
        SELECT ff_text_key({column}) AS text_key, COUNT(*) AS matches FROM crawl_records
        WHERE {SUCCESS_HTML_SQL}
        GROUP BY ff_text_key({column}) HAVING COUNT(*) > 1
    )"
        ),
    )
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

/// Cross-page evidence for one source row, in the same order as the supplied records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalDiagnostics {
    pub uncrawled: bool,
    pub to_redirect: bool,
    pub to_error: bool,
    pub non_indexable: bool,
    pub chain: bool,
    /// The canonical path enters a cycle of distinct pages. Self-canonicals are valid.
    pub loop_detected: bool,
}

impl CanonicalDiagnostics {
    fn flags(self) -> u8 {
        u8::from(self.uncrawled)
            | (u8::from(self.to_redirect) << 1)
            | (u8::from(self.to_error) << 2)
            | (u8::from(self.non_indexable) << 3)
            | (u8::from(self.chain) << 4)
            | (u8::from(self.loop_detected) << 5)
    }
}

/// Canonical, pagination and AMP evidence share one target index and cache revision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReferenceDiagnostics {
    pub canonical: CanonicalDiagnostics,
    pub pagination_next_to_error: bool,
    pub pagination_prev_to_error: bool,
    pub amp_to_error: bool,
    /// The source's next-only path enters a cycle, including self-pagination.
    pub pagination_next_loop: bool,
    /// The source's previous-only path enters a cycle, including self-pagination.
    pub pagination_prev_loop: bool,
    /// An observed next page has no captured previous return, or returns to another known page.
    pub pagination_next_non_reciprocal: bool,
    /// An observed previous page has no captured next return, or returns to another known page.
    pub pagination_prev_non_reciprocal: bool,
}

impl ReferenceDiagnostics {
    fn flags(self) -> u16 {
        u16::from(self.canonical.flags())
            | (u16::from(self.pagination_next_to_error) << 6)
            | (u16::from(self.pagination_prev_to_error) << 7)
            | (u16::from(self.amp_to_error) << 8)
            | (u16::from(self.pagination_next_loop) << 9)
            | (u16::from(self.pagination_prev_loop) << 10)
            | (u16::from(self.pagination_next_non_reciprocal) << 11)
            | (u16::from(self.pagination_prev_non_reciprocal) << 12)
    }
}

#[derive(Debug)]
struct ReferenceAuditRecord {
    id: u64,
    url: String,
    final_url: String,
    canonical: Option<String>,
    rel_next: Option<String>,
    rel_prev: Option<String>,
    amphtml: Option<String>,
    eligible: bool,
    internal: bool,
    fetched: bool,
    broken: bool,
    non_indexable: bool,
    redirect: bool,
    redirect_urls: Vec<String>,
    observed: bool,
    target_error: bool,
    final_observed: bool,
}

impl From<&CrawlRecord> for ReferenceAuditRecord {
    fn from(record: &CrawlRecord) -> Self {
        Self {
            id: record.id,
            url: record.url.clone(),
            final_url: record.final_url.clone(),
            canonical: record.canonical.clone(),
            rel_next: record.rel_next.clone(),
            rel_prev: record.rel_prev.clone(),
            amphtml: record.amphtml.clone(),
            eligible: is_success_html_record(record),
            internal: record.classification == UrlClassification::Internal,
            fetched: record.status_code.is_some()
                || record.error.is_some()
                || is_robots_blocked_record(record),
            broken: is_broken_record(record),
            non_indexable: is_robots_blocked_record(record)
                || (record.indexability == "Non-indexable"
                    && !is_broken_record(record)
                    && !matches!(record.status_code, Some(300..=399))),
            redirect: matches!(record.status_code, Some(300..=399)),
            redirect_urls: record
                .redirect_chain
                .iter()
                .filter(|hop| (300..400).contains(&hop.status_code))
                .map(|hop| hop.url.clone())
                .collect(),
            observed: record.status_code.is_some() || is_no_response_record(record),
            target_error: is_no_response_record(record)
                || matches!(record.status_code, Some(400..=599))
                || (matches!(record.status_code, Some(300..=399)) && record.error.is_some()),
            // The redirect limiter records the next destination before requesting it.
            final_observed: record.error.as_deref() != Some("Redirect limit exceeded"),
        }
    }
}

fn reference_audit_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ReferenceAuditRecord> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Hop {
        url: String,
        status_code: u16,
    }
    let hops = serde_json::from_str::<Vec<Hop>>(&row.get::<_, String>(10)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(ReferenceAuditRecord {
        id: row.get::<_, i64>(0)? as u64,
        url: row.get(1)?,
        final_url: row.get(2)?,
        canonical: row.get(3)?,
        rel_next: row.get(11)?,
        rel_prev: row.get(12)?,
        amphtml: row.get(16)?,
        eligible: row.get(4)?,
        internal: row.get(5)?,
        fetched: row.get(6)?,
        broken: row.get(7)?,
        non_indexable: row.get::<_, Option<bool>>(8)?.unwrap_or(false),
        redirect: row.get(9)?,
        redirect_urls: hops
            .into_iter()
            .filter(|hop| (300..400).contains(&hop.status_code))
            .map(|hop| hop.url)
            .collect(),
        observed: row.get(13)?,
        target_error: row.get(14)?,
        final_observed: row.get(15)?,
    })
}

/// Audits complete HTML sources without cloning record payloads or walking each chain repeatedly.
pub fn canonical_diagnostics(records: &[CrawlRecord]) -> Vec<CanonicalDiagnostics> {
    reference_diagnostics(records)
        .into_iter()
        .map(|diagnostic| diagnostic.canonical)
        .collect()
}

/// Audits complete HTML sources using only known target evidence; unknown pagination/AMP targets
/// are not failures. Results preserve source order and separate List occurrences.
pub fn reference_diagnostics(records: &[CrawlRecord]) -> Vec<ReferenceDiagnostics> {
    build_reference_diagnostics(
        &records
            .iter()
            .map(ReferenceAuditRecord::from)
            .collect::<Vec<_>>(),
    )
}

type ReferenceCandidate = (bool, u8, u64, usize, bool);

struct ReferenceTarget {
    canonical: ReferenceCandidate,
    observed: Option<ReferenceCandidate>,
}

fn build_reference_diagnostics(records: &[ReferenceAuditRecord]) -> Vec<ReferenceDiagnostics> {
    // Direct request evidence outranks redirect hops, then final URL aliases. Earliest List
    // occurrences break ties; a pending occurrence must not hide an observed response.
    let mut targets = HashMap::<String, ReferenceTarget>::new();
    for (index, record) in records.iter().enumerate() {
        let mut add = |url: &str, priority: u8, redirect: bool| {
            let candidate = (!record.fetched, priority, record.id, index, redirect);
            let observed =
                (record.observed && (priority != 2 || record.final_observed)).then_some(candidate);
            for alias in url_aliases(url) {
                targets
                    .entry(alias)
                    .and_modify(|previous| {
                        if candidate < previous.canonical {
                            previous.canonical = candidate;
                        }
                        if let Some(candidate) = observed
                            && previous
                                .observed
                                .is_none_or(|previous| candidate < previous)
                        {
                            previous.observed = Some(candidate);
                        }
                    })
                    .or_insert(ReferenceTarget {
                        canonical: candidate,
                        observed,
                    });
            }
        };
        add(
            &record.url,
            0,
            record.redirect || !record.redirect_urls.is_empty(),
        );
        for url in &record.redirect_urls {
            add(url, 1, true);
        }
        add(&record.final_url, 2, record.redirect);
    }
    let canonical = build_canonical_diagnostics(records, &targets);
    let mut next = vec![None; records.len()];
    let mut prev = vec![None; records.len()];
    let mut diagnostics = records
        .iter()
        .zip(canonical)
        .enumerate()
        .map(|(source_index, (record, canonical))| {
            let mut diagnostic = ReferenceDiagnostics {
                canonical,
                ..ReferenceDiagnostics::default()
            };
            if !record.eligible
                || (record.rel_next.is_none()
                    && record.rel_prev.is_none()
                    && record.amphtml.is_none())
            {
                return diagnostic;
            }
            // A successful source proves its own route and final page work even when an older
            // List occurrence failed. Recorded redirect hops are part of that successful route.
            let source_aliases = url_aliases_many(
                [record.url.as_str(), record.final_url.as_str()]
                    .into_iter()
                    .chain(record.redirect_urls.iter().map(String::as_str)),
            );
            let observed_target = |value: Option<&str>| {
                let target = value.and_then(normalized_final_url)?;
                let aliases = url_aliases(&target);
                if aliases_overlap(&aliases, &source_aliases) {
                    return Some(source_index);
                }
                aliases
                    .iter()
                    .filter_map(|alias| targets.get(alias).and_then(|target| target.observed))
                    .min()
                    .map(|(_, _, _, index, _)| index)
            };
            let next_target = observed_target(record.rel_next.as_deref());
            let prev_target = observed_target(record.rel_prev.as_deref());
            let target_has_error =
                |target: Option<usize>| target.is_some_and(|index| records[index].target_error);
            diagnostic.pagination_next_to_error = target_has_error(next_target);
            diagnostic.pagination_prev_to_error = target_has_error(prev_target);
            diagnostic.amp_to_error = target_has_error(observed_target(record.amphtml.as_deref()));
            let source_final = normalized_final_url(&record.final_url);
            let loop_target = |target: Option<usize>| {
                target
                    .filter(|&index| records[index].eligible)
                    .map(|index| {
                        // A distinct request route returning to this final page is self-pagination,
                        // regardless of whether that route's List occurrence has another relation.
                        if source_final.is_some()
                            && source_final == normalized_final_url(&records[index].final_url)
                        {
                            source_index
                        } else {
                            index
                        }
                    })
            };
            next[source_index] = loop_target(next_target);
            prev[source_index] = loop_target(prev_target);
            diagnostic
        })
        .collect::<Vec<_>>();
    for (source_index, ((diagnostic, next_loop), prev_loop)) in diagnostics
        .iter_mut()
        .zip(paths_entering_cycles(&next))
        .zip(paths_entering_cycles(&prev))
        .enumerate()
    {
        diagnostic.pagination_next_loop = next_loop;
        diagnostic.pagination_prev_loop = prev_loop;
        if next[source_index].is_none_or(|index| index == source_index)
            && prev[source_index].is_none_or(|index| index == source_index)
        {
            continue;
        }
        let source = &records[source_index];
        let source_final = normalized_final_url(&source.final_url);
        let source_aliases = url_aliases_many(
            [source.url.as_str(), source.final_url.as_str()]
                .into_iter()
                .chain(source.redirect_urls.iter().map(String::as_str)),
        );
        let non_reciprocal = |target: Option<usize>, return_is_prev: bool| {
            // The existing edges contain only complete HTML targets, resolved in each source's
            // own context. Self-pagination is already reported by the direction-specific loops.
            let Some(target_index) = target.filter(|&index| index != source_index) else {
                return false;
            };
            let target = &records[target_index];
            let (return_url, return_target) = if return_is_prev {
                (target.rel_prev.as_deref(), prev[target_index])
            } else {
                (target.rel_next.as_deref(), next[target_index])
            };
            let Some(return_url) = return_url.filter(|value| !value.trim().is_empty()) else {
                return true;
            };
            let Some(return_url) = normalized_final_url(return_url) else {
                return false;
            };
            // Current source evidence outranks an older failed List occurrence of its route.
            if aliases_overlap(&url_aliases(&return_url), &source_aliases) {
                return false;
            }
            // A different URL may redirect back to this page. Only a known complete return
            // destination supports a mismatch; robots, errors and uncrawled returns stay unknown.
            match (
                source_final.as_ref(),
                return_target.and_then(|index| normalized_final_url(&records[index].final_url)),
            ) {
                (Some(source), Some(returned)) => source != &returned,
                _ => false,
            }
        };
        diagnostic.pagination_next_non_reciprocal = non_reciprocal(next[source_index], true);
        diagnostic.pagination_prev_non_reciprocal = non_reciprocal(prev[source_index], false);
    }
    diagnostics
}

fn build_canonical_diagnostics(
    records: &[ReferenceAuditRecord],
    targets: &HashMap<String, ReferenceTarget>,
) -> Vec<CanonicalDiagnostics> {
    let canonical_urls = records
        .iter()
        .map(|record| {
            record
                .canonical
                .as_deref()
                .and_then(|value| url::Url::parse(value.trim()).ok())
                .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        })
        .collect::<Vec<_>>();
    let has_next = records
        .iter()
        .zip(&canonical_urls)
        .map(|(record, canonical)| {
            record.eligible
                && canonical.as_ref().is_some_and(|canonical| {
                    !aliases_overlap(
                        &url_aliases(canonical.as_str()),
                        &url_aliases_many([record.url.as_str(), record.final_url.as_str()]),
                    )
                })
        })
        .collect::<Vec<_>>();
    let mut diagnostics = vec![CanonicalDiagnostics::default(); records.len()];
    let mut next = vec![None; records.len()];
    for (index, record) in records.iter().enumerate() {
        let Some(canonical) = canonical_urls[index].as_ref().filter(|_| record.eligible) else {
            continue;
        };
        let canonical_aliases = url_aliases(canonical.as_str());
        let target = if !has_next[index] {
            // A source's own successful response is stronger self-canonical evidence than
            // another List occurrence's old error or redirect to the same final URL.
            let redirects = record.redirect
                || record
                    .redirect_urls
                    .iter()
                    .any(|url| aliases_overlap(&canonical_aliases, &url_aliases(url)));
            Some((false, 0, record.id, index, redirects))
        } else {
            canonical_aliases
                .iter()
                .filter_map(|alias| targets.get(alias).map(|target| target.canonical))
                .min()
        };
        let result = &mut diagnostics[index];
        if target.is_none_or(|candidate| candidate.0) {
            // Storage does not own crawl scope; only same-host internal targets are definite
            // candidates for "uncrawled". Unknown subdomains/external hosts stay unclassified.
            result.uncrawled = record.internal
                && url::Url::parse(&record.final_url).is_ok_and(|source| {
                    source.host_str().is_some() && source.host_str() == canonical.host_str()
                });
            continue;
        }
        let (_, _, _, target_index, redirects) = target.expect("observed target");
        let target = &records[target_index];
        result.to_redirect = redirects;
        result.to_error = target.broken;
        result.non_indexable = target.non_indexable;
        let same_page = aliases_overlap(
            &url_aliases(&record.final_url),
            &url_aliases(&target.final_url),
        );
        if has_next[index] && !same_page && target.eligible {
            next[index] = Some(target_index);
            result.chain = has_next[target_index];
        }
    }
    for (diagnostic, loops) in diagnostics.iter_mut().zip(paths_entering_cycles(&next)) {
        diagnostic.loop_detected = loops;
    }
    diagnostics
}

// Each node has at most one edge. Memoize whether its path enters a cycle in linear time,
// without recursion or a depth limit. Callers decide whether self-edges are valid relations.
fn paths_entering_cycles(next: &[Option<usize>]) -> Vec<bool> {
    let mut state = vec![0u8; next.len()];
    let mut enters_cycle = vec![false; next.len()];
    let mut path = Vec::new();
    for start in 0..next.len() {
        if state[start] != 0 {
            continue;
        }
        path.clear();
        let mut current = Some(start);
        while let Some(index) = current {
            if state[index] != 0 {
                break;
            }
            state[index] = 1;
            path.push(index);
            current = next[index];
        }
        let loops = current.is_some_and(|index| state[index] == 1 || enters_cycle[index]);
        for &index in &path {
            state[index] = 2;
            enters_cycle[index] = loops;
        }
    }
    enters_cycle
}

fn reference_view_mask(view: &IssueView) -> Option<u16> {
    Some(match view {
        IssueView::CanonicalUncrawled => 1,
        IssueView::CanonicalToRedirect => 2,
        IssueView::CanonicalToError => 4,
        IssueView::CanonicalNonIndexable => 8,
        IssueView::CanonicalChain => 16,
        IssueView::CanonicalLoop => 32,
        IssueView::PaginationNextToError => 64,
        IssueView::PaginationPrevToError => 128,
        IssueView::AmpToError => 256,
        IssueView::PaginationNextLoop => 512,
        IssueView::PaginationPrevLoop => 1024,
        IssueView::PaginationNextNonReciprocal => 2048,
        IssueView::PaginationPrevNonReciprocal => 4096,
        _ => return None,
    })
}

fn reference_counts(diagnostics: &[ReferenceDiagnostics]) -> [usize; 13] {
    let mut counts = [0; 13];
    for diagnostic in diagnostics {
        for (index, count) in counts.iter_mut().enumerate() {
            *count += usize::from(diagnostic.flags() & (1 << index) != 0);
        }
    }
    counts
}

fn set_reference_summary(summary: &mut CrawlSummary, counts: [usize; 13]) {
    [
        summary.canonical_uncrawled,
        summary.canonical_to_redirect,
        summary.canonical_to_error,
        summary.canonical_non_indexable,
        summary.canonical_chain,
        summary.canonical_loop,
        summary.pagination_next_to_error,
        summary.pagination_prev_to_error,
        summary.amp_to_error,
        summary.pagination_next_loop,
        summary.pagination_prev_loop,
        summary.pagination_next_non_reciprocal,
        summary.pagination_prev_non_reciprocal,
    ] = counts;
}

fn add_reference_summary(summary: &mut CrawlSummary, diagnostics: &[ReferenceDiagnostics]) {
    set_reference_summary(summary, reference_counts(diagnostics));
}

#[derive(Clone, Debug)]
struct HreflangAuditRecord {
    id: u64,
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
                id: record.id,
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
            .filter_map(|alias| self.records_by_alias.get(&alias))
            .min_by_key(|record| record.id)
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
    exact_hashes: &HashSet<String>,
    hreflang_index: &HreflangAuditIndex,
    references: &ReferenceDiagnostics,
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
        IssueView::TitleMultiple => row.title_count.is_some_and(|count| count > 1),
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
        IssueView::MetaMultiple => row.meta_description_count.is_some_and(|count| count > 1),
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
        IssueView::CanonicalUncrawled => references.canonical.uncrawled,
        IssueView::CanonicalToRedirect => references.canonical.to_redirect,
        IssueView::CanonicalToError => references.canonical.to_error,
        IssueView::CanonicalNonIndexable => references.canonical.non_indexable,
        IssueView::CanonicalChain => references.canonical.chain,
        IssueView::CanonicalLoop => references.canonical.loop_detected,
        IssueView::PaginationNextToError => references.pagination_next_to_error,
        IssueView::PaginationPrevToError => references.pagination_prev_to_error,
        IssueView::PaginationNextLoop => references.pagination_next_loop,
        IssueView::PaginationPrevLoop => references.pagination_prev_loop,
        IssueView::PaginationNextNonReciprocal => references.pagination_next_non_reciprocal,
        IssueView::PaginationPrevNonReciprocal => references.pagination_prev_non_reciprocal,
        IssueView::AmpToError => references.amp_to_error,
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
        IssueView::ExactDuplicate => is_exact_duplicate_record(row, exact_hashes),
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
            | IssueView::TitleMultiple
            | IssueView::TitleTooShort
            | IssueView::TitleTooLong
            | IssueView::TitlePixelTooNarrow
            | IssueView::TitlePixelTooWide
            | IssueView::MetaMissing
            | IssueView::MetaDuplicate
            | IssueView::MetaMultiple
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
            | IssueView::CanonicalUncrawled
            | IssueView::CanonicalToRedirect
            | IssueView::CanonicalToError
            | IssueView::CanonicalNonIndexable
            | IssueView::CanonicalChain
            | IssueView::CanonicalLoop
            | IssueView::PaginationNextToError
            | IssueView::PaginationPrevToError
            | IssueView::PaginationNextLoop
            | IssueView::PaginationPrevLoop
            | IssueView::PaginationNextNonReciprocal
            | IssueView::PaginationPrevNonReciprocal
            | IssueView::AmpToError
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
            | IssueView::ExactDuplicate
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

fn normalized_final_url(value: &str) -> Option<String> {
    let mut url = url::Url::parse(value.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

/// Hashes shared by complete HTML responses at two or more distinct final URLs.
/// The hash covers downloaded response bytes, before text selection or rendering.
pub fn exact_duplicate_hashes(records: &[CrawlRecord]) -> HashSet<String> {
    let mut groups = HashMap::<&str, (String, bool)>::new();
    for record in records.iter().filter(|row| is_success_html_record(row)) {
        let Some(hash) = record
            .response_hash
            .as_deref()
            .filter(|hash| !hash.trim().is_empty())
        else {
            continue;
        };
        let Some(identity) = normalized_final_url(&record.final_url) else {
            continue;
        };
        match groups.entry(hash) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((identity, false));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let (first, distinct) = entry.get_mut();
                *distinct |= first != &identity;
            }
        }
    }
    groups
        .into_iter()
        .filter_map(|(hash, (_, distinct))| distinct.then(|| hash.to_string()))
        .collect()
}

pub fn is_exact_duplicate_record(record: &CrawlRecord, hashes: &HashSet<String>) -> bool {
    is_success_html_record(record)
        && record
            .response_hash
            .as_ref()
            .is_some_and(|hash| hashes.contains(hash))
        && normalized_final_url(&record.final_url).is_some()
}

pub fn is_success_html_record(row: &CrawlRecord) -> bool {
    is_success_record(row)
        && row.indexability_status != "Response body incomplete"
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
        || custom_data_matches_search(
            &row.custom_extractions,
            &row.custom_searches,
            &row.structured_data_issues,
            search,
        )
}

fn custom_data_matches_search(
    extractions: &[CustomExtractionValue],
    searches: &[CustomSearchValue],
    issues: &[StructuredDataIssue],
    search: &str,
) -> bool {
    extractions.iter().any(|extraction| {
        extraction.name.to_lowercase().contains(search)
            || extraction
                .values
                .iter()
                .any(|value| value.to_lowercase().contains(search))
    }) || searches.iter().any(|custom_search| {
        custom_search.name.to_lowercase().contains(search)
            || custom_search.match_count.to_string().contains(search)
            || custom_search
                .snippets
                .iter()
                .any(|value| value.to_lowercase().contains(search))
    }) || issues.iter().any(|issue| {
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
        return custom_extraction_sort_value(&left.custom_extractions, &name).cmp(
            &custom_extraction_sort_value(&right.custom_extractions, &name),
        );
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
        "titleCount" => left.title_count.cmp(&right.title_count),
        "titlePixelWidth" => left.title_pixel_width.cmp(&right.title_pixel_width),
        "metaDescription" => left.meta_description.cmp(&right.meta_description),
        "metaDescriptionLen" => left.meta_description_len.cmp(&right.meta_description_len),
        "metaDescriptionCount" => left
            .meta_description_count
            .cmp(&right.meta_description_count),
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
        "relNext" => left.rel_next.cmp(&right.rel_next),
        "relPrev" => left.rel_prev.cmp(&right.rel_prev),
        "amphtml" => left.amphtml.cmp(&right.amphtml),
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

fn custom_extraction_sort_value(extractions: &[CustomExtractionValue], name: &str) -> String {
    extractions
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

// Record aliases are cached by evidence revision; image references stay live.
// No crawl records or off-page image strings are decoded into Rust.
const IMAGE_ASSET_SIZE_CTE: &str = "WITH annotated_images AS (
    SELECT ia.*, (
        SELECT sizes.size_bytes
        FROM json_each(ff_url_aliases(ia.image_url, '', '')) refs
        JOIN ff_image_record_aliases sizes ON sizes.alias = refs.value
        ORDER BY sizes.record_id DESC LIMIT 1
    ) AS size_bytes FROM image_assets ia
)";

fn image_asset_filter_sql(query: &ImageAssetQuery) -> Result<(String, Vec<String>), StorageError> {
    let mut clauses = Vec::new();
    let mut args = Vec::new();
    if let Some(page_url) = query
        .page_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        clauses.push(
            "EXISTS (SELECT 1 FROM json_each(ff_url_aliases(page_url, '', '')) stored
            JOIN json_each(?) wanted ON stored.value = wanted.value)"
                .to_string(),
        );
        args.push(serde_json::to_string(&sorted_aliases(url_aliases(
            page_url,
        )))?);
    }
    if query.missing_alt_only {
        clauses.push("missing_alt != 0".into());
    }
    if query.oversized_only {
        clauses.push(format!("size_bytes > {IMAGE_ASSET_OVERSIZE_BYTES}"));
    }
    if let Some(search) = query
        .global_search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        clauses.push(
            "(ff_contains(page_url, ?) OR ff_contains(image_url, ?) OR ff_contains(alt_text, ?))"
                .into(),
        );
        args.extend(std::iter::repeat_n(search.to_lowercase(), 3));
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    Ok((where_clause, args))
}

fn image_asset_sort_column(sort_by: Option<&str>) -> &'static str {
    match sort_by {
        None | Some("pageUrl") => "page_url",
        Some("imageUrl") => "image_url",
        Some("altText") => "alt_text",
        Some("altLen") => "alt_len",
        Some("missingAlt") => "missing_alt",
        Some("altTooLong") => "alt_too_long",
        Some("width") => "width",
        Some("height") => "height",
        Some("sourcePosition") => "source_position",
        Some("sizeBytes") => "size_bytes",
        Some("oversized") => "COALESCE(size_bytes > 204800, 0)",
        _ => "id",
    }
}

fn image_size_by_alias(records: &[CrawlRecord]) -> HashMap<String, (u64, u64)> {
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
            sizes.insert(alias, (record.id, record.size_bytes as u64));
        }
    }
    sizes
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
mod canonical_tests;

#[cfg(test)]
mod pagination_tests;

#[cfg(test)]
mod pagespeed_tests;

#[cfg(test)]
mod amp_tests;

#[cfg(test)]
mod multiple_metadata_tests;

#[cfg(test)]
mod grid_filter_tests;

#[cfg(test)]
mod exact_duplicate_tests;

#[cfg(test)]
mod sitemap_validation_tests;

#[cfg(test)]
mod graph_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_ingestion_lookups_do_not_scan_unrelated_records() {
        for row_count in [50, 500] {
            let store = MemoryStore::new();
            for index in 0..row_count {
                let mut record =
                    CrawlRecord::pending(format!("https://example.com/page/{index}"), index);
                record.status_code = Some(200);
                store.upsert(record);
            }
            let target = format!("https://example.com/page/{}", row_count - 1);
            URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
            let edge = store.add_link_edge(test_edge(
                "https://example.com/not-stored-yet",
                &format!("{target}#section"),
                LinkType::Internal,
            ));
            let edge_work = URL_ALIAS_EXPANSIONS.with(|count| count.replace(0));
            store.add_inlink(&target);
            let inlink_work = URL_ALIAS_EXPANSIONS.with(|count| count.get());
            eprintln!(
                "Memory lookup: {row_count} records, {edge_work} edge and {inlink_work} inlink alias expansions"
            );
            assert_eq!(edge.source_status_code, None);
            assert_eq!(edge.target_status_code, Some(200));
            assert_eq!(edge.target_depth, Some(row_count - 1));
            assert_eq!(store.records().last().unwrap().inlink_count, 1);
            assert!(
                edge_work <= 4 && inlink_work <= 8,
                "Lookup scanned {row_count} records: {edge_work} edge and {inlink_work} inlink alias expansions"
            );
        }
    }

    #[test]
    fn memory_progress_summary_does_not_annotate_link_sources() {
        let store = ActiveStore::memory();
        for path in ["source", "target"] {
            let mut record = CrawlRecord::pending(format!("https://example.com/{path}"), 1);
            record.status_code = Some(200);
            record.content_type = Some("text/html".into());
            record.title = Some("Shared title".into());
            record.canonical = Some("https://example.com/uncrawled".into());
            record.in_sitemap = true;
            store.upsert(record);
        }
        store.add_link_edge(test_edge(
            "https://example.com/source",
            "https://example.com/target",
            LinkType::Internal,
        ));
        store.add_inlink("https://example.com/target");
        let expected = summarize_without_canonicals(&store.records());
        URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
        let summary = store.progress_summary();
        let alias_work = URL_ALIAS_EXPANSIONS.with(|count| count.get());
        assert_eq!(
            serde_json::to_value(&summary).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(summary.title_duplicate, 2);
        assert_eq!(summary.sitemap_orphans, 1);
        assert_eq!(alias_work, 0, "Progress rebuilt URL/source annotations");
    }

    #[test]
    fn memory_alias_status_lookup_preserves_sqlite_precedence_and_updates() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            let mut first = CrawlRecord::pending("https://example.com/original".into(), 1);
            first.storage_key = "list:1:https://example.com/original".into();
            first.final_url = "https://example.com/shared".into();
            first.status_code = Some(301);
            let mut first = store.upsert(first);
            let original_id = first.id;
            for (key, depth, status) in [
                ("https://example.com/shared", 2, 404),
                ("list:3:https://example.com/shared", 3, 503),
            ] {
                let mut record = CrawlRecord::pending("https://example.com/shared".into(), depth);
                record.storage_key = key.into();
                record.status_code = Some(status);
                store.upsert(record);
            }
            let edge = store.add_link_edge(test_edge(
                "https://example.com/shared#fragment",
                "https://example.com/shared",
                LinkType::Internal,
            ));
            assert_eq!(edge.source_status_code, Some(301));
            assert_eq!(edge.source_depth, 1);
            assert_eq!(edge.target_status_code, Some(301));
            assert_eq!(edge.target_depth, Some(1));

            first.url = "https://example.com/replacement".into();
            first.final_url = "https://example.com/".into();
            first.status_code = Some(200);
            first.depth = 4;
            assert_eq!(store.upsert(first.clone()).id, original_id);
            for source in [&first.url, &first.storage_key] {
                let edge = store.add_link_edge(test_edge(
                    source,
                    "https://example.com#fragment",
                    LinkType::Internal,
                ));
                assert_eq!(edge.source_status_code, Some(200));
                assert_eq!(edge.source_depth, 4);
                assert_eq!(edge.target_status_code, Some(200));
                assert_eq!(edge.target_depth, Some(4));
            }
            let edge = store.add_link_edge(test_edge(
                "https://example.com/shared",
                "https://example.com/shared#fragment",
                LinkType::Internal,
            ));
            assert_eq!(edge.source_status_code, Some(404));
            assert_eq!(edge.source_depth, 2);
            assert_eq!(edge.target_status_code, Some(404));
            assert_eq!(edge.target_depth, Some(2));

            let mut missing = test_edge(
                "https://example.com/original",
                "https://example.com/unknown",
                LinkType::Internal,
            );
            missing.source_status_code = Some(202);
            missing.source_depth = 9;
            missing.target_status_code = Some(418);
            missing.target_depth = Some(10);
            let edge = store.add_link_edge(missing);
            assert_eq!(edge.source_status_code, Some(202));
            assert_eq!(edge.source_depth, 9);
            assert_eq!(edge.target_status_code, Some(418));
            assert_eq!(edge.target_depth, Some(10));

            first.final_url = "https://example.com/shared".into();
            first.status_code = Some(201);
            first.depth = 5;
            store.upsert(first);
            let edge = store.add_link_edge(test_edge(
                "https://example.com/",
                "https://example.com/shared",
                LinkType::Internal,
            ));
            assert_eq!(edge.source_status_code, None);
            assert_eq!(edge.target_status_code, Some(201));
            assert_eq!(edge.target_depth, Some(5));
        }
    }

    #[test]
    fn memory_alias_lookup_normalizes_stored_urls() {
        let store = MemoryStore::new();
        let mut record = CrawlRecord::pending("HTTPS://EXAMPLE.COM/#section".into(), 2);
        record.status_code = Some(200);
        store.upsert(record);
        let edge = store.add_link_edge(test_edge(
            "https://example.com",
            "https://example.com/#different-fragment",
            LinkType::Internal,
        ));
        assert_eq!(edge.source_status_code, Some(200));
        assert_eq!(edge.source_depth, 2);
        assert_eq!(edge.target_status_code, Some(200));
        assert_eq!(edge.target_depth, Some(2));
        store.add_inlink("https://example.com/");
        assert_eq!(store.records()[0].inlink_count, 1);
    }

    #[test]
    fn memory_alias_inlinks_preserve_sqlite_duplicates_replacement_and_clear() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            store.add_inlink("https://example.com");
            let mut first = CrawlRecord::pending("https://example.com/".into(), 1);
            first.storage_key = "list:1:https://example.com/".into();
            first.status_code = Some(200);
            let mut first = store.upsert(first);
            let mut second = first.clone();
            second.storage_key = "list:2:https://example.com/".into();
            store.upsert(second);
            store.add_inlink("https://example.com/");
            assert!(
                store
                    .records()
                    .iter()
                    .all(|record| record.inlink_count == 2)
            );

            first.url = "https://example.com/moved".into();
            first.final_url = first.url.clone();
            first.inlink_count = 0;
            store.upsert(first);
            store.add_inlink("https://example.com");
            store.add_inlink("https://example.com/moved");
            store.add_inlink("https://example.com/absent");
            let records = store.records();
            assert_eq!(records[0].inlink_count, 1);
            assert_eq!(records[1].inlink_count, 3);

            store.clear();
            store.upsert(CrawlRecord::pending(
                "https://example.com/unrelated".into(),
                0,
            ));
            assert_eq!(store.records().len(), 1);
            let edge = store.add_link_edge(test_edge(
                "list:1:https://example.com/",
                "https://example.com/",
                LinkType::Internal,
            ));
            assert_eq!(edge.source_status_code, None);
            assert_eq!(edge.target_depth, None);
            store.add_inlink("https://example.com");
            assert_eq!(store.records()[0].inlink_count, 0);
            let fresh = store.upsert(CrawlRecord::pending("https://example.com/".into(), 1));
            assert_eq!(fresh.inlink_count, 1);
        }
    }

    #[test]
    fn sqlite_sitemap_membership_updates_do_not_scan_the_crawl() {
        let mut measurements = Vec::new();
        for row_count in [500, 2_000] {
            let store = SqliteStore::in_memory().unwrap();
            for index in 0..row_count {
                store.upsert(CrawlRecord::pending(
                    format!("https://example.com/page/{index}"),
                    0,
                ));
            }
            let conn = store.connection().unwrap();
            let mut statement = conn.prepare_cached(MARK_SITEMAP_URLS_SQL).unwrap();
            assert_eq!(
                statement.execute(["https://example.com/page/250"]).unwrap(),
                1
            );
            let steps = statement.get_status(rusqlite::StatementStatus::VmStep);
            eprintln!("Sitemap membership update: {row_count} rows, {steps} VM steps");
            measurements.push(steps);
        }
        assert!(
            measurements.iter().all(|steps| *steps < 300),
            "One sitemap membership update scanned the crawl: {measurements:?} VM steps"
        );
        assert!(
            measurements[1] <= measurements[0] + 10,
            "Sitemap membership update cost grew with the crawl: {measurements:?}"
        );
    }

    #[test]
    fn sitemap_discovery_updates_existing_records_in_both_stores() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            for (path, target) in [
                ("original", "target"),
                ("alias", "target"),
                ("other", "other"),
            ] {
                let mut record = CrawlRecord::pending(format!("https://example.com/{path}"), 0);
                record.final_url = format!("https://example.com/{target}");
                store.upsert(record);
            }
            assert_eq!(store.summary().sitemap_orphans, 0);
            let urls = [
                "https://example.com/target".to_string(),
                "https://example.com/unknown".to_string(),
            ];
            store.mark_sitemap_urls(&urls);
            store.mark_sitemap_urls(&urls);
            store.mark_sitemap_urls(&[]);
            assert_eq!(store.summary().sitemap_orphans, 2);
            let rows = store.query(GridQuery::default()).rows;
            assert_eq!(
                rows.len(),
                3,
                "Sitemap metadata must not create or merge URL records"
            );
            assert_eq!(rows.iter().filter(|row| row.in_sitemap).count(), 2);
            store.mark_sitemap_urls(&["https://example.com/other".to_string()]);
            assert_eq!(store.summary().sitemap_orphans, 3);
        }
    }

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
        let mut incomplete = empty.clone();
        incomplete.url = "https://example.com/incomplete".to_string();
        incomplete.final_url = incomplete.url.clone();
        incomplete.storage_key = incomplete.url.clone();
        incomplete.indexability_status = "Response body incomplete".to_string();
        incomplete.error = Some("Response exceeds the configured download limit".to_string());
        store.upsert(incomplete);
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
    fn sqlite_duplicate_and_regex_queries_only_decode_the_requested_window() {
        let store = SqliteStore::in_memory().unwrap();
        let memory = MemoryStore::new();
        for index in 0..8 {
            let mut record = CrawlRecord::pending(format!("https://example.test/{index}"), 0);
            record.status_code = Some(if index == 7 { 404 } else { 200 });
            record.content_type = Some("text/html".into());
            record.title = Some(
                if index % 2 == 0 {
                    "  CAFÉ\t TITLE  "
                } else {
                    "café title"
                }
                .into(),
            );
            record.meta_description = record.title.clone();
            record.h1 = record.title.clone();
            record.h2 = record.title.clone();
            store.try_upsert(record.clone()).unwrap();
            memory.upsert(record);
        }
        // Unselected large/invalid payloads must never be decoded by a paged query.
        store.connection().unwrap().execute("UPDATE crawl_records SET response_time_ms = 'invalid-integer' WHERE url = 'https://example.test/6'", []).unwrap();
        assert!(store.try_records().is_err());
        for view in [
            IssueView::All,
            IssueView::TitleDuplicate,
            IssueView::MetaDuplicate,
            IssueView::H1Duplicate,
            IssueView::H2Duplicate,
        ] {
            for pattern in [None, Some("/[0-4]$"), Some("[")] {
                let query = GridQuery {
                    view: view.clone(),
                    segment_pattern: pattern.map(str::to_string),
                    segment_regex: pattern.is_some(),
                    offset: 1,
                    limit: 2,
                    sort_by: Some("url".into()),
                    ..GridQuery::default()
                };
                let actual = store.try_query(query.clone()).unwrap();
                let expected = memory.query(query);
                assert_eq!(actual.total, expected.total, "{view:?} {pattern:?}");
                assert_eq!(
                    actual.rows.iter().map(|row| &row.url).collect::<Vec<_>>(),
                    expected.rows.iter().map(|row| &row.url).collect::<Vec<_>>()
                );
                assert_eq!(actual.summary.title_duplicate, 7);
            }
        }
    }

    #[test]
    fn sqlite_search_and_list_order_match_memory_for_duplicate_and_regex_pages() {
        let store = SqliteStore::in_memory().unwrap();
        let memory = MemoryStore::new();
        for (index, path) in ["a_b", "axb", "100%_saved"].into_iter().enumerate() {
            let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
            record.list_position = Some([2, 1, 3][index]);
            record.status_code = Some(200);
            record.content_type = Some("text/html".into());
            record.title = Some("CAFÉ".into());
            record.meta_description = Some("Keep  spaces".into());
            record.js_rendered = index == 0;
            record.custom_extractions = vec![
                CustomExtractionValue {
                    name: "Code".into(),
                    values: vec!["ÜBER".into()],
                },
                CustomExtractionValue {
                    name: "Order".into(),
                    values: vec!["same".into(), ["z", "a", "m"][index].into()],
                },
            ];
            store.try_upsert(record.clone()).unwrap();
            memory.upsert(record);
        }
        for (position, anchor) in [(1, "INLINKONLY"), (2, "LaterOnly")] {
            let mut edge = test_edge(
                "https://example.test/source",
                "https://example.test/a_b",
                LinkType::Internal,
            );
            edge.anchor_text = anchor.into();
            edge.source_position = position;
            edge.discovery_order = u64::from(position);
            store.add_link_edge(edge.clone());
            memory.add_link_edge(edge);
        }
        for (view, regex) in [
            (IssueView::All, false),
            (IssueView::TitleDuplicate, false),
            (IssueView::All, true),
        ] {
            for search in [
                None,
                Some("café"),
                Some("a_b"),
                Some("%"),
                Some("true"),
                Some("false"),
                Some("über"),
                Some("values"),
                Some("keep  spaces"),
                Some("keep spaces"),
                Some("inlinkonly"),
                Some("lateronly"),
            ] {
                let query = GridQuery {
                    view: view.clone(),
                    global_search: search.map(str::to_string),
                    segment_pattern: regex.then(|| "example".into()),
                    segment_regex: regex,
                    limit: 2,
                    ..GridQuery::default()
                };
                let expected = memory.query(query.clone());
                let actual = store.try_query(query).unwrap();
                assert_eq!(actual.total, expected.total, "{view:?} {regex} {search:?}");
                assert_eq!(
                    actual.rows.iter().map(|row| &row.url).collect::<Vec<_>>(),
                    expected.rows.iter().map(|row| &row.url).collect::<Vec<_>>(),
                    "{view:?} {regex} {search:?}"
                );
            }
            for sort_by in [
                "custom:Order:0",
                "firstInlinkSourceUrl",
                "firstInlinkSourcePosition",
                "unknown",
            ] {
                for sort_dir in [SortDirection::Asc, SortDirection::Desc] {
                    let query = GridQuery {
                        view: view.clone(),
                        sort_by: Some(sort_by.into()),
                        sort_dir,
                        ..GridQuery::default()
                    };
                    let expected = memory.query(query.clone());
                    let actual = store.try_query(query).unwrap();
                    assert_eq!(
                        actual.rows.iter().map(|row| &row.url).collect::<Vec<_>>(),
                        expected.rows.iter().map(|row| &row.url).collect::<Vec<_>>(),
                        "{sort_by}"
                    );
                }
            }
        }
        for pattern in ["a_b", "%", "["] {
            let query = GridQuery {
                segment_pattern: Some(pattern.into()),
                ..GridQuery::default()
            };
            assert_eq!(
                store.try_query(query.clone()).unwrap().total,
                memory.query(query).total,
                "{pattern}"
            );
        }
        for search in ["inlinkonly", "a_b", "a%b", "_"] {
            let query = LinkEdgeQuery {
                global_search: Some(search.into()),
                ..LinkEdgeQuery::default()
            };
            assert_eq!(
                store.try_link_edges(query.clone()).unwrap().total,
                memory.link_edges(query).total,
                "links: {search}"
            );
        }
    }

    #[test]
    fn sqlite_reuses_summary_until_the_dataset_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let store = SqliteStore::in_memory().unwrap();
        let work = Arc::new(AtomicUsize::new(0));
        let calls = work.clone();
        store
            .connection()
            .unwrap()
            .create_scalar_function(
                "ff_text_key",
                1,
                rusqlite::functions::FunctionFlags::SQLITE_UTF8,
                move |context| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(normalize_text_key(
                        context
                            .get::<Option<String>>(0)?
                            .as_deref()
                            .unwrap_or_default(),
                    ))
                },
            )
            .unwrap();
        let mut record = CrawlRecord::pending("https://example.test/one".into(), 0);
        record.status_code = Some(200);
        record.content_type = Some("text/html".into());
        record.title = Some("Shared title".into());
        store.try_upsert(record.clone()).unwrap();
        assert_eq!(store.try_summary().unwrap().total, 1);
        let initial_work = work.load(Ordering::Relaxed);
        assert!(initial_work > 0);
        store.try_query(GridQuery::default()).unwrap();
        assert_eq!(
            work.load(Ordering::Relaxed),
            initial_work,
            "Paging unchanged results must reuse summary aggregates"
        );
        record.url = "https://example.test/two".into();
        record.final_url = record.url.clone();
        record.storage_key = record.url.clone();
        store.try_upsert(record).unwrap();
        let summary = store.try_summary().unwrap();
        assert_eq!((summary.total, summary.title_duplicate), (2, 2));
        assert!(work.load(Ordering::Relaxed) > initial_work);
        store.try_clear().unwrap();
        assert_eq!(store.try_summary().unwrap().total, 0);
    }

    #[test]
    fn sqlite_summary_observes_writes_from_another_connection() {
        let path = std::env::temp_dir().join(format!(
            "ferrous-frog-summary-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let reader = SqliteStore::open(&path).unwrap();
        let writer = SqliteStore::open(&path).unwrap();
        assert_eq!(reader.try_summary().unwrap().total, 0);
        writer
            .try_upsert(CrawlRecord::pending("https://example.test/".into(), 0))
            .unwrap();
        assert_eq!(reader.try_summary().unwrap().total, 1);
        writer.try_clear().unwrap();
        assert_eq!(reader.try_summary().unwrap().total, 0);
        drop(reader);
        drop(writer);
        std::fs::remove_file(path).unwrap();
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
    fn sqlite_hreflang_work_does_not_multiply_shared_redirect_aliases() {
        let mut work = Vec::new();
        for count in [200, 400] {
            let store = SqliteStore::in_memory().unwrap();
            for index in 0..count {
                let mut record = hreflang_record(
                    &format!("https://example.com/redirect-{index}"),
                    vec![hreflang_link("en", "https://example.com/en")],
                    Some("https://example.com/en"),
                );
                record.final_url = "https://example.com/en".into();
                store.try_upsert(record).unwrap();
            }
            let conn = store.connection().unwrap();
            let mut steps = 0;
            for view in [
                IssueView::HreflangMissingReturnLink,
                IssueView::HreflangNonCanonicalTarget,
            ] {
                let (filter, args) = query_filter_sql(&GridQuery {
                    view,
                    ..GridQuery::default()
                });
                let mut statement = conn
                    .prepare(&format!(
                        "{HREFLANG_AUDIT_CTES}SELECT COUNT(*) FROM crawl_records{filter}"
                    ))
                    .unwrap();
                let total: i64 = statement
                    .query_row(rusqlite::params_from_iter(args), |row| row.get(0))
                    .unwrap();
                assert_eq!(
                    total, 0,
                    "Shared final URLs still recognize self hreflang and canonicals"
                );
                steps += statement.get_status(rusqlite::StatementStatus::VmStep);
            }
            work.push(steps);
        }
        assert!(
            work[1] < work[0] * 3,
            "Doubling shared aliases must not quadruple SQLite work: {work:?}"
        );
    }

    #[test]
    fn sqlite_hreflang_queries_match_memory_without_decoding_other_pages() {
        let store = SqliteStore::in_memory().unwrap();
        let memory = MemoryStore::new();
        let mut records = vec![
            hreflang_record(
                "https://example.com/en",
                vec![hreflang_link("fr", "https://example.com/fr#section")],
                None,
            ),
            hreflang_record(
                "https://example.com/fr",
                vec![hreflang_link("en", "https://example.com/en#return")],
                Some("https://example.com/fr-new"),
            ),
            hreflang_record(
                "https://example.com/missing-return",
                vec![hreflang_link("de", "https://example.com/de#hint")],
                None,
            ),
            hreflang_record(
                "https://example.com/de",
                vec![],
                Some("https://example.com/preferred-de"),
            ),
            hreflang_record(
                "https://example.com/unseen",
                vec![hreflang_link("fr", "https://uncrawled.test/fr")],
                None,
            ),
            hreflang_record(
                "https://example.com/invalid",
                vec![HreflangLink {
                    valid: false,
                    ..hreflang_link("bad_locale", "https://example.com/de")
                }],
                None,
            ),
            hreflang_record(
                "https://host-only.test/",
                vec![hreflang_link("en", "https://host-only.test")],
                Some("https://host-only.test"),
            ),
            hreflang_record(
                "https://example.com/failure",
                vec![hreflang_link("de", "https://example.com/de")],
                None,
            ),
        ];
        records[1].final_url = "https://example.com/fr-new".into();
        records[3].hreflang_links = vec![HreflangLink {
            valid: false,
            ..hreflang_link("bad_locale", "https://example.com/missing-return")
        }];
        records[7].status_code = Some(404);
        // A later List duplicate must not replace the earliest target's evidence.
        let mut duplicate = records[3].clone();
        duplicate.url = "https://example.com/de#hint".into();
        duplicate.storage_key = "list:2:https://example.com/de".into();
        duplicate.canonical = Some("https://example.com/de".into());
        duplicate.hreflang_links = vec![hreflang_link("en", "https://example.com/missing-return")];
        records.push(duplicate);
        let mut duplicate_self = records[6].clone();
        duplicate_self.storage_key = "list:2:https://host-only.test/".into();
        records.push(duplicate_self);
        for record in records {
            store.try_upsert(record.clone()).unwrap();
            memory.upsert(record);
        }
        for view in [
            IssueView::HreflangMissingReturnLink,
            IssueView::HreflangNonCanonicalTarget,
        ] {
            let query = GridQuery {
                view: view.clone(),
                ..GridQuery::default()
            };
            let expected = memory.query(query.clone());
            assert_eq!(expected.total, 1, "{view:?}");
            assert_eq!(expected.rows[0].url, "https://example.com/missing-return");
            // Audit joins need only the target's URL, canonical and hreflang fields.
            store.connection().unwrap().execute("UPDATE crawl_records SET response_time_ms = 'unselected-payload' WHERE url = 'https://example.com/de'", []).unwrap();
            assert!(store.try_records().is_err());
            for (offset, search) in [
                (0, None),
                (1, None),
                (0, Some("missing")),
                (0, Some("no-match")),
            ] {
                let query = GridQuery {
                    offset,
                    limit: 1,
                    global_search: search.map(str::to_string),
                    sort_by: Some("url".into()),
                    sort_dir: SortDirection::Desc,
                    ..query.clone()
                };
                let actual = store.try_query(query.clone()).unwrap();
                let expected = memory.query(query);
                assert_eq!(
                    actual.total, expected.total,
                    "{view:?} offset={offset} search={search:?}"
                );
                assert_eq!(
                    actual
                        .rows
                        .iter()
                        .map(|row| &row.storage_key)
                        .collect::<Vec<_>>(),
                    expected
                        .rows
                        .iter()
                        .map(|row| &row.storage_key)
                        .collect::<Vec<_>>()
                );
            }
        }
        // New return/canonical evidence must take effect on the next query.
        let changed = hreflang_record(
            "https://example.com/de",
            vec![hreflang_link("en", "https://example.com/missing-return")],
            Some("https://example.com/de"),
        );
        store.try_upsert(changed).unwrap();
        for view in [
            IssueView::HreflangMissingReturnLink,
            IssueView::HreflangNonCanonicalTarget,
        ] {
            assert_eq!(
                store
                    .try_query(GridQuery {
                        view,
                        ..GridQuery::default()
                    })
                    .unwrap()
                    .total,
                0
            );
        }
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
            record.title = Some(format!("Synthetic page {}", index % 10_000));
            record.title_len = record.title.as_deref().unwrap_or_default().len();
            record.meta_description =
                Some(format!("Synthetic benchmark description for page {index}."));
            record.meta_description_len =
                record.meta_description.as_deref().unwrap_or_default().len();
            record.h1 = Some(format!("Synthetic page {index}"));
            record.h1_len = record.h1.as_deref().unwrap_or_default().len();
            if index % 100 == 0 && index + 1 < url_count {
                record.hreflang_links = vec![hreflang_link(
                    "fr",
                    &format!("https://synthetic.example.com/page/{:08}", index + 1),
                )];
                record.hreflang_count = 1;
            }
            if index % 100 == 1 {
                record.canonical = Some(format!(
                    "https://synthetic.example.com/page/{:08}",
                    index + 1
                ));
                record.canonical_count = 1;
            }
            record.outlink_count = 12;
            record.internal_outlink_count = 11;
            record.external_outlink_count = 1;
            record.inlink_count = if index == 0 { 0 } else { 1 };
            record.size_bytes = 24_000 + (index % 4096);
            store.upsert(record);
        }

        let elapsed = started.elapsed();
        eprintln!(
            "Insert: {url_count} URLs, {:.2?}, {:.2} URLs/sec",
            elapsed,
            url_count as f64 / elapsed.as_secs_f64().max(0.001)
        );
        let queried = std::time::Instant::now();
        let summary = store.summary();
        eprintln!("Summary: {:.2?}", queried.elapsed());
        let queried = std::time::Instant::now();
        let tail = store.query(GridQuery {
            offset: url_count.saturating_sub(10),
            limit: 10,
            sort_by: Some("finalUrl".to_string()),
            ..GridQuery::default()
        });

        assert_eq!(summary.total, url_count);
        assert!(!tail.rows.is_empty() || url_count == 0);
        eprintln!("Last page including summary: {:.2?}", queried.elapsed());
        for (name, query) in [
            (
                "Duplicate title page",
                GridQuery {
                    view: IssueView::TitleDuplicate,
                    limit: 10,
                    ..GridQuery::default()
                },
            ),
            (
                "Regex page",
                GridQuery {
                    segment_pattern: Some("/page/.*[13579]$".into()),
                    segment_regex: true,
                    limit: 10,
                    ..GridQuery::default()
                },
            ),
            (
                "Hreflang return-link page",
                GridQuery {
                    view: IssueView::HreflangMissingReturnLink,
                    limit: 10,
                    ..GridQuery::default()
                },
            ),
            (
                "Hreflang canonical-target page",
                GridQuery {
                    view: IssueView::HreflangNonCanonicalTarget,
                    limit: 10,
                    ..GridQuery::default()
                },
            ),
        ] {
            let queried = std::time::Instant::now();
            let result = store.try_query(query).unwrap();
            assert!(result.rows.len() <= 10);
            eprintln!(
                "{name} including summary: {:.2?}, {} matching URLs",
                queried.elapsed(),
                result.total
            );
        }
        eprintln!(
            "Database size: {} bytes",
            std::fs::metadata(&path).unwrap().len()
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
