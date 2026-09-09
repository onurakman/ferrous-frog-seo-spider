use anyhow::{Context, Result};
use ferrous_frog_extractors::{CustomExtractor, CustomSearch, run_extractors, run_searches};
use ferrous_frog_parser::{
    ContentSelectors, PageReferenceKind, PageResourceType, PageSignals,
    canonical_header_references, canonical_link_headers, contains_robots_directive,
    parse_html_with_content, same_host,
};
use ferrous_frog_storage::{
    CrawlFrontierItem, CrawlFrontierState, CrawlRecord, CrawlStore, CrawlSummary,
    CustomExtractionValue, CustomSearchSource, CustomSearchValue, HreflangLink, ImageAsset,
    LinkEdge, LinkType, RedirectHop, StructuredDataIssue, UrlClassification,
};
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use regex::Regex;
use reqwest::header::{
    CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderName, LINK, LOCATION, RETRY_AFTER,
};
use reqwest::{Client, StatusCode, redirect::Policy};
use rustls::ClientConfig;
use rustls_pki_types::ServerName;
use rustls_platform_verifier::ConfigVerifierExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use texting_robots::{Robot, get_robots_url};
use tokio::net::{TcpStream, lookup_host};
use tokio::sync::{Mutex, OnceCell};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tokio_rustls::TlsConnector;
use url::Url;

mod rendering;

