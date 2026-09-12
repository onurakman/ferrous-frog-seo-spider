//! Basic HTTP credentials kept in the OS credential store, separate from profiles.

use crate::KEYRING_SERVICE;
use ferrous_frog_crawler_core::BasicCredentials;
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};

const KEYRING_ACCOUNT: &str = "http-basic-auth";
const FORM_LOGIN_ACCOUNT: &str = "form-login-credentials";
const KEYRING_UNAVAILABLE: &str =
    "The OS credential store is unavailable. Unlock it and try again.";

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthStatus {
    saved: bool,
    username: Option<String>,
    keyring_available: bool,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveHttpAuthRequest {
    username: String,
    password: String,
}

#[derive(Serialize, Deserialize)]
struct StoredCredentials {
    username: String,
    password: String,
}

fn validate(username: &str, password: &str) -> Result<StoredCredentials, String> {
    let username = username.trim();
    if username.is_empty()
        || username.len() > 512
        || username.chars().any(|c| c.is_control() || c == ':')
    {
        return Err("Username must contain 1–512 bytes without control characters or ':'".into());
    }
    if password.is_empty() || password.len() > 1024 || password.chars().any(char::is_control) {
        return Err("Password must contain 1–1024 bytes without control characters".into());
    }
    Ok(StoredCredentials {
        username: username.to_string(),
        password: password.to_string(),
    })
}

fn decode(value: &str) -> Option<StoredCredentials> {
    let stored: StoredCredentials = serde_json::from_str(value).ok()?;
    validate(&stored.username, &stored.password).ok()
}

fn status(result: Result<String, KeyringError>) -> HttpAuthStatus {
    match result {
        Ok(value) => match decode(&value) {
            Some(stored) => HttpAuthStatus {
                saved: true,
                username: Some(stored.username),
                keyring_available: true,
                message: None,
            },
            None => HttpAuthStatus {
                saved: false,
                username: None,
                keyring_available: true,
                message: Some(
                    "The saved HTTP credentials are invalid; replace or clear them.".into(),
                ),
            },
        },
        Err(KeyringError::NoEntry) => HttpAuthStatus {
            saved: false,
            username: None,
            keyring_available: true,
            message: None,
        },
        Err(_) => HttpAuthStatus {
            saved: false,
            username: None,
            keyring_available: false,
            message: Some(KEYRING_UNAVAILABLE.into()),
        },
    }
}

fn keyring_entry(account: &str) -> Result<Entry, KeyringError> {
    Entry::new(KEYRING_SERVICE, account)
}

/// Credentials for the crawl, or an error when enabled credentials are missing or unreadable.
pub fn saved_credentials(result: Result<String, KeyringError>) -> Result<BasicCredentials, String> {
    match result {
        Ok(value) => decode(&value)
            .map(|stored| BasicCredentials {
                username: stored.username,
                password: stored.password,
            })
            .ok_or_else(|| "The saved HTTP credentials are invalid; replace or clear them in Settings > HTTP headers".into()),
        Err(KeyringError::NoEntry) => Err(
            "HTTP authentication is enabled but no credentials are saved; add them in Settings > HTTP headers or turn the option off".into(),
        ),
        Err(_) => Err(KEYRING_UNAVAILABLE.into()),
    }
}

async fn load_account(account: &'static str) -> Result<BasicCredentials, String> {
    tauri::async_runtime::spawn_blocking(move || {
        saved_credentials(keyring_entry(account).and_then(|entry| entry.get_password()))
    })
    .await
    .map_err(|_| "HTTP credential worker failed".to_string())?
}

pub async fn load_saved_credentials() -> Result<BasicCredentials, String> {
    load_account(KEYRING_ACCOUNT).await
}

pub async fn load_form_login_credentials() -> Result<BasicCredentials, String> {
    load_account(FORM_LOGIN_ACCOUNT)
        .await
        .map_err(|error| error.replace("HTTP authentication is enabled", "Form login is enabled"))
}

async fn account_status(account: &'static str) -> Result<HttpAuthStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        status(keyring_entry(account).and_then(|entry| entry.get_password()))
    })
    .await
    .map_err(|_| "HTTP credential worker failed".into())
}

async fn save_account(
    account: &'static str,
    request: SaveHttpAuthRequest,
) -> Result<HttpAuthStatus, String> {
    let stored = validate(&request.username, &request.password)?;
    let username = stored.username.clone();
    let encoded = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        keyring_entry(account)
            .and_then(|entry| entry.set_password(&encoded))
            .map_err(|_| "Could not save the credentials in the OS credential store".to_string())?;
        Ok(HttpAuthStatus {
            saved: true,
            username: Some(username),
            keyring_available: true,
            message: None,
        })
    })
    .await
    .map_err(|_| "HTTP credential worker failed")?
}

async fn clear_account(account: &'static str) -> Result<HttpAuthStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        match keyring_entry(account).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(HttpAuthStatus {
                saved: false,
                username: None,
                keyring_available: true,
                message: None,
            }),
            Err(_) => Err("Could not clear the credentials from the OS credential store".into()),
        }
    })
    .await
    .map_err(|_| "HTTP credential worker failed")?
}

#[tauri::command]
pub async fn get_http_auth_status() -> Result<HttpAuthStatus, String> {
    account_status(KEYRING_ACCOUNT).await
}

#[tauri::command]
pub async fn save_http_auth_credentials(
    request: SaveHttpAuthRequest,
) -> Result<HttpAuthStatus, String> {
    save_account(KEYRING_ACCOUNT, request).await
}

#[tauri::command]
pub async fn clear_http_auth_credentials() -> Result<HttpAuthStatus, String> {
    clear_account(KEYRING_ACCOUNT).await
}

#[tauri::command]
pub async fn get_form_login_status() -> Result<HttpAuthStatus, String> {
    account_status(FORM_LOGIN_ACCOUNT).await
}

#[tauri::command]
pub async fn save_form_login_credentials(
    request: SaveHttpAuthRequest,
) -> Result<HttpAuthStatus, String> {
    save_account(FORM_LOGIN_ACCOUNT, request).await
}

#[tauri::command]
pub async fn clear_form_login_credentials() -> Result<HttpAuthStatus, String> {
    clear_account(FORM_LOGIN_ACCOUNT).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_validated_decoded_and_reported_without_secrets() {
        assert!(validate("", "x").is_err());
        assert!(validate("user:name", "x").is_err());
        assert!(validate("user", "").is_err());
        assert!(validate("user", "bad\nline").is_err());
        let stored = serde_json::to_string(&validate(" frog ", "pass word").unwrap()).unwrap();
        let ready = status(Ok(stored.clone()));
        assert_eq!(ready.username.as_deref(), Some("frog"));
        assert!(ready.saved && ready.keyring_available);
        assert!(!serde_json::to_string(&ready).unwrap().contains("pass word"));
        assert_eq!(
            saved_credentials(Ok(stored)).unwrap(),
            BasicCredentials {
                username: "frog".into(),
                password: "pass word".into()
            }
        );
        assert!(status(Ok("not json".into())).message.is_some());
        assert!(saved_credentials(Ok("not json".into())).is_err());
        let missing = status(Err(KeyringError::NoEntry));
        assert!(!missing.saved && missing.keyring_available && missing.message.is_none());
        assert!(
            saved_credentials(Err(KeyringError::NoEntry))
                .unwrap_err()
                .contains("no credentials are saved")
        );
        let unavailable = status(Err(KeyringError::NoStorageAccess("locked".into())));
        assert!(!unavailable.keyring_available);
    }
}
