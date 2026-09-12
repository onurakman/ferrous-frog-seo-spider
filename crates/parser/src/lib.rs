use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use url::Url;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ContentConfig {
    pub include_selectors: Vec<String>,
    pub exclude_selectors: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ContentSelectors {
    include: Vec<Selector>,
    exclude: Vec<Selector>,
}

impl ContentSelectors {
    pub fn compile(config: &ContentConfig) -> Result<Self, String> {
        let compile = |values: &[String], kind: &str| {
            if values.len() > 100 {
                return Err(format!(
                    "Content {kind} selector at row 101 exceeds the limit of 100 selectors"
                ));
            }
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let value = value.trim();
                    if value.chars().take(2_001).count() > 2_000 {
                        return Err(format!(
                            "Content {kind} selector at row {} exceeds 2,000 characters",
                            index + 1
                        ));
                    }
                    Selector::parse(value).map_err(|_| {
                        format!("Invalid content {kind} selector at row {}", index + 1)
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(Self {
            include: compile(&config.include_selectors, "include")?,
            exclude: compile(&config.exclude_selectors, "exclude")?,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentPreview {
    pub text: String,
    pub word_count: usize,
    pub text_to_code_ratio: f64,
}

/// Uses the same text selection as a crawl, without extracting other page signals.
pub fn preview_content(html: &str, content: &ContentSelectors) -> ContentPreview {
    content_preview(&Html::parse_document(html), html.len(), content)
}

fn content_preview(document: &Html, html_len: usize, content: &ContentSelectors) -> ContentPreview {
    let text = visible_text(document, content);
    let word_count = text.split_whitespace().count();
    let text_to_code_ratio = if html_len == 0 {
        0.0
    } else {
        text.len() as f64 / html_len as f64
    };
    ContentPreview {
        text,
        word_count,
        text_to_code_ratio,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSignals {
    pub title: Option<String>,
    #[serde(default)]
    pub title_count: usize,
    pub title_len: usize,
    pub title_pixel_width: u32,
    pub meta_description: Option<String>,
    #[serde(default)]
    pub meta_description_count: usize,
    pub meta_description_len: usize,
    pub meta_description_pixel_width: u32,
    #[serde(default)]
    pub meta_keywords: Option<String>,
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
    #[serde(default)]
    pub sitemaps: Vec<String>,
    #[serde(default)]
    pub reference_links: Vec<PageReferenceLink>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PageReferenceKind {
    Canonical,
    Hreflang,
    Pagination,
    Amp,
    MetaRefresh,
    Iframe,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageReferenceLink {
    pub url: String,
    pub kind: PageReferenceKind,
    pub rel_nofollow: bool,
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
    parse_html_with_content(base_url, html, &ContentSelectors::default())
}

pub fn parse_html_with_content(
    base_url: &Url,
    html: &str,
    content: &ContentSelectors,
) -> PageSignals {
    let document = Html::parse_document(html);
    let title = first_text(&document, "title");
    let meta_description = meta_content(&document, "description");
    let meta_keywords = meta_content(&document, "keywords");
    let (title_count, meta_description_count) = metadata_tag_counts(&document);
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
    let ContentPreview {
        text: visible_text,
        word_count,
        text_to_code_ratio,
    } = content_preview(&document, html.len(), content);
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
        title_count,
        title_len,
        title_pixel_width,
        meta_description,
        meta_description_count,
        meta_description_len,
        meta_description_pixel_width,
        meta_keywords,
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
        sitemaps: link_hrefs_by_rel(&document, base_url, "sitemap"),
        reference_links: reference_links(&document, base_url),
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

pub fn estimate_text_pixel_width(text: &str, font_size_px: f32) -> u32 {
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
        .find(is_active_html_document_element)
        .map(|node| {
            let text = node
                .descendants()
                .filter_map(|descendant| {
                    let text = descendant.value().as_text()?;
                    (!descendant
                        .ancestors()
                        .any(|ancestor| ancestor.value().is_fragment()))
                    .then_some(text.text.as_ref())
                })
                .collect::<Vec<_>>()
                .join(" ");
            normalize_whitespace(&text)
        })
        .filter(|value| !value.is_empty())
}

fn element_count(document: &Html, selector_value: &str) -> usize {
    let selector = selector(selector_value);
    document
        .select(&selector)
        .filter(is_active_html_document_element)
        .count()
}

fn is_active_html_document_element(node: &ElementRef<'_>) -> bool {
    node.value().name.ns.as_ref() == "http://www.w3.org/1999/xhtml"
        && !node
            .ancestors()
            .any(|ancestor| ancestor.value().is_fragment())
}

fn metadata_tag_counts(document: &Html) -> (usize, usize) {
    let mut counts = (0, 0);
    let candidates = selector("title, meta[name]");
    for node in document
        .select(&candidates)
        .filter(is_active_html_document_element)
    {
        if node.value().name() == "title" {
            counts.0 += 1;
        } else if node
            .value()
            .attr("name")
            .is_some_and(|name| name.eq_ignore_ascii_case("description"))
        {
            counts.1 += 1;
        }
    }
    counts
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
    let mut contents = document
        .select(&selector)
        .filter(is_active_html_document_element)
        .filter_map(|node| {
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

/// Extract canonical targets from HTTP Link fields (RFC 8288).
pub fn canonical_link_headers<'a>(
    base_url: &Url,
    values: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    canonical_header_references(base_url, values)
        .into_iter()
        .map(|reference| reference.url)
        .collect()
}

/// Retain canonical relation directives for optional crawler discovery.
pub fn canonical_header_references<'a>(
    base_url: &Url,
    values: impl IntoIterator<Item = &'a str>,
) -> Vec<PageReferenceLink> {
    values
        .into_iter()
        .flat_map(|value| split_link_header(value, ','))
        .filter_map(|link| {
            let link = link.trim().strip_prefix('<')?;
            let (target, parameters) = link.split_once('>')?;
            let mut relation = None;
            let mut anchor = None;
            for parameter in
                split_link_header(parameters, ';').filter(|part| !part.trim().is_empty())
            {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    continue;
                };
                let value = value.trim();
                let value = if let Some(quoted) = value.strip_prefix('"') {
                    let quoted = quoted.strip_suffix('"')?;
                    let mut chars = quoted.chars();
                    let mut decoded = String::new();
                    while let Some(ch) = chars.next() {
                        decoded.push(if ch == '\\' { chars.next()? } else { ch });
                    }
                    decoded
                } else {
                    value.to_string()
                };
                // RFC 8288 uses the first occurrence of rel and anchor.
                if name.trim().eq_ignore_ascii_case("rel") && relation.is_none() {
                    relation = Some(value);
                } else if name.trim().eq_ignore_ascii_case("anchor") && anchor.is_none() {
                    anchor = Some(value);
                }
            }
            let relation = relation?;
            if !relation
                .split_ascii_whitespace()
                .any(|rel| rel.eq_ignore_ascii_case("canonical"))
            {
                return None;
            }
            if let Some(anchor) = anchor
                && base_url.join(&anchor).ok()?.as_str() != base_url.as_str()
            {
                return None;
            }
            let mut target = base_url.join(target).ok()?;
            if !matches!(target.scheme(), "http" | "https") {
                return None;
            }
            target.set_fragment(None);
            Some(PageReferenceLink {
                url: target.to_string(),
                kind: PageReferenceKind::Canonical,
                rel_nofollow: relation
                    .split_ascii_whitespace()
                    .any(|rel| rel.eq_ignore_ascii_case("nofollow")),
            })
        })
        .collect()
}

fn split_link_header(value: &str, separator: char) -> impl Iterator<Item = &str> {
    let (mut quoted, mut escaped, mut in_target) = (false, false, false);
    value.split(move |ch| {
        if escaped {
            escaped = false;
            return false;
        }
        match ch {
            '\\' if quoted => escaped = true,
            '"' if !in_target => quoted = !quoted,
            '<' if !quoted => in_target = true,
            '>' if !quoted => in_target = false,
            _ => {}
        }
        ch == separator && !quoted && !in_target
    })
}

fn canonical_href(document: &Html, base_url: &Url) -> Option<String> {
    link_href_by_rel(document, base_url, "canonical")
}

fn canonical_count(document: &Html) -> usize {
    let selector = selector("link[rel][href]");
    document
        .select(&selector)
        .filter(is_active_html_document_element)
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
    link_hrefs_by_rel(document, base_url, rel_name)
        .into_iter()
        .next()
}

fn link_hrefs_by_rel(document: &Html, base_url: &Url, rel_name: &str) -> Vec<String> {
    let selector = selector("link[rel][href]");
    document
        .select(&selector)
        .filter(is_active_html_document_element)
        .filter_map(|node| {
            let rel = node.value().attr("rel")?;
            if rel.split_whitespace().any(|part| {
                part.eq_ignore_ascii_case(rel_name)
                    || (rel_name.eq_ignore_ascii_case("prev")
                        && part.eq_ignore_ascii_case("previous"))
            }) {
                normalize_url(base_url, node.value().attr("href")?).map(|url| url.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn reference_links(document: &Html, base_url: &Url) -> Vec<PageReferenceLink> {
    let mut links = Vec::new();
    for node in document
        .select(&selector("link[rel][href]"))
        .filter(is_active_html_document_element)
    {
        let rel = node.value().attr("rel").unwrap_or_default();
        let has_rel = |name: &str| {
            rel.split_whitespace()
                .any(|part| part.eq_ignore_ascii_case(name))
        };
        let Some(url) = normalize_url(base_url, node.value().attr("href").unwrap_or_default())
        else {
            continue;
        };
        for (kind, matches) in [
            (PageReferenceKind::Canonical, has_rel("canonical")),
            (
                PageReferenceKind::Hreflang,
                has_rel("alternate") && node.value().attr("hreflang").is_some(),
            ),
            (
                PageReferenceKind::Pagination,
                has_rel("next") || has_rel("prev") || has_rel("previous"),
            ),
            (PageReferenceKind::Amp, has_rel("amphtml")),
        ] {
            if matches {
                links.push(PageReferenceLink {
                    url: url.to_string(),
                    kind,
                    rel_nofollow: has_rel("nofollow"),
                });
            }
        }
    }
    for node in document
        .select(&selector("meta[http-equiv][content]"))
        .filter(is_active_html_document_element)
        .filter(|node| {
            node.value()
                .attr("http-equiv")
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("refresh"))
        })
    {
        if let Some(url) = meta_refresh_target(node.value().attr("content").unwrap_or_default())
            .and_then(|target| normalize_url(base_url, target))
        {
            links.push(PageReferenceLink {
                url: url.to_string(),
                kind: PageReferenceKind::MetaRefresh,
                rel_nofollow: false,
            });
        }
    }
    for node in document
        .select(&selector("iframe[src]"))
        .filter(is_active_html_document_element)
    {
        if let Some(url) = normalize_url(base_url, node.value().attr("src").unwrap_or_default()) {
            links.push(PageReferenceLink {
                url: url.to_string(),
                kind: PageReferenceKind::Iframe,
                rel_nofollow: false,
            });
        }
    }
    links
}

/// Extracts the target from a refresh directive such as `5; url='/next'` or `0;/next`.
fn meta_refresh_target(content: &str) -> Option<&str> {
    let (_, rest) = content.split_once([';', ','])?;
    let rest = rest.trim();
    let target = match rest.get(..4) {
        Some(prefix) if prefix.eq_ignore_ascii_case("url=") => &rest[4..],
        _ => rest,
    };
    let target = target.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    (!target.is_empty()).then_some(target)
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

    for node in document
        .select(&selector)
        .filter(is_active_html_document_element)
        .filter(|node| {
            node.value()
                .attr("rel")
                .map(|rel| {
                    rel.split_whitespace()
                        .any(|part| part.eq_ignore_ascii_case("alternate"))
                })
                .unwrap_or(false)
        })
    {
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

fn visible_text(document: &Html, content: &ContentSelectors) -> String {
    if content.include.is_empty() && content.exclude.is_empty() {
        // Saved crawls without content settings retain their original body-text metrics.
        return document
            .select(&selector("body"))
            .next()
            .map(|node| normalize_whitespace(&node.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
    }

    let mut text = String::new();
    let mut pending = vec![(document.tree.root(), content.include.is_empty())];
    while let Some((node, mut included)) = pending.pop() {
        if let Some(element) = ElementRef::wrap(node) {
            if matches!(
                element.value().name(),
                "head" | "script" | "style" | "noscript" | "template"
            ) || content.exclude.iter().any(|rule| rule.matches(&element))
            {
                continue;
            }
            included |= content.include.iter().any(|rule| rule.matches(&element));
        }
        if included && let Some(value) = node.value().as_text() {
            for word in value.split_whitespace() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(word);
            }
        }
        // A single document walk counts overlapping includes only once. Skipping
        // excluded subtrees also prevents an inner include from restoring them.
        pending.extend(node.children().rev().map(|child| (child, included)));
    }
    text
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn active_document_metadata_ignores_svg_titles() {
        let signals = super::parse_html(
            &url::Url::parse("https://example.test/").unwrap(),
            "<html><head></head><body><svg><title>Chart label</title></svg></body></html>",
        );
        assert_eq!(signals.title, None);
        assert_eq!(signals.title_count, 0);
        assert_eq!(signals.title_len, 0);
        assert_eq!(signals.title_pixel_width, 0);
    }

    #[test]
    fn active_document_metadata_ignores_template_metadata_and_directives() {
        let signals = super::parse_html(
            &url::Url::parse("https://example.test/").unwrap(),
            "<html><head><template><title>Template title</title><meta name='description' content='Template description'><meta name='robots' content='noindex, nofollow'><meta name='viewport' content='width=device-width'></template></head><body>Active body</body></html>",
        );
        assert_eq!(
            (
                signals.title,
                signals.meta_description,
                signals.meta_robots,
                signals.viewport,
                signals.indexability.as_str()
            ),
            (None, None, None, false, "Indexable")
        );
        assert_eq!(signals.title_count, 0);
        assert_eq!(signals.meta_description_count, 0);
        assert_eq!(signals.indexability_status, "Indexable");
    }

    #[test]
    fn active_document_headings_ignore_template_elements() {
        let signals = super::parse_html(
            &url::Url::parse("https://example.test/").unwrap(),
            "<body><template><h1>Template heading</h1><h2>Template subheading</h2><center>Template deprecated tag</center></template>Active body</body>",
        );
        assert_eq!((signals.h1, signals.h1_count, signals.h1_len), (None, 0, 0));
        assert_eq!((signals.h2, signals.h2_count, signals.h2_len), (None, 0, 0));
        assert_eq!(signals.deprecated_html_tag_count, 0);
    }

    #[test]
    fn active_document_headings_ignore_nested_template_text() {
        let signals = super::parse_html(
            &url::Url::parse("https://example.test/").unwrap(),
            "<body><h1>Active <svg><text>chart</text></svg> heading<template>inert text <span>nested text</span></template> end</h1><h2>Active<template><span>inert subheading</span></template> subheading</h2></body>",
        );
        assert_eq!(signals.h1.as_deref(), Some("Active chart heading end"));
        assert_eq!(signals.h1_count, 1);
        assert_eq!(signals.h2.as_deref(), Some("Active subheading"));
        assert_eq!(signals.h2_count, 1);
    }

    #[test]
    fn active_document_metadata_preserves_order_robots_merging_and_property_fallback() {
        let html = "<html><head><template><title>Template title</title><meta name='description' content='Template description'><meta name='robots' content='noindex'><meta name='viewport' content='fake'></template><meta property='description' content='Legacy description'><meta NAME='DeScRiPtIoN' content='Later description'><meta name='ROBOTS' content='index, follow'><meta property='robots' content='max-image-preview: large'></head><body><svg><title>Chart label</title></svg><title>Active title</title><title>Later title</title><template><h1>Fake heading</h1><h2>Fake subheading</h2></template><h1>Active heading</h1><h1>Later heading</h1><h2>Active subheading</h2><main>Selected words</main></body></html>";
        for (include, exclude) in [(&[][..], &[][..]), (&["main"][..], &["h1", "h2"][..])] {
            let signals = scoped_signals(html, include, exclude);
            assert_eq!(signals.title.as_deref(), Some("Active title"));
            assert_eq!(signals.title_count, 2);
            assert_eq!(
                signals.meta_description.as_deref(),
                Some("Legacy description")
            );
            assert_eq!(signals.meta_description_count, 1);
            assert_eq!(
                signals.meta_robots.as_deref(),
                Some("index, follow, max-image-preview: large")
            );
            assert_eq!(signals.indexability, "Indexable");
            assert!(!signals.viewport);
            assert_eq!(signals.h1.as_deref(), Some("Active heading"));
            assert_eq!(signals.h1_count, 2);
            assert_eq!(signals.h2.as_deref(), Some("Active subheading"));
            assert_eq!(signals.h2_count, 1);
        }
        let empty_first = scoped_signals(
            "<template><title>Template title</title></template><title> </title><title>Later active title</title>",
            &[],
            &[],
        );
        assert_eq!(empty_first.title, None);
        assert_eq!(empty_first.title_count, 2);
    }

    #[test]
    fn multiple_metadata_counts_document_tags_without_changing_retained_values() {
        let html = r#"<html><head><title>First title</title><TITLE></TITLE>
            <meta name="description" content="First description">
            <META NAME="DESCRIPTION"><meta name="description" content=" ">
            <meta property="description" content="Legacy property"><meta property="og:description" content="Social description">
            <template><title>Template title</title><meta name="description" content="Template description"></template>
            <script>const markup = '<title>Script title</title><meta name="description">';</script>
            </head><body><svg><title>SVG label</title></svg></body></html>"#;
        let signals = super::parse_html(&url::Url::parse("https://example.test/").unwrap(), html);
        assert_eq!(signals.title.as_deref(), Some("First title"));
        assert_eq!(
            signals.meta_description.as_deref(),
            Some("First description")
        );
        let value = serde_json::to_value(signals).unwrap();
        assert_eq!(value["titleCount"], 2);
        assert_eq!(value["metaDescriptionCount"], 3);
        let empty = super::parse_html(
            &url::Url::parse("https://example.test/").unwrap(),
            "<title> </title><title>Later title</title><meta name=description><meta name=description content='Later description'>",
        );
        assert_eq!(empty.title, None, "Keep the existing first-title behavior");
        assert_eq!(empty.meta_description.as_deref(), Some("Later description"));
        let value = serde_json::to_value(empty).unwrap();
        assert_eq!(value["titleCount"], 2);
        assert_eq!(value["metaDescriptionCount"], 2);
    }

    #[test]
    fn multiple_metadata_counts_measure_zero_and_ignore_text_region_settings() {
        let base = url::Url::parse("https://example.test/").unwrap();
        let empty = super::parse_html(
            &base,
            "<template><title>Template only</title><meta name=description></template><svg><title>SVG only</title></svg>",
        );
        let value = serde_json::to_value(empty).unwrap();
        assert_eq!(value["titleCount"], 0);
        assert_eq!(value["metaDescriptionCount"], 0);
        let content = super::ContentSelectors::compile(&super::ContentConfig {
            include_selectors: vec!["main".into()],
            exclude_selectors: Vec::new(),
        })
        .unwrap();
        let selected = super::parse_html_with_content(
            &base,
            "<title>One</title><title>Two</title><meta name=description><meta name=description><main>Selected text</main>",
            &content,
        );
        let value = serde_json::to_value(selected).unwrap();
        assert_eq!(value["titleCount"], 2);
        assert_eq!(value["metaDescriptionCount"], 2);
    }

    use super::*;

    fn scoped_signals(html: &str, include: &[&str], exclude: &[&str]) -> PageSignals {
        let selectors = ContentSelectors::compile(&ContentConfig {
            include_selectors: include.iter().map(|value| (*value).to_owned()).collect(),
            exclude_selectors: exclude.iter().map(|value| (*value).to_owned()).collect(),
        })
        .unwrap();
        parse_html_with_content(
            &Url::parse("https://example.test/").unwrap(),
            html,
            &selectors,
        )
    }

    #[test]
    fn content_regions_union_overlapping_matches_in_document_order() {
        let html = "<nav>Navigation</nav><main>First <section class='copy'>second <b>third</b></section> fourth</main><article>fifth</article>";
        let signals = scoped_signals(html, &["article", ".copy", "main", "main b"], &[]);
        assert_eq!(signals.visible_text, "First second third fourth fifth");
        assert_eq!(signals.word_count, 5);
        assert_eq!(
            signals.text_to_code_ratio,
            signals.visible_text.len() as f64 / html.len() as f64
        );
    }

    #[test]
    fn content_exclusions_override_included_descendants_and_nested_regions() {
        let html = "<main>Keep <aside>discard <b>nested</b></aside><p>also keep</p></main><footer><p>discard too</p></footer>";
        assert_eq!(
            scoped_signals(html, &["main", "aside b", "footer p"], &["aside", "footer"])
                .visible_text,
            "Keep also keep"
        );
        assert_eq!(scoped_signals(html, &["main"], &["body"]).visible_text, "");
        assert_eq!(scoped_signals(html, &["main"], &["main"]).visible_text, "");
    }

    #[test]
    fn content_regions_with_no_matches_have_no_text_and_exclusions_use_the_body() {
        let html = "<head><title>Head</title></head><body><nav>skip this</nav><main>Keep this</main></body>";
        let no_match = scoped_signals(html, &[".missing"], &[]);
        assert_eq!(no_match.visible_text, "");
        assert_eq!(no_match.word_count, 0);
        assert_eq!(no_match.text_to_code_ratio, 0.0);
        assert_eq!(
            scoped_signals(html, &[], &["nav"]).visible_text,
            "Keep this"
        );
        assert_eq!(
            scoped_signals(html, &["html"], &["nav"]).visible_text,
            "Keep this"
        );
    }

    #[test]
    fn configured_content_regions_exclude_non_content_elements_and_keep_other_evidence() {
        let html = "<html><head><title>Page title</title><meta name='robots' content='noindex'><link rel='canonical' href='/canonical'><link rel='stylesheet' href='/site.css'></head><body><h1>Outside heading</h1><a href='/outside'>Outside link</a><main>Keep <b>these words</b><script>script words</script><style>style words</style><noscript>noscript words</noscript><template>template words</template></main><img src='/image.png'></body></html>";
        let scoped = scoped_signals(html, &["html", "main script"], &["h1", "a"]);
        assert_eq!(scoped.visible_text, "Keep these words");
        let whole = parse_html(&Url::parse("https://example.test/").unwrap(), html);
        let mut whole_evidence = serde_json::to_value(whole).unwrap();
        let mut scoped_evidence = serde_json::to_value(scoped).unwrap();
        for field in ["visibleText", "wordCount", "textToCodeRatio"] {
            whole_evidence.as_object_mut().unwrap().remove(field);
            scoped_evidence.as_object_mut().unwrap().remove(field);
        }
        assert_eq!(scoped_evidence, whole_evidence);
    }

    #[test]
    fn content_regions_preserve_legacy_body_text_when_settings_are_empty() {
        let html = "<head><title>Title</title></head><body>Visible <script>legacy script text</script><style>legacy style text</style></body>";
        assert_eq!(
            scoped_signals(html, &[], &[]).visible_text,
            "Visible legacy script text legacy style text"
        );
    }

    #[test]
    fn content_regions_reject_invalid_or_empty_selectors_with_their_location() {
        for (include, exclude, kind) in [
            (vec!["[".into()], vec![], "include"),
            (vec![], vec!["main".into(), "".into()], "exclude"),
            (vec!["   ".into()], vec![], "include"),
        ] {
            let error = ContentSelectors::compile(&ContentConfig {
                include_selectors: include,
                exclude_selectors: exclude,
            })
            .unwrap_err();
            assert!(error.contains(kind), "{error}");
            assert!(error.contains("row"), "{error}");
        }
    }

    #[test]
    fn content_selector_inputs_are_bounded_before_parsing() {
        for (kind, include) in [("include", true), ("exclude", false)] {
            let config = |selectors: Vec<String>| ContentConfig {
                include_selectors: if include {
                    selectors.clone()
                } else {
                    Vec::new()
                },
                exclude_selectors: if include { Vec::new() } else { selectors },
            };
            assert!(ContentSelectors::compile(&config(vec!["main".into(); 100])).is_ok());
            for values in [vec!["main".into(); 101], vec![String::new(); 101]] {
                let error = ContentSelectors::compile(&config(values)).unwrap_err();
                assert!(error.contains(kind), "{error}");
                assert!(error.contains("row 101"), "{error}");
                assert!(error.contains("100"), "{error}");
            }

            let longest = format!(".{}", "é".repeat(1_999));
            assert!(ContentSelectors::compile(&config(vec![longest])).is_ok());
            let too_long = format!(".{}privateTag", "é".repeat(1_990));
            assert_eq!(too_long.chars().count(), 2_001);
            let error =
                ContentSelectors::compile(&config(vec!["main".into(), too_long])).unwrap_err();
            assert!(error.contains(kind), "{error}");
            assert!(error.contains("row 2"), "{error}");
            assert!(error.contains("2,000"), "{error}");
            assert!(!error.contains("privateTag"), "{error}");
            let error = ContentSelectors::compile(&config(vec![":privateTag".into()])).unwrap_err();
            assert!(error.contains(kind), "{error}");
            assert!(error.contains("row 1"), "{error}");
            assert!(!error.contains("privateTag"), "{error}");
        }
    }

    #[test]
    fn content_preview_matches_crawl_text_metrics_and_serialization() {
        let config = ContentConfig {
            include_selectors: vec!["main".into(), "b".into()],
            exclude_selectors: vec!["aside".into()],
        };
        let selectors = ContentSelectors::compile(&config).unwrap();
        for html in [
            "<main> First <b>café 世界</b> <aside>discard</aside></main>",
            "",
        ] {
            let preview = preview_content(html, &selectors);
            let signals = parse_html_with_content(
                &Url::parse("https://example.test/").unwrap(),
                html,
                &selectors,
            );
            assert_eq!(preview.text, signals.visible_text);
            assert_eq!(preview.word_count, signals.word_count);
            assert_eq!(preview.text_to_code_ratio, signals.text_to_code_ratio);
            let json = serde_json::to_value(preview).unwrap();
            assert_eq!(json["wordCount"], signals.word_count);
            assert_eq!(json["textToCodeRatio"], signals.text_to_code_ratio);
        }
    }

    #[test]
    fn discovers_linked_sitemaps_with_http_urls_and_rel_tokens() {
        let signals = parse_html(
            &Url::parse("https://example.test/docs/").unwrap(),
            "<link rel='alternate SITEMAP' href='../map.xml#fragment'><link rel='sitemap' href='/second'><link rel='sitemap' href='file:///private'><link rel='canonical' href='/page'>",
        );
        let serialized = serde_json::to_value(signals).unwrap();
        assert_eq!(
            serialized["sitemaps"],
            serde_json::json!([
                "https://example.test/map.xml",
                "https://example.test/second"
            ])
        );
    }

    #[test]
    fn active_document_relations_ignore_template_only_self_references() {
        let signals = parse_html(
            &Url::parse("https://example.test/page").unwrap(),
            "<title>Active page</title><template><link rel='next prev canonical amphtml alternate sitemap' hreflang='en' href='/page'></template>",
        );
        assert_eq!(
            (
                signals.rel_next,
                signals.rel_prev,
                signals.canonical,
                signals.amphtml
            ),
            (None, None, None, None)
        );
        assert_eq!(signals.canonical_count, 0);
        assert_eq!(signals.hreflang_count, 0);
        assert!(!signals.hreflang_missing_self_reference);
        assert!(signals.hreflang_links.is_empty());
        assert!(signals.reference_links.is_empty());
        assert!(signals.sitemaps.is_empty());
    }

    #[test]
    fn active_document_relations_ignore_foreign_namespace_links() {
        let signals = parse_html(
            &Url::parse("https://example.test/page").unwrap(),
            "<title>Active page</title><svg><link rel='next prev canonical amphtml alternate sitemap' hreflang='en' href='/page'/></svg>",
        );
        assert_eq!(
            (
                signals.rel_next,
                signals.rel_prev,
                signals.canonical,
                signals.amphtml
            ),
            (None, None, None, None)
        );
        assert_eq!(signals.canonical_count, 0);
        assert_eq!(signals.hreflang_count, 0);
        assert!(signals.reference_links.is_empty());
        assert!(signals.sitemaps.is_empty());
    }

    #[test]
    fn active_document_relations_keep_later_targets_order_and_existing_link_resource_evidence() {
        let base = Url::parse("https://example.test/page").unwrap();
        let html = r#"<head><title>Active page</title><template>
            <link rel="next prev canonical amphtml alternate sitemap" hreflang="invalid_code" href="/page">
            </template>
            <link rel="NEXT" href="/real-next#fragment"><link rel="next" href="/second-next">
            <link rel="prev nofollow" href="/real-prev"><link rel="CANONICAL" href="/real-canonical">
            <link rel="canonical" href="javascript:alert(1)"><link rel="amphtml" href="/real-amp">
            <link rel="alternate nofollow" hreflang="en" href="/page"><link rel="sitemap" href="/real.xml">
            </head><body><template><a href="/template-anchor">Template anchor</a><img src="/template-image.png"></template>
            <main>Selected words</main><a href="/active-anchor">Active anchor</a><link rel="stylesheet" href="/site.css"></body>"#;
        let content = ContentSelectors::compile(&ContentConfig {
            include_selectors: vec!["main".into()],
            exclude_selectors: Vec::new(),
        })
        .unwrap();
        for selectors in [&ContentSelectors::default(), &content] {
            let signals = parse_html_with_content(&base, html, selectors);
            assert_eq!(
                signals.rel_next.as_deref(),
                Some("https://example.test/real-next")
            );
            assert_eq!(
                signals.rel_prev.as_deref(),
                Some("https://example.test/real-prev")
            );
            assert_eq!(
                signals.canonical.as_deref(),
                Some("https://example.test/real-canonical")
            );
            assert_eq!(
                signals.canonical_count, 2,
                "Active invalid hrefs still count as canonical tags"
            );
            assert_eq!(
                signals.amphtml.as_deref(),
                Some("https://example.test/real-amp")
            );
            assert_eq!(signals.hreflang_count, 1);
            assert_eq!(signals.hreflang_invalid_count, 0);
            assert!(!signals.hreflang_missing_self_reference);
            assert_eq!(signals.hreflang_links.len(), 1);
            assert_eq!(signals.sitemaps, ["https://example.test/real.xml"]);
            assert_eq!(
                serde_json::to_value(&signals.reference_links).unwrap(),
                serde_json::json!([
                    {"url":"https://example.test/real-next","kind":"pagination","relNofollow":false},
                    {"url":"https://example.test/second-next","kind":"pagination","relNofollow":false},
                    {"url":"https://example.test/real-prev","kind":"pagination","relNofollow":true},
                    {"url":"https://example.test/real-canonical","kind":"canonical","relNofollow":false},
                    {"url":"https://example.test/real-amp","kind":"amp","relNofollow":false},
                    {"url":"https://example.test/page","kind":"hreflang","relNofollow":true}
                ])
            );
            assert_eq!(
                signals
                    .links
                    .iter()
                    .map(|link| (&*link.url, link.source_position))
                    .collect::<Vec<_>>(),
                [
                    ("https://example.test/template-anchor", 1),
                    ("https://example.test/active-anchor", 2)
                ]
            );
            assert_eq!(
                signals
                    .resources
                    .iter()
                    .map(|resource| (&*resource.url, resource.resource_type))
                    .collect::<Vec<_>>(),
                [
                    (
                        "https://example.test/template-image.png",
                        PageResourceType::Image
                    ),
                    ("https://example.test/site.css", PageResourceType::Css)
                ]
            );
        }
        assert_eq!(
            canonical_link_headers(&base, ["</header-canonical>; rel=canonical"]),
            ["https://example.test/header-canonical"]
        );
    }

    #[test]
    fn legacy_previous_relation_preserves_document_order_and_reference_discovery() {
        for (first, second) in [("PREVIOUS", "prev"), ("prev", "PrEvIoUs")] {
            let signals = scoped_signals(
                &format!(
                    "<body><template><link rel='previous' href='/inert'></template>
                    <svg><link rel='previous' href='/foreign'/></svg>
                    <link rel='previous' href='javascript:alert(1)'>
                    <link rel='{first} nofollow' href='/first#fragment'>
                    <link rel='{second}' href='/second'>
                    <link rel='notprevious' href='/unrelated'>
                    <main>Selected text</main></body>"
                ),
                &["main"],
                &[],
            );
            assert_eq!(
                signals.rel_prev.as_deref(),
                Some("https://example.test/first")
            );
            assert!(signals.rel_next.is_none());
            assert_eq!(
                serde_json::to_value(&signals.reference_links).unwrap(),
                serde_json::json!([
                    {"url":"https://example.test/first","kind":"pagination","relNofollow":true},
                    {"url":"https://example.test/second","kind":"pagination","relNofollow":false}
                ])
            );
            assert_eq!(signals.visible_text, "Selected text");
            assert!(signals.links.is_empty());
        }
    }

    #[test]
    fn meta_keywords_are_captured_from_the_first_active_tag() {
        let signals = parse_html(
            &Url::parse("https://example.test/").unwrap(),
            r#"<head><meta name="KEYWORDS" content=" seo, crawler ">
                <meta name="keywords" content="second"></head>"#,
        );
        assert_eq!(signals.meta_keywords.as_deref(), Some("seo, crawler"));
        let signals = parse_html(
            &Url::parse("https://example.test/").unwrap(),
            "<head></head>",
        );
        assert_eq!(signals.meta_keywords, None);
    }

    #[test]
    fn meta_refresh_and_iframe_targets_are_typed_references() {
        assert_eq!(meta_refresh_target("5; url='/next'"), Some("/next"));
        assert_eq!(meta_refresh_target("0;URL=/next"), Some("/next"));
        assert_eq!(meta_refresh_target("0, /next"), Some("/next"));
        assert_eq!(meta_refresh_target("30"), None);
        assert_eq!(meta_refresh_target("0; url="), None);
        let signals = parse_html(
            &Url::parse("https://example.test/page").unwrap(),
            r#"<head><meta http-equiv="Refresh" content="0; url=/moved">
                <meta http-equiv="content-type" content="text/html"></head>
                <body><iframe src="https://embed.test/video"></iframe>
                <template><iframe src="/template"></iframe></template>
                <iframe></iframe></body>"#,
        );
        assert_eq!(
            serde_json::to_value(&signals.reference_links).unwrap(),
            serde_json::json!([
                {"url":"https://example.test/moved","kind":"metaRefresh","relNofollow":false},
                {"url":"https://embed.test/video","kind":"iframe","relNofollow":false}
            ])
        );
    }

    #[test]
    fn reference_links_keep_all_typed_targets_outside_the_selected_content() {
        let signals = scoped_signals(
            r##"<head>
                <link rel="CANONICAL alternate" href="/first#fragment">
                <link rel="canonical" href="/second">
                <link rel="alternate nofollow" hreflang="invalid_code" href="/language">
                <link rel="NEXT prev amphtml" href="/shared">
                <link rel="alternate" media="screen" href="/mobile">
                <link rel="canonical" href="javascript:alert(1)">
                <link rel="amphtml" href="file:///local">
                <link rel="next" href="#local">
            </head><body><main>Selected text</main><a href="/anchor">Anchor</a></body>"##,
            &["main"],
            &[],
        );
        let serialized = serde_json::to_value(&signals).unwrap();
        assert_eq!(
            serialized["referenceLinks"],
            serde_json::json!([
                {"url":"https://example.test/first","kind":"canonical","relNofollow":false},
                {"url":"https://example.test/second","kind":"canonical","relNofollow":false},
                {"url":"https://example.test/language","kind":"hreflang","relNofollow":true},
                {"url":"https://example.test/shared","kind":"pagination","relNofollow":false},
                {"url":"https://example.test/shared","kind":"amp","relNofollow":false}
            ])
        );
        assert_eq!(signals.canonical_count, 3);
        assert_eq!(signals.hreflang_invalid_count, 1);
        assert_eq!(signals.visible_text, "Selected text");
        assert_eq!(signals.links.len(), 1);
        assert_eq!(signals.links[0].url, "https://example.test/anchor");
        assert!(signals.resources.is_empty());
    }

    #[test]
    fn canonical_http_links_handle_repeated_fields_quotes_and_context() {
        let base = Url::parse("https://example.test/docs/file.pdf").unwrap();
        assert_eq!(
            canonical_link_headers(
                &base,
                [
                    r#"</next>; rel=next, </docs/canonical?a=1,2>; title="A, B; \"quoted\""; rel="alternate CANONICAL""#,
                    "<../other>; rel=canonical",
                    "</unrelated>; rel=canonical; anchor=\"/elsewhere\"",
                    "</ignored>; rel=next; rel=canonical",
                    "<javascript:alert(1)>; rel=canonical",
                    "</broken>; rel=\"canonical",
                ]
            ),
            [
                "https://example.test/docs/canonical?a=1,2",
                "https://example.test/other"
            ]
        );
        assert_eq!(
            canonical_link_headers(&base, ["<>; rel=canonical"]),
            [base.to_string()]
        );
    }

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
