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
    ChromeUxReport,
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

/// Real-user (field) Core Web Vitals from the Chrome UX Report, distinct from Lighthouse lab runs.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldVitalsMetrics {
    /// False when CrUX has no record for the URL and form factor.
    pub has_data: bool,
    pub lcp_ms_p75: Option<f64>,
    pub cls_p75: Option<f64>,
    pub inp_ms_p75: Option<f64>,
    pub fcp_ms_p75: Option<f64>,
    pub ttfb_ms_p75: Option<f64>,
    /// Collection period as ISO dates (`YYYY-MM-DD`).
    pub collection_period_start: Option<String>,
    pub collection_period_end: Option<String>,
    pub normalized_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FieldFormFactor {
    Phone,
    Desktop,
    Tablet,
}

impl FieldFormFactor {
    fn as_api_value(&self) -> &'static str {
        match self {
            Self::Phone => "PHONE",
            Self::Desktop => "DESKTOP",
            Self::Tablet => "TABLET",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FieldVitalsConfig {
    pub api_key: String,
    pub form_factor: FieldFormFactor,
}

pub struct FieldVitalsProvider {
    client: reqwest::Client,
    config: FieldVitalsConfig,
}

const MAX_FIELD_VITALS_RESPONSE_BYTES: usize = 1024 * 1024;

impl FieldVitalsProvider {
    pub fn new(config: FieldVitalsConfig) -> Result<Self, IntegrationError> {
        if config.api_key.trim().is_empty() {
            return Err(IntegrationError::NotConfigured(
                "Chrome UX Report needs a Google API key with the API enabled".to_string(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
        Ok(Self { client, config })
    }

    pub async fn fetch(&self, url: &str) -> Result<FieldVitalsMetrics, IntegrationError> {
        let endpoint =
            url::Url::parse("https://chromeuxreport.googleapis.com/v1/records:queryRecord")
                .map_err(|error| IntegrationError::InvalidData(error.to_string()))?;
        self.fetch_url(url, endpoint).await
    }

    async fn fetch_url(
        &self,
        url: &str,
        mut endpoint: url::Url,
    ) -> Result<FieldVitalsMetrics, IntegrationError> {
        if !url::Url::parse(url).is_ok_and(|parsed| {
            matches!(parsed.scheme(), "http" | "https")
                && parsed.has_host()
                && parsed.username().is_empty()
                && parsed.password().is_none()
        }) {
            return Err(IntegrationError::InvalidData(
                "Chrome UX Report requires an absolute HTTP or HTTPS URL without credentials"
                    .to_string(),
            ));
        }
        endpoint
            .query_pairs_mut()
            .append_pair("key", self.config.api_key.trim());
        let body = serde_json::json!({
            "url": url,
            "formFactor": self.config.form_factor.as_api_value(),
        });
        let response = self
            .client
            .post(endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_FIELD_VITALS_RESPONSE_BYTES as u64)
        {
            return Err(IntegrationError::InvalidData(
                "Chrome UX Report response exceeds the 1 MiB limit".to_string(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| IntegrationError::InvalidData(reqwest_error_message(error)))?;
        if bytes.len() > MAX_FIELD_VITALS_RESPONSE_BYTES {
            return Err(IntegrationError::InvalidData(
                "Chrome UX Report response exceeds the 1 MiB limit".to_string(),
            ));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(FieldVitalsMetrics::default());
        }
        if !status.is_success() {
            return Err(IntegrationError::RequestFailed(format!(
                "Chrome UX Report returned HTTP {status}"
            )));
        }
        let payload: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            IntegrationError::InvalidData(format!(
                "Chrome UX Report returned invalid JSON at line {}, column {}",
                error.line(),
                error.column()
            ))
        })?;
        Ok(field_vitals_from_payload(&payload))
    }
}

fn field_vitals_from_payload(payload: &serde_json::Value) -> FieldVitalsMetrics {
    let record = &payload["record"];
    let p75 = |metric: &str| -> Option<f64> {
        let value = &record["metrics"][metric]["percentiles"]["p75"];
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
            .filter(|value| value.is_finite() && *value >= 0.0)
    };
    let date = |key: &str| -> Option<String> {
        let value = &record["collectionPeriod"][key];
        Some(format!(
            "{:04}-{:02}-{:02}",
            value["year"].as_u64()?,
            value["month"].as_u64()?,
            value["day"].as_u64()?
        ))
    };
    let metrics = FieldVitalsMetrics {
        has_data: record.get("metrics").is_some_and(|m| m.is_object()),
        lcp_ms_p75: p75("largest_contentful_paint"),
        cls_p75: p75("cumulative_layout_shift"),
        inp_ms_p75: p75("interaction_to_next_paint"),
        fcp_ms_p75: p75("first_contentful_paint"),
        ttfb_ms_p75: p75("experimental_time_to_first_byte"),
        collection_period_start: date("firstDate"),
        collection_period_end: date("lastDate"),
        normalized_url: record["key"]["url"].as_str().map(str::to_string),
    };
    FieldVitalsMetrics {
        has_data: metrics.has_data
            && [
                metrics.lcp_ms_p75,
                metrics.cls_p75,
                metrics.inp_ms_p75,
                metrics.fcp_ms_p75,
                metrics.ttfb_ms_p75,
            ]
            .iter()
            .any(Option::is_some),
        ..metrics
    }
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
pub struct AnalyticsConfig {
    /// Numeric GA4 property ID (the part after `properties/`).
    pub property_id: String,
    pub access_token: String,
}

/// Google Analytics 4 Data API `runReport` per host + page path.
#[derive(Clone)]
pub struct GoogleAnalyticsProvider {
    client: reqwest::Client,
    config: AnalyticsConfig,
    endpoint: Option<url::Url>,
}

impl GoogleAnalyticsProvider {
    pub fn new(config: AnalyticsConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            endpoint: None,
        }
    }

    #[cfg(test)]
    fn with_endpoint(mut self, endpoint: url::Url) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    fn endpoint(&self) -> Result<url::Url, IntegrationError> {
        if let Some(endpoint) = &self.endpoint {
            return Ok(endpoint.clone());
        }
        let property = self
            .config
            .property_id
            .trim()
            .trim_start_matches("properties/");
        if property.is_empty() || !property.chars().all(|c| c.is_ascii_digit()) {
            return Err(IntegrationError::NotConfigured(
                "Google Analytics 4 needs a numeric property ID".to_string(),
            ));
        }
        url::Url::parse(&format!(
            "https://analyticsdata.googleapis.com/v1beta/properties/{property}:runReport"
        ))
        .map_err(|error| IntegrationError::InvalidData(error.to_string()))
    }
}

impl UrlMetricProvider for GoogleAnalyticsProvider {
    fn source(&self) -> IntegrationSource {
        IntegrationSource::GoogleAnalytics4
    }

    fn fetch_metrics<'a>(&'a self, request: MetricRequest) -> ProviderFuture<'a, MetricResponse> {
        Box::pin(async move {
            let date_range = request.date_range.ok_or_else(|| {
                IntegrationError::InvalidData(
                    "Google Analytics 4 requests require a date range".to_string(),
                )
            })?;
            if self.config.access_token.trim().is_empty() {
                return Err(IntegrationError::NotConfigured(
                    "Google Analytics 4 access token is empty".to_string(),
                ));
            }
            let endpoint = self.endpoint()?;
            let body = serde_json::json!({
                "dateRanges": [{ "startDate": date_range.start_date, "endDate": date_range.end_date }],
                "dimensions": [{ "name": "hostName" }, { "name": "pagePath" }],
                "metrics": [{ "name": "sessions" }, { "name": "engagedSessions" }, { "name": "keyEvents" }, { "name": "totalRevenue" }],
                "limit": "100000",
                "returnPropertyQuota": false,
            });
            let response = self
                .client
                .post(endpoint)
                .bearer_auth(self.config.access_token.trim())
                .json(&body)
                .send()
                .await
                .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
            if !response.status().is_success() {
                return Err(IntegrationError::RequestFailed(format!(
                    "Google Analytics 4 returned HTTP {}",
                    response.status()
                )));
            }
            let payload = response
                .json::<serde_json::Value>()
                .await
                .map_err(|error| IntegrationError::InvalidData(reqwest_error_message(error)))?;
            Ok(MetricResponse {
                source: IntegrationSource::GoogleAnalytics4,
                rows: analytics_rows_to_metrics(&payload),
            })
        })
    }
}

fn analytics_rows_to_metrics(payload: &serde_json::Value) -> Vec<UrlMetrics> {
    let number = |value: &serde_json::Value| {
        value
            .get("value")
            .and_then(|v| v.as_str())
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
    };
    payload["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let dimensions = row["dimensionValues"].as_array()?;
            let host = dimensions.first()?.get("value")?.as_str()?.trim();
            let path = dimensions.get(1)?.get("value")?.as_str()?.trim();
            if host.is_empty() || path.is_empty() {
                return None;
            }
            let metrics = row["metricValues"].as_array()?;
            let mut url_metrics = UrlMetrics::new(
                format!(
                    "https://{host}{}",
                    if path.starts_with('/') {
                        path.to_string()
                    } else {
                        format!("/{path}")
                    }
                ),
                IntegrationSource::GoogleAnalytics4,
            );
            url_metrics.analytics = Some(AnalyticsMetrics {
                sessions: metrics.first().map(number).unwrap_or(0.0),
                engaged_sessions: metrics.get(1).map(number).unwrap_or(0.0),
                conversions: metrics.get(2).map(number).unwrap_or(0.0),
                revenue: metrics.get(3).map(number).unwrap_or(0.0),
            });
            Some(url_metrics)
        })
        .collect()
}

/// A user-supplied backlink API: a URL template with `{url}` plus an optional header credential.
/// The endpoint must return JSON with `backlinks`, `referringDomains` and optional `authorityScore`
/// (snake_case accepted), which keeps commercial providers behind a thin, documented contract.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BacklinkEndpointConfig {
    pub endpoint_template: String,
    pub header_name: Option<String>,
    pub header_value: Option<String>,
}

pub struct BacklinkEndpointProvider {
    client: reqwest::Client,
    config: BacklinkEndpointConfig,
}

impl BacklinkEndpointProvider {
    pub fn new(config: BacklinkEndpointConfig) -> Result<Self, IntegrationError> {
        let template = config.endpoint_template.trim();
        if !template.contains("{url}")
            || !url::Url::parse(&template.replace("{url}", "https://example.test/"))
                .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.has_host())
        {
            return Err(IntegrationError::NotConfigured(
                "The backlink endpoint must be an HTTP(S) URL template containing {url}"
                    .to_string(),
            ));
        }
        if let Some(name) = config.header_name.as_deref().map(str::trim)
            && !name.is_empty()
            && reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err()
        {
            return Err(IntegrationError::NotConfigured(format!(
                "Invalid backlink credential header name: {name}"
            )));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
        Ok(Self { client, config })
    }

    fn request_url(&self, url: &str) -> String {
        let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
        self.config
            .endpoint_template
            .trim()
            .replace("{url}", &encoded)
    }

    pub async fn fetch_url(&self, url: &str) -> Result<BacklinkMetrics, IntegrationError> {
        let mut request = self.client.get(self.request_url(url));
        if let (Some(name), Some(value)) = (
            self.config.header_name.as_deref().map(str::trim),
            self.config.header_value.as_deref().map(str::trim),
        ) && !name.is_empty()
            && !value.is_empty()
        {
            request = request.header(name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
        let status = response.status();
        if !status.is_success() {
            return Err(IntegrationError::RequestFailed(format!(
                "Backlink endpoint returned HTTP {status}"
            )));
        }
        let payload = response
            .json::<serde_json::Value>()
            .await
            .map_err(|error| IntegrationError::InvalidData(reqwest_error_message(error)))?;
        backlink_metrics_from_payload(&payload).ok_or_else(|| {
            IntegrationError::InvalidData(
                "Backlink endpoint response needs numeric backlinks and referringDomains fields"
                    .to_string(),
            )
        })
    }
}

fn backlink_metrics_from_payload(payload: &serde_json::Value) -> Option<BacklinkMetrics> {
    let field = |camel: &str, snake: &str| {
        let value = payload.get(camel).or_else(|| payload.get(snake))?;
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
    };
    let backlinks = field("backlinks", "backlinks")?;
    let referring_domains = field("referringDomains", "referring_domains")?;
    if !backlinks.is_finite()
        || !referring_domains.is_finite()
        || backlinks < 0.0
        || referring_domains < 0.0
    {
        return None;
    }
    Some(BacklinkMetrics {
        referring_domains: referring_domains as u64,
        backlinks: backlinks as u64,
        authority_score: field("authorityScore", "authority_score")
            .filter(|score| score.is_finite()),
    })
}

/// Tokens returned by Google's OAuth token endpoint.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in_secs: u64,
    pub scope: String,
}

pub const GOOGLE_AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

pub fn base64url_no_pad(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let buffer = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let bits = u32::from(buffer[0]) << 16 | u32::from(buffer[1]) << 8 | u32::from(buffer[2]);
        let count = chunk.len() + 1;
        for index in 0..count {
            let shift = 18 - 6 * index;
            output.push(TABLE[((bits >> shift) & 0x3f) as usize] as char);
        }
    }
    output
}

/// PKCE S256 challenge for a code verifier (RFC 7636).
pub fn pkce_challenge(verifier: &str) -> String {
    use sha2::Digest as _;
    base64url_no_pad(&sha2::Sha256::digest(verifier.as_bytes()))
}

pub fn google_authorization_url(
    client_id: &str,
    redirect_uri: &str,
    scopes: &[&str],
    state: &str,
    code_verifier: &str,
) -> Result<url::Url, IntegrationError> {
    let mut url = url::Url::parse(GOOGLE_AUTH_ENDPOINT)
        .map_err(|error| IntegrationError::InvalidData(error.to_string()))?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id.trim())
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scopes.join(" "))
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("state", state)
        .append_pair("code_challenge", &pkce_challenge(code_verifier))
        .append_pair("code_challenge_method", "S256");
    Ok(url)
}

async fn google_token_request(
    token_endpoint: &str,
    form: &[(&str, &str)],
) -> Result<OAuthTokens, IntegrationError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let response = client
        .post(token_endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|error| IntegrationError::RequestFailed(reqwest_error_message(error)))?;
    let status = response.status();
    let payload = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| IntegrationError::InvalidData(reqwest_error_message(error)))?;
    if !status.is_success() {
        let detail = payload["error_description"]
            .as_str()
            .or_else(|| payload["error"].as_str())
            .unwrap_or("no details");
        return Err(IntegrationError::RequestFailed(format!(
            "Google token endpoint returned HTTP {status}: {detail}"
        )));
    }
    let access_token = payload["access_token"]
        .as_str()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            IntegrationError::InvalidData("Google token response has no access token".to_string())
        })?;
    Ok(OAuthTokens {
        access_token: access_token.to_string(),
        refresh_token: payload["refresh_token"].as_str().map(str::to_string),
        expires_in_secs: payload["expires_in"].as_u64().unwrap_or(3600),
        scope: payload["scope"].as_str().unwrap_or_default().to_string(),
    })
}

