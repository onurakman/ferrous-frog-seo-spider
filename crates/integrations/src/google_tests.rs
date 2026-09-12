use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn serve_json(
    status: &'static str,
    body: &'static str,
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
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

#[test]
fn pkce_and_authorization_url_follow_the_rfc_vectors() {
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_eq!(base64url_no_pad(b""), "");
    assert_eq!(base64url_no_pad(b"f"), "Zg");
    assert_eq!(base64url_no_pad(b"fo"), "Zm8");
    assert_eq!(base64url_no_pad(b"foo"), "Zm9v");
    let url = google_authorization_url(
        " client-id ",
        "http://127.0.0.1:4567",
        &[
            "https://www.googleapis.com/auth/webmasters.readonly",
            "openid",
        ],
        "state-1",
        "verifier",
    )
    .unwrap();
    let pairs: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(url.host_str(), Some("accounts.google.com"));
    assert_eq!(pairs["client_id"], "client-id");
    assert_eq!(pairs["redirect_uri"], "http://127.0.0.1:4567");
    assert_eq!(
        pairs["scope"],
        "https://www.googleapis.com/auth/webmasters.readonly openid"
    );
    assert_eq!(pairs["code_challenge_method"], "S256");
    assert_eq!(pairs["code_challenge"], pkce_challenge("verifier"));
    assert_eq!(pairs["access_type"], "offline");
}

#[tokio::test]
async fn token_exchange_and_refresh_post_form_bodies_and_parse_tokens() {
    let (endpoint, server) = serve_json(
        "200 OK",
        r#"{"access_token":"ya29.a","expires_in":3599,"refresh_token":"1//r","scope":"openid","token_type":"Bearer"}"#,
    )
    .await;
    let tokens = exchange_google_code(
        &endpoint,
        "id",
        "secret",
        "code-1",
        "http://127.0.0.1:1",
        "verifier",
    )
    .await
    .unwrap();
    let request = server.await.unwrap();
    assert!(request.starts_with("POST /token HTTP/1.1"));
    assert!(request.ends_with("client_id=id&client_secret=secret&code=code-1&redirect_uri=http%3A%2F%2F127.0.0.1%3A1&grant_type=authorization_code&code_verifier=verifier"), "{request}");
    assert_eq!(
        tokens,
        OAuthTokens {
            access_token: "ya29.a".into(),
            refresh_token: Some("1//r".into()),
            expires_in_secs: 3599,
            scope: "openid".into()
        }
    );

    let (endpoint, server) =
        serve_json("200 OK", r#"{"access_token":"ya29.b","expires_in":100}"#).await;
    let refreshed = refresh_google_tokens(&endpoint, "id", "secret", "1//r")
        .await
        .unwrap();
    assert!(server.await.unwrap().ends_with(
        "client_id=id&client_secret=secret&refresh_token=1%2F%2Fr&grant_type=refresh_token"
    ));
    assert_eq!(refreshed.refresh_token, None);
    assert_eq!(refreshed.expires_in_secs, 100);

    let (endpoint, server) = serve_json(
        "400 Bad Request",
        r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
    )
    .await;
    let error = refresh_google_tokens(&endpoint, "id", "secret", "1//r")
        .await
        .unwrap_err()
        .to_string();
    server.await.unwrap();
    assert!(
        error.contains("HTTP 400") && error.contains("expired or revoked"),
        "{error}"
    );
}

#[tokio::test]
async fn analytics_provider_reports_host_paths_and_metrics() {
    let (endpoint, server) = serve_json(
        "200 OK",
        r#"{"dimensionHeaders":[{"name":"hostName"},{"name":"pagePath"}],
            "metricHeaders":[{"name":"sessions"},{"name":"engagedSessions"},{"name":"keyEvents"},{"name":"totalRevenue"}],
            "rows":[{"dimensionValues":[{"value":"example.test"},{"value":"/docs/"}],"metricValues":[{"value":"120"},{"value":"80"},{"value":"3"},{"value":"12.5"}]},
                    {"dimensionValues":[{"value":"example.test"},{"value":"blog"}],"metricValues":[{"value":"7"},{"value":"x"},{"value":"0"},{"value":"0"}]},
                    {"dimensionValues":[{"value":""},{"value":"/skip"}],"metricValues":[]}]}"#,
    )
    .await;
    let provider = GoogleAnalyticsProvider::new(AnalyticsConfig {
        property_id: "properties/123456".into(),
        access_token: " token ".into(),
    })
    .with_endpoint(url::Url::parse(&endpoint).unwrap());
    let response = provider
        .fetch_metrics(MetricRequest {
            urls: Vec::new(),
            date_range: Some(DateRange {
                start_date: "2026-08-01".into(),
                end_date: "2026-08-31".into(),
            }),
        })
        .await
        .unwrap();
    let request = server.await.unwrap();
    assert!(request.contains("authorization: Bearer token"));
    assert!(request.contains(r#""name":"pagePath""#) && request.contains(r#""name":"keyEvents""#));
    assert_eq!(response.rows.len(), 2);
    assert_eq!(response.rows[0].url, "https://example.test/docs/");
    assert_eq!(
        response.rows[0].analytics,
        Some(AnalyticsMetrics {
            sessions: 120.0,
            engaged_sessions: 80.0,
            conversions: 3.0,
            revenue: 12.5
        })
    );
    assert_eq!(response.rows[1].url, "https://example.test/blog");
    assert_eq!(
        response.rows[1]
            .analytics
            .as_ref()
            .unwrap()
            .engaged_sessions,
        0.0
    );
    assert!(
        GoogleAnalyticsProvider::new(AnalyticsConfig {
            property_id: "abc".into(),
            access_token: "t".into()
        })
        .endpoint()
        .is_err()
    );
    assert_eq!(
        GoogleAnalyticsProvider::new(AnalyticsConfig {
            property_id: " 42 ".into(),
            access_token: "t".into()
        })
        .endpoint()
        .unwrap()
        .as_str(),
        "https://analyticsdata.googleapis.com/v1beta/properties/42:runReport"
    );
}

#[tokio::test]
async fn backlink_endpoint_provider_substitutes_urls_sends_credentials_and_parses_counts() {
    let (endpoint, server) = serve_json(
        "200 OK",
        r#"{"backlinks":"42","referring_domains":7,"authorityScore":31.5}"#,
    )
    .await;
    let provider = BacklinkEndpointProvider::new(BacklinkEndpointConfig {
        endpoint_template: format!("{endpoint}?target={{url}}&mode=exact"),
        header_name: Some(" X-Api-Key ".into()),
        header_value: Some("secret-1".into()),
    })
    .unwrap();
    let metrics = provider
        .fetch_url("https://example.test/a b?x=1")
        .await
        .unwrap();
    let request = server.await.unwrap();
    assert!(
        request.starts_with(
            "GET /token?target=https%3A%2F%2Fexample.test%2Fa+b%3Fx%3D1&mode=exact HTTP/1.1"
        ),
        "{request}"
    );
    assert!(request.contains("x-api-key: secret-1"));
    assert_eq!(
        metrics,
        BacklinkMetrics {
            referring_domains: 7,
            backlinks: 42,
            authority_score: Some(31.5)
        }
    );
    let (endpoint, server) = serve_json("200 OK", r#"{"backlinks":"many"}"#).await;
    let provider = BacklinkEndpointProvider::new(BacklinkEndpointConfig {
        endpoint_template: format!("{endpoint}/{{url}}"),
        header_name: None,
        header_value: None,
    })
    .unwrap();
    assert!(provider.fetch_url("https://example.test/").await.is_err());
    server.await.unwrap();
    assert!(
        BacklinkEndpointProvider::new(BacklinkEndpointConfig {
            endpoint_template: "https://api.test/lookup".into(),
            header_name: None,
            header_value: None,
        })
        .is_err()
    );
    assert!(
        BacklinkEndpointProvider::new(BacklinkEndpointConfig {
            endpoint_template: "https://api.test/{url}".into(),
            header_name: Some("bad header".into()),
            header_value: Some("x".into()),
        })
        .is_err()
    );
}
