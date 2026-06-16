use anyhow::{Context, Result};
use ferrous_frog_extractors::{CustomExtractor, run_extractors};
use ferrous_frog_parser::{PageResourceType, parse_html, same_host};
use ferrous_frog_storage::{
    CrawlRecord, CrawlStore, CrawlSummary, CustomExtractionValue, LinkEdge, LinkType, RedirectHop,
    UrlClassification, summarize,
};
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use quick_xml::Reader;
use quick_xml::events::Event;
use regex::Regex;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderName, LOCATION};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use texting_robots::{Robot, get_robots_url};
use tokio::net::lookup_host;
use tokio::task::JoinSet;
use tokio::time::sleep;
use url::Url;

const DEFAULT_USER_AGENT: &str = "FerrousFrogSeoSpider/0.1 (+https://example.invalid/ferrous-frog)";
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
    pub near_duplicate_threshold: u32,
    #[serde(default)]
    pub include_url_patterns: Vec<String>,
    #[serde(default)]
    pub exclude_url_patterns: Vec<String>,
    #[serde(default)]
    pub resource_types: CrawlResourceTypes,
    #[serde(default)]
    pub query_settings: QuerySettings,
    #[serde(default)]
    pub custom_extractors: Vec<CustomExtractor>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CrawlMode {
    #[default]
    Spider,
    List,
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
            max_urls: 250,
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
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
    let robot = Robot::new(user_agent, bytes).context("invalid robots.txt content")?;
    Ok(RobotsTxtTestResult {
        allowed: robot.allowed(url.as_str()),
        crawl_delay_ms: parse_robots_crawl_delay(bytes),
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
}

struct FetchOutput {
    record: CrawlRecord,
    links: Vec<DiscoveredUrl>,
    edges: Vec<LinkEdge>,
    from_sitemap: bool,
}

struct RobotsPolicy {
    robot: Robot,
    crawl_delay_ms: Option<u64>,
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
    let mut config = normalize_config(config);
    let scope_rules = compile_scope_rules(&config)?;
    let query_rules = compile_query_rules(&config)?;
    let root_url = root_url_from_config(&config, &query_rules)?;
    let client = Client::builder()
        .redirect(Policy::none())
        .user_agent(config.user_agent.clone())
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .context("failed to build HTTP client")?;
    let robots = fetch_robots(&client, &root_url, &config).await;
    if let Some(crawl_delay_ms) = robots.as_ref().and_then(|policy| policy.crawl_delay_ms) {
        config.request_delay_ms = config.request_delay_ms.max(crawl_delay_ms);
    }
    let sitemap_urls =
        if config.mode == CrawlMode::Spider && should_fetch_default_sitemap(&root_url) {
            fetch_sitemap_urls(&client, &root_url).await
        } else {
            Vec::new()
        };
    let rate_limiter = host_rate_limiter(config.requests_per_second);
    let started_at = Instant::now();
    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();
    for item in seed_queue_items(&config, &root_url, &query_rules)? {
        let normalized = item.url.to_string();
        if seen.insert(normalized) {
            queue.push_back(item);
        }
    }
    for mut sitemap_url in sitemap_urls {
        if seen.len() >= config.max_urls {
            break;
        }
        normalize_url_query(&mut sitemap_url, &config.query_settings, &query_rules);
        if same_host(&sitemap_url, &root_url) && scope_allows(&sitemap_url, &scope_rules) {
            let normalized = sitemap_url.to_string();
            if seen.insert(normalized) {
                queue.push_back(QueueItem {
                    url: sitemap_url,
                    depth: 0,
                    from_sitemap: true,
                });
            }
        }
    }
    let mut active = JoinSet::new();
    let mut crawled = 0usize;
    let mut content_fingerprints = Vec::new();

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
            if !robots_allowed(robots.as_ref(), &item.url, config.respect_robots) {
                let mut record = blocked_record(&item.url, item.depth, &root_url);
                record.in_sitemap = item.from_sitemap;
                let record = store.upsert(record);
                crawled += 1;
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
                continue;
            }

            let task_client = client.clone();
            let task_config = config.clone();
            let task_root = root_url.clone();
            let task_rate_limiter = rate_limiter.clone();
            let task_query_rules = query_rules.clone();
            active.spawn(async move {
                fetch_one(
                    task_client,
                    task_config,
                    task_root,
                    task_rate_limiter,
                    task_query_rules,
                    item,
                )
                .await
            });
        }

        if active.is_empty() {
            break;
        }

        if let Some(joined) = active.join_next().await {
            let output = match joined {
                Ok(output) => output,
                Err(error) => {
                    on_event(CrawlerEvent::error(format!("crawl worker failed: {error}")));
                    continue;
                }
            };

            let output = match output {
                Ok(output) => output,
                Err(error) => {
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
                    if seen.len() >= config.max_urls {
                        break;
                    }
                    let Ok(mut link_url) = Url::parse(&link.url) else {
                        continue;
                    };
                    normalize_url_query(&mut link_url, &config.query_settings, &query_rules);
                    if !scope_allows(&link_url, &scope_rules)
                        || !should_crawl_discovered(
                            &link_url,
                            &root_url,
                            link.resource_type,
                            &config.resource_types,
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
                        });
                    }
                }
            }

            let mut record = output.record;
            record.in_sitemap = output.from_sitemap;
            assign_near_duplicate_cluster(
                &mut record,
                &mut content_fingerprints,
                config.near_duplicate_threshold,
            );
            let record = store.upsert(record);
            crawled += 1;
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
            .unwrap_or(config.start_url.trim())
    } else {
        config.start_url.trim()
    };
    let mut root_url = Url::parse(root_seed).context("invalid start URL")?;
    normalize_url_query(&mut root_url, &config.query_settings, query_rules);
    Ok(root_url)
}

