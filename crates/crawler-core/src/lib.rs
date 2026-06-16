use anyhow::{Context, Result};
use ferrous_frog_parser::{parse_html, same_host};
use ferrous_frog_storage::{
    CrawlRecord, CrawlSummary, MemoryStore, RedirectHop, UrlClassification, summarize,
};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, LOCATION};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use texting_robots::{Robot, get_robots_url};
use tokio::task::JoinSet;
use tokio::time::sleep;
use url::Url;

const DEFAULT_USER_AGENT: &str = "FerrousFrogSeoSpider/0.1 (+https://example.invalid/ferrous-frog)";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlConfig {
    pub start_url: String,
    pub max_urls: usize,
    pub max_depth: usize,
    pub concurrency: usize,
    pub request_delay_ms: u64,
    pub respect_robots: bool,
    pub user_agent: String,
    pub timeout_secs: u64,
    pub max_redirects: usize,
}

impl Default for CrawlConfig {
    fn default() -> Self {
        Self {
            start_url: "https://example.com/".to_string(),
            max_urls: 250,
            max_depth: 3,
            concurrency: 4,
            request_delay_ms: 250,
            respect_robots: true,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            timeout_secs: 20,
            max_redirects: 10,
        }
    }
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

#[derive(Clone)]
struct QueueItem {
    url: Url,
    depth: usize,
}

struct FetchOutput {
    record: CrawlRecord,
    links: Vec<String>,
}

pub async fn crawl<F>(
    config: CrawlConfig,
    store: MemoryStore,
    control: CrawlControl,
    on_event: F,
) -> Result<CrawlProgress>
where
    F: Fn(CrawlerEvent) + Send + Sync + 'static,
{
    let on_event = Arc::new(on_event);
    let config = normalize_config(config);
    let root_url = Url::parse(&config.start_url).context("invalid start URL")?;
    let client = Client::builder()
        .redirect(Policy::none())
        .user_agent(config.user_agent.clone())
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .context("failed to build HTTP client")?;
    let robots = fetch_robots(&client, &root_url, &config).await;
    let started_at = Instant::now();
    let mut queue = VecDeque::from([QueueItem {
        url: root_url.clone(),
        depth: 0,
    }]);
    let mut seen = HashSet::from([root_url.to_string()]);
    let mut active = JoinSet::new();
    let mut crawled = 0usize;

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
                let record = blocked_record(&item.url, item.depth, &root_url);
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
            active.spawn(async move { fetch_one(task_client, task_config, task_root, item).await });
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
                store.add_inlink(link);
            }

            if next_depth <= config.max_depth {
                for link in &output.links {
                    if seen.len() >= config.max_urls {
                        break;
                    }
                    let Ok(link_url) = Url::parse(link) else {
                        continue;
                    };
                    if !same_host(&link_url, &root_url) {
                        continue;
                    }
                    let normalized = link_url.to_string();
                    if seen.insert(normalized.clone()) {
                        queue.push_back(QueueItem {
                            url: link_url,
                            depth: next_depth,
                        });
                    }
                }
            }

            let record = store.upsert(output.record);
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
    if config.user_agent.trim().is_empty() {
        config.user_agent = DEFAULT_USER_AGENT.to_string();
    }
    config
}

async fn wait_if_paused(control: &CrawlControl) {
    while control.is_paused() && !control.is_cancelled() {
        sleep(Duration::from_millis(100)).await;
    }
}

