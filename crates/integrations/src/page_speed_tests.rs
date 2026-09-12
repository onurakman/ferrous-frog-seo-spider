use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::timeout;

const API_KEY: &str = "PSI_SENTINEL_SECRET+?&=";
const REQUESTED_URL: &str =
    "https://example.test/path%2Fpart?name=a%2Bb&space=one%20two&repeated=1&repeated=2";
const SUCCESS: &str = r#"{
  "lighthouseResult": {
    "finalUrl": "https://example.test/final?name=a%2Bb",
    "fetchTime": "2026-09-09T12:34:56.000Z",
    "lighthouseVersion": "13.0.0",
    "categories": { "performance": { "score": 0.92 } },
    "audits": { "total-blocking-time": { "numericValue": 0.0 } }
  }
}"#;

fn config(strategy: PageSpeedStrategy) -> PageSpeedConfig {
    PageSpeedConfig {
        api_key: Some(format!(" {API_KEY} ")),
        strategy,
        locale: Some(" en-US ".to_string()),
        categories: PageSpeedCategory::ALL.to_vec(),
    }
}

async fn read_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0, "client closed before sending request headers");
        request.extend_from_slice(&buffer[..read]);
        assert!(request.len() <= 16 * 1024);
    }
    String::from_utf8(request).unwrap()
}

async fn serve_once(response: Vec<u8>) -> (url::Url, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = url::Url::parse(&format!(
        "http://{}/pagespeedonline/v5/runPagespeed",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        // Rejection/cancellation may close the connection before the fixture is sent in full.
        let _ = stream.write_all(&response).await;
        let _ = stream.shutdown().await;
        request
    });
    (endpoint, server)
}

fn response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn chunked_response(body: &[u8], extra_headers: &str) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n{extra_headers}Connection: close\r\n\r\n"
    )
    .into_bytes();
    for chunk in body.chunks(32 * 1024) {
        response.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
        response.extend_from_slice(chunk);
        response.extend_from_slice(b"\r\n");
    }
    response.extend_from_slice(b"0\r\n\r\n");
    response
}

fn assert_redacted(error: &IntegrationError, endpoint: &url::Url) {
    let text = format!("{error}\n{error:?}");
    assert!(!text.contains("PSI_SENTINEL_SECRET"), "{text}");
    assert!(!text.contains(REQUESTED_URL), "{text}");
    assert!(!text.contains(endpoint.as_str()), "{text}");
}

#[tokio::test]
async fn requests_preserve_encoding_strategy_categories_and_attribution() {
    for (strategy, expected_strategy) in [
        (PageSpeedStrategy::Mobile, "mobile"),
        (PageSpeedStrategy::Desktop, "desktop"),
    ] {
        let (endpoint, server) = serve_once(response("200 OK", SUCCESS.as_bytes())).await;
        let provider = PageSpeedProvider::new(config(strategy)).unwrap();
        let row = provider
            .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
            .await
            .unwrap();
        assert_eq!(row.url, REQUESTED_URL);
        let metrics = serde_json::to_value(row.page_speed.unwrap()).unwrap();
        assert_eq!(metrics["performanceScore"], 0.92);
        assert_eq!(metrics["tbtMs"], 0.0);
        assert_eq!(metrics["inpMs"], serde_json::Value::Null);
        assert_eq!(metrics["finalUrl"], "https://example.test/final?name=a%2Bb");
        assert_eq!(metrics["fetchedAt"], "2026-09-09T12:34:56.000Z");
        assert_eq!(metrics["lighthouseVersion"], "13.0.0");

        let request = server.await.unwrap();
        assert!(request.starts_with("GET "));
        let path = request.split_whitespace().nth(1).unwrap();
        let request_url = endpoint.join(path).unwrap();
        let query = request_url.query_pairs().into_owned().collect::<Vec<_>>();
        assert_eq!(query.len(), 8);
        assert_eq!(query[0], ("url".to_string(), REQUESTED_URL.to_string()));
        assert_eq!(
            query[1],
            ("strategy".to_string(), expected_strategy.to_string())
        );
        assert_eq!(
            query
                .iter()
                .filter(|(key, _)| key == "category")
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            ["performance", "accessibility", "best-practices", "seo"]
        );
        assert!(query.contains(&("key".to_string(), API_KEY.to_string())));
        assert!(query.contains(&("locale".to_string(), "en-US".to_string())));
    }
}

#[tokio::test]
async fn optional_empty_key_and_locale_are_omitted() {
    for api_key in [None, Some("  ".to_string())] {
        let (endpoint, server) = serve_once(response("200 OK", SUCCESS.as_bytes())).await;
        let provider = PageSpeedProvider::new(PageSpeedConfig {
            api_key,
            strategy: PageSpeedStrategy::Mobile,
            locale: None,
            categories: Vec::new(),
        })
        .unwrap();
        provider
            .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
            .await
            .unwrap();
        let request = server.await.unwrap();
        let request_url = endpoint
            .join(request.split_whitespace().nth(1).unwrap())
            .unwrap();
        assert!(
            !request_url
                .query_pairs()
                .any(|(name, _)| name == "key" || name == "locale")
        );
    }
}

