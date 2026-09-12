use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn serve_once(
    status: &'static str,
    body: &'static str,
) -> (url::Url, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = url::Url::parse(&format!(
        "http://{}/v1/records:queryRecord",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&request);
            let body_len = text
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if let Some(end) = text.find("\r\n\r\n")
                && request.len() >= end + 4 + body_len
            {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        String::from_utf8(request).unwrap()
    });
    (endpoint, server)
}

fn provider(form_factor: FieldFormFactor) -> FieldVitalsProvider {
    FieldVitalsProvider::new(FieldVitalsConfig {
        api_key: " crux-key ".into(),
        form_factor,
    })
    .unwrap()
}

const SUCCESS: &str = r#"{"record":{"key":{"formFactor":"PHONE","url":"https://example.test/page/"},
 "metrics":{"largest_contentful_paint":{"percentiles":{"p75":2100}},
 "cumulative_layout_shift":{"percentiles":{"p75":"0.05"}},
 "interaction_to_next_paint":{"percentiles":{"p75":180}},
 "first_contentful_paint":{"percentiles":{"p75":1400}},
 "experimental_time_to_first_byte":{"percentiles":{"p75":600}}},
 "collectionPeriod":{"firstDate":{"year":2026,"month":8,"day":15},"lastDate":{"year":2026,"month":9,"day":11}}}}"#;

#[tokio::test]
async fn field_vitals_parse_p75_values_and_collection_period() {
    let (endpoint, server) = serve_once("200 OK", SUCCESS).await;
    let metrics = provider(FieldFormFactor::Phone)
        .fetch_url("https://example.test/page", endpoint)
        .await
        .unwrap();
    let request = server.await.unwrap();
    assert!(request.starts_with("POST /v1/records:queryRecord?key=crux-key HTTP/1.1"));
    assert!(request.ends_with(r#"{"formFactor":"PHONE","url":"https://example.test/page"}"#));
    assert_eq!(
        metrics,
        FieldVitalsMetrics {
            has_data: true,
            lcp_ms_p75: Some(2100.0),
            cls_p75: Some(0.05),
            inp_ms_p75: Some(180.0),
            fcp_ms_p75: Some(1400.0),
            ttfb_ms_p75: Some(600.0),
            collection_period_start: Some("2026-08-15".into()),
            collection_period_end: Some("2026-09-11".into()),
            normalized_url: Some("https://example.test/page/".into()),
        }
    );
}

#[tokio::test]
async fn field_vitals_report_missing_records_failures_and_invalid_input() {
    let (endpoint, server) = serve_once(
        "404 Not Found",
        r#"{"error":{"code":404,"status":"NOT_FOUND"}}"#,
    )
    .await;
    let metrics = provider(FieldFormFactor::Desktop)
        .fetch_url("https://example.test/unknown", endpoint)
        .await
        .unwrap();
    assert!(server.await.unwrap().contains("\"formFactor\":\"DESKTOP\""));
    assert_eq!(metrics, FieldVitalsMetrics::default());

    let (endpoint, server) = serve_once("500 Internal Server Error", "{}").await;
    let error = provider(FieldFormFactor::Phone)
        .fetch_url("https://example.test/", endpoint)
        .await
        .unwrap_err()
        .to_string();
    server.await.unwrap();
    assert!(error.contains("HTTP 500"), "{error}");

    let (endpoint, server) = serve_once("200 OK", "{not json").await;
    let error = provider(FieldFormFactor::Phone)
        .fetch_url("https://example.test/", endpoint)
        .await
        .unwrap_err()
        .to_string();
    server.await.unwrap();
    assert!(error.contains("invalid JSON"), "{error}");

    let (endpoint, _server) = serve_once("200 OK", "{}").await;
    assert!(
        provider(FieldFormFactor::Phone)
            .fetch_url("ftp://example.test/", endpoint)
            .await
            .is_err()
    );
    assert!(
        FieldVitalsProvider::new(FieldVitalsConfig {
            api_key: " ".into(),
            form_factor: FieldFormFactor::Phone
        })
        .is_err()
    );
    assert!(!field_vitals_from_payload(&serde_json::json!({"record":{"metrics":{}}})).has_data);
}
