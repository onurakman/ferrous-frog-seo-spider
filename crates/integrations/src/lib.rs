use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
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
    /// Retained for compatibility; PSI navigation runs do not measure INP.
    pub inp_ms: Option<f64>,
    /// Lighthouse Total Blocking Time in milliseconds, a lab metric separate from INP.
    pub tbt_ms: Option<f64>,
    pub cls: Option<f64>,
    pub final_url: Option<String>,
    /// Lighthouse's fetchTime for the measured run, when supplied by PSI.
    pub fetched_at: Option<String>,
    pub lighthouse_version: Option<String>,
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
                .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
            if !response.status().is_success() {
                return Err(IntegrationError::RequestFailed(format!(
                    "Google Search Console returned HTTP {}",
                    response.status()
                )));
            }
            let payload = response
                .json::<SearchAnalyticsResponse>()
                .await
                .map_err(|error| IntegrationError::InvalidData(reqwest_error_message(error)))?;

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

const MAX_PAGE_SPEED_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

impl PageSpeedProvider {
    pub fn new(config: PageSpeedConfig) -> Result<Self, IntegrationError> {
        Self::with_request_timeout(config, Duration::from_secs(90))
    }

    fn with_request_timeout(
        config: PageSpeedConfig,
        request_timeout: Duration,
    ) -> Result<Self, IntegrationError> {
        let client = reqwest::Client::builder()
            .timeout(request_timeout)
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
        Ok(Self { client, config })
    }

