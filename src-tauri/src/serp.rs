use ferrous_frog_parser::estimate_text_pixel_width;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write};
use tauri::AppHandle;
use url::Url;

const MAX_ROWS: usize = 1000;
const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_FIELD_BYTES: usize = 20_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SerpSnippet {
    pub url: String,
    pub title: String,
    pub description: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetMetrics {
    title_length: usize,
    title_pixel_width: u32,
    description_length: usize,
    description_pixel_width: u32,
}

fn validate_snippet(snippet: &SerpSnippet) -> Result<(), String> {
    if [&snippet.url, &snippet.title, &snippet.description]
        .iter()
        .any(|field| field.len() > MAX_FIELD_BYTES)
    {
        return Err("Each snippet field must be at most 20,000 bytes.".into());
    }
    if !Url::parse(&snippet.url)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
    {
        return Err("Enter an absolute HTTP or HTTPS URL.".into());
    }
    Ok(())
}

#[tauri::command]
pub fn measure_serp_snippet(snippet: SerpSnippet) -> Result<SnippetMetrics, String> {
    validate_snippet(&snippet)?;
    Ok(SnippetMetrics {
        title_length: snippet.title.chars().count(),
        title_pixel_width: estimate_text_pixel_width(&snippet.title, 18.0),
        description_length: snippet.description.chars().count(),
        description_pixel_width: estimate_text_pixel_width(&snippet.description, 13.0),
    })
}

#[tauri::command]
pub fn import_serp_snippets(text: String) -> Result<Vec<SerpSnippet>, String> {
    parse_snippets(&text)
}

fn parse_snippets(text: &str) -> Result<Vec<SerpSnippet>, String> {
    if text.len() > MAX_BYTES {
        return Err("Snippet CSV files must be at most 5 MiB.".into());
    }
    let mut reader = csv::Reader::from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let headers = reader.headers().map_err(|error| error.to_string())?;
    let column = |names: &[&str]| {
        headers.iter().position(|header| {
            names
                .iter()
                .any(|name| header.trim().eq_ignore_ascii_case(name))
        })
    };
    let (Some(url), Some(title), Some(description)) = (
        column(&["url", "address"]),
        column(&["title"]),
        column(&["description", "meta_description"]),
    ) else {
        return Err(
            "CSV requires url, title, and description (or meta_description) columns.".into(),
        );
    };
    let mut rows = Vec::new();
    for (index, result) in reader.records().enumerate() {
        if index >= MAX_ROWS {
            return Err("Import at most 1,000 snippets at a time.".into());
        }
        let record = result.map_err(|error| format!("CSV record {}: {error}", index + 2))?;
        let row = SerpSnippet {
            url: record[url].trim().to_string(),
            title: record[title].to_string(),
            description: record[description].to_string(),
        };
        validate_snippet(&row).map_err(|error| format!("CSV record {}: {error}", index + 2))?;
        rows.push(row);
    }
    if rows.is_empty() {
        return Err("The CSV contains no snippets.".into());
    }
    Ok(rows)
}

fn snippets_to_csv(snippets: &[SerpSnippet]) -> Result<Vec<u8>, String> {
    if snippets.is_empty() || snippets.len() > MAX_ROWS {
        return Err("Export between 1 and 1,000 snippets.".into());
    }
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer
        .write_record(["url", "title", "description"])
        .map_err(|error| error.to_string())?;
    let mut size = 0;
    for row in snippets {
        validate_snippet(row)?;
        size += row.url.len() + row.title.len() + row.description.len();
        if size > MAX_BYTES {
            return Err("Snippet exports must be at most 5 MiB.".into());
        }
        let cells = [&row.url, &row.title, &row.description].map(|value| {
            if value.trim_start().starts_with(['=', '+', '-', '@']) {
                format!("'{value}")
            } else {
                value.clone()
            }
        });
        writer
            .write_record(cells)
            .map_err(|error| error.to_string())?;
    }
    let bytes = writer.into_inner().map_err(|error| error.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Snippet exports must be at most 5 MiB.".into());
    }
    Ok(bytes)
}

#[tauri::command]
pub fn export_serp_snippets(
    app: AppHandle,
    snippets: Vec<SerpSnippet>,
) -> Result<crate::ExportFileResult, String> {
    let bytes = snippets_to_csv(&snippets)?;
    let path = crate::export_path(
        &app,
        &format!("ferrous-frog-snippets-{}.csv", crate::now_ms()),
    )?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("Could not create snippet export: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("Could not save snippets: {error}"))?;
    Ok(crate::ExportFileResult {
        path: path.to_string_lossy().into_owned(),
        row_count: snippets.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_csv_preserves_unicode_quotes_multiline_fields_and_duplicates() {
        let input = "\u{feff}url,title,meta_description\r\nhttps://example.test/,\"A, \\\"title\\\"\",\"Line one\nİstanbul 🦀\"\r\nhttps://example.test/,,\r\n".replace("\\\"", "\"\"");
        let rows = parse_snippets(&input).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "A, \"title\"");
        assert_eq!(rows[0].description, "Line one\nİstanbul 🦀");
        assert_eq!(rows[1].title, "");
        assert_eq!(
            parse_snippets(std::str::from_utf8(&snippets_to_csv(&rows).unwrap()).unwrap()).unwrap(),
            rows
        );
        assert!(
            parse_snippets("Address,Title,Description\nhttps://example.test/,Title,Description")
                .is_ok()
        );
    }

    #[test]
    fn snippet_csv_rejects_invalid_urls_missing_columns_and_excess_rows_atomically() {
        for input in [
            "url,title\nhttps://example.test/,Title".to_string(),
            "url,title,description\nhttps://example.test/,Title,Text\njavascript:alert(1),Title,Text".into(),
            "url,title,description\nfile:///etc/hosts,Title,Text".into(),
            "url,title,description\nhttps://example.test/,Title,Text\n,Missing,Text".into(),
            format!("url,title,description\n{}", "https://example.test/,Title,Text\n".repeat(1001)),
            "x".repeat(5 * 1024 * 1024 + 1),
        ] {
            assert!(parse_snippets(&input).is_err());
        }
        assert!(
            snippets_to_csv(&[SerpSnippet {
                url: "bad URL".into(),
                title: String::new(),
                description: String::new()
            }])
            .is_err()
        );
    }

    #[test]
    fn snippet_measurement_matches_crawled_metadata_and_export_escapes_formulas() {
        let snippet = SerpSnippet {
            url: "https://example.test/".into(),
            title: "İstanbul 🦀".into(),
            description: "A wide WWW and narrow iii".into(),
        };
        let signals = ferrous_frog_parser::parse_html(
            &Url::parse(&snippet.url).unwrap(),
            &format!(
                "<title>{}</title><meta name='description' content='{}'>",
                snippet.title, snippet.description
            ),
        );
        let metrics = measure_serp_snippet(snippet.clone()).unwrap();
        assert_eq!(metrics.title_pixel_width, signals.title_pixel_width);
        assert_eq!(
            metrics.description_pixel_width,
            signals.meta_description_pixel_width
        );
        assert_eq!(metrics.title_length, snippet.title.chars().count());
        let mut formula = snippet;
        formula.title = " =1+1".into();
        formula.description = "@SUM(1,1)".into();
        let exported = snippets_to_csv(&[formula]).unwrap();
        let rows = parse_snippets(std::str::from_utf8(&exported).unwrap()).unwrap();
        assert_eq!(rows[0].title, "' =1+1");
        assert_eq!(rows[0].description, "'@SUM(1,1)");
    }
}