async fn fetch_robots(client: &Client, root_url: &Url, config: &CrawlConfig) -> Option<Robot> {
    if !config.respect_robots {
        return None;
    }

    let robots_url = get_robots_url(root_url.as_str()).ok()?;
    let response = client.get(robots_url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    Robot::new(&config.user_agent, &bytes).ok()
}

fn robots_allowed(robot: Option<&Robot>, url: &Url, respect_robots: bool) -> bool {
    if !respect_robots {
        return true;
    }

    robot
        .map(|robot| robot.allowed(url.as_str()))
        .unwrap_or(true)
}

async fn fetch_one(
    client: Client,
    config: CrawlConfig,
    root_url: Url,
    item: QueueItem,
) -> Result<FetchOutput> {
    if config.request_delay_ms > 0 {
        sleep(Duration::from_millis(config.request_delay_ms)).await;
    }

    let original_url = item.url.clone();
    let started_at = Instant::now();
    let mut current_url = item.url;
    let mut redirect_chain = Vec::new();

    for _ in 0..=config.max_redirects {
        let response = match client.get(current_url.clone()).send().await {
            Ok(response) => response,
            Err(error) => {
                return Ok(FetchOutput {
                    record: error_record(
                        &original_url,
                        &current_url,
                        item.depth,
                        &root_url,
                        started_at,
                        error.to_string(),
                        redirect_chain,
                    ),
                    links: Vec::new(),
                });
            }
        };

        let status = response.status();
        let headers = response.headers().clone();

        if status.is_redirection() {
            let Some(location) = redirect_location(&headers) else {
                return Ok(FetchOutput {
                    record: status_record(
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
                    ),
                    links: Vec::new(),
                });
            };

            let next_url = match current_url.join(&location) {
                Ok(url) => url,
                Err(error) => {
                    return Ok(FetchOutput {
                        record: error_record(
                            &original_url,
                            &current_url,
                            item.depth,
                            &root_url,
                            started_at,
                            format!("Invalid redirect target: {error}"),
                            redirect_chain,
                        ),
                        links: Vec::new(),
                    });
                }
            };

            redirect_chain.push(RedirectHop {
                url: current_url.to_string(),
                status_code: status.as_u16(),
                location: Some(next_url.to_string()),
            });
            current_url = next_url;
            continue;
        }

        let headers_for_record = headers.clone();
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(FetchOutput {
                    record: error_record(
                        &original_url,
                        &current_url,
                        item.depth,
                        &root_url,
                        started_at,
                        error.to_string(),
                        redirect_chain,
                    ),
                    links: Vec::new(),
                });
            }
        };

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
        let mut links = Vec::new();

        if is_html {
            let html = String::from_utf8_lossy(&bytes);
            let signals = parse_html(&current_url, &html);
            record.title = signals.title;
            record.title_len = signals.title_len;
            record.meta_description = signals.meta_description;
            record.meta_description_len = signals.meta_description_len;
            record.h1 = signals.h1;
            record.h1_len = signals.h1_len;
            record.canonical = signals.canonical;
            record.indexability = signals.indexability;
            record.indexability_status = signals.indexability_status;

            for link in signals.links {
                if same_host(&Url::parse(&link.url)?, &root_url) {
                    record.internal_outlink_count += 1;
                    links.push(link.url);
                } else {
                    record.external_outlink_count += 1;
                }
            }
            record.outlink_count = record.internal_outlink_count + record.external_outlink_count;
        }

        return Ok(FetchOutput { record, links });
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
    })
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

    CrawlRecord {
        id: 0,
        url: original_url.to_string(),
        final_url: final_url.to_string(),
        classification: if same_host(final_url, root_url) {
            UrlClassification::Internal
        } else {
            UrlClassification::External
        },
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
        size_bytes,
        depth,
        redirect_target,
        redirect_type,
        redirect_chain,
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
        status_code: None,
        status_text: "No response".to_string(),
        content_type: None,
        indexability: "Non-indexable".to_string(),
        indexability_status: "No response".to_string(),
        response_time_ms,
        size_bytes: 0,
        depth,
        redirect_target: redirect_chain.last().and_then(|hop| hop.location.clone()),
        redirect_type: redirect_chain.last().map(|hop| hop.status_code.to_string()),
        redirect_chain,
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
        status_code: None,
        status_text: "Blocked by robots.txt".to_string(),
        content_type: None,
        indexability: "Non-indexable".to_string(),
        indexability_status: "Blocked by robots.txt".to_string(),
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
    store: &MemoryStore,
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
