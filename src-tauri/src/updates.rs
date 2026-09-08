use reqwest::{Client, StatusCode, redirect::Policy};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/onurakman/ferrous-frog-seo-spider/releases/latest";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    current_version: String,
    update: Option<UpdateInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    version: String,
    release_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

#[tauri::command]
pub async fn check_for_updates(app: tauri::AppHandle) -> Result<UpdateCheck, String> {
    let current = &app.package_info().version;
    Ok(UpdateCheck {
        current_version: current.to_string(),
        update: fetch_update(current, LATEST_RELEASE_URL).await?,
    })
}

async fn fetch_update(current: &Version, endpoint: &str) -> Result<Option<UpdateInfo>, String> {
    let client = Client::builder()
        .user_agent(format!("FerrousFrogSeoSpider/{current}"))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .redirect(Policy::none())
        .build()
        .map_err(|error| format!("Could not prepare the update check: {error}"))?;
    let response = client
        .get(endpoint)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2026-03-10")
        .send()
        .await
        .map_err(|_| "Could not reach GitHub. Check your connection and try again.".to_string())?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub could not check for updates (HTTP {}). Please try again later.",
            response.status().as_u16()
        ));
    }
    let release = response.json::<GithubRelease>().await.map_err(|_| {
        "GitHub returned an unreadable release. Please try again later.".to_string()
    })?;
    newer_release(release, current)
}

fn newer_release(release: GithubRelease, current: &Version) -> Result<Option<UpdateInfo>, String> {
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .map_err(|_| "The latest release has an invalid version number.".to_string())?;
    if !version.pre.is_empty() || !version.cmp_precedence(current).is_gt() {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        version: version.to_string(),
        // Construct the trusted project URL from the validated tag, not remote HTML or links.
        release_url: format!(
            "https://github.com/onurakman/ferrous-frog-seo-spider/releases/tag/{}",
            release.tag_name
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn release(tag: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag.into(),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn update_checks_use_semantic_precedence_and_ignore_prereleases() {
        for (current, tag, expected) in [
            ("0.9.0", "v0.10.0", true),
            ("1.9.0", "1.10.0", true),
            ("1.10.0", "v1.9.0", false),
            ("1.0.0", "v1.0.0", false),
            ("1.0.0+aaa", "v1.0.0+zzz", false),
            ("1.0.0-rc.1", "v1.0.0", true),
            ("1.0.0", "v1.1.0-beta.1", false),
        ] {
            let update = newer_release(release(tag), &Version::parse(current).unwrap()).unwrap();
            assert_eq!(update.is_some(), expected, "{current} -> {tag}");
        }
    }

    #[test]
    fn updates_reject_invalid_tags_and_only_link_to_the_project_release() {
        let current = Version::new(0, 1, 0);
        for tag in ["main", "v1.0", "v1.0.0/../../other", "https://example.com"] {
            assert!(newer_release(release(tag), &current).is_err(), "{tag}");
        }
        for (draft, prerelease) in [(true, false), (false, true)] {
            assert!(
                newer_release(
                    GithubRelease {
                        draft,
                        prerelease,
                        ..release("v0.2.0")
                    },
                    &current
                )
                .unwrap()
                .is_none()
            );
        }
        let update = newer_release(release("v0.2.0"), &current).unwrap().unwrap();
        assert_eq!(update.version, "0.2.0");
        assert_eq!(
            update.release_url,
            "https://github.com/onurakman/ferrous-frog-seo-spider/releases/tag/v0.2.0"
        );
    }

    #[tokio::test]
    async fn github_requests_distinguish_missing_releases_from_network_and_payload_errors() {
        for (status, body, expected) in [
            (
                "200 OK",
                r#"{"tag_name":"v0.2.0","draft":false,"prerelease":false,"html_url":"https://example.com/untrusted"}"#,
                Ok(true),
            ),
            ("404 Not Found", r#"{"message":"Not Found"}"#, Ok(false)),
            ("403 Forbidden", "{}", Err(())),
            ("429 Too Many Requests", "{}", Err(())),
            ("500 Internal Server Error", "{}", Err(())),
            ("200 OK", "not JSON", Err(())),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/releases/latest", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.ends_with(b"\r\n\r\n") {
                    let length = stream.read(&mut buffer).await.unwrap();
                    assert!(length > 0 && request.len() < 8192);
                    request.extend_from_slice(&buffer[..length]);
                }
                let request = String::from_utf8(request).unwrap().to_lowercase();
                assert!(request.starts_with("get /releases/latest http/1.1"));
                assert!(request.contains("user-agent: ferrousfrogseospider/0.1.0"));
                assert!(request.contains("accept: application/vnd.github+json"));
                assert!(!request.contains("authorization:"));
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
            let result = fetch_update(&Version::new(0, 1, 0), &endpoint).await;
            server.await.unwrap();
            assert_eq!(
                result.map(|update| update.is_some()).map_err(|_| ()),
                expected,
                "{status}: {body}"
            );
        }
    }
}
