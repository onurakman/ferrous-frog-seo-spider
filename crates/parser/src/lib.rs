use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use url::Url;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSignals {
    pub title: Option<String>,
    pub title_len: usize,
    pub title_pixel_width: u32,
    pub meta_description: Option<String>,
    pub meta_description_len: usize,
    pub meta_description_pixel_width: u32,
    pub meta_robots: Option<String>,
    pub h1: Option<String>,
    pub h1_len: usize,
    pub h1_count: usize,
    pub h2: Option<String>,
    pub h2_len: usize,
    pub h2_count: usize,
    pub canonical: Option<String>,
    pub canonical_count: usize,
    pub indexability: String,
    pub indexability_status: String,
    pub visible_text: String,
    pub word_count: usize,
    pub text_to_code_ratio: f64,
    pub image_count: u32,
    pub images_missing_alt: u32,
    pub images_alt_too_long: u32,
    pub images: Vec<PageImage>,
    pub mixed_content_count: u32,
    pub insecure_form_count: u32,
    pub viewport: bool,
    pub amphtml: Option<String>,
    pub rel_next: Option<String>,
    pub rel_prev: Option<String>,
    pub hreflang_count: u32,
    pub hreflang_invalid_count: u32,
    pub hreflang_missing_self_reference: bool,
    pub hreflang_links: Vec<HreflangLink>,
    pub json_ld_count: u32,
    pub json_ld_invalid_count: u32,
    pub structured_data_error_count: u32,
    pub structured_data_warning_count: u32,
    pub structured_data_issues: Vec<StructuredDataIssue>,
    pub open_graph_count: u32,
    pub twitter_card_count: u32,
    pub deprecated_html_tag_count: u32,
    pub duplicate_id_count: u32,
    pub links: Vec<PageLink>,
    pub resources: Vec<PageResource>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLink {
    pub url: String,
    pub text: String,
    pub rel: String,
    pub rel_nofollow: bool,
    pub source_position: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageResourceType {
    Image,
    Css,
    JavaScript,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResource {
    pub url: String,
    pub label: String,
    pub resource_type: PageResourceType,
    pub source_position: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageImage {
    pub url: String,
    pub alt_text: Option<String>,
    pub alt_len: u32,
    pub missing_alt: bool,
    pub alt_too_long: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source_position: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HreflangLink {
    pub hreflang: String,
    pub url: String,
    pub valid: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StructuredDataIssue {
    pub severity: String,
    pub message: String,
    pub path: String,
}

const IMAGE_ALT_MAX: usize = 125;
const DEPRECATED_HTML_TAGS: &[&str] = &[
    "acronym",
    "applet",
    "basefont",
    "big",
    "blink",
    "center",
    "dir",
    "font",
    "frame",
    "frameset",
    "isindex",
    "listing",
    "marquee",
    "noframes",
    "plaintext",
    "strike",
    "tt",
    "xmp",
];

pub fn parse_html(base_url: &Url, html: &str) -> PageSignals {
    let document = Html::parse_document(html);
    let title = first_text(&document, "title");
    let meta_description = meta_content(&document, "description");
    let meta_robots = meta_content(&document, "robots");
    let h1 = first_text(&document, "h1");
    let h1_count = element_count(&document, "h1");
    let h2 = first_text(&document, "h2");
    let h2_count = element_count(&document, "h2");
    let canonical = canonical_href(&document, base_url);
    let canonical_count = canonical_count(&document);
    let amphtml = link_href_by_rel(&document, base_url, "amphtml");
    let rel_next = link_href_by_rel(&document, base_url, "next");
    let rel_prev = link_href_by_rel(&document, base_url, "prev");
    let hreflang_stats = hreflang_stats(&document, base_url);
    let json_ld_stats = json_ld_stats(&document);
    let visible_text = visible_text(&document);
    let word_count = visible_text.split_whitespace().count();
    let text_to_code_ratio = if html.is_empty() {
        0.0
    } else {
        visible_text.len() as f64 / html.len() as f64
    };
    let links = extract_links(&document, base_url);
    let images = extract_images(&document, base_url);
    let resources = extract_resources(&document, base_url);
    let title_len = title
        .as_deref()
        .map(str::chars)
        .map(Iterator::count)
        .unwrap_or(0);
    let meta_description_len = meta_description
        .as_deref()
        .map(str::chars)
        .map(Iterator::count)
        .unwrap_or(0);
    let title_pixel_width = title
        .as_deref()
        .map(|value| estimate_text_pixel_width(value, 18.0))
        .unwrap_or(0);
    let meta_description_pixel_width = meta_description
        .as_deref()
        .map(|value| estimate_text_pixel_width(value, 13.0))
        .unwrap_or(0);
    let h1_len = h1
        .as_deref()
        .map(str::chars)
        .map(Iterator::count)
        .unwrap_or(0);
    let h2_len = h2
        .as_deref()
        .map(str::chars)
        .map(Iterator::count)
        .unwrap_or(0);
    let has_noindex = contains_robots_directive(meta_robots.as_deref(), "noindex");
    let image_stats = image_stats(&images);

    PageSignals {
        title,
        title_len,
        title_pixel_width,
        meta_description,
        meta_description_len,
        meta_description_pixel_width,
        meta_robots,
        h1,
        h1_len,
        h1_count,
        h2,
        h2_len,
        h2_count,
        canonical,
        canonical_count,
        indexability: if has_noindex {
            "Non-indexable".to_string()
        } else {
            "Indexable".to_string()
        },
        indexability_status: if has_noindex {
            "Meta robots noindex".to_string()
        } else {
            "Indexable".to_string()
        },
        visible_text,
        word_count,
        text_to_code_ratio,
        image_count: image_stats.image_count,
        images_missing_alt: image_stats.images_missing_alt,
        images_alt_too_long: image_stats.images_alt_too_long,
        images,
        mixed_content_count: mixed_content_count(&document, base_url),
        insecure_form_count: insecure_form_count(&document, base_url),
        viewport: meta_content(&document, "viewport").is_some(),
        amphtml,
        rel_next,
        rel_prev,
        hreflang_count: hreflang_stats.count,
        hreflang_invalid_count: hreflang_stats.invalid_count,
        hreflang_missing_self_reference: hreflang_stats.missing_self_reference,
        hreflang_links: hreflang_stats.links,
        json_ld_count: json_ld_stats.count,
        json_ld_invalid_count: json_ld_stats.invalid_count,
        structured_data_error_count: json_ld_stats.error_count,
        structured_data_warning_count: json_ld_stats.warning_count,
        structured_data_issues: json_ld_stats.issues,
        open_graph_count: meta_prefix_count(&document, "og:"),
        twitter_card_count: meta_prefix_count(&document, "twitter:"),
        deprecated_html_tag_count: deprecated_html_tag_count(&document),
        duplicate_id_count: duplicate_id_count(&document),
        links,
        resources,
    }
}

pub fn normalize_url(base_url: &Url, href: &str) -> Option<Url> {
    let href = href.trim();
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with("javascript:")
    {
        return None;
    }

    let mut url = base_url.join(href).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }

    url.set_fragment(None);
    if url.path().is_empty() {
        url.set_path("/");
    }
    Some(url)
}

pub fn same_host(left: &Url, right: &Url) -> bool {
    left.domain() == right.domain()
}

pub fn contains_robots_directive(value: Option<&str>, directive: &str) -> bool {
    value
        .unwrap_or_default()
        .split([',', ';'])
        .flat_map(|part| {
            let mut skip_parameter_value = false;
            part.split_whitespace().filter(move |token| {
                let is_parameter_value = skip_parameter_value;
                skip_parameter_value = token.ends_with(':');
                !is_parameter_value && !token.contains(':')
            })
        })
        .any(|part| {
            part.eq_ignore_ascii_case(directive)
                || (part.eq_ignore_ascii_case("none")
                    && matches!(directive, "noindex" | "nofollow"))
        })
}

fn estimate_text_pixel_width(text: &str, font_size_px: f32) -> u32 {
    text.chars()
        .map(estimated_glyph_width)
        .map(|width| width * font_size_px)
        .sum::<f32>()
        .round()
        .max(0.0) as u32
}

fn estimated_glyph_width(character: char) -> f32 {
    match character {
        ' ' | '\t' => 0.28,
        'i' | 'j' | 'l' | 'I' | '!' | '|' | '\'' | ',' | '.' | ':' | ';' => 0.26,
        'f' | 'r' | 't' | '(' | ')' | '[' | ']' | '{' | '}' => 0.34,
        'm' | 'w' | 'M' | 'W' | '@' | '%' | '&' => 0.82,
        'A'..='Z' => 0.64,
        '0'..='9' => 0.56,
        '-' | '_' | '/' | '\\' => 0.42,
        character if character.is_ascii_punctuation() => 0.5,
        character if character.is_ascii_lowercase() => 0.52,
        character if character.is_ascii() => 0.56,
        _ => 1.0,
    }
}

fn selector(selector: &str) -> Selector {
    Selector::parse(selector).expect("static selector must parse")
}

fn first_text(document: &Html, selector_value: &str) -> Option<String> {
    let selector = selector(selector_value);
    document
        .select(&selector)
        .next()
        .map(|node| normalize_whitespace(&node.text().collect::<Vec<_>>().join(" ")))
        .filter(|value| !value.is_empty())
}

fn element_count(document: &Html, selector_value: &str) -> usize {
    let selector = selector(selector_value);
    document.select(&selector).count()
}

fn deprecated_html_tag_count(document: &Html) -> u32 {
    DEPRECATED_HTML_TAGS
        .iter()
        .map(|tag| element_count(document, tag))
        .sum::<usize>()
        .min(u32::MAX as usize) as u32
}

fn duplicate_id_count(document: &Html) -> u32 {
    let selector = selector("[id]");
    let mut counts = HashMap::<String, usize>::new();
    for node in document.select(&selector) {
        let Some(id) = node.value().attr("id").map(str::trim) else {
            continue;
        };
        if !id.is_empty() {
            *counts.entry(id.to_string()).or_default() += 1;
        }
    }
    counts
        .values()
        .map(|count| count.saturating_sub(1))
        .sum::<usize>()
        .min(u32::MAX as usize) as u32
}

fn meta_content(document: &Html, name: &str) -> Option<String> {
    let selector = selector("meta[name], meta[property]");
    let mut contents = document.select(&selector).filter_map(|node| {
        let value = node
            .value()
            .attr("name")
            .or_else(|| node.value().attr("property"))?;
        if value.eq_ignore_ascii_case(name) {
            node.value()
                .attr("content")
                .map(normalize_whitespace)
                .filter(|content| !content.is_empty())
        } else {
            None
        }
    });
    let mut content = contents.next()?;
    if name.eq_ignore_ascii_case("robots") {
        for value in contents {
            content.push_str(", ");
            content.push_str(&value);
        }
    }
    Some(content)
}

fn canonical_href(document: &Html, base_url: &Url) -> Option<String> {
    let selector = selector("link[rel][href]");
    document.select(&selector).find_map(|node| {
        let rel = node.value().attr("rel")?;
        if rel
            .split_whitespace()
            .any(|part| part.eq_ignore_ascii_case("canonical"))
        {
            normalize_url(base_url, node.value().attr("href")?).map(|url| url.to_string())
        } else {
            None
        }
    })
}

fn canonical_count(document: &Html) -> usize {
    let selector = selector("link[rel][href]");
    document
        .select(&selector)
        .filter(|node| {
            node.value()
                .attr("rel")
                .map(|rel| {
                    rel.split_whitespace()
                        .any(|part| part.eq_ignore_ascii_case("canonical"))
                })
                .unwrap_or(false)
        })
        .count()
}

fn link_href_by_rel(document: &Html, base_url: &Url, rel_name: &str) -> Option<String> {
    let selector = selector("link[rel][href]");
    document.select(&selector).find_map(|node| {
        let rel = node.value().attr("rel")?;
        if rel
            .split_whitespace()
            .any(|part| part.eq_ignore_ascii_case(rel_name))
        {
            normalize_url(base_url, node.value().attr("href")?).map(|url| url.to_string())
        } else {
            None
        }
    })
}

#[derive(Default)]
struct HreflangStats {
    count: u32,
    invalid_count: u32,
    missing_self_reference: bool,
    links: Vec<HreflangLink>,
}

fn hreflang_stats(document: &Html, base_url: &Url) -> HreflangStats {
    let selector = selector("link[rel][hreflang][href]");
    let mut stats = HreflangStats::default();
    let mut has_self_reference = false;

    for node in document.select(&selector).filter(|node| {
        node.value()
            .attr("rel")
            .map(|rel| {
                rel.split_whitespace()
                    .any(|part| part.eq_ignore_ascii_case("alternate"))
            })
            .unwrap_or(false)
    }) {
        stats.count = stats.count.saturating_add(1);

        let hreflang = node.value().attr("hreflang").unwrap_or_default().trim();
        let valid = valid_hreflang(hreflang);
        if !valid {
            stats.invalid_count = stats.invalid_count.saturating_add(1);
        }

        if let Some(url) = node
            .value()
            .attr("href")
            .and_then(|href| normalize_url(base_url, href))
        {
            if url.as_str() == base_url.as_str() {
                has_self_reference = true;
            }
            stats.links.push(HreflangLink {
                hreflang: hreflang.to_string(),
                url: url.to_string(),
                valid,
            });
        }
    }

    stats.missing_self_reference = stats.count > 0 && !has_self_reference;
    stats
}

fn valid_hreflang(value: &str) -> bool {
    if value.eq_ignore_ascii_case("x-default") {
        return true;
    }

    let mut parts = value.split('-');
    let Some(language) = parts.next() else {
        return false;
    };
    if !(2..=3).contains(&language.len()) || !language.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }

    for part in parts {
        let valid_region = part.len() == 2 && part.chars().all(|ch| ch.is_ascii_alphabetic())
            || part.len() == 3 && part.chars().all(|ch| ch.is_ascii_digit());
        let valid_script = part.len() == 4 && part.chars().all(|ch| ch.is_ascii_alphabetic());
        if !valid_region && !valid_script {
            return false;
        }
    }

    true
}

#[derive(Default)]
struct JsonLdStats {
    count: u32,
    invalid_count: u32,
    warning_count: u32,
    error_count: u32,
    issues: Vec<StructuredDataIssue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StructuredDataSeverity {
    Warning,
    Error,
}

struct JsonLdEntity<'a> {
    value: &'a Value,
    path: String,
    has_context: bool,
    context_is_schema_org: bool,
}

fn json_ld_stats(document: &Html) -> JsonLdStats {
    let selector = selector("script[type]");
    document
        .select(&selector)
        .fold(JsonLdStats::default(), |mut stats, node| {
            let is_json_ld = node
                .value()
                .attr("type")
                .map(|value| value.eq_ignore_ascii_case("application/ld+json"))
                .unwrap_or(false);
            if !is_json_ld {
                return stats;
            }

            stats.count = stats.count.saturating_add(1);
            let block_index = stats.count;
            let source = node.text().collect::<Vec<_>>().join("");
            match serde_json::from_str::<Value>(&source) {
                Ok(value) => validate_json_ld_block(&value, block_index, &mut stats),
                Err(error) => {
                    stats.invalid_count = stats.invalid_count.saturating_add(1);
                    push_structured_data_issue(
                        &mut stats,
                        StructuredDataSeverity::Error,
                        format!("JSON-LD block {block_index} is not valid JSON: {error}"),
                        format!("script[{block_index}]"),
                    );
                }
            }
            stats
        })
}

fn validate_json_ld_block(value: &Value, block_index: u32, stats: &mut JsonLdStats) {
    let mut entities = Vec::new();
    collect_json_ld_entities(
        value,
        format!("script[{block_index}]"),
        false,
        false,
        &mut entities,
    );

    if entities.is_empty() {
        push_structured_data_issue(
            stats,
            StructuredDataSeverity::Error,
            format!("JSON-LD block {block_index} does not contain an object entity"),
            format!("script[{block_index}]"),
        );
        return;
    }

    for entity in entities {
        validate_json_ld_entity(&entity, stats);
    }
}

fn collect_json_ld_entities<'a>(
    value: &'a Value,
    path: String,
    inherited_context: bool,
    inherited_schema_context: bool,
    entities: &mut Vec<JsonLdEntity<'a>>,
) {
    match value {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_json_ld_entities(
                    item,
                    format!("{path}[{index}]"),
                    inherited_context,
                    inherited_schema_context,
                    entities,
                );
            }
        }
        Value::Object(object) => {
            let has_own_context = object.contains_key("@context");
            let own_schema_context = object
                .get("@context")
                .is_some_and(context_mentions_schema_org);
            let has_context = inherited_context || has_own_context;
            let schema_context = inherited_schema_context || own_schema_context;
            let has_graph = object.contains_key("@graph");
            let has_type = object.contains_key("@type");

            if has_type || !has_graph {
                entities.push(JsonLdEntity {
                    value,
                    path: path.clone(),
                    has_context,
                    context_is_schema_org: schema_context,
                });
            }

            if let Some(graph) = object.get("@graph") {
                collect_json_ld_entities(
                    graph,
                    format!("{path}.@graph"),
                    has_context,
                    schema_context,
                    entities,
                );
            }
        }
        _ => {}
    }
}

fn validate_json_ld_entity(entity: &JsonLdEntity<'_>, stats: &mut JsonLdStats) {
    let Some(object) = entity.value.as_object() else {
        push_structured_data_issue(
            stats,
            StructuredDataSeverity::Error,
            "JSON-LD entity must be an object".to_string(),
            entity.path.clone(),
        );
        return;
    };

    if !entity.has_context {
        push_structured_data_issue(
            stats,
            StructuredDataSeverity::Warning,
            "JSON-LD entity is missing @context".to_string(),
            entity.path.clone(),
        );
    } else if !entity.context_is_schema_org {
        push_structured_data_issue(
            stats,
            StructuredDataSeverity::Warning,
            "JSON-LD @context does not reference schema.org".to_string(),
            format!("{}.@context", entity.path),
        );
    }

    let types = json_ld_types(object.get("@type"));
    if types.is_empty() {
        push_structured_data_issue(
            stats,
            StructuredDataSeverity::Error,
            "JSON-LD entity is missing @type".to_string(),
            entity.path.clone(),
        );
        return;
    }

    for schema_type in types {
        for required in required_structured_data_fields(&schema_type) {
            if !object
                .get(*required)
                .is_some_and(meaningful_structured_data_value)
            {
                push_structured_data_issue(
                    stats,
                    StructuredDataSeverity::Error,
                    format!("{schema_type} is missing required field {required}"),
                    format!("{}.{}", entity.path, required),
                );
            }
        }
    }
}

fn context_mentions_schema_org(value: &Value) -> bool {
    match value {
        Value::String(text) => text.to_lowercase().contains("schema.org"),
        Value::Array(items) => items.iter().any(context_mentions_schema_org),
        Value::Object(object) => object.values().any(context_mentions_schema_org),
        _ => false,
    }
}

fn json_ld_types(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => {
            vec![value.trim().to_string()]
        }
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn required_structured_data_fields(schema_type: &str) -> &'static [&'static str] {
    match schema_type {
        "Article" | "BlogPosting" | "NewsArticle" => &["headline", "author", "datePublished"],
        "Product" => &["name"],
        "Offer" => &["price", "priceCurrency", "availability"],
        "BreadcrumbList" => &["itemListElement"],
        "Organization" | "LocalBusiness" => &["name"],
        "WebSite" => &["name", "url"],
        "FAQPage" => &["mainEntity"],
        "HowTo" => &["name", "step"],
        "VideoObject" => &["name", "thumbnailUrl", "uploadDate"],
        "Event" => &["name", "startDate", "location"],
        "Recipe" => &["name", "recipeIngredient", "recipeInstructions"],
        _ => &[],
    }
}

fn meaningful_structured_data_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        _ => true,
    }
}

