use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSignals {
    pub title: Option<String>,
    pub title_len: usize,
    pub meta_description: Option<String>,
    pub meta_description_len: usize,
    pub h1: Option<String>,
    pub h1_len: usize,
    pub canonical: Option<String>,
    pub indexability: String,
    pub indexability_status: String,
    pub links: Vec<PageLink>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLink {
    pub url: String,
    pub text: String,
    pub rel_nofollow: bool,
}

pub fn parse_html(base_url: &Url, html: &str) -> PageSignals {
    let document = Html::parse_document(html);
    let title = first_text(&document, "title");
    let meta_description = meta_content(&document, "description");
    let robots = meta_content(&document, "robots").unwrap_or_default();
    let h1 = first_text(&document, "h1");
    let canonical = canonical_href(&document, base_url);
    let links = extract_links(&document, base_url);
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
    let h1_len = h1
        .as_deref()
        .map(str::chars)
        .map(Iterator::count)
        .unwrap_or(0);
    let robots_lower = robots.to_lowercase();
    let has_noindex = robots_lower
        .split(',')
        .map(str::trim)
        .any(|directive| directive == "noindex");

    PageSignals {
        title,
        title_len,
        meta_description,
        meta_description_len,
        h1,
        h1_len,
        canonical,
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
        links,
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

fn meta_content(document: &Html, name: &str) -> Option<String> {
    let selector = selector("meta[name], meta[property]");
    document.select(&selector).find_map(|node| {
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
    })
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

fn extract_links(document: &Html, base_url: &Url) -> Vec<PageLink> {
    let selector = selector("a[href]");
    document
        .select(&selector)
        .filter_map(|node| {
            let href = node.value().attr("href")?;
            let url = normalize_url(base_url, href)?;
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
                rel_nofollow,
            })
        })
        .collect()
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
                <link rel="canonical" href="/canonical">
              </head>
              <body>
                <h1> Main Heading </h1>
                <a href="/next#fragment">Next</a>
              </body>
            </html>
        "#;

        let signals = parse_html(&base, html);

        assert_eq!(signals.title.as_deref(), Some("Ferrous Frog"));
        assert_eq!(
            signals.meta_description.as_deref(),
            Some("A rusty SEO crawler")
        );
        assert_eq!(signals.h1.as_deref(), Some("Main Heading"));
        assert_eq!(
            signals.canonical.as_deref(),
            Some("https://example.com/canonical")
        );
        assert_eq!(signals.indexability, "Non-indexable");
        assert_eq!(signals.links[0].url, "https://example.com/next");
    }

    #[test]
    fn ignores_non_crawlable_links() {
        let base = Url::parse("https://example.com/").unwrap();

        assert!(normalize_url(&base, "mailto:test@example.com").is_none());
        assert!(normalize_url(&base, "#section").is_none());
        assert!(normalize_url(&base, "/ok").is_some());
    }
}
