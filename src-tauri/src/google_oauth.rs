//! Google account connection: a desktop OAuth client (loopback redirect + PKCE) whose tokens live
//! in the OS credential store and refresh themselves for Search Console and Analytics requests.

use crate::{KEYRING_SERVICE, now_ms};
use ferrous_frog_integrations::{
    GOOGLE_TOKEN_ENDPOINT, OAuthTokens, base64url_no_pad, exchange_google_code,
    google_authorization_url, refresh_google_tokens,
};
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const CLIENT_ACCOUNT: &str = "google-oauth-client";
const TOKEN_ACCOUNT: &str = "google-oauth-tokens";
const KEYRING_UNAVAILABLE: &str =
    "The OS credential store is unavailable. Unlock it and try again.";
const CONSENT_TIMEOUT: Duration = Duration::from_secs(300);
pub const SEARCH_CONSOLE_SCOPE: &str = "https://www.googleapis.com/auth/webmasters.readonly";
pub const ANALYTICS_SCOPE: &str = "https://www.googleapis.com/auth/analytics.readonly";

#[derive(Default)]
pub struct GoogleOAuthState {
    connecting: AtomicBool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredClient {
    client_id: String,
    client_secret: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredTokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_ms: i64,
    scope: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleOAuthStatus {
    client_configured: bool,
    client_id: Option<String>,
    connected: bool,
    refreshable: bool,
    expires_at_ms: Option<i64>,
    scopes: Vec<String>,
    keyring_available: bool,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveGoogleOAuthClientRequest {
    client_id: String,
    client_secret: String,
}

fn entry(account: &str) -> Result<Entry, KeyringError> {
    Entry::new(KEYRING_SERVICE, account)
}

fn read_json<T: for<'de> Deserialize<'de>>(account: &str) -> Result<Option<T>, String> {
    match entry(account).and_then(|entry| entry.get_password()) {
        Ok(value) => serde_json::from_str(&value).map(Some).map_err(|_| {
            "The saved Google credentials are unreadable; disconnect and connect again".to_string()
        }),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(_) => Err(KEYRING_UNAVAILABLE.to_string()),
    }
}

fn write_json<T: Serialize>(account: &str, value: &T) -> Result<(), String> {
    let encoded = serde_json::to_string(value).map_err(|error| error.to_string())?;
    entry(account)
        .and_then(|entry| entry.set_password(&encoded))
        .map_err(|_| "Could not save Google credentials in the OS credential store".to_string())
}

fn delete(account: &str) -> Result<(), String> {
    match entry(account).and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(_) => Err("Could not clear Google credentials from the OS credential store".into()),
    }
}

fn validate_client(client_id: &str, client_secret: &str) -> Result<StoredClient, String> {
    let client_id = client_id.trim();
    let client_secret = client_secret.trim();
    let clean = |value: &str, max: usize| {
        !value.is_empty()
            && value.len() <= max
            && !value.chars().any(|c| c.is_control() || c.is_whitespace())
    };
    if !clean(client_id, 512) || !clean(client_secret, 512) {
        return Err(
            "OAuth client ID and secret must contain 1–512 bytes without whitespace or control characters".into(),
        );
    }
    Ok(StoredClient {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
    })
}

/// 32 bytes drawn from process-random hasher seeds and the clock; enough entropy for PKCE/state.
fn random_token() -> String {
    let mut bytes = Vec::with_capacity(32);
    for round in 0..4u64 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_i64(now_ms());
        hasher.write_u128(std::time::Instant::now().elapsed().as_nanos());
        bytes.extend_from_slice(&hasher.finish().to_le_bytes());
    }
    base64url_no_pad(&bytes)
}

fn status_from(
    client: Result<Option<StoredClient>, String>,
    tokens: Result<Option<StoredTokens>, String>,
) -> GoogleOAuthStatus {
    let mut status = GoogleOAuthStatus {
        client_configured: false,
        client_id: None,
        connected: false,
        refreshable: false,
        expires_at_ms: None,
        scopes: Vec::new(),
        keyring_available: true,
        message: None,
    };
    match client {
        Ok(Some(client)) => {
            status.client_configured = true;
            status.client_id = Some(client.client_id);
        }
        Ok(None) => {}
        Err(error) => {
            status.keyring_available = error != KEYRING_UNAVAILABLE && status.keyring_available;
            status.message = Some(error);
        }
    }
    match tokens {
        Ok(Some(tokens)) => {
            status.connected = true;
            status.refreshable = tokens.refresh_token.is_some();
            status.expires_at_ms = Some(tokens.expires_at_ms);
            status.scopes = tokens
                .scope
                .split_whitespace()
                .map(str::to_string)
                .collect();
        }
        Ok(None) => {}
        Err(error) => {
            status.keyring_available = error != KEYRING_UNAVAILABLE && status.keyring_available;
            status.message.get_or_insert(error);
        }
    }
    status
}

fn tokens_from(tokens: OAuthTokens, previous_refresh: Option<String>) -> StoredTokens {
    StoredTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.or(previous_refresh),
        expires_at_ms: now_ms()
            .saturating_add((tokens.expires_in_secs as i64).saturating_mul(1000)),
        scope: tokens.scope,
    }
}