    async fn fetch_url(
        &self,
        url: String,
        mut endpoint: url::Url,
    ) -> Result<UrlMetrics, IntegrationError> {
        // Check the original authority too: URL parsing removes empty userinfo and normalizes
        // backslashes/control characters, while the original URL is sent to preserve encoding.
        let plain_authority = url.split_once("://").is_some_and(|(_, rest)| {
            !rest
                .split(['/', '?', '#'])
                .next()
                .unwrap_or("")
                .contains('@')
        });
        let valid_url = plain_authority
            && url.trim() == url
            && !url
                .chars()
                .any(|character| character.is_control() || character == '\\')
            && url::Url::parse(&url).is_ok_and(|parsed| {
                matches!(parsed.scheme(), "http" | "https")
                    && parsed.has_host()
                    && parsed.username().is_empty()
                    && parsed.password().is_none()
            });
        if !valid_url {
            return Err(IntegrationError::InvalidData(
                "PageSpeed Insights requires an absolute HTTP or HTTPS URL without credentials"
                    .to_string(),
            ));
        }
        {
            let mut query = endpoint.query_pairs_mut();
            query.append_pair("url", &url);
            query.append_pair("strategy", self.config.strategy.as_api_value());
            for category in ["performance", "accessibility", "best-practices", "seo"] {
                query.append_pair("category", category);
            }
            if let Some(locale) = self.config.locale.as_deref()
                && !locale.trim().is_empty()
            {
                query.append_pair("locale", locale.trim());
            }
            if let Some(api_key) = self.config.api_key.as_deref()
                && !api_key.trim().is_empty()
            {
                query.append_pair("key", api_key.trim());
            }
        }

        let mut response = self
            .client
            .get(endpoint)
            .send()
            .await
            .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
        if !response.status().is_success() {
            return Err(IntegrationError::RequestFailed(format!(
                "PageSpeed Insights returned HTTP {}",
                response.status()
            )));
        }
        let too_large = || {
            IntegrationError::InvalidData(
                "PageSpeed Insights response exceeds the 16 MiB limit".to_string(),
            )
        };
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PAGE_SPEED_RESPONSE_BYTES as u64)
        {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| IntegrationError::InvalidData(reqwest_error_message(error)))?
        {
            // reqwest returns decoded chunks, so compressed responses share the same bound.
            if chunk.len() > MAX_PAGE_SPEED_RESPONSE_BYTES.saturating_sub(body.len()) {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        let payload = serde_json::from_slice::<PageSpeedResponse>(&body).map_err(|error| {
            // serde's raw error can echo response strings, including credentials or URLs.
            IntegrationError::InvalidData(format!(
                "PageSpeed Insights returned invalid Lighthouse JSON at line {}, column {}",
                error.line(),
                error.column()
            ))
        })?;
        let mut metrics = UrlMetrics::new(url, IntegrationSource::PageSpeedInsights);
        metrics.page_speed = Some(page_speed_response_to_metrics(payload)?);
        Ok(metrics)
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

            let endpoint =
                url::Url::parse("https://www.googleapis.com/pagespeedonline/v5/runPagespeed")
                    .map_err(|error| IntegrationError::InvalidData(error.to_string()))?;
            let mut rows = Vec::new();
            for url in request.urls {
                rows.push(self.fetch_url(url, endpoint.clone()).await?);
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
    runtime_error: Option<LighthouseRuntimeError>,
    final_url: Option<String>,
    fetch_time: Option<String>,
    lighthouse_version: Option<String>,
    categories: HashMap<String, LighthouseCategory>,
    audits: HashMap<String, LighthouseAudit>,
}

#[derive(Debug, Deserialize)]
struct LighthouseRuntimeError {
    code: String,
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

fn reqwest_error_message(error: reqwest::Error) -> String {
    if error.is_timeout() {
        return "request timed out".to_string();
    }
    // PSI puts the API key in the request URL; remove it before creating any display text.
    error.without_url().to_string()
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

fn page_speed_response_to_metrics(
    payload: PageSpeedResponse,
) -> Result<PageSpeedMetrics, IntegrationError> {
    let Some(lighthouse) = payload.lighthouse_result else {
        return Err(IntegrationError::InvalidData(
            "PageSpeed Insights response is missing Lighthouse results".to_string(),
        ));
    };
    if let Some(error) = &lighthouse.runtime_error
        && error.code != "NO_ERROR"
    {
        // Report documented codes only; arbitrary response messages can echo sensitive data.
        let code = match error.code.as_str() {
            "ERRORED_DOCUMENT_REQUEST"
            | "FAILED_DOCUMENT_REQUEST"
            | "INSECURE_DOCUMENT_REQUEST"
            | "INVALID_SPEEDLINE"
            | "NO_DCL"
            | "NO_DOCUMENT_REQUEST"
            | "NO_FCP"
            | "NO_NAVSTART"
            | "NO_SCREENSHOTS"
            | "NO_SPEEDLINE_FRAMES"
            | "NO_TRACING_STARTED"
            | "PARSING_PROBLEM"
            | "PROTOCOL_TIMEOUT"
            | "READ_FAILED"
            | "SPEEDINDEX_OF_ZERO"
            | "TRACING_ALREADY_STARTED" => error.code.as_str(),
            _ => "UNKNOWN_ERROR",
        };
        return Err(IntegrationError::InvalidData(format!(
            "PageSpeed Insights Lighthouse failed ({code})"
        )));
    }

    let metrics = PageSpeedMetrics {
        performance_score: lighthouse_category_score(&lighthouse, "performance"),
        accessibility_score: lighthouse_category_score(&lighthouse, "accessibility"),
        best_practices_score: lighthouse_category_score(&lighthouse, "best-practices"),
        seo_score: lighthouse_category_score(&lighthouse, "seo"),
        lcp_ms: lighthouse_audit_numeric_value(&lighthouse, "largest-contentful-paint"),
        inp_ms: None,
        tbt_ms: lighthouse_audit_numeric_value(&lighthouse, "total-blocking-time"),
        cls: lighthouse_audit_numeric_value(&lighthouse, "cumulative-layout-shift"),
        final_url: lighthouse.final_url,
        fetched_at: lighthouse.fetch_time,
        lighthouse_version: lighthouse.lighthouse_version,
    };
    let invalid_score = [
        metrics.performance_score,
        metrics.accessibility_score,
        metrics.best_practices_score,
        metrics.seo_score,
    ]
    .into_iter()
    .flatten()
    .any(|score| !score.is_finite() || !(0.0..=1.0).contains(&score));
    let invalid_lab_value = [metrics.lcp_ms, metrics.tbt_ms, metrics.cls]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite() || value < 0.0);
    if invalid_score || invalid_lab_value {
        return Err(IntegrationError::InvalidData(
            "PageSpeed Insights returned invalid lab metric values".to_string(),
        ));
    }
    Ok(metrics)
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
mod page_speed_tests;

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
    fn page_speed_response_maps_navigation_lab_metrics_separately_from_field_inp() {
        let payload = serde_json::from_str::<PageSpeedResponse>(
            r#"
            {
              "id": "https://example.test/",
              "loadingExperience": {
                "metrics": {
                  "INTERACTION_TO_NEXT_PAINT": { "percentile": 140, "category": "FAST" }
                }
              },
              "lighthouseResult": {
                "requestedUrl": "https://example.test/",
                "finalUrl": "https://example.test/",
                "runtimeError": null,
                "categories": {
                  "performance": { "score": 0.91 },
                  "accessibility": { "score": 0.88 },
                  "best-practices": { "score": 0.97 },
                  "seo": { "score": 1.0 }
                },
                "audits": {
                  "largest-contentful-paint": {
                    "numericValue": 1234.5, "numericUnit": "millisecond",
                    "score": 0.91, "scoreDisplayMode": "numeric"
                  },
                  "total-blocking-time": {
                    "numericValue": 87.0, "numericUnit": "millisecond",
                    "score": 0.99, "scoreDisplayMode": "numeric"
                  },
                  "cumulative-layout-shift": {
                    "numericValue": 0.02, "numericUnit": "unitless",
                    "score": 0.98, "scoreDisplayMode": "numeric"
                  }
                }
              }
            }
            "#,
        )
        .unwrap();

        let metrics = page_speed_response_to_metrics(payload).unwrap();
        assert_eq!(metrics.performance_score, Some(0.91));
        assert_eq!(metrics.accessibility_score, Some(0.88));
        assert_eq!(metrics.best_practices_score, Some(0.97));
        assert_eq!(metrics.seo_score, Some(1.0));
        assert_eq!(metrics.lcp_ms, Some(1234.5));
        assert_eq!(metrics.inp_ms, None);
        assert_eq!(metrics.cls, Some(0.02));
        assert_eq!(serde_json::to_value(metrics).unwrap()["tbtMs"], 87.0);
    }

    #[test]
    fn page_speed_response_preserves_zero_and_absent_lab_metrics() {
        let payload = serde_json::from_value::<PageSpeedResponse>(serde_json::json!({
            "lighthouseResult": {
                "categories": {
                    "performance": { "score": 0.0 },
                    "accessibility": { "score": null }
                },
                "audits": {
                    "largest-contentful-paint": { "numericValue": null },
                    "total-blocking-time": { "numericValue": 0.0 },
                    "cumulative-layout-shift": { "numericValue": 0.0 }
                }
            }
        }))
        .unwrap();

        let metrics = page_speed_response_to_metrics(payload).unwrap();
        assert_eq!(metrics.performance_score, Some(0.0));
        assert_eq!(metrics.accessibility_score, None);
        assert_eq!(metrics.best_practices_score, None);
        assert_eq!(metrics.seo_score, None);
        assert_eq!(metrics.lcp_ms, None);
        assert_eq!(metrics.inp_ms, None);
        assert_eq!(metrics.cls, Some(0.0));
        assert_eq!(serde_json::to_value(metrics).unwrap()["tbtMs"], 0.0);
    }

    #[test]
    fn page_speed_response_does_not_treat_legacy_lab_audits_as_inp() {
        for audit in [
            "interaction-to-next-paint",
            "experimental-interaction-to-next-paint",
        ] {
            let payload = serde_json::from_value::<PageSpeedResponse>(serde_json::json!({
                "lighthouseResult": {
                    "categories": {},
                    "audits": { audit: { "numericValue": 87.0 } }
                }
            }))
            .unwrap();

            let metrics = page_speed_response_to_metrics(payload).unwrap();
            assert_eq!(
                metrics.inp_ms, None,
                "navigation audit {audit} is not field INP"
            );
            assert_eq!(
                serde_json::to_value(metrics).unwrap()["tbtMs"],
                serde_json::Value::Null
            );
        }
    }

    #[test]
    fn page_speed_response_rejects_missing_lighthouse_results() {
        for fixture in ["{}", r#"{"lighthouseResult":null}"#] {
            let payload = serde_json::from_str::<PageSpeedResponse>(fixture).unwrap();
            assert!(matches!(
                page_speed_response_to_metrics(payload),
                Err(IntegrationError::InvalidData(_))
            ));
        }
    }

    #[test]
    fn page_speed_response_rejects_runtime_errors_even_with_partial_metrics() {
        let payload = serde_json::from_value::<PageSpeedResponse>(serde_json::json!({
            "lighthouseResult": {
                "runtimeError": {
                    "code": "NO_FCP",
                    "message": "The page did not paint any content."
                },
                "categories": { "performance": { "score": 0.0 } },
                "audits": { "total-blocking-time": { "numericValue": 0.0 } }
            }
        }))
        .unwrap();

        let error = page_speed_response_to_metrics(payload).unwrap_err();
        assert!(matches!(error, IntegrationError::InvalidData(_)));
        assert!(error.to_string().contains("NO_FCP"));
    }

    #[test]
    fn page_speed_metrics_accept_existing_payloads_without_new_fields() {
        let metrics = serde_json::from_value::<PageSpeedMetrics>(serde_json::json!({
            "performanceScore": 0.91,
            "inpMs": 87.0
        }))
        .unwrap();

        assert_eq!(metrics.performance_score, Some(0.91));
        assert_eq!(metrics.inp_ms, Some(87.0));
        assert_eq!(metrics.tbt_ms, None);
        assert_eq!(metrics.final_url, None);
        assert_eq!(metrics.fetched_at, None);
        assert_eq!(metrics.lighthouse_version, None);
    }

    #[test]
    fn reqwest_errors_strip_api_key_urls_before_formatting() {
        let endpoint = url::Url::parse(
            "https://www.googleapis.com/pagespeedonline/v5/runPagespeed?key=PSI_SENTINEL_SECRET&url=https%3A%2F%2Fexample.test%2F",
        )
        .unwrap();
        // Building an invalid request creates a real reqwest error without sending traffic.
        let error = reqwest::Client::new()
            .get("not a URL")
            .build()
            .unwrap_err()
            .with_url(endpoint);
        assert!(error.to_string().contains("PSI_SENTINEL_SECRET"));

        let message = reqwest_error_message(error);
        for error in [
            IntegrationError::RequestFailed(message.clone()),
            IntegrationError::InvalidData(message),
        ] {
            let formatted = format!("{error}\n{error:?}");
            assert!(!formatted.contains("PSI_SENTINEL_SECRET"));
            assert!(!formatted.contains("googleapis.com"));
            assert!(!formatted.contains("example.test"));
        }
    }
}
