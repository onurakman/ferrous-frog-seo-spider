use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;

pub type ProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, IntegrationError>> + Send + 'a>>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IntegrationSource {
    GoogleSearchConsole,
    GoogleAnalytics4,
    PageSpeedInsights,
    BacklinkProvider,
    Custom(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UrlMetrics {
    pub url: String,
    pub source: IntegrationSource,
    pub search_console: Option<SearchConsoleMetrics>,
    pub analytics: Option<AnalyticsMetrics>,
    pub page_speed: Option<PageSpeedMetrics>,
    pub backlinks: Option<BacklinkMetrics>,
}

impl UrlMetrics {
    pub fn new(url: impl Into<String>, source: IntegrationSource) -> Self {
        Self {
            url: url.into(),
            source,
            search_console: None,
            analytics: None,
            page_speed: None,
            backlinks: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchConsoleMetrics {
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub average_position: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsMetrics {
    pub sessions: f64,
    pub engaged_sessions: f64,
    pub conversions: f64,
    pub revenue: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedMetrics {
    pub performance_score: Option<f64>,
    pub accessibility_score: Option<f64>,
    pub best_practices_score: Option<f64>,
    pub seo_score: Option<f64>,
    pub lcp_ms: Option<f64>,
    pub inp_ms: Option<f64>,
    pub cls: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BacklinkMetrics {
    pub referring_domains: u64,
    pub backlinks: u64,
    pub authority_score: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MetricRequest {
    pub urls: Vec<String>,
    pub date_range: Option<DateRange>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DateRange {
    pub start_date: String,
    pub end_date: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetricResponse {
    pub source: IntegrationSource,
    pub rows: Vec<UrlMetrics>,
}

pub trait UrlMetricProvider: Send + Sync {
    fn source(&self) -> IntegrationSource;
    fn fetch_metrics<'a>(&'a self, request: MetricRequest) -> ProviderFuture<'a, MetricResponse>;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchConsoleConfig {
    pub site_url: String,
    pub access_token: String,
    pub row_limit: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageSpeedStrategy {
    Mobile,
    Desktop,
}

impl PageSpeedStrategy {
    fn as_api_value(&self) -> &'static str {
        match self {
            Self::Mobile => "mobile",
            Self::Desktop => "desktop",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedConfig {
    pub api_key: Option<String>,
    pub strategy: PageSpeedStrategy,
    pub locale: Option<String>,
}

#[derive(Clone)]
pub struct SearchConsoleProvider {
    client: reqwest::Client,
    config: SearchConsoleConfig,
}

impl SearchConsoleProvider {
    pub fn new(config: SearchConsoleConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub fn endpoint(&self) -> String {
        search_console_endpoint(&self.config.site_url)
    }
}

impl UrlMetricProvider for SearchConsoleProvider {
    fn source(&self) -> IntegrationSource {
        IntegrationSource::GoogleSearchConsole
    }

    fn fetch_metrics<'a>(&'a self, request: MetricRequest) -> ProviderFuture<'a, MetricResponse> {
        Box::pin(async move {
            let date_range = request.date_range.ok_or_else(|| {
                IntegrationError::InvalidData(
                    "Google Search Console requests require a date range".to_string(),
                )
            })?;
            if self.config.access_token.trim().is_empty() {
                return Err(IntegrationError::NotConfigured(
                    "Google Search Console access token is empty".to_string(),
                ));
            }

            let row_limit = self.config.row_limit.clamp(1, 25_000);
            let body = SearchAnalyticsRequest {
                start_date: date_range.start_date,
                end_date: date_range.end_date,
                dimensions: vec!["page".to_string()],
                row_limit,
                start_row: 0,
            };
            let response = self
                .client
                .post(self.endpoint())
                .bearer_auth(self.config.access_token.trim())
                .json(&body)
                .send()
                .await
                .map_err(|error| IntegrationError::RequestFailed(error.to_string()))?;
            if !response.status().is_success() {
                return Err(IntegrationError::RequestFailed(format!(
                    "Google Search Console returned HTTP {}",
                    response.status()
                )));
            }
            let payload = response
                .json::<SearchAnalyticsResponse>()
                .await
                .map_err(|error| IntegrationError::InvalidData(error.to_string()))?;

            Ok(MetricResponse {
                source: IntegrationSource::GoogleSearchConsole,
                rows: search_analytics_rows_to_metrics(payload.rows.unwrap_or_default()),
            })
        })
    }
}

#[derive(Clone)]
pub struct PageSpeedProvider {
    client: reqwest::Client,
    config: PageSpeedConfig,
}

impl PageSpeedProvider {
    pub fn new(config: PageSpeedConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }
}

impl UrlMetricProvider for PageSpeedProvider {
    fn source(&self) -> IntegrationSource {
        IntegrationSource::PageSpeedInsights
    }

    fn fetch_metrics<'a>(&'a self, request: MetricRequest) -> ProviderFuture<'a, MetricResponse> {
        Box::pin(async move {
            if request.urls.is_empty() {
                return Err(IntegrationError::InvalidData(
                    "PageSpeed Insights requests require at least one URL".to_string(),
                ));
            }

            let mut rows = Vec::new();
            for url in request.urls {
                let mut query = vec![
                    ("url".to_string(), url.clone()),
                    (
                        "strategy".to_string(),
                        self.config.strategy.as_api_value().to_string(),
                    ),
                    ("category".to_string(), "performance".to_string()),
                    ("category".to_string(), "accessibility".to_string()),
                    ("category".to_string(), "best-practices".to_string()),
                    ("category".to_string(), "seo".to_string()),
                ];
                if let Some(locale) = self.config.locale.as_deref()
                    && !locale.trim().is_empty()
                {
                    query.push(("locale".to_string(), locale.trim().to_string()));
                }
                if let Some(api_key) = self.config.api_key.as_deref()
                    && !api_key.trim().is_empty()
                {
                    query.push(("key".to_string(), api_key.trim().to_string()));
                }
                let mut endpoint =
                    url::Url::parse("https://www.googleapis.com/pagespeedonline/v5/runPagespeed")
                        .map_err(|error| IntegrationError::InvalidData(error.to_string()))?;
                {
                    let mut pairs = endpoint.query_pairs_mut();
                    for (key, value) in &query {
                        pairs.append_pair(key, value);
                    }
                }

                let response = self
                    .client
                    .get(endpoint)
                    .send()
                    .await
                    .map_err(|error| IntegrationError::RequestFailed(error.to_string()))?;
                if !response.status().is_success() {
                    return Err(IntegrationError::RequestFailed(format!(
                        "PageSpeed Insights returned HTTP {} for {url}",
                        response.status()
                    )));
                }
                let payload = response
                    .json::<PageSpeedResponse>()
                    .await
                    .map_err(|error| IntegrationError::InvalidData(error.to_string()))?;
                let mut metrics = UrlMetrics::new(url, IntegrationSource::PageSpeedInsights);
                metrics.page_speed = Some(page_speed_response_to_metrics(payload));
                rows.push(metrics);
            }

            Ok(MetricResponse {
                source: IntegrationSource::PageSpeedInsights,
                rows,
            })
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchAnalyticsRequest {
    start_date: String,
    end_date: String,
    dimensions: Vec<String>,
    row_limit: u32,
    start_row: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchAnalyticsResponse {
    rows: Option<Vec<SearchAnalyticsRow>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchAnalyticsRow {
    keys: Vec<String>,
    clicks: f64,
    impressions: f64,
    ctr: f64,
    position: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageSpeedResponse {
    lighthouse_result: Option<LighthouseResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LighthouseResult {
    categories: HashMap<String, LighthouseCategory>,
    audits: HashMap<String, LighthouseAudit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LighthouseCategory {
    score: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LighthouseAudit {
    numeric_value: Option<f64>,
}

#[derive(Debug, Error)]
pub enum IntegrationError {
    #[error("provider is not configured: {0}")]
    NotConfigured(String),
    #[error("provider request failed: {0}")]
    RequestFailed(String),
    #[error("provider returned invalid data: {0}")]
    InvalidData(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MergedUrlMetrics {
    pub url: String,
    pub search_console: Option<SearchConsoleMetrics>,
    pub analytics: Option<AnalyticsMetrics>,
    pub page_speed: Option<PageSpeedMetrics>,
    pub backlinks: Option<BacklinkMetrics>,
}

pub fn merge_metric_responses(responses: &[MetricResponse]) -> Vec<MergedUrlMetrics> {
    let mut rows = HashMap::<String, MergedUrlMetrics>::new();
    for response in responses {
        for metrics in &response.rows {
            let key = canonical_metric_url(&metrics.url);
            let row = rows.entry(key.clone()).or_insert_with(|| MergedUrlMetrics {
                url: key,
                ..MergedUrlMetrics::default()
            });
            if let Some(search_console) = metrics.search_console.clone() {
                row.search_console = Some(search_console);
            }
            if let Some(analytics) = metrics.analytics.clone() {
                row.analytics = Some(analytics);
            }
            if let Some(page_speed) = metrics.page_speed.clone() {
                row.page_speed = Some(page_speed);
            }
            if let Some(backlinks) = metrics.backlinks.clone() {
                row.backlinks = Some(backlinks);
            }
        }
    }

    let mut merged = rows.into_values().collect::<Vec<_>>();
    merged.sort_by(|left, right| left.url.cmp(&right.url));
    merged
}

pub fn canonical_metric_url(value: &str) -> String {
    let trimmed = value.trim();
    if let Ok(mut parsed) = url::Url::parse(trimmed) {
        parsed.set_fragment(None);
        return parsed.to_string();
    }
    trimmed.to_string()
}

fn search_console_endpoint(site_url: &str) -> String {
    let encoded_site_url =
        url::form_urlencoded::byte_serialize(site_url.trim().as_bytes()).collect::<String>();
    format!(
        "https://www.googleapis.com/webmasters/v3/sites/{encoded_site_url}/searchAnalytics/query"
    )
}

fn search_analytics_rows_to_metrics(rows: Vec<SearchAnalyticsRow>) -> Vec<UrlMetrics> {
    rows.into_iter()
        .filter_map(|row| {
            let url = row.keys.first()?.clone();
            let mut metrics = UrlMetrics::new(url, IntegrationSource::GoogleSearchConsole);
            metrics.search_console = Some(SearchConsoleMetrics {
                clicks: row.clicks,
                impressions: row.impressions,
                ctr: row.ctr,
                average_position: row.position,
            });
            Some(metrics)
        })
        .collect()
}

fn page_speed_response_to_metrics(payload: PageSpeedResponse) -> PageSpeedMetrics {
    let Some(lighthouse) = payload.lighthouse_result else {
        return PageSpeedMetrics::default();
    };

    PageSpeedMetrics {
        performance_score: lighthouse_category_score(&lighthouse, "performance"),
        accessibility_score: lighthouse_category_score(&lighthouse, "accessibility"),
        best_practices_score: lighthouse_category_score(&lighthouse, "best-practices"),
        seo_score: lighthouse_category_score(&lighthouse, "seo"),
        lcp_ms: lighthouse_audit_numeric_value(&lighthouse, "largest-contentful-paint"),
        inp_ms: lighthouse_audit_numeric_value(&lighthouse, "interaction-to-next-paint").or_else(
            || {
                lighthouse_audit_numeric_value(
                    &lighthouse,
                    "experimental-interaction-to-next-paint",
                )
            },
        ),
        cls: lighthouse_audit_numeric_value(&lighthouse, "cumulative-layout-shift"),
    }
}

fn lighthouse_category_score(lighthouse: &LighthouseResult, key: &str) -> Option<f64> {
    lighthouse
        .categories
        .get(key)
        .and_then(|category| category.score)
}

fn lighthouse_audit_numeric_value(lighthouse: &LighthouseResult, key: &str) -> Option<f64> {
    lighthouse
        .audits
        .get(key)
        .and_then(|audit| audit.numeric_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_metric_url_removes_fragments() {
        assert_eq!(
            canonical_metric_url("https://example.com/page#section"),
            "https://example.com/page"
        );
    }

    #[test]
    fn merge_metric_responses_combines_sources_by_url() {
        let mut gsc = UrlMetrics::new(
            "https://example.com/page#main",
            IntegrationSource::GoogleSearchConsole,
        );
        gsc.search_console = Some(SearchConsoleMetrics {
            clicks: 10.0,
            impressions: 100.0,
            ctr: 0.1,
            average_position: 4.2,
        });

        let mut psi = UrlMetrics::new(
            "https://example.com/page",
            IntegrationSource::PageSpeedInsights,
        );
        psi.page_speed = Some(PageSpeedMetrics {
            performance_score: Some(0.91),
            ..PageSpeedMetrics::default()
        });

        let merged = merge_metric_responses(&[
            MetricResponse {
                source: IntegrationSource::GoogleSearchConsole,
                rows: vec![gsc],
            },
            MetricResponse {
                source: IntegrationSource::PageSpeedInsights,
                rows: vec![psi],
            },
        ]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].search_console.as_ref().unwrap().clicks, 10.0);
        assert_eq!(
            merged[0].page_speed.as_ref().unwrap().performance_score,
            Some(0.91)
        );
    }

    #[test]
    fn search_console_endpoint_encodes_site_url() {
        assert_eq!(
            search_console_endpoint("https://example.com/"),
            "https://www.googleapis.com/webmasters/v3/sites/https%3A%2F%2Fexample.com%2F/searchAnalytics/query"
        );
    }

    #[test]
    fn search_console_rows_map_to_metrics() {
        let rows = search_analytics_rows_to_metrics(vec![SearchAnalyticsRow {
            keys: vec!["https://example.com/page".to_string()],
            clicks: 12.0,
            impressions: 120.0,
            ctr: 0.1,
            position: 3.4,
        }]);

        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].search_console.as_ref().unwrap().average_position,
            3.4
        );
    }

    #[test]
    fn page_speed_response_maps_lighthouse_scores_and_core_web_vitals() {
        let payload = serde_json::from_str::<PageSpeedResponse>(
            r#"
            {
              "lighthouseResult": {
                "categories": {
                  "performance": { "score": 0.91 },
                  "accessibility": { "score": 0.88 },
                  "best-practices": { "score": 0.97 },
                  "seo": { "score": 1.0 }
                },
                "audits": {
                  "largest-contentful-paint": { "numericValue": 1234.5 },
                  "interaction-to-next-paint": { "numericValue": 87.0 },
                  "cumulative-layout-shift": { "numericValue": 0.02 }
                }
              }
            }
            "#,
        )
        .unwrap();

        let metrics = page_speed_response_to_metrics(payload);
        assert_eq!(metrics.performance_score, Some(0.91));
        assert_eq!(metrics.accessibility_score, Some(0.88));
        assert_eq!(metrics.best_practices_score, Some(0.97));
        assert_eq!(metrics.seo_score, Some(1.0));
        assert_eq!(metrics.lcp_ms, Some(1234.5));
        assert_eq!(metrics.inp_ms, Some(87.0));
        assert_eq!(metrics.cls, Some(0.02));
    }
}
