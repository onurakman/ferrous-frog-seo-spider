use regex::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use sxd_document::parser;
use sxd_xpath::{Context, Factory, Value};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExtractorKind {
    #[serde(rename = "cssText")]
    CssText,
    #[serde(rename = "cssAttribute")]
    CssAttribute,
    #[serde(rename = "xpath")]
    XPath,
    #[serde(rename = "regex")]
    Regex,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomExtractor {
    pub name: String,
    pub kind: ExtractorKind,
    pub pattern: String,
    pub attribute: Option<String>,
    pub all_matches: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionResult {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("invalid CSS selector for extractor '{name}': {message}")]
    Css { name: String, message: String },
    #[error("invalid regex for extractor '{name}': {source}")]
    Regex { name: String, source: regex::Error },
    #[error("invalid XPath for extractor '{name}': {message}")]
    XPath { name: String, message: String },
    #[error("invalid XML/HTML document for XPath extractor '{name}': {message}")]
    XPathDocument { name: String, message: String },
}

pub fn run_extractors(
    html: &str,
    extractors: &[CustomExtractor],
) -> Result<Vec<ExtractionResult>, ExtractionError> {
    let document = Html::parse_document(html);
    let mut results = Vec::with_capacity(extractors.len());

    for extractor in extractors {
        let values = match extractor.kind {
            ExtractorKind::CssText => css_text_values(&document, extractor)?,
            ExtractorKind::CssAttribute => css_attribute_values(&document, extractor)?,
            ExtractorKind::XPath => xpath_values(html, extractor)?,
            ExtractorKind::Regex => regex_values(html, extractor)?,
        };
        results.push(ExtractionResult {
            name: extractor.name.clone(),
            values,
        });
    }

    Ok(results)
}

fn css_text_values(
    document: &Html,
    extractor: &CustomExtractor,
) -> Result<Vec<String>, ExtractionError> {
    let selector = parse_selector(extractor)?;
    let values = document
        .select(&selector)
        .map(|node| normalize_whitespace(&node.text().collect::<Vec<_>>().join(" ")))
        .filter(|value| !value.is_empty());
    Ok(limit_matches(values, extractor.all_matches))
}

fn css_attribute_values(
    document: &Html,
    extractor: &CustomExtractor,
) -> Result<Vec<String>, ExtractionError> {
    let selector = parse_selector(extractor)?;
    let attribute = extractor.attribute.as_deref().unwrap_or_default();
    let values = document
        .select(&selector)
        .filter_map(|node| node.value().attr(attribute))
        .map(normalize_whitespace)
        .filter(|value| !value.is_empty());
    Ok(limit_matches(values, extractor.all_matches))
}

fn regex_values(html: &str, extractor: &CustomExtractor) -> Result<Vec<String>, ExtractionError> {
    let regex = Regex::new(&extractor.pattern).map_err(|source| ExtractionError::Regex {
        name: extractor.name.clone(),
        source,
    })?;
    let values = regex.captures_iter(html).filter_map(|captures| {
        captures
            .get(1)
            .or_else(|| captures.get(0))
            .map(|match_value| match_value.as_str().to_string())
    });
    Ok(limit_matches(values, extractor.all_matches))
}

fn xpath_values(html: &str, extractor: &CustomExtractor) -> Result<Vec<String>, ExtractionError> {
    let package = parser::parse(html).map_err(|error| ExtractionError::XPathDocument {
        name: extractor.name.clone(),
        message: error.to_string(),
    })?;
    let document = package.as_document();
    let factory = Factory::new();
    let xpath = factory
        .build(&extractor.pattern)
        .map_err(|error| ExtractionError::XPath {
            name: extractor.name.clone(),
            message: error.to_string(),
        })?
        .ok_or_else(|| ExtractionError::XPath {
            name: extractor.name.clone(),
            message: "empty XPath expression".to_string(),
        })?;
    let context = Context::new();
    let value =
        xpath
            .evaluate(&context, document.root())
            .map_err(|error| ExtractionError::XPath {
                name: extractor.name.clone(),
                message: error.to_string(),
            })?;
    let values = match value {
        Value::Nodeset(nodeset) => nodeset
            .document_order()
            .iter()
            .map(|node| normalize_whitespace(node.string_value()))
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>(),
        Value::String(value) => vec![normalize_whitespace(value)],
        Value::Number(value) => vec![value.to_string()],
        Value::Boolean(value) => vec![value.to_string()],
    };

    Ok(if extractor.all_matches {
        values
    } else {
        values.into_iter().take(1).collect()
    })
}

fn parse_selector(extractor: &CustomExtractor) -> Result<Selector, ExtractionError> {
    Selector::parse(&extractor.pattern).map_err(|error| ExtractionError::Css {
        name: extractor.name.clone(),
        message: error.to_string(),
    })
}

fn limit_matches(values: impl Iterator<Item = String>, all_matches: bool) -> Vec<String> {
    if all_matches {
        values.collect()
    } else {
        values.take(1).collect()
    }
}

fn normalize_whitespace(value: impl AsRef<str>) -> String {
    value
        .as_ref()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_css_text() {
        let html = "<main><h1> Example   Heading </h1><h1>Second</h1></main>";
        let extractors = [CustomExtractor {
            name: "heading".to_string(),
            kind: ExtractorKind::CssText,
            pattern: "h1".to_string(),
            attribute: None,
            all_matches: false,
        }];

        let results = run_extractors(html, &extractors).unwrap();

        assert_eq!(results[0].values, ["Example Heading"]);
    }

    #[test]
    fn extracts_css_attribute_values() {
        let html = r#"<a href="/a">A</a><a href="/b">B</a>"#;
        let extractors = [CustomExtractor {
            name: "links".to_string(),
            kind: ExtractorKind::CssAttribute,
            pattern: "a[href]".to_string(),
            attribute: Some("href".to_string()),
            all_matches: true,
        }];

        let results = run_extractors(html, &extractors).unwrap();

        assert_eq!(results[0].values, ["/a", "/b"]);
    }

    #[test]
    fn extracts_regex_capture_group() {
        let html = r#"<script>window.analyticsId = "abc-123";</script>"#;
        let extractors = [CustomExtractor {
            name: "analytics".to_string(),
            kind: ExtractorKind::Regex,
            pattern: r#"analyticsId = "([^"]+)""#.to_string(),
            attribute: None,
            all_matches: false,
        }];

        let results = run_extractors(html, &extractors).unwrap();

        assert_eq!(results[0].values, ["abc-123"]);
    }

    #[test]
    fn extracts_xpath_values_from_xml_compatible_html() {
        let html = r#"<html><body><main><h1>XPath Heading</h1></main></body></html>"#;
        let extractors = [CustomExtractor {
            name: "xpath_heading".to_string(),
            kind: ExtractorKind::XPath,
            pattern: "//h1/text()".to_string(),
            attribute: None,
            all_matches: false,
        }];

        let results = run_extractors(html, &extractors).unwrap();

        assert_eq!(results[0].values, ["XPath Heading"]);
    }
}
