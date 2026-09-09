use crate::{CrawlConfig, CrawlControl, RequestPolicy};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum JsRenderingBackend {
    #[default]
    ChromeCdp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsRenderingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub backend: JsRenderingBackend,
    #[serde(default = "default_wait_after_load_ms")]
    pub wait_after_load_ms: u64,
}

impl Default for JsRenderingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: JsRenderingBackend::ChromeCdp,
            wait_after_load_ms: default_wait_after_load_ms(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedPage {
    pub html: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderingStatus {
    pub available: bool,
    pub browser_path: Option<String>,
    pub message: String,
}

pub fn rendering_status() -> RenderingStatus {
    #[cfg(feature = "js-rendering")]
    {
        match browser_executable() {
            Ok(path) => RenderingStatus {
                available: true,
                browser_path: Some(path.display().to_string()),
                message: "Compatible browser found.".into(),
            },
            Err(error) => RenderingStatus {
                available: false,
                browser_path: None,
                message: format!(
                    "Browser unavailable: {error}. Install Chrome or Chromium, then check again."
                ),
            },
        }
    }
    #[cfg(not(feature = "js-rendering"))]
    RenderingStatus {
        available: false,
        browser_path: None,
        message: "JavaScript rendering is not included in this build.".into(),
    }
}

pub fn validate_rendering(config: &JsRenderingConfig) -> Result<()> {
    if config.enabled {
        let status = rendering_status();
        anyhow::ensure!(
            status.available,
            "{} Turn off Render DOM in Settings to crawl HTML.",
            status.message
        );
    }
    Ok(())
}

#[cfg(feature = "js-rendering")]
fn browser_executable() -> Result<std::path::PathBuf> {
    let path = match std::env::var_os("CHROME").filter(|path| !path.is_empty()) {
        Some(path) => std::path::PathBuf::from(path),
        None => chromiumoxide::detection::default_executable(Default::default())
            .map_err(anyhow::Error::msg)?,
    };
    checked_browser_executable(path)
}

#[cfg(feature = "js-rendering")]
fn checked_browser_executable(path: std::path::PathBuf) -> Result<std::path::PathBuf> {
    use anyhow::Context;
    let metadata = path
        .metadata()
        .with_context(|| format!("cannot read browser at {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file(),
        "browser path is not a file: {}",
        path.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            metadata.permissions().mode() & 0o111 != 0,
            "browser file is not executable: {}",
            path.display()
        );
    }
    Ok(std::path::absolute(path)?)
}

pub(super) async fn render_page_if_enabled(
    config: &CrawlConfig,
    url: &Url,
    client: &reqwest::Client,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
) -> Result<Option<RenderedPage>> {
    if !config.rendering.enabled {
        return Ok(None);
    }

    #[cfg(feature = "js-rendering")]
    {
        render_with_pause_restart(control, || {
            render_with_chrome_cdp(config, url, client, request_policy, control)
        })
        .await
        .map(Some)
    }
    #[cfg(not(feature = "js-rendering"))]
    {
        let _ = (client, request_policy, control);
        anyhow::bail!(
            "JavaScript rendering for {url} requires the crawler-core js-rendering feature"
        )
    }
}

#[cfg(any(feature = "js-rendering", test))]
async fn render_with_pause_restart<T, F, Fut>(control: &CrawlControl, mut render: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    loop {
        crate::wait_if_paused(control).await;
        crate::ensure_not_cancelled(control)?;
        // Chrome command deadlines use wall time. Restart the unfinished render
        // after a pause instead of publishing a timeout or partially rendered DOM.
        tokio::select! {
            biased;
            _ = crate::wait_until_cancelled(control) => anyhow::bail!(crate::CRAWL_CANCELLED_MESSAGE),
            _ = async {
                while !control.is_paused() {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            } => {},
            result = render() => {
                if !control.is_paused() {
                    return result;
                }
            },
        }
    }
}

fn default_wait_after_load_ms() -> u64 {
    500
}

#[cfg(feature = "js-rendering")]
async fn render_with_chrome_cdp(
    config: &CrawlConfig,
    url: &Url,
    client: &reqwest::Client,
    request_policy: &RequestPolicy,
    control: &CrawlControl,
) -> Result<RenderedPage> {
    use anyhow::Context;
    use chromiumoxide::cdp::browser_protocol::{
        fetch, network::ErrorReason, target::SetAutoAttachParams,
    };
    use chromiumoxide::{Browser, BrowserConfig};
    use futures::StreamExt;
    use std::time::{Duration, Instant};

    let started_at = Instant::now();
    let timeout_duration = Duration::from_secs(config.timeout_secs.max(1));
    let mut request_headers = request_policy.request_headers.clone();
    // Chromium supplies resource-specific Accept/Upgrade defaults. Language remains an explicit
    // origin-restricted override so the configured locale does not depend on the installation.
    for (name, value) in crate::DEFAULT_REQUEST_HEADERS {
        if matches!(name, "Accept" | "Upgrade-Insecure-Requests")
            && request_headers
                .get(name)
                .is_some_and(|configured| configured.as_bytes() == value.as_bytes())
        {
            request_headers.remove(name);
        }
    }
    let profile = tempfile::Builder::new()
        .prefix("ferrous-frog-render-")
        .tempdir()?;
    let browser_config = BrowserConfig::builder()
        .chrome_executable(browser_executable()?)
        .user_data_dir(profile.path())
        .new_headless_mode()
        .respect_https_errors()
        .launch_timeout(timeout_duration)
        .request_timeout(timeout_duration)
        .arg(("user-agent", config.user_agent.as_str()))
        .build()
        .map_err(|error| anyhow::anyhow!(error))?;
    let (mut browser, mut handler) = Browser::launch(browser_config)
        .await
        .context("failed to launch Chrome for JavaScript rendering")?;
    // JoinSet aborts the handler when this worker is cancelled or returns early.
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    let render_result = tokio::time::timeout(timeout_duration, async {
        // Browser-level interception includes child frames and worker requests.
        // Enable it before creating any page so the first navigation cannot escape.
        let mut requests = browser
            .event_listener::<fetch::EventRequestPaused>()
            .await?;
        browser.execute(fetch::EnableParams::default()).await?;
        let intercept = async {
            while let Some(request) = requests.next().await {
                let allowed = async {
                    let target = Url::parse(&request.request.url)?;
                    anyhow::ensure!(
                        matches!(target.scheme(), "http" | "https"),
                        "Unsupported rendered request scheme"
                    );
                    let delay = request_policy
                        .robots_delay(client, config, &target, control)
                        .await?;
                    request_policy.wait(&target, delay, control).await
                }
                .await;
                if let Err(error) = allowed {
                    tracing::debug!(url = %request.request.url, %error, "Blocked rendered request");
                    browser
                        .execute(fetch::FailRequestParams::new(
                            request.request_id.clone(),
                            ErrorReason::BlockedByClient,
                        ))
                        .await?;
                } else {
                    let mut params = fetch::ContinueRequestParams::new(request.request_id.clone());
                    if !request_headers.is_empty() {
                        let target = Url::parse(&request.request.url)?;
                        let mut headers = request
                            .request
                            .headers
                            .inner()
                            .as_object()
                            .context("invalid rendered request headers")?
                            .iter()
                            .filter(|(name, _)| !request_headers.contains_key(name.as_str()))
                            .filter_map(|(name, value)| {
                                value
                                    .as_str()
                                    .map(|value| fetch::HeaderEntry::new(name, value))
                            })
                            .collect::<Vec<_>>();
                        if target.origin() == request_policy.header_origin {
                            headers.extend(request_headers.iter().map(|(name, value)| {
                                fetch::HeaderEntry::new(
                                    name.as_str(),
                                    value.to_str().expect("validated ASCII request header"),
                                )
                            }));
                        }
                        // Recompute on every intercepted hop; custom headers cannot follow redirects off-origin.
                        params.headers = Some(headers);
                    }
                    browser.execute(params).await?;
                }
            }
            anyhow::bail!("Chrome request interception disconnected")
        };
        let render = async {
            let page = browser.new_page("about:blank").await?;
            // Requests are intercepted at the browser, so child workers do not need
            // debugger attachment (which can leave service workers suspended).
            page.execute(SetAutoAttachParams::new(false, false)).await?;
            page.goto(url.as_str())
                .await
                .with_context(|| format!("rendered page navigation failed: {url}"))?;
            tokio::time::sleep(Duration::from_millis(config.rendering.wait_after_load_ms)).await;
            let html = page
                .content()
                .await
                .context("failed to read rendered DOM")?;
            Ok::<_, anyhow::Error>(html)
        };
        tokio::select! {
            result = intercept => result,
            result = render => result,
        }
    })
    .await;

    // Bound graceful shutdown; Browser's kill-on-drop handles an unresponsive process.
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        let _ = browser.close().await;
        let _ = browser.wait().await;
    })
    .await;

    let html =
        render_result.with_context(|| format!("JavaScript rendering timed out: {url}"))??;

    Ok(RenderedPage {
        html,
        elapsed_ms: started_at.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn paused_render_work_is_discarded_before_resume_or_stop() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        for (stop, finish_immediately) in [(false, false), (true, false), (false, true)] {
            let control = CrawlControl::default();
            let worker_control = control.clone();
            let started = Arc::new(tokio::sync::Notify::new());
            let worker_started = started.clone();
            let attempts = Arc::new(AtomicUsize::new(0));
            let worker_attempts = attempts.clone();
            let task = tokio::spawn(async move {
                render_with_pause_restart(&worker_control, || async {
                    let attempt = worker_attempts.fetch_add(1, Ordering::SeqCst);
                    if attempt == 0 {
                        worker_control.pause();
                        worker_started.notify_one();
                        if finish_immediately {
                            anyhow::bail!("Render failed as pause was requested");
                        }
                        tokio::time::timeout(Duration::from_secs(1), std::future::pending::<()>())
                            .await?;
                    }
                    Ok(attempt)
                })
                .await
            });
            started.notified().await;
            // Advance beyond the render deadline without depending on Chrome or wall time.
            tokio::time::advance(Duration::from_secs(5)).await;
            tokio::task::yield_now().await;
            assert!(
                !task.is_finished(),
                "Paused work must not publish an expired result"
            );
            assert_eq!(attempts.load(Ordering::SeqCst), 1);
            if stop {
                control.cancel();
                assert!(task.await.unwrap().is_err());
                assert_eq!(attempts.load(Ordering::SeqCst), 1);
            } else {
                control.resume();
                assert_eq!(task.await.unwrap().unwrap(), 1);
                assert_eq!(attempts.load(Ordering::SeqCst), 2);
            }
        }
    }

    #[tokio::test]
    async fn rendering_disabled_returns_none() {
        assert!(validate_rendering(&JsRenderingConfig::default()).is_ok());
        let url = Url::parse("https://example.com/").unwrap();
        let policy = RequestPolicy {
            origins: Default::default(),
            rate_limiter: None,
            respect_robots: true,
            header_origin: url.origin(),
            request_headers: Default::default(),
            on_event: std::sync::Arc::new(|_| {}),
        };
        let rendered = render_page_if_enabled(
            &CrawlConfig::default(),
            &url,
            &reqwest::Client::new(),
            &policy,
            &CrawlControl::default(),
        )
        .await
        .unwrap();

        assert_eq!(rendered, None);
    }

    #[cfg(not(feature = "js-rendering"))]
    #[test]
    fn standard_build_reports_rendering_unavailable() {
        let status = rendering_status();
        assert!(!status.available);
        assert!(status.browser_path.is_none());
        assert!(status.message.contains("not included"));
    }

    #[cfg(feature = "js-rendering")]
    #[test]
    fn browser_detection_rejects_directories_missing_paths_and_non_executable_files() {
        let executable = std::env::current_exe().unwrap();
        assert_eq!(
            checked_browser_executable(executable.clone()).unwrap(),
            executable
        );
        assert!(checked_browser_executable(std::env::temp_dir()).is_err());
        assert!(checked_browser_executable(executable.join("missing-browser")).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = std::env::temp_dir()
                .join(format!("ferrous-frog-browser-test-{}", std::process::id()));
            std::fs::write(&path, "not executable").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let result = checked_browser_executable(path.clone());
            std::fs::remove_file(path).unwrap();
            assert!(result.unwrap_err().to_string().contains("executable"));
        }
    }
}
