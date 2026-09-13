use ferrous_frog_storage::{
    CapturedHeader, MAX_CAPTURE_BYTES, MAX_CAPTURE_HEADER_BYTES, MAX_CAPTURE_HEADERS, PageCapture,
};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CaptureConfig {
    pub response_headers: bool,
    pub raw_html: bool,
    pub rendered_html: bool,
    pub visible_text: bool,
    pub max_bytes: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            response_headers: false,
            raw_html: false,
            rendered_html: false,
            visible_text: false,
            max_bytes: MAX_CAPTURE_BYTES,
        }
    }
}

pub(super) fn sensitive_header_name(name: &str) -> bool {
    [
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
    .any(|part| name.contains(part))
        || name == "key"
        || name.ends_with("-key")
}

pub(super) fn bounded_text(text: &str, limit: usize) -> (String, bool) {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), end < text.len())
}

pub(super) fn observed_response(
    config: &CaptureConfig,
    headers: &HeaderMap,
    key: &str,
    source_url: &Url,
    final_url: &Url,
) -> Option<PageCapture> {
    if !config.response_headers && !config.raw_html && !config.rendered_html && !config.visible_text
    {
        return None;
    }
    let mut capture = PageCapture {
        source_storage_key: key.to_owned(),
        source_url: source_url.to_string(),
        final_url: final_url.to_string(),
        response_headers: None,
        raw_html: None,
        rendered_html: None,
        visible_text: None,
        raw_html_truncated: false,
        rendered_html_truncated: false,
        visible_text_truncated: false,
        headers_truncated: false,
    };
    if config.response_headers {
        let mut retained = Vec::new();
        let mut bytes = 0;
        for (name, value) in headers {
            let name = name.as_str();
            let sensitive = sensitive_header_name(name);
            if retained.len() == MAX_CAPTURE_HEADERS
                || (!sensitive && value.as_bytes().len() > MAX_CAPTURE_HEADER_BYTES)
            {
                capture.headers_truncated = true;
                break;
            }
            let value = if sensitive {
                "[redacted]".to_owned()
            } else {
                std::str::from_utf8(value.as_bytes())
                    .map(str::to_owned)
                    .unwrap_or_else(|_| {
                        format!("[non-UTF-8 bytes: {}]", value.as_bytes().escape_ascii())
                    })
            };
            bytes += name.len() + value.len();
            if bytes > MAX_CAPTURE_HEADER_BYTES {
                capture.headers_truncated = true;
                break;
            }
            retained.push(CapturedHeader {
                name: name.to_owned(),
                value,
            });
        }
        capture.response_headers = Some(retained);
    }
    Some(capture)
}
