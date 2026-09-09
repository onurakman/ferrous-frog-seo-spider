use ferrous_frog_extractors::{CustomExtractor, ExtractionPreview, preview_extractor};
use ferrous_frog_parser::{ContentConfig, ContentSelectors, preview_content};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct PreviewRequest {
    html: String,
    content: ContentConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResponse {
    text: String,
    word_count: usize,
    text_to_code_ratio: f64,
    text_truncated: bool,
}

fn preview(request: PreviewRequest) -> Result<PreviewResponse, String> {
    if request.html.len() > 512 * 1024 {
        return Err("Preview HTML exceeds the 512 KiB limit.".into());
    }
    let selectors = ContentSelectors::compile(&request.content)?;
    let result = preview_content(&request.html, &selectors);
    let text_truncated = result.text.chars().count() > 8_000;
    Ok(PreviewResponse {
        text: result.text.chars().take(8_000).collect(),
        word_count: result.word_count,
        text_to_code_ratio: result.text_to_code_ratio,
        text_truncated,
    })
}

#[tauri::command]
pub async fn preview_content_area(request: PreviewRequest) -> Result<PreviewResponse, String> {
    tauri::async_runtime::spawn_blocking(move || preview(request))
        .await
        .map_err(|error| format!("failed to preview content: {error}"))?
}

#[derive(Deserialize)]
pub struct ExtractorPreviewRequest {
    html: String,
    extractor: CustomExtractor,
}

#[tauri::command]
pub async fn preview_custom_extractor(
    request: ExtractorPreviewRequest,
) -> Result<ExtractionPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        preview_extractor(&request.html, &request.extractor).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("failed to preview extractor: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn extractor_preview_command_preserves_the_rule_contract_and_reports_truncation() {
        let request = serde_json::from_value(serde_json::json!({
            "html": format!("<html>{}</html>", "<p>café</p>".repeat(101)),
            "extractor": {
                "name": "Heading",
                "kind": "cssText",
                "pattern": "p",
                "attribute": null,
                "allMatches": true
            }
        }))
        .unwrap();
        let response = preview_custom_extractor(request).await.unwrap();
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["values"].as_array().unwrap().len(), 100);
        assert_eq!(json["values"][0], "café");
        assert_eq!(json["valuesTruncated"], true);
        assert_eq!(json["textTruncated"], false);

        let request = serde_json::from_value(serde_json::json!({
            "html": "界".repeat(2_001),
            "extractor": {
                "name": "First",
                "kind": "regex",
                "pattern": ".+",
                "attribute": null,
                "allMatches": false
            }
        }))
        .unwrap();
        let response = preview_custom_extractor(request).await.unwrap();
        assert_eq!(response.values, ["界".repeat(2_000)]);
        assert!(response.text_truncated);
        assert!(!response.values_truncated);
    }

    #[tokio::test]
    async fn extractor_preview_command_returns_errors_separately_from_valid_no_match() {
        for (html, pattern, expected_error) in [
            ("<html/>".into(), "missing", None),
            ("<html/>".into(), "(", Some("invalid regex")),
            ("x".repeat(512 * 1024 + 1), "missing", Some("512 KiB")),
        ] {
            let request = serde_json::from_value(serde_json::json!({
                "html": html,
                "extractor": {
                    "name": "Sample",
                    "kind": "regex",
                    "pattern": pattern,
                    "attribute": null,
                    "allMatches": true
                }
            }))
            .unwrap();
            let response = preview_custom_extractor(request).await;
            if let Some(message) = expected_error {
                assert!(response.unwrap_err().contains(message));
            } else {
                assert!(response.unwrap().values.is_empty());
            }
        }
    }

    #[test]
    fn preview_uses_crawl_selectors_and_bounds_text_without_truncating_statistics() {
        let content = ContentConfig {
            include_selectors: vec!["main".into()],
            exclude_selectors: vec!["aside".into()],
        };
        let result = preview(PreviewRequest {
            html: "<nav>Navigation</nav><main>Hello world<aside>Related links</aside></main>"
                .into(),
            content: content.clone(),
        })
        .unwrap();
        assert_eq!(result.text, "Hello world");
        assert_eq!(result.word_count, 2);
        assert!(!result.text_truncated);
        let result = preview(PreviewRequest {
            html: format!("<main>{}</main>", "café ".repeat(3_000)),
            content,
        })
        .unwrap();
        assert_eq!(result.word_count, 3_000);
        assert_eq!(result.text.chars().count(), 8_000);
        assert!(result.text_truncated);
        assert!(
            preview(PreviewRequest {
                html: "x".repeat(512 * 1024 + 1),
                content: ContentConfig::default()
            })
            .unwrap_err()
            .contains("512 KiB")
        );
    }
}