pub async fn exchange_google_code(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<OAuthTokens, IntegrationError> {
    google_token_request(
        token_endpoint,
        &[
            ("client_id", client_id.trim()),
            ("client_secret", client_secret.trim()),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
            ("code_verifier", code_verifier),
        ],
    )
    .await
}

pub async fn refresh_google_tokens(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<OAuthTokens, IntegrationError> {
    google_token_request(
        token_endpoint,
        &[
            ("client_id", client_id.trim()),
            ("client_secret", client_secret.trim()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ],
    )
    .await
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PageSpeedCategory {
    Performance,
    Accessibility,
    BestPractices,
    Seo,
}

impl PageSpeedCategory {
    pub const ALL: [PageSpeedCategory; 4] = [
        PageSpeedCategory::Performance,
        PageSpeedCategory::Accessibility,
        PageSpeedCategory::BestPractices,
        PageSpeedCategory::Seo,
    ];

    fn as_api_value(&self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Accessibility => "accessibility",
            Self::BestPractices => "best-practices",
            Self::Seo => "seo",
        }
    }
}

fn all_page_speed_categories() -> Vec<PageSpeedCategory> {
    PageSpeedCategory::ALL.to_vec()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageSpeedConfig {
    pub api_key: Option<String>,
    pub strategy: PageSpeedStrategy,
    pub locale: Option<String>,
    /// Lighthouse categories to request; an empty list means all four.
    #[serde(default = "all_page_speed_categories")]
    pub categories: Vec<PageSpeedCategory>,
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
            let mut categories: Vec<PageSpeedCategory> = if self.config.categories.is_empty() {
                PageSpeedCategory::ALL.to_vec()
            } else {
                self.config.categories.clone()
            };
            categories.dedup();
            for category in PageSpeedCategory::ALL {
                if categories.contains(&category) {
                    query.append_pair("category", category.as_api_value());
                }
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

pub mod llm;

#[cfg(test)]
mod field_vitals_tests;
#[cfg(test)]
mod google_tests;
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