fn push_structured_data_issue(
    stats: &mut JsonLdStats,
    severity: StructuredDataSeverity,
    message: String,
    path: String,
) {
    match severity {
        StructuredDataSeverity::Warning => {
            stats.warning_count = stats.warning_count.saturating_add(1)
        }
        StructuredDataSeverity::Error => stats.error_count = stats.error_count.saturating_add(1),
    }
    stats.issues.push(StructuredDataIssue {
        severity: match severity {
            StructuredDataSeverity::Warning => "warning",
            StructuredDataSeverity::Error => "error",
        }
        .to_string(),
        message,
        path,
    });
}

fn meta_prefix_count(document: &Html, prefix: &str) -> u32 {
    let selector = selector("meta[name], meta[property]");
    document
        .select(&selector)
        .filter(|node| {
            node.value()
                .attr("name")
                .or_else(|| node.value().attr("property"))
                .map(|value| {
                    value
                        .to_ascii_lowercase()
                        .starts_with(&prefix.to_ascii_lowercase())
                })
                .unwrap_or(false)
        })
        .count()
        .min(u32::MAX as usize) as u32
}

#[derive(Default)]
struct ImageStats {
    image_count: u32,
    images_missing_alt: u32,
    images_alt_too_long: u32,
}

fn image_stats(images: &[PageImage]) -> ImageStats {
    images
        .iter()
        .fold(ImageStats::default(), |mut stats, image| {
            stats.image_count = stats.image_count.saturating_add(1);
            if image.missing_alt {
                stats.images_missing_alt = stats.images_missing_alt.saturating_add(1);
            }
            if image.alt_too_long {
                stats.images_alt_too_long = stats.images_alt_too_long.saturating_add(1);
            }
            stats
        })
}

