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

#[derive(Clone, Debug)]
pub struct RenderRequest {
    pub url: Url,
    pub wait_after_load_ms: u64,
    pub timeout_secs: u64,
}

pub async fn render_page_if_enabled(
    config: &JsRenderingConfig,
    url: &Url,
    timeout_secs: u64,
) -> Result<Option<RenderedPage>> {
    if !config.enabled {
        return Ok(None);
    }

    let request = RenderRequest {
        url: url.clone(),
        wait_after_load_ms: config.wait_after_load_ms,
        timeout_secs,
    };

    match config.backend {
        JsRenderingBackend::ChromeCdp => render_with_chrome_cdp(request).await.map(Some),
    }
}

fn default_wait_after_load_ms() -> u64 {
    500
}

#[cfg(feature = "js-rendering")]
async fn render_with_chrome_cdp(request: RenderRequest) -> Result<RenderedPage> {
    use anyhow::Context;
    use chromiumoxide::{Browser, BrowserConfig};
    use futures::StreamExt;
    use std::time::{Duration, Instant};

    let started_at = Instant::now();
    let browser_config = BrowserConfig::builder()
        .build()
        .map_err(|error| anyhow::anyhow!(error))?;
    let (mut browser, mut handler) = Browser::launch(browser_config)
        .await
        .context("failed to launch Chrome for JavaScript rendering")?;
    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    let timeout_duration = Duration::from_secs(request.timeout_secs.max(1));
    let render_result = tokio::time::timeout(timeout_duration, async {
        let page = browser
            .new_page(request.url.as_str())
            .await
            .with_context(|| format!("failed to open rendered page: {}", request.url))?;
        page.wait_for_navigation()
            .await
            .with_context(|| format!("rendered page navigation failed: {}", request.url))?;
        if request.wait_after_load_ms > 0 {
            tokio::time::sleep(Duration::from_millis(request.wait_after_load_ms)).await;
        }
        let html = page
            .content()
            .await
            .with_context(|| format!("failed to read rendered DOM: {}", request.url))?;
        let _ = page.close().await;
        Ok::<_, anyhow::Error>(html)
    })
    .await;

    let _ = browser.close().await;
    let _ = browser.wait().await;
    handler_task.abort();

    let html = render_result
        .with_context(|| format!("JavaScript rendering timed out: {}", request.url))??;

    Ok(RenderedPage {
        html,
        elapsed_ms: started_at.elapsed().as_millis() as u64,
    })
}

#[cfg(not(feature = "js-rendering"))]
async fn render_with_chrome_cdp(request: RenderRequest) -> Result<RenderedPage> {
    let _ = (request.wait_after_load_ms, request.timeout_secs);
    anyhow::bail!(
        "JavaScript rendering for {} requires the crawler-core js-rendering feature",
        request.url
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rendering_disabled_returns_none() {
        let url = Url::parse("https://example.com/").unwrap();
        let rendered = render_page_if_enabled(&JsRenderingConfig::default(), &url, 5)
            .await
            .unwrap();

        assert_eq!(rendered, None);
    }
}
