use super::*;
use crate::tests::{response, spawn_recording_site};
use ferrous_frog_storage::{ActiveStore, PageCaptureQuery, SqliteStore};

#[test]
fn retention_is_opt_in_utf8_bounded_and_redacts_credential_headers() {
    let config: CrawlConfig =
        serde_json::from_value(serde_json::to_value(CrawlConfig::default()).unwrap()).unwrap();
    let url = Url::parse("https://example.test/").unwrap();
    assert!(
        capture::observed_response(&config.capture, &HeaderMap::new(), "key", &url, &url).is_none()
    );
    assert_eq!(capture::bounded_text("aé🦀z", 5), ("aé".into(), true));
    assert_eq!(capture::bounded_text("", 1024), (String::new(), false));
    let mut headers = HeaderMap::new();
    for name in [
        "set-cookie",
        "authorization",
        "x-api-key",
        "x-session-id",
        "x-csrf-token",
    ] {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_static("private-value"),
        );
    }
    headers.append("x-trace", HeaderValue::from_static("first"));
    headers.append("x-trace", HeaderValue::from_static("second"));
    headers.insert("x-opaque", HeaderValue::from_bytes(&[255]).unwrap());
    let enabled = CaptureConfig {
        response_headers: true,
        ..Default::default()
    };
    let retained = capture::observed_response(&enabled, &headers, "key", &url, &url).unwrap();
    let headers = retained.response_headers.unwrap();
    assert_eq!(
        headers
            .iter()
            .filter(|header| header.name == "x-trace")
            .count(),
        2
    );
    assert!(
        headers
            .iter()
            .filter(|header| capture::sensitive_header_name(&header.name))
            .all(|header| header.value == "[redacted]")
    );
    assert!(
        headers
            .iter()
            .any(|header| header.name == "x-opaque" && header.value.contains("\\xff"))
    );
    let mut oversized = HeaderMap::new();
    for _ in 0..513 {
        oversized.append("x-empty", HeaderValue::from_static(""));
    }
    let retained = capture::observed_response(&enabled, &oversized, "key", &url, &url).unwrap();
    assert!(retained.headers_truncated);
    assert_eq!(retained.response_headers.unwrap().len(), 512);
    oversized.clear();
    oversized.insert(
        "x-large",
        HeaderValue::from_str(&"v".repeat(65_537)).unwrap(),
    );
    assert!(
        capture::observed_response(&enabled, &oversized, "key", &url, &url)
            .unwrap()
            .headers_truncated
    );
    for max_bytes in [0, 1023, 1_048_577] {
        let config = CrawlConfig {
            capture: CaptureConfig {
                max_bytes,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(validate_configuration(&config).is_err());
    }
}

#[tokio::test]
async fn retained_sources_preserve_occurrences_empty_and_incomplete_evidence_without_changing_parsing()
 {
    let html = format!(
        "<title>Retained page</title><main>Visible {}<a href='/tail'>Tail link</a></main><script>Hidden script</script>",
        "é".repeat(700)
    );
    let body = html.clone();
    let (base, _, server) = spawn_recording_site(move |path| {
        let result = match path {
            "/robots.txt" => response(200, "OK", "text/plain", "User-agent: *\nAllow: /\n"),
            "/page" => response(200, "OK", "text/html", &body),
            "/empty" => response(200, "OK", "text/html", ""),
            "/asset" => response(200, "OK", "application/octet-stream", "binary content"),
            "/large" => response(200, "OK", "text/html", &"x".repeat(8192)),
            "/redirect" => "HTTP/1.1 302 Found\r\nLocation: /page\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
            _ => response(404, "Not Found", "text/html", "<main>Missing page</main>"),
        };
        result.replacen("\r\n\r\n", "\r\nX-Trace: first\r\nX-Trace: second\r\nSet-Cookie: credential=private\r\n\r\n", 1)
    }).await;
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let config = CrawlConfig {
            start_url: format!("{base}page"),
            mode: CrawlMode::List,
            list_urls: [
                "page", "page", "empty", "asset", "large", "missing", "redirect",
            ]
            .map(|path| format!("{base}{path}"))
            .to_vec(),
            max_urls: 7,
            concurrency: 2,
            requests_per_second: 0,
            request_delay_ms: 0,
            max_response_bytes: 4096,
            sitemap: SitemapConfig {
                enabled: false,
                ..Default::default()
            },
            capture: CaptureConfig {
                response_headers: true,
                raw_html: true,
                rendered_html: true,
                visible_text: true,
                max_bytes: 1024,
            },
            ..Default::default()
        };
        crawl(
            config.clone(),
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        let records = store.records();
        assert_eq!(records.len(), 7);
        let retained = store.page_captures(PageCaptureQuery {
            limit: 16,
            ..Default::default()
        });
        assert_eq!(retained.total, 7);
        for record in &records {
            let capture = retained
                .captures
                .iter()
                .find(|capture| capture.source_storage_key == record.storage_key)
                .unwrap();
            assert_eq!(capture.source_url, record.url);
            assert_eq!(capture.final_url, record.final_url);
            assert!(capture.rendered_html.is_none());
            assert!(
                capture
                    .response_headers
                    .as_ref()
                    .unwrap()
                    .iter()
                    .filter(|header| header.name == "set-cookie")
                    .all(|header| header.value == "[redacted]")
            );
            if record.url.ends_with("/page") || record.url.ends_with("/redirect") {
                assert_eq!(record.title.as_deref(), Some("Retained page"));
                assert_eq!(record.outlink_count, 1);
                assert_eq!(
                    record.response_hash.as_deref(),
                    Some(blake3::hash(html.as_bytes()).to_hex().as_str())
                );
                assert!(capture.raw_html_truncated && capture.visible_text_truncated);
                assert!(capture.raw_html.as_ref().unwrap().len() <= 1024);
                assert!(
                    !capture
                        .visible_text
                        .as_ref()
                        .unwrap()
                        .contains("Hidden script")
                );
            } else if record.url.ends_with("/empty") {
                assert_eq!(capture.raw_html.as_deref(), Some(""));
                assert_eq!(capture.visible_text.as_deref(), Some(""));
                assert!(!capture.raw_html_truncated);
            } else if record.url.ends_with("/asset") || record.url.ends_with("/large") {
                assert!(capture.raw_html.is_none() && capture.visible_text.is_none());
                if record.url.ends_with("/large") {
                    assert!(record.error.is_some());
                }
            } else {
                assert_eq!(record.status_code, Some(404));
                assert_eq!(capture.visible_text.as_deref(), Some("Missing page"));
            }
        }
        // A fresh run with the default disabled policy removes prior retained data.
        crawl(
            CrawlConfig {
                capture: CaptureConfig::default(),
                ..config
            },
            store.clone(),
            CrawlControl::default(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(store.page_captures(PageCaptureQuery::default()).total, 0);
        assert_eq!(store.records().len(), 7);
    }
    server.abort();
}