#[tokio::test]
async fn invalid_or_authenticated_urls_are_rejected_before_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = url::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
    for url in [
        "https://@example.test/",
        "https:/@example.test/",
        "https://\\@example.test/",
        "https://\t@example.test/",
        "not a URL",
        "file:///tmp/page.html",
        "data:text/html,hello",
        "https://user:password@example.test/",
        "https://user@example.test/",
        "https://example.test:invalid/",
    ] {
        let result = timeout(
            Duration::from_millis(200),
            provider.fetch_url(url.to_string(), endpoint.clone()),
        )
        .await
        .expect("invalid URLs must be rejected before awaiting the network");
        assert!(matches!(result, Err(IntegrationError::InvalidData(_))));
    }
    assert!(
        timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn http_failures_report_status_without_echoing_urls_or_response_bodies() {
    for status in ["429 Too Many Requests", "403 Forbidden"] {
        let body = format!("{API_KEY}: {REQUESTED_URL}");
        let (endpoint, server) = serve_once(response(status, body.as_bytes())).await;
        let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
        let error = provider
            .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
            .await
            .unwrap_err();
        assert!(matches!(error, IntegrationError::RequestFailed(_)));
        assert!(error.to_string().contains(&status[..3]));
        assert_redacted(&error, &endpoint);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn redirects_are_rejected_without_contacting_the_target() {
    let (target, mut target_server) = serve_once(response("200 OK", SUCCESS.as_bytes())).await;
    let redirect = format!(
        "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let (endpoint, server) = serve_once(redirect.into_bytes()).await;
    let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
    let error = provider
        .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
        .await
        .unwrap_err();
    assert!(matches!(error, IntegrationError::RequestFailed(_)));
    assert!(error.to_string().contains("302"));
    assert_redacted(&error, &endpoint);
    server.await.unwrap();
    assert!(
        timeout(Duration::from_millis(30), &mut target_server)
            .await
            .is_err()
    );
    target_server.abort();
}

#[tokio::test]
async fn invalid_json_and_failed_runs_do_not_echo_sensitive_payload_values() {
    let echoed_value = format!("{API_KEY} {REQUESTED_URL}");
    let fixtures = [
        "not JSON".to_string(),
        "{}".to_string(),
        r#"{"lighthouseResult":null}"#.to_string(),
        serde_json::json!({ "lighthouseResult": {
            "categories": { "performance": { "score": &echoed_value } }, "audits": {}
        }})
        .to_string(),
        serde_json::json!({ "lighthouseResult": {
            "categories": {}, "audits": {},
            "runtimeError": { "code": "NO_FCP", "message": &echoed_value }
        }})
        .to_string(),
        serde_json::json!({ "lighthouseResult": {
            "categories": {}, "audits": {},
            "runtimeError": { "code": &echoed_value, "message": "Analysis failed" }
        }})
        .to_string(),
    ];
    for fixture in fixtures {
        let (endpoint, server) = serve_once(response("200 OK", fixture.as_bytes())).await;
        let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
        let error = provider
            .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
            .await
            .unwrap_err();
        assert!(matches!(error, IntegrationError::InvalidData(_)));
        assert_redacted(&error, &endpoint);
        server.await.unwrap();
    }
}

#[test]
fn invalid_scores_and_lab_values_are_rejected_before_becoming_metrics() {
    for category in ["performance", "accessibility", "best-practices", "seo"] {
        for invalid in [
            -0.01,
            1.01,
            200.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let mut payload: PageSpeedResponse = serde_json::from_str(SUCCESS).unwrap();
            payload
                .lighthouse_result
                .as_mut()
                .unwrap()
                .categories
                .insert(
                    category.to_string(),
                    LighthouseCategory {
                        score: Some(invalid),
                    },
                );
            assert!(
                matches!(
                    page_speed_response_to_metrics(payload),
                    Err(IntegrationError::InvalidData(_))
                ),
                "invalid {category} score {invalid} was accepted"
            );
        }
    }
    for audit in [
        "largest-contentful-paint",
        "total-blocking-time",
        "cumulative-layout-shift",
    ] {
        for invalid in [-0.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut payload: PageSpeedResponse = serde_json::from_str(SUCCESS).unwrap();
            payload.lighthouse_result.as_mut().unwrap().audits.insert(
                audit.to_string(),
                LighthouseAudit {
                    numeric_value: Some(invalid),
                },
            );
            assert!(
                matches!(
                    page_speed_response_to_metrics(payload),
                    Err(IntegrationError::InvalidData(_))
                ),
                "invalid {audit} value {invalid} was accepted"
            );
        }
    }
}

#[test]
fn no_error_runtime_status_preserves_successful_metrics() {
    let mut payload: serde_json::Value = serde_json::from_str(SUCCESS).unwrap();
    payload["lighthouseResult"]["runtimeError"] = serde_json::json!({
        "code": "NO_ERROR", "message": ""
    });
    let metrics = page_speed_response_to_metrics(serde_json::from_value(payload).unwrap()).unwrap();
    assert_eq!(metrics.performance_score, Some(0.92));
}

#[tokio::test]
async fn chunked_response_accepts_the_byte_limit_and_rejects_one_extra_byte() {
    for (size, succeeds) in [(16 * 1024 * 1024, true), (16 * 1024 * 1024 + 1, false)] {
        let mut body = br#"{"lighthouseResult":{"categories":{},"audits":{}},"padding":""#.to_vec();
        body.resize(size - 2, b'a');
        body.extend_from_slice(b"\"}");
        let (endpoint, server) = serve_once(chunked_response(&body, "")).await;
        let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
        let result = provider
            .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
            .await;
        if succeeds {
            assert!(result.is_ok(), "{result:?}");
        } else {
            let error = result.unwrap_err();
            assert!(matches!(error, IntegrationError::InvalidData(_)));
            assert!(error.to_string().contains("16 MiB"));
            assert_redacted(&error, &endpoint);
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn response_limit_applies_after_gzip_decoding() {
    // The fixture is valid Lighthouse JSON with a padding string, 16 MiB + 1 decoded bytes.
    let compressed = include_bytes!("fixtures/pagespeed-oversized.json.gz");
    let (endpoint, server) =
        serve_once(chunked_response(compressed, "Content-Encoding: gzip\r\n")).await;
    let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
    let error = provider
        .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("16 MiB"));
    assert_redacted(&error, &endpoint);
    server.await.unwrap();
}

#[tokio::test]
async fn advertised_oversized_response_is_rejected_before_waiting_for_its_body() {
    let wire = b"HTTP/1.1 200 OK\r\nContent-Length: 16777217\r\nConnection: close\r\n\r\n";
    let (endpoint, server) = serve_once(wire.to_vec()).await;
    let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
    let error = provider
        .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("16 MiB"));
    assert_redacted(&error, &endpoint);
    server.await.unwrap();
}

#[tokio::test]
async fn deadline_covers_both_response_headers_and_incomplete_response_bodies() {
    for after_headers in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint =
            url::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            if after_headers {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n{\r\n")
                    .await
                    .unwrap();
            }
            let mut byte = [0];
            stream.read(&mut byte).await
        });
        let provider = PageSpeedProvider::with_request_timeout(
            config(PageSpeedStrategy::Mobile),
            Duration::from_millis(60),
        )
        .unwrap();
        let result = timeout(
            Duration::from_secs(2),
            provider.fetch_url(REQUESTED_URL.to_string(), endpoint.clone()),
        )
        .await
        .expect("configured request deadline must finish a stalled response");
        let error = result.unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert_redacted(&error, &endpoint);
        let closed = timeout(Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(closed, Ok(0) | Err(_)),
            "timed-out request retained its socket"
        );
    }
}

#[tokio::test]
async fn dropping_measurement_closes_the_incomplete_transport() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = url::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let (started, received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n{\r\n")
            .await
            .unwrap();
        started.send(()).unwrap();
        let mut byte = [0];
        stream.read(&mut byte).await
    });
    let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
    let mut measurement = Box::pin(provider.fetch_url(REQUESTED_URL.to_string(), endpoint));
    timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = &mut measurement => panic!("incomplete response finished early: {result:?}"),
            result = received => result.unwrap(),
        }
    })
    .await
    .unwrap();
    drop(measurement);
    let closed = timeout(Duration::from_secs(1), server)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(closed, Ok(0) | Err(_)),
        "dropped measurement retained its socket"
    );
}