/// Parses the loopback redirect request and returns the authorization code when the state matches.
fn parse_redirect(request: &str, expected_state: &str) -> Result<String, String> {
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or("malformed redirect request")?;
    let url = url::Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| "malformed redirect URL".to_string())?;
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Err(format!("Google refused the consent request: {error}"));
    }
    if state.as_deref() != Some(expected_state) {
        return Err("The consent response did not match this connection attempt".into());
    }
    code.filter(|code| !code.is_empty())
        .ok_or_else(|| "Google did not return an authorization code".into())
}

/// Waits for Google's redirect on the loopback listener and answers with a small HTML page.
async fn receive_code(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    let accept = tokio::time::timeout(CONSENT_TIMEOUT, listener.accept()).await;
    let (mut stream, _) = accept
        .map_err(|_| "Timed out waiting for the Google consent page (5 minutes)".to_string())?
        .map_err(|error| format!("loopback listener failed: {error}"))?;
    let mut buffer = vec![0_u8; 8192];
    let read = stream
        .read(&mut buffer)
        .await
        .map_err(|error| format!("could not read the consent redirect: {error}"))?;
    let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
    let outcome = parse_redirect(&request, expected_state);
    let body = match &outcome {
        Ok(_) => {
            "<html><body style=\"font-family:sans-serif\"><h2>Ferrous Frog is connected</h2><p>You can close this window and return to the app.</p></body></html>"
        }
        Err(_) => {
            "<html><body style=\"font-family:sans-serif\"><h2>Connection failed</h2><p>Return to Ferrous Frog for details.</p></body></html>"
        }
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    outcome
}

/// Runs the consent flow: loopback listener, browser launch, code exchange, token storage.
async fn connect(
    open_browser: impl FnOnce(&str) -> Result<(), String>,
    token_endpoint: &str,
    scopes: &[&str],
) -> Result<GoogleOAuthStatus, String> {
    let client = read_json::<StoredClient>(CLIENT_ACCOUNT)?
        .ok_or("Save the OAuth client ID and secret before connecting a Google account")?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("could not open a loopback port: {error}"))?;
    let redirect_uri = format!(
        "http://127.0.0.1:{}",
        listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port()
    );
    let verifier = format!("{}{}", random_token(), random_token());
    let state = random_token();
    let url = google_authorization_url(&client.client_id, &redirect_uri, scopes, &state, &verifier)
        .map_err(|error| error.to_string())?;
    open_browser(url.as_str())?;
    let code = receive_code(listener, &state).await?;
    let tokens = exchange_google_code(
        token_endpoint,
        &client.client_id,
        &client.client_secret,
        &code,
        &redirect_uri,
        &verifier,
    )
    .await
    .map_err(|error| error.to_string())?;
    let stored = tokens_from(tokens, None);
    write_json(TOKEN_ACCOUNT, &stored)?;
    Ok(status_from(Ok(Some(client)), Ok(Some(stored))))
}

/// A valid access token for API calls, refreshed through the stored refresh token when needed.
pub async fn access_token_with_endpoint(token_endpoint: &str) -> Result<Option<String>, String> {
    let Some(tokens) = read_json::<StoredTokens>(TOKEN_ACCOUNT)? else {
        return Ok(None);
    };
    if tokens.expires_at_ms.saturating_sub(now_ms()) > 60_000 {
        return Ok(Some(tokens.access_token));
    }
    let Some(refresh_token) = tokens.refresh_token.clone() else {
        return Err(
            "The Google access token expired and no refresh token is saved; connect the account again".into(),
        );
    };
    let client = read_json::<StoredClient>(CLIENT_ACCOUNT)?
        .ok_or("The OAuth client is missing; save it and connect the account again")?;
    let refreshed = refresh_google_tokens(
        token_endpoint,
        &client.client_id,
        &client.client_secret,
        &refresh_token,
    )
    .await
    .map_err(|error| error.to_string())?;
    let stored = StoredTokens {
        scope: if refreshed.scope.is_empty() {
            tokens.scope
        } else {
            refreshed.scope.clone()
        },
        ..tokens_from(refreshed, Some(refresh_token))
    };
    write_json(TOKEN_ACCOUNT, &stored)?;
    Ok(Some(stored.access_token))
}

