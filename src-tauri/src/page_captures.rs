use crate::{AppState, ExportFileResult, query_store_rows, with_store_worker, write_atomic_export};
use ferrous_frog_storage::{ActiveStore, GridQuery, PageCapture, validate_grid_query};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::io::Write;
use std::path::PathBuf;
use tauri::State;

const PREVIEW_BYTES: usize = 64 * 1024;
const EXPORT_PAGE_SIZE: usize = 200;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum PageCaptureKind {
    ResponseHeaders,
    RawHtml,
    RenderedHtml,
    VisibleText,
}

impl PageCaptureKind {
    fn column(self) -> &'static str {
        match self {
            Self::ResponseHeaders => "client_observed_response_headers",
            Self::RawHtml => "raw_html",
            Self::RenderedHtml => "rendered_html",
            Self::VisibleText => "visible_text",
        }
    }

    fn truncated(self, capture: &PageCapture) -> bool {
        match self {
            Self::ResponseHeaders => capture.headers_truncated,
            Self::RawHtml => capture.raw_html_truncated,
            Self::RenderedHtml => capture.rendered_html_truncated,
            Self::VisibleText => capture.visible_text_truncated,
        }
    }

    fn text(self, capture: &PageCapture) -> Result<Option<Cow<'_, str>>, String> {
        Ok(match self {
            Self::ResponseHeaders => capture
                .response_headers
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| error.to_string())?
                .map(Cow::Owned),
            Self::RawHtml => capture.raw_html.as_deref().map(Cow::Borrowed),
            Self::RenderedHtml => capture.rendered_html.as_deref().map(Cow::Borrowed),
            Self::VisibleText => capture.visible_text.as_deref().map(Cow::Borrowed),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageCapturePreview {
    source_storage_key: String,
    source_url: String,
    final_url: String,
    kind: PageCaptureKind,
    text: String,
    stored_truncated: bool,
    preview_truncated: bool,
}

fn preview(
    store: &ActiveStore,
    key: &str,
    kind: PageCaptureKind,
) -> Result<Option<PageCapturePreview>, String> {
    let Some(capture) = store
        .try_page_capture(key)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(text) = kind.text(&capture)? else {
        return Ok(None);
    };
    let preview_truncated = text.len() > PREVIEW_BYTES;
    let mut end = text.len().min(PREVIEW_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let text = text[..end].to_owned();
    let stored_truncated = kind.truncated(&capture);
    Ok(Some(PageCapturePreview {
        source_storage_key: capture.source_storage_key,
        source_url: capture.source_url,
        final_url: capture.final_url,
        kind,
        text,
        stored_truncated,
        preview_truncated,
    }))
}

#[tauri::command(rename_all = "camelCase")]
pub(super) async fn get_page_capture(
    state: State<'_, AppState>,
    source_storage_key: String,
    kind: PageCaptureKind,
) -> Result<Option<PageCapturePreview>, String> {
    with_store_worker(&state, false, move |store| {
        preview(store, &source_storage_key, kind)
    })
    .await
}

pub(super) async fn export_capture_file(
    state: &AppState,
    path: PathBuf,
    query: GridQuery,
    kind: PageCaptureKind,
) -> Result<ExportFileResult, String> {
    with_store_worker(state, true, move |store| {
        write_atomic_export(&path, |file| write_capture_csv(store, query, kind, file))
    })
    .await
}

fn write_capture_csv(
    store: &ActiveStore,
    mut query: GridQuery,
    kind: PageCaptureKind,
    writer: &mut impl Write,
) -> Result<usize, String> {
    validate_grid_query(&query).map_err(|error| error.to_string())?;
    query.offset = 0;
    query.limit = EXPORT_PAGE_SIZE;
    let mut writer = csv::Writer::from_writer(writer);
    writer
        .write_record([
            "record_id",
            "source_storage_key",
            "source_url",
            "final_url",
            "list_position",
            "list_duplicate_index",
            kind.column(),
            "truncated",
        ])
        .map_err(|error| error.to_string())?;
    let mut count = 0;
    let mut expected_total = None;
    loop {
        let page = query_store_rows(store, query.clone())?;
        let total = *expected_total.get_or_insert(page.total);
        if page.total != total
            || page.rows.len() != total.saturating_sub(query.offset).min(query.limit)
        {
            return Err("crawl records changed while exporting captures; retry the export".into());
        }
        for record in &page.rows {
            let Some(capture) = store
                .try_page_capture(&record.storage_key)
                .map_err(|error| error.to_string())?
            else {
                continue;
            };
            let Some(text) = kind.text(&capture)? else {
                continue;
            };
            let cells = [
                Cow::Owned(record.id.to_string()),
                Cow::Borrowed(capture.source_storage_key.as_str()),
                Cow::Borrowed(capture.source_url.as_str()),
                Cow::Borrowed(capture.final_url.as_str()),
                Cow::Owned(
                    record
                        .list_position
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                ),
                Cow::Owned(record.list_duplicate_index.to_string()),
                text,
                Cow::Owned(kind.truncated(&capture).to_string()),
            ]
            .map(|value| {
                if value.trim_start().starts_with(['=', '+', '-', '@']) {
                    Cow::Owned(format!("'{value}"))
                } else {
                    value
                }
            });
            writer
                .write_record(cells.iter().map(|cell| cell.as_bytes()))
                .map_err(|error| error.to_string())?;
            count += 1;
        }
        query.offset += page.rows.len();
        if query.offset == total {
            break;
        }
    }
    writer.flush().map_err(|error| error.to_string())?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{CapturedHeader, CrawlRecord, CrawlStore, SqliteStore};

    fn idle_state() -> AppState {
        AppState {
            store: std::sync::Mutex::new(ActiveStore::memory()),
            control: std::sync::Mutex::new(None),
            crawl_task: tokio::sync::Mutex::new(None),
            current_session_id: std::sync::Mutex::new(None),
            comparison: std::sync::Mutex::new(crate::ComparisonState::default()),
            frontend_ready: std::sync::atomic::AtomicBool::new(true),
            exit_confirmed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn captured(store: &ActiveStore, position: u32) {
        let url = format!("https://example.test/{position:04}");
        let mut record = CrawlRecord::pending(url.clone(), 0);
        record.storage_key = format!("list:{position}");
        record.list_position = Some(position);
        record.list_duplicate_index = 1;
        record.title = Some(
            if position == 0 {
                "Excluded"
            } else {
                "Included"
            }
            .into(),
        );
        record.status_code = Some(200);
        store.upsert(record.clone());
        store
            .try_replace_page_capture(
                &record.storage_key,
                Some(PageCapture {
                    source_storage_key: record.storage_key.clone(),
                    source_url: url.clone(),
                    final_url: url,
                    raw_html: Some("<p>raw, \"quoted\"\nUnicode: İ</p>".into()),
                    rendered_html: Some(String::new()),
                    visible_text: Some(" =SUM(1,2)".into()),
                    response_headers: Some(vec![
                        CapturedHeader {
                            name: "link".into(),
                            value: "a".into(),
                        },
                        CapturedHeader {
                            name: "link".into(),
                            value: "b".into(),
                        },
                    ]),
                    visible_text_truncated: true,
                    ..Default::default()
                }),
            )
            .unwrap();
    }

    #[test]
    fn capture_csv_pages_the_full_filtered_view_and_preserves_text_headers_and_empty_captures() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            for position in 0..205 {
                captured(&store, position);
            }
            let query = GridQuery {
                global_search: Some("Included".into()),
                offset: 202,
                limit: 1,
                ..Default::default()
            };
            for kind in [
                PageCaptureKind::RawHtml,
                PageCaptureKind::RenderedHtml,
                PageCaptureKind::VisibleText,
                PageCaptureKind::ResponseHeaders,
            ] {
                let mut output = Vec::new();
                assert_eq!(
                    write_capture_csv(&store, query.clone(), kind, &mut output).unwrap(),
                    204
                );
                let mut reader = csv::Reader::from_reader(output.as_slice());
                let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
                assert_eq!(rows.len(), 204);
                assert!(!rows.iter().any(|row| &row[1] == "list:0"));
                assert!(rows.iter().any(|row| &row[1] == "list:204"));
                match kind {
                    PageCaptureKind::RawHtml => {
                        assert_eq!(&rows[0][6], "<p>raw, \"quoted\"\nUnicode: İ</p>")
                    }
                    PageCaptureKind::RenderedHtml => assert_eq!(&rows[0][6], ""),
                    PageCaptureKind::VisibleText => {
                        assert_eq!(&rows[0][6], "' =SUM(1,2)");
                        assert_eq!(&rows[0][7], "true");
                    }
                    PageCaptureKind::ResponseHeaders => {
                        let headers: Vec<CapturedHeader> =
                            serde_json::from_str(&rows[0][6]).unwrap();
                        assert_eq!(headers.len(), 2);
                    }
                }
            }
        }
    }

    #[test]
    fn capture_preview_returns_only_selected_kind_and_marks_utf8_bounds() {
        let store = ActiveStore::memory();
        captured(&store, 1);
        let mut capture = store.try_page_capture("list:1").unwrap().unwrap();
        capture.raw_html = Some("界".repeat(PREVIEW_BYTES));
        capture.raw_html_truncated = true;
        capture.visible_text = None;
        store
            .try_replace_page_capture("list:1", Some(capture))
            .unwrap();
        let result = preview(&store, "list:1", PageCaptureKind::RawHtml)
            .unwrap()
            .unwrap();
        assert_eq!(result.text.len(), PREVIEW_BYTES - 1);
        assert!(result.stored_truncated && result.preview_truncated);
        let value = serde_json::to_value(result).unwrap();
        assert!(value.get("responseHeaders").is_none());
        assert!(value.get("renderedHtml").is_none());
        assert!(
            preview(&store, "list:1", PageCaptureKind::VisibleText)
                .unwrap()
                .is_none()
        );
        assert!(
            preview(&store, "missing", PageCaptureKind::RawHtml)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn capture_exports_publish_atomically_and_surface_storage_errors() {
        let state = idle_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("captures.csv");
        captured(&state.store.lock().unwrap(), 1);
        let result = export_capture_file(
            &state,
            path.clone(),
            GridQuery::default(),
            PageCaptureKind::RawHtml,
        )
        .await
        .unwrap();
        assert_eq!(result.row_count, 1);
        let before = std::fs::read(&path).unwrap();
        assert!(
            export_capture_file(
                &state,
                path.clone(),
                GridQuery::default(),
                PageCaptureKind::RawHtml
            )
            .await
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);

        let database = directory.path().join("source.sqlite3");
        *state.store.lock().unwrap() = ActiveStore::sqlite(&database).unwrap();
        captured(&state.store.lock().unwrap(), 1);
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute("DROP TABLE page_captures", [])
            .unwrap();
        let failed = directory.path().join("failed.csv");
        assert!(
            export_capture_file(
                &state,
                failed.clone(),
                GridQuery::default(),
                PageCaptureKind::RawHtml
            )
            .await
            .is_err()
        );
        assert!(!failed.exists());
    }

    #[test]
    fn archive_roundtrip_retains_captures_and_rejects_invalid_payloads_before_library_changes() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            captured(&store, 1);
            let mut output = Vec::new();
            crate::write_crawl_archive_stream(&store, 123, &mut output).unwrap();
            let archive: crate::CrawlArchive = serde_json::from_slice(&output).unwrap();
            let expected = archive.page_captures.clone();
            assert_eq!(expected.len(), 1);
            let directory = tempfile::tempdir().unwrap();
            let state = idle_state();
            crate::import_archive_into_session(&state, directory.path(), archive).unwrap();
            assert_eq!(
                state
                    .store
                    .lock()
                    .unwrap()
                    .try_page_capture("list:1")
                    .unwrap()
                    .unwrap(),
                expected[0]
            );

            let mut old: serde_json::Value = serde_json::from_slice(&output).unwrap();
            old.as_object_mut().unwrap().remove("pageCaptures");
            assert!(
                serde_json::from_value::<crate::CrawlArchive>(old)
                    .unwrap()
                    .page_captures
                    .is_empty()
            );
            for error_kind in ["oversized", "orphan", "duplicate"] {
                let mut invalid: serde_json::Value = serde_json::from_slice(&output).unwrap();
                match error_kind {
                    "oversized" => {
                        invalid["pageCaptures"][0]["rawHtml"] = "x"
                            .repeat(ferrous_frog_storage::MAX_CAPTURE_BYTES + 1)
                            .into()
                    }
                    "orphan" => invalid["pageCaptures"][0]["sourceStorageKey"] = "missing".into(),
                    _ => {
                        let duplicate = invalid["pageCaptures"][0].clone();
                        invalid["pageCaptures"]
                            .as_array_mut()
                            .unwrap()
                            .push(duplicate);
                    }
                }
                let unchanged = idle_state();
                captured(&unchanged.store.lock().unwrap(), 9);
                *unchanged.current_session_id.lock().unwrap() = Some("current".into());
                let empty = tempfile::tempdir().unwrap();
                assert!(
                    crate::import_archive_into_session(
                        &unchanged,
                        empty.path(),
                        serde_json::from_value(invalid).unwrap()
                    )
                    .is_err()
                );
                assert_eq!(
                    unchanged.current_session_id.lock().unwrap().as_deref(),
                    Some("current")
                );
                assert_eq!(unchanged.store.lock().unwrap().records().len(), 1);
                assert!(std::fs::read_dir(empty.path()).unwrap().next().is_none());
            }
        }
    }
}