pub use ferrous_frog_parser::ContentConfig;
pub use rendering::{
    JsRenderingBackend, JsRenderingConfig, RenderingStatus, rendering_status, validate_rendering,
};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36";
const DEFAULT_REQUEST_HEADERS: [(&str, &str); 3] = [
    // Signed exchanges need a decoder the HTTP fetcher does not provide.
    (
        "Accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
    ),
    ("Accept-Language", "en-US,en;q=0.9"),
    ("Upgrade-Insecure-Requests", "1"),
];
const CRAWL_CANCELLED_MESSAGE: &str = "crawl cancelled";
const MIN_NEAR_DUPLICATE_WORDS: usize = 20;
type HostRateLimiter = Arc<RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlConfig {
    #[serde(default)]
    pub mode: CrawlMode,
    pub start_url: String,
    #[serde(default)]
    pub list_urls: Vec<String>,
    #[serde(default)]
    pub list_sitemap_urls: Vec<String>,
    #[serde(default)]
    pub sitemap: SitemapConfig,
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: usize,
    pub max_urls: usize,
    pub max_depth: usize,
    pub concurrency: usize,
    pub requests_per_second: u32,
    pub request_delay_ms: u64,
    pub respect_robots: bool,
    #[serde(default)]
    pub use_robots_txt_override: bool,
    #[serde(default)]
    pub robots_txt_override: String,
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
    #[serde(default = "default_request_headers")]
    pub request_headers: Vec<RequestHeader>,
    pub timeout_secs: u64,
    pub max_redirects: usize,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
    pub near_duplicate_threshold: u32,
    #[serde(default)]
    pub include_url_patterns: Vec<String>,
    #[serde(default)]
    pub exclude_url_patterns: Vec<String>,
    #[serde(default)]
    pub subdomain_scope: SubdomainScope,
    #[serde(default)]
    pub folder_scope: FolderScope,
    #[serde(default)]
    pub check_links_outside_start_folder: bool,
    #[serde(default = "default_true")]
    pub follow_nofollow: bool,
    // Missing per-host choices inherit the setting in older saved profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_internal_nofollow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_external_nofollow: Option<bool>,
    #[serde(default)]
    pub resource_types: CrawlResourceTypes,
    #[serde(default)]
    pub reference_links: ReferenceLinksConfig,
    #[serde(default)]
    pub query_settings: QuerySettings,
    #[serde(default)]
    pub content: ContentConfig,
    #[serde(default)]
    pub custom_extractors: Vec<CustomExtractor>,
    #[serde(default)]
    pub custom_searches: Vec<CustomSearch>,
    #[serde(default)]
    pub rendering: JsRenderingConfig,
    #[serde(default)]
    pub resume_from_state: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReferenceLinksConfig {
    pub canonical: bool,
    pub hreflang: bool,
    pub pagination: bool,
    pub amp: bool,
}

impl ReferenceLinksConfig {
    fn allows(&self, kind: PageReferenceKind) -> bool {
        match kind {
            PageReferenceKind::Canonical => self.canonical,
            PageReferenceKind::Hreflang => self.hreflang,
            PageReferenceKind::Pagination => self.pagination,
            PageReferenceKind::Amp => self.amp,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SitemapConfig {
    pub enabled: bool,
    pub discover_from_robots: bool,
    pub probe_default: bool,
    pub follow_linked: bool,
    pub urls: Vec<String>,
}

impl Default for SitemapConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            discover_from_robots: true,
            probe_default: true,
            follow_linked: true,
            urls: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CrawlMode {
    #[default]
    Spider,
    List,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubdomainScope {
    #[default]
    IncludeSubdomains,
    ExactHost,
    AllSubdomains,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FolderScope {
    #[default]
    Anywhere,
    StartFolder,
    ExactFolder,
    ExactUrl,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlResourceTypes {
    #[serde(default = "default_true")]
    pub html: bool,
    #[serde(default)]
    pub images: bool,
    #[serde(default)]
    pub css: bool,
    #[serde(default)]
    pub javascript: bool,
    #[serde(default)]
    pub external: bool,
    #[serde(default)]
    pub other: bool,
}

impl Default for CrawlResourceTypes {
    fn default() -> Self {
        Self {
            html: true,
            images: false,
            css: false,
            javascript: false,
            external: false,
            other: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySettings {
    #[serde(default)]
    pub sort_parameters: bool,
    #[serde(default)]
    pub strip_all: bool,
    #[serde(default)]
    pub max_parameters: usize,
    #[serde(default)]
    pub strip_parameter_patterns: Vec<String>,
}

impl Default for CrawlConfig {
    fn default() -> Self {
        Self {
            mode: CrawlMode::Spider,
            start_url: "https://example.com/".to_string(),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            sitemap: SitemapConfig::default(),
            max_response_bytes: default_max_response_bytes(),
            max_urls: 5_000,
            max_depth: 3,
            concurrency: 8,
            requests_per_second: 10,
            request_delay_ms: 100,
            respect_robots: true,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: default_user_agent(),
            request_headers: default_request_headers(),
            timeout_secs: 20,
            max_redirects: 10,
            retry_attempts: default_retry_attempts(),
            retry_backoff_ms: default_retry_backoff_ms(),
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            check_links_outside_start_folder: false,
            follow_nofollow: true,
            follow_internal_nofollow: None,
            follow_external_nofollow: None,
            resource_types: CrawlResourceTypes::default(),
            reference_links: ReferenceLinksConfig::default(),
            query_settings: QuerySettings::default(),
            content: ContentConfig::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
        }
    }
}

fn default_user_agent() -> String {
    DEFAULT_USER_AGENT.to_string()
}

fn default_request_headers() -> Vec<RequestHeader> {
    DEFAULT_REQUEST_HEADERS
        .into_iter()
        .map(|(name, value)| RequestHeader {
            name: name.to_string(),
            value: value.to_string(),
        })
        .collect()
}

fn default_max_response_bytes() -> usize {
    20 * 1024 * 1024
}

fn default_true() -> bool {
    true
}

fn default_retry_attempts() -> u32 {
    1
}

fn default_retry_backoff_ms() -> u64 {
    250
}

#[derive(Clone, Default)]
pub struct CrawlControl {
    cancelled: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

impl CrawlControl {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlProgress {
    pub status: String,
    pub crawled: usize,
    pub queued: usize,
    pub discovered: usize,
    pub elapsed_ms: u64,
    pub pages_per_second: f64,
    pub summary: CrawlSummary,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlerEvent {
    pub kind: String,
    pub record: Option<CrawlRecord>,
    pub progress: Option<CrawlProgress>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotsTxtTestRequest {
    pub user_agent: String,
    pub robots_txt: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotsTxtTestResult {
    pub allowed: bool,
    pub crawl_delay_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotsTxtBatchTestRequest {
    pub user_agent: String,
    pub robots_txt: String,
    pub urls: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotsTxtBatchTestRow {
    pub url: String,
    pub allowed: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotsTxtBatchTestResult {
    pub crawl_delay_ms: Option<u64>,
    pub allowed: usize,
    pub blocked: usize,
    pub invalid: usize,
    pub rows: Vec<RobotsTxtBatchTestRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotsTxtDownloadRequest {
    pub url: String,
    pub user_agent: String,
    pub timeout_secs: u64,
    #[serde(default = "default_request_headers")]
    pub request_headers: Vec<RequestHeader>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobotsTxtDownloadResult {
    pub robots_url: String,
    pub status_code: u16,
    pub robots_txt: String,
}

impl CrawlerEvent {
    pub fn started(progress: CrawlProgress) -> Self {
        Self {
            kind: "started".to_string(),
            record: None,
            progress: Some(progress),
            message: None,
        }
    }

    pub fn record(record: CrawlRecord, progress: CrawlProgress) -> Self {
        Self {
            kind: "record".to_string(),
            record: Some(record),
            progress: Some(progress),
            message: None,
        }
    }

    pub fn progress(progress: CrawlProgress) -> Self {
        Self {
            kind: "progress".to_string(),
            record: None,
            progress: Some(progress),
            message: None,
        }
    }

    pub fn finished(progress: CrawlProgress) -> Self {
        Self {
            kind: "finished".to_string(),
            record: None,
            progress: Some(progress),
            message: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            kind: "error".to_string(),
            record: None,
            progress: None,
            message: Some(message),
        }
    }

    pub fn notice(message: String) -> Self {
        Self {
            kind: "notice".to_string(),
            record: None,
            progress: None,
            message: Some(message),
        }
    }
}

pub fn test_robots_txt(request: RobotsTxtTestRequest) -> Result<RobotsTxtTestResult> {
    let url = Url::parse(request.url.trim()).context("invalid robots test URL")?;
    let user_agent = if request.user_agent.trim().is_empty() {
        DEFAULT_USER_AGENT
    } else {
        request.user_agent.trim()
    };
    let bytes = request.robots_txt.as_bytes();
    let robot = parse_robots(user_agent, bytes)?;
    Ok(RobotsTxtTestResult {
        allowed: robot.allowed(url.as_str()),
        crawl_delay_ms: robots_crawl_delay_ms(&robot),
    })
}

pub fn test_robots_txt_batch(
    request: RobotsTxtBatchTestRequest,
) -> Result<RobotsTxtBatchTestResult> {
    let user_agent = if request.user_agent.trim().is_empty() {
        DEFAULT_USER_AGENT
    } else {
        request.user_agent.trim()
    };
    let bytes = request.robots_txt.as_bytes();
    let robot = parse_robots(user_agent, bytes)?;
    let mut rows = Vec::new();
    let mut allowed = 0usize;
    let mut blocked = 0usize;
    let mut invalid = 0usize;

    for raw_url in request.urls {
        let trimmed = raw_url.trim();
        if trimmed.is_empty() {
            continue;
        }
        match Url::parse(trimmed) {
            Ok(url) => {
                let is_allowed = robot.allowed(url.as_str());
                if is_allowed {
                    allowed = allowed.saturating_add(1);
                } else {
                    blocked = blocked.saturating_add(1);
                }
                rows.push(RobotsTxtBatchTestRow {
                    url: url.to_string(),
                    allowed: is_allowed,
                    error: None,
                });
            }
            Err(error) => {
                invalid = invalid.saturating_add(1);
                rows.push(RobotsTxtBatchTestRow {
                    url: trimmed.to_string(),
                    allowed: false,
                    error: Some(error.to_string()),
                });
            }
        }
    }

    Ok(RobotsTxtBatchTestResult {
        crawl_delay_ms: robots_crawl_delay_ms(&robot),
        allowed,
        blocked,
        invalid,
        rows,
    })
}

pub async fn download_robots_txt(
    request: RobotsTxtDownloadRequest,
) -> Result<RobotsTxtDownloadResult> {
    let root_url = Url::parse(request.url.trim()).context("invalid robots download URL")?;
    anyhow::ensure!(
        matches!(root_url.scheme(), "http" | "https") && root_url.host().is_some(),
        "Robots download URL must use HTTP or HTTPS"
    );
    let robots_url = get_robots_url(root_url.as_str()).context("failed to build robots URL")?;
    let headers = parse_request_headers(&request.request_headers)?;
    let user_agent = if request.user_agent.trim().is_empty() {
        DEFAULT_USER_AGENT
    } else {
        request.user_agent.trim()
    };
    let timeout = Duration::from_secs(if request.timeout_secs == 0 {
        20
    } else {
        request.timeout_secs
    });
    let client = Client::builder()
        .redirect(Policy::none())
        .user_agent(user_agent)
        .timeout(timeout)
        .build()
        .context("failed to build HTTP client")?;
    let mut current_url = Url::parse(&robots_url)?;
    let started_at = Instant::now();
    let mut redirects = 0;
    let response = loop {
        let response = request_with_headers(&client, &current_url, &root_url.origin(), &headers)
            .timeout(timeout.saturating_sub(started_at.elapsed()))
            .send()
            .await
            .context("failed to download robots.txt")?;
        if !response.status().is_redirection() {
            break response;
        }
        anyhow::ensure!(redirects < 5, "robots.txt exceeded five redirects");
        let location = redirect_location(response.headers())
            .context("robots.txt redirect missing Location header")?;
        current_url = ferrous_frog_parser::normalize_url(&current_url, &location)
            .context("invalid robots.txt redirect")?;
        redirects += 1;
    };
    let status_code = response.status().as_u16();
    let bytes = read_response_body(
        response,
        default_max_response_bytes(),
        &CrawlControl::default(),
    )
    .await
    .context("failed to read robots.txt body")?;
    Ok(RobotsTxtDownloadResult {
        robots_url,
        status_code,
        robots_txt: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

#[derive(Clone)]
struct QueueItem {
    url: Url,
    depth: usize,
    from_sitemap: bool,
    storage_key: String,
    list_position: Option<u32>,
    list_duplicate_index: u32,
}

#[derive(Clone, Debug)]
struct QueueIdentity {
    storage_key: String,
    list_position: Option<u32>,
    list_duplicate_index: u32,
    from_sitemap: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiscoveredResourceType {
    Html,
    Sitemap,
    Image,
    Css,
    JavaScript,
    Other,
}

#[derive(Clone, Debug)]
struct DiscoveredUrl {
    url: String,
    resource_type: DiscoveredResourceType,
    rel_nofollow: bool,
    reference_kind: Option<PageReferenceKind>,
}

struct FetchOutput {
    record: CrawlRecord,
    links: Vec<DiscoveredUrl>,
    edges: Vec<LinkEdge>,
    images: Vec<ImageAsset>,
    from_sitemap: bool,
}

#[derive(Default)]
struct OriginPolicy {
    // ponytail: cache for one crawl; add expiry before supporting crawls over 24 hours.
    robots: OnceCell<std::result::Result<Option<Robot>, String>>,
    last_request: Mutex<Option<Instant>>,
    retry_after: Mutex<Option<RetryAfterDeadline>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RetryAfterDeadline {
    Until(Instant),
    Indefinite,
}

struct RequestPolicy {
    origins: Mutex<HashMap<String, Arc<OriginPolicy>>>,
    rate_limiter: Option<HostRateLimiter>,
    respect_robots: bool,
    header_origin: url::Origin,
    request_headers: HeaderMap,
    on_event: Arc<dyn Fn(CrawlerEvent) + Send + Sync>,
}

struct FetchedResponse {
    response: std::result::Result<reqwest::Response, reqwest::Error>,
    network_timings: NetworkTimings,
    request_started_at: Instant,
}

#[derive(Clone)]
struct ScopeRules {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
}

#[derive(Clone)]
struct QueryRules {
    strip_parameter_patterns: Vec<Regex>,
}

#[derive(Clone, Copy, Debug, Default)]
struct NetworkTimings {
    dns_lookup_time_ms: Option<u64>,
    tcp_connect_time_ms: Option<u64>,
    tls_handshake_time_ms: Option<u64>,
    ttfb_ms: Option<u64>,
    download_time_ms: Option<u64>,
    total_network_time_ms: Option<u64>,
    transfer_rate_bytes_per_sec: Option<u64>,
    resolved_ip_count: u32,
}

#[derive(Clone, Debug)]
struct ContentFingerprint {
    simhash: u64,
    cluster_id: u64,
}

pub async fn crawl<S, F>(
    config: CrawlConfig,
    store: S,
    control: CrawlControl,
    on_event: F,
) -> Result<CrawlProgress>
where
    S: CrawlStore,
    F: Fn(CrawlerEvent) + Send + Sync + 'static,
{
    validate_crawl_start(&config)?;
    let on_event = Arc::new(on_event);
    let config = normalize_config(config);
    validate_rendering(&config.rendering)?;
    let scope_rules = compile_scope_rules(&config)?;
    let query_rules = compile_query_rules(&config)?;
    let content_selectors =
        Arc::new(ContentSelectors::compile(&config.content).map_err(anyhow::Error::msg)?);
    let root_url = root_url_from_config(&config, &query_rules)?;
    let client = Client::builder()
        .redirect(Policy::none())
        .user_agent(config.user_agent.clone())
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .context("failed to build HTTP client")?;
    let request_policy = Arc::new(RequestPolicy {
        origins: Mutex::new(HashMap::new()),
        rate_limiter: host_rate_limiter(config.requests_per_second),
        respect_robots: config.respect_robots,
        header_origin: root_url.origin(),
        request_headers: parse_request_headers(&config.request_headers)?,
        on_event: on_event.clone(),
    });
    let started_at = Instant::now();
    let mut sitemap_traversal = SitemapTraversal::default();
    let mut sitemap_pages = HashSet::new();
    let sitemap_result = if config.mode == CrawlMode::Spider
        && config.sitemap.enabled
        && config.folder_scope != FolderScope::ExactUrl
    {
        fetch_spider_sitemap_urls(
            &client,
            &root_url,
            &config,
            &request_policy,
            &control,
            &mut sitemap_traversal,
        )
        .await
    } else if config.mode == CrawlMode::List {
        fetch_list_sitemap_seed_urls(
            &client,
            &config,
            &request_policy,
            &control,
            &mut sitemap_traversal,
        )
        .await
    } else {
        Ok(Vec::new())
    };
    let discovered_sitemap_urls = match sitemap_result {
        Ok(urls) => urls,
        Err(_) if control.is_cancelled() => Vec::new(),
        Err(error) => anyhow::bail!("{error:#}"),
    };
    if control.is_cancelled() {
        let saved = config
            .resume_from_state
            .then(|| store.load_frontier_state())
            .flatten();
        let stopped = progress(
            "stopped",
            saved.as_ref().map_or(0, |state| state.crawled),
            saved.as_ref().map_or(0, |state| state.queued.len()),
            saved.as_ref().map_or(0, |state| state.seen.len()),
            started_at,
            &store,
        );
        on_event(CrawlerEvent::finished(stopped.clone()));
        return Ok(stopped);
    }
    let (sitemap_urls, list_sitemap_seed_urls) = if config.mode == CrawlMode::Spider {
        sitemap_pages.extend(
            discovered_sitemap_urls
                .iter()
                .filter(|url| {
                    scope_host_allows(url, &root_url, config.subdomain_scope)
                        && scope_allows(url, &root_url, &scope_rules, &config)
                })
                .map(ToString::to_string),
        );
        (discovered_sitemap_urls, Vec::new())
    } else {
        (Vec::new(), discovered_sitemap_urls)
    };
    let (mut queue, mut seen, mut crawled) = if config.resume_from_state {
        match restore_frontier_state(&store, &root_url, &scope_rules, &query_rules, &config)? {
            Some(state) => state,
            None => {
                let (queue, seen) = seed_frontier(
                    &config,
                    &root_url,
                    &query_rules,
                    list_sitemap_seed_urls,
                    sitemap_urls,
                    &scope_rules,
                )?;
                (queue, seen, 0)
            }
        }
    } else {
        let (queue, seen) = seed_frontier(
            &config,
            &root_url,
            &query_rules,
            list_sitemap_seed_urls,
            sitemap_urls,
            &scope_rules,
        )?;
        store.clear_frontier_state();
        (queue, seen, 0)
    };
    for item in &mut queue {
        item.from_sitemap |= sitemap_pages.contains(item.url.as_str());
    }
    store.mark_sitemap_urls(&sitemap_pages.iter().cloned().collect::<Vec<_>>());
    let mut active = JoinSet::new();
    let mut active_items = HashMap::<String, QueueItem>::new();
    let mut content_fingerprints = Vec::new();
    save_frontier_state(&store, &queue, &active_items, &seen, crawled);

    on_event(CrawlerEvent::started(progress(
        "running",
        crawled,
        queue.len(),
        seen.len(),
        started_at,
        &store,
    )));

    loop {
        wait_if_paused(&control).await;
        if control.is_cancelled() {
            break;
        }

        while active.len() < config.concurrency
            && !queue.is_empty()
            && crawled + active.len() < config.max_urls
        {
            wait_if_paused(&control).await;
            if control.is_cancelled() {
                break;
            }

            let item = queue.pop_front().expect("queue checked as non-empty");
            let item_key = item.storage_key.clone();
            let task_client = client.clone();
            let task_config = config.clone();
            let task_root = root_url.clone();
            let task_request_policy = request_policy.clone();
            let task_query_rules = query_rules.clone();
            let task_content_selectors = content_selectors.clone();
            let task_control = control.clone();
            active_items.insert(item_key.clone(), item.clone());
            save_frontier_state(&store, &queue, &active_items, &seen, crawled);
            active.spawn(async move {
                let result = fetch_one(
                    task_client,
                    task_config,
                    task_root,
                    task_request_policy,
                    task_query_rules,
                    task_content_selectors,
                    task_control,
                    item,
                )
                .await;
                (item_key, result)
            });
        }

        if active.is_empty() {
            break;
        }

        let joined = tokio::select! {
            joined = active.join_next() => joined,
            _ = wait_until_cancelled(&control) => break,
        };
        // A worker may finish before the cancellation poll. Keep its frontier
        // entry until the next run instead of publishing a cancelled result.
        if control.is_cancelled() {
            break;
        }

        if let Some(joined) = joined {
            let (item_key, output) = match joined {
                Ok(output) => output,
                Err(error) => {
                    on_event(CrawlerEvent::error(format!("crawl worker failed: {error}")));
                    continue;
                }
            };
            let output = match output {
                Ok(output) => output,
                Err(error) => {
                    if !control.is_cancelled() {
                        active_items.remove(&item_key);
                    }
                    save_frontier_state(&store, &queue, &active_items, &seen, crawled);
                    if control.is_cancelled() && error.to_string() == CRAWL_CANCELLED_MESSAGE {
                        continue;
                    }
                    on_event(CrawlerEvent::error(error.to_string()));
                    continue;
                }
            };

            let mut record = output.record;
            record.classification = if Url::parse(&record.final_url)
                .is_ok_and(|url| scope_host_allows(&url, &root_url, config.subdomain_scope))
            {
                UrlClassification::Internal
            } else {
                UrlClassification::External
            };
            let next_depth = record.depth + 1;

            if config.mode == CrawlMode::Spider
                && record.classification == UrlClassification::Internal
                && can_expand_record(&record, &root_url, &config)
                && next_depth <= config.max_depth
            {
                if config.sitemap.enabled
                    && config.sitemap.follow_linked
                    && config.folder_scope != FolderScope::ExactUrl
                {
                    for link in &output.links {
                        if link.resource_type != DiscoveredResourceType::Sitemap {
                            continue;
                        }
                        let Ok(link_url) = Url::parse(&link.url) else {
                            continue;
                        };
                        if !scope_host_allows(&link_url, &root_url, config.subdomain_scope)
                            || !scope_allows(&link_url, &root_url, &scope_rules, &config)
                            || (link.rel_nofollow
                                && !config
                                    .follow_internal_nofollow
                                    .unwrap_or(config.follow_nofollow))
                        {
                            continue;
                        }
                        let discovered = match fetch_sitemap_locations(
                            &client,
                            &link_url,
                            &config,
                            &request_policy,
                            &control,
                            &mut sitemap_traversal,
                        )
                        .await
                        {
                            Ok(discovered) => discovered,
                            Err(_) if control.is_cancelled() => break,
                            Err(error) => {
                                tracing::debug!(url = %link_url, %error, "Could not read linked sitemap");
                                continue;
                            }
                        };
                        let mut new_sitemap_pages = Vec::new();
                        for url in discovered {
                            if !scope_host_allows(&url, &root_url, config.subdomain_scope)
                                || !scope_allows(&url, &root_url, &scope_rules, &config)
                            {
                                continue;
                            }
                            let normalized = url.to_string();
                            if sitemap_pages.insert(normalized.clone()) {
                                new_sitemap_pages.push(normalized.clone());
                            }
                            if seen.len() < config.max_urls && seen.insert(normalized.clone()) {
                                queue.push_back(QueueItem {
                                    url,
                                    depth: 0,
                                    from_sitemap: true,
                                    storage_key: normalized,
                                    list_position: None,
                                    list_duplicate_index: 0,
                                });
                            }
                        }
                        if !new_sitemap_pages.is_empty() {
                            store.mark_sitemap_urls(&new_sitemap_pages);
                            for item in queue.iter_mut().chain(active_items.values_mut()) {
                                item.from_sitemap |= sitemap_pages.contains(item.url.as_str());
                            }
                        }
                    }
                }
                for link in &output.links {
                    if link
                        .reference_kind
                        .is_some_and(|kind| !config.reference_links.allows(kind))
                    {
                        continue;
                    }
                    if link.resource_type == DiscoveredResourceType::Sitemap
                        && config.sitemap.enabled
                        && config.sitemap.follow_linked
                        && sitemap_traversal.sitemaps.contains(&link.url)
                    {
                        continue;
                    }
                    if seen.len() >= config.max_urls {
                        break;
                    }
                    let Ok(mut link_url) = Url::parse(&link.url) else {
                        continue;
                    };
                    let follow_nofollow =
                        if scope_host_allows(&link_url, &root_url, config.subdomain_scope) {
                            config.follow_internal_nofollow
                        } else {
                            config.follow_external_nofollow
                        }
                        .unwrap_or(config.follow_nofollow);
                    if link.rel_nofollow && !follow_nofollow {
                        continue;
                    }
                    normalize_url_query(&mut link_url, &config.query_settings, &query_rules);
                    if !check_scope_allows(&link_url, &root_url, &scope_rules, &config)
                        || !should_crawl_discovered(
                            &link_url,
                            &root_url,
                            link.resource_type,
                            &config.resource_types,
                            config.subdomain_scope,
                        )
                    {
                        continue;
                    }
                    let normalized = link_url.to_string();
                    if seen.insert(normalized.clone()) {
                        queue.push_back(QueueItem {
                            url: link_url,
                            depth: next_depth,
                            from_sitemap: false,
                            storage_key: normalized,
                            list_position: None,
                            list_duplicate_index: 0,
                        });
                    }
                }
            }

            // Linked discovery must finish before this source leaves the saved frontier.
            if control.is_cancelled() {
                break;
            }
            active_items.remove(&item_key);
            for link in output
                .links
                .iter()
                .filter(|link| link.reference_kind.is_none())
            {
                if let Ok(mut link_url) = Url::parse(&link.url) {
                    normalize_url_query(&mut link_url, &config.query_settings, &query_rules);
                    store.add_inlink(link_url.as_str());
                }
            }
            for edge in &output.edges {
                store.add_link_edge(edge.clone());
            }

            record.in_sitemap = output.from_sitemap
                || sitemap_pages.contains(&record.url)
                || sitemap_pages.contains(&record.final_url);
            store.add_image_assets(&record.final_url, output.images);
            assign_near_duplicate_cluster(
                &mut record,
                &mut content_fingerprints,
                config.near_duplicate_threshold,
            );
            let record = store.upsert(record);
            crawled += 1;
            save_frontier_state(&store, &queue, &active_items, &seen, crawled);
            on_event(CrawlerEvent::record(
                record,
                progress(
                    "running",
                    crawled,
                    queue.len(),
                    seen.len(),
                    started_at,
                    &store,
                ),
            ));
        }
    }

    let status = if control.is_cancelled() {
        "stopped"
    } else {
        "finished"
    };
    if status == "finished" && queue.is_empty() && active_items.is_empty() {
        store.clear_frontier_state();
    } else {
        save_frontier_state(&store, &queue, &active_items, &seen, crawled);
    }
    let final_progress = progress(status, crawled, queue.len(), seen.len(), started_at, &store);
    on_event(CrawlerEvent::finished(final_progress.clone()));
    Ok(final_progress)
}

fn normalize_config(mut config: CrawlConfig) -> CrawlConfig {
    if config.max_urls == 0 {
        config.max_urls = 1;
    }
    if config.concurrency == 0 {
        config.concurrency = 1;
    }
    if config.timeout_secs == 0 {
        config.timeout_secs = 20;
    }
    if config.max_redirects == 0 {
        config.max_redirects = 10;
    }
    config.retry_attempts = config.retry_attempts.min(5);
    config.retry_backoff_ms = config.retry_backoff_ms.min(30_000);
    config.near_duplicate_threshold = config.near_duplicate_threshold.min(64);
    if config.user_agent.trim().is_empty() {
        config.user_agent = DEFAULT_USER_AGENT.to_string();
    }
    config
}

pub fn validate_configuration(config: &CrawlConfig) -> Result<()> {
    anyhow::ensure!(
        (1..=1024 * 1024 * 1024).contains(&config.max_response_bytes),
        "Maximum response size must be greater than zero and at most 1 GiB"
    );
    anyhow::ensure!(
        config.max_urls > 0 && config.concurrency > 0 && config.timeout_secs > 0,
        "Max URLs, concurrency and timeout must be greater than zero"
    );
    anyhow::ensure!(
        config.retry_attempts <= 5
            && config.retry_backoff_ms <= 30_000
            && config.near_duplicate_threshold <= 64,
        "Retries must be at most 5, backoff at most 30,000 ms and duplicate distance at most 64 bits"
    );
    anyhow::ensure!(
        !config.user_agent.trim().is_empty(),
        "User-Agent must not be empty"
    );
    reqwest::header::HeaderValue::from_bytes(config.user_agent.as_bytes())
        .context("invalid User-Agent")?;
    for value in std::iter::once(&config.start_url)
        .chain(&config.list_urls)
        .chain(&config.list_sitemap_urls)
        .chain(&config.sitemap.urls)
    {
        if value.trim().is_empty() {
            continue;
        }
        anyhow::ensure!(
            Url::parse(value.trim())
                .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some()),
            "Crawl targets and sitemap sources must be absolute HTTP or HTTPS URLs: {value}"
        );
    }
    parse_request_headers(&config.request_headers)?;
    compile_scope_rules(config)?;
    compile_query_rules(config)?;
    ContentSelectors::compile(&config.content).map_err(anyhow::Error::msg)?;
    run_extractors("<html><head/><body/></html>", &config.custom_extractors)?;
    run_searches("", &config.custom_searches)?;
    Ok(())
}

fn parse_request_headers(headers: &[RequestHeader]) -> Result<HeaderMap> {
    let mut parsed = HeaderMap::new();
    for (index, header) in headers.iter().enumerate() {
        let name = HeaderName::from_bytes(header.name.trim().as_bytes())
            .with_context(|| format!("Invalid HTTP request header name at row {}", index + 1))?;
        let key = name.as_str();
        let reserved = [
            "auth",
            "token",
            "secret",
            "password",
            "credential",
            "cookie",
            "api-key",
            "apikey",
            "csrf",
            "xsrf",
            "session",
            "access-key",
        ]
        .iter()
        .any(|part| key.contains(part))
            || key == "key"
            || key.ends_with("-key")
            || ["proxy-", "sec-", "x-forwarded-"]
                .iter()
                .any(|prefix| key.starts_with(prefix))
            || [
                "host",
                "content-length",
                "content-encoding",
                "content-range",
                "transfer-encoding",
                "connection",
                "keep-alive",
                "te",
                "trailer",
                "upgrade",
                "expect",
                "accept-encoding",
                "range",
                "if-range",
                "user-agent",
                "origin",
                "referer",
                "forwarded",
                "x-real-ip",
            ]
            .contains(&key);
        anyhow::ensure!(
            !reserved,
            "Request header {name} is reserved or may contain credentials"
        );
        anyhow::ensure!(
            !parsed.contains_key(&name),
            "Duplicate request header: {name}"
        );
        anyhow::ensure!(
            header
                .value
                .bytes()
                .all(|byte| byte == b'\t' || (32..=126).contains(&byte)),
            "Request header {name} must contain printable ASCII or tabs"
        );
        let value = reqwest::header::HeaderValue::from_bytes(header.value.as_bytes())
            .with_context(|| format!("Invalid value for request header {name}"))?;
        parsed.insert(name, value);
    }
    Ok(parsed)
}

pub fn validate_crawl_start(config: &CrawlConfig) -> Result<()> {
    validate_configuration(config)?;
    anyhow::ensure!(
        !config.start_url.trim().is_empty()
            || (config.mode == CrawlMode::List
                && config
                    .list_urls
                    .iter()
                    .chain(&config.list_sitemap_urls)
                    .any(|url| !url.trim().is_empty())),
        "Enter a seed URL or supply URLs/sitemaps in List mode before starting a crawl"
    );
    Ok(())
}

fn root_url_from_config(config: &CrawlConfig, query_rules: &QueryRules) -> Result<Url> {
    let root_seed = if config.mode == CrawlMode::List {
        config
            .list_urls
            .iter()
            .map(|url| url.trim())
            .find(|url| !url.is_empty())
            .or_else(|| {
                config
                    .list_sitemap_urls
                    .iter()
                    .map(|url| url.trim())
                    .find(|url| !url.is_empty())
            })
            .unwrap_or(config.start_url.trim())
    } else {
        config.start_url.trim()
    };
    let mut root_url = Url::parse(root_seed).context("invalid start URL")?;
    normalize_url_query(&mut root_url, &config.query_settings, query_rules);
    Ok(root_url)
}

fn seed_frontier(
    config: &CrawlConfig,
    root_url: &Url,
    query_rules: &QueryRules,
    list_sitemap_seed_urls: Vec<Url>,
    sitemap_urls: Vec<Url>,
    scope_rules: &ScopeRules,
) -> Result<(VecDeque<QueueItem>, HashSet<String>)> {
    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();
    for item in seed_queue_items(config, root_url, query_rules, &list_sitemap_seed_urls)? {
        let normalized = item.storage_key.clone();
        if seen.insert(normalized) {
            queue.push_back(item);
        }
    }
    for mut sitemap_url in sitemap_urls {
        if seen.len() >= config.max_urls {
            break;
        }
        normalize_url_query(&mut sitemap_url, &config.query_settings, query_rules);
        if scope_host_allows(&sitemap_url, root_url, config.subdomain_scope)
            && scope_allows(&sitemap_url, root_url, scope_rules, config)
        {
            let normalized = sitemap_url.to_string();
            if seen.insert(normalized.clone()) {
                queue.push_back(QueueItem {
                    url: sitemap_url,
                    depth: 0,
                    from_sitemap: true,
                    storage_key: normalized,
                    list_position: None,
                    list_duplicate_index: 0,
                });
            }
        }
    }

    Ok((queue, seen))
}

#[expect(
    clippy::type_complexity,
    reason = "Returns the scheduler's existing queue, seen set and counter"
)]
fn restore_frontier_state<S: CrawlStore>(
    store: &S,
    root_url: &Url,
    scope_rules: &ScopeRules,
    query_rules: &QueryRules,
    config: &CrawlConfig,
) -> Result<Option<(VecDeque<QueueItem>, HashSet<String>, usize)>> {
    let Some(state) = store.load_frontier_state() else {
        return Ok(None);
    };
    if state.queued.is_empty() && state.seen.is_empty() {
        return Ok(None);
    }

    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();
    for key in state.seen {
        if config.mode == CrawlMode::Spider {
            let mut url = Url::parse(&key).context("invalid saved Spider URL")?;
            normalize_url_query(&mut url, &config.query_settings, query_rules);
            seen.insert(url.to_string());
        } else {
            seen.insert(key);
        }
    }
    let mut queued_keys = HashSet::new();
    for item in state.queued {
        let mut item = queue_item_from_frontier(item)?;
        if config.mode == CrawlMode::Spider {
            normalize_url_query(&mut item.url, &config.query_settings, query_rules);
            item.storage_key = item.url.to_string();
        }
        // A resumed Spider crawl can have a narrower scope than its saved queue.
        // Explicit List entries and the Spider seed remain eligible.
        if config.mode == CrawlMode::Spider
            && item.url != *root_url
            && !check_scope_allows(&item.url, root_url, scope_rules, config)
        {
            seen.remove(&item.storage_key);
        } else if queued_keys.insert(item.storage_key.clone()) {
            seen.insert(item.storage_key.clone());
            queue.push_back(item);
        }
    }
    Ok(Some((queue, seen, state.crawled)))
}

fn save_frontier_state<S: CrawlStore>(
    store: &S,
    queue: &VecDeque<QueueItem>,
    active_items: &HashMap<String, QueueItem>,
    seen: &HashSet<String>,
    crawled: usize,
) {
    let mut queued = active_items
        .values()
        .map(frontier_item_from_queue)
        .collect::<Vec<_>>();
    queued.extend(queue.iter().map(frontier_item_from_queue));
    let mut seen = seen.iter().cloned().collect::<Vec<_>>();
    seen.sort();
    store.save_frontier_state(CrawlFrontierState {
        queued,
        seen,
        crawled,
    });
}

fn frontier_item_from_queue(item: &QueueItem) -> CrawlFrontierItem {
    CrawlFrontierItem {
        url: item.url.to_string(),
        depth: item.depth,
        from_sitemap: item.from_sitemap,
        storage_key: item.storage_key.clone(),
        list_position: item.list_position,
        list_duplicate_index: item.list_duplicate_index,
    }
}

fn queue_item_from_frontier(item: CrawlFrontierItem) -> Result<QueueItem> {
    Ok(QueueItem {
        url: Url::parse(&item.url).with_context(|| format!("invalid queued URL: {}", item.url))?,
        depth: item.depth,
        from_sitemap: item.from_sitemap,
        storage_key: item.storage_key,
        list_position: item.list_position,
        list_duplicate_index: item.list_duplicate_index,
    })
}

fn seed_queue_items(
    config: &CrawlConfig,
    root_url: &Url,
    query_rules: &QueryRules,
    list_sitemap_seed_urls: &[Url],
) -> Result<Vec<QueueItem>> {
    if config.mode == CrawlMode::Spider {
        return Ok(vec![QueueItem {
            url: root_url.clone(),
            depth: 0,
            from_sitemap: false,
            storage_key: root_url.to_string(),
            list_position: None,
            list_duplicate_index: 0,
        }]);
    }

    let manual_seeds = if config.list_urls.iter().all(|url| url.trim().is_empty())
        && list_sitemap_seed_urls.is_empty()
    {
        vec![config.start_url.trim()]
    } else {
        config
            .list_urls
            .iter()
            .map(|url| url.trim())
            .filter(|url| !url.is_empty())
            .collect::<Vec<_>>()
    };

    let mut items = Vec::new();
    let mut duplicate_counts = HashMap::<String, u32>::new();
    for seed in manual_seeds {
        let mut url = Url::parse(seed).with_context(|| format!("invalid list URL: {seed}"))?;
        normalize_url_query(&mut url, &config.query_settings, query_rules);
        push_list_queue_item(&mut items, &mut duplicate_counts, url, false);
    }

    for sitemap_seed_url in list_sitemap_seed_urls {
        let mut url = sitemap_seed_url.clone();
        normalize_url_query(&mut url, &config.query_settings, query_rules);
        push_list_queue_item(&mut items, &mut duplicate_counts, url, true);
    }

    if items.is_empty() {
        anyhow::bail!("list mode requires at least one URL");
    }
    Ok(items)
}

fn push_list_queue_item(
    items: &mut Vec<QueueItem>,
    duplicate_counts: &mut HashMap<String, u32>,
    url: Url,
    from_sitemap: bool,
) {
    let normalized = url.to_string();
    let duplicate_index = duplicate_counts.entry(normalized.clone()).or_insert(0);
    *duplicate_index = duplicate_index.saturating_add(1);
    let list_position = items.len().saturating_add(1).min(u32::MAX as usize) as u32;
    items.push(QueueItem {
        url,
        depth: 0,
        from_sitemap,
        storage_key: format!("list:{list_position}:{normalized}"),
        list_position: Some(list_position),
        list_duplicate_index: *duplicate_index,
    });
}

fn compile_scope_rules(config: &CrawlConfig) -> Result<ScopeRules> {
    let include = compile_regex_list(&config.include_url_patterns, "include")?;
    let exclude = compile_regex_list(&config.exclude_url_patterns, "exclude")?;
    Ok(ScopeRules { include, exclude })
}

fn compile_query_rules(config: &CrawlConfig) -> Result<QueryRules> {
    Ok(QueryRules {
        strip_parameter_patterns: compile_regex_list(
            &config.query_settings.strip_parameter_patterns,
            "query parameter strip",
        )?,
    })
}

fn compile_regex_list(patterns: &[String], label: &str) -> Result<Vec<Regex>> {
    patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| {
            Regex::new(pattern)
                .with_context(|| format!("invalid {label} URL regex pattern: {pattern}"))
        })
        .collect()
}

fn scope_patterns_allow(url: &Url, rules: &ScopeRules) -> bool {
    (rules.include.is_empty()
        || rules
            .include
            .iter()
            .any(|pattern| pattern.is_match(url.as_str())))
        && !rules
            .exclude
            .iter()
            .any(|pattern| pattern.is_match(url.as_str()))
}

fn scope_allows(url: &Url, root_url: &Url, rules: &ScopeRules, config: &CrawlConfig) -> bool {
    scope_patterns_allow(url, rules)
        && if scope_host_allows(url, root_url, config.subdomain_scope) {
            folder_scope_allows(url, root_url, config.folder_scope)
        } else {
            config.resource_types.external && config.folder_scope != FolderScope::ExactUrl
        }
}

fn check_scope_allows(url: &Url, root_url: &Url, rules: &ScopeRules, config: &CrawlConfig) -> bool {
    scope_allows(url, root_url, rules, config)
        || (config.mode == CrawlMode::Spider
            && config.folder_scope == FolderScope::StartFolder
            && config.check_links_outside_start_folder
            && scope_host_allows(url, root_url, config.subdomain_scope)
            && scope_patterns_allow(url, rules))
}

fn can_expand_record(record: &CrawlRecord, root_url: &Url, config: &CrawlConfig) -> bool {
    config.folder_scope != FolderScope::StartFolder
        || !config.check_links_outside_start_folder
        || [&record.url, &record.final_url].into_iter().all(|value| {
            Url::parse(value).is_ok_and(|url| {
                scope_host_allows(&url, root_url, config.subdomain_scope)
                    && folder_scope_allows(&url, root_url, FolderScope::StartFolder)
            })
        })
}

fn scope_host_allows(url: &Url, root_url: &Url, scope: SubdomainScope) -> bool {
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    let Some(root_host) = root_url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    let host = host.trim_end_matches('.');
    let root_host = root_host.trim_end_matches('.');

    match scope {
        SubdomainScope::ExactHost => host == root_host,
        SubdomainScope::IncludeSubdomains => {
            let base_host = root_host.strip_prefix("www.").unwrap_or(root_host);
            host == root_host || host == base_host || host.ends_with(&format!(".{base_host}"))
        }
        SubdomainScope::AllSubdomains => {
            if host == root_host {
                return true;
            }
            if !matches!(root_url.host(), Some(url::Host::Domain(_)))
                || !matches!(url.host(), Some(url::Host::Domain(_)))
            {
                return false;
            }
            match (
                psl::domain(host.as_bytes()),
                psl::domain(root_host.as_bytes()),
            ) {
                (Some(domain), Some(root_domain)) => {
                    root_domain.suffix().is_known() && domain == root_domain
                }
                _ => false,
            }
        }
    }
}

fn folder_scope_allows(url: &Url, root_url: &Url, scope: FolderScope) -> bool {
    match scope {
        FolderScope::Anywhere => true,
        FolderScope::StartFolder => url.path().starts_with(&folder_path(root_url)),
        FolderScope::ExactFolder => folder_path(url) == folder_path(root_url),
        FolderScope::ExactUrl => {
            url[..url::Position::AfterQuery] == root_url[..url::Position::AfterQuery]
        }
    }
}

fn folder_path(url: &Url) -> String {
    let path = url.path();
    if path.ends_with('/') {
        return path.to_string();
    }
    path.rsplit_once('/')
        .map(|(folder, _)| {
            if folder.is_empty() {
                "/".to_string()
            } else {
                format!("{folder}/")
            }
        })
        .unwrap_or_else(|| "/".to_string())
}

fn normalize_url_query(url: &mut Url, settings: &QuerySettings, rules: &QueryRules) {
    url.set_fragment(None);
    if settings.strip_all {
        url.set_query(None);
        return;
    }

    if url.query().is_none() {
        return;
    }

    let mut pairs = url
        .query_pairs()
        .filter_map(|(name, value)| {
            let name = name.into_owned();
            if rules
                .strip_parameter_patterns
                .iter()
                .any(|pattern| pattern.is_match(&name))
            {
                return None;
            }
            Some((name, value.into_owned()))
        })
        .collect::<Vec<_>>();

    if settings.sort_parameters {
        pairs.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    }

    if settings.max_parameters > 0 && pairs.len() > settings.max_parameters {
        pairs.truncate(settings.max_parameters);
    }

    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
}

fn should_crawl_discovered(
    url: &Url,
    root_url: &Url,
    resource_type: DiscoveredResourceType,
    resource_types: &CrawlResourceTypes,
    subdomain_scope: SubdomainScope,
) -> bool {
    let is_internal = scope_host_allows(url, root_url, subdomain_scope);
    if !is_internal && !resource_types.external {
        return false;
    }

    match resource_type {
        DiscoveredResourceType::Html => resource_types.html,
        DiscoveredResourceType::Image => resource_types.images,
        DiscoveredResourceType::Css => resource_types.css,
        DiscoveredResourceType::JavaScript => resource_types.javascript,
        DiscoveredResourceType::Other | DiscoveredResourceType::Sitemap => resource_types.other,
    }
}

fn classify_anchor_resource(url: &Url) -> DiscoveredResourceType {
    let extension = url
        .path_segments()
        .and_then(Iterator::last)
        .and_then(|segment| segment.rsplit_once('.').map(|(_, extension)| extension))
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some("html" | "htm" | "php" | "asp" | "aspx" | "jsp" | "cfm") | None => {
            DiscoveredResourceType::Html
        }
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "svg" | "ico") => {
            DiscoveredResourceType::Image
        }
        Some("css") => DiscoveredResourceType::Css,
        Some("js" | "mjs") => DiscoveredResourceType::JavaScript,
        Some("xml") => DiscoveredResourceType::Sitemap,
        _ => DiscoveredResourceType::Other,
    }
}

fn discovered_reference(
    url: String,
    kind: PageReferenceKind,
    rel_nofollow: bool,
) -> Option<DiscoveredUrl> {
    let resource_type = match classify_anchor_resource(&Url::parse(&url).ok()?) {
        // A reference to XML is a resource, not a linked sitemap declaration.
        DiscoveredResourceType::Sitemap => DiscoveredResourceType::Other,
        resource_type => resource_type,
    };
    Some(DiscoveredUrl {
        url,
        resource_type,
        rel_nofollow,
        reference_kind: Some(kind),
    })
}

fn map_page_resource_type(resource_type: PageResourceType) -> DiscoveredResourceType {
    match resource_type {
        PageResourceType::Image => DiscoveredResourceType::Image,
        PageResourceType::Css => DiscoveredResourceType::Css,
        PageResourceType::JavaScript => DiscoveredResourceType::JavaScript,
        PageResourceType::Other => DiscoveredResourceType::Other,
    }
}

async fn wait_if_paused(control: &CrawlControl) {
    while control.is_paused() && !control.is_cancelled() {
        sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_until_cancelled(control: &CrawlControl) {
    while !control.is_cancelled() {
        sleep(Duration::from_millis(25)).await;
    }
}

fn ensure_not_cancelled(control: &CrawlControl) -> Result<()> {
    if control.is_cancelled() {
        anyhow::bail!(CRAWL_CANCELLED_MESSAGE);
    }
    Ok(())
}

async fn read_response_body(
    mut response: reqwest::Response,
    max_bytes: usize,
    control: &CrawlControl,
) -> Result<Vec<u8>> {
    anyhow::ensure!(
        !response
            .content_length()
            .is_some_and(|length| length > max_bytes as u64),
        "Response body exceeds the configured limit of {max_bytes} bytes"
    );
    let mut bytes = Vec::new();
    loop {
        // Pause gates new requests; drain in-flight bodies before their timeout expires.
        ensure_not_cancelled(control)?;
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk.context("failed to read response body")?,
            _ = wait_until_cancelled(control) => anyhow::bail!(CRAWL_CANCELLED_MESSAGE),
        };
        let Some(chunk) = chunk else {
            return Ok(bytes);
        };
        anyhow::ensure!(
            chunk.len() <= max_bytes - bytes.len(),
            "Response body exceeds the configured limit of {max_bytes} bytes"
        );
        bytes.extend_from_slice(&chunk);
    }
}

async fn fetch_robots(
    client: &Client,
    url: &Url,
    config: &CrawlConfig,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
) -> Result<Option<Robot>> {
    if config.use_robots_txt_override && !config.robots_txt_override.trim().is_empty() {
        return parse_robots(&config.user_agent, config.robots_txt_override.as_bytes()).map(Some);
    }

    let mut robots_url = Url::parse(&get_robots_url(url.as_str())?)?;
    // RFC 9309 permits robots.txt redirects across origins; the resulting policy
    // still belongs to the origin that requested it.
    for redirect_count in 0..=5 {
        let response = request_policy
            .send(
                client,
                config,
                &robots_url,
                config.request_delay_ms,
                control,
                false,
            )
            .await?
            .response
            .with_context(|| format!("failed to fetch robots.txt: {robots_url}"))?;
        if response.status().is_redirection() {
            if redirect_count == 5 {
                anyhow::bail!("robots.txt exceeded five redirects: {robots_url}");
            }
            let location = redirect_location(response.headers())
                .context("robots.txt redirect missing Location header")?;
            robots_url = robots_url
                .join(&location)
                .context("invalid robots.txt redirect")?;
            continue;
        }
        if response.status().is_client_error() {
            return Ok(None);
        }
        if !response.status().is_success() {
            anyhow::bail!(
                "robots.txt returned HTTP {}: {robots_url}",
                response.status().as_u16()
            );
        }
        let bytes = read_response_body(response, config.max_response_bytes, control)
            .await
            .with_context(|| format!("failed to read robots.txt: {robots_url}"))?;
        return parse_robots(&config.user_agent, &bytes).map(Some);
    }
    unreachable!("robots redirect loop always returns");
}

fn parse_robots(user_agent: &str, bytes: &[u8]) -> Result<Robot> {
    let product_token = user_agent
        .trim()
        .split(['/', ' ', '\t'])
        .next()
        .unwrap_or(user_agent);
    Robot::new(product_token, bytes).context("invalid robots.txt content")
}

fn robots_crawl_delay_ms(robot: &Robot) -> Option<u64> {
    robot
        .delay
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(|seconds| (f64::from(seconds) * 1_000.0).round() as u64)
}

#[derive(Clone, Debug, Default)]
struct ParsedSitemap {
    urls: Vec<Url>,
    sitemaps: Vec<Url>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SitemapEntryKind {
    Url,
    Sitemap,
}

const MAX_SITEMAP_DOCUMENTS: usize = 128;
const MAX_SITEMAP_DEPTH: usize = 4;

#[derive(Default)]
struct SitemapTraversal {
    documents: HashSet<String>,
    sitemaps: HashSet<String>,
    urls: HashSet<String>,
    url_count: usize,
}

async fn fetch_spider_sitemap_urls(
    client: &Client,
    root_url: &Url,
    config: &CrawlConfig,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
    traversal: &mut SitemapTraversal,
) -> Result<Vec<Url>> {
    let mut urls = Vec::new();
    for source in config
        .sitemap
        .urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
    {
        let source = Url::parse(source)?;
        match fetch_sitemap_locations(client, &source, config, request_policy, control, traversal)
            .await
        {
            Ok(discovered) => urls.extend(discovered),
            Err(_) if control.is_cancelled() => return Ok(urls),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read explicit sitemap: {source}"));
            }
        }
    }
    let mut sources = Vec::new();
    if config.sitemap.discover_from_robots {
        let origin = request_policy
            .load_robots(client, config, root_url, control)
            .await?;
        if let Some(Ok(Some(robot))) = origin.robots.get() {
            sources.extend(
                robot
                    .sitemaps
                    .iter()
                    .filter_map(|source| ferrous_frog_parser::normalize_url(root_url, source)),
            );
        }
    }
    if config.sitemap.probe_default {
        sources.push(root_url.join("/sitemap.xml")?);
    }
    for source in sources {
        match fetch_sitemap_locations(client, &source, config, request_policy, control, traversal)
            .await
        {
            Ok(discovered) => urls.extend(discovered),
            Err(_) if control.is_cancelled() => break,
            Err(error) => tracing::debug!(url = %source, %error, "Could not discover sitemap"),
        }
    }
    Ok(urls)
}

async fn fetch_list_sitemap_seed_urls(
    client: &Client,
    config: &CrawlConfig,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
    traversal: &mut SitemapTraversal,
) -> Result<Vec<Url>> {
    let mut urls = Vec::new();
    for seed in config
        .list_sitemap_urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
    {
        let sitemap_url =
            Url::parse(seed).with_context(|| format!("invalid list sitemap URL: {seed}"))?;
        match fetch_sitemap_locations(
            client,
            &sitemap_url,
            config,
            request_policy,
            control,
            traversal,
        )
        .await
        {
            Ok(discovered) => urls.extend(discovered),
            Err(_) if control.is_cancelled() => break,
            Err(error) => return Err(error),
        }
    }
    Ok(urls)
}

async fn fetch_sitemap_locations(
    client: &Client,
    sitemap_url: &Url,
    config: &CrawlConfig,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
    traversal: &mut SitemapTraversal,
) -> Result<Vec<Url>> {
    let mut pending = VecDeque::from([(sitemap_url.clone(), 0usize)]);
    let mut queued = HashSet::from([sitemap_url.to_string()]);
    let mut urls = Vec::new();
    while let Some((current_url, depth)) = pending.pop_front() {
        if traversal.documents.len() >= MAX_SITEMAP_DOCUMENTS
            || traversal.url_count >= config.max_urls
        {
            break;
        }
        let parsed = match fetch_sitemap_document(
            client,
            &current_url,
            config,
            request_policy,
            control,
            traversal,
        )
        .await
        {
            Ok(Some(parsed)) => parsed,
            Ok(None) => continue,
            Err(error) if depth == 0 || control.is_cancelled() => return Err(error),
            Err(error) => {
                tracing::warn!(url = %current_url, %error, "Could not read nested sitemap");
                continue;
            }
        };
        for url in parsed.urls {
            if traversal.url_count >= config.max_urls {
                break;
            }
            if config.mode == CrawlMode::List || traversal.urls.insert(url.to_string()) {
                traversal.url_count += 1;
                urls.push(url);
            }
        }
        if depth < MAX_SITEMAP_DEPTH {
            for child in parsed.sitemaps {
                if pending.len() + traversal.documents.len() >= MAX_SITEMAP_DOCUMENTS {
                    break;
                }
                if !traversal.documents.contains(child.as_str()) && queued.insert(child.to_string())
                {
                    pending.push_back((child, depth + 1));
                }
            }
        }
    }
    Ok(urls)
}

async fn fetch_sitemap_document(
    client: &Client,
    sitemap_url: &Url,
    config: &CrawlConfig,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
    traversal: &mut SitemapTraversal,
) -> Result<Option<ParsedSitemap>> {
    let mut current_url = sitemap_url.clone();
    current_url.set_fragment(None);
    let mut redirects = HashSet::new();
    for redirect_count in 0..=config.max_redirects {
        anyhow::ensure!(
            redirects.insert(current_url.to_string()),
            "Sitemap redirect loop: {current_url}"
        );
        if traversal.sitemaps.contains(current_url.as_str()) {
            traversal.sitemaps.extend(redirects);
            return Ok(None);
        }
        if traversal.documents.len() >= MAX_SITEMAP_DOCUMENTS
            || !traversal.documents.insert(current_url.to_string())
        {
            return Ok(None);
        }
        let delay = request_policy
            .robots_delay(client, config, &current_url, control)
            .await
            .with_context(|| format!("cannot crawl sitemap: {current_url}"))?;
        let response = request_policy
            .send(client, config, &current_url, delay, control, false)
            .await?
            .response
            .with_context(|| format!("failed to fetch sitemap: {current_url}"))?;
        if response.status().is_redirection() {
            anyhow::ensure!(
                redirect_count < config.max_redirects,
                "Sitemap redirect limit exceeded: {current_url}"
            );
            let location = redirect_location(response.headers())
                .context("sitemap redirect missing Location header")?;
            current_url = ferrous_frog_parser::normalize_url(&current_url, &location)
                .with_context(|| format!("invalid sitemap redirect: {location}"))?;
            continue;
        }
        anyhow::ensure!(
            response.status().is_success(),
            "sitemap returned status {}: {current_url}",
            response.status().as_u16()
        );
        let bytes = read_response_body(response, config.max_response_bytes, control)
            .await
            .with_context(|| format!("failed to read sitemap body: {current_url}"))?;
        let xml = std::str::from_utf8(&bytes)
            .with_context(|| format!("sitemap is not valid UTF-8: {current_url}"))?;
        let parsed = parse_sitemap_document(xml, &current_url, config)
            .with_context(|| format!("invalid sitemap: {current_url}"))?;
        traversal.sitemaps.extend(redirects);
        return Ok(Some(parsed));
    }
    unreachable!("sitemap redirects always return");
}

fn parse_sitemap_document(
    xml: &str,
    sitemap_url: &Url,
    config: &CrawlConfig,
) -> Result<ParsedSitemap> {
    let query_rules = compile_query_rules(config)?;
    let scope_rules = compile_scope_rules(config)?;
    let root_url = root_url_from_config(config, &query_rules)?;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut parsed = ParsedSitemap::default();
    let mut current_entry = None;
    let mut root_kind = None;
    let mut depth = 0usize;
    let mut seen = HashSet::new();
    loop {
        match reader.read_event().context("invalid sitemap XML")? {
            event @ (Event::Start(_) | Event::Empty(_)) if depth == 0 => {
                let empty = matches!(&event, Event::Empty(_));
                let (Event::Start(element) | Event::Empty(element)) = event else {
                    unreachable!()
                };
                anyhow::ensure!(root_kind.is_none(), "multiple sitemap root elements");
                root_kind = Some(match element.local_name().as_ref() {
                    "urlset" => SitemapEntryKind::Url,
                    "sitemapindex" => SitemapEntryKind::Sitemap,
                    _ => anyhow::bail!("expected a urlset or sitemapindex root element"),
                });
                // Empty roots have no entries and are valid sitemap documents.
                depth = usize::from(!empty);
            }
            Event::Start(element) => {
                depth += 1;
                match (depth, element.local_name().as_ref()) {
                    (2, "url") if root_kind == Some(SitemapEntryKind::Url) => {
                        current_entry = Some(SitemapEntryKind::Url)
                    }
                    (2, "sitemap") if root_kind == Some(SitemapEntryKind::Sitemap) => {
                        current_entry = Some(SitemapEntryKind::Sitemap)
                    }
                    (3, "loc") if current_entry.is_some() => {
                        let text = reader
                            .read_text(element.name())
                            .context("invalid sitemap location")?;
                        depth -= 1;
                        let value =
                            unescape(text.trim()).context("invalid sitemap location escape")?;
                        if let Some(mut url) =
                            ferrous_frog_parser::normalize_url(sitemap_url, &value)
                        {
                            if current_entry == Some(SitemapEntryKind::Url) {
                                normalize_url_query(&mut url, &config.query_settings, &query_rules);
                                if config.mode == CrawlMode::Spider
                                    && (!scope_host_allows(&url, &root_url, config.subdomain_scope)
                                        || !scope_allows(&url, &root_url, &scope_rules, config))
                                {
                                    continue;
                                }
                            }
                            let entries = if current_entry == Some(SitemapEntryKind::Sitemap) {
                                &mut parsed.sitemaps
                            } else {
                                &mut parsed.urls
                            };
                            let limit = if current_entry == Some(SitemapEntryKind::Sitemap) {
                                MAX_SITEMAP_DOCUMENTS
                            } else {
                                config.max_urls
                            };
                            if entries.len() < limit
                                && (config.mode == CrawlMode::List || seen.insert(url.to_string()))
                            {
                                entries.push(url);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .context("unexpected sitemap closing element")?;
                if depth == 1 {
                    current_entry = None;
                }
            }
            Event::Text(text) if depth == 0 => {
                anyhow::ensure!(text.as_ref().trim().is_empty(), "text outside sitemap root")
            }
            Event::Eof => {
                anyhow::ensure!(root_kind.is_some() && depth == 0, "incomplete sitemap XML");
                return Ok(parsed);
            }
            _ => {}
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Shares compiled content selectors across requests without reparsing them"
)]
async fn fetch_one(
    client: Client,
    config: CrawlConfig,
    root_url: Url,
    request_policy: Arc<RequestPolicy>,
    query_rules: QueryRules,
    content_selectors: Arc<ContentSelectors>,
    control: CrawlControl,
    item: QueueItem,
) -> Result<FetchOutput> {
    let original_url = item.url.clone();
    let started_at = Instant::now();
    let queue_identity = QueueIdentity {
        storage_key: item.storage_key.clone(),
        list_position: item.list_position,
        list_duplicate_index: item.list_duplicate_index,
        from_sitemap: item.from_sitemap,
    };
    let mut current_url = item.url;
    let mut redirect_chain = Vec::new();

    for _ in 0..=config.max_redirects {
        wait_if_paused(&control).await;
        ensure_not_cancelled(&control)?;
        let request_delay_ms = match request_policy
            .robots_delay(&client, &config, &current_url, &control)
            .await
        {
            Ok(delay) => delay,
            Err(error) => {
                ensure_not_cancelled(&control)?;
                let mut record = blocked_record(&current_url, item.depth, &root_url);
                record.url = original_url.to_string();
                record.response_time_ms = elapsed_ms(started_at);
                record.redirect_target = redirect_chain
                    .last()
                    .and_then(|hop: &RedirectHop| hop.location.clone());
                record.redirect_type = redirect_chain.last().map(|hop| hop.status_code.to_string());
                record.redirect_chain = redirect_chain;
                record.error = Some(format!("{error:#}"));
                return Ok(fetch_output(
                    record,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    &queue_identity,
                ));
            }
        };
        let fetched = request_policy
            .send(
                &client,
                &config,
                &current_url,
                request_delay_ms,
                &control,
                true,
            )
            .await?;
        let mut network_timings = fetched.network_timings;
        let request_started_at = fetched.request_started_at;
        let response = match fetched.response {
            Ok(response) => response,
            Err(error) => {
                return Ok(fetch_output(
                    with_network_timings(
                        error_record(
                            &original_url,
                            &current_url,
                            item.depth,
                            &root_url,
                            started_at,
                            error.to_string(),
                            redirect_chain,
                        ),
                        network_timings,
                    ),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    &queue_identity,
                ));
            }
        };
        network_timings.ttfb_ms = Some(elapsed_ms(request_started_at));
        network_timings.total_network_time_ms = network_timings.ttfb_ms;

        let status = response.status();
        let headers = response.headers().clone();

        if status.is_redirection() {
            let Some(location) = redirect_location(&headers) else {
                let mut record = status_record(
                    &original_url,
                    &current_url,
                    item.depth,
                    &root_url,
                    started_at,
                    status,
                    headers,
                    &[],
                    redirect_chain,
                    Some("Redirect response missing Location header".to_string()),
                );
                apply_network_timings(&mut record, network_timings);
                return Ok(fetch_output(
                    record,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    &queue_identity,
                ));
            };

            let next_url = match current_url.join(&location) {
                Ok(url) => url,
                Err(error) => {
                    return Ok(fetch_output(
                        with_network_timings(
                            error_record(
                                &original_url,
                                &current_url,
                                item.depth,
                                &root_url,
                                started_at,
                                format!("Invalid redirect target: {error}"),
                                redirect_chain,
                            ),
                            network_timings,
                        ),
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        &queue_identity,
                    ));
                }
            };

            let next_url_string = next_url.to_string();
            let redirect_loop_detected = current_url.as_str() == next_url_string
                || redirect_chain.iter().any(|hop| hop.url == next_url_string);
            redirect_chain.push(RedirectHop {
                url: current_url.to_string(),
                status_code: status.as_u16(),
                location: Some(next_url_string.clone()),
                dns_lookup_time_ms: network_timings.dns_lookup_time_ms,
                tcp_connect_time_ms: network_timings.tcp_connect_time_ms,
                tls_handshake_time_ms: network_timings.tls_handshake_time_ms,
                ttfb_ms: network_timings.ttfb_ms,
                elapsed_ms: network_timings.ttfb_ms,
            });
            if redirect_loop_detected {
                return Ok(fetch_output(
                    with_network_timings(
                        error_record(
                            &original_url,
                            &current_url,
                            item.depth,
                            &root_url,
                            started_at,
                            format!("Redirect loop detected: {next_url_string}"),
                            redirect_chain,
                        ),
                        network_timings,
                    ),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    &queue_identity,
                ));
            }
            current_url = next_url;
            continue;
        }

        let mut links = canonical_header_references(
            &current_url,
            headers
                .get_all(LINK)
                .iter()
                .filter_map(|value| value.to_str().ok()),
        )
        .into_iter()
        .filter_map(|reference| {
            discovered_reference(reference.url, reference.kind, reference.rel_nofollow)
        })
        .collect::<Vec<_>>();
        let headers_for_record = headers.clone();
        let download_started_at = Instant::now();
        let bytes = match read_response_body(response, config.max_response_bytes, &control).await {
            Ok(bytes) => bytes,
            Err(error) => {
                ensure_not_cancelled(&control)?;
                network_timings.download_time_ms = Some(elapsed_ms(download_started_at));
                network_timings.total_network_time_ms = Some(elapsed_ms(request_started_at));
                let mut record = status_record(
                    &original_url,
                    &current_url,
                    item.depth,
                    &root_url,
                    started_at,
                    status,
                    headers_for_record,
                    &[],
                    redirect_chain,
                    Some(format!("{error:#}")),
                );
                apply_directives_and_canonical(&mut record, &current_url);
                if record.indexability == "Indexable" {
                    record.indexability = "Unknown".into();
                }
                record.indexability_status = "Response body incomplete".into();
                return Ok(fetch_output(
                    with_network_timings(record, network_timings),
                    links,
                    Vec::new(),
                    Vec::new(),
                    &queue_identity,
                ));
            }
        };
        network_timings.download_time_ms = Some(elapsed_ms(download_started_at));
        network_timings.total_network_time_ms = Some(elapsed_ms(request_started_at));
        network_timings.transfer_rate_bytes_per_sec =
            transfer_rate_bytes_per_sec(bytes.len(), network_timings.download_time_ms);

        let content_type = header_string(&headers_for_record, CONTENT_TYPE);
        let is_html = content_type
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains("text/html"))
            .unwrap_or(false);
        let mut record = status_record(
            &original_url,
            &current_url,
            item.depth,
            &root_url,
            started_at,
            status,
            headers_for_record,
            &bytes,
            redirect_chain,
            None,
        );
        apply_network_timings(&mut record, network_timings);
        let mut edges = Vec::new();
        let mut image_assets = Vec::new();

        if is_html {
            let raw_html = String::from_utf8_lossy(&bytes);
            let rendered_html = match rendering::render_page_if_enabled(
                &config,
                &current_url,
                &client,
                &request_policy,
                &control,
            )
            .await
            {
                Ok(rendered) => rendered.map(|page| page.html),
                Err(error) => {
                    ensure_not_cancelled(&control)?;
                    record.error = Some(format!("JavaScript rendering failed: {error:#}"));
                    None
                }
            };
            let raw_signals = rendered_html
                .as_ref()
                .map(|_| parse_html_with_content(&current_url, &raw_html, &content_selectors));
            let html = rendered_html.as_deref().unwrap_or(&raw_html);
            let signals = parse_html_with_content(&current_url, html, &content_selectors);
            if let Some(raw_signals) = raw_signals.as_ref() {
                apply_rendered_dom_diff(&mut record, raw_signals, &signals);
            }
            let page_images = signals.images;
            let visible_text = signals.visible_text;
            record.title = signals.title;
            record.title_count = Some(signals.title_count);
            record.title_len = signals.title_len;
            record.title_pixel_width = signals.title_pixel_width;
            record.meta_description = signals.meta_description;
            record.meta_description_count = Some(signals.meta_description_count);
            record.meta_description_len = signals.meta_description_len;
            record.meta_description_pixel_width = signals.meta_description_pixel_width;
            record.meta_robots = signals.meta_robots;
            record.h1 = signals.h1;
            record.h1_len = signals.h1_len;
            record.h1_count = signals.h1_count;
            record.h2 = signals.h2;
            record.h2_len = signals.h2_len;
            record.h2_count = signals.h2_count;
            record.canonical = signals.canonical.or(record.canonical);
            record.canonical_count += signals.canonical_count;
            record.word_count = signals.word_count;
            record.text_to_code_ratio = signals.text_to_code_ratio;
            record.image_count = signals.image_count;
            record.images_missing_alt = signals.images_missing_alt;
            record.images_alt_too_long = signals.images_alt_too_long;
            record.mixed_content_count = signals.mixed_content_count;
            record.insecure_form_count = signals.insecure_form_count;
            record.viewport = signals.viewport;
            record.amphtml = signals.amphtml;
            record.rel_next = signals.rel_next;
            record.rel_prev = signals.rel_prev;
            record.hreflang_count = signals.hreflang_count;
            record.hreflang_invalid_count = signals.hreflang_invalid_count;
            record.hreflang_missing_self_reference = signals.hreflang_missing_self_reference;
            record.hreflang_links = signals
                .hreflang_links
                .into_iter()
                .map(|link| HreflangLink {
                    hreflang: link.hreflang,
                    url: link.url,
                    valid: link.valid,
                })
                .collect();
            record.json_ld_count = signals.json_ld_count;
            record.json_ld_invalid_count = signals.json_ld_invalid_count;
            record.structured_data_error_count = signals.structured_data_error_count;
            record.structured_data_warning_count = signals.structured_data_warning_count;
            record.structured_data_issues = signals
                .structured_data_issues
                .into_iter()
                .map(|issue| StructuredDataIssue {
                    severity: issue.severity,
                    message: issue.message,
                    path: issue.path,
                })
                .collect();
            record.open_graph_count = signals.open_graph_count;
            record.twitter_card_count = signals.twitter_card_count;
            record.deprecated_html_tag_count = signals.deprecated_html_tag_count;
            record.duplicate_id_count = signals.duplicate_id_count;
            let page_nofollow =
                contains_robots_directive(record.meta_robots.as_deref(), "nofollow")
                    || contains_robots_directive(record.x_robots_tag.as_deref(), "nofollow");
            if !visible_text.is_empty() {
                record.simhash = Some(simhash::simhash(&visible_text));
            }
            record.custom_extractions = extract_custom_values(html, &config.custom_extractors);
            record.custom_searches = search_custom_values(
                &raw_html,
                &config.custom_searches,
                CustomSearchSource::RawHtml,
            );
            if let Some(rendered_html) = rendered_html.as_deref() {
                record.custom_searches.extend(search_custom_values(
                    rendered_html,
                    &config.custom_searches,
                    CustomSearchSource::RenderedHtml,
                ));
            }

            for image in page_images {
                image_assets.push(ImageAsset {
                    id: 0,
                    page_url: current_url.to_string(),
                    image_url: image.url,
                    alt_text: image.alt_text,
                    alt_len: image.alt_len,
                    missing_alt: image.missing_alt,
                    alt_too_long: image.alt_too_long,
                    width: image.width,
                    height: image.height,
                    source_position: image.source_position,
                    size_bytes: None,
                    oversized: false,
                });
            }

            links.extend(signals.sitemaps.into_iter().map(|url| DiscoveredUrl {
                url,
                resource_type: DiscoveredResourceType::Sitemap,
                rel_nofollow: page_nofollow,
                reference_kind: None,
            }));
            links.extend(signals.reference_links.into_iter().filter_map(|reference| {
                discovered_reference(reference.url, reference.kind, reference.rel_nofollow)
            }));
            for link in signals.links {
                let rel_nofollow = link.rel_nofollow || page_nofollow;
                let mut target_url = Url::parse(&link.url)?;
                normalize_url_query(&mut target_url, &config.query_settings, &query_rules);
                let target_url_string = target_url.to_string();
                let resource_type = if link
                    .rel
                    .split_whitespace()
                    .any(|rel| rel.eq_ignore_ascii_case("sitemap"))
                {
                    DiscoveredResourceType::Sitemap
                } else {
                    classify_anchor_resource(&target_url)
                };
                let link_type = if scope_host_allows(&target_url, &root_url, config.subdomain_scope)
                {
                    LinkType::Internal
                } else {
                    LinkType::External
                };
                if link_type == LinkType::Internal {
                    record.internal_outlink_count += 1;
                } else {
                    record.external_outlink_count += 1;
                }
                links.push(DiscoveredUrl {
                    url: if resource_type == DiscoveredResourceType::Sitemap
                        && config.mode == CrawlMode::Spider
                        && config.sitemap.enabled
                        && config.sitemap.follow_linked
                    {
                        link.url
                    } else {
                        target_url_string.clone()
                    },
                    resource_type,
                    rel_nofollow,
                    reference_kind: None,
                });
                edges.push(LinkEdge {
                    id: 0,
                    source_url: current_url.to_string(),
                    target_url: target_url_string,
                    anchor_text: link.text,
                    rel: link.rel,
                    rel_nofollow,
                    link_type,
                    source_status_code: Some(status.as_u16()),
                    target_status_code: None,
                    source_depth: item.depth,
                    target_depth: None,
                    source_position: link.source_position,
                    discovery_order: 0,
                });
            }

            for resource in signals.resources {
                let mut target_url = Url::parse(&resource.url)?;
                normalize_url_query(&mut target_url, &config.query_settings, &query_rules);
                let target_url_string = target_url.to_string();
                let resource_type = map_page_resource_type(resource.resource_type);
                let link_type = if scope_host_allows(&target_url, &root_url, config.subdomain_scope)
                {
                    LinkType::Internal
                } else {
                    LinkType::External
                };
                if link_type == LinkType::Internal {
                    record.internal_outlink_count += 1;
                } else {
                    record.external_outlink_count += 1;
                }
                links.push(DiscoveredUrl {
                    url: target_url_string.clone(),
                    resource_type,
                    rel_nofollow: page_nofollow,
                    reference_kind: None,
                });
                edges.push(LinkEdge {
                    id: 0,
                    source_url: current_url.to_string(),
                    target_url: target_url_string,
                    anchor_text: resource.label,
                    rel: String::new(),
                    rel_nofollow: page_nofollow,
                    link_type,
                    source_status_code: Some(status.as_u16()),
                    target_status_code: None,
                    source_depth: item.depth,
                    target_depth: None,
                    source_position: resource.source_position,
                    discovery_order: 0,
                });
            }
            record.outlink_count = record.internal_outlink_count + record.external_outlink_count;
        }
        apply_directives_and_canonical(&mut record, &current_url);

        return Ok(fetch_output(
            record,
            links,
            edges,
            image_assets,
            &queue_identity,
        ));
    }

    Ok(fetch_output(
        error_record(
            &original_url,
            &current_url,
            item.depth,
            &root_url,
            started_at,
            "Redirect limit exceeded".to_string(),
            redirect_chain,
        ),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &queue_identity,
    ))
}

fn fetch_output(
    mut record: CrawlRecord,
    mut links: Vec<DiscoveredUrl>,
    edges: Vec<LinkEdge>,
    images: Vec<ImageAsset>,
    identity: &QueueIdentity,
) -> FetchOutput {
    let page_nofollow = contains_robots_directive(record.meta_robots.as_deref(), "nofollow")
        || contains_robots_directive(record.x_robots_tag.as_deref(), "nofollow");
    for link in links
        .iter_mut()
        .filter(|link| link.reference_kind.is_some())
    {
        link.rel_nofollow |= page_nofollow;
    }
    record.storage_key = identity.storage_key.clone();
    record.list_position = identity.list_position;
    record.list_duplicate_index = identity.list_duplicate_index;
    FetchOutput {
        record,
        links,
        edges,
        images,
        from_sitemap: identity.from_sitemap,
    }
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

async fn measure_dns_lookup(url: &Url) -> (Option<u64>, u32, Option<SocketAddr>) {
    let Some(host) = url.host_str() else {
        return (None, 0, None);
    };
    let Some(port) = url.port_or_known_default() else {
        return (None, 0, None);
    };

    let started_at = Instant::now();
    match lookup_host((host, port)).await {
        Ok(addresses) => {
            let addresses = addresses.collect::<Vec<_>>();
            (
                Some(elapsed_ms(started_at)),
                addresses.len().min(u32::MAX as usize) as u32,
                addresses.first().copied(),
            )
        }
        Err(_) => (None, 0, None),
    }
}

async fn measure_connection_probe(
    url: &Url,
    address: Option<SocketAddr>,
    timeout: Duration,
) -> (Option<u64>, Option<u64>) {
    let Some(address) = address else {
        return (None, None);
    };
    let started_at = Instant::now();
    let stream = match tokio::time::timeout(timeout, TcpStream::connect(address)).await {
        Ok(Ok(stream)) => stream,
        _ => return (None, None),
    };
    let tcp_connect_time_ms = Some(elapsed_ms(started_at));

    if url.scheme() != "https" {
        drop(stream);
        return (tcp_connect_time_ms, None);
    }

    let Some(host) = url.host_str() else {
        drop(stream);
        return (tcp_connect_time_ms, None);
    };
    let Some(config) = tls_client_config() else {
        drop(stream);
        return (tcp_connect_time_ms, None);
    };
    let Ok(server_name) = ServerName::try_from(host.to_string()) else {
        drop(stream);
        return (tcp_connect_time_ms, None);
    };
    let connector = TlsConnector::from(config);
    let started_at = Instant::now();
    let tls_handshake_time_ms =
        match tokio::time::timeout(timeout, connector.connect(server_name, stream)).await {
            Ok(Ok(stream)) => {
                drop(stream);
                Some(elapsed_ms(started_at))
            }
            _ => None,
        };

    (tcp_connect_time_ms, tls_handshake_time_ms)
}

fn tls_client_config() -> Option<Arc<ClientConfig>> {
    static TLS_CLIENT_CONFIG: OnceLock<Result<Arc<ClientConfig>, String>> = OnceLock::new();
    TLS_CLIENT_CONFIG
        .get_or_init(|| {
            ClientConfig::with_platform_verifier()
                .map(Arc::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .ok()
        .cloned()
}

fn tcp_connect_timeout(config: &CrawlConfig) -> Duration {
    Duration::from_secs(config.timeout_secs.clamp(1, 5))
}

fn transfer_rate_bytes_per_sec(size_bytes: usize, download_time_ms: Option<u64>) -> Option<u64> {
    let download_time_ms = download_time_ms?;
    if size_bytes == 0 || download_time_ms == 0 {
        return None;
    }

    Some((size_bytes as u64).saturating_mul(1000) / download_time_ms)
}

fn with_network_timings(mut record: CrawlRecord, timings: NetworkTimings) -> CrawlRecord {
    apply_network_timings(&mut record, timings);
    record
}

fn apply_network_timings(record: &mut CrawlRecord, timings: NetworkTimings) {
    record.dns_lookup_time_ms = timings.dns_lookup_time_ms;
    record.tcp_connect_time_ms = timings.tcp_connect_time_ms;
    record.tls_handshake_time_ms = timings.tls_handshake_time_ms;
    record.ttfb_ms = timings.ttfb_ms;
    record.download_time_ms = timings.download_time_ms;
    record.total_network_time_ms = timings.total_network_time_ms;
    record.transfer_rate_bytes_per_sec = timings.transfer_rate_bytes_per_sec;
    record.resolved_ip_count = timings.resolved_ip_count;
}

fn host_rate_limiter(requests_per_second: u32) -> Option<HostRateLimiter> {
    NonZeroU32::new(requests_per_second)
        .map(Quota::per_second)
        .map(RateLimiter::keyed)
        .map(Arc::new)
}

fn request_with_headers(
    client: &Client,
    url: &Url,
    origin: &url::Origin,
    headers: &HeaderMap,
) -> reqwest::RequestBuilder {
    let request = client.get(url.clone());
    if url.origin() == *origin {
        request.headers(headers.clone())
    } else {
        request
    }
}

impl RequestPolicy {
    fn request(&self, client: &Client, url: &Url) -> reqwest::RequestBuilder {
        request_with_headers(client, url, &self.header_origin, &self.request_headers)
    }

    async fn send(
        &self,
        client: &Client,
        config: &CrawlConfig,
        url: &Url,
        delay_ms: u64,
        control: &CrawlControl,
        measure_timings: bool,
    ) -> Result<FetchedResponse> {
        let mut attempt = 0;
        loop {
            wait_if_paused(control).await;
            ensure_not_cancelled(control)?;
            let mut network_timings = NetworkTimings::default();
            if measure_timings {
                let (dns_lookup_time_ms, resolved_ip_count, tcp_address) =
                    measure_dns_lookup(url).await;
                network_timings.dns_lookup_time_ms = dns_lookup_time_ms;
                network_timings.resolved_ip_count = resolved_ip_count;
                ensure_not_cancelled(control)?;
                let (tcp_connect_time_ms, tls_handshake_time_ms) =
                    measure_connection_probe(url, tcp_address, tcp_connect_timeout(config)).await;
                network_timings.tcp_connect_time_ms = tcp_connect_time_ms;
                network_timings.tls_handshake_time_ms = tls_handshake_time_ms;
            }
            self.wait(url, delay_ms, control).await?;
            let request_started_at = Instant::now();
            let response = tokio::select! {
                response = self.request(client, url).send() => response,
                _ = wait_until_cancelled(control) => anyhow::bail!(CRAWL_CANCELLED_MESSAGE),
            };
            network_timings.total_network_time_ms = Some(elapsed_ms(request_started_at));
            let retryable = match &response {
                Ok(response) => {
                    network_timings.ttfb_ms = network_timings.total_network_time_ms;
                    self.observe_retry_after(url, response.status(), response.headers())
                        .await;
                    retryable_status(response.status())
                }
                Err(_) => true,
            };
            if retryable && attempt < config.retry_attempts {
                attempt += 1;
                drop(response);
                sleep_retry_backoff(config, attempt, control).await?;
                continue;
            }
            return Ok(FetchedResponse {
                response,
                network_timings,
                request_started_at,
            });
        }
    }

    async fn observe_retry_after(&self, url: &Url, status: StatusCode, headers: &HeaderMap) {
        if !matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        ) {
            return;
        }
        let Some(delay) =
            retry_after_delay(headers, SystemTime::now()).filter(|delay| !delay.is_zero())
        else {
            return;
        };
        let deadline = Instant::now()
            .checked_add(delay)
            .map(RetryAfterDeadline::Until)
            .unwrap_or(RetryAfterDeadline::Indefinite);
        let origin = self.origin(url).await;
        let mut retry_after = origin.retry_after.lock().await;
        if retry_after.is_none_or(|current| deadline > current) {
            *retry_after = Some(deadline);
            drop(retry_after);
            let wait = if deadline == RetryAfterDeadline::Indefinite {
                "an unrepresentable delay; waiting until the crawl is stopped".to_string()
            } else {
                format!(
                    "at least {} seconds",
                    delay
                        .as_secs()
                        .saturating_add(u64::from(delay.subsec_nanos() > 0))
                )
            };
            (self.on_event)(CrawlerEvent::notice(format!(
                "HTTP {} requested Retry-After for {}: {wait}. Pause and Stop remain available.",
                status.as_u16(),
                url.origin().ascii_serialization()
            )));
        }
    }

    async fn origin(&self, url: &Url) -> Arc<OriginPolicy> {
        self.origins
            .lock()
            .await
            .entry(url.origin().ascii_serialization())
            .or_default()
            .clone()
    }

    async fn load_robots(
        &self,
        client: &Client,
        config: &CrawlConfig,
        url: &Url,
        control: &CrawlControl,
    ) -> Result<Arc<OriginPolicy>> {
        let origin = self.origin(url).await;
        tokio::select! {
            _ = origin.robots.get_or_init(|| async {
                fetch_robots(client, url, config, self, control)
                    .await.map_err(|error| format!("{error:#}"))
            }) => {},
            _ = wait_until_cancelled(control) => anyhow::bail!(CRAWL_CANCELLED_MESSAGE),
        }
        Ok(origin)
    }

    async fn robots_delay(
        &self,
        client: &Client,
        config: &CrawlConfig,
        url: &Url,
        control: &CrawlControl,
    ) -> Result<u64> {
        wait_if_paused(control).await;
        ensure_not_cancelled(control)?;
        if !config.respect_robots {
            return Ok(config.request_delay_ms);
        }
        let origin = self.load_robots(client, config, url, control).await?;
        let robots = origin.robots.get().expect("robots cache initialized");
        match robots {
            Ok(Some(robot)) => {
                if !robot.allowed(url.as_str()) {
                    anyhow::bail!("Blocked by robots.txt");
                }
                Ok(config
                    .request_delay_ms
                    .max(robots_crawl_delay_ms(robot).unwrap_or(0)))
            }
            Ok(None) => Ok(config.request_delay_ms),
            Err(error) => anyhow::bail!("Cannot verify robots.txt: {error}"),
        }
    }

    async fn wait(&self, url: &Url, delay_ms: u64, control: &CrawlControl) -> Result<()> {
        let origin = self.origin(url).await;
        let mut last_request = origin.last_request.lock().await;
        // Read only initialized policies: fetching robots.txt must not recursively load them.
        let delay_ms = match origin.robots.get() {
            Some(Ok(Some(robot))) if self.respect_robots => {
                delay_ms.max(robots_crawl_delay_ms(robot).unwrap_or(0))
            }
            _ => delay_ms,
        };
        wait_if_paused(control).await;
        ensure_not_cancelled(control)?;
        if let Some(last_request) = *last_request {
            let remaining = Duration::from_millis(delay_ms).saturating_sub(last_request.elapsed());
            tokio::select! {
                _ = sleep(remaining) => {},
                _ = wait_until_cancelled(control) => anyhow::bail!(CRAWL_CANCELLED_MESSAGE),
            }
        }
        if let Some(rate_limiter) = &self.rate_limiter {
            let host = url.host_str().unwrap_or_default().to_string();
            tokio::select! {
                _ = rate_limiter.until_key_ready(&host) => {},
                _ = wait_until_cancelled(control) => anyhow::bail!(CRAWL_CANCELLED_MESSAGE),
            }
        }
        // Check after pacing and any pause: another response can extend the
        // origin cooldown while this worker waits for its request slot.
        loop {
            wait_if_paused(control).await;
            ensure_not_cancelled(control)?;
            let remaining = {
                let mut retry_after = origin.retry_after.lock().await;
                match *retry_after {
                    Some(RetryAfterDeadline::Until(deadline)) => {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            *retry_after = None;
                        }
                        remaining
                    }
                    Some(RetryAfterDeadline::Indefinite) => Duration::MAX,
                    None => Duration::ZERO,
                }
            };
            if remaining.is_zero() {
                break;
            }
            // Chunk long waits so even overflowing server delays never reach
            // the timer's representable-time limit. Recheck extended cooldowns.
            tokio::select! {
                _ = sleep(remaining.min(Duration::from_secs(60))) => {},
                _ = wait_until_cancelled(control) => anyhow::bail!(CRAWL_CANCELLED_MESSAGE),
            }
        }
        *last_request = Some(Instant::now());
        Ok(())
    }
}

fn retry_after_delay(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    headers
        .get_all(RETRY_AFTER)
        .iter()
        .filter_map(|value| {
            let value = value.to_str().ok()?.trim();
            if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
                // An overflowing integer still requests a delay; treating it as
                // malformed would incorrectly permit an immediate retry.
                Some(Duration::from_secs(
                    value.parse::<u64>().unwrap_or(u64::MAX),
                ))
            } else {
                httpdate::parse_http_date(value)
                    .ok()
                    .map(|deadline| deadline.duration_since(now).unwrap_or(Duration::ZERO))
            }
        })
        .max()
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

async fn sleep_retry_backoff(
    config: &CrawlConfig,
    attempt: u32,
    control: &CrawlControl,
) -> Result<()> {
    let exponent = attempt.saturating_sub(1).min(10);
    let multiplier = 1_u64 << exponent;
    let delay_ms = config
        .retry_backoff_ms
        .saturating_mul(multiplier)
        .min(30_000);
    tokio::select! {
        _ = sleep(Duration::from_millis(delay_ms)) => {},
        _ = wait_until_cancelled(control) => anyhow::bail!(CRAWL_CANCELLED_MESSAGE),
    }
    ensure_not_cancelled(control)
}

fn extract_custom_values(html: &str, extractors: &[CustomExtractor]) -> Vec<CustomExtractionValue> {
    if extractors.is_empty() {
        return Vec::new();
    }

    match run_extractors(html, extractors) {
        Ok(results) => results
            .into_iter()
            .map(|result| CustomExtractionValue {
                name: result.name,
                values: result.values,
            })
            .collect(),
        Err(error) => vec![CustomExtractionValue {
            name: "extractor_error".to_string(),
            values: vec![error.to_string()],
        }],
    }
}

fn search_custom_values(
    html: &str,
    searches: &[CustomSearch],
    source: CustomSearchSource,
) -> Vec<CustomSearchValue> {
    if searches.is_empty() {
        return Vec::new();
    }

    match run_searches(html, searches) {
        Ok(results) => results
            .into_iter()
            .map(|result| CustomSearchValue {
                name: result.name,
                source: source.clone(),
                matched: result.matched,
                match_count: result.match_count,
                snippets: result.snippets,
            })
            .collect(),
        Err(error) => vec![CustomSearchValue {
            name: "search_error".to_string(),
            source,
            matched: false,
            match_count: 0,
            snippets: vec![error.to_string()],
        }],
    }
}

fn apply_rendered_dom_diff(
    record: &mut CrawlRecord,
    raw_signals: &PageSignals,
    rendered_signals: &PageSignals,
) {
    record.js_rendered = true;
    record.rendered_word_count_delta =
        signed_delta(rendered_signals.word_count, raw_signals.word_count);
    record.rendered_link_count_delta =
        signed_delta(rendered_signals.links.len(), raw_signals.links.len());
    record.rendered_dom_changed = raw_signals.title != rendered_signals.title
        || raw_signals.title_count != rendered_signals.title_count
        || raw_signals.meta_description_count != rendered_signals.meta_description_count
        || raw_signals.meta_description != rendered_signals.meta_description
        || raw_signals.h1 != rendered_signals.h1
        || raw_signals.canonical != rendered_signals.canonical
        || raw_signals.word_count != rendered_signals.word_count
        || raw_signals.links.len() != rendered_signals.links.len()
        || raw_signals.image_count != rendered_signals.image_count
        || raw_signals.json_ld_count != rendered_signals.json_ld_count;
}

fn signed_delta(current: usize, previous: usize) -> i32 {
    let delta = current as i128 - previous as i128;
    delta.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32
}

fn apply_directives_and_canonical(record: &mut CrawlRecord, current_url: &Url) {
    if !record
        .status_code
        .is_some_and(|status| (200..300).contains(&status))
    {
        return;
    }
    if contains_robots_directive(record.x_robots_tag.as_deref(), "noindex") {
        record.indexability = "Non-indexable".to_string();
        record.indexability_status = "X-Robots-Tag noindex".to_string();
        return;
    }

    if contains_robots_directive(record.meta_robots.as_deref(), "noindex") {
        record.indexability = "Non-indexable".to_string();
        record.indexability_status = "Meta robots noindex".to_string();
        return;
    }

    if let Some(canonical) = record.canonical.as_deref()
        && canonical != current_url.as_str()
    {
        record.indexability = "Non-indexable".to_string();
        record.indexability_status = "Canonicalized".to_string();
    }
}

fn assign_near_duplicate_cluster(
    record: &mut CrawlRecord,
    fingerprints: &mut Vec<ContentFingerprint>,
    threshold: u32,
) {
    if record.word_count < MIN_NEAR_DUPLICATE_WORDS {
        return;
    }

    let Some(hash) = record.simhash else {
        return;
    };

    let cluster_id = fingerprints
        .iter()
        .find(|fingerprint| simhash::hamming_distance(fingerprint.simhash, hash) <= threshold)
        .map(|fingerprint| fingerprint.cluster_id)
        .unwrap_or_else(|| fingerprints.len() as u64 + 1);

    record.near_duplicate_cluster_id = Some(cluster_id);
    fingerprints.push(ContentFingerprint {
        simhash: hash,
        cluster_id,
    });
}

#[expect(
    clippy::too_many_arguments,
    reason = "Keeps HTTP response fields explicit at the record boundary"
)]
fn status_record(
    original_url: &Url,
    final_url: &Url,
    depth: usize,
    root_url: &Url,
    started_at: Instant,
    status: StatusCode,
    headers: HeaderMap,
    body: &[u8],
    redirect_chain: Vec<RedirectHop>,
    error: Option<String>,
) -> CrawlRecord {
    let redirect_target = redirect_chain.last().and_then(|hop| hop.location.clone());
    let redirect_type = redirect_chain.last().map(|hop| hop.status_code.to_string());
    let response_time_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let size_bytes = if body.is_empty() {
        header_string(&headers, CONTENT_LENGTH)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
    } else {
        body.len()
    };
    let response_hash = if body.is_empty() {
        None
    } else {
        Some(blake3::hash(body).to_hex().to_string())
    };
    let x_robots_tag = headers
        .get_all(HeaderName::from_static("x-robots-tag"))
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let x_robots_tag = (!x_robots_tag.is_empty()).then_some(x_robots_tag);
    let canonicals = canonical_link_headers(
        final_url,
        headers
            .get_all(LINK)
            .iter()
            .filter_map(|value| value.to_str().ok()),
    );
    let canonical_count = canonicals.len();
    let hsts_header = headers.contains_key(HeaderName::from_static("strict-transport-security"));
    let content_security_policy_header =
        headers.contains_key(HeaderName::from_static("content-security-policy"));
    let x_frame_options_header = headers.contains_key(HeaderName::from_static("x-frame-options"));
    let x_content_type_options_header =
        headers.contains_key(HeaderName::from_static("x-content-type-options"));

    CrawlRecord {
        id: 0,
        storage_key: final_url.to_string(),
        url: original_url.to_string(),
        final_url: final_url.to_string(),
        list_position: None,
        list_duplicate_index: 0,
        classification: if same_host(final_url, root_url) {
            UrlClassification::Internal
        } else {
            UrlClassification::External
        },
        in_sitemap: false,
        status_code: Some(status.as_u16()),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        content_type: header_string(&headers, CONTENT_TYPE),
        indexability: if status.is_success() {
            "Indexable".to_string()
        } else {
            "Non-indexable".to_string()
        },
        indexability_status: if status.is_success() {
            "Indexable".to_string()
        } else {
            format!("HTTP {}", status.as_u16())
        },
        response_time_ms,
        dns_lookup_time_ms: None,
        tcp_connect_time_ms: None,
        tls_handshake_time_ms: None,
        ttfb_ms: None,
        download_time_ms: None,
        total_network_time_ms: None,
        transfer_rate_bytes_per_sec: None,
        resolved_ip_count: 0,
        size_bytes,
        response_hash,
        depth,
        redirect_target,
        redirect_type,
        redirect_chain,
        title: None,
        title_count: None,
        title_len: 0,
        title_pixel_width: 0,
        meta_description: None,
        meta_description_count: None,
        meta_description_len: 0,
        meta_description_pixel_width: 0,
        meta_robots: None,
        x_robots_tag,
        h1: None,
        h1_len: 0,
        h1_count: 0,
        h2: None,
        h2_len: 0,
        h2_count: 0,
        canonical: canonicals.into_iter().next(),
        canonical_count,
        simhash: None,
        word_count: 0,
        text_to_code_ratio: 0.0,
        image_count: 0,
        images_missing_alt: 0,
        images_alt_too_long: 0,
        mixed_content_count: 0,
        insecure_form_count: 0,
        hsts_header,
        content_security_policy_header,
        x_frame_options_header,
        x_content_type_options_header,
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
        error,
    }
}

fn error_record(
    original_url: &Url,
    final_url: &Url,
    depth: usize,
    root_url: &Url,
    started_at: Instant,
    error: String,
    redirect_chain: Vec<RedirectHop>,
) -> CrawlRecord {
    let response_time_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    CrawlRecord {
        id: 0,
        storage_key: final_url.to_string(),
        url: original_url.to_string(),
        final_url: final_url.to_string(),
        list_position: None,
        list_duplicate_index: 0,
        classification: if same_host(final_url, root_url) {
            UrlClassification::Internal
        } else {
            UrlClassification::External
        },
        in_sitemap: false,
        status_code: None,
        status_text: "No response".to_string(),
        content_type: None,
        indexability: "Non-indexable".to_string(),
        indexability_status: "No response".to_string(),
        response_time_ms,
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
        redirect_target: redirect_chain.last().and_then(|hop| hop.location.clone()),
        redirect_type: redirect_chain.last().map(|hop| hop.status_code.to_string()),
        redirect_chain,
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
        error: Some(error),
    }
}

fn blocked_record(url: &Url, depth: usize, root_url: &Url) -> CrawlRecord {
    CrawlRecord {
        id: 0,
        storage_key: url.to_string(),
        url: url.to_string(),
        final_url: url.to_string(),
        list_position: None,
        list_duplicate_index: 0,
        classification: if same_host(url, root_url) {
            UrlClassification::Internal
        } else {
            UrlClassification::External
        },
        in_sitemap: false,
        status_code: None,
        status_text: "Blocked by robots.txt".to_string(),
        content_type: None,
        indexability: "Non-indexable".to_string(),
        indexability_status: "Blocked by robots.txt".to_string(),
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
        error: Some("Blocked by robots.txt".to_string()),
    }
}

fn redirect_location(headers: &HeaderMap) -> Option<String> {
    header_string(headers, LOCATION)
}

fn header_string(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn progress(
    status: &str,
    crawled: usize,
    queued: usize,
    discovered: usize,
    started_at: Instant,
    store: &impl CrawlStore,
) -> CrawlProgress {
    let elapsed = started_at.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();
    CrawlProgress {
        status: status.to_string(),
        crawled,
        queued,
        discovered,
        elapsed_ms: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
        pages_per_second: if elapsed_secs > 0.0 {
            crawled as f64 / elapsed_secs
        } else {
            0.0
        },
        summary: store.progress_summary(),
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn template_metadata_does_not_block_active_page_indexability_or_discovery() {
        const HTML: &str = "<html><head><template><title>Template title</title><meta name='description' content='Template description'><meta name='robots' content='noindex, nofollow'><meta name='viewport' content='fake'></template><title>Active title</title><meta name='description' content='Active description'></head><body><template><h1>Template heading</h1><h2>Template subheading</h2></template><h1>Active heading</h1><h2>Active subheading</h2><main>Selected content</main><a href='/target'>Active link</a></body></html>";
        let (base_url, _, server) = spawn_recording_site(|path| match path {
            "/" => response(200, "OK", "text/html", HTML),
            _ => response(
                200,
                "OK",
                "text/html",
                "<title>Target</title><main>Selected target</main>",
            ),
        })
        .await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: base_url.clone(),
                max_urls: 2,
                max_depth: 1,
                respect_robots: false,
                follow_internal_nofollow: Some(false),
                requests_per_second: 0,
                request_delay_ms: 0,
                sitemap: SitemapConfig {
                    enabled: false,
                    ..SitemapConfig::default()
                },
                content: ContentConfig {
                    include_selectors: vec!["main".into()],
                    exclude_selectors: Vec::new(),
                },
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let rows = store.records();
        let root = rows.iter().find(|row| row.url == base_url).unwrap();
        assert_eq!(root.indexability, "Indexable");
        assert_eq!(root.meta_robots, None);
        assert_eq!(root.title.as_deref(), Some("Active title"));
        assert_eq!(root.title_count, Some(1));
        assert_eq!(root.meta_description.as_deref(), Some("Active description"));
        assert_eq!(root.meta_description_count, Some(1));
        assert_eq!(root.h1.as_deref(), Some("Active heading"));
        assert_eq!(root.h1_count, 1);
        assert_eq!(root.h2.as_deref(), Some("Active subheading"));
        assert_eq!(root.h2_count, 1);
        assert!(!root.viewport);
        assert_eq!(root.word_count, 2);
        assert_eq!(
            root.response_hash,
            Some(blake3::hash(HTML.as_bytes()).to_hex().to_string())
        );
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.url.ends_with("/target")));
        server.abort();
    }

    #[tokio::test]
    async fn multiple_metadata_counts_reach_records_only_when_html_was_parsed() {
        let (base_url, _, server) = spawn_recording_site(|path| match path {
            "/multiple" => response(200, "OK", "text/html", "<title>First</title><title>Second</title><meta name=description content='First description'><meta name=description content='Second description'>"),
            "/zero" => response(200, "OK", "text/html", "<p>No metadata</p>"),
            "/image" => response(200, "OK", "image/png", "image bytes"),
            _ => response(200, "OK", "text/html", &"a".repeat(2048)),
        }).await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: format!("{base_url}multiple"),
                mode: CrawlMode::List,
                list_urls: ["multiple", "zero", "image", "large"]
                    .map(|path| format!("{base_url}{path}"))
                    .to_vec(),
                max_urls: 4,
                max_response_bytes: 1024,
                respect_robots: false,
                requests_per_second: 0,
                request_delay_ms: 0,
                sitemap: SitemapConfig {
                    enabled: false,
                    ..SitemapConfig::default()
                },
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let rows = store.records();
        assert_eq!(rows.len(), 4);
        for (path, expected) in [
            ("multiple", serde_json::json!(2)),
            ("zero", serde_json::json!(0)),
            ("image", serde_json::Value::Null),
            ("large", serde_json::Value::Null),
        ] {
            let row = rows.iter().find(|row| row.url.ends_with(path)).unwrap();
            let value = serde_json::to_value(row).unwrap();
            assert_eq!(value["titleCount"], expected, "{path}");
            assert_eq!(value["metaDescriptionCount"], expected, "{path}");
            if path == "multiple" {
                assert_eq!(row.title.as_deref(), Some("First"));
                assert_eq!(row.meta_description.as_deref(), Some("First description"));
            }
        }
        server.abort();
    }

    #[test]
    fn multiple_metadata_count_changes_are_rendered_dom_changes() {
        let base = Url::parse("https://example.test/").unwrap();
        let raw = parse_html_with_content(
            &base,
            "<title>Same</title><meta name=description content='Same'>",
            &ContentSelectors::default(),
        );
        for html in [
            "<title>Same</title><title>Extra</title><meta name=description content='Same'>",
            "<title>Same</title><meta name=description content='Same'><meta name=description content='Extra'>",
        ] {
            let rendered = parse_html_with_content(&base, html, &ContentSelectors::default());
            let mut record = CrawlRecord::pending(base.to_string(), 0);
            apply_rendered_dom_diff(&mut record, &raw, &rendered);
            assert!(record.rendered_dom_changed);
        }
    }

    use super::*;
    use ferrous_frog_storage::{GridQuery, IssueView, MemoryStore};
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn config_with_controls(mut config: CrawlConfig, controls: serde_json::Value) -> CrawlConfig {
        let mut value = serde_json::to_value(&config).unwrap();
        for (key, value_part) in controls.as_object().unwrap() {
            value[key] = value_part.clone();
        }
        config = serde_json::from_value(value).unwrap();
        config
    }

    fn retry_after_response(status: u16, retry_after: &str) -> String {
        let body = "<title>Temporarily unavailable</title>";
        format!(
            "HTTP/1.1 {status} Unavailable\r\nRetry-After: {retry_after}\r\nContent-Type: text/html\r\nStrict-Transport-Security: max-age=31536000\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn reference_links_default_off_for_legacy_and_partial_configs() {
        let mut value = serde_json::to_value(CrawlConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("referenceLinks");
        let restored: CrawlConfig = serde_json::from_value(value).unwrap();
        let value = serde_json::to_value(restored).unwrap();
        assert_eq!(
            value["referenceLinks"],
            serde_json::json!({"canonical":false,"hreflang":false,"pagination":false,"amp":false})
        );
        let config = config_with_controls(
            CrawlConfig::default(),
            serde_json::json!({"referenceLinks":{"canonical":true}}),
        );
        assert_eq!(
            serde_json::to_value(config).unwrap()["referenceLinks"],
            serde_json::json!({"canonical":true,"hreflang":false,"pagination":false,"amp":false})
        );
    }

    #[tokio::test]
    async fn reference_links_discover_each_type_without_changing_hyperlink_evidence() {
        let (base_url, requests, server) = spawn_recording_site(|path| {
            if path == "/" {
                response(200, "OK", "text/html", r#"<head>
                    <link rel="canonical" href="/html-canonical">
                    <link rel="canonical" href="/html-second">
                    <link rel="canonical" href="/shared">
                    <link rel="alternate" hreflang="en" href="/english">
                    <link rel="alternate" hreflang="bad_code" href="/invalid-language">
                    <link rel="next" href="/next"><link rel="previous" href="/previous"><link rel="prev" href="/older">
                    <link rel="amphtml NEXT" href="/amp-page">
                    <link rel="alternate" media="screen" href="/mobile">
                    </head><body><main>Selected text</main><footer><a href="/shared">Ordinary link</a></footer></body>"#)
                    .replacen("Content-Type:", "Link: </http-canonical>; rel=canonical\r\nLink: </html-canonical>; rel=canonical\r\nContent-Type:", 1)
            } else {
                response(200, "OK", "text/html", "<main>Target page</main><link rel='canonical' href='/deeper'>")
            }
        }).await;
        for selected in ["none", "canonical", "hreflang", "pagination", "amp", "all"] {
            requests.lock().unwrap().clear();
            let config = config_with_controls(
                CrawlConfig {
                    start_url: base_url.clone(),
                    max_depth: 1,
                    respect_robots: false,
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    sitemap: SitemapConfig {
                        enabled: false,
                        ..SitemapConfig::default()
                    },
                    content: ContentConfig {
                        include_selectors: vec!["main".into()],
                        ..ContentConfig::default()
                    },
                    ..CrawlConfig::default()
                },
                serde_json::json!({"referenceLinks": {
                    "canonical": selected == "canonical" || selected == "all",
                    "hreflang": selected == "hreflang" || selected == "all",
                    "pagination": selected == "pagination" || selected == "all",
                    "amp": selected == "amp" || selected == "all"
                }}),
            );
            let store = MemoryStore::new();
            crawl(config, store.clone(), CrawlControl::default(), |_| {})
                .await
                .unwrap();
            let mut expected = vec!["/", "/shared"];
            if matches!(selected, "canonical" | "all") {
                expected.extend(["/html-canonical", "/html-second", "/http-canonical"]);
            }
            if matches!(selected, "hreflang" | "all") {
                expected.extend(["/english", "/invalid-language"]);
            }
            if matches!(selected, "pagination" | "all") {
                expected.extend(["/next", "/previous", "/older", "/amp-page"]);
            } else if selected == "amp" {
                expected.push("/amp-page");
            }
            expected.sort_unstable();
            let mut actual = requests
                .lock()
                .unwrap()
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            actual.sort_unstable();
            assert_eq!(actual, expected, "{selected}");
            let records = store.records();
            let root = records.iter().find(|row| row.url == base_url).unwrap();
            assert_eq!(
                root.canonical.as_deref(),
                Some(format!("{base_url}html-canonical").as_str())
            );
            assert_eq!(root.canonical_count, 5);
            assert_eq!(root.hreflang_count, 2);
            assert_eq!(root.hreflang_invalid_count, 1);
            assert!(root.rel_next.as_deref().unwrap().ends_with("/next"));
            assert!(root.rel_prev.as_deref().unwrap().ends_with("/previous"));
            assert!(root.amphtml.as_deref().unwrap().ends_with("/amp-page"));
            assert_eq!(root.outlink_count, 1);
            assert_eq!(
                store
                    .link_edges(ferrous_frog_storage::LinkEdgeQuery::default())
                    .edges
                    .len(),
                1
            );
            for row in records.iter().filter(|row| row.url != base_url) {
                assert_eq!(row.depth, 1);
                if row.url.ends_with("/shared") {
                    assert_eq!(row.inlink_count, 1);
                    assert_eq!(
                        row.first_inlink_source_url.as_deref(),
                        Some(base_url.as_str())
                    );
                } else {
                    assert_eq!(row.inlink_count, 0, "{}", row.url);
                    assert_eq!(row.first_inlink_source_url, None);
                }
            }
        }
        server.abort();
    }

    fn reference_test_config(start_url: &str) -> CrawlConfig {
        CrawlConfig {
            start_url: start_url.into(),
            max_depth: 2,
            request_delay_ms: 0,
            requests_per_second: 0,
            respect_robots: false,
            retry_attempts: 0,
            sitemap: SitemapConfig {
                enabled: false,
                ..SitemapConfig::default()
            },
            reference_links: ReferenceLinksConfig {
                canonical: true,
                hreflang: true,
                pagination: true,
                amp: true,
            },
            ..CrawlConfig::default()
        }
    }

    #[tokio::test]
    async fn reference_links_honor_page_and_relation_nofollow_with_internal_override() {
        for source in ["rel", "meta", "header", "header-rel"] {
            let (base_url, requests, server) = spawn_recording_site(move |path| {
                if path != "/" {
                    return response(200, "OK", "text/plain", "Target");
                }
                let meta = if source == "meta" { "<meta name='robots' content='none'>" } else { "" };
                let rel = if matches!(source, "rel" | "header-rel") { "nofollow" } else { "" };
                let header_rel = if source == "header-rel" { "canonical nofollow" } else { "canonical" };
                let html = format!("<head>{meta}<link rel='canonical {rel}' href='/canonical'><link rel='alternate {rel}' hreflang='en' href='/hreflang'><link rel='next {rel}' href='/next'><link rel='amphtml {rel}' href='/amp'></head>");
                robots_response("text/html", &html, if source == "header" { &["nofollow"] } else { &[] })
                    .replacen("Content-Type:", &format!("Link: </header>; rel=\"{header_rel}\"\r\nContent-Type:"), 1)
            }).await;
            for follow in [false, true] {
                requests.lock().unwrap().clear();
                let store = MemoryStore::new();
                crawl(
                    CrawlConfig {
                        follow_nofollow: !follow,
                        follow_internal_nofollow: Some(follow),
                        ..reference_test_config(&base_url)
                    },
                    store.clone(),
                    CrawlControl::default(),
                    |_| {},
                )
                .await
                .unwrap();
                assert_eq!(
                    requests.lock().unwrap().len(),
                    if follow {
                        6
                    } else if source == "rel" {
                        2
                    } else {
                        1
                    },
                    "{source}, follow={follow}"
                );
                let records = store.records();
                let root = records.iter().find(|row| row.url == base_url).unwrap();
                assert_eq!(root.canonical_count, 2);
                assert_eq!(root.hreflang_count, 1);
                assert_eq!(root.outlink_count, 0);
                assert!(
                    store
                        .link_edges(ferrous_frog_storage::LinkEdgeQuery::default())
                        .edges
                        .is_empty()
                );
            }
            server.abort();
        }
    }

    #[tokio::test]
    async fn reference_links_preserve_scope_redirect_robots_query_and_resource_gates() {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/robots.txt" => response(200, "OK", "text/plain", "User-agent: *\nDisallow: /docs/blocked"),
            "/docs/start" => response(200, "OK", "text/html", "<link rel='canonical' href='/docs/redirect'><link rel='amphtml' href='/outside/check'><link rel='next' href='/docs/follow?utm=one'><link rel='prev' href='/docs/follow?utm=two'><link rel='canonical' href='/docs/excluded'><link rel='canonical' href='/docs/image.png'><link rel='canonical' href='/docs/map.xml'><link rel='alternate' hreflang='en' href='/docs/blocked'>"),
            "/docs/redirect" => redirect_response("/outside/final"),
            "/outside/check" => redirect_response("/docs/back"),
            "/outside/final" | "/docs/back" => response(200, "OK", "text/html", "<link rel='canonical' href='/docs/unwanted'><link rel='sitemap' href='/docs/unwanted.xml'><a href='/docs/also-unwanted'>Not recursive</a>"),
            "/docs/follow" => response(200, "OK", "text/html", "<link rel='next' href='/docs/leaf'>"),
            "/docs/leaf" => response(200, "OK", "text/html", "<link rel='next' href='/docs/too-deep'>"),
            "/docs/map.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/docs/from-map</loc></url></urlset>"),
            _ => response(200, "OK", "text/html", "<title>Unexpected</title>"),
        }).await;
        for other in [false, true] {
            requests.lock().unwrap().clear();
            let config = CrawlConfig {
                respect_robots: true,
                folder_scope: FolderScope::StartFolder,
                check_links_outside_start_folder: true,
                exclude_url_patterns: vec!["/excluded$".into()],
                query_settings: QuerySettings {
                    strip_parameter_patterns: vec!["^utm$".into()],
                    ..QuerySettings::default()
                },
                resource_types: CrawlResourceTypes {
                    other,
                    ..CrawlResourceTypes::default()
                },
                sitemap: SitemapConfig {
                    enabled: true,
                    discover_from_robots: false,
                    probe_default: false,
                    ..SitemapConfig::default()
                },
                ..reference_test_config(&format!("{base_url}docs/start"))
            };
            let store = MemoryStore::new();
            crawl(config, store.clone(), CrawlControl::default(), |_| {})
                .await
                .unwrap();
            let mut actual = requests
                .lock()
                .unwrap()
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            actual.sort_unstable();
            let mut expected = vec![
                "/robots.txt",
                "/docs/start",
                "/docs/redirect",
                "/outside/final",
                "/outside/check",
                "/docs/back",
                "/docs/follow",
                "/docs/leaf",
            ];
            if other {
                expected.push("/docs/map.xml");
            }
            expected.sort_unstable();
            assert_eq!(actual, expected, "Other={other}");
            let records = store.records();
            let blocked = records
                .iter()
                .find(|row| row.url.ends_with("/docs/blocked"))
                .unwrap();
            assert_eq!(blocked.status_text, "Blocked by robots.txt");
            let redirect = records
                .iter()
                .find(|row| row.url.ends_with("/docs/redirect"))
                .unwrap();
            assert!(redirect.final_url.ends_with("/outside/final"));
            assert_eq!(redirect.redirect_chain.len(), 1);
            assert_eq!(
                records
                    .iter()
                    .find(|row| row.url.ends_with("/docs/leaf"))
                    .unwrap()
                    .depth,
                2
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn reference_links_check_external_targets_only_with_external_and_nofollow_permission() {
        let (foreign_url, foreign_requests, foreign_server) = spawn_recording_site(|_| response(200, "OK", "text/html", "<link rel='canonical' href='/recursive'><link rel='sitemap' href='/recursive.xml'>")).await;
        let foreign_url = foreign_url.replace("127.0.0.1", "localhost");
        let (base_url, _, server) = spawn_recording_site(move |_| {
            response(
                200,
                "OK",
                "text/html",
                &format!("<link rel='canonical nofollow' href='{foreign_url}'>"),
            )
        })
        .await;
        for external in [false, true] {
            for follow in [false, true] {
                foreign_requests.lock().unwrap().clear();
                let store = MemoryStore::new();
                crawl(
                    CrawlConfig {
                        resource_types: CrawlResourceTypes {
                            external,
                            ..CrawlResourceTypes::default()
                        },
                        follow_internal_nofollow: Some(false),
                        follow_external_nofollow: Some(follow),
                        ..reference_test_config(&base_url)
                    },
                    store.clone(),
                    CrawlControl::default(),
                    |_| {},
                )
                .await
                .unwrap();
                let allowed = usize::from(external && follow);
                assert_eq!(foreign_requests.lock().unwrap().len(), allowed);
                assert_eq!(store.records().len(), 1 + allowed);
                assert!(
                    store
                        .link_edges(ferrous_frog_storage::LinkEdgeQuery::default())
                        .edges
                        .is_empty()
                );
            }
        }
        server.abort();
        foreign_server.abort();
    }

    #[tokio::test]
    async fn reference_links_keep_http_targets_on_non_html_and_incomplete_responses() {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/file.pdf" => response(200, "OK", "application/pdf", "PDF data")
                .replacen("Content-Type:", "Link: </first>; rel=canonical, </second>; rel=canonical\r\nContent-Type:", 1),
            "/incomplete" => "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 200\r\nLink: </first>; rel=canonical\r\nConnection: close\r\n\r\n<link rel='canonical' href='/truncated-html'>".into(),
            _ => response(200, "OK", "text/plain", "Target"),
        }).await;
        for (path, expected) in [("file.pdf", 3), ("incomplete", 2)] {
            requests.lock().unwrap().clear();
            let store = MemoryStore::new();
            let seed = format!("{base_url}{path}");
            crawl(
                reference_test_config(&seed),
                store.clone(),
                CrawlControl::default(),
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(requests.lock().unwrap().len(), expected);
            assert!(
                !requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(path, _)| path == "/truncated-html")
            );
            let records = store.records();
            let source = records.iter().find(|row| row.url == seed).unwrap();
            assert!(source.canonical.as_deref().unwrap().ends_with("/first"));
            assert_eq!(source.canonical_count, expected - 1);
            if path == "incomplete" {
                assert_eq!(source.indexability_status, "Response body incomplete");
                assert!(source.title.is_none());
            }
        }
        server.abort();
    }

    #[tokio::test]
    async fn reference_links_obey_list_exact_url_and_limits_and_survive_stop_resume() {
        let (base_url, requests, server) = spawn_recording_site(|_| {
            response(
                200,
                "OK",
                "text/html",
                "<link rel='canonical' href='/first'><link rel='canonical' href='/second'>",
            )
        })
        .await;
        let base_config = reference_test_config(&base_url);
        for config in [
            CrawlConfig {
                mode: CrawlMode::List,
                ..base_config.clone()
            },
            CrawlConfig {
                folder_scope: FolderScope::ExactUrl,
                ..base_config.clone()
            },
            CrawlConfig {
                max_depth: 0,
                ..base_config.clone()
            },
            CrawlConfig {
                max_urls: 1,
                ..base_config.clone()
            },
        ] {
            requests.lock().unwrap().clear();
            crawl(config, MemoryStore::new(), CrawlControl::default(), |_| {})
                .await
                .unwrap();
            assert_eq!(requests.lock().unwrap().len(), 1);
        }
        requests.lock().unwrap().clear();
        let store = MemoryStore::new();
        let control = CrawlControl::default();
        let cancel = control.clone();
        let progress = crawl(
            CrawlConfig {
                concurrency: 1,
                ..base_config.clone()
            },
            store.clone(),
            control,
            move |event| {
                if event.kind == "record" {
                    cancel.cancel();
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(progress.status, "stopped");
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert_eq!(store.load_frontier_state().unwrap().queued.len(), 2);
        crawl(
            CrawlConfig {
                resume_from_state: true,
                ..base_config
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert_eq!(store.records().len(), 3);
        assert!(store.load_frontier_state().is_none());
        assert!(store.records().iter().all(|row| row.inlink_count == 0));
        server.abort();
    }

    #[test]
    fn retry_after_parses_seconds_dates_and_repeated_fields_without_overflow_or_early_retries() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        for (value, expected) in [
            ("0", Some(Duration::ZERO)),
            (" 005 ", Some(Duration::from_secs(5))),
            (
                "Sun, 09 Sep 2001 01:46:42 GMT",
                Some(Duration::from_secs(2)),
            ),
            ("Sun, 06 Nov 1994 08:49:37 GMT", Some(Duration::ZERO)),
            (
                "99999999999999999999999999999",
                Some(Duration::from_secs(u64::MAX)),
            ),
            ("+1", None),
            ("-1", None),
            ("1.5", None),
            ("later", None),
            ("", None),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(RETRY_AFTER, value.parse().unwrap());
            assert_eq!(retry_after_delay(&headers, now), expected, "{value}");
        }
        let mut headers = HeaderMap::new();
        for value in ["2", "invalid", "5", "0"] {
            headers.append(RETRY_AFTER, value.parse().unwrap());
        }
        assert_eq!(
            retry_after_delay(&headers, now),
            Some(Duration::from_secs(5))
        );
    }

    #[tokio::test]
    async fn retry_after_http_date_on_503_delays_until_the_requested_time() {
        let date = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(2));
        let requested_time = httpdate::parse_http_date(&date).unwrap();
        let retried_at = Arc::new(std::sync::Mutex::new(None));
        let captured_time = retried_at.clone();
        let attempts = Arc::new(AtomicUsize::new(0));
        let captured_attempts = attempts.clone();
        let (base_url, _, server) = spawn_recording_site(move |_| {
            if captured_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                retry_after_response(503, &date)
            } else {
                *captured_time.lock().unwrap() = Some(SystemTime::now());
                response(200, "OK", "text/html", "<title>Recovered</title>")
            }
        })
        .await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                respect_robots: false,
                sitemap: SitemapConfig {
                    enabled: false,
                    ..SitemapConfig::default()
                },
                retry_attempts: 1,
                retry_backoff_ms: 0,
                request_delay_ms: 0,
                requests_per_second: 0,
                timeout_secs: 1,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(retried_at.lock().unwrap().unwrap() >= requested_time);
        assert_eq!(store.records()[0].status_code, Some(200));
        server.abort();
    }

    #[tokio::test]
    async fn retry_after_is_shared_with_workers_already_waiting_for_origin_pacing() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let captured = attempts.clone();
        let (base_url, requests, server) = spawn_recording_site(move |_| {
            if captured.fetch_add(1, Ordering::SeqCst) == 0 {
                retry_after_response(429, "1")
            } else {
                response(200, "OK", "text/html", "<title>Ready</title>")
            }
        })
        .await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                mode: CrawlMode::List,
                start_url: base_url.clone(),
                list_urls: vec![format!("{base_url}limited"), format!("{base_url}queued")],
                concurrency: 2,
                max_urls: 2,
                respect_robots: false,
                retry_attempts: 1,
                retry_backoff_ms: 0,
                request_delay_ms: 75,
                requests_per_second: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[1].1.duration_since(requests[0].1) >= Duration::from_millis(990));
        assert!(requests[2].1.duration_since(requests[1].1) >= Duration::from_millis(65));
        assert!(
            store
                .records()
                .iter()
                .all(|row| row.status_code == Some(200))
        );
        server.abort();
    }

    #[tokio::test]
    async fn retry_after_extensions_hold_already_waiting_requests_until_the_latest_deadline() {
        let (base_url, requests, server) =
            spawn_recording_site(|_| response(200, "OK", "text/html", "Ready")).await;
        let url = Url::parse(&base_url).unwrap();
        let policy = Arc::new(RequestPolicy {
            origins: Default::default(),
            rate_limiter: None,
            respect_robots: false,
            header_origin: url.origin(),
            request_headers: HeaderMap::new(),
            on_event: Arc::new(|_| {}),
        });
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, "1".parse().unwrap());
        policy
            .observe_retry_after(&url, StatusCode::TOO_MANY_REQUESTS, &headers)
            .await;
        let task_policy = policy.clone();
        let target = url.clone();
        let handle = tokio::spawn(async move {
            task_policy
                .send(
                    &Client::new(),
                    &CrawlConfig::default(),
                    &target,
                    0,
                    &CrawlControl::default(),
                    false,
                )
                .await
                .unwrap()
                .response
                .unwrap()
        });
        sleep(Duration::from_millis(100)).await;
        let extended_at = Instant::now();
        policy
            .observe_retry_after(&url, StatusCode::SERVICE_UNAVAILABLE, &headers)
            .await;
        headers.insert(RETRY_AFTER, "0".parse().unwrap());
        policy
            .observe_retry_after(&url, StatusCode::TOO_MANY_REQUESTS, &headers)
            .await;
        headers.insert(RETRY_AFTER, "3600".parse().unwrap());
        policy
            .observe_retry_after(&url, StatusCode::INTERNAL_SERVER_ERROR, &headers)
            .await;
        let response = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            requests.lock().unwrap()[0].1.duration_since(extended_at) >= Duration::from_millis(990)
        );
        server.abort();
    }

    #[tokio::test]
    async fn retry_after_is_origin_local_and_survives_an_exhausted_retry_budget() {
        let (foreign_url, foreign_requests, foreign_server) =
            spawn_recording_site(|_| response(200, "OK", "text/html", "Ready")).await;
        let (base_url, requests, server) = spawn_recording_site(|path| {
            if path == "/limited" {
                retry_after_response(429, "1")
            } else {
                response(200, "OK", "text/html", "Ready")
            }
        })
        .await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                mode: CrawlMode::List,
                start_url: base_url.clone(),
                list_urls: vec![
                    format!("{base_url}limited"),
                    foreign_url,
                    format!("{base_url}next"),
                ],
                max_urls: 3,
                concurrency: 1,
                respect_robots: false,
                retry_attempts: 0,
                retry_backoff_ms: 0,
                request_delay_ms: 0,
                requests_per_second: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            foreign_requests.lock().unwrap()[0]
                .1
                .duration_since(requests[0].1)
                < Duration::from_millis(500)
        );
        assert!(requests[1].1.duration_since(requests[0].1) >= Duration::from_millis(990));
        let rows = store.records();
        assert_eq!(rows.len(), 3);
        let limited = rows
            .iter()
            .find(|row| row.url.ends_with("/limited"))
            .unwrap();
        assert_eq!(limited.status_code, Some(429));
        assert_eq!(limited.indexability_status, "HTTP 429");
        assert!(limited.hsts_header);
        assert_eq!(limited.title.as_deref(), Some("Temporarily unavailable"));
        server.abort();
        foreign_server.abort();
    }

    #[tokio::test]
    async fn retry_after_fallback_backoff_and_attempt_budget_preserve_final_response() {
        for header in ["invalid", "Sun, 06 Nov 1994 08:49:37 GMT", "0"] {
            let (base_url, requests, server) =
                spawn_recording_site(move |_| retry_after_response(503, header)).await;
            let store = MemoryStore::new();
            crawl(
                CrawlConfig {
                    start_url: base_url,
                    max_urls: 1,
                    respect_robots: false,
                    sitemap: SitemapConfig {
                        enabled: false,
                        ..SitemapConfig::default()
                    },
                    retry_attempts: 2,
                    retry_backoff_ms: 20,
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    ..CrawlConfig::default()
                },
                store.clone(),
                CrawlControl::default(),
                |_| {},
            )
            .await
            .unwrap();
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(requests[1].1.duration_since(requests[0].1) >= Duration::from_millis(15));
            assert!(requests[2].1.duration_since(requests[1].1) >= Duration::from_millis(35));
            let record = &store.records()[0];
            assert_eq!(record.status_code, Some(503));
            assert_eq!(record.content_type.as_deref(), Some("text/html"));
            assert!(record.hsts_header);
            assert_eq!(record.title.as_deref(), Some("Temporarily unavailable"));
            server.abort();
        }
    }

    #[tokio::test]
    async fn retry_after_applies_to_robots_and_sitemap_fetches() {
        let attempts = Arc::new(std::sync::Mutex::new(HashMap::<String, usize>::new()));
        let counted = attempts.clone();
        let (base_url, requests, server) = spawn_recording_site(move |path| {
            let mut attempts = counted.lock().unwrap();
            let attempt = attempts.entry(path.into()).or_default();
            *attempt += 1;
            match path {
                "/robots.txt" if *attempt == 1 => retry_after_response(429, "1"),
                "/robots.txt" => response(200, "OK", "text/plain", "User-agent: *\nAllow: /\n"),
                "/sitemap.xml" if *attempt == 1 => retry_after_response(503, "1"),
                "/sitemap.xml" => response(
                    200,
                    "OK",
                    "application/xml",
                    "<urlset><url><loc>/from-map</loc></url></urlset>",
                ),
                _ => response(200, "OK", "text/html", "Ready"),
            }
        })
        .await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: base_url,
                max_urls: 2,
                retry_attempts: 1,
                retry_backoff_ms: 0,
                request_delay_ms: 0,
                requests_per_second: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let requests = requests.lock().unwrap();
        for path in ["/robots.txt", "/sitemap.xml"] {
            let attempts = requests
                .iter()
                .filter(|(requested, _)| requested == path)
                .collect::<Vec<_>>();
            assert_eq!(attempts.len(), 2, "{path}");
            assert!(
                attempts[1].1.duration_since(attempts[0].1) >= Duration::from_millis(990),
                "{path}"
            );
        }
        assert!(
            store
                .records()
                .iter()
                .any(|row| row.url.ends_with("/from-map") && row.in_sitemap)
        );
        server.abort();
    }

    #[tokio::test]
    async fn retry_after_waits_do_not_issue_requests_while_paused() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let (base_url, requests, server) = spawn_recording_site(move |_| {
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                retry_after_response(429, "1")
            } else {
                response(200, "OK", "text/html", "Ready")
            }
        })
        .await;
        let control = CrawlControl::default();
        let store = MemoryStore::new();
        let handle = tokio::spawn(crawl(
            CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                respect_robots: false,
                sitemap: SitemapConfig {
                    enabled: false,
                    ..SitemapConfig::default()
                },
                retry_attempts: 1,
                retry_backoff_ms: 0,
                request_delay_ms: 0,
                requests_per_second: 0,
                timeout_secs: 1,
                ..CrawlConfig::default()
            },
            store.clone(),
            control.clone(),
            |_| {},
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while requests.lock().unwrap().is_empty() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        control.pause();
        sleep(Duration::from_millis(1150)).await;
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        control.resume();
        let result = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result.status, "finished");
        assert_eq!(store.records()[0].status_code, Some(200));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn retry_after_seconds_preserve_redirect_identity_and_delay_the_retry() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let captured = attempts.clone();
        let (base_url, requests, server) = spawn_recording_site(move |path| match path {
            "/start" => redirect_response("/limited"),
            "/limited" if captured.fetch_add(1, Ordering::SeqCst) == 0 => {
                retry_after_response(429, "1")
            }
            _ => response(200, "OK", "text/html", "<title>Recovered</title>"),
        })
        .await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: format!("{base_url}start"),
                max_urls: 1,
                respect_robots: false,
                sitemap: SitemapConfig {
                    enabled: false,
                    ..SitemapConfig::default()
                },
                retry_attempts: 1,
                retry_backoff_ms: 0,
                request_delay_ms: 0,
                requests_per_second: 0,
                timeout_secs: 3,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let requests = requests.lock().unwrap();
        let attempts = requests
            .iter()
            .filter(|(path, _)| path == "/limited")
            .collect::<Vec<_>>();
        assert_eq!(attempts.len(), 2);
        assert!(attempts[1].1.duration_since(attempts[0].1) >= Duration::from_millis(990));
        let record = &store.records()[0];
        assert_eq!(record.status_code, Some(200));
        assert_eq!(record.url, format!("{base_url}start"));
        assert_eq!(record.final_url, format!("{base_url}limited"));
        assert_eq!(record.redirect_chain.len(), 1);
        assert_eq!(record.title.as_deref(), Some("Recovered"));
        server.abort();
    }

    #[tokio::test]
    async fn retry_after_overflow_waits_until_stop_without_consuming_the_frontier() {
        let (base_url, requests, server) =
            spawn_recording_site(|_| retry_after_response(503, "99999999999999999999999999999"))
                .await;
        let store = MemoryStore::new();
        let control = CrawlControl::default();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_events = events.clone();
        let handle = tokio::spawn(crawl(
            CrawlConfig {
                mode: CrawlMode::List,
                start_url: base_url.clone(),
                list_urls: vec![format!("{base_url}limited"), format!("{base_url}next")],
                max_urls: 2,
                concurrency: 1,
                respect_robots: false,
                retry_attempts: 2,
                retry_backoff_ms: 0,
                request_delay_ms: 0,
                requests_per_second: 0,
                timeout_secs: 1,
                ..CrawlConfig::default()
            },
            store.clone(),
            control.clone(),
            move |event| captured_events.lock().unwrap().push(event),
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while requests.lock().unwrap().is_empty() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        sleep(Duration::from_millis(75)).await;
        assert_eq!(requests.lock().unwrap().len(), 1);
        control.cancel();
        let progress = tokio::time::timeout(Duration::from_millis(300), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(progress.status, "stopped");
        assert!(store.records().is_empty());
        assert_eq!(store.load_frontier_state().unwrap().queued.len(), 2);
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event.kind == "notice"
                    && event.message.as_deref().unwrap().contains("Retry-After"))
        );
        server.abort();
    }

    #[test]
    fn content_regions_default_for_saved_configs_and_validate_selectors() {
        let mut value = serde_json::to_value(CrawlConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("content");
        let restored: CrawlConfig = serde_json::from_value(value).unwrap();
        assert_eq!(
            serde_json::to_value(restored).unwrap()["content"],
            serde_json::json!({"includeSelectors": [], "excludeSelectors": []})
        );
        for field in ["includeSelectors", "excludeSelectors"] {
            for selector in ["[", "", "   "] {
                let config = config_with_controls(
                    CrawlConfig::default(),
                    serde_json::json!({"content": {field: [selector]}}),
                );
                assert!(
                    validate_configuration(&config).is_err(),
                    "{field}: {selector}"
                );
            }
        }
        let config = config_with_controls(
            CrawlConfig::default(),
            serde_json::json!({"content": {"includeSelectors": ["main, article"]}}),
        );
        assert!(validate_configuration(&config).is_ok());
    }

    #[tokio::test]
    async fn content_regions_change_text_metrics_without_hiding_crawl_or_raw_evidence() {
        const WORDS: &str = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone";
        let html = |path: &str| {
            format!(
                "<html><head><title>Page {path}</title><meta name='robots' content='noindex'><link rel='canonical' href='/canonical'></head><body><h1>Outside heading</h1><nav><a href='/second'>Outside link</a></nav><main><section class='copy'>{WORDS}</section><aside><a href='/third'>Excluded link</a></aside><script>ignored script words</script></main><footer>Footer {path}</footer></body></html>"
            )
        };
        let root_html = html("/");
        let (base_url, _, server) =
            spawn_recording_site(move |path| response(200, "OK", "text/html", &html(path))).await;
        let config = config_with_controls(
            CrawlConfig {
                start_url: base_url.clone(),
                max_urls: 3,
                max_depth: 1,
                respect_robots: false,
                requests_per_second: 0,
                request_delay_ms: 0,
                sitemap: SitemapConfig {
                    enabled: false,
                    ..SitemapConfig::default()
                },
                custom_extractors: vec![CustomExtractor {
                    name: "Footer".into(),
                    kind: ferrous_frog_extractors::ExtractorKind::CssText,
                    pattern: "footer".into(),
                    attribute: None,
                    all_matches: false,
                }],
                custom_searches: vec![CustomSearch {
                    name: "Script".into(),
                    pattern: "ignored script words".into(),
                    regex: false,
                    case_sensitive: true,
                    max_snippets: 1,
                }],
                ..CrawlConfig::default()
            },
            serde_json::json!({"content": {"includeSelectors": ["main", ".copy"], "excludeSelectors": ["aside"]}}),
        );
        let store = MemoryStore::new();
        crawl(
            config.clone(),
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let rows = store.records();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|row| row.url.ends_with("/second")));
        assert!(rows.iter().any(|row| row.url.ends_with("/third")));
        for row in &rows {
            assert_eq!(row.word_count, 21);
            assert_eq!(row.simhash, Some(simhash::simhash(WORDS)));
            assert_eq!(
                row.near_duplicate_cluster_id,
                rows[0].near_duplicate_cluster_id
            );
            assert_eq!(row.h1.as_deref(), Some("Outside heading"));
            assert_eq!(row.meta_robots.as_deref(), Some("noindex"));
            assert!(row.canonical.as_deref().unwrap().ends_with("/canonical"));
            assert!(row.custom_extractions[0].values[0].starts_with("Footer"));
            assert!(row.custom_searches[0].matched);
        }
        let root = rows.iter().find(|row| row.url == base_url).unwrap();
        assert_eq!(
            root.text_to_code_ratio,
            WORDS.len() as f64 / root_html.len() as f64
        );
        assert_eq!(
            root.response_hash,
            Some(blake3::hash(root_html.as_bytes()).to_hex().to_string())
        );
        assert_ne!(rows[0].response_hash, rows[1].response_hash);

        let config = config_with_controls(
            config,
            serde_json::json!({"content": {"includeSelectors": [".missing"]}}),
        );
        let no_matches = MemoryStore::new();
        crawl(config, no_matches.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(no_matches.records().len(), 3);
        for row in no_matches.records() {
            assert_eq!(row.word_count, 0);
            assert_eq!(row.text_to_code_ratio, 0.0);
            assert_eq!(row.simhash, None);
            assert_eq!(row.near_duplicate_cluster_id, None);
        }
        server.abort();
    }

    #[test]
    fn legacy_configs_restore_sitemap_and_download_defaults() {
        let mut value = serde_json::to_value(CrawlConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("sitemap");
        value.as_object_mut().unwrap().remove("maxResponseBytes");
        let restored: CrawlConfig = serde_json::from_value(value).unwrap();
        let value = serde_json::to_value(restored).unwrap();
        assert_eq!(value["maxResponseBytes"], 20 * 1024 * 1024);
        assert_eq!(
            value["sitemap"],
            serde_json::json!({
                "enabled": true, "discoverFromRobots": true, "probeDefault": true,
                "followLinked": true, "urls": []
            })
        );
        let config = config_with_controls(
            CrawlConfig::default(),
            serde_json::json!({
                "sitemap": { "enabled": false }
            }),
        );
        let value = serde_json::to_value(config).unwrap();
        assert_eq!(value["sitemap"]["enabled"], false);
        assert_eq!(value["sitemap"]["followLinked"], true);
    }

    #[test]
    fn download_and_sitemap_controls_validate_at_the_config_boundary() {
        for limit in [0_u64, 1_073_741_825] {
            let config = config_with_controls(
                CrawlConfig::default(),
                serde_json::json!({
                    "maxResponseBytes": limit
                }),
            );
            assert!(validate_configuration(&config).is_err(), "limit: {limit}");
        }
        for limit in [1_u64, 1_073_741_824] {
            let config = config_with_controls(
                CrawlConfig::default(),
                serde_json::json!({
                    "maxResponseBytes": limit
                }),
            );
            assert!(validate_configuration(&config).is_ok());
        }
        let config = config_with_controls(
            CrawlConfig::default(),
            serde_json::json!({
                "sitemap": { "urls": ["file:///tmp/sitemap.xml"] }
            }),
        );
        assert!(validate_configuration(&config).is_err());
    }

    #[tokio::test]
    async fn response_limits_preserve_status_without_parsing_incomplete_html() {
        let (base_url, _, server) = spawn_recording_site(|path| match path {
            "/declared-error" => response(404, "Not Found", "text/html", &"x".repeat(65)),
            "/declared-noindex" => robots_response("text/html", &"x".repeat(65), &["noindex"]),
            "/declared" => response(200, "OK", "text/html", &"x".repeat(65)),
            "/chunked" => format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n20\r\n{}\r\n21\r\n{}\r\n0\r\n\r\n", "a".repeat(32), "b".repeat(33)),
            "/interrupted" => "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 64\r\nConnection: close\r\n\r\n<title>Incomplete</title>".into(),
            _ => response(200, "OK", "text/html", &format!("<title>Complete</title>{}", " ".repeat(41))),
        }).await;
        let store = MemoryStore::new();
        let config = config_with_controls(
            CrawlConfig {
                mode: CrawlMode::List,
                start_url: base_url.clone(),
                list_urls: [
                    "declared",
                    "declared-error",
                    "declared-noindex",
                    "chunked",
                    "interrupted",
                    "exact",
                ]
                .map(|path| format!("{base_url}{path}"))
                .to_vec(),
                respect_robots: false,
                request_delay_ms: 0,
                requests_per_second: 0,
                retry_attempts: 0,
                ..CrawlConfig::default()
            },
            serde_json::json!({ "maxResponseBytes": 64 }),
        );
        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        let rows = store.records();
        assert_eq!(rows.len(), 6);
        for row in &rows {
            assert_eq!(
                row.status_code,
                Some(if row.url.ends_with("/declared-error") {
                    404
                } else {
                    200
                })
            );
            assert_eq!(row.content_type.as_deref(), Some("text/html"));
            if row.url.ends_with("/exact") {
                assert_eq!(row.title.as_deref(), Some("Complete"));
                assert!(row.error.is_none());
                assert_eq!(row.size_bytes, 64);
            } else {
                assert!(row.error.is_some(), "{}", row.url);
                assert!(row.title.is_none());
                assert!(row.response_hash.is_none());
                assert_eq!(row.indexability_status, "Response body incomplete");
                assert_eq!(
                    row.indexability,
                    if row.url.ends_with("/declared-error")
                        || row.url.ends_with("/declared-noindex")
                    {
                        "Non-indexable"
                    } else {
                        "Unknown"
                    }
                );
                if !row.url.ends_with("/interrupted") {
                    assert!(row.error.as_deref().unwrap().contains("64 bytes"));
                }
            }
        }
        assert_eq!(store.summary().title_missing, 0);
        server.abort();
    }

    #[tokio::test]
    async fn robots_and_explicit_sitemaps_use_the_response_limit() {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/robots.txt" => response(
                200,
                "OK",
                "text/plain",
                &format!("User-agent: *\n{}\nDisallow: /\n", "#".repeat(65)),
            ),
            "/oversized.xml" => response(
                200,
                "OK",
                "application/xml",
                &format!("<urlset><url><loc>/{}</loc></url></urlset>", "a".repeat(65)),
            ),
            _ => response(200, "OK", "text/html", "<title>Page</title>"),
        })
        .await;
        let config = config_with_controls(
            CrawlConfig {
                start_url: base_url.clone(),
                folder_scope: FolderScope::ExactUrl,
                request_delay_ms: 0,
                requests_per_second: 0,
                ..CrawlConfig::default()
            },
            serde_json::json!({ "maxResponseBytes": 64 }),
        );
        let store = MemoryStore::new();
        crawl(
            config.clone(),
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(
            store.records()[0]
                .error
                .as_deref()
                .unwrap()
                .contains("64 bytes")
        );
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .all(|(path, _)| path == "/robots.txt")
        );
        let config = CrawlConfig {
            mode: CrawlMode::List,
            respect_robots: false,
            list_sitemap_urls: vec![format!("{base_url}oversized.xml")],
            ..config
        };
        let error = crawl(config, MemoryStore::new(), CrawlControl::default(), |_| {})
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("64 bytes"));
        server.abort();
    }

    #[tokio::test]
    async fn sitemap_url_budget_applies_after_page_scope_and_query_normalization() {
        let (base_url, _, server) = spawn_recording_site(|path| match path {
            "/sitemap.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/outside-one</loc></url><url><loc>/outside-two</loc></url><url><loc>/docs/orphan?tracking=1</loc></url><url><loc>/docs/orphan?tracking=2</loc></url><url><loc>/docs/second</loc></url></urlset>"),
            _ => response(200, "OK", "text/html", "<title>Page</title>"),
        }).await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: format!("{base_url}docs/"),
                max_urls: 3,
                folder_scope: FolderScope::StartFolder,
                query_settings: QuerySettings {
                    strip_all: true,
                    ..QuerySettings::default()
                },
                sitemap: SitemapConfig {
                    discover_from_robots: false,
                    ..SitemapConfig::default()
                },
                respect_robots: false,
                request_delay_ms: 0,
                requests_per_second: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let paths = store
            .records()
            .iter()
            .map(|row| Url::parse(&row.url).unwrap().path().to_string())
            .collect::<HashSet<_>>();
        assert_eq!(
            paths,
            HashSet::from([
                "/docs/".into(),
                "/docs/orphan".into(),
                "/docs/second".into()
            ])
        );
        server.abort();
    }

    #[tokio::test]
    async fn stop_interrupts_initial_sitemap_sources_without_a_seed_url() {
        let (base_url, server) = spawn_slow_page_site(Duration::from_secs(5)).await;
        let control = CrawlControl::default();
        let task_control = control.clone();
        let handle = tokio::spawn(async move {
            crawl(
                CrawlConfig {
                    mode: CrawlMode::List,
                    start_url: String::new(),
                    list_sitemap_urls: vec![base_url],
                    respect_robots: false,
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    ..CrawlConfig::default()
                },
                MemoryStore::new(),
                task_control,
                |_| {},
            )
            .await
        });
        sleep(Duration::from_millis(50)).await;
        control.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result.status, "stopped");
        assert_eq!(result.crawled, 0);
        server.abort();
    }

    #[tokio::test]
    async fn decoded_response_bytes_are_bounded_even_when_gzip_is_small() {
        // gzip of <title>Decoded</title> followed by 300 ASCII 'a' bytes.
        const GZIP: &[u8] = &[
            31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 179, 41, 201, 44, 201, 73, 181, 115, 73, 77, 206,
            79, 73, 77, 177, 209, 135, 112, 19, 71, 1, 209, 0, 0, 54, 248, 167, 203, 66, 1, 0, 0,
        ];
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 1024];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", GZIP.len()).as_bytes()).await.unwrap();
                stream.write_all(GZIP).await.unwrap();
            }
        });
        let client = Client::new();
        let response = client.get(&url).send().await.unwrap();
        let error = read_response_body(response, 64, &CrawlControl::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("64 bytes"));
        let response = client.get(&url).send().await.unwrap();
        let bytes = read_response_body(response, 322, &CrawlControl::default())
            .await
            .unwrap();
        assert_eq!(bytes.len(), 322);
        assert!(bytes.starts_with(b"<title>Decoded</title>"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn response_reader_drains_in_flight_bodies_while_paused_and_stops_when_cancelled() {
        for cancel in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let control = CrawlControl::default();
            let server_control = control.clone();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 1024];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\ntest\r\n").await.unwrap();
                server_control.pause();
                sleep(Duration::from_millis(30)).await;
                if cancel {
                    server_control.cancel();
                    sleep(Duration::from_secs(5)).await;
                } else {
                    stream.write_all(b"0\r\n\r\n").await.unwrap();
                }
            });
            let client = Client::builder()
                .timeout(Duration::from_millis(500))
                .build()
                .unwrap();
            let response = client.get(url).send().await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_millis(300),
                read_response_body(response, 4, &control),
            )
            .await
            .unwrap();
            if cancel {
                assert_eq!(result.unwrap_err().to_string(), CRAWL_CANCELLED_MESSAGE);
            } else {
                assert_eq!(result.unwrap(), b"test");
                assert!(control.is_paused());
            }
            server.abort();
        }
    }

    #[tokio::test]
    async fn stopping_linked_sitemap_discovery_keeps_the_source_page_resumable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let waiting = Arc::new(tokio::sync::Notify::new());
        let server_waiting = waiting.clone();
        let sitemap_requests = Arc::new(AtomicUsize::new(0));
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let waiting = server_waiting.clone();
                let sitemap_requests = sitemap_requests.clone();
                tokio::spawn(async move {
                    let mut request = [0; 2048];
                    let read = stream.read(&mut request).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    let request = String::from_utf8_lossy(&request[..read]);
                    let path = request
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap();
                    let payload = if path == "/only.xml" {
                        if sitemap_requests.fetch_add(1, Ordering::SeqCst) == 0 {
                            waiting.notify_one();
                            sleep(Duration::from_secs(5)).await;
                        }
                        response(
                            200,
                            "OK",
                            "application/xml",
                            "<urlset><url><loc>/orphan</loc></url></urlset>",
                        )
                    } else if path == "/" {
                        response(
                            200,
                            "OK",
                            "text/html",
                            "<title>Seed</title><link rel='sitemap' href='/only.xml'>",
                        )
                    } else {
                        response(200, "OK", "text/html", "<title>Orphan</title>")
                    };
                    let _ = stream.write_all(payload.as_bytes()).await;
                });
            }
        });
        let config = CrawlConfig {
            start_url: base_url.clone(),
            sitemap: SitemapConfig {
                discover_from_robots: false,
                probe_default: false,
                ..SitemapConfig::default()
            },
            respect_robots: false,
            request_delay_ms: 0,
            requests_per_second: 0,
            concurrency: 1,
            ..CrawlConfig::default()
        };
        let store = MemoryStore::new();
        let control = CrawlControl::default();
        let handle = tokio::spawn(crawl(
            config.clone(),
            store.clone(),
            control.clone(),
            |_| {},
        ));
        tokio::time::timeout(Duration::from_secs(1), waiting.notified())
            .await
            .unwrap();
        control.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result.status, "stopped");
        assert!(
            store
                .load_frontier_state()
                .unwrap()
                .queued
                .iter()
                .any(|item| item.url == base_url)
        );
        crawl(
            CrawlConfig {
                resume_from_state: true,
                ..config
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let rows = store.records();
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .any(|row| row.url.ends_with("/orphan") && row.in_sitemap)
        );
        server.abort();
    }

    #[tokio::test]
    async fn non_sitemap_xml_links_still_produce_resource_and_broken_link_records() {
        let (base_url, _, server) = spawn_recording_site(|path| match path {
            "/" => response(200, "OK", "text/html", "<title>Seed</title><a href='/missing.xml'>Missing</a><a href='/feed.xml'>Feed</a><a href='/missing.xml'>Duplicate</a>"),
            "/feed.xml" => response(200, "OK", "application/xml", "<rss><channel><title>Feed</title></channel></rss>"),
            _ => response(404, "Not Found", "application/xml", "<error>Missing</error>"),
        }).await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: base_url,
                sitemap: SitemapConfig {
                    discover_from_robots: false,
                    probe_default: false,
                    ..SitemapConfig::default()
                },
                resource_types: CrawlResourceTypes {
                    other: true,
                    ..CrawlResourceTypes::default()
                },
                respect_robots: false,
                request_delay_ms: 0,
                requests_per_second: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let rows = store.records();
        assert_eq!(rows.len(), 3);
        assert!(
            rows.iter()
                .any(|row| row.url.ends_with("/missing.xml") && row.status_code == Some(404))
        );
        assert!(
            rows.iter()
                .any(|row| row.url.ends_with("/feed.xml") && row.status_code == Some(200))
        );
        assert_eq!(store.summary().broken, 1);
        server.abort();
    }

    #[tokio::test]
    async fn discovering_sitemaps_does_not_enforce_ignored_robots_delays() {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/robots.txt" => response(
                200,
                "OK",
                "text/plain",
                "User-agent: *\nDisallow: /\nCrawl-delay: 60\nSitemap: /map.xml\n",
            ),
            "/map.xml" => response(
                200,
                "OK",
                "application/xml",
                "<urlset><url><loc>/orphan</loc></url></urlset>",
            ),
            _ => response(200, "OK", "text/html", "<title>Page</title>"),
        })
        .await;
        let store = MemoryStore::new();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            crawl(
                CrawlConfig {
                    start_url: base_url,
                    sitemap: SitemapConfig {
                        probe_default: false,
                        ..SitemapConfig::default()
                    },
                    respect_robots: false,
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    ..CrawlConfig::default()
                },
                store.clone(),
                CrawlControl::default(),
                |_| {},
            ),
        )
        .await;
        assert!(
            result.is_ok(),
            "Ignored robots rules must not delay page/sitemap requests"
        );
        result.unwrap().unwrap();
        assert_eq!(store.records().len(), 2);
        assert_eq!(
            requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path == "/robots.txt")
                .count(),
            1
        );
        server.abort();
    }

    #[tokio::test]
    async fn sitemap_indexes_share_document_depth_and_url_limits() {
        for (source, max_urls, expected_documents, expected_rows) in [
            ("wide.xml", 4, 128, 1),
            ("deep-0.xml", 4, 5, 1),
            ("urls.xml", 4, 1, 4),
        ] {
            let (base_url, requests, server) = spawn_recording_site(|path| {
                let body = if path == "/wide.xml" {
                    format!(
                        "<sitemapindex>{}</sitemapindex>",
                        (0..200)
                            .map(|number| format!(
                                "<sitemap><loc>/map-{number}.xml</loc></sitemap>"
                            ))
                            .collect::<String>()
                    )
                } else if let Some(number) = path
                    .strip_prefix("/deep-")
                    .and_then(|path| path.strip_suffix(".xml"))
                {
                    format!(
                        "<sitemapindex><sitemap><loc>/deep-{}.xml</loc></sitemap></sitemapindex>",
                        number.parse::<usize>().unwrap() + 1
                    )
                } else if path == "/urls.xml" {
                    format!(
                        "<urlset>{}</urlset>",
                        (0..200)
                            .map(|number| format!("<url><loc>/page-{number}</loc></url>"))
                            .collect::<String>()
                    )
                } else if path.ends_with(".xml") {
                    "<urlset/>".into()
                } else {
                    "<title>Page</title>".into()
                };
                response(
                    200,
                    "OK",
                    if path.ends_with(".xml") {
                        "application/xml"
                    } else {
                        "text/html"
                    },
                    &body,
                )
            })
            .await;
            let store = MemoryStore::new();
            crawl(
                CrawlConfig {
                    start_url: base_url.clone(),
                    max_urls,
                    sitemap: SitemapConfig {
                        urls: vec![
                            format!("{base_url}{source}"),
                            format!("{base_url}{source}#duplicate"),
                        ],
                        discover_from_robots: false,
                        probe_default: false,
                        follow_linked: false,
                        ..SitemapConfig::default()
                    },
                    respect_robots: false,
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    ..CrawlConfig::default()
                },
                store.clone(),
                CrawlControl::default(),
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(store.records().len(), expected_rows, "{source}");
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(path, _)| path.ends_with(".xml"))
                    .count(),
                expected_documents,
                "{source}"
            );
            assert!(
                !requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(path, _)| path == "/deep-5.xml")
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn sitemap_redirects_preserve_robots_pacing_and_detect_loops() {
        for source in ["loop.xml", "denied.xml"] {
            let (base_url, requests, server) = spawn_recording_site(|path| match path {
                "/robots.txt" => response(
                    200,
                    "OK",
                    "text/plain",
                    "User-agent: *\nDisallow: /blocked.xml\nCrawl-delay: 0.03\n",
                ),
                "/loop.xml" => redirect_response("/loop-next.xml"),
                "/loop-next.xml" => redirect_response("/loop.xml"),
                "/denied.xml" => redirect_response("/blocked.xml"),
                _ => response(200, "OK", "application/xml", "<urlset/>"),
            })
            .await;
            let error = crawl(
                CrawlConfig {
                    start_url: base_url.clone(),
                    sitemap: SitemapConfig {
                        urls: vec![format!("{base_url}{source}")],
                        discover_from_robots: false,
                        probe_default: false,
                        ..SitemapConfig::default()
                    },
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    ..CrawlConfig::default()
                },
                MemoryStore::new(),
                CrawlControl::default(),
                |_| {},
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains(if source == "loop.xml" {
                "redirect loop"
            } else {
                "Blocked by robots.txt"
            }));
            let requests = requests.lock().unwrap();
            assert!(!requests.iter().any(|(path, _)| path == "/blocked.xml"));
            assert_eq!(
                requests
                    .iter()
                    .filter(|(path, _)| path == "/robots.txt")
                    .count(),
                1
            );
            assert!(
                requests
                    .windows(2)
                    .all(|pair| pair[1].1.duration_since(pair[0].1) >= Duration::from_millis(25))
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn linked_sitemap_requests_keep_source_queries_while_page_queries_are_normalized() {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/" => response(
                200,
                "OK",
                "text/html",
                "<title>Seed</title><a rel='sitemap' href='/map?part=one'>Map</a>",
            ),
            "/map?part=one" => response(
                200,
                "OK",
                "application/xml",
                "<urlset><url><loc>/orphan?tracking=one</loc></url></urlset>",
            ),
            "/orphan" => response(200, "OK", "text/html", "<title>Orphan</title>"),
            _ => response(404, "Not Found", "text/plain", "not found"),
        })
        .await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: base_url,
                sitemap: SitemapConfig {
                    discover_from_robots: false,
                    probe_default: false,
                    ..SitemapConfig::default()
                },
                query_settings: QuerySettings {
                    strip_all: true,
                    ..QuerySettings::default()
                },
                respect_robots: false,
                request_delay_ms: 0,
                requests_per_second: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(
            store
                .records()
                .iter()
                .any(|row| row.url.ends_with("/orphan") && row.in_sitemap)
        );
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .any(|(path, _)| path == "/map?part=one")
        );
        server.abort();
    }

    #[tokio::test]
    async fn spider_sitemap_sources_obey_controls_and_share_deduplication() {
        for (options, expected_paths) in [
            (
                serde_json::json!({}),
                vec![
                    "/explicit-page",
                    "/robots-page",
                    "/default-page",
                    "/nested-page",
                    "/linked-page",
                    "/anchor-page",
                ],
            ),
            (serde_json::json!({ "enabled": false }), vec![]),
            (
                serde_json::json!({ "discoverFromRobots": false, "probeDefault": false, "followLinked": false }),
                vec!["/explicit-page", "/nested-page"],
            ),
            (
                serde_json::json!({ "discoverFromRobots": false, "probeDefault": false }),
                vec![
                    "/explicit-page",
                    "/nested-page",
                    "/linked-page",
                    "/anchor-page",
                ],
            ),
        ] {
            let (base_url, requests, server) = spawn_recording_site(|path| match path {
                "/robots.txt" => response(200, "OK", "text/plain", "User-agent: *\nAllow: /\nSitemap: /robots-map.xml\nSitemap: /explicit.xml#duplicate\n"),
                "/" => response(200, "OK", "text/html", "<title>Seed</title><link rel='sitemap' href='/linked-map'><a href='/anchor.xml'>Map</a>"),
                "/explicit.xml" => response(200, "OK", "application/xml", "<sitemapindex><sitemap><loc>/nested.xml</loc></sitemap></sitemapindex>"),
                "/nested.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/nested-page</loc></url><url><loc>/explicit-page</loc></url></urlset>"),
                "/direct.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/explicit-page</loc></url><url><loc>/</loc></url></urlset>"),
                "/robots-map.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/robots-page</loc></url><url><loc>/explicit-page</loc></url></urlset>"),
                "/sitemap.xml" => redirect_response("/default.xml"),
                "/default.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/default-page</loc></url></urlset>"),
                "/linked-map" => response(200, "OK", "application/xml", "<urlset><url><loc>/linked-page</loc></url><url><loc>/</loc></url></urlset>"),
                "/anchor.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/anchor-page</loc></url></urlset>"),
                _ => response(200, "OK", "text/html", "<title>Page</title>"),
            }).await;
            let mut options = options;
            options["urls"] = serde_json::json!([
                format!("{base_url}explicit.xml"),
                format!("{base_url}direct.xml")
            ]);
            let config = config_with_controls(
                CrawlConfig {
                    start_url: base_url.clone(),
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    max_urls: 30,
                    ..CrawlConfig::default()
                },
                serde_json::json!({ "sitemap": options }),
            );
            let store = MemoryStore::new();
            crawl(config, store.clone(), CrawlControl::default(), |_| {})
                .await
                .unwrap();
            let rows = store.records();
            let actual_paths = rows
                .iter()
                .filter(|row| row.url != base_url)
                .map(|row| Url::parse(&row.url).unwrap().path().to_string())
                .collect::<HashSet<_>>();
            assert_eq!(
                actual_paths,
                expected_paths.iter().map(|path| (*path).into()).collect()
            );
            if !expected_paths.is_empty() {
                assert!(rows.iter().all(|row| row.in_sitemap));
            }
            let requests = requests.lock().unwrap();
            for path in [
                "/robots.txt",
                "/explicit.xml",
                "/direct.xml",
                "/nested.xml",
                "/linked-map",
                "/anchor.xml",
            ] {
                assert!(
                    requests
                        .iter()
                        .filter(|(requested, _)| requested == path)
                        .count()
                        <= 1,
                    "duplicate {path}: {requests:?}"
                );
            }
            server.abort();
        }
    }

    #[tokio::test]
    async fn linked_sitemap_pages_preserve_scope_and_existing_record_provenance() {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/docs/" => response(200, "OK", "text/html", "<title>Seed</title><a href='/docs/child'>Child</a>"),
            "/docs/child" => response(200, "OK", "text/html", "<title>Child</title><link rel='sitemap' href='/docs/pages-map'>"),
            "/docs/pages-map" => response(200, "OK", "application/xml", "<urlset><url><loc>/docs/</loc></url><url><loc>/docs/orphan</loc></url><url><loc>/outside</loc></url><url><loc>https://example.invalid/external</loc></url></urlset>"),
            _ => response(200, "OK", "text/html", "<title>Orphan</title>"),
        }).await;
        let config = config_with_controls(
            CrawlConfig {
                start_url: format!("{base_url}docs/"),
                respect_robots: false,
                folder_scope: FolderScope::StartFolder,
                request_delay_ms: 0,
                requests_per_second: 0,
                concurrency: 1,
                ..CrawlConfig::default()
            },
            serde_json::json!({ "sitemap": { "discoverFromRobots": false, "probeDefault": false } }),
        );
        let store = MemoryStore::new();
        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        let rows = store.records();
        assert_eq!(rows.len(), 3);
        assert!(
            rows.iter()
                .find(|row| row.url.ends_with("/docs/"))
                .unwrap()
                .in_sitemap
        );
        assert!(
            rows.iter()
                .find(|row| row.url.ends_with("/docs/orphan"))
                .unwrap()
                .in_sitemap
        );
        assert!(
            !requests
                .lock()
                .unwrap()
                .iter()
                .any(|(path, _)| path == "/outside")
        );
        server.abort();
    }

    #[tokio::test]
    async fn explicit_sitemap_failures_are_visible_and_exact_url_avoids_discovery() {
        for body in ["not a sitemap", "<urlset><url><loc>/partial</loc></url>"] {
            let (base_url, requests, server) = spawn_recording_site(move |path| {
                response(
                    200,
                    "OK",
                    if path == "/bad.xml" {
                        "application/xml"
                    } else {
                        "text/html"
                    },
                    body,
                )
            })
            .await;
            let config = config_with_controls(
                CrawlConfig {
                    start_url: base_url.clone(),
                    respect_robots: false,
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    ..CrawlConfig::default()
                },
                serde_json::json!({ "sitemap": { "urls": [format!("{base_url}bad.xml")], "discoverFromRobots": false, "probeDefault": false } }),
            );
            let result = crawl(
                config.clone(),
                MemoryStore::new(),
                CrawlControl::default(),
                |_| {},
            )
            .await;
            assert!(
                result.is_err(),
                "Malformed explicit sources must report errors"
            );
            assert!(format!("{:#}", result.unwrap_err()).contains("bad.xml"));
            requests.lock().unwrap().clear();
            let store = MemoryStore::new();
            crawl(
                CrawlConfig {
                    folder_scope: FolderScope::ExactUrl,
                    ..config
                },
                store.clone(),
                CrawlControl::default(),
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(store.records().len(), 1);
            assert!(
                !requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(path, _)| path == "/bad.xml")
            );
            server.abort();
        }
    }

    #[test]
    fn chrome_request_defaults_preserve_explicit_saved_headers_and_user_agents() {
        const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36";
        let expected_headers = serde_json::json!([
            {"name":"Accept","value":"text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8"},
            {"name":"Accept-Language","value":"en-US,en;q=0.9"},
            {"name":"Upgrade-Insecure-Requests","value":"1"},
        ]);
        let value = serde_json::to_value(CrawlConfig::default()).unwrap();
        assert_eq!(value["userAgent"], CHROME_UA);
        assert_eq!(value["requestHeaders"], expected_headers);
        let mut legacy = value;
        legacy
            .as_object_mut()
            .unwrap()
            .remove("checkLinksOutsideStartFolder");
        legacy.as_object_mut().unwrap().remove("requestHeaders");
        legacy.as_object_mut().unwrap().remove("userAgent");
        let restored =
            serde_json::to_value(serde_json::from_value::<CrawlConfig>(legacy.clone()).unwrap())
                .unwrap();
        assert_eq!(restored["checkLinksOutsideStartFolder"], false);
        assert_eq!(restored["requestHeaders"], expected_headers);
        assert_eq!(restored["userAgent"], CHROME_UA);
        let mut download = serde_json::json!({"url":"https://example.test/", "userAgent":CHROME_UA, "timeoutSecs":2});
        let request: RobotsTxtDownloadRequest = serde_json::from_value(download.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(request.request_headers).unwrap(),
            expected_headers
        );
        download["requestHeaders"] = serde_json::json!([]);
        let request: RobotsTxtDownloadRequest = serde_json::from_value(download).unwrap();
        assert!(request.request_headers.is_empty());
        for (user_agent, headers) in [
            (
                "FerrousFrogSeoSpider/0.1 (+https://example.invalid/ferrous-frog)",
                serde_json::json!([]),
            ),
            (
                "SavedCrawler/1.0",
                serde_json::json!([
                    {"name":"accept-language","value":"tr-TR"},
                    {"name":"X-Environment","value":"preview"}
                ]),
            ),
        ] {
            legacy["userAgent"] = serde_json::json!(user_agent);
            legacy["requestHeaders"] = headers.clone();
            let config = serde_json::from_value::<CrawlConfig>(legacy.clone()).unwrap();
            validate_configuration(&config).unwrap();
            let saved = serde_json::to_value(normalize_config(config)).unwrap();
            assert_eq!(saved["userAgent"], user_agent);
            assert_eq!(saved["requestHeaders"], headers);
        }
    }

    #[tokio::test]
    async fn outside_folder_checks_are_terminal_and_preserve_filters_robots_and_link_evidence() {
        let (external_url, external_requests, external_server) =
            spawn_recording_site(|_| response(200, "OK", "text/html", "<title>External</title>"))
                .await;
        let (base_url, requests, server) = spawn_recording_site(move |path| match path {
            "/robots.txt" => response(200, "OK", "text/plain", "User-agent: *\nDisallow: /blocked\n"),
            "/docs/" => response(200, "OK", "text/html", &format!("<title>Seed</title><a href='/outside'>Outside</a><a href='/outside'>Duplicate</a><a href='/outside-back'>Back</a><a href='/docs/inside-redirect'>Redirect</a><a href='/docs/child'>Child</a><a href='/outside-map.xml'>Map</a><a rel='nofollow' href='/nofollow'>Nofollow</a><a href='/blocked'>Blocked</a><a href='/excluded'>Excluded</a><a href='{}'>External</a>", external_url.replace("127.0.0.1", "localhost"))),
            "/outside-back" => redirect_response("/docs/back"),
            "/docs/inside-redirect" => redirect_response("/outside-final"),
            "/docs/child" => response(200, "OK", "text/html", "<title>Child</title><a href='/docs/deep'>Deep</a>"),
            "/outside-map.xml" => response(200, "OK", "application/xml", "<sitemapindex><sitemap><loc>/docs/only.xml</loc></sitemap></sitemapindex>"),
            "/outside" | "/docs/back" | "/outside-final" => response(200, "OK", "text/html", "<title>Diagnostic</title><a href='/docs/escaped'>Inside link</a><a href='/far'>Far</a><link rel='sitemap' href='/docs/only.xml'>"),
            "/docs/only.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/docs/sitemap-escaped</loc></url></urlset>"),
            _ => response(200, "OK", "text/html", "<title>Page</title>"),
        }).await;
        let config = config_with_controls(
            CrawlConfig {
                start_url: format!("{base_url}docs/"),
                folder_scope: FolderScope::StartFolder,
                subdomain_scope: SubdomainScope::ExactHost,
                exclude_url_patterns: vec!["/excluded$".into()],
                follow_internal_nofollow: Some(false),
                resource_types: CrawlResourceTypes {
                    other: true,
                    ..CrawlResourceTypes::default()
                },
                sitemap: SitemapConfig {
                    discover_from_robots: false,
                    probe_default: false,
                    ..SitemapConfig::default()
                },
                requests_per_second: 0,
                request_delay_ms: 0,
                ..CrawlConfig::default()
            },
            serde_json::json!({ "checkLinksOutsideStartFolder": true }),
        );
        let store = MemoryStore::new();
        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        let rows = store.records();
        for suffix in [
            "/outside",
            "/outside-back",
            "/docs/inside-redirect",
            "/outside-map.xml",
            "/docs/deep",
        ] {
            assert!(
                rows.iter().any(|row| row.url.ends_with(suffix)),
                "missing {suffix}"
            );
        }
        assert!(
            rows.iter()
                .any(|row| row.url.ends_with("/blocked") && row.status_code.is_none())
        );
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|(path, _)| path == "/outside")
                .count(),
            1
        );
        for path in [
            "/docs/escaped",
            "/far",
            "/docs/only.xml",
            "/docs/sitemap-escaped",
            "/nofollow",
            "/blocked",
            "/excluded",
        ] {
            assert!(
                !requests.iter().any(|(requested, _)| requested == path),
                "unexpected {path}"
            );
        }
        assert!(external_requests.lock().unwrap().is_empty());
        assert!(
            store
                .link_edges(Default::default())
                .edges
                .iter()
                .any(|edge| edge.source_url.ends_with("/outside")
                    && edge.target_url.ends_with("/docs/escaped"))
        );
        server.abort();
        external_server.abort();
    }

    #[tokio::test]
    async fn outside_folder_checks_only_apply_to_start_folder_and_resume_keeps_diagnostics_terminal()
     {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/docs/" => response(
                200,
                "OK",
                "text/html",
                "<title>Seed</title><a href='/outside'>Outside</a>",
            ),
            "/outside" => response(
                200,
                "OK",
                "text/html",
                "<title>Outside</title><a href='/docs/escaped'>Escaped</a>",
            ),
            _ => response(200, "OK", "text/html", "<title>Page</title>"),
        })
        .await;
        let base = CrawlConfig {
            start_url: format!("{base_url}docs/"),
            respect_robots: false,
            request_delay_ms: 0,
            requests_per_second: 0,
            sitemap: SitemapConfig {
                enabled: false,
                ..SitemapConfig::default()
            },
            ..CrawlConfig::default()
        };
        for scope in [FolderScope::ExactFolder, FolderScope::ExactUrl] {
            let store = MemoryStore::new();
            let config = config_with_controls(
                CrawlConfig {
                    folder_scope: scope,
                    ..base.clone()
                },
                serde_json::json!({ "checkLinksOutsideStartFolder": true }),
            );
            crawl(config, store.clone(), CrawlControl::default(), |_| {})
                .await
                .unwrap();
            assert_eq!(store.records().len(), 1);
        }
        let store = MemoryStore::new();
        store.save_frontier_state(CrawlFrontierState {
            queued: vec![CrawlFrontierItem {
                url: format!("{base_url}outside"),
                depth: 1,
                from_sitemap: false,
                storage_key: format!("{base_url}outside"),
                list_position: None,
                list_duplicate_index: 0,
            }],
            seen: vec![base.start_url.clone(), format!("{base_url}outside")],
            crawled: 1,
        });
        let config = config_with_controls(
            CrawlConfig {
                folder_scope: FolderScope::StartFolder,
                resume_from_state: true,
                ..base
            },
            serde_json::json!({ "checkLinksOutsideStartFolder": true }),
        );
        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(store.records().len(), 1);
        assert!(store.records()[0].url.ends_with("/outside"));
        assert!(
            !requests
                .lock()
                .unwrap()
                .iter()
                .any(|(path, _)| path == "/docs/escaped")
        );
        server.abort();
    }

    #[test]
    fn request_header_validation_rejects_secrets_transport_overrides_and_ambiguous_headers() {
        for (name, value) in [
            ("Authorization", "test-value"),
            ("Cookie", "test-value"),
            ("Proxy-Authorization", "test-value"),
            ("X-Api-Key", "test-value"),
            ("X-Auth-Token", "test-value"),
            ("X-Session-Id", "test-value"),
            ("X-CSRF", "test-value"),
            ("X-Access-Key-Id", "test-value"),
            ("Host", "test-value"),
            ("Content-Length", "123"),
            ("Transfer-Encoding", "chunked"),
            ("User-Agent", "other"),
            ("Connection", "close"),
            ("Sec-Fetch-Site", "same-origin"),
            ("X-Forwarded-For", "test-value"),
            ("Bad Name", "test-value"),
            ("X-Test", "test-value\r\nInjected: yes"),
            ("X-Test", "test-value\0"),
        ] {
            let config = config_with_controls(
                CrawlConfig::default(),
                serde_json::json!({ "requestHeaders": [{"name":name,"value":value}] }),
            );
            let error = validate_configuration(&config).expect_err(name).to_string();
            assert!(!error.contains("test-value"));
        }
        let config = config_with_controls(
            CrawlConfig::default(),
            serde_json::json!({ "requestHeaders": [{"name":"X-Variant","value":"a"},{"name":"x-variant","value":"b"}] }),
        );
        assert!(validate_configuration(&config).is_err());
        let config = config_with_controls(
            CrawlConfig::default(),
            serde_json::json!({ "requestHeaders": [{"name":"Accept-Language","value":"en-GB"},{"name":"X-Environment","value":"preview"}] }),
        );
        assert!(validate_configuration(&config).is_ok());
    }

    #[tokio::test]
    async fn chrome_request_defaults_and_explicit_empty_headers_are_origin_restricted_on_wire() {
        let foreign_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = foreign_headers.clone();
        let (foreign_url, _, foreign_server) =
            spawn_recording_site_with_request(move |path, request| {
                captured.lock().unwrap().push(request.to_string());
                if path == "/robots.txt" {
                    response(200, "OK", "text/plain", "User-agent: *\nAllow: /\n")
                } else {
                    response(200, "OK", "text/html", "<title>Foreign</title>")
                }
            })
            .await;
        let local_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = local_headers.clone();
        let (base_url, _, server) = spawn_recording_site_with_request(move |path, request| {
            captured
                .lock()
                .unwrap()
                .push((path.to_string(), request.to_string()));
            match path {
                "/robots.txt" => redirect_response("/rules"),
                "/rules" => response(200, "OK", "text/plain", "User-agent: *\nAllow: /\n"),
                "/source.xml" => redirect_response("/map.xml"),
                "/map.xml" => response(
                    200,
                    "OK",
                    "application/xml",
                    "<urlset><url><loc>/from-map</loc></url></urlset>",
                ),
                "/redirect" => redirect_response("/landing"),
                "/cross" => redirect_response(&foreign_url),
                _ => response(200, "OK", "text/html", "<title>Local</title>"),
            }
        })
        .await;
        for explicit_empty in [false, true] {
            let mut config = CrawlConfig {
                mode: CrawlMode::List,
                start_url: base_url.clone(),
                list_urls: vec![format!("{base_url}redirect"), format!("{base_url}cross")],
                list_sitemap_urls: vec![format!("{base_url}source.xml")],
                request_delay_ms: 0,
                requests_per_second: 0,
                ..CrawlConfig::default()
            };
            if explicit_empty {
                config.request_headers.clear();
                config.user_agent = "SavedCrawler/1.0".into();
            }
            let user_agent = config.user_agent.clone();
            crawl(config, MemoryStore::new(), CrawlControl::default(), |_| {})
                .await
                .unwrap();
            let local = local_headers.lock().unwrap();
            for path in [
                "/robots.txt",
                "/rules",
                "/source.xml",
                "/map.xml",
                "/from-map",
                "/redirect",
                "/landing",
                "/cross",
            ] {
                let (_, request) = local
                    .iter()
                    .find(|(requested, _)| requested == path)
                    .unwrap();
                assert_eq!(
                    request_header_value(request, "user-agent"),
                    Some(user_agent.as_str())
                );
                if explicit_empty {
                    assert_eq!(request_header_value(request, "accept-language"), None);
                    assert_eq!(
                        request_header_value(request, "upgrade-insecure-requests"),
                        None
                    );
                    assert!(
                        request_header_value(request, "accept").is_none_or(|value| value == "*/*")
                    );
                } else {
                    assert_eq!(
                        request_header_value(request, "accept"),
                        Some(
                            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8"
                        )
                    );
                    assert_eq!(
                        request_header_value(request, "accept-language"),
                        Some("en-US,en;q=0.9")
                    );
                    assert_eq!(
                        request_header_value(request, "upgrade-insecure-requests"),
                        Some("1")
                    );
                }
                assert!(!request.to_ascii_lowercase().contains("\r\nsec-"));
            }
            drop(local);
            let foreign = foreign_headers.lock().unwrap();
            assert!(!foreign.is_empty());
            for request in &*foreign {
                assert_eq!(
                    request_header_value(request, "user-agent"),
                    Some(user_agent.as_str())
                );
                assert_eq!(request_header_value(request, "accept-language"), None);
                assert_eq!(
                    request_header_value(request, "upgrade-insecure-requests"),
                    None
                );
                assert!(request_header_value(request, "accept").is_none_or(|value| value == "*/*"));
            }
            drop(foreign);
            local_headers.lock().unwrap().clear();
            foreign_headers.lock().unwrap().clear();
        }
        server.abort();
        foreign_server.abort();
    }

    #[tokio::test]
    async fn custom_request_headers_follow_only_the_seed_origin_across_pages_robots_and_sitemaps() {
        let foreign_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_foreign = foreign_headers.clone();
        let (foreign_url, _, foreign_server) =
            spawn_recording_site_with_request(move |path, request| {
                captured_foreign
                    .lock()
                    .unwrap()
                    .push((path.to_string(), request.to_string()));
                match path {
                    "/robots.txt" | "/delegated-robots" => {
                        response(200, "OK", "text/plain", "User-agent: *\nAllow: /\n")
                    }
                    "/map.xml" => response(
                        200,
                        "OK",
                        "application/xml",
                        "<urlset><url><loc>/foreign-page</loc></url></urlset>",
                    ),
                    _ => response(200, "OK", "text/html", "<title>Foreign</title>"),
                }
            })
            .await;
        let local_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_local = local_headers.clone();
        let foreign_target = foreign_url.clone();
        let (base_url, _, server) = spawn_recording_site_with_request(move |path, request| {
            captured_local
                .lock()
                .unwrap()
                .push((path.to_string(), request.to_string()));
            match path {
                "/robots.txt" => redirect_response("/local-robots"),
                "/local-robots" => redirect_response(&format!("{foreign_target}delegated-robots")),
                "/source.xml" => redirect_response("/local.xml"),
                "/local.xml" => response(
                    200,
                    "OK",
                    "application/xml",
                    "<urlset><url><loc>/from-map</loc></url></urlset>",
                ),
                "/remote.xml" => redirect_response(&format!("{foreign_target}map.xml")),
                "/redirect" => redirect_response("/landing"),
                "/cross" => redirect_response(&format!("{foreign_target}landing")),
                _ => response(200, "OK", "text/html", "<title>Local</title>"),
            }
        })
        .await;
        let config = config_with_controls(
            CrawlConfig {
                mode: CrawlMode::List,
                start_url: base_url.clone(),
                list_urls: vec![
                    format!("{base_url}redirect"),
                    format!("{base_url}cross"),
                    format!("{foreign_url}direct"),
                ],
                list_sitemap_urls: vec![
                    format!("{base_url}source.xml"),
                    format!("{base_url}remote.xml"),
                ],
                request_delay_ms: 0,
                requests_per_second: 0,
                ..CrawlConfig::default()
            },
            serde_json::json!({ "userAgent":"HeaderFixture/1.0", "requestHeaders": [
                {"name":"X-Environment","value":"preview"},
                {"name":"Accept-Language","value":"en-GB"},
                {"name":"Accept","value":"application/x-fixture"},
                {"name":"Upgrade-Insecure-Requests","value":"0"}
            ] }),
        );
        let store = MemoryStore::new();
        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(store.records().len(), 5);
        let local_headers = local_headers.lock().unwrap();
        for path in [
            "/robots.txt",
            "/local-robots",
            "/source.xml",
            "/local.xml",
            "/remote.xml",
            "/redirect",
            "/landing",
            "/cross",
            "/from-map",
        ] {
            let (_, request) = local_headers
                .iter()
                .find(|(requested, _)| requested == path)
                .unwrap();
            let request = request.to_ascii_lowercase();
            assert!(
                request.contains("x-environment: preview"),
                "missing custom header at {path}"
            );
            assert!(
                request.contains("accept-language: en-gb"),
                "missing language at {path}"
            );
            assert_eq!(
                request_header_value(&request, "user-agent"),
                Some("headerfixture/1.0")
            );
            assert_eq!(
                request_header_value(&request, "accept"),
                Some("application/x-fixture")
            );
            assert_eq!(
                request_header_value(&request, "upgrade-insecure-requests"),
                Some("0")
            );
        }
        let foreign_headers = foreign_headers.lock().unwrap();
        for path in [
            "/delegated-robots",
            "/robots.txt",
            "/map.xml",
            "/landing",
            "/direct",
            "/foreign-page",
        ] {
            let (_, request) = foreign_headers
                .iter()
                .find(|(requested, _)| requested == path)
                .unwrap();
            let request = request.to_ascii_lowercase();
            assert!(
                !request.contains("x-environment:"),
                "custom header leaked to {path}"
            );
            assert!(
                !request.contains("accept-language:"),
                "language leaked to {path}"
            );
            assert_eq!(
                request_header_value(&request, "user-agent"),
                Some("headerfixture/1.0")
            );
            assert_ne!(
                request_header_value(&request, "accept"),
                Some("application/x-fixture")
            );
            assert_eq!(
                request_header_value(&request, "upgrade-insecure-requests"),
                None
            );
        }
        server.abort();
        foreign_server.abort();
    }

    #[tokio::test]
    async fn robots_download_headers_do_not_follow_cross_origin_redirects() {
        let foreign_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_foreign = foreign_headers.clone();
        let (foreign_url, _, foreign_server) =
            spawn_recording_site_with_request(move |_, request| {
                captured_foreign.lock().unwrap().push(request.to_string());
                response(200, "OK", "text/plain", "User-agent: *\nAllow: /\n")
            })
            .await;
        let local_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_local = local_headers.clone();
        let (base_url, _, server) = spawn_recording_site_with_request(move |_, request| {
            captured_local.lock().unwrap().push(request.to_string());
            redirect_response(&foreign_url)
        })
        .await;
        let request = serde_json::from_value(serde_json::json!({
            "url": base_url, "userAgent": "HeaderFixture", "timeoutSecs": 2,
            "requestHeaders": [{"name":"X-Environment","value":"preview"}]
        }))
        .unwrap();
        let result = download_robots_txt(request).await.unwrap();
        assert!(result.robots_txt.contains("Allow: /"));
        assert!(
            local_headers.lock().unwrap()[0]
                .to_ascii_lowercase()
                .contains("x-environment: preview")
        );
        assert!(
            !foreign_headers.lock().unwrap()[0]
                .to_ascii_lowercase()
                .contains("x-environment:")
        );
        server.abort();
        foreign_server.abort();
    }

    #[tokio::test]
    async fn http_canonicals_apply_to_documents_and_combine_with_html_signals() {
        let (base_url, _, server) = spawn_recording_site(|path| {
            let (status, content_type, body, headers) = match path {
                "/file.pdf" => (200, "application/pdf", "%PDF-fixture", "Link: </download>; rel=alternate, </canonical>; title=\"A, B\"; rel=canonical\r\n"),
                "/html" => (200, "text/html", "<link rel='canonical' href='/html'>", "Link: </different>; rel=canonical\r\n"),
                "/self" => (200, "text/html", "<title>Self</title>", "Link: </self>; rel=canonical\r\n"),
                "/repeated" => (200, "application/pdf", "%PDF-fixture", "Link: </one>; rel=canonical\r\nLink: </two>; rel=canonical\r\n"),
                _ => (404, "text/html", "Missing", "Link: </canonical>; rel=canonical\r\n"),
            };
            format!("HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        }).await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                mode: CrawlMode::List,
                start_url: base_url.clone(),
                list_urls: ["file.pdf", "html", "self", "repeated", "missing"]
                    .map(|path| format!("{base_url}{path}"))
                    .to_vec(),
                requests_per_second: 100,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        server.abort();
        let rows = store.records();
        let row = |path: &str| {
            rows.iter()
                .find(|row| row.url == format!("{base_url}{path}"))
                .unwrap()
        };
        assert_eq!(
            row("file.pdf").canonical,
            Some(format!("{base_url}canonical"))
        );
        assert_eq!(row("file.pdf").indexability_status, "Canonicalized");
        assert_eq!(row("html").canonical, Some(format!("{base_url}html")));
        assert_eq!(row("html").canonical_count, 2);
        assert_eq!(row("self").indexability, "Indexable");
        assert_eq!(row("repeated").canonical_count, 2);
        assert_eq!(row("missing").indexability_status, "HTTP 404");
    }

    #[cfg(feature = "js-rendering")]
    #[tokio::test]
    #[ignore = "requires Chrome; run make test-rendering"]
    async fn chrome_rendering_reference_links_discover_dom_and_http_headers_outside_content() {
        let (base_url, _, server) = spawn_recording_site(|path| {
            if path == "/" {
                response(200, "OK", "text/html", r#"<html><head><title>Raw</title></head><body><main>Selected content</main><script>
                    for (const [rel, href] of [['canonical', '/canonical'], ['alternate', '/language'], ['next', '/next'], ['amphtml', '/amp']]) {
                        const link = document.createElement('link'); link.rel = rel; link.href = href;
                        if (rel === 'alternate') link.hreflang = 'en';
                        document.head.append(link);
                    }
                </script></body></html>"#)
                    .replacen("Content-Type:", "Link: </http-canonical>; rel=canonical\r\nContent-Type:", 1)
            } else {
                response(200, "OK", "text/plain", "Target")
            }
        }).await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                max_depth: 1,
                content: ContentConfig {
                    include_selectors: vec!["main".into()],
                    ..ContentConfig::default()
                },
                rendering: JsRenderingConfig {
                    enabled: true,
                    wait_after_load_ms: 50,
                    ..JsRenderingConfig::default()
                },
                ..reference_test_config(&base_url)
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let records = store.records();
        assert_eq!(records.len(), 6);
        for target in ["canonical", "language", "next", "amp", "http-canonical"] {
            assert!(
                records
                    .iter()
                    .any(|row| row.url == format!("{base_url}{target}")),
                "{target}"
            );
        }
        let root = records.iter().find(|row| row.url == base_url).unwrap();
        assert!(root.js_rendered);
        assert_eq!(root.word_count, 2);
        assert_eq!(root.canonical_count, 2);
        assert_eq!(root.hreflang_count, 1);
        assert!(
            store
                .link_edges(ferrous_frog_storage::LinkEdgeQuery::default())
                .edges
                .is_empty()
        );
        assert!(
            records
                .iter()
                .all(|row| row.inlink_count == 0 && row.outlink_count == 0)
        );
        server.abort();
    }

    #[cfg(feature = "js-rendering")]
    #[tokio::test]
    #[ignore = "requires Chrome; run make test-rendering"]
    async fn chrome_rendering_requests_respect_retry_after_observed_by_http() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let (base_url, requests, server) = spawn_recording_site(move |path| {
            if path == "/" && counted.fetch_add(1, Ordering::SeqCst) == 0 {
                retry_after_response(429, "1")
            } else {
                response(
                    200,
                    "OK",
                    "text/html",
                    "<title>Rendered after delay</title>",
                )
            }
        })
        .await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                respect_robots: false,
                sitemap: SitemapConfig {
                    enabled: false,
                    ..SitemapConfig::default()
                },
                retry_attempts: 0,
                retry_backoff_ms: 0,
                request_delay_ms: 0,
                requests_per_second: 0,
                rendering: JsRenderingConfig {
                    enabled: true,
                    wait_after_load_ms: 50,
                    ..JsRenderingConfig::default()
                },
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let requests = requests.lock().unwrap();
        let attempts = requests
            .iter()
            .filter(|(path, _)| path == "/")
            .collect::<Vec<_>>();
        assert!(attempts.len() >= 2);
        assert!(attempts[1].1.duration_since(attempts[0].1) >= Duration::from_millis(990));
        let record = &store.records()[0];
        assert_eq!(record.status_code, Some(429));
        assert!(record.hsts_header);
        assert!(record.js_rendered);
        assert_eq!(record.title.as_deref(), Some("Rendered after delay"));
        server.abort();
    }

    #[cfg(feature = "js-rendering")]
    #[tokio::test]
    #[ignore = "requires Chrome; run make test-rendering"]
    async fn chrome_rendering_content_regions_keep_full_dom_links_and_scoped_word_delta() {
        const HTML: &str = "<html><head><title>Raw</title></head><body><main>Raw words</main><nav><a href='/outside'>Outside navigation</a></nav><script>document.querySelector('main').textContent = 'Rendered content has four'; document.title = 'Rendered'; document.querySelector('nav').insertAdjacentHTML('beforeend', '<a href=\"/injected\">Injected outside link</a>');</script></body></html>";
        let (base_url, _, server) = spawn_recording_site(|path| {
            response(
                200,
                "OK",
                "text/html",
                if path == "/" {
                    HTML
                } else {
                    "<main>Child content</main>"
                },
            )
        })
        .await;
        let config = CrawlConfig {
            start_url: base_url.clone(),
            max_urls: 3,
            max_depth: 1,
            respect_robots: false,
            request_delay_ms: 0,
            requests_per_second: 0,
            sitemap: SitemapConfig {
                enabled: false,
                ..SitemapConfig::default()
            },
            content: ContentConfig {
                include_selectors: vec!["main".into()],
                exclude_selectors: Vec::new(),
            },
            rendering: JsRenderingConfig {
                enabled: true,
                wait_after_load_ms: 50,
                ..JsRenderingConfig::default()
            },
            ..CrawlConfig::default()
        };
        let store = MemoryStore::new();
        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        let rows = store.records();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|row| row.url.ends_with("/outside")));
        assert!(rows.iter().any(|row| row.url.ends_with("/injected")));
        let root = rows.iter().find(|row| row.url == base_url).unwrap();
        assert_eq!(root.title.as_deref(), Some("Rendered"));
        assert_eq!(root.word_count, 4);
        assert_eq!(root.rendered_word_count_delta, 2);
        assert_eq!(
            root.simhash,
            Some(simhash::simhash("Rendered content has four"))
        );
        assert_eq!(
            root.response_hash,
            Some(blake3::hash(HTML.as_bytes()).to_hex().to_string())
        );
        server.abort();
    }

    #[cfg(feature = "js-rendering")]
    #[tokio::test]
    #[ignore = "requires Chrome; run make test-rendering"]
    async fn chrome_rendering_request_defaults_preserve_resource_specific_headers() {
        let foreign_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = foreign_headers.clone();
        let (foreign_url, _, foreign_server) =
            spawn_recording_site_with_request(move |_, request| {
                captured.lock().unwrap().push(request.to_string());
                response(200, "OK", "image/png", "fixture")
            })
            .await;
        let local_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = local_headers.clone();
        let (base_url, _, server) = spawn_recording_site_with_request(move |path, request| {
            captured.lock().unwrap().push((path.to_string(), request.to_string()));
            match path {
                "/" => response(200, "OK", "text/html", "<title>Raw</title><script src='/app.js'></script><img src='/image.png'><img src='/redirect-image'>"),
                "/app.js" => response(200, "OK", "text/javascript", "new Worker('/worker.js').onmessage = () => document.title = 'Native headers';"),
                "/worker.js" => response(200, "OK", "text/javascript", "fetch('/data').then(() => postMessage('ready'))"),
                "/redirect-image" => redirect_response(&foreign_url),
                "/image.png" => response(200, "OK", "image/png", "fixture"),
                _ => response(200, "OK", "text/plain", "ready"),
            }
        }).await;
        for language in ["en-US,en;q=0.9", "tr-TR"] {
            let config = config_with_controls(
                CrawlConfig {
                    start_url: base_url.clone(),
                    max_urls: 1,
                    respect_robots: false,
                    request_delay_ms: 0,
                    requests_per_second: 0,
                    sitemap: SitemapConfig {
                        enabled: false,
                        ..SitemapConfig::default()
                    },
                    rendering: JsRenderingConfig {
                        enabled: true,
                        wait_after_load_ms: 1000,
                        ..JsRenderingConfig::default()
                    },
                    ..CrawlConfig::default()
                },
                serde_json::json!({ "requestHeaders": [
                    {"name":"Accept","value":"text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8"},
                    {"name":"Accept-Language","value":language},
                    {"name":"Upgrade-Insecure-Requests","value":"1"},
                ] }),
            );
            let store = MemoryStore::new();
            crawl(config, store.clone(), CrawlControl::default(), |_| {})
                .await
                .unwrap();
            assert_eq!(store.records()[0].title.as_deref(), Some("Native headers"));
            let local = local_headers.lock().unwrap();
            for path in [
                "/app.js",
                "/worker.js",
                "/data",
                "/image.png",
                "/redirect-image",
            ] {
                let matching = local
                    .iter()
                    .filter(|(requested, _)| requested == path)
                    .collect::<Vec<_>>();
                assert!(!matching.is_empty(), "missing {path}");
                for (_, request) in matching {
                    let accept = request_header_value(request, "accept").unwrap_or_default();
                    if path.ends_with("image.png") || path == "/redirect-image" {
                        assert!(
                            accept.starts_with("image/"),
                            "non-image Accept at {path}: {accept}"
                        );
                    } else {
                        assert_eq!(accept, "*/*", "non-native Accept at {path}");
                    }
                    assert_eq!(
                        request_header_value(request, "upgrade-insecure-requests"),
                        None,
                        "navigation-only upgrade at {path}"
                    );
                    assert_eq!(
                        request_header_value(request, "accept-language"),
                        Some(language)
                    );
                }
            }
            drop(local);
            let foreign = foreign_headers.lock().unwrap();
            assert!(!foreign.is_empty());
            for request in &*foreign {
                assert!(
                    request_header_value(request, "accept")
                        .unwrap_or_default()
                        .starts_with("image/")
                );
                assert_eq!(
                    request_header_value(request, "upgrade-insecure-requests"),
                    None
                );
                assert_ne!(
                    request_header_value(request, "accept-language"),
                    Some("tr-TR")
                );
            }
            drop(foreign);
            local_headers.lock().unwrap().clear();
            foreign_headers.lock().unwrap().clear();
        }
        server.abort();
        foreign_server.abort();
    }

    #[cfg(feature = "js-rendering")]
    #[tokio::test]
    #[ignore = "requires Chrome; run make test-rendering"]
    async fn chrome_rendering_custom_headers_are_origin_restricted() {
        let foreign_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_foreign = foreign_headers.clone();
        let (foreign_url, _, foreign_server) =
            spawn_recording_site_with_request(move |path, request| {
                captured_foreign
                    .lock()
                    .unwrap()
                    .push((path.to_string(), request.to_string()));
                if path == "/frame" {
                    response(200, "OK", "text/html", "<img src='/image.png'>")
                } else {
                    response(200, "OK", "image/png", "fixture")
                }
            })
            .await;
        let local_headers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_local = local_headers.clone();
        let (base_url, _, server) = spawn_recording_site_with_request(move |path, request| {
            captured_local.lock().unwrap().push((path.to_string(), request.to_string()));
            match path {
                "/" => response(200, "OK", "text/html", &format!("<title>Raw</title><script src='/app.js'></script><img src='/redirect-image'><iframe src='{}frame'></iframe>", foreign_url.replace("127.0.0.1", "localhost"))),
                "/app.js" => response(200, "OK", "text/javascript", "new Worker('/worker.js').onmessage = () => document.title = 'Header render';"),
                "/worker.js" => response(200, "OK", "text/javascript", "fetch('/worker-data').then(() => postMessage('ready'))"),
                "/redirect-image" => redirect_response(&format!("{foreign_url}image.png")),
                _ => response(200, "OK", "text/plain", "fixture"),
            }
        }).await;
        let config = config_with_controls(
            CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                respect_robots: false,
                request_delay_ms: 0,
                requests_per_second: 0,
                sitemap: SitemapConfig {
                    enabled: false,
                    ..SitemapConfig::default()
                },
                rendering: JsRenderingConfig {
                    enabled: true,
                    wait_after_load_ms: 1000,
                    ..JsRenderingConfig::default()
                },
                ..CrawlConfig::default()
            },
            serde_json::json!({ "userAgent":"RenderedHeaderFixture/1.0", "requestHeaders": [
                {"name":"X-Environment","value":"preview"},
                {"name":"Accept","value":"application/x-fixture"},
                {"name":"Upgrade-Insecure-Requests","value":"0"}
            ] }),
        );
        let store = MemoryStore::new();
        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(store.records()[0].title.as_deref(), Some("Header render"));
        let local_headers = local_headers.lock().unwrap();
        for path in [
            "/",
            "/app.js",
            "/worker.js",
            "/worker-data",
            "/redirect-image",
        ] {
            let matching = local_headers
                .iter()
                .filter(|(requested, _)| requested == path)
                .collect::<Vec<_>>();
            assert!(!matching.is_empty(), "missing {path}");
            assert!(
                matching.iter().all(|(_, request)| request
                    .to_ascii_lowercase()
                    .contains("x-environment: preview")),
                "missing header at {path}"
            );
            for (_, request) in matching {
                assert_eq!(
                    request_header_value(request, "user-agent"),
                    Some("RenderedHeaderFixture/1.0")
                );
                assert_eq!(
                    request_header_value(request, "accept"),
                    Some("application/x-fixture")
                );
                assert_eq!(
                    request_header_value(request, "upgrade-insecure-requests"),
                    Some("0")
                );
            }
        }
        let foreign_headers = foreign_headers.lock().unwrap();
        assert!(foreign_headers.iter().any(|(path, _)| path == "/frame"));
        assert!(foreign_headers.iter().any(|(path, _)| path == "/image.png"));
        assert!(
            foreign_headers
                .iter()
                .all(|(_, request)| !request.to_ascii_lowercase().contains("x-environment:")),
            "headers leaked to browser subresources"
        );
        for (_, request) in foreign_headers.iter() {
            assert_eq!(
                request_header_value(request, "user-agent"),
                Some("RenderedHeaderFixture/1.0")
            );
            assert_ne!(
                request_header_value(request, "accept"),
                Some("application/x-fixture")
            );
            assert_ne!(
                request_header_value(request, "upgrade-insecure-requests"),
                Some("0")
            );
        }
        server.abort();
        foreign_server.abort();
    }

    #[cfg(feature = "js-rendering")]
    #[tokio::test]
    #[ignore = "requires Chrome; run make test-rendering"]
    async fn chrome_rendering_applies_robots_and_pacing_to_nested_requests() {
        let (other_url, other_requests, other_server) = spawn_recording_site(|path| match path {
            "/robots.txt" => response(
                200,
                "OK",
                "text/plain",
                "User-agent: *\nDisallow: /blocked\nCrawl-delay: 0.08\n",
            ),
            "/frame" => response(
                200,
                "OK",
                "text/html",
                "<script src='/frame.js'></script><img src='/blocked.png'>",
            ),
            _ => response(200, "OK", "text/javascript", ""),
        })
        .await;
        let agents = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_agents = agents.clone();
        let (base_url, requests, server) = spawn_recording_site_with_request(move |path, request| {
            captured_agents.lock().unwrap().push(request.lines().find(|line| line.to_ascii_lowercase().starts_with("user-agent:")).unwrap_or("").to_string());
            match path {
                "/robots.txt" => response(200, "OK", "text/plain", "User-agent: *\nDisallow: /blocked\nCrawl-delay: 0.08\n"),
                "/" => response(200, "OK", "text/html", &format!(r#"<title>Raw</title><script src='/app.js'></script><link rel='stylesheet' href='/style.css'><img src='/blocked.png'><iframe src='{}frame'></iframe>"#, other_url.replace("127.0.0.1", "localhost"))),
                "/app.js" => response(200, "OK", "text/javascript", "new Worker('/worker.js').onmessage = () => { document.title = 'Rendered safely'; }; navigator.serviceWorker.register('/service.js').then(() => fetch('/registered')).catch(e => fetch('/registration-failed?' + encodeURIComponent(e))); fetch('/redirect').catch(() => {}); window.open('/popup');"),
                "/worker.js" => response(200, "OK", "text/javascript", "Promise.all([fetch('/worker-data'), fetch('/blocked-worker').catch(() => {})]).then(() => postMessage('ready'));"),
                "/service.js" => response(200, "OK", "text/javascript", "addEventListener('activate', event => event.waitUntil(Promise.all([fetch('/service-data'), fetch('/blocked-service').catch(() => {})])));"),
                "/popup" => response(200, "OK", "text/html", "<img src='/blocked-popup'>"),
                "/redirect" => redirect_response("/blocked-redirect"),
                "/style.css" => response(200, "OK", "text/css", "body { color: black; }"),
                _ => response(200, "OK", "text/plain", "ok"),
            }
        }).await;
        let store = MemoryStore::new();
        crawl(
            CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                user_agent: "FerrousFrogRenderingTest/1.0".into(),
                request_delay_ms: 50,
                requests_per_second: 100,
                timeout_secs: 30,
                rendering: JsRenderingConfig {
                    enabled: true,
                    wait_after_load_ms: 2500,
                    ..JsRenderingConfig::default()
                },
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        server.abort();
        other_server.abort();
        let records = store.records();
        assert_eq!(
            records[0].title.as_deref(),
            Some("Rendered safely"),
            "{:?}",
            records[0].error
        );
        let requests = requests.lock().unwrap();
        let other_requests = other_requests.lock().unwrap();
        for requests in [&*requests, &*other_requests] {
            assert!(
                !requests
                    .iter()
                    .any(|(path, _)| path.starts_with("/blocked")),
                "{requests:?}"
            );
            assert_eq!(
                requests
                    .iter()
                    .filter(|(path, _)| path == "/robots.txt")
                    .count(),
                1
            );
            // Chrome delivery and server scheduling can compress individual arrival gaps.
            // ponytail: exclude startup and allow one boundary interval of delivery jitter;
            // add CDP timing capture if per-request browser assertions are needed.
            // Exact admission spacing is verified directly in the request-policy test.
            let start = requests
                .iter()
                .rposition(|(path, _)| path == "/")
                .unwrap_or(0);
            let rendered_requests = &requests[start..];
            let minimum_span =
                Duration::from_millis(80 * rendered_requests.len().saturating_sub(2) as u64);
            assert!(
                rendered_requests
                    .last()
                    .unwrap()
                    .1
                    .duration_since(rendered_requests[0].1)
                    >= minimum_span,
                "Nested requests must share sustained robots pacing: {rendered_requests:?}"
            );
        }
        for expected in [
            "/app.js",
            "/worker.js",
            "/worker-data",
            "/service-data",
            "/popup",
            "/redirect",
            "/style.css",
        ] {
            assert!(
                requests.iter().any(|(path, _)| path == expected),
                "Missing {expected}: {requests:?}"
            );
        }
        assert!(other_requests.iter().any(|(path, _)| path == "/frame.js"));
        let agents = agents.lock().unwrap();
        assert!(
            agents
                .iter()
                .all(|header| header.contains("FerrousFrogRenderingTest/1.0")),
            "{agents:?}"
        );
    }

    #[cfg(feature = "js-rendering")]
    #[tokio::test]
    #[ignore = "requires Chrome; run make test-rendering"]
    async fn chrome_rendering_obeys_pause_resume_and_stop() {
        for (stop, pause_ms) in [(false, 200), (true, 0), (true, 200)] {
            let control = CrawlControl::default();
            let site_control = control.clone();
            let visits = AtomicUsize::new(0);
            let (base_url, requests, server) = spawn_recording_site(move |path| {
                if path == "/" {
                    if visits.fetch_add(1, Ordering::SeqCst) == 1 {
                        site_control.pause();
                    }
                    response(
                        200,
                        "OK",
                        "text/html",
                        "<title>Raw</title><script src='/late.js'></script>",
                    )
                } else if path == "/late.js" {
                    response(200, "OK", "text/javascript", "document.title = 'Resumed';")
                } else {
                    response(404, "Not Found", "text/plain", "")
                }
            })
            .await;
            let store = MemoryStore::new();
            let crawl_task = tokio::spawn(crawl(
                CrawlConfig {
                    start_url: base_url,
                    max_urls: 1,
                    rendering: JsRenderingConfig {
                        enabled: true,
                        ..JsRenderingConfig::default()
                    },
                    ..CrawlConfig::default()
                },
                store.clone(),
                control.clone(),
                |_| {},
            ));
            tokio::time::timeout(Duration::from_secs(45), async {
                while !control.is_paused() {
                    assert!(
                        !crawl_task.is_finished(),
                        "Rendering must reach the browser navigation: {:?}",
                        store
                            .records()
                            .iter()
                            .map(|row| &row.error)
                            .collect::<Vec<_>>()
                    );
                    sleep(Duration::from_millis(25)).await;
                }
            })
            .await
            .unwrap();
            // Observe network silence while paused; deadline expiry is covered with virtual time.
            sleep(Duration::from_millis(pause_ms)).await;
            assert!(
                store.records().is_empty(),
                "Paused rendering must not publish a raw fallback"
            );
            assert!(
                !requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(path, _)| path == "/late.js")
            );
            if stop {
                control.cancel();
            } else {
                control.resume();
            }
            tokio::time::timeout(Duration::from_secs(if stop { 5 } else { 45 }), crawl_task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if stop {
                assert!(
                    store.records().is_empty(),
                    "Stop must not publish a cancelled render"
                );
                assert_eq!(store.load_frontier_state().unwrap().queued.len(), 1);
                sleep(Duration::from_millis(200)).await;
                assert!(
                    !requests
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|(path, _)| path == "/late.js")
                );
            } else {
                let records = store.records();
                assert_eq!(
                    records[0].title.as_deref(),
                    Some("Resumed"),
                    "Resume must finish rendering: js_rendered={}, error={:?}, requests={:?}",
                    records[0].js_rendered,
                    records[0].error,
                    requests.lock().unwrap(),
                );
            }
            server.abort();
        }
    }

    #[cfg(not(feature = "js-rendering"))]
    #[tokio::test]
    async fn unsupported_rendering_preserves_results_and_frontier_without_http_requests() {
        let (base_url, requests, server) =
            spawn_recording_site(|_| response(200, "OK", "text/html", "<title>New crawl</title>"))
                .await;
        let store = MemoryStore::new();
        store.upsert(CrawlRecord::pending("https://saved.test/".into(), 0));
        let frontier = CrawlFrontierState {
            queued: Vec::new(),
            seen: vec!["https://saved.test/".into()],
            crawled: 1,
        };
        store.save_frontier_state(frontier.clone());
        let result = crawl(
            CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                rendering: JsRenderingConfig {
                    enabled: true,
                    ..JsRenderingConfig::default()
                },
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| panic!("An unsupported rendering crawl must not start"),
        )
        .await;
        server.abort();
        assert!(result.unwrap_err().to_string().contains("not included"));
        assert!(requests.lock().unwrap().is_empty());
        assert_eq!(store.records().len(), 1);
        assert_eq!(store.records()[0].url, "https://saved.test/");
        assert_eq!(store.load_frontier_state(), Some(frontier));
    }

    #[test]
    fn all_subdomains_respects_registrable_domains_and_private_suffixes() {
        let scope = serde_json::from_str::<SubdomainScope>("\"allSubdomains\"").unwrap();
        for (root, candidate, allowed) in [
            (
                "https://shop.example.co.uk/",
                "https://blog.example.co.uk/",
                true,
            ),
            (
                "https://shop.example.co.uk/",
                "https://example.co.uk/",
                true,
            ),
            ("https://shop.example.co.uk/", "https://other.co.uk/", false),
            (
                "https://shop.example.co.uk/",
                "https://example.co.uk.evil.com/",
                false,
            ),
            (
                "https://docs.project.github.io/",
                "https://blog.project.github.io/",
                true,
            ),
            (
                "https://docs.project.github.io/",
                "https://other.github.io/",
                false,
            ),
            ("https://co.uk/", "https://example.co.uk/", false),
            ("https://EXAMPLE.COM./", "https://api.example.com/", true),
            ("http://127.0.0.1/", "http://127.0.0.1:8080/", true),
            ("http://127.0.0.1/", "http://127.0.0.1.example/", false),
            ("http://[::1]/", "http://[::2]/", false),
            ("http://localhost/", "http://other.localhost/", false),
        ] {
            assert_eq!(
                scope_host_allows(
                    &Url::parse(candidate).unwrap(),
                    &Url::parse(root).unwrap(),
                    scope
                ),
                allowed,
                "{root} → {candidate}"
            );
        }
        // Existing saved profiles keep their narrower host/descendant behavior.
        assert!(!scope_host_allows(
            &Url::parse("https://blog.example.com/").unwrap(),
            &Url::parse("https://shop.example.com/").unwrap(),
            SubdomainScope::IncludeSubdomains
        ));
    }

    #[test]
    fn exact_url_scope_includes_query_and_origin_but_ignores_fragments() {
        let root = Url::parse("https://example.com/docs/page?lang=en#top").unwrap();
        let config = CrawlConfig {
            folder_scope: serde_json::from_str("\"exactUrl\"").unwrap(),
            ..CrawlConfig::default()
        };
        let rules = compile_scope_rules(&config).unwrap();
        for (candidate, allowed) in [
            ("https://example.com/docs/page?lang=en", true),
            ("https://example.com/docs/page?lang=en#section", true),
            ("https://example.com/docs/page?lang=tr", false),
            ("https://example.com/docs/other?lang=en", false),
            ("http://example.com/docs/page?lang=en", false),
            ("https://example.com:444/docs/page?lang=en", false),
            ("https://other.example.com/docs/page?lang=en", false),
        ] {
            assert_eq!(
                scope_allows(&Url::parse(candidate).unwrap(), &root, &rules, &config),
                allowed,
                "{candidate}"
            );
        }
    }

    #[tokio::test]
    async fn exact_url_fetches_only_seed_and_redirects_while_retaining_links() {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/" => redirect_response("/landing"),
            "/landing" => response(200, "OK", "text/html", "<title>Landing</title><a href='/child'>Child</a><a href='/'>Home</a><img src='/image.png'>"),
            "/sitemap.xml" => response(200, "OK", "application/xml", "<urlset><url><loc>/sitemap-page</loc></url></urlset>"),
            _ => response(404, "Not Found", "text/plain", ""),
        }).await;
        let store = MemoryStore::new();
        let config = CrawlConfig {
            start_url: format!("{base_url}#top"),
            folder_scope: serde_json::from_str("\"exactUrl\"").unwrap(),
            requests_per_second: 100,
            resource_types: CrawlResourceTypes {
                images: true,
                ..CrawlResourceTypes::default()
            },
            ..CrawlConfig::default()
        };
        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        let rows = store.records();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title.as_deref(), Some("Landing"));
        assert_eq!(rows[0].url, base_url);
        assert_eq!(rows[0].redirect_chain.len(), 1);
        assert!(store.link_edges(Default::default()).total >= 2);
        let paths = requests
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        assert_eq!(paths.iter().filter(|path| path.as_str() == "/").count(), 1);
        assert!(
            !paths.iter().any(
                |path| ["/sitemap.xml", "/sitemap-page", "/child", "/image.png"]
                    .contains(&path.as_str())
            ),
            "{paths:?}"
        );
        server.abort();
    }

    #[test]
    fn scope_modes_limit_subdomains_and_folders() {
        let root = Url::parse("https://www.example.com/docs/").unwrap();
        let same_folder = Url::parse("https://www.example.com/docs/page").unwrap();
        let child_folder = Url::parse("https://www.example.com/docs/guides/page").unwrap();
        let sibling_folder = Url::parse("https://www.example.com/blog/page").unwrap();
        let subdomain = Url::parse("https://cdn.example.com/docs/page").unwrap();
        let rules = ScopeRules {
            include: Vec::new(),
            exclude: Vec::new(),
        };

        let mut config = CrawlConfig {
            subdomain_scope: SubdomainScope::ExactHost,
            folder_scope: FolderScope::StartFolder,
            ..CrawlConfig::default()
        };

        assert!(scope_allows(&same_folder, &root, &rules, &config));
        assert!(scope_allows(&child_folder, &root, &rules, &config));
        assert!(!scope_allows(&sibling_folder, &root, &rules, &config));
        assert!(!scope_allows(&subdomain, &root, &rules, &config));

        config.subdomain_scope = SubdomainScope::IncludeSubdomains;
        assert!(scope_allows(&subdomain, &root, &rules, &config));

        config.folder_scope = FolderScope::ExactFolder;
        assert!(scope_allows(&same_folder, &root, &rules, &config));
        assert!(!scope_allows(&child_folder, &root, &rules, &config));
    }

    #[test]
    fn configuration_validation_accepts_drafts_and_rejects_invalid_engine_rules() {
        let mut config = CrawlConfig::default();
        config.start_url.clear();
        assert!(
            validate_configuration(&config).is_ok(),
            "A crawl target is optional while editing configuration"
        );
        config.include_url_patterns = vec!["[".into()];
        assert!(
            validate_configuration(&config)
                .unwrap_err()
                .to_string()
                .contains("include")
        );
        config.include_url_patterns.clear();
        config.query_settings.strip_parameter_patterns = vec!["[".into()];
        assert!(
            validate_configuration(&config)
                .unwrap_err()
                .to_string()
                .contains("query")
        );
        config.query_settings.strip_parameter_patterns.clear();
        config.list_urls = vec!["file:///etc/hosts".into()];
        assert!(
            validate_configuration(&config)
                .unwrap_err()
                .to_string()
                .contains("HTTP")
        );
        config.list_urls.clear();
        config.user_agent = "Crawler\r\nInjected: header".into();
        assert!(
            validate_configuration(&config)
                .unwrap_err()
                .to_string()
                .contains("User-Agent")
        );
        config.user_agent = DEFAULT_USER_AGENT.into();
        config.custom_searches = vec![CustomSearch {
            name: "Broken search".into(),
            pattern: "[".into(),
            regex: true,
            case_sensitive: false,
            max_snippets: 1,
        }];
        assert!(
            validate_configuration(&config)
                .unwrap_err()
                .to_string()
                .contains("Broken search")
        );
        config.custom_searches.clear();
        config.max_urls = 0;
        assert!(validate_configuration(&config).is_err());
    }

    #[tokio::test]
    async fn starting_requires_a_target_but_editing_a_profile_does_not() {
        let mut config = CrawlConfig {
            start_url: "  ".into(),
            ..CrawlConfig::default()
        };
        assert!(validate_configuration(&config).is_ok());
        assert!(validate_crawl_start(&config).is_err());
        config.list_urls = vec!["https://example.test/page".into()];
        assert!(
            validate_crawl_start(&config).is_err(),
            "Spider needs its own seed"
        );
        config.mode = CrawlMode::List;
        assert!(validate_crawl_start(&config).is_ok());
        config.list_urls.clear();
        config.list_sitemap_urls = vec!["https://example.test/sitemap.xml".into()];
        assert!(validate_crawl_start(&config).is_ok());
        config.list_sitemap_urls = vec![" ".into()];
        assert!(validate_crawl_start(&config).is_err());
        config.start_url = "https://example.test/".into();
        config.list_urls = vec![" ".into()];
        assert!(validate_crawl_start(&config).is_ok());
        let rules = compile_query_rules(&config).unwrap();
        let root = root_url_from_config(&config, &rules).unwrap();
        let seeds = seed_queue_items(&config, &root, &rules, &[]).unwrap();
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].url, root);
        config.custom_searches = vec![CustomSearch {
            name: "Invalid".into(),
            pattern: "[".into(),
            regex: true,
            case_sensitive: false,
            max_snippets: 1,
        }];
        assert!(validate_crawl_start(&config).is_err());
        let store = MemoryStore::new();
        store.upsert(CrawlRecord::pending("https://example.test/saved".into(), 0));
        assert!(
            crawl(config, store.clone(), CrawlControl::default(), |_| panic!(
                "Invalid configuration must not emit crawl events"
            ))
            .await
            .is_err()
        );
        assert_eq!(store.records()[0].url, "https://example.test/saved");
    }

    #[test]
    fn all_subdomains_applies_to_sitemap_seeds_and_resource_discovery() {
        let config = CrawlConfig {
            start_url: "https://docs.project.github.io/".into(),
            subdomain_scope: SubdomainScope::AllSubdomains,
            exclude_url_patterns: vec!["/skip$".into()],
            ..CrawlConfig::default()
        };
        let query_rules = compile_query_rules(&config).unwrap();
        let root = root_url_from_config(&config, &query_rules).unwrap();
        let candidates = [
            "https://blog.project.github.io/page",
            "https://blog.project.github.io/skip",
            "https://other.github.io/page",
        ]
        .map(|value| Url::parse(value).unwrap());
        let (queue, _) = seed_frontier(
            &config,
            &root,
            &query_rules,
            Vec::new(),
            candidates.to_vec(),
            &compile_scope_rules(&config).unwrap(),
        )
        .unwrap();
        assert_eq!(
            queue
                .iter()
                .map(|item| item.url.as_str())
                .collect::<Vec<_>>(),
            vec![root.as_str(), candidates[0].as_str()]
        );
        assert!(should_crawl_discovered(
            &candidates[0],
            &root,
            DiscoveredResourceType::Html,
            &config.resource_types,
            config.subdomain_scope
        ));
        assert!(!should_crawl_discovered(
            &candidates[2],
            &root,
            DiscoveredResourceType::Html,
            &config.resource_types,
            config.subdomain_scope
        ));
    }

    #[tokio::test]
    async fn resumed_spider_rechecks_scope_but_list_preserves_explicit_entries() {
        for mode in [CrawlMode::Spider, CrawlMode::List] {
            let (base_url, requests, server) = spawn_recording_site(|path| match path {
                "/" | "/child" => response(200, "OK", "text/html", "<title>Saved entry</title>"),
                _ => response(404, "Not Found", "text/plain", ""),
            })
            .await;
            let urls = vec![base_url.clone(), format!("{base_url}child")];
            let store = MemoryStore::new();
            store.save_frontier_state(CrawlFrontierState {
                queued: urls
                    .iter()
                    .map(|url| CrawlFrontierItem {
                        url: url.clone(),
                        storage_key: url.clone(),
                        depth: 0,
                        from_sitemap: false,
                        list_position: None,
                        list_duplicate_index: 0,
                    })
                    .collect(),
                seen: urls,
                crawled: 0,
            });
            crawl(
                CrawlConfig {
                    start_url: base_url,
                    mode,
                    resume_from_state: true,
                    folder_scope: FolderScope::ExactUrl,
                    requests_per_second: 100,
                    ..CrawlConfig::default()
                },
                store.clone(),
                CrawlControl::default(),
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(
                store.records().len(),
                if mode == CrawlMode::Spider { 1 } else { 2 }
            );
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(path, _)| path == "/child"),
                mode == CrawlMode::List
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn resumed_exact_url_normalizes_saved_seed_and_deduplicates_pending_aliases() {
        for suffix in ["#top", "?tracking=1#top"] {
            let (base_url, requests, server) = spawn_recording_site(|path| match path {
                "/" => response(
                    200,
                    "OK",
                    "text/html",
                    "<title>Seed</title><a href='/'>Self</a>",
                ),
                _ => response(404, "Not Found", "text/plain", ""),
            })
            .await;
            let seed = format!("{base_url}{suffix}");
            let aliases = if suffix.starts_with('?') {
                vec![seed.clone()]
            } else {
                vec![seed.clone(), base_url.clone()]
            };
            let store = MemoryStore::new();
            store.save_frontier_state(CrawlFrontierState {
                queued: aliases
                    .iter()
                    .map(|url| CrawlFrontierItem {
                        url: url.clone(),
                        storage_key: url.clone(),
                        depth: 0,
                        from_sitemap: false,
                        list_position: None,
                        list_duplicate_index: 0,
                    })
                    .collect(),
                seen: aliases,
                crawled: 0,
            });
            crawl(
                CrawlConfig {
                    start_url: seed,
                    resume_from_state: true,
                    folder_scope: FolderScope::ExactUrl,
                    query_settings: QuerySettings {
                        strip_all: true,
                        ..QuerySettings::default()
                    },
                    requests_per_second: 100,
                    ..CrawlConfig::default()
                },
                store.clone(),
                CrawlControl::default(),
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(store.records().len(), 1);
            assert_eq!(store.records()[0].url, base_url);
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(path, _)| path == "/")
                    .count(),
                1
            );
            server.abort();
        }
    }

    #[test]
    fn parses_sitemap_locations() {
        let root_url = Url::parse("https://example.com/start").unwrap();
        let parsed = parse_sitemap_document(
            r#"
                <urlset>
                  <url><loc>/one</loc></url>
                  <url><loc>https://example.com/two</loc></url>
                  <url><loc>https://example.com/event/MVNO&apos;s%20World%202026</loc></url>
                  <url><loc>https://example.com/search?a=1&amp;b=2</loc></url>
                </urlset>
            "#,
            &root_url,
            &CrawlConfig::default(),
        )
        .unwrap();

        let urls = parsed.urls;
        assert_eq!(urls.len(), 4);
        assert_eq!(urls[0].as_str(), "https://example.com/one");
        assert_eq!(urls[1].as_str(), "https://example.com/two");
        assert_eq!(
            urls[2].as_str(),
            "https://example.com/event/MVNO's%20World%202026"
        );
        assert_eq!(urls[3].as_str(), "https://example.com/search?a=1&b=2");
    }

    #[test]
    fn parses_prefixed_sitemap_locations_with_unicode_and_entities() {
        let root_url = Url::parse("https://example.com/sitemap.xml").unwrap();
        let parsed = parse_sitemap_document(
            r#"<sm:urlset xmlns:sm="http://www.sitemaps.org/schemas/sitemap/0.9">
                <sm:url><sm:loc> /café?a=1&amp;b=2&#38;c=3 </sm:loc></sm:url>
            </sm:urlset>"#,
            &root_url,
            &CrawlConfig::default(),
        )
        .unwrap();

        assert!(parsed.sitemaps.is_empty());
        assert_eq!(parsed.urls.len(), 1);
        assert_eq!(
            parsed.urls[0].as_str(),
            "https://example.com/caf%C3%A9?a=1&b=2&c=3"
        );
    }

    #[test]
    fn parses_sitemap_index_locations_separately() {
        let root_url = Url::parse("https://example.com/sitemap.xml").unwrap();
        let parsed = parse_sitemap_document(
            r#"
                <sitemapindex>
                  <sitemap><loc>/posts-sitemap.xml</loc></sitemap>
                  <sitemap><loc>https://example.com/pages-sitemap.xml</loc></sitemap>
                </sitemapindex>
            "#,
            &root_url,
            &CrawlConfig::default(),
        )
        .unwrap();

        assert!(parsed.urls.is_empty());
        assert_eq!(parsed.sitemaps.len(), 2);
        assert_eq!(
            parsed.sitemaps[0].as_str(),
            "https://example.com/posts-sitemap.xml"
        );
        assert_eq!(
            parsed.sitemaps[1].as_str(),
            "https://example.com/pages-sitemap.xml"
        );
    }

    #[test]
    fn parses_robots_crawl_delay() {
        let robot =
            parse_robots(DEFAULT_USER_AGENT, b"User-agent: *\nCrawl-delay: 0.02\n").unwrap();
        let delay = robots_crawl_delay_ms(&robot);
        assert_eq!(delay, Some(20));
    }

    #[tokio::test]
    async fn request_policy_spaces_same_origin_admissions_using_robots_delay() {
        let url = Url::parse("https://example.test/page").unwrap();
        let policy = RequestPolicy {
            origins: Default::default(),
            rate_limiter: None,
            respect_robots: true,
            header_origin: url.origin(),
            request_headers: HeaderMap::new(),
            on_event: Arc::new(|_| {}),
        };
        let origin = policy.origin(&url).await;
        origin
            .robots
            .set(Ok(Some(
                parse_robots(DEFAULT_USER_AGENT, b"User-agent: *\nCrawl-delay: 0.08\n").unwrap(),
            )))
            .unwrap();
        let control = CrawlControl::default();
        policy.wait(&url, 0, &control).await.unwrap();
        let mut previous = origin.last_request.lock().await.unwrap();
        for (path, delay_ms, minimum_ms) in [
            ("/style.css", 0, 80),
            ("/redirect", 50, 80),
            ("/worker.js", 120, 120),
        ] {
            policy
                .wait(&url.join(path).unwrap(), delay_ms, &control)
                .await
                .unwrap();
            let admitted = origin.last_request.lock().await.unwrap();
            assert!(
                admitted.duration_since(previous) >= Duration::from_millis(minimum_ms),
                "{path} was admitted before the effective request delay"
            );
            previous = admitted;
        }
        let targets =
            ["/image.png", "/worker-data", "/service-data"].map(|path| url.join(path).unwrap());
        let (first, second, third) = tokio::join!(
            policy.wait(&targets[0], 0, &control),
            policy.wait(&targets[1], 0, &control),
            policy.wait(&targets[2], 0, &control),
        );
        first.unwrap();
        second.unwrap();
        third.unwrap();
        let last = origin.last_request.lock().await.unwrap();
        assert!(
            last.duration_since(previous) >= Duration::from_millis(240),
            "Concurrent callers must share all three 80 ms request slots"
        );
    }

    #[test]
    fn tests_robots_txt_rules() {
        let result = test_robots_txt(RobotsTxtTestRequest {
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            robots_txt: "User-agent: *\nDisallow: /private\nCrawl-delay: 0.5\n".to_string(),
            url: "https://example.com/private/page".to_string(),
        })
        .unwrap();

        assert!(!result.allowed);
        assert_eq!(result.crawl_delay_ms, Some(500));
    }

    #[test]
    fn tests_robots_txt_rules_in_batches() {
        let result = test_robots_txt_batch(RobotsTxtBatchTestRequest {
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            robots_txt: "User-agent: *\nDisallow: /private\nCrawl-delay: 0.5\n".to_string(),
            urls: vec![
                "https://example.com/public".to_string(),
                "https://example.com/private/page".to_string(),
                "not a url".to_string(),
            ],
        })
        .unwrap();

        assert_eq!(result.allowed, 1);
        assert_eq!(result.blocked, 1);
        assert_eq!(result.invalid, 1);
        assert_eq!(result.crawl_delay_ms, Some(500));
        assert_eq!(result.rows.len(), 3);
    }

    #[test]
    fn robots_rules_and_delay_use_the_crawler_product_token() {
        let robots_txt = "User-agent: OtherBot\nCrawl-delay: 9\n\nUser-agent: *\nAllow: /\nCrawl-delay: 0.01\n\nUser-agent: FerrousFrogSeoSpider\nDisallow: /private\nCrawl-delay: 0.06\n";
        let result = test_robots_txt(RobotsTxtTestRequest {
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            robots_txt: robots_txt.to_string(),
            url: "https://example.com/private".to_string(),
        })
        .unwrap();
        assert!(!result.allowed);
        assert_eq!(result.crawl_delay_ms, Some(60));

        let batch = test_robots_txt_batch(RobotsTxtBatchTestRequest {
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            robots_txt: robots_txt.to_string(),
            urls: vec!["https://example.com/private".to_string()],
        })
        .unwrap();
        assert_eq!(batch.blocked, 1);
        assert_eq!(batch.crawl_delay_ms, Some(60));
    }

    #[tokio::test]
    async fn robots_policies_are_cached_separately_for_each_list_origin() {
        let (first, first_requests, first_server) = spawn_recording_site(|path| {
            if path == "/robots.txt" {
                response(200, "OK", "text/plain", "User-agent: *\nDisallow: /a\n")
            } else {
                response(200, "OK", "text/html", "<h1>Public</h1>")
            }
        })
        .await;
        let (second, second_requests, second_server) = spawn_recording_site(|path| {
            if path == "/robots.txt" {
                response(200, "OK", "text/plain", "User-agent: *\nDisallow: /b\n")
            } else {
                response(200, "OK", "text/html", "<h1>Public</h1>")
            }
        })
        .await;
        let store = MemoryStore::new();
        let result = crawl(
            CrawlConfig {
                mode: CrawlMode::List,
                list_urls: vec![
                    format!("{first}a"),
                    format!("{first}b"),
                    format!("{second}a"),
                    format!("{second}b"),
                ],
                concurrency: 4,
                requests_per_second: 100,
                request_delay_ms: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await;
        first_server.abort();
        second_server.abort();
        assert_eq!(result.unwrap().crawled, 4);
        for (requests, allowed, blocked) in
            [(first_requests, "/b", "/a"), (second_requests, "/a", "/b")]
        {
            let requests = requests.lock().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|(path, _)| path == "/robots.txt")
                    .count(),
                1
            );
            assert_eq!(
                requests.iter().filter(|(path, _)| path == allowed).count(),
                1
            );
            assert!(!requests.iter().any(|(path, _)| path == blocked));
        }
        let records = store.records();
        assert_eq!(
            records
                .iter()
                .filter(|record| record.status_code == Some(200))
                .count(),
            2
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.status_text == "Blocked by robots.txt")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn robots_block_same_origin_and_cross_origin_redirect_targets() {
        let (target, target_requests, target_server) = spawn_recording_site(|path| {
            if path == "/robots.txt" {
                response(
                    200,
                    "OK",
                    "text/plain",
                    "User-agent: *\nDisallow: /private\n",
                )
            } else {
                response(200, "OK", "text/html", "<h1>Private</h1>")
            }
        })
        .await;
        let private_target = format!("{target}private");
        let redirect_target = private_target.clone();
        let (source, source_requests, source_server) =
            spawn_recording_site(move |path| match path {
                "/robots.txt" => response(
                    200,
                    "OK",
                    "text/plain",
                    "User-agent: *\nDisallow: /blocked\n",
                ),
                "/same" => redirect_response("/blocked"),
                "/external" => redirect_response(&redirect_target),
                _ => response(200, "OK", "text/html", "<h1>Private</h1>"),
            })
            .await;
        let store = MemoryStore::new();
        let result = crawl(
            CrawlConfig {
                mode: CrawlMode::List,
                list_urls: vec![format!("{source}same"), format!("{source}external")],
                requests_per_second: 100,
                request_delay_ms: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await;
        source_server.abort();
        target_server.abort();
        assert_eq!(result.unwrap().crawled, 2);
        assert!(
            !source_requests
                .lock()
                .unwrap()
                .iter()
                .any(|(path, _)| path == "/blocked")
        );
        assert!(
            !target_requests
                .lock()
                .unwrap()
                .iter()
                .any(|(path, _)| path == "/private")
        );
        let records = store.records();
        for (original, blocked) in [
            (format!("{source}same"), format!("{source}blocked")),
            (format!("{source}external"), private_target),
        ] {
            let record = records
                .iter()
                .find(|record| record.url == original)
                .unwrap();
            assert_eq!(record.final_url, blocked);
            assert_eq!(record.redirect_chain.len(), 1);
            assert_eq!(record.redirect_target.as_deref(), Some(blocked.as_str()));
            assert_eq!(record.status_text, "Blocked by robots.txt");
            assert_eq!(record.status_code, None);
            assert_eq!(record.indexability, "Non-indexable");
        }
    }

    #[tokio::test]
    async fn robots_and_configured_delays_space_concurrent_requests_and_redirects() {
        for respect_robots in [true, false] {
            let (base_url, requests, server) = spawn_recording_site(|path| match path {
                "/robots.txt" => {
                    response(200, "OK", "text/plain", "User-agent: *\nCrawl-delay: 0.1\n")
                }
                "/redirect" => redirect_response("/target"),
                _ => response(200, "OK", "text/html", "<h1>Public</h1>"),
            })
            .await;
            let result = crawl(
                CrawlConfig {
                    mode: CrawlMode::List,
                    list_urls: vec![
                        format!("{base_url}a"),
                        format!("{base_url}b"),
                        format!("{base_url}redirect"),
                    ],
                    concurrency: 3,
                    requests_per_second: 100,
                    request_delay_ms: if respect_robots { 0 } else { 100 },
                    respect_robots,
                    ..CrawlConfig::default()
                },
                MemoryStore::new(),
                CrawlControl::default(),
                |_| {},
            )
            .await;
            server.abort();
            assert_eq!(result.unwrap().crawled, 3);
            let requests = requests.lock().unwrap();
            let page_requests = requests
                .iter()
                .filter(|(path, _)| path != "/robots.txt")
                .collect::<Vec<_>>();
            assert_eq!(page_requests.len(), 4);
            for pair in page_requests.windows(2) {
                let gap = pair[1].1.duration_since(pair[0].1);
                assert!(
                    gap >= Duration::from_millis(80),
                    "requests arrived only {gap:?} apart (respect_robots={respect_robots})"
                );
            }
            assert_eq!(
                requests
                    .iter()
                    .filter(|(path, _)| path == "/robots.txt")
                    .count(),
                usize::from(respect_robots)
            );
        }
    }

    #[tokio::test]
    async fn robots_server_errors_block_crawling_but_missing_robots_and_opt_out_allow_it() {
        for (robots_status, respect_robots, should_fetch) in
            [(503, true, false), (404, true, true), (503, false, true)]
        {
            let (base_url, requests, server) = spawn_recording_site(move |path| {
                if path == "/robots.txt" {
                    response(robots_status, "Unavailable", "text/plain", "unavailable")
                } else {
                    response(200, "OK", "text/html", "<h1>Public</h1>")
                }
            })
            .await;
            let store = MemoryStore::new();
            let result = crawl(
                CrawlConfig {
                    start_url: format!("{base_url}page"),
                    max_urls: 1,
                    request_delay_ms: 0,
                    respect_robots,
                    ..CrawlConfig::default()
                },
                store.clone(),
                CrawlControl::default(),
                |_| {},
            )
            .await;
            server.abort();
            assert_eq!(result.unwrap().crawled, 1);
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(path, _)| path == "/page"),
                should_fetch
            );
            let records = store.records();
            if should_fetch {
                assert_eq!(records[0].status_code, Some(200));
            } else {
                assert_eq!(records[0].status_code, None);
                assert!(records[0].error.as_deref().unwrap().contains("503"));
            }
        }
    }

    #[tokio::test]
    async fn robots_redirects_are_followed_before_crawling() {
        let (base_url, requests, server) = spawn_recording_site(|path| match path {
            "/robots.txt" => redirect_response("/rules-1"),
            "/rules-1" => redirect_response("/rules-2"),
            "/rules-2" => redirect_response("/rules-3"),
            "/rules-3" => redirect_response("/rules-4"),
            "/rules-4" => redirect_response("/rules-5"),
            "/rules-5" => response(
                200,
                "OK",
                "text/plain",
                "User-agent: *\nDisallow: /private\n",
            ),
            _ => response(200, "OK", "text/html", "<h1>Private</h1>"),
        })
        .await;
        let result = crawl(
            CrawlConfig {
                start_url: format!("{base_url}private"),
                max_urls: 1,
                request_delay_ms: 0,
                requests_per_second: 100,
                ..CrawlConfig::default()
            },
            MemoryStore::new(),
            CrawlControl::default(),
            |_| {},
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap().crawled, 1);
        let requests = requests.lock().unwrap();
        assert!(requests.iter().any(|(path, _)| path == "/rules-5"));
        assert!(!requests.iter().any(|(path, _)| path == "/private"));
    }

    #[tokio::test]
    async fn robots_redirects_honor_the_destination_origins_cached_delay() {
        let (destination, requests, destination_server) = spawn_recording_site(|path| match path {
            "/robots.txt" => response(200, "OK", "text/plain", "User-agent: *\nCrawl-delay: 0.1\n"),
            "/delegated-robots" => response(200, "OK", "text/plain", "User-agent: *\nAllow: /\n"),
            _ => response(200, "OK", "text/html", "<h1>Public</h1>"),
        })
        .await;
        let delegated_robots = format!("{destination}delegated-robots");
        let (source, _, source_server) = spawn_recording_site(move |path| {
            if path == "/robots.txt" {
                redirect_response(&delegated_robots)
            } else {
                response(200, "OK", "text/html", "<h1>Public</h1>")
            }
        })
        .await;
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            crawl(
                CrawlConfig {
                    mode: CrawlMode::List,
                    list_urls: vec![format!("{destination}warmup"), format!("{source}page")],
                    concurrency: 1,
                    requests_per_second: 100,
                    request_delay_ms: 0,
                    ..CrawlConfig::default()
                },
                MemoryStore::new(),
                CrawlControl::default(),
                |_| {},
            ),
        )
        .await;
        destination_server.abort();
        source_server.abort();
        assert_eq!(result.unwrap().unwrap().crawled, 2);
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            ["/robots.txt", "/warmup", "/delegated-robots"]
        );
        let gap = requests[2].1.duration_since(requests[1].1);
        assert!(
            gap >= Duration::from_millis(80),
            "robots redirect ignored cached 100 ms delay: {gap:?}"
        );
    }

    #[tokio::test]
    async fn robots_rules_also_guard_default_sitemap_requests() {
        let (base_url, requests, server) = spawn_recording_site(|path| {
            if path == "/robots.txt" {
                response(
                    200,
                    "OK",
                    "text/plain",
                    "User-agent: *\nDisallow: /sitemap.xml\n",
                )
            } else {
                response(200, "OK", "text/html", "<h1>Public</h1>")
            }
        })
        .await;
        let result = crawl(
            CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                request_delay_ms: 0,
                ..CrawlConfig::default()
            },
            MemoryStore::new(),
            CrawlControl::default(),
            |_| {},
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap().crawled, 1);
        assert!(
            !requests
                .lock()
                .unwrap()
                .iter()
                .any(|(path, _)| path == "/sitemap.xml")
        );
    }

    #[tokio::test]
    async fn indexability_preserves_http_errors_after_parsing_html() {
        let (base_url, _, server) = spawn_recording_site(|path| match path {
            "/missing" => response(
                404,
                "Not Found",
                "text/html",
                "<html><head><title>Missing</title></head><body>Not found</body></html>",
            ),
            "/failure" => response(
                500,
                "Internal Server Error",
                "text/html",
                r#"<html><head><title>Failure</title><meta name="robots" content="noindex"><link rel="canonical" href="/elsewhere"></head></html>"#,
            ),
            _ => response(200, "OK", "text/html", "<html><title>Public</title></html>"),
        })
        .await;
        let store = MemoryStore::new();
        let result = crawl(
            CrawlConfig {
                mode: CrawlMode::List,
                list_urls: ["missing", "failure", "public"]
                    .map(|path| format!("{base_url}{path}"))
                    .to_vec(),
                respect_robots: false,
                request_delay_ms: 0,
                requests_per_second: 100,
                retry_attempts: 0,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap().crawled, 3);
        let records = store.records();
        for (path, status, indexability, reason) in [
            ("missing", 404, "Non-indexable", "HTTP 404"),
            ("failure", 500, "Non-indexable", "HTTP 500"),
            ("public", 200, "Indexable", "Indexable"),
        ] {
            let record = records
                .iter()
                .find(|record| record.url == format!("{base_url}{path}"))
                .unwrap();
            assert_eq!(record.status_code, Some(status));
            assert_eq!(record.indexability, indexability, "path: {path}");
            assert_eq!(record.indexability_status, reason, "path: {path}");
            assert!(record.title.is_some());
        }
    }

    #[tokio::test]
    async fn indexability_honors_repeated_robots_headers_meta_and_non_html_directives() {
        let (base_url, _, server) = spawn_recording_site(|path| {
            let plain_html = "<html><head><title>Public</title></head></html>";
            match path {
                "/file.pdf" => robots_response("application/pdf", "%PDF-1.4", &["noindex"]),
                "/headers" => robots_response("text/html", plain_html, &["index, follow", "NoInDeX"]),
                "/header-none" => robots_response("text/html", plain_html, &["all", "NoNe"]),
                "/meta" => robots_response("text/html", r#"<html><head><meta name="robots" content="index"><meta name="ROBOTS" content="noindex"></head></html>"#, &["index, follow"]),
                "/meta-none" => robots_response("text/html", r#"<html><head><meta name="robots" content="none"></head></html>"#, &["index"]),
                "/preview" => robots_response("text/html", r#"<html><head><meta name="robots" content="max-image-preview: none"></head></html>"#, &["max-image-preview: none"]),
                _ => unreachable!(),
            }
        })
        .await;
        let store = MemoryStore::new();
        let result = crawl(
            CrawlConfig {
                mode: CrawlMode::List,
                list_urls: [
                    "file.pdf",
                    "headers",
                    "header-none",
                    "meta",
                    "meta-none",
                    "preview",
                ]
                .map(|path| format!("{base_url}{path}"))
                .to_vec(),
                respect_robots: false,
                request_delay_ms: 0,
                requests_per_second: 100,
                ..CrawlConfig::default()
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap().crawled, 6);
        let records = store.records();
        for (path, expected, reason) in [
            ("file.pdf", "Non-indexable", "X-Robots-Tag noindex"),
            ("headers", "Non-indexable", "X-Robots-Tag noindex"),
            ("header-none", "Non-indexable", "X-Robots-Tag noindex"),
            ("meta", "Non-indexable", "Meta robots noindex"),
            ("meta-none", "Non-indexable", "Meta robots noindex"),
            ("preview", "Indexable", "Indexable"),
        ] {
            let record = records
                .iter()
                .find(|record| record.url == format!("{base_url}{path}"))
                .unwrap();
            assert_eq!(record.indexability, expected, "path: {path}");
            assert_eq!(record.indexability_status, reason, "path: {path}");
        }
    }

    #[tokio::test]
    async fn nofollow_choices_separate_internal_external_discovery_and_keep_link_evidence() {
        for page_directive in ["", "meta", "header"] {
            for (internal, external, external_enabled) in [
                (false, false, true),
                (true, false, true),
                (false, true, true),
                (true, true, true),
                (true, true, false),
            ] {
                let (external_base, external_requests, external_server) =
                    spawn_recording_site(|_| {
                        response(
                            200,
                            "OK",
                            "text/html",
                            r#"<h1>External</h1><a href="/never-expand">Do not expand</a>"#,
                        )
                    })
                    .await;
                let external_url =
                    format!("{}target", external_base.replace("127.0.0.1", "localhost"));
                let external_target = external_url.clone();
                let (base_url, requests, server) = spawn_recording_site(move |path| {
                    if path == "/start" {
                        let meta = if page_directive == "meta" {
                            r#"<meta name="robots" content="nofollow">"#
                        } else {
                            ""
                        };
                        let headers = if page_directive == "header" {
                            vec!["nofollow"]
                        } else {
                            Vec::new()
                        };
                        robots_response(
                            "text/html",
                            &format!(
                                r#"<html><head>{meta}</head><body>
                            <a href="/target" rel="NoFoLlOw sponsored">Internal</a>
                            <a href="{external_target}" rel="nofollow ugc">External</a>
                            <a href="/ordinary">Ordinary</a></body></html>"#
                            ),
                            &headers,
                        )
                    } else {
                        response(200, "OK", "text/html", "<h1>Internal</h1>")
                    }
                })
                .await;
                let mut serialized = serde_json::to_value(CrawlConfig {
                    start_url: format!("{base_url}start"),
                    max_depth: 2,
                    request_delay_ms: 0,
                    requests_per_second: 100,
                    respect_robots: false,
                    // Legacy false must not override either explicit per-host choice.
                    follow_nofollow: false,
                    resource_types: CrawlResourceTypes {
                        external: external_enabled,
                        ..CrawlResourceTypes::default()
                    },
                    ..CrawlConfig::default()
                })
                .unwrap();
                serialized["followInternalNofollow"] = internal.into();
                serialized["followExternalNofollow"] = external.into();
                let config = serde_json::from_value(serialized).unwrap();
                let store = MemoryStore::new();
                let result = crawl(config, store.clone(), CrawlControl::default(), |_| {}).await;
                server.abort();
                external_server.abort();
                let context = format!(
                    "directive={page_directive}, internal={internal}, external={external}, external enabled={external_enabled}"
                );
                result.unwrap();
                let paths = requests.lock().unwrap();
                assert_eq!(
                    paths.iter().any(|(path, _)| path == "/target"),
                    internal,
                    "{context}"
                );
                assert_eq!(
                    paths.iter().any(|(path, _)| path == "/ordinary"),
                    page_directive.is_empty() || internal,
                    "{context}"
                );
                assert_eq!(
                    !external_requests.lock().unwrap().is_empty(),
                    external && external_enabled,
                    "{context}"
                );
                assert!(
                    external_requests
                        .lock()
                        .unwrap()
                        .iter()
                        .all(|(path, _)| path == "/target"),
                    "External checks must not expand links: {context}"
                );
                let edges = store.link_edges(ferrous_frog_storage::LinkEdgeQuery {
                    source_url: Some(format!("{base_url}start")),
                    ..ferrous_frog_storage::LinkEdgeQuery::default()
                });
                assert_eq!(edges.total, 3, "{context}");
                assert!(
                    edges
                        .edges
                        .iter()
                        .filter(|edge| edge.target_url.ends_with("/target"))
                        .all(|edge| edge.rel_nofollow),
                    "{context}"
                );
                assert!(
                    edges
                        .edges
                        .iter()
                        .any(|edge| edge.target_url == external_url
                            && edge.link_type == LinkType::External
                            && edge.rel == "nofollow ugc"),
                    "{context}"
                );
            }
        }
    }

    #[test]
    fn external_checks_preserve_scope_filters_sitemap_boundaries_and_resume_permissions() {
        let root = Url::parse("https://example.com/docs/start").unwrap();
        let external = Url::parse("https://other.test/outside").unwrap();
        let mut config = CrawlConfig {
            start_url: root.to_string(),
            subdomain_scope: SubdomainScope::ExactHost,
            folder_scope: FolderScope::StartFolder,
            resource_types: CrawlResourceTypes {
                external: true,
                ..CrawlResourceTypes::default()
            },
            ..CrawlConfig::default()
        };
        let rules = compile_scope_rules(&config).unwrap();
        let query_rules = compile_query_rules(&config).unwrap();
        assert!(scope_allows(&external, &root, &rules, &config));
        assert!(!scope_allows(
            &root.join("/outside").unwrap(),
            &root,
            &rules,
            &config
        ));
        config.folder_scope = FolderScope::ExactUrl;
        assert!(!scope_allows(&external, &root, &rules, &config));
        config.folder_scope = FolderScope::StartFolder;
        config.exclude_url_patterns = vec!["other\\.test".into()];
        assert!(!scope_allows(
            &external,
            &root,
            &compile_scope_rules(&config).unwrap(),
            &config
        ));
        config.exclude_url_patterns.clear();
        config.include_url_patterns = vec!["example\\.com".into()];
        assert!(!scope_allows(
            &external,
            &root,
            &compile_scope_rules(&config).unwrap(),
            &config
        ));
        config.include_url_patterns.clear();

        let (queue, seen) = seed_frontier(
            &config,
            &root,
            &query_rules,
            Vec::new(),
            vec![external.clone()],
            &rules,
        )
        .unwrap();
        assert_eq!(
            queue.len(),
            1,
            "An external sitemap entry must not become a site seed"
        );
        assert!(!seen.contains(external.as_str()));

        let store = MemoryStore::new();
        let mut queued = queue;
        queued.push_back(QueueItem {
            url: external.clone(),
            storage_key: external.to_string(),
            depth: 1,
            from_sitemap: false,
            list_position: None,
            list_duplicate_index: 0,
        });
        save_frontier_state(&store, &queued, &HashMap::new(), &seen, 0);
        for external_enabled in [true, false] {
            config.resource_types.external = external_enabled;
            let (restored, seen, _) =
                restore_frontier_state(&store, &root, &rules, &query_rules, &config)
                    .unwrap()
                    .unwrap();
            assert_eq!(
                restored.iter().any(|item| item.url == external),
                external_enabled
            );
            assert_eq!(seen.contains(external.as_str()), external_enabled);
        }
    }

    #[tokio::test]
    async fn indexability_page_nofollow_directives_respect_the_follow_setting() {
        for (source, directive) in [
            ("meta", "nofollow"),
            ("meta", "none"),
            ("header", "nofollow"),
            ("header", "none"),
            ("header", "max-image-preview: none nofollow"),
        ] {
            for follow_nofollow in [false, true] {
                let (base_url, requests, server) = spawn_recording_site(move |path| {
                    if path == "/start" {
                        let meta = if source == "meta" { directive } else { "index, follow" };
                        let headers = if source == "header" { vec!["index, follow", directive] } else { Vec::new() };
                        robots_response("text/html", &format!(r#"<html><head>
                            <meta name="robots" content="index, follow"><meta name="robots" content="{meta}">
                            <link rel="stylesheet" href="/style.css"><script src="/script.js"></script>
                            </head><body><a href="/target">Target</a><img src="/image.png"></body></html>"#), &headers)
                    } else {
                        response(200, "OK", "text/plain", "resource")
                    }
                })
                .await;
                let store = MemoryStore::new();
                let result = crawl(
                    CrawlConfig {
                        start_url: format!("{base_url}start"),
                        max_depth: 1,
                        respect_robots: false,
                        sitemap: SitemapConfig {
                            enabled: false,
                            ..SitemapConfig::default()
                        },
                        request_delay_ms: 0,
                        requests_per_second: 100,
                        follow_nofollow,
                        resource_types: CrawlResourceTypes {
                            images: true,
                            css: true,
                            javascript: true,
                            ..CrawlResourceTypes::default()
                        },
                        ..CrawlConfig::default()
                    },
                    store.clone(),
                    CrawlControl::default(),
                    |_| {},
                )
                .await;
                server.abort();
                assert_eq!(
                    result.unwrap().crawled,
                    if follow_nofollow { 5 } else { 1 },
                    "{source} {directive}, follow={follow_nofollow}"
                );
                assert_eq!(
                    requests.lock().unwrap().len(),
                    if follow_nofollow { 5 } else { 1 }
                );
                let edges = store.link_edges(ferrous_frog_storage::LinkEdgeQuery::default());
                assert_eq!(edges.edges.len(), 4);
                assert!(edges.edges.iter().all(|edge| edge.rel_nofollow));
                let records = store.records();
                let page = records
                    .iter()
                    .find(|record| record.url == format!("{base_url}start"))
                    .unwrap();
                assert_eq!(
                    page.indexability,
                    if directive == "none" {
                        "Non-indexable"
                    } else {
                        "Indexable"
                    }
                );
            }
        }
    }

    #[tokio::test]
    async fn crawls_mock_site_with_redirects_broken_links_and_duplicate_titles() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 12,
            max_depth: 3,
            concurrency: 2,
            requests_per_second: 20,
            request_delay_ms: 0,
            respect_robots: true,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        let result = crawl(config, store.clone(), CrawlControl::default(), |_| {}).await;
        server.abort();
        result.unwrap();

        let records = store.records();
        let home_record = records
            .iter()
            .find(|record| record.final_url == base_url)
            .expect("home page should be crawled");
        assert!(home_record.tcp_connect_time_ms.is_some());
        let missing_record = records
            .iter()
            .find(|record| {
                record.final_url == format!("{base_url}missing") && record.status_code == Some(404)
            })
            .expect("missing URL should be crawled with 404 status");
        assert_eq!(
            missing_record.first_inlink_source_url.as_deref(),
            Some(base_url.as_str())
        );
        assert_eq!(
            missing_record.first_inlink_anchor_text.as_deref(),
            Some("Missing page")
        );
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}target") && !record.redirect_chain.is_empty()
        }));
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}blocked")
                && record.status_text == "Blocked by robots.txt"
        }));
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}hreflang")
                && record.hreflang_invalid_count == 1
                && record.hreflang_missing_self_reference
        }));
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}structured") && record.json_ld_invalid_count == 1
        }));
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}orphan")
                && record.in_sitemap
                && record.inlink_count == 0
        }));

        let duplicate_titles = store.query(ferrous_frog_storage::GridQuery {
            view: IssueView::TitleDuplicate,
            ..ferrous_frog_storage::GridQuery::default()
        });
        assert_eq!(duplicate_titles.total, 2);
        let hreflang_issues = store.query(ferrous_frog_storage::GridQuery {
            view: IssueView::HreflangInvalid,
            ..ferrous_frog_storage::GridQuery::default()
        });
        assert_eq!(hreflang_issues.total, 1);
        let structured_data_issues = store.query(ferrous_frog_storage::GridQuery {
            view: IssueView::StructuredDataInvalid,
            ..ferrous_frog_storage::GridQuery::default()
        });
        assert_eq!(structured_data_issues.total, 1);
        let sitemap_orphans = store.query(ferrous_frog_storage::GridQuery {
            view: IssueView::SitemapOrphan,
            ..ferrous_frog_storage::GridQuery::default()
        });
        assert_eq!(sitemap_orphans.total, 1);
        assert_eq!(store.summary().sitemap_orphans, 1);

        let edges = store.link_edges(ferrous_frog_storage::LinkEdgeQuery {
            limit: 100,
            ..ferrous_frog_storage::LinkEdgeQuery::default()
        });
        assert!(
            edges
                .edges
                .iter()
                .any(|edge| edge.target_url == format!("{base_url}missing")
                    && edge.target_status_code == Some(404))
        );
    }

    #[tokio::test]
    async fn resumes_from_persisted_frontier_state() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();
        let resumed_url = format!("{base_url}a");
        let mut existing_root = CrawlRecord::pending(base_url.clone(), 0);
        existing_root.status_code = Some(299);
        store.upsert(existing_root);
        store.save_frontier_state(CrawlFrontierState {
            queued: vec![CrawlFrontierItem {
                url: resumed_url.clone(),
                depth: 1,
                from_sitemap: false,
                storage_key: resumed_url.clone(),
                list_position: None,
                list_duplicate_index: 0,
            }],
            seen: vec![base_url.clone(), resumed_url.clone()],
            crawled: 1,
        });

        let config = CrawlConfig {
            start_url: base_url.clone(),
            max_urls: 2,
            max_depth: 3,
            concurrency: 1,
            requests_per_second: 50,
            request_delay_ms: 0,
            respect_robots: false,
            timeout_secs: 5,
            resume_from_state: true,
            ..CrawlConfig::default()
        };

        let progress = crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        let records = store.records();
        assert_eq!(progress.crawled, 2);
        assert_eq!(
            records
                .iter()
                .find(|record| record.final_url == base_url)
                .and_then(|record| record.status_code),
            Some(299)
        );
        assert!(
            records.iter().any(|record| {
                record.final_url == resumed_url && record.status_code == Some(200)
            })
        );
        assert_eq!(store.load_frontier_state(), None);
    }

    #[tokio::test]
    async fn flags_redirect_loops_before_redirect_limit() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: format!("{base_url}loop-a"),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 4,
            max_depth: 1,
            concurrency: 1,
            requests_per_second: 20,
            request_delay_ms: 0,
            respect_robots: false,
            sitemap: SitemapConfig {
                enabled: false,
                ..SitemapConfig::default()
            },
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 10,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        let result = crawl(config, store.clone(), CrawlControl::default(), |_| {}).await;
        server.abort();
        result.unwrap();

        let records = store.records();
        assert_eq!(records.len(), 1);
        assert!(
            records[0]
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("Redirect loop detected")
        );
        assert_eq!(records[0].redirect_chain.len(), 2);
    }

    #[tokio::test]
    async fn applies_robots_crawl_delay() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url,
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 2,
            max_depth: 1,
            concurrency: 1,
            requests_per_second: 100,
            request_delay_ms: 0,
            respect_robots: true,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        let progress = crawl(config, store, CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        assert!(progress.elapsed_ms >= 30);
    }

    #[tokio::test]
    async fn applies_exclude_scope_patterns() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 12,
            max_depth: 3,
            concurrency: 2,
            requests_per_second: 20,
            request_delay_ms: 0,
            respect_robots: true,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: vec!["/missing$".to_string()],
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        assert!(
            !store
                .records()
                .iter()
                .any(|record| record.final_url == format!("{base_url}missing"))
        );
    }

    #[tokio::test]
    async fn can_record_but_not_crawl_nofollow_links() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 20,
            max_depth: 1,
            concurrency: 2,
            requests_per_second: 20,
            request_delay_ms: 0,
            respect_robots: false,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: false,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        assert!(
            !store
                .records()
                .iter()
                .any(|record| { record.final_url == format!("{base_url}nofollow-target") })
        );

        let edges = store.link_edges(ferrous_frog_storage::LinkEdgeQuery {
            limit: 100,
            ..ferrous_frog_storage::LinkEdgeQuery::default()
        });
        assert!(edges.edges.iter().any(|edge| {
            edge.target_url == format!("{base_url}nofollow-target") && edge.rel_nofollow
        }));
    }

    #[tokio::test]
    async fn uses_custom_robots_txt_override() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 12,
            max_depth: 2,
            concurrency: 2,
            requests_per_second: 20,
            request_delay_ms: 0,
            respect_robots: true,
            use_robots_txt_override: true,
            robots_txt_override: "User-agent: *\nDisallow: /missing\nAllow: /\n".to_string(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        assert!(store.records().iter().any(|record| {
            record.final_url == format!("{base_url}missing")
                && record.status_text == "Blocked by robots.txt"
        }));
    }

    #[tokio::test]
    async fn downloads_robots_txt_for_override() {
        let (base_url, server) = spawn_mock_site().await;

        let result = download_robots_txt(RobotsTxtDownloadRequest {
            url: base_url.clone(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            request_headers: Vec::new(),
        })
        .await
        .unwrap();
        server.abort();

        assert_eq!(result.robots_url, format!("{base_url}robots.txt"));
        assert_eq!(result.status_code, 200);
        assert!(result.robots_txt.contains("Disallow: /blocked"));
    }

    #[tokio::test]
    async fn crawls_enabled_asset_resource_types() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 20,
            max_depth: 1,
            concurrency: 4,
            requests_per_second: 50,
            request_delay_ms: 0,
            respect_robots: false,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes {
                images: true,
                css: true,
                javascript: true,
                ..CrawlResourceTypes::default()
            },
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        let records = store.records();
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}assets/logo.png")
                && record.content_type.as_deref() == Some("image/png")
        }));
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}assets/site.css")
                && record.content_type.as_deref() == Some("text/css")
        }));
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}assets/app.js")
                && record.content_type.as_deref() == Some("application/javascript")
        }));
    }

    #[tokio::test]
    async fn normalizes_query_parameters_before_deduping() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 20,
            max_depth: 1,
            concurrency: 4,
            requests_per_second: 50,
            request_delay_ms: 0,
            respect_robots: false,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings {
                sort_parameters: true,
                strip_all: false,
                max_parameters: 2,
                strip_parameter_patterns: vec!["^utm_".to_string()],
            },
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        assert!(store.records().iter().any(|record| {
            record.final_url == format!("{base_url}params?a=1&b=2")
                && record.status_code == Some(200)
        }));
        assert!(
            !store
                .records()
                .iter()
                .any(|record| { record.final_url.contains("utm_source") })
        );
    }

    #[tokio::test]
    async fn runs_custom_searches_against_raw_html() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 1,
            max_depth: 0,
            concurrency: 1,
            requests_per_second: 50,
            request_delay_ms: 0,
            respect_robots: false,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: vec![CustomSearch {
                name: "duplicate_links".to_string(),
                pattern: "Duplicate".to_string(),
                regex: false,
                case_sensitive: true,
                max_snippets: 2,
            }],
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        let records = store.records();
        let search = records[0]
            .custom_searches
            .iter()
            .find(|search| search.name == "duplicate_links")
            .expect("custom search result should be stored");
        assert!(search.matched);
        assert_eq!(search.match_count, 2);
        assert_eq!(search.source, CustomSearchSource::RawHtml);
        assert_eq!(search.snippets.len(), 2);
    }

    #[test]
    fn custom_search_values_preserve_raw_and_rendered_sources() {
        let searches = [CustomSearch {
            name: "injected".to_string(),
            pattern: "Injected".to_string(),
            regex: false,
            case_sensitive: true,
            max_snippets: 1,
        }];
        let mut values = search_custom_values(
            "<html><body>Raw</body></html>",
            &searches,
            CustomSearchSource::RawHtml,
        );
        values.extend(search_custom_values(
            "<html><body>Injected</body></html>",
            &searches,
            CustomSearchSource::RenderedHtml,
        ));

        assert_eq!(values.len(), 2);
        assert_eq!(values[0].source, CustomSearchSource::RawHtml);
        assert!(!values[0].matched);
        assert_eq!(values[1].source, CustomSearchSource::RenderedHtml);
        assert!(values[1].matched);
    }

    #[tokio::test]
    async fn preserves_encoded_spaces_when_fetching_discovered_urls() {
        let encoded_hits = Arc::new(AtomicUsize::new(0));
        let double_encoded_hits = Arc::new(AtomicUsize::new(0));
        let (base_url, server) =
            spawn_encoded_space_site(encoded_hits.clone(), double_encoded_hits.clone()).await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: format!("{base_url}start"),
            list_urls: Vec::new(),
            list_sitemap_urls: Vec::new(),
            max_urls: 5,
            max_depth: 1,
            concurrency: 2,
            requests_per_second: 50,
            request_delay_ms: 0,
            respect_robots: false,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        let target_url = format!("{base_url}en/content-hub/event/MVNOs%20World%202026");
        assert!(
            store.records().iter().any(|record| {
                record.final_url == target_url && record.status_code == Some(200)
            })
        );
        assert_eq!(encoded_hits.load(Ordering::SeqCst), 1);
        assert_eq!(double_encoded_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn list_mode_crawls_only_supplied_urls() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::List,
            start_url: base_url.clone(),
            list_urls: vec![
                format!("{base_url}a"),
                format!("{base_url}missing"),
                format!("{base_url}a"),
            ],
            list_sitemap_urls: Vec::new(),
            max_urls: 20,
            max_depth: 3,
            concurrency: 2,
            requests_per_second: 50,
            request_delay_ms: 0,
            respect_robots: false,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        let records = store.records();
        assert_eq!(records.len(), 3);
        assert!(
            records
                .iter()
                .any(|record| record.final_url == format!("{base_url}a"))
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.final_url == format!("{base_url}a"))
                .count(),
            2
        );
        assert!(
            records
                .iter()
                .any(|record| record.final_url == format!("{base_url}missing")
                    && record.status_code == Some(404))
        );
        assert!(!records.iter().any(|record| record.final_url == base_url));

        let ordered = store.query(GridQuery::default()).rows;
        assert_eq!(ordered[0].list_position, Some(1));
        assert_eq!(ordered[0].list_duplicate_index, 1);
        assert_eq!(ordered[1].list_position, Some(2));
        assert_eq!(ordered[2].list_position, Some(3));
        assert_eq!(ordered[2].list_duplicate_index, 2);
    }

    #[tokio::test]
    async fn list_mode_expands_sitemap_url_sources() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::List,
            sitemap: SitemapConfig {
                enabled: false,
                discover_from_robots: false,
                probe_default: false,
                follow_linked: false,
                urls: vec!["https://example.invalid/unused.xml".into()],
            },
            start_url: base_url.clone(),
            list_urls: Vec::new(),
            list_sitemap_urls: vec![format!("{base_url}list-sitemap.xml")],
            max_urls: 20,
            max_depth: 3,
            concurrency: 2,
            requests_per_second: 50,
            request_delay_ms: 0,
            respect_robots: false,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: "FerrousFrogSeoSpider/Test".to_string(),
            timeout_secs: 5,
            max_redirects: 5,
            retry_attempts: 1,
            retry_backoff_ms: 10,
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            subdomain_scope: SubdomainScope::default(),
            folder_scope: FolderScope::default(),
            follow_nofollow: true,
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
            custom_searches: Vec::new(),
            rendering: JsRenderingConfig::default(),
            resume_from_state: false,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        let records = store.records();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records
                .iter()
                .filter(|record| record.final_url == format!("{base_url}a"))
                .count(),
            2
        );
        assert!(
            records
                .iter()
                .any(|record| record.final_url == format!("{base_url}missing")
                    && record.status_code == Some(404))
        );
        assert!(records.iter().all(|record| record.in_sitemap));

        let ordered = store.query(GridQuery::default()).rows;
        assert_eq!(ordered[0].list_position, Some(1));
        assert_eq!(ordered[1].list_position, Some(2));
        assert_eq!(ordered[2].list_position, Some(3));
        assert_eq!(ordered[2].list_duplicate_index, 2);
    }

    #[tokio::test]
    async fn retries_retryable_status_codes() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let (base_url, server) = spawn_retry_site(attempts.clone()).await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            start_url: base_url.clone(),
            max_urls: 1,
            max_depth: 0,
            requests_per_second: 0,
            request_delay_ms: 0,
            respect_robots: false,
            retry_attempts: 1,
            retry_backoff_ms: 1,
            ..CrawlConfig::default()
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        let records = store.records();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status_code, Some(200));
    }

    #[tokio::test]
    async fn stop_during_worker_completion_preserves_the_pending_url_for_resume() {
        let control = CrawlControl::default();
        let site_control = control.clone();
        let visits = AtomicUsize::new(0);
        let (base_url, _, server) = spawn_recording_site(move |path| {
            if path == "/" {
                if visits.fetch_add(1, Ordering::SeqCst) == 0 {
                    site_control.cancel();
                }
                response(
                    200,
                    "OK",
                    "text/html",
                    "<title>Completed after resume</title>",
                )
            } else {
                response(404, "Not Found", "text/plain", "")
            }
        })
        .await;
        let store = MemoryStore::new();
        let config = CrawlConfig {
            start_url: base_url.clone(),
            max_urls: 1,
            requests_per_second: 100,
            ..CrawlConfig::default()
        };
        let progress = crawl(config.clone(), store.clone(), control, |_| {})
            .await
            .unwrap();
        assert_eq!(progress.status, "stopped");
        assert!(
            store.records().is_empty(),
            "A worker completing after Stop must remain pending"
        );
        let frontier = store.load_frontier_state().unwrap();
        assert_eq!(frontier.queued.len(), 1);
        assert_eq!(frontier.queued[0].url, base_url);
        let progress = crawl(
            CrawlConfig {
                resume_from_state: true,
                ..config
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(progress.status, "finished");
        assert_eq!(
            store.records()[0].title.as_deref(),
            Some("Completed after resume")
        );
        server.abort();
    }

    #[tokio::test]
    async fn stop_cancels_active_requests_without_waiting_for_timeout() {
        let (base_url, server) = spawn_slow_page_site(Duration::from_secs(5)).await;
        let store = MemoryStore::new();
        let control = CrawlControl::default();
        let task_control = control.clone();
        let started_at = Instant::now();

        let handle = tokio::spawn(async move {
            let config = CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                max_depth: 0,
                concurrency: 1,
                requests_per_second: 0,
                request_delay_ms: 0,
                respect_robots: false,
                timeout_secs: 10,
                ..CrawlConfig::default()
            };
            crawl(config, store, task_control, |_| {}).await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        control.cancel();

        let progress = handle.await.unwrap().unwrap();
        server.abort();

        assert_eq!(progress.status, "stopped");
        assert!(started_at.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn pause_blocks_worker_before_following_redirect_request() {
        let requests = Arc::new(AtomicUsize::new(0));
        let (base_url, server) = spawn_pause_redirect_site(requests.clone()).await;
        let store = MemoryStore::new();
        let control = CrawlControl::default();
        let task_control = control.clone();

        let handle = tokio::spawn(async move {
            let config = CrawlConfig {
                start_url: base_url,
                max_urls: 1,
                max_depth: 0,
                concurrency: 1,
                requests_per_second: 0,
                request_delay_ms: 0,
                respect_robots: false,
                timeout_secs: 10,
                ..CrawlConfig::default()
            };
            crawl(config, store, task_control, |_| {}).await
        });

        for _ in 0..50 {
            if requests.load(Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        control.pause();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(requests.load(Ordering::SeqCst), 1);

        control.resume();
        let progress = handle.await.unwrap().unwrap();
        server.abort();

        assert_eq!(progress.status, "finished");
        assert_eq!(requests.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "runs a 1,000-page localhost crawler load test against Memory and file-backed SQLite"]
    async fn synthetic_local_site_crawler_load() {
        use ferrous_frog_storage::{ActiveStore, LinkEdgeQuery};

        const PAGES: usize = 1_000;
        const PLANT_EVERY: usize = 100;
        const PLANTED: usize = PAGES / PLANT_EVERY;
        const CONCURRENCY: usize = 8;
        const STOP_AFTER: usize = 250;
        const EXPECTED_URLS: usize = PAGES + 3 * PLANTED;
        const EXPECTED_EDGES: usize = 7 * PAGES + 11 * PLANTED;
        const FRESH_REQUESTS: usize = PAGES + 3 * PLANTED + 1;

        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak_in_flight = Arc::new(AtomicUsize::new(0));
        let active = in_flight.clone();
        let peak = peak_in_flight.clone();
        let (base_url, requests, server) = spawn_recording_site_with_async_response(move |path, _| {
            let reply = if path == "/robots.txt" {
                response(200, "OK", "text/plain", "User-agent: *\nDisallow: /private/\n")
            } else if let Some(index) = path.strip_prefix("/page/").and_then(|value| value.parse::<usize>().ok()).filter(|index| *index < PAGES) {
                let next = (index + 1) % PAGES;
                let mut html = format!("<html><head><title>Page {index}</title><meta name='description' content='Synthetic page {index}'><link rel='canonical' href='/page/{index}'></head><body><h1>Page {index}</h1>");
                for target in [next, (index + PAGES - 1) % PAGES, (index + 31) % PAGES, (index * 2 + 1) % PAGES, (index * 2 + 2) % PAGES] {
                    html.push_str(&format!("<a href='/page/{target}'>Page</a>"));
                }
                html.push_str(&format!("<a href='/page/{next}#duplicate'>Fragment duplicate</a><a href='/page/{next}?tracking={index}'>Query duplicate</a>"));
                if index % PLANT_EVERY == 0 {
                    let planted = index / PLANT_EVERY;
                    for kind in ["broken", "redirect", "private", "excluded"] {
                        html.push_str(&format!("<a href='/{kind}/{planted}'>{kind}</a>"));
                    }
                }
                html.push_str("</body></html>");
                response(200, "OK", "text/html", &html)
            } else if let Some(index) = path.strip_prefix("/redirect/").and_then(|value| value.parse::<usize>().ok()).filter(|index| *index < PLANTED) {
                redirect_response(&format!("/page/{}", index * PLANT_EVERY + 1))
            } else {
                response(404, "Not Found", "text/html", "<title>Missing</title>")
            };
            let active = active.clone();
            let peak = peak.clone();
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                sleep(Duration::from_millis(5)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                reply
            }
        }).await;
        let config = CrawlConfig {
            start_url: format!("{base_url}page/0"),
            max_urls: EXPECTED_URLS,
            max_depth: PAGES,
            concurrency: CONCURRENCY,
            requests_per_second: 0,
            request_delay_ms: 0,
            respect_robots: true,
            timeout_secs: 10,
            retry_attempts: 0,
            max_response_bytes: 256 * 1024,
            exclude_url_patterns: vec!["/excluded/".into()],
            query_settings: QuerySettings {
                strip_all: true,
                ..QuerySettings::default()
            },
            sitemap: SitemapConfig {
                enabled: false,
                ..SitemapConfig::default()
            },
            ..CrawlConfig::default()
        };

        for backend in ["memory", "sqlite"] {
            for interrupted in [false, true] {
                requests.lock().unwrap().clear();
                peak_in_flight.store(0, Ordering::SeqCst);
                let path = std::env::temp_dir().join(format!(
                    "ferrous-frog-crawler-load-{}-{}.sqlite3",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                let mut store = if backend == "sqlite" {
                    ActiveStore::sqlite(&path).unwrap()
                } else {
                    ActiveStore::memory()
                };
                let control = CrawlControl::default();
                let cancel = control.clone();
                let deadline_control = control.clone();
                let deadline = tokio::spawn(async move {
                    sleep(Duration::from_secs(120)).await;
                    deadline_control.cancel();
                });
                let started = Instant::now();
                let mut completed_before_stop = HashMap::new();
                let first = tokio::time::timeout(
                    Duration::from_secs(120),
                    crawl(config.clone(), store.clone(), control, move |event| {
                        if interrupted
                            && event.kind == "record"
                            && event
                                .progress
                                .is_some_and(|progress| progress.crawled == STOP_AFTER)
                        {
                            cancel.cancel();
                        }
                    }),
                )
                .await
                .expect("synthetic crawl exceeded its 120-second limit")
                .unwrap();
                deadline.abort();
                if interrupted {
                    assert_eq!(first.status, "stopped");
                    assert_eq!(first.crawled, STOP_AFTER);
                    let frontier = store
                        .load_frontier_state()
                        .expect("Stop must retain the frontier");
                    assert!(!frontier.queued.is_empty());
                    assert_eq!(frontier.crawled, STOP_AFTER);
                    assert_eq!(
                        frontier.queued.len() + frontier.crawled,
                        frontier.seen.len()
                    );
                    assert!(frontier.seen.len() <= EXPECTED_URLS);
                    completed_before_stop
                        .extend(store.records().into_iter().map(|row| (row.url, row.id)));
                    assert_eq!(completed_before_stop.len(), STOP_AFTER);
                    eprintln!(
                        "Crawler load {backend}/stop: {} records, {} pending, {} seen, {:.3}s",
                        frontier.crawled,
                        frontier.queued.len(),
                        frontier.seen.len(),
                        started.elapsed().as_secs_f64()
                    );
                    if backend == "sqlite" {
                        drop(store);
                        store = ActiveStore::sqlite(&path).unwrap();
                        assert_eq!(
                            store.load_frontier_state().unwrap().queued.len(),
                            frontier.queued.len()
                        );
                    }
                    // Let the mock server finish responses whose clients Stop cancelled.
                    tokio::time::timeout(Duration::from_secs(1), async {
                        while in_flight.load(Ordering::SeqCst) != 0 {
                            sleep(Duration::from_millis(1)).await;
                        }
                    })
                    .await
                    .unwrap();
                    let resume_control = CrawlControl::default();
                    let deadline_control = resume_control.clone();
                    let deadline = tokio::spawn(async move {
                        sleep(Duration::from_secs(120)).await;
                        deadline_control.cancel();
                    });
                    let resumed = tokio::time::timeout(
                        Duration::from_secs(120),
                        crawl(
                            CrawlConfig {
                                resume_from_state: true,
                                ..config.clone()
                            },
                            store.clone(),
                            resume_control,
                            |_| {},
                        ),
                    )
                    .await
                    .expect("resumed synthetic crawl exceeded its 120-second limit")
                    .unwrap();
                    deadline.abort();
                    assert_eq!(resumed.status, "finished");
                    assert_eq!(resumed.crawled, EXPECTED_URLS);
                } else {
                    assert_eq!(first.status, "finished");
                    assert_eq!(first.crawled, EXPECTED_URLS);
                }
                let elapsed = started.elapsed();
                assert!(store.load_frontier_state().is_none());
                let rows = store.records();
                assert_eq!(rows.len(), EXPECTED_URLS);
                assert_eq!(
                    rows.iter()
                        .map(|row| &row.url)
                        .collect::<HashSet<_>>()
                        .len(),
                    EXPECTED_URLS
                );
                assert_eq!(
                    rows.iter()
                        .filter(|row| row.status_code == Some(404))
                        .count(),
                    PLANTED
                );
                assert_eq!(
                    rows.iter()
                        .filter(|row| row.status_text == "Blocked by robots.txt")
                        .count(),
                    PLANTED
                );
                assert_eq!(
                    rows.iter()
                        .filter(|row| !row.redirect_chain.is_empty())
                        .count(),
                    PLANTED
                );
                assert!(rows.iter().all(|row| row.status_code == Some(200)
                    || row.status_code == Some(404)
                    || row.status_text == "Blocked by robots.txt"));
                for row in &rows {
                    if let Some(id) = completed_before_stop.get(&row.url) {
                        assert_eq!(row.id, *id, "Resume must retain completed records");
                    }
                    if !row.redirect_chain.is_empty() {
                        assert_eq!(row.redirect_chain.len(), 1);
                        assert_eq!(row.redirect_chain[0].status_code, 302);
                        assert!(row.url.contains("/redirect/"));
                        assert!(row.final_url.contains("/page/"));
                    }
                }
                let edges = store.link_edges(LinkEdgeQuery {
                    limit: 1,
                    ..LinkEdgeQuery::default()
                });
                assert_eq!(edges.total, EXPECTED_EDGES);
                let summary = store.summary();
                assert_eq!(summary.total, EXPECTED_URLS);
                assert_eq!(summary.broken, PLANTED);
                assert_eq!(
                    store
                        .query(GridQuery {
                            view: IssueView::BrokenLinks,
                            limit: 1,
                            ..GridQuery::default()
                        })
                        .total,
                    PLANTED
                );
                let requests = requests.lock().unwrap();
                assert!(
                    requests
                        .iter()
                        .all(|(path, _)| !path.starts_with("/private/")
                            && !path.starts_with("/excluded/")
                            && !path.contains('?'))
                );
                assert_eq!(
                    requests
                        .iter()
                        .map(|(path, _)| path)
                        .collect::<HashSet<_>>()
                        .len(),
                    PAGES + 2 * PLANTED + 1
                );
                assert_eq!(
                    requests
                        .iter()
                        .filter(|(path, _)| path == "/robots.txt")
                        .count(),
                    if interrupted { 2 } else { 1 }
                );
                if interrupted {
                    assert!(
                        (FRESH_REQUESTS + 1..=FRESH_REQUESTS + 1 + 2 * CONCURRENCY)
                            .contains(&requests.len()),
                        "Stop may retry only the bounded in-flight requests: {}",
                        requests.len()
                    );
                } else {
                    assert_eq!(
                        requests.len(),
                        FRESH_REQUESTS,
                        "Cycles and duplicate links must not refetch pages"
                    );
                }
                let peak = peak_in_flight.load(Ordering::SeqCst);
                assert!(
                    (2..=CONCURRENCY).contains(&peak),
                    "peak HTTP concurrency: {peak}"
                );
                assert_eq!(in_flight.load(Ordering::SeqCst), 0);
                eprintln!(
                    "Crawler load {backend}/{}: {} URLs, {} HTTP requests, {} edges, peak HTTP concurrency {}, {:.3}s ({:.1} URLs/s)",
                    if interrupted { "resumed" } else { "complete" },
                    rows.len(),
                    requests.len(),
                    edges.total,
                    peak,
                    elapsed.as_secs_f64(),
                    rows.len() as f64 / elapsed.as_secs_f64()
                );
                drop(requests);
                drop(store);
                if backend == "sqlite" {
                    std::fs::remove_file(&path).unwrap();
                }
            }
        }
        server.abort();
    }

    type RecordedRequests = Arc<std::sync::Mutex<Vec<(String, Instant)>>>;

    fn request_header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        request.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }

    fn robots_response(content_type: &str, body: &str, directives: &[&str]) -> String {
        let headers = directives
            .iter()
            .map(|directive| format!("X-Robots-Tag: {directive}\r\n"))
            .collect::<String>();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn redirect_response(target: &str) -> String {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    }

    async fn spawn_recording_site(
        handler: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> (String, RecordedRequests, tokio::task::JoinHandle<()>) {
        spawn_recording_site_with_request(move |path, _| handler(path)).await
    }

    async fn spawn_recording_site_with_request(
        handler: impl Fn(&str, &str) -> String + Send + Sync + 'static,
    ) -> (String, RecordedRequests, tokio::task::JoinHandle<()>) {
        spawn_recording_site_with_async_response(move |path, request| {
            std::future::ready(handler(path, request))
        })
        .await
    }

    async fn spawn_recording_site_with_async_response<F>(
        handler: impl Fn(&str, &str) -> F + Send + Sync + 'static,
    ) -> (String, RecordedRequests, tokio::task::JoinHandle<()>)
    where
        F: std::future::Future<Output = String> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let handler = Arc::new(handler);
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let requests = recorded.clone();
                let handler = handler.clone();
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 2048];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    requests
                        .lock()
                        .unwrap()
                        .push((path.to_string(), Instant::now()));
                    let response = handler(path, &request).await;
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (base_url, requests, server)
    }

    async fn spawn_mock_site() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}/");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 2048];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let response = mock_response(path);
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (base_url, handle)
    }

    async fn spawn_pause_redirect_site(
        requests: Arc<AtomicUsize>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}/");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let requests = requests.clone();
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 2048];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let response = match path {
                        "/" => {
                            requests.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(150)).await;
                            "HTTP/1.1 302 Found\r\nLocation: /after-pause\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                        }
                        "/after-pause" => {
                            requests.fetch_add(1, Ordering::SeqCst);
                            response(
                                200,
                                "OK",
                                "text/html",
                                "<html><head><title>After Pause</title></head><body><h1>After Pause</h1></body></html>",
                            )
                        }
                        _ => response(404, "Not Found", "text/plain", "not found"),
                    };
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (base_url, handle)
    }

    async fn spawn_encoded_space_site(
        encoded_hits: Arc<AtomicUsize>,
        double_encoded_hits: Arc<AtomicUsize>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}/");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let encoded_hits = encoded_hits.clone();
                let double_encoded_hits = double_encoded_hits.clone();
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 2048];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let body =
                        "<html><head><title>Event</title></head><body><h1>Event</h1></body></html>";
                    let output = match path {
                        "/start" => response(
                            200,
                            "OK",
                            "text/html",
                            r#"
                                <html>
                                  <head><title>Start</title></head>
                                  <body>
                                    <a href="/en/content-hub/event/MVNOs%20World%202026">Event</a>
                                  </body>
                                </html>
                            "#,
                        ),
                        "/en/content-hub/event/MVNOs%20World%202026" => {
                            encoded_hits.fetch_add(1, Ordering::SeqCst);
                            response(200, "OK", "text/html", body)
                        }
                        "/en/content-hub/event/MVNOs%2520World%25202026" => {
                            double_encoded_hits.fetch_add(1, Ordering::SeqCst);
                            response(404, "Not Found", "text/html", "<h1>Double encoded</h1>")
                        }
                        _ => response(404, "Not Found", "text/html", "<h1>Not found</h1>"),
                    };
                    let _ = stream.write_all(output.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (base_url, handle)
    }

    async fn spawn_slow_page_site(delay: Duration) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}/");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 2048];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    if path == "/" {
                        tokio::time::sleep(delay).await;
                        let body = "<html><head><title>Slow Page</title></head><body><h1>Slow</h1></body></html>";
                        let _ = stream
                            .write_all(response(200, "OK", "text/html", body).as_bytes())
                            .await;
                    } else {
                        let _ = stream
                            .write_all(
                                response(404, "Not Found", "text/plain", "not found").as_bytes(),
                            )
                            .await;
                    }
                    let _ = stream.shutdown().await;
                });
            }
        });
        (base_url, handle)
    }

    async fn spawn_retry_site(attempts: Arc<AtomicUsize>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}/");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let attempts = attempts.clone();
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 2048];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let response = if path == "/" {
                        let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                        if attempt == 0 {
                            response(503, "Service Unavailable", "text/plain", "try again")
                        } else {
                            response(200, "OK", "text/html", "<html><body>ok</body></html>")
                        }
                    } else {
                        response(404, "Not Found", "text/plain", "not found")
                    };
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (base_url, handle)
    }

    fn mock_response(path: &str) -> String {
        match path {
            "/robots.txt" => response(
                200,
                "OK",
                "text/plain",
                "User-agent: *\nDisallow: /blocked\nCrawl-delay: 0.02\nAllow: /\n",
            ),
            "/sitemap.xml" => response(
                200,
                "OK",
                "application/xml",
                r#"
                    <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                      <url><loc>/orphan</loc></url>
                    </urlset>
                "#,
            ),
            "/list-sitemap.xml" => response(
                200,
                "OK",
                "application/xml",
                r#"
                    <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                      <url><loc>/a</loc></url>
                      <url><loc>/missing</loc></url>
                      <url><loc>/a</loc></url>
                    </urlset>
                "#,
            ),
            "/" => response(
                200,
                "OK",
                "text/html",
                r#"
                    <html>
                      <head>
                        <title>Home page with a useful title</title>
                        <meta name="description" content="A test home page with useful crawl links.">
                        <link rel="stylesheet" href="/assets/site.css">
                        <script src="/assets/app.js"></script>
                      </head>
                      <body>
                        <h1>Home</h1>
                        <img src="/assets/logo.png" alt="Logo">
                        <a href="/a">Duplicate A</a>
                        <a href="/b">Duplicate B</a>
                        <a href="/missing">Missing page</a>
                        <a href="/redirect">Redirecting page</a>
                        <a href="/blocked">Robots blocked page</a>
                        <a href="/hreflang">Hreflang issue page</a>
                        <a href="/structured">Structured data issue page</a>
                        <a href="/params?b=2&utm_source=test&a=1&keep=3">Parameterized page</a>
                        <a href="/nofollow-target" rel="nofollow">Nofollow target</a>
                      </body>
                    </html>
                "#,
            ),
            "/a" => duplicate_page("Duplicate A"),
            "/b" => duplicate_page("Duplicate B"),
            "/missing" => response(404, "Not Found", "text/html", "<h1>Missing</h1>"),
            "/redirect" => {
                "HTTP/1.1 301 Moved Permanently\r\nLocation: /target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            }
            "/loop-a" => {
                "HTTP/1.1 302 Found\r\nLocation: /loop-b\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            }
            "/loop-b" => {
                "HTTP/1.1 302 Found\r\nLocation: /loop-a\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            }
            "/target" => response(
                200,
                "OK",
                "text/html",
                "<html><head><title>Redirect Target</title></head><body><h1>Target</h1></body></html>",
            ),
            "/blocked" => response(
                200,
                "OK",
                "text/html",
                "<html><head><title>Blocked Page</title></head><body><h1>Blocked</h1></body></html>",
            ),
            "/hreflang" => response(
                200,
                "OK",
                "text/html",
                r#"
                    <html>
                      <head>
                        <title>Hreflang fixture page</title>
                        <link rel="alternate" hreflang="bad_locale_code" href="/hreflang-alt">
                      </head>
                      <body><h1>Hreflang Fixture</h1></body>
                    </html>
                "#,
            ),
            "/structured" => response(
                200,
                "OK",
                "text/html",
                r#"
                    <html>
                      <head>
                        <title>Structured data fixture page</title>
                        <script type="application/ld+json">{"@context": "https://schema.org",</script>
                      </head>
                      <body><h1>Structured Data Fixture</h1></body>
                    </html>
                "#,
            ),
            "/orphan" => response(
                200,
                "OK",
                "text/html",
                "<html><head><title>Sitemap Orphan</title></head><body><h1>Orphan</h1></body></html>",
            ),
            path if path.starts_with("/params") => response(
                200,
                "OK",
                "text/html",
                "<html><head><title>Parameterized Page</title></head><body><h1>Parameterized</h1></body></html>",
            ),
            "/nofollow-target" => response(
                200,
                "OK",
                "text/html",
                "<html><head><title>Nofollow Target</title></head><body><h1>Nofollow Target</h1></body></html>",
            ),
            "/assets/logo.png" => response(200, "OK", "image/png", "png"),
            "/assets/site.css" => response(200, "OK", "text/css", "body { color: #222; }"),
            "/assets/app.js" => response(
                200,
                "OK",
                "application/javascript",
                "console.log('ferrous frog');",
            ),
            _ => response(404, "Not Found", "text/html", "<h1>Not found</h1>"),
        }
    }

    fn duplicate_page(label: &str) -> String {
        response(
            200,
            "OK",
            "text/html",
            &format!(
                r#"
                    <html>
                      <head>
                        <title>Duplicate Product Title</title>
                        <meta name="description" content="Duplicate description for fixture pages.">
                      </head>
                      <body><h1>{label}</h1></body>
                    </html>
                "#
            ),
        )
    }

    fn response(status: u16, reason: &str, content_type: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }
}