fn seed_queue_items(
    config: &CrawlConfig,
    root_url: &Url,
    query_rules: &QueryRules,
) -> Result<Vec<QueueItem>> {
    if config.mode == CrawlMode::Spider {
        return Ok(vec![QueueItem {
            url: root_url.clone(),
            depth: 0,
            from_sitemap: false,
        }]);
    }

    let seeds = if config.list_urls.is_empty() {
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
    for seed in seeds {
        let mut url = Url::parse(seed).with_context(|| format!("invalid list URL: {seed}"))?;
        normalize_url_query(&mut url, &config.query_settings, query_rules);
        items.push(QueueItem {
            url,
            depth: 0,
            from_sitemap: false,
        });
    }

    if items.is_empty() {
        anyhow::bail!("list mode requires at least one URL");
    }
    Ok(items)
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

fn scope_allows(url: &Url, rules: &ScopeRules) -> bool {
    let value = url.as_str();
    if !rules.include.is_empty() && !rules.include.iter().any(|pattern| pattern.is_match(value)) {
        return false;
    }
    !rules.exclude.iter().any(|pattern| pattern.is_match(value))
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
) -> bool {
    let is_internal = same_host(url, root_url);
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

async fn fetch_robots(
    client: &Client,
    root_url: &Url,
    config: &CrawlConfig,
) -> Option<RobotsPolicy> {
    if !config.respect_robots {
        return None;
    }

    if config.use_robots_txt_override && !config.robots_txt_override.trim().is_empty() {
        let bytes = config.robots_txt_override.as_bytes();
        let crawl_delay_ms = parse_robots_crawl_delay(bytes);
        let robot = Robot::new(&config.user_agent, bytes).ok()?;
        return Some(RobotsPolicy {
            robot,
            crawl_delay_ms,
        });
    }

    let robots_url = get_robots_url(root_url.as_str()).ok()?;
    let response = client.get(robots_url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    let crawl_delay_ms = parse_robots_crawl_delay(&bytes);
    let robot = Robot::new(&config.user_agent, &bytes).ok()?;
    Some(RobotsPolicy {
        robot,
        crawl_delay_ms,
    })
}

fn parse_robots_crawl_delay(bytes: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(bytes).ok()?;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("crawl-delay") {
            let seconds = value.trim().parse::<f64>().ok()?;
            if seconds.is_finite() && seconds >= 0.0 {
                return Some((seconds * 1_000.0).round() as u64);
            }
        }
    }
    None
}

async fn fetch_sitemap_urls(client: &Client, root_url: &Url) -> Vec<Url> {
    let Ok(sitemap_url) = root_url.join("/sitemap.xml") else {
        return Vec::new();
    };
    let Ok(response) = client.get(sitemap_url).send().await else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(bytes) = response.bytes().await else {
        return Vec::new();
    };
    let Ok(xml) = std::str::from_utf8(&bytes) else {
        return Vec::new();
    };
    parse_sitemap_urls(xml, root_url)
}

fn should_fetch_default_sitemap(root_url: &Url) -> bool {
    root_url.path() == "/" && root_url.query().is_none()
}

fn parse_sitemap_urls(xml: &str, root_url: &Url) -> Vec<Url> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut urls = Vec::new();
    let mut in_loc = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if element.name().as_ref() == b"loc" => {
                in_loc = true;
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"loc" => {
                in_loc = false;
            }
            Ok(Event::Text(text)) if in_loc => {
                if let Ok(value) = text.decode() {
                    if let Ok(url) = root_url.join(value.trim()) {
                        urls.push(url);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    urls
}

fn robots_allowed(policy: Option<&RobotsPolicy>, url: &Url, respect_robots: bool) -> bool {
    if !respect_robots {
        return true;
    }

    policy
        .map(|policy| policy.robot.allowed(url.as_str()))
        .unwrap_or(true)
}

async fn fetch_one(
    client: Client,
    config: CrawlConfig,
    root_url: Url,
    rate_limiter: Option<HostRateLimiter>,
    query_rules: QueryRules,
    item: QueueItem,
) -> Result<FetchOutput> {
    let original_url = item.url.clone();
    let started_at = Instant::now();
    let mut current_url = item.url;
    let mut redirect_chain = Vec::new();

    for _ in 0..=config.max_redirects {
        wait_for_politeness(&current_url, config.request_delay_ms, rate_limiter.as_ref()).await;

        let mut network_timings = NetworkTimings::default();
        let (dns_lookup_time_ms, resolved_ip_count) = measure_dns_lookup(&current_url).await;
        network_timings.dns_lookup_time_ms = dns_lookup_time_ms;
        network_timings.resolved_ip_count = resolved_ip_count;

        let request_started_at = Instant::now();
        let response = match client.get(current_url.clone()).send().await {
            Ok(response) => response,
            Err(error) => {
                network_timings.total_network_time_ms = Some(elapsed_ms(request_started_at));
                return Ok(FetchOutput {
                    record: with_network_timings(
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
                    links: Vec::new(),
                    edges: Vec::new(),
                    from_sitemap: item.from_sitemap,
                });
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
                return Ok(FetchOutput {
                    record,
                    links: Vec::new(),
                    edges: Vec::new(),
                    from_sitemap: item.from_sitemap,
                });
            };

            let next_url = match current_url.join(&location) {
                Ok(url) => url,
                Err(error) => {
                    return Ok(FetchOutput {
                        record: with_network_timings(
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
                        links: Vec::new(),
                        edges: Vec::new(),
                        from_sitemap: item.from_sitemap,
                    });
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
                ttfb_ms: network_timings.ttfb_ms,
                elapsed_ms: network_timings.ttfb_ms,
            });
            if redirect_loop_detected {
                return Ok(FetchOutput {
                    record: with_network_timings(
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
                    links: Vec::new(),
                    edges: Vec::new(),
                    from_sitemap: item.from_sitemap,
                });
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
                return Ok(FetchOutput {
                    record: with_network_timings(
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
                    links: Vec::new(),
                    edges: Vec::new(),
                    from_sitemap: item.from_sitemap,
                });
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

        if is_html {
            let html = String::from_utf8_lossy(&bytes);
            let signals = parse_html(&current_url, &html);
            let visible_text = signals.visible_text;
            record.title = signals.title;
            record.title_len = signals.title_len;
            record.meta_description = signals.meta_description;
            record.meta_description_len = signals.meta_description_len;
            record.meta_robots = signals.meta_robots;
            record.h1 = signals.h1;
            record.h1_len = signals.h1_len;
            record.h1_count = signals.h1_count;
            record.h2 = signals.h2;
            record.h2_len = signals.h2_len;
            record.h2_count = signals.h2_count;
            record.canonical = signals.canonical;
            record.canonical_count = signals.canonical_count;
            record.indexability = signals.indexability;
            record.indexability_status = signals.indexability_status;
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
            record.json_ld_count = signals.json_ld_count;
            record.json_ld_invalid_count = signals.json_ld_invalid_count;
            record.open_graph_count = signals.open_graph_count;
            record.twitter_card_count = signals.twitter_card_count;
            apply_directives_and_canonical(&mut record, &current_url);
            if !visible_text.is_empty() {
                record.simhash = Some(simhash::simhash(&visible_text));
            }
            record.custom_extractions = extract_custom_values(&html, &config.custom_extractors);

            for link in signals.links {
                let mut target_url = Url::parse(&link.url)?;
                normalize_url_query(&mut target_url, &config.query_settings, &query_rules);
                let target_url_string = target_url.to_string();
                let resource_type = classify_anchor_resource(&target_url);
                let link_type = if same_host(&target_url, &root_url) {
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
                });
                edges.push(LinkEdge {
                    id: 0,
                    source_url: current_url.to_string(),
                    target_url: target_url_string,
                    anchor_text: link.text,
                    rel: link.rel,
                    rel_nofollow: link.rel_nofollow,
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
                let link_type = if same_host(&target_url, &root_url) {
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
                });
                edges.push(LinkEdge {
                    id: 0,
                    source_url: current_url.to_string(),
                    target_url: target_url_string,
                    anchor_text: resource.label,
                    rel: String::new(),
                    rel_nofollow: false,
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

        return Ok(FetchOutput {
            record,
            links,
            edges,
            from_sitemap: item.from_sitemap,
        });
    }

    Ok(FetchOutput {
        record: error_record(
            &original_url,
            &current_url,
            item.depth,
            &root_url,
            started_at,
            "Redirect limit exceeded".to_string(),
            redirect_chain,
        ),
        links: Vec::new(),
        edges: Vec::new(),
        from_sitemap: item.from_sitemap,
    })
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

async fn measure_dns_lookup(url: &Url) -> (Option<u64>, u32) {
    let Some(host) = url.host_str() else {
        return (None, 0);
    };
    let Some(port) = url.port_or_known_default() else {
        return (None, 0);
    };

    let started_at = Instant::now();
    match lookup_host((host, port)).await {
        Ok(addresses) => (
            Some(elapsed_ms(started_at)),
            addresses.count().min(u32::MAX as usize) as u32,
        ),
        Err(_) => (None, 0),
    }
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

async fn wait_for_politeness(
    url: &Url,
    request_delay_ms: u64,
    rate_limiter: Option<&HostRateLimiter>,
) {
    if let Some(rate_limiter) = rate_limiter {
        let host = url.host_str().unwrap_or_default().to_string();
        rate_limiter.until_key_ready(&host).await;
    }

    if request_delay_ms > 0 {
        sleep(Duration::from_millis(request_delay_ms)).await;
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

fn apply_directives_and_canonical(record: &mut CrawlRecord, current_url: &Url) {
    if contains_directive(record.x_robots_tag.as_deref(), "noindex") {
        record.indexability = "Non-indexable".to_string();
        record.indexability_status = "X-Robots-Tag noindex".to_string();
        return;
    }

    if contains_directive(record.meta_robots.as_deref(), "noindex") {
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

fn contains_directive(value: Option<&str>, directive: &str) -> bool {
    value
        .unwrap_or_default()
        .to_ascii_lowercase()
        .split([',', ';'])
        .flat_map(str::split_whitespace)
        .map(|part| part.trim_matches(':'))
        .any(|part| part == directive)
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
    let x_robots_tag = header_string(&headers, HeaderName::from_static("x-robots-tag"));
    let hsts_header = headers.contains_key(HeaderName::from_static("strict-transport-security"));
    let content_security_policy_header =
        headers.contains_key(HeaderName::from_static("content-security-policy"));
    let x_frame_options_header = headers.contains_key(HeaderName::from_static("x-frame-options"));
    let x_content_type_options_header =
        headers.contains_key(HeaderName::from_static("x-content-type-options"));

    CrawlRecord {
        id: 0,
        url: original_url.to_string(),
        final_url: final_url.to_string(),
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
        meta_description: None,
        meta_description_len: 0,
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
        url: original_url.to_string(),
        final_url: final_url.to_string(),
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
        error: Some(error),
    }
}

fn blocked_record(url: &Url, depth: usize, root_url: &Url) -> CrawlRecord {
    CrawlRecord {
        id: 0,
        url: url.to_string(),
        final_url: url.to_string(),
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
    use ferrous_frog_storage::{IssueView, MemoryStore};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn parses_sitemap_locations() {
        let root_url = Url::parse("https://example.com/start").unwrap();
        let urls = parse_sitemap_urls(
            r#"
                <urlset>
                  <url><loc>/one</loc></url>
                  <url><loc>https://example.com/two</loc></url>
                </urlset>
            "#,
            &root_url,
        );

        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].as_str(), "https://example.com/one");
        assert_eq!(urls[1].as_str(), "https://example.com/two");
    }

    #[test]
    fn parses_robots_crawl_delay() {
        let delay = parse_robots_crawl_delay(b"User-agent: *\nCrawl-delay: 0.02\n");
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

    #[tokio::test]
    async fn crawls_mock_site_with_redirects_broken_links_and_duplicate_titles() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
        };

        let result = crawl(config, store.clone(), CrawlControl::default(), |_| {}).await;
        server.abort();
        result.unwrap();

        let records = store.records();
        assert!(records.iter().any(|record| record.final_url == base_url));
        assert!(records.iter().any(|record| {
            record.final_url == format!("{base_url}missing") && record.status_code == Some(404)
        }));
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
    async fn flags_redirect_loops_before_redirect_limit() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: format!("{base_url}loop-a"),
            list_urls: Vec::new(),
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: vec!["/missing$".to_string()],
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
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
    async fn uses_custom_robots_txt_override() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::Spider,
            start_url: base_url.clone(),
            list_urls: Vec::new(),
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            resource_types: CrawlResourceTypes {
                images: true,
                css: true,
                javascript: true,
                ..CrawlResourceTypes::default()
            },
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings {
                sort_parameters: true,
                strip_all: false,
                max_parameters: 2,
                strip_parameter_patterns: vec!["^utm_".to_string()],
            },
            custom_extractors: Vec::new(),
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
    async fn list_mode_crawls_only_supplied_urls() {
        let (base_url, server) = spawn_mock_site().await;
        let store = MemoryStore::new();

        let config = CrawlConfig {
            mode: CrawlMode::List,
            start_url: base_url.clone(),
            list_urls: vec![format!("{base_url}a"), format!("{base_url}missing")],
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
            near_duplicate_threshold: 6,
            include_url_patterns: Vec::new(),
            exclude_url_patterns: Vec::new(),
            resource_types: CrawlResourceTypes::default(),
            query_settings: QuerySettings::default(),
            custom_extractors: Vec::new(),
        };

        crawl(config, store.clone(), CrawlControl::default(), |_| {})
            .await
            .unwrap();
        server.abort();

        let records = store.records();
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .any(|record| record.final_url == format!("{base_url}a"))
        );
        assert!(
            records
                .iter()
                .any(|record| record.final_url == format!("{base_url}missing")
                    && record.status_code == Some(404))
        );
        assert!(!records.iter().any(|record| record.final_url == base_url));
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