fn extract_images(document: &Html, base_url: &Url) -> Vec<PageImage> {
    let selector = selector("img[src]");
    document
        .select(&selector)
        .enumerate()
        .filter_map(|(index, node)| {
            let src = node.value().attr("src")?;
            let url = normalize_url(base_url, src)?;
            let raw_alt = node.value().attr("alt").map(str::trim).unwrap_or_default();
            let alt_text = if raw_alt.is_empty() {
                None
            } else {
                Some(raw_alt.to_string())
            };
            let alt_len = raw_alt.chars().count().min(u32::MAX as usize) as u32;
            Some(PageImage {
                url: url.to_string(),
                alt_text,
                alt_len,
                missing_alt: raw_alt.is_empty(),
                alt_too_long: raw_alt.chars().count() > IMAGE_ALT_MAX,
                width: node.value().attr("width").and_then(parse_dimension_attr),
                height: node.value().attr("height").and_then(parse_dimension_attr),
                source_position: index.saturating_add(1).min(u32::MAX as usize) as u32,
            })
        })
        .collect()
}

fn parse_dimension_attr(value: &str) -> Option<u32> {
    let digits = value
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn mixed_content_count(document: &Html, base_url: &Url) -> u32 {
    if base_url.scheme() != "https" {
        return 0;
    }

    let selector = selector("[src], [href]");
    document
        .select(&selector)
        .filter(|node| {
            node.value()
                .attr("src")
                .or_else(|| node.value().attr("href"))
                .map(|value| value.trim().to_ascii_lowercase().starts_with("http://"))
                .unwrap_or(false)
        })
        .count()
        .min(u32::MAX as usize) as u32
}

fn insecure_form_count(document: &Html, base_url: &Url) -> u32 {
    let selector = selector("form");
    document
        .select(&selector)
        .filter(|node| {
            if base_url.scheme() == "http" {
                return true;
            }
            node.value()
                .attr("action")
                .and_then(|action| normalize_url(base_url, action))
                .map(|url| url.scheme() == "http")
                .unwrap_or(false)
        })
        .count()
        .min(u32::MAX as usize) as u32
}

fn extract_links(document: &Html, base_url: &Url) -> Vec<PageLink> {
    let selector = selector("a[href]");
    document
        .select(&selector)
        .enumerate()
        .filter_map(|(index, node)| {
            let href = node.value().attr("href")?;
            let url = normalize_url(base_url, href)?;
            let rel = node.value().attr("rel").unwrap_or_default().to_string();
            let rel_nofollow = node
                .value()
                .attr("rel")
                .unwrap_or_default()
                .split_whitespace()
                .any(|part| part.eq_ignore_ascii_case("nofollow"));
            let text = normalize_whitespace(&node.text().collect::<Vec<_>>().join(" "));
            Some(PageLink {
                url: url.to_string(),
                text,
                rel,
                rel_nofollow,
                source_position: index.saturating_add(1).min(u32::MAX as usize) as u32,
            })
        })
        .collect()
}

fn extract_resources(document: &Html, base_url: &Url) -> Vec<PageResource> {
    let mut resources = Vec::new();

    let image_selector = selector("img[src], source[src], source[srcset]");
    for node in document.select(&image_selector) {
        let raw = node
            .value()
            .attr("src")
            .or_else(|| node.value().attr("srcset"))
            .and_then(first_srcset_candidate);
        push_resource(
            &mut resources,
            base_url,
            raw,
            "image",
            PageResourceType::Image,
        );
    }

    let style_selector = selector("link[rel][href]");
    for node in document.select(&style_selector) {
        let rel = node.value().attr("rel").unwrap_or_default();
        let is_stylesheet = rel
            .split_whitespace()
            .any(|part| part.eq_ignore_ascii_case("stylesheet"));
        if is_stylesheet {
            push_resource(
                &mut resources,
                base_url,
                node.value().attr("href"),
                "stylesheet",
                PageResourceType::Css,
            );
        }
    }

    let script_selector = selector("script[src]");
    for node in document.select(&script_selector) {
        push_resource(
            &mut resources,
            base_url,
            node.value().attr("src"),
            "script",
            PageResourceType::JavaScript,
        );
    }

    resources
}

fn first_srcset_candidate(value: &str) -> Option<&str> {
    value
        .split(',')
        .next()
        .and_then(|candidate| candidate.split_whitespace().next())
}

fn push_resource(
    resources: &mut Vec<PageResource>,
    base_url: &Url,
    raw: Option<&str>,
    label: &str,
    resource_type: PageResourceType,
) {
    let Some(url) = raw.and_then(|value| normalize_url(base_url, value)) else {
        return;
    };
    let source_position = resources.len().saturating_add(1).min(u32::MAX as usize) as u32;
    resources.push(PageResource {
        url: url.to_string(),
        label: label.to_string(),
        resource_type,
        source_position,
    });
}

fn visible_text(document: &Html) -> String {
    let selector = selector("body");
    document
        .select(&selector)
        .next()
        .map(|node| normalize_whitespace(&node.text().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default()
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_basic_page_signals() {
        let base = Url::parse("https://example.com/docs/index.html").unwrap();
        let html = r#"
            <html>
              <head>
                <title> Ferrous Frog </title>
                <meta name="description" content="A rusty SEO crawler">
                <meta name="robots" content="noindex, follow">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <meta property="og:title" content="Ferrous Frog">
                <meta name="twitter:card" content="summary">
                <link rel="canonical" href="/canonical">
                <link rel="canonical" href="/other">
                <link rel="amphtml" href="/amp">
                <link rel="next" href="/docs/page-2">
                <link rel="prev" href="/docs/page-0">
                <link rel="alternate" hreflang="en" href="/docs/index.html">
                <link rel="alternate" hreflang="bad_locale_code" href="/docs/bad.html">
                <link rel="stylesheet" href="/assets/site.css">
                <script type="application/ld+json">{ bad json }</script>
                <script src="/assets/app.js"></script>
              </head>
              <body>
                <h1> Main Heading </h1>
                <h2> Secondary Heading </h2>
                <font id="reused-id">Legacy copy</font>
                <center id="reused-id">Centered copy</center>
                <a href="/next#fragment" rel="nofollow sponsored">Next</a>
                <img src="/image.jpg" width="640" height="480">
                <img src="http://cdn.example.com/mixed.jpg" alt="Mixed">
                <form action="http://example.com/login"></form>
              </body>
            </html>
        "#;

        let signals = parse_html(&base, html);

        assert_eq!(signals.title.as_deref(), Some("Ferrous Frog"));
        assert!(signals.title_pixel_width > 0);
        assert_eq!(
            signals.meta_description.as_deref(),
            Some("A rusty SEO crawler")
        );
        assert!(signals.meta_description_pixel_width > 0);
        assert_eq!(signals.h1.as_deref(), Some("Main Heading"));
        assert_eq!(signals.h1_count, 1);
        assert_eq!(signals.h2.as_deref(), Some("Secondary Heading"));
        assert_eq!(signals.h2_count, 1);
        assert_eq!(
            signals.canonical.as_deref(),
            Some("https://example.com/canonical")
        );
        assert_eq!(signals.canonical_count, 2);
        assert_eq!(signals.meta_robots.as_deref(), Some("noindex, follow"));
        assert_eq!(signals.indexability, "Non-indexable");
        assert_eq!(signals.links[0].url, "https://example.com/next");
        assert_eq!(signals.links[0].rel, "nofollow sponsored");
        assert!(signals.links[0].rel_nofollow);
        assert_eq!(signals.links[0].source_position, 1);
        assert_eq!(signals.image_count, 2);
        assert_eq!(signals.images_missing_alt, 1);
        assert_eq!(signals.images.len(), 2);
        assert_eq!(signals.images[0].url, "https://example.com/image.jpg");
        assert!(signals.images[0].missing_alt);
        assert_eq!(signals.images[0].width, Some(640));
        assert_eq!(signals.images[0].height, Some(480));
        assert_eq!(signals.mixed_content_count, 1);
        assert_eq!(signals.insecure_form_count, 1);
        assert!(signals.viewport);
        assert_eq!(signals.amphtml.as_deref(), Some("https://example.com/amp"));
        assert_eq!(
            signals.rel_next.as_deref(),
            Some("https://example.com/docs/page-2")
        );
        assert_eq!(
            signals.rel_prev.as_deref(),
            Some("https://example.com/docs/page-0")
        );
        assert_eq!(signals.hreflang_count, 2);
        assert_eq!(signals.hreflang_invalid_count, 1);
        assert!(!signals.hreflang_missing_self_reference);
        assert_eq!(signals.hreflang_links.len(), 2);
        assert_eq!(signals.hreflang_links[0].hreflang, "en");
        assert_eq!(
            signals.hreflang_links[0].url,
            "https://example.com/docs/index.html"
        );
        assert!(signals.hreflang_links[0].valid);
        assert_eq!(signals.hreflang_links[1].hreflang, "bad_locale_code");
        assert!(!signals.hreflang_links[1].valid);
        assert_eq!(signals.json_ld_count, 1);
        assert_eq!(signals.json_ld_invalid_count, 1);
        assert_eq!(signals.structured_data_error_count, 1);
        assert_eq!(signals.structured_data_warning_count, 0);
        assert!(
            signals.structured_data_issues[0]
                .message
                .contains("not valid JSON")
        );
        assert_eq!(signals.open_graph_count, 1);
        assert_eq!(signals.twitter_card_count, 1);
        assert_eq!(signals.deprecated_html_tag_count, 2);
        assert_eq!(signals.duplicate_id_count, 1);
        assert_eq!(signals.resources.len(), 4);
        assert!(signals.resources.iter().any(|resource| {
            resource.url == "https://example.com/image.jpg"
                && resource.resource_type == PageResourceType::Image
        }));
        assert!(signals.resources.iter().any(|resource| {
            resource.url == "https://example.com/assets/site.css"
                && resource.resource_type == PageResourceType::Css
        }));
        assert!(signals.resources.iter().any(|resource| {
            resource.url == "https://example.com/assets/app.js"
                && resource.resource_type == PageResourceType::JavaScript
        }));
        assert_eq!(signals.word_count, 9);
        assert!(signals.text_to_code_ratio > 0.0);
    }

    #[test]
    fn combines_robots_meta_tags_and_applies_none_without_matching_parameter_values() {
        let base = Url::parse("https://example.com/page").unwrap();
        for (directive, expected) in [
            ("NoInDeX", "Non-indexable"),
            ("NoNe", "Non-indexable"),
            ("noindex follow", "Non-indexable"),
            ("max-image-preview:none noindex", "Non-indexable"),
            ("max-image-preview: none noindex", "Non-indexable"),
            ("noindex max-image-preview: none", "Non-indexable"),
            ("nofollow", "Indexable"),
            ("max-image-preview: none", "Indexable"),
            ("all", "Indexable"),
        ] {
            let html = format!(
                r#"<html><head>
                    <meta name="robots" content="index, follow">
                    <meta name="ROBOTS" content="{directive}">
                    <meta name="otherbot" content="noindex">
                </head><body><a href="/target">Target</a></body></html>"#
            );
            let signals = parse_html(&base, &html);
            assert_eq!(signals.indexability, expected, "directive: {directive}");
            assert!(signals.meta_robots.as_deref().unwrap().contains(directive));
        }
    }

    #[test]
    fn ignores_non_crawlable_links() {
        let base = Url::parse("https://example.com/").unwrap();

        assert!(normalize_url(&base, "mailto:test@example.com").is_none());
        assert!(normalize_url(&base, "#section").is_none());
        assert!(normalize_url(&base, "/ok").is_some());
    }

    #[test]
    fn validates_json_ld_entity_shape() {
        let base = Url::parse("https://example.com/article").unwrap();
        let html = r#"
            <html>
              <head>
                <script type="application/ld+json">
                  {
                    "@context": "https://schema.org",
                    "@type": "Article",
                    "headline": "Article headline"
                  }
                </script>
              </head>
              <body></body>
            </html>
        "#;

        let signals = parse_html(&base, html);

        assert_eq!(signals.json_ld_count, 1);
        assert_eq!(signals.json_ld_invalid_count, 0);
        assert_eq!(signals.structured_data_error_count, 2);
        assert_eq!(signals.structured_data_warning_count, 0);
        assert!(signals.structured_data_issues.iter().any(|issue| {
            issue.message == "Article is missing required field author"
                && issue.path == "script[1].author"
        }));
        assert!(signals.structured_data_issues.iter().any(|issue| {
            issue.message == "Article is missing required field datePublished"
                && issue.path == "script[1].datePublished"
        }));
    }
}
