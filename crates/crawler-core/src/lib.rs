use anyhow::{Context, Result};
use ferrous_frog_extractors::{CustomExtractor, CustomSearch, run_extractors, run_searches};
use ferrous_frog_parser::{
    PageResourceType, PageSignals, contains_robots_directive, parse_html, same_host,
};
use ferrous_frog_storage::{
    CrawlFrontierItem, CrawlFrontierState, CrawlRecord, CrawlStore, CrawlSummary,
    CustomExtractionValue, CustomSearchSource, CustomSearchValue, HreflangLink, ImageAsset,
    LinkEdge, LinkType, RedirectHop, StructuredDataIssue, UrlClassification, summarize,
};
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use regex::Regex;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderName, LOCATION};
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
use std::time::{Duration, Instant};
use texting_robots::{Robot, get_robots_url};
use tokio::net::{TcpStream, lookup_host};
use tokio::sync::{Mutex, OnceCell};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tokio_rustls::TlsConnector;
use url::Url;

mod rendering;

pub use rendering::{JsRenderingBackend, JsRenderingConfig};

const DEFAULT_USER_AGENT: &str = "FerrousFrogSeoSpider/0.1 (+https://example.invalid/ferrous-frog)";
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
    pub user_agent: String,
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
    #[serde(default = "default_true")]
    pub follow_nofollow: bool,
    #[serde(default)]
    pub resource_types: CrawlResourceTypes,
    #[serde(default)]
    pub query_settings: QuerySettings,
    #[serde(default)]
    pub custom_extractors: Vec<CustomExtractor>,
    #[serde(default)]
    pub custom_searches: Vec<CustomSearch>,
    #[serde(default)]
    pub rendering: JsRenderingConfig,
    #[serde(default)]
    pub resume_from_state: bool,
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
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FolderScope {
    #[default]
    Anywhere,
    StartFolder,
    ExactFolder,
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
            max_urls: 5_000,
            max_depth: 3,
            concurrency: 4,
            requests_per_second: 2,
            request_delay_ms: 250,
            respect_robots: true,
            use_robots_txt_override: false,
            robots_txt_override: String::new(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            timeout_secs: 20,
            max_redirects: 10,
            retry_attempts: default_retry_attempts(),
            retry_backoff_ms: default_retry_backoff_ms(),
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
        }
    }
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
    let robots_url = get_robots_url(root_url.as_str()).context("failed to build robots URL")?;
    let user_agent = if request.user_agent.trim().is_empty() {
        DEFAULT_USER_AGENT
    } else {
        request.user_agent.trim()
    };
    let timeout_secs = if request.timeout_secs == 0 {
        20
    } else {
        request.timeout_secs
    };
    let client = Client::builder()
        .redirect(Policy::limited(5))
        .user_agent(user_agent)
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("failed to build HTTP client")?;
    let response = client
        .get(robots_url.clone())
        .send()
        .await
        .context("failed to download robots.txt")?;
    let status_code = response.status().as_u16();
    let robots_txt = response
        .text()
        .await
        .context("failed to read robots.txt body")?;
    Ok(RobotsTxtDownloadResult {
        robots_url,
        status_code,
        robots_txt,
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
}

struct RequestPolicy {
    origins: Mutex<HashMap<String, Arc<OriginPolicy>>>,
    rate_limiter: Option<HostRateLimiter>,
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
    let on_event = Arc::new(on_event);
    let config = normalize_config(config);
    let scope_rules = compile_scope_rules(&config)?;
    let query_rules = compile_query_rules(&config)?;
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
    });
    let sitemap_urls =
        if config.mode == CrawlMode::Spider && should_fetch_default_sitemap(&root_url) {
            fetch_default_sitemap_urls(&client, &root_url, &config, &request_policy, &control).await
        } else {
            Vec::new()
        };
    let list_sitemap_seed_urls = if config.mode == CrawlMode::List {
        fetch_list_sitemap_seed_urls(&client, &config, &root_url, &request_policy, &control).await?
    } else {
        Vec::new()
    };
    let started_at = Instant::now();
    let (mut queue, mut seen, mut crawled) = if config.resume_from_state {
        match restore_frontier_state(&store)? {
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

        if let Some(joined) = joined {
            let (item_key, output) = match joined {
                Ok(output) => output,
                Err(error) => {
                    on_event(CrawlerEvent::error(format!("crawl worker failed: {error}")));
                    continue;
                }
            };
            active_items.remove(&item_key);

            let output = match output {
                Ok(output) => output,
                Err(error) => {
                    save_frontier_state(&store, &queue, &active_items, &seen, crawled);
                    if control.is_cancelled() && error.to_string() == CRAWL_CANCELLED_MESSAGE {
                        continue;
                    }
                    on_event(CrawlerEvent::error(error.to_string()));
                    continue;
                }
            };

            let next_depth = output.record.depth + 1;
            for link in &output.links {
                if let Ok(mut link_url) = Url::parse(&link.url) {
                    normalize_url_query(&mut link_url, &config.query_settings, &query_rules);
                    store.add_inlink(link_url.as_str());
                }
            }
            for edge in &output.edges {
                store.add_link_edge(edge.clone());
            }

            if config.mode == CrawlMode::Spider && next_depth <= config.max_depth {
                for link in &output.links {
                    if link.rel_nofollow && !config.follow_nofollow {
                        continue;
                    }
                    if seen.len() >= config.max_urls {
                        break;
                    }
                    let Ok(mut link_url) = Url::parse(&link.url) else {
                        continue;
                    };
                    normalize_url_query(&mut link_url, &config.query_settings, &query_rules);
                    if !scope_allows(&link_url, &root_url, &scope_rules, &config)
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

            let mut record = output.record;
            record.in_sitemap = output.from_sitemap;
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
        if same_host(&sitemap_url, root_url)
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

fn restore_frontier_state<S: CrawlStore>(
    store: &S,
) -> Result<Option<(VecDeque<QueueItem>, HashSet<String>, usize)>> {
    let Some(state) = store.load_frontier_state() else {
        return Ok(None);
    };
    if state.queued.is_empty() && state.seen.is_empty() {
        return Ok(None);
    }

    let mut queue = VecDeque::new();
    for item in state.queued {
        queue.push_back(queue_item_from_frontier(item)?);
    }
    let seen = state.seen.into_iter().collect::<HashSet<_>>();
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

    let manual_seeds = if config.list_urls.is_empty() && list_sitemap_seed_urls.is_empty() {
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

fn scope_allows(url: &Url, root_url: &Url, rules: &ScopeRules, config: &CrawlConfig) -> bool {
    let value = url.as_str();
    if !rules.include.is_empty() && !rules.include.iter().any(|pattern| pattern.is_match(value)) {
        return false;
    }
    if rules.exclude.iter().any(|pattern| pattern.is_match(value)) {
        return false;
    }

    scope_modes_allow(url, root_url, config)
}

fn scope_modes_allow(url: &Url, root_url: &Url, config: &CrawlConfig) -> bool {
    scope_host_allows(url, root_url, config.subdomain_scope)
        && folder_scope_allows(url, root_url, config.folder_scope)
}

fn scope_host_allows(url: &Url, root_url: &Url, scope: SubdomainScope) -> bool {
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    let Some(root_host) = root_url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };

    match scope {
        SubdomainScope::ExactHost => host == root_host,
        SubdomainScope::IncludeSubdomains => {
            let base_host = root_host.strip_prefix("www.").unwrap_or(&root_host);
            host == root_host || host == base_host || host.ends_with(&format!(".{base_host}"))
        }
    }
}

fn folder_scope_allows(url: &Url, root_url: &Url, scope: FolderScope) -> bool {
    match scope {
        FolderScope::Anywhere => true,
        FolderScope::StartFolder => url.path().starts_with(&folder_path(root_url)),
        FolderScope::ExactFolder => folder_path(url) == folder_path(root_url),
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
        DiscoveredResourceType::Other => resource_types.other,
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
        _ => DiscoveredResourceType::Other,
    }
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
        request_policy
            .wait(&robots_url, config.request_delay_ms, control)
            .await?;
        let response = client
            .get(robots_url.clone())
            .send()
            .await
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
        let bytes = response
            .bytes()
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

async fn fetch_default_sitemap_urls(
    client: &Client,
    root_url: &Url,
    config: &CrawlConfig,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
) -> Vec<Url> {
    let Ok(sitemap_url) = root_url.join("/sitemap.xml") else {
        return Vec::new();
    };
    fetch_sitemap_locations(client, &sitemap_url, config, request_policy, control)
        .await
        .unwrap_or_default()
}

fn should_fetch_default_sitemap(root_url: &Url) -> bool {
    root_url.path() == "/" && root_url.query().is_none()
}

async fn fetch_list_sitemap_seed_urls(
    client: &Client,
    config: &CrawlConfig,
    root_url: &Url,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
) -> Result<Vec<Url>> {
    let mut urls = Vec::new();
    for seed in config
        .list_sitemap_urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
    {
        let sitemap_url = Url::parse(seed)
            .or_else(|_| root_url.join(seed))
            .with_context(|| format!("invalid list sitemap URL: {seed}"))?;
        urls.extend(
            fetch_sitemap_locations(client, &sitemap_url, config, request_policy, control).await?,
        );
    }
    Ok(urls)
}

async fn fetch_sitemap_locations(
    client: &Client,
    sitemap_url: &Url,
    config: &CrawlConfig,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
) -> Result<Vec<Url>> {
    const MAX_SITEMAP_DOCUMENTS: usize = 128;
    const MAX_SITEMAP_DEPTH: usize = 4;

    let mut pending = VecDeque::from([(sitemap_url.clone(), 0usize)]);
    let mut seen_sitemaps = HashSet::new();
    let mut urls = Vec::new();

    while let Some((current_url, depth)) = pending.pop_front() {
        if seen_sitemaps.len() >= MAX_SITEMAP_DOCUMENTS {
            break;
        }
        if !seen_sitemaps.insert(current_url.to_string()) {
            continue;
        }

        let delay = request_policy
            .robots_delay(client, config, &current_url, control)
            .await
            .with_context(|| format!("cannot crawl sitemap: {current_url}"))?;
        request_policy.wait(&current_url, delay, control).await?;
        let response = client
            .get(current_url.clone())
            .send()
            .await
            .with_context(|| format!("failed to fetch sitemap: {current_url}"))?;
        if !response.status().is_success() {
            anyhow::bail!(
                "sitemap returned status {}: {}",
                response.status().as_u16(),
                current_url
            );
        }
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("failed to read sitemap body: {current_url}"))?;
        let xml = std::str::from_utf8(&bytes)
            .with_context(|| format!("sitemap is not valid UTF-8: {current_url}"))?;
        let parsed = parse_sitemap_document(xml, &current_url);
        urls.extend(parsed.urls);

        if depth < MAX_SITEMAP_DEPTH {
            for child_sitemap_url in parsed.sitemaps {
                if !seen_sitemaps.contains(child_sitemap_url.as_str()) {
                    pending.push_back((child_sitemap_url, depth + 1));
                }
            }
        }
    }

    Ok(urls)
}

fn parse_sitemap_document(xml: &str, root_url: &Url) -> ParsedSitemap {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut parsed = ParsedSitemap::default();
    let mut current_entry = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if xml_name_matches(element.name().as_ref(), b"url") => {
                current_entry = Some(SitemapEntryKind::Url);
            }
            Ok(Event::Start(element)) if xml_name_matches(element.name().as_ref(), b"sitemap") => {
                current_entry = Some(SitemapEntryKind::Sitemap);
            }
            Ok(Event::Start(element)) if xml_name_matches(element.name().as_ref(), b"loc") => {
                if let Ok(text) = reader.read_text(element.name()) {
                    if let Ok(value) = text.decode() {
                        let value = unescape(value.trim())
                            .map(|value| value.into_owned())
                            .unwrap_or_else(|_| value.trim().to_string());
                        if let Ok(url) = root_url.join(&value) {
                            match current_entry {
                                Some(SitemapEntryKind::Sitemap) => parsed.sitemaps.push(url),
                                _ => parsed.urls.push(url),
                            }
                        }
                    }
                }
            }
            Ok(Event::End(element)) if xml_name_matches(element.name().as_ref(), b"url") => {
                current_entry = None;
            }
            Ok(Event::End(element)) if xml_name_matches(element.name().as_ref(), b"sitemap") => {
                current_entry = None;
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    parsed
}

fn xml_name_matches(name: &[u8], expected: &[u8]) -> bool {
    name == expected
        || name
            .rsplit(|byte| *byte == b':')
            .next()
            .map(|local_name| local_name == expected)
            .unwrap_or(false)
}

async fn fetch_one(
    client: Client,
    config: CrawlConfig,
    root_url: Url,
    request_policy: Arc<RequestPolicy>,
    query_rules: QueryRules,
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
        let mut request_attempt = 0;
        let (response, mut network_timings, request_started_at) = loop {
            wait_if_paused(&control).await;
            ensure_not_cancelled(&control)?;

            let mut network_timings = NetworkTimings::default();
            let (dns_lookup_time_ms, resolved_ip_count, tcp_address) =
                measure_dns_lookup(&current_url).await;
            network_timings.dns_lookup_time_ms = dns_lookup_time_ms;
            network_timings.resolved_ip_count = resolved_ip_count;
            ensure_not_cancelled(&control)?;
            let (tcp_connect_time_ms, tls_handshake_time_ms) =
                measure_connection_probe(&current_url, tcp_address, tcp_connect_timeout(&config))
                    .await;
            network_timings.tcp_connect_time_ms = tcp_connect_time_ms;
            network_timings.tls_handshake_time_ms = tls_handshake_time_ms;
            request_policy
                .wait(&current_url, request_delay_ms, &control)
                .await?;

            let request_started_at = Instant::now();
            match client.get(current_url.clone()).send().await {
                Ok(response) => {
                    network_timings.ttfb_ms = Some(elapsed_ms(request_started_at));
                    network_timings.total_network_time_ms = network_timings.ttfb_ms;
                    if retryable_status(response.status())
                        && request_attempt < config.retry_attempts
                    {
                        request_attempt = request_attempt.saturating_add(1);
                        sleep_retry_backoff(&config, request_attempt).await;
                        wait_if_paused(&control).await;
                        ensure_not_cancelled(&control)?;
                        continue;
                    }
                    break (response, network_timings, request_started_at);
                }
                Err(error) => {
                    network_timings.total_network_time_ms = Some(elapsed_ms(request_started_at));
                    if request_attempt < config.retry_attempts {
                        request_attempt = request_attempt.saturating_add(1);
                        sleep_retry_backoff(&config, request_attempt).await;
                        wait_if_paused(&control).await;
                        ensure_not_cancelled(&control)?;
                        continue;
                    }
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
                    Vec::new(),
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

        let headers_for_record = headers.clone();
        let download_started_at = Instant::now();
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                network_timings.download_time_ms = Some(elapsed_ms(download_started_at));
                network_timings.total_network_time_ms = Some(elapsed_ms(request_started_at));
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
            bytes.to_vec(),
            redirect_chain,
            None,
        );
        apply_network_timings(&mut record, network_timings);
        let mut links = Vec::new();
        let mut edges = Vec::new();
        let mut image_assets = Vec::new();

        if is_html {
            let raw_html = String::from_utf8_lossy(&bytes);
            let rendered_html = match rendering::render_page_if_enabled(
                &config.rendering,
                &current_url,
                config.timeout_secs,
            )
            .await
            {
                Ok(rendered) => rendered.map(|page| page.html),
                Err(error) => {
                    record.error = Some(format!("JavaScript rendering failed: {error}"));
                    None
                }
            };
            let raw_signals = rendered_html
                .as_ref()
                .map(|_| parse_html(&current_url, &raw_html));
            let html = rendered_html.as_deref().unwrap_or(&raw_html);
            let signals = parse_html(&current_url, html);
            if let Some(raw_signals) = raw_signals.as_ref() {
                apply_rendered_dom_diff(&mut record, raw_signals, &signals);
            }
            let page_images = signals.images;
            let visible_text = signals.visible_text;
            record.title = signals.title;
            record.title_len = signals.title_len;
            record.title_pixel_width = signals.title_pixel_width;
            record.meta_description = signals.meta_description;
            record.meta_description_len = signals.meta_description_len;
            record.meta_description_pixel_width = signals.meta_description_pixel_width;
            record.meta_robots = signals.meta_robots;
            record.h1 = signals.h1;
            record.h1_len = signals.h1_len;
            record.h1_count = signals.h1_count;
            record.h2 = signals.h2;
            record.h2_len = signals.h2_len;
            record.h2_count = signals.h2_count;
            record.canonical = signals.canonical;
            record.canonical_count = signals.canonical_count;
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

            for link in signals.links {
                let rel_nofollow = link.rel_nofollow || page_nofollow;
                let mut target_url = Url::parse(&link.url)?;
                normalize_url_query(&mut target_url, &config.query_settings, &query_rules);
                let target_url_string = target_url.to_string();
                let resource_type = classify_anchor_resource(&target_url);
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
                    rel_nofollow,
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
    links: Vec<DiscoveredUrl>,
    edges: Vec<LinkEdge>,
    images: Vec<ImageAsset>,
    identity: &QueueIdentity,
) -> FetchOutput {
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

impl RequestPolicy {
    async fn origin(&self, url: &Url) -> Arc<OriginPolicy> {
        self.origins
            .lock()
            .await
            .entry(url.origin().ascii_serialization())
            .or_default()
            .clone()
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
        let origin = self.origin(url).await;
        let robots = tokio::select! {
            robots = origin.robots.get_or_init(|| async {
                fetch_robots(client, url, config, self, control)
                    .await
                    .map_err(|error| format!("{error:#}"))
            }) => robots,
            _ = wait_until_cancelled(control) => anyhow::bail!(CRAWL_CANCELLED_MESSAGE),
        };
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
            Some(Ok(Some(robot))) => delay_ms.max(robots_crawl_delay_ms(robot).unwrap_or(0)),
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
        wait_if_paused(control).await;
        ensure_not_cancelled(control)?;
        *last_request = Some(Instant::now());
        Ok(())
    }
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

async fn sleep_retry_backoff(config: &CrawlConfig, attempt: u32) {
    if config.retry_backoff_ms == 0 {
        return;
    }
    let exponent = attempt.saturating_sub(1).min(10);
    let multiplier = 1_u64 << exponent;
    let delay_ms = config
        .retry_backoff_ms
        .saturating_mul(multiplier)
        .min(30_000);
    if delay_ms > 0 {
        sleep(Duration::from_millis(delay_ms)).await;
    }
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

    if let Some(canonical) = record.canonical.as_deref() {
        if canonical != current_url.as_str() {
            record.indexability = "Non-indexable".to_string();
            record.indexability_status = "Canonicalized".to_string();
        }
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

fn status_record(
    original_url: &Url,
    final_url: &Url,
    depth: usize,
    root_url: &Url,
    started_at: Instant,
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
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
        Some(blake3::hash(&body).to_hex().to_string())
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
        title_len: 0,
        title_pixel_width: 0,
        meta_description: None,
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
        summary: summarize(&store.records()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{GridQuery, IssueView, MemoryStore};
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

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
        );

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
        );

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

    type RecordedRequests = Arc<std::sync::Mutex<Vec<(String, Instant)>>>;

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
                    let _ = stream.write_all(handler(path).as_bytes()).await;
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
