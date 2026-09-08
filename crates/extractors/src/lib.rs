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
pub struct CustomSearch {
    pub name: String,
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub max_snippets: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionResult {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub name: String,
    pub matched: bool,
    pub match_count: usize,
    pub snippets: Vec<String>,
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
    #[error("invalid regex for search '{name}': {source}")]
    SearchRegex { name: String, source: regex::Error },
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

pub fn run_searches(
    html: &str,
    searches: &[CustomSearch],
) -> Result<Vec<SearchResult>, ExtractionError> {
    searches
        .iter()
        .map(|search| run_search(html, search))
        .collect()
}

fn run_search(html: &str, search: &CustomSearch) -> Result<SearchResult, ExtractionError> {
    let max_snippets = search.max_snippets.min(20);
    let snippets_and_count = if search.regex {
        regex_search_values(html, search, max_snippets)?
    } else {
        text_search_values(html, search, max_snippets)?
    };

    Ok(SearchResult {
        name: search.name.clone(),
        matched: snippets_and_count.0 > 0,
        match_count: snippets_and_count.0,
        snippets: snippets_and_count.1,
    })
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

fn regex_search_values(
    html: &str,
    search: &CustomSearch,
    max_snippets: usize,
) -> Result<(usize, Vec<String>), ExtractionError> {
    let pattern = if search.case_sensitive {
        search.pattern.clone()
    } else {
        format!("(?i:{})", search.pattern)
    };
    let regex = Regex::new(&pattern).map_err(|source| ExtractionError::SearchRegex {
        name: search.name.clone(),
        source,
    })?;
    let mut count = 0usize;
    let mut snippets = Vec::new();
    for match_value in regex.find_iter(html) {
        count = count.saturating_add(1);
        if snippets.len() < max_snippets {
            snippets.push(snippet(html, match_value.start(), match_value.end()));
        }
    }
    Ok((count, snippets))
}

fn text_search_values(
    html: &str,
    search: &CustomSearch,
    max_snippets: usize,
) -> Result<(usize, Vec<String>), ExtractionError> {
    if search.pattern.is_empty() {
        return Ok((0, Vec::new()));
    }

    let escaped = regex::escape(&search.pattern);
    let pattern = if search.case_sensitive {
        escaped
    } else {
        format!("(?i:{escaped})")
    };
    let regex = Regex::new(&pattern).map_err(|source| ExtractionError::SearchRegex {
        name: search.name.clone(),
        source,
    })?;
    let mut count = 0usize;
    let mut snippets = Vec::new();
    for match_value in regex.find_iter(html) {
        count = count.saturating_add(1);
        if snippets.len() < max_snippets {
            snippets.push(snippet(html, match_value.start(), match_value.end()));
        }
    }

    Ok((count, snippets))
}

fn snippet(html: &str, start: usize, end: usize) -> String {
    let context = 48usize;
    let safe_start = html
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= start)
        .last()
        .unwrap_or(0);
    let safe_end = html
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= end)
        .unwrap_or(html.len());
    let prefix_start = html[..safe_start]
        .char_indices()
        .rev()
        .nth(context)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let suffix_end = html[safe_end..]
        .char_indices()
        .nth(context)
        .map(|(index, _)| safe_end + index)
        .unwrap_or(html.len());
    normalize_whitespace(&html[prefix_start..suffix_end])
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

    #[test]
    fn searches_text_in_raw_html() {
        let html = r#"<script>window.analyticsId = "abc";</script><p>Analytics ready</p>"#;
        let searches = [CustomSearch {
            name: "analytics".to_string(),
            pattern: "analytics".to_string(),
            regex: false,
            case_sensitive: false,
            max_snippets: 2,
        }];

        let results = run_searches(html, &searches).unwrap();

        assert!(results[0].matched);
        assert_eq!(results[0].match_count, 2);
        assert_eq!(results[0].snippets.len(), 2);
    }

    #[test]
    fn searches_regex_in_raw_html() {
        let html = r#"<script>window.analyticsId = "abc-123";</script>"#;
        let searches = [CustomSearch {
            name: "analytics_id".to_string(),
            pattern: r#"analyticsId\s*="#.to_string(),
            regex: true,
            case_sensitive: true,
            max_snippets: 1,
        }];

        let results = run_searches(html, &searches).unwrap();

        assert_eq!(results[0].match_count, 1);
        assert!(results[0].snippets[0].contains("analyticsId"));
    }
}