pub async fn access_token() -> Result<Option<String>, String> {
    access_token_with_endpoint(GOOGLE_TOKEN_ENDPOINT).await
}

#[tauri::command]
pub async fn get_google_oauth_status() -> Result<GoogleOAuthStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        status_from(read_json(CLIENT_ACCOUNT), read_json(TOKEN_ACCOUNT))
    })
    .await
    .map_err(|_| "Google credential worker failed".into())
}

#[tauri::command]
pub async fn save_google_oauth_client(
    request: SaveGoogleOAuthClientRequest,
) -> Result<GoogleOAuthStatus, String> {
    let client = validate_client(&request.client_id, &request.client_secret)?;
    tauri::async_runtime::spawn_blocking(move || {
        write_json(CLIENT_ACCOUNT, &client)?;
        Ok(status_from(Ok(Some(client)), read_json(TOKEN_ACCOUNT)))
    })
    .await
    .map_err(|_| "Google credential worker failed")?
}

#[tauri::command]
pub async fn disconnect_google_account(clear_client: bool) -> Result<GoogleOAuthStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        delete(TOKEN_ACCOUNT)?;
        if clear_client {
            delete(CLIENT_ACCOUNT)?;
        }
        Ok(status_from(read_json(CLIENT_ACCOUNT), Ok(None)))
    })
    .await
    .map_err(|_| "Google credential worker failed")?
}

#[tauri::command]
pub async fn connect_google_account(
    app: AppHandle,
    state: tauri::State<'_, GoogleOAuthState>,
) -> Result<GoogleOAuthStatus, String> {
    if state
        .connecting
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A Google connection is already waiting for consent in the browser".into());
    }
    let result = connect(
        |url| {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|error| format!("could not open the browser for Google consent: {error}"))
        },
        GOOGLE_TOKEN_ENDPOINT,
        &[SEARCH_CONSOLE_SCOPE, ANALYTICS_SCOPE],
    )
    .await;
    state.connecting.store(false, Ordering::SeqCst);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_parsing_checks_state_code_and_errors() {
        assert_eq!(
            parse_redirect("GET /?state=s1&code=abc HTTP/1.1\r\nHost: x\r\n\r\n", "s1").unwrap(),
            "abc"
        );
        assert!(
            parse_redirect("GET /?state=other&code=abc HTTP/1.1\r\n", "s1")
                .unwrap_err()
                .contains("did not match")
        );
        assert!(
            parse_redirect("GET /?state=s1&error=access_denied HTTP/1.1\r\n", "s1")
                .unwrap_err()
                .contains("access_denied")
        );
        assert!(parse_redirect("GET /?state=s1 HTTP/1.1\r\n", "s1").is_err());
        assert!(parse_redirect("", "s1").is_err());
        let token = random_token();
        assert!(token.len() >= 40 && token != random_token());
        assert!(validate_client(" id ", "secret").is_ok());
        assert!(validate_client("", "secret").is_err());
        assert!(validate_client("id", "se cret").is_err());
    }

    #[test]
    fn status_reports_client_tokens_and_keyring_problems_without_secrets() {
        let client = StoredClient {
            client_id: "id".into(),
            client_secret: "secret".into(),
        };
        let tokens = StoredTokens {
            access_token: "ya29".into(),
            refresh_token: Some("1//r".into()),
            expires_at_ms: 5,
            scope: "a b".into(),
        };
        let status = status_from(Ok(Some(client.clone())), Ok(Some(tokens)));
        assert!(status.client_configured && status.connected && status.refreshable);
        assert_eq!(status.scopes, vec!["a", "b"]);
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("secret") && !json.contains("ya29") && !json.contains("1//r"));
        let missing = status_from(Ok(Some(client)), Ok(None));
        assert!(missing.client_configured && !missing.connected);
        let broken = status_from(Err(KEYRING_UNAVAILABLE.to_string()), Ok(None));
        assert!(!broken.keyring_available && broken.message.is_some());
        let stored = tokens_from(
            OAuthTokens {
                access_token: "new".into(),
                refresh_token: None,
                expires_in_secs: 10,
                scope: "s".into(),
            },
            Some("kept".into()),
        );
        assert_eq!(stored.refresh_token.as_deref(), Some("kept"));
        assert!(stored.expires_at_ms > now_ms());
    }

    #[tokio::test]
    async fn loopback_receiver_answers_the_browser_and_returns_the_code() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(b"GET /?code=4%2Fabc&state=state-1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .await
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).await.unwrap();
            response
        });
        let code = receive_code(listener, "state-1").await.unwrap();
        assert_eq!(code, "4/abc");
        let response = browser.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK") && response.contains("connected"));
    }
}