#[tokio::test]
async fn transport_disconnects_do_not_expose_the_api_request_url() {
    for wire in [
        b"".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{",
    ] {
        let (endpoint, server) = serve_once(wire.to_vec()).await;
        let provider = PageSpeedProvider::new(config(PageSpeedStrategy::Mobile)).unwrap();
        let error = provider
            .fetch_url(REQUESTED_URL.to_string(), endpoint.clone())
            .await
            .unwrap_err();
        assert_redacted(&error, &endpoint);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn selected_categories_limit_the_request_and_empty_means_all() {
    let (endpoint, server) = serve_once(response("200 OK", SUCCESS.as_bytes())).await;
    let provider = PageSpeedProvider::new(PageSpeedConfig {
        categories: vec![
            PageSpeedCategory::Seo,
            PageSpeedCategory::Performance,
            PageSpeedCategory::Seo,
        ],
        ..config(PageSpeedStrategy::Mobile)
    })
    .unwrap();
    provider
        .fetch_url(REQUESTED_URL.to_string(), endpoint)
        .await
        .unwrap();
    let request = server.await.unwrap();
    let line = request.lines().next().unwrap();
    assert!(line.contains("category=performance&category=seo"), "{line}");
    assert!(
        !line.contains("accessibility") && !line.contains("best-practices"),
        "{line}"
    );
    let parsed: PageSpeedConfig =
        serde_json::from_str(r#"{"apiKey":null,"strategy":"mobile","locale":null}"#).unwrap();
    assert_eq!(parsed.categories, PageSpeedCategory::ALL.to_vec());
}
