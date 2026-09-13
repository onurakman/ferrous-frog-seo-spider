use crate::*;

pub const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
pub const MAX_CAPTURE_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_CAPTURE_HEADERS: usize = 512;
pub const MAX_CAPTURE_PAGE_SIZE: usize = 16;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapturedHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageCapture {
    pub source_storage_key: String,
    pub source_url: String,
    pub final_url: String,
    pub response_headers: Option<Vec<CapturedHeader>>,
    pub raw_html: Option<String>,
    pub rendered_html: Option<String>,
    pub visible_text: Option<String>,
    #[serde(default)]
    pub raw_html_truncated: bool,
    #[serde(default)]
    pub rendered_html_truncated: bool,
    #[serde(default)]
    pub visible_text_truncated: bool,
    #[serde(default)]
    pub headers_truncated: bool,
}

impl PageCapture {
    pub fn validate(&self) -> Result<(), StorageError> {
        validate_key(&self.source_storage_key)?;
        for (name, value) in [
            ("source URL", &self.source_url),
            ("final URL", &self.final_url),
        ] {
            if !url::Url::parse(value)
                .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
            {
                return Err(StorageError::InvalidPageCapture(format!(
                    "{name} must be an absolute HTTP URL"
                )));
            }
        }
        for (name, value) in [
            ("raw HTML", &self.raw_html),
            ("rendered HTML", &self.rendered_html),
            ("visible text", &self.visible_text),
        ] {
            if value
                .as_ref()
                .is_some_and(|text| text.len() > MAX_CAPTURE_BYTES)
            {
                return Err(StorageError::InvalidPageCapture(format!(
                    "{name} exceeds the 1 MiB capture limit"
                )));
            }
        }
        if let Some(headers) = &self.response_headers {
            let bytes = headers.iter().fold(0usize, |bytes, header| {
                bytes
                    .saturating_add(header.name.len())
                    .saturating_add(header.value.len())
            });
            if headers.len() > MAX_CAPTURE_HEADERS || bytes > MAX_CAPTURE_HEADER_BYTES {
                return Err(StorageError::InvalidPageCapture(
                    "response headers exceed the 512-field / 64 KiB capture limit".into(),
                ));
            }
        }
        Ok(())
    }
}

fn validate_key(key: &str) -> Result<(), StorageError> {
    if key.trim().is_empty() || key.chars().any(char::is_control) {
        return Err(StorageError::InvalidPageCapture(
            "source storage key must be non-empty and contain no control characters".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PageCaptureQuery {
    pub source_storage_key: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

impl Default for PageCaptureQuery {
    fn default() -> Self {
        Self {
            source_storage_key: None,
            offset: 0,
            limit: 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageCaptureResponse {
    pub captures: Vec<PageCapture>,
    pub total: usize,
}

pub(crate) fn initialize(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS page_captures (
            source_storage_key TEXT PRIMARY KEY NOT NULL,
            source_url TEXT NOT NULL, final_url TEXT NOT NULL,
            response_headers TEXT, raw_html TEXT, rendered_html TEXT, visible_text TEXT,
            raw_html_truncated INTEGER NOT NULL DEFAULT 0,
            rendered_html_truncated INTEGER NOT NULL DEFAULT 0,
            visible_text_truncated INTEGER NOT NULL DEFAULT 0,
            headers_truncated INTEGER NOT NULL DEFAULT 0
         );
         CREATE TRIGGER IF NOT EXISTS crawl_records_delete_capture AFTER DELETE ON crawl_records
         BEGIN DELETE FROM page_captures WHERE source_storage_key = old.storage_key; END;
         CREATE TRIGGER IF NOT EXISTS crawl_records_rekey_capture AFTER UPDATE OF storage_key ON crawl_records
         WHEN old.storage_key IS NOT new.storage_key
         BEGIN DELETE FROM page_captures WHERE source_storage_key = old.storage_key; END;",
    )?;
    Ok(())
}

impl MemoryStore {
    pub fn try_replace_page_capture(
        &self,
        key: &str,
        capture: Option<PageCapture>,
    ) -> Result<(), StorageError> {
        validate_key(key)?;
        let capture = capture.map(|mut capture| {
            capture.source_storage_key = key.to_owned();
            capture
        });
        if let Some(capture) = &capture {
            capture.validate()?;
        }
        let mut inner = self.inner.write().map_err(|_| StorageError::LockPoisoned)?;
        if let Some(capture) = capture {
            inner.page_captures.insert(key.to_owned(), capture);
        } else {
            inner.page_captures.remove(key);
        }
        Ok(())
    }

    pub fn try_page_captures(
        &self,
        query: PageCaptureQuery,
    ) -> Result<PageCaptureResponse, StorageError> {
        let inner = self.inner.read().map_err(|_| StorageError::LockPoisoned)?;
        let limit = query.limit.min(MAX_CAPTURE_PAGE_SIZE);
        if let Some(key) = &query.source_storage_key {
            validate_key(key)?;
            let capture = inner.page_captures.get(key);
            return Ok(PageCaptureResponse {
                total: usize::from(capture.is_some()),
                captures: capture
                    .filter(|_| query.offset == 0 && limit > 0)
                    .cloned()
                    .into_iter()
                    .collect(),
            });
        }
        Ok(PageCaptureResponse {
            total: inner.page_captures.len(),
            captures: inner
                .page_captures
                .values()
                .skip(query.offset)
                .take(limit)
                .cloned()
                .collect(),
        })
    }
}

impl SqliteStore {
    pub fn try_replace_page_capture(
        &self,
        key: &str,
        capture: Option<PageCapture>,
    ) -> Result<(), StorageError> {
        validate_key(key)?;
        let connection = self.connection()?;
        let Some(mut capture) = capture else {
            connection.execute(
                "DELETE FROM page_captures WHERE source_storage_key = ?1",
                [key],
            )?;
            return Ok(());
        };
        capture.source_storage_key = key.to_owned();
        capture.validate()?;
        let headers = capture
            .response_headers
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        connection.execute(
            "INSERT INTO page_captures
             (source_storage_key, source_url, final_url, response_headers, raw_html, rendered_html,
              visible_text, raw_html_truncated, rendered_html_truncated, visible_text_truncated, headers_truncated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(source_storage_key) DO UPDATE SET
              source_url = excluded.source_url, final_url = excluded.final_url,
              response_headers = excluded.response_headers, raw_html = excluded.raw_html,
              rendered_html = excluded.rendered_html, visible_text = excluded.visible_text,
              raw_html_truncated = excluded.raw_html_truncated, rendered_html_truncated = excluded.rendered_html_truncated,
              visible_text_truncated = excluded.visible_text_truncated, headers_truncated = excluded.headers_truncated",
            params![key, capture.source_url, capture.final_url, headers, capture.raw_html, capture.rendered_html,
                capture.visible_text, capture.raw_html_truncated, capture.rendered_html_truncated,
                capture.visible_text_truncated, capture.headers_truncated],
        )?;
        Ok(())
    }

    pub fn try_page_captures(
        &self,
        query: PageCaptureQuery,
    ) -> Result<PageCaptureResponse, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut parameters = Vec::<Value>::new();
        let filter = if let Some(key) = query.source_storage_key {
            validate_key(&key)?;
            parameters.push(Value::Text(key));
            " WHERE source_storage_key = ?"
        } else {
            ""
        };
        let total = transaction.query_row(
            &format!("SELECT COUNT(*) FROM page_captures{filter}"),
            rusqlite::params_from_iter(parameters.iter()),
            |row| row.get::<_, i64>(0),
        )? as usize;
        parameters.push(Value::Integer(query.limit.min(MAX_CAPTURE_PAGE_SIZE) as i64));
        parameters.push(Value::Integer(
            i64::try_from(query.offset).unwrap_or(i64::MAX),
        ));
        let captures = transaction
            .prepare(&format!(
                "SELECT * FROM page_captures{filter} ORDER BY source_storage_key LIMIT ? OFFSET ?"
            ))?
            .query_map(
                rusqlite::params_from_iter(parameters.iter()),
                capture_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        transaction.commit()?;
        Ok(PageCaptureResponse { captures, total })
    }
}

impl ActiveStore {
    pub fn try_replace_page_capture(
        &self,
        key: &str,
        capture: Option<PageCapture>,
    ) -> Result<(), StorageError> {
        match self {
            Self::Memory(store) => store.try_replace_page_capture(key, capture),
            Self::Sqlite(store) => store.try_replace_page_capture(key, capture),
        }
    }

    pub fn try_page_captures(
        &self,
        query: PageCaptureQuery,
    ) -> Result<PageCaptureResponse, StorageError> {
        match self {
            Self::Memory(store) => store.try_page_captures(query),
            Self::Sqlite(store) => store.try_page_captures(query),
        }
    }

    pub fn try_page_capture(&self, key: &str) -> Result<Option<PageCapture>, StorageError> {
        Ok(self
            .try_page_captures(PageCaptureQuery {
                source_storage_key: Some(key.to_owned()),
                ..Default::default()
            })?
            .captures
            .pop())
    }
}

fn capture_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PageCapture> {
    Ok(PageCapture {
        source_storage_key: row.get("source_storage_key")?,
        source_url: row.get("source_url")?,
        final_url: row.get("final_url")?,
        response_headers: json_column(row, "response_headers")?,
        raw_html: row.get("raw_html")?,
        rendered_html: row.get("rendered_html")?,
        visible_text: row.get("visible_text")?,
        raw_html_truncated: row.get("raw_html_truncated")?,
        rendered_html_truncated: row.get("rendered_html_truncated")?,
        visible_text_truncated: row.get("visible_text_truncated")?,
        headers_truncated: row.get("headers_truncated")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(key: &str) -> PageCapture {
        PageCapture {
            source_storage_key: key.into(),
            source_url: "https://example.test/source".into(),
            final_url: "https://example.test/final".into(),
            raw_html: Some("<p>café</p>".into()),
            visible_text: Some(String::new()),
            response_headers: Some(vec![
                CapturedHeader {
                    name: "link".into(),
                    value: "first".into(),
                },
                CapturedHeader {
                    name: "link".into(),
                    value: "second".into(),
                },
            ]),
            raw_html_truncated: true,
            ..Default::default()
        }
    }

    #[test]
    fn capture_replacement_queries_and_clear_preserve_exact_list_occurrences() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            for position in 0..20 {
                let key = format!("list:{position:02}");
                store
                    .try_replace_page_capture(&key, Some(capture("overridden")))
                    .unwrap();
            }
            let page = store
                .try_page_captures(PageCaptureQuery {
                    limit: usize::MAX,
                    offset: 2,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!((page.total, page.captures.len()), (20, 16));
            assert_eq!(page.captures[0].source_storage_key, "list:02");
            let first = store.try_page_capture("list:01").unwrap().unwrap();
            assert_eq!(first, capture("list:01"));
            store.try_replace_page_capture("list:01", None).unwrap();
            assert!(store.try_page_capture("list:01").unwrap().is_none());
            assert!(store.try_page_capture("list:02").unwrap().is_some());
            assert!(
                store
                    .try_page_captures(PageCaptureQuery {
                        offset: usize::MAX,
                        ..Default::default()
                    })
                    .unwrap()
                    .captures
                    .is_empty()
            );
            assert!(
                store
                    .try_page_captures(PageCaptureQuery {
                        limit: 0,
                        ..Default::default()
                    })
                    .unwrap()
                    .captures
                    .is_empty()
            );
            store.clear();
            assert_eq!(
                store
                    .try_page_captures(PageCaptureQuery::default())
                    .unwrap()
                    .total,
                0
            );
        }
    }

    #[test]
    fn oversized_or_invalid_capture_cannot_replace_existing_evidence() {
        for store in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            store
                .try_replace_page_capture("one", Some(capture("one")))
                .unwrap();
            let mut oversized = capture("one");
            oversized.raw_html = Some("x".repeat(MAX_CAPTURE_BYTES + 1));
            assert!(
                store
                    .try_replace_page_capture("one", Some(oversized))
                    .is_err()
            );
            let mut oversized = capture("one");
            oversized.response_headers = Some(vec![CapturedHeader {
                name: "x".into(),
                value: "x".repeat(MAX_CAPTURE_HEADER_BYTES),
            }]);
            assert!(
                store
                    .try_replace_page_capture("one", Some(oversized))
                    .is_err()
            );
            assert!(
                store
                    .try_replace_page_capture("\n", Some(capture("one")))
                    .is_err()
            );
            assert_eq!(
                store.try_page_capture("one").unwrap().unwrap(),
                capture("one")
            );
        }
    }

    #[test]
    fn sqlite_capture_reads_are_bounded_and_failed_replacement_rolls_back() {
        let sqlite = SqliteStore::in_memory().unwrap();
        sqlite
            .try_replace_page_capture("one", Some(capture("one")))
            .unwrap();
        sqlite
            .try_replace_page_capture("two", Some(capture("two")))
            .unwrap();
        sqlite.connection().unwrap().execute_batch(
            "UPDATE page_captures SET response_headers = 'invalid-json' WHERE source_storage_key = 'two';
             CREATE TRIGGER fail_capture_update BEFORE UPDATE ON page_captures
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        ).unwrap();
        assert_eq!(
            sqlite
                .try_page_captures(PageCaptureQuery::default())
                .unwrap()
                .captures,
            [capture("one")]
        );
        assert!(
            sqlite
                .try_page_captures(PageCaptureQuery {
                    offset: 1,
                    ..Default::default()
                })
                .is_err()
        );
        assert!(
            sqlite
                .try_replace_page_capture(
                    "one",
                    Some(PageCapture {
                        raw_html: None,
                        ..capture("one")
                    })
                )
                .is_err()
        );
        assert_eq!(
            sqlite
                .try_page_captures(PageCaptureQuery::default())
                .unwrap()
                .captures,
            [capture("one")]
        );
    }

    #[test]
    fn sqlite_capture_reopens_and_record_removal_clears_only_its_evidence() {
        let path = std::env::temp_dir().join(format!(
            "ferrous-frog-captures-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sqlite = SqliteStore::open(&path).unwrap();
        let mut record = CrawlRecord::pending("https://example.test/source".into(), 0);
        record.storage_key = "one".into();
        sqlite.try_upsert(record).unwrap();
        sqlite
            .try_replace_page_capture("one", Some(capture("one")))
            .unwrap();
        sqlite
            .try_replace_page_capture("two", Some(capture("two")))
            .unwrap();
        drop(sqlite);
        let sqlite = SqliteStore::open(&path).unwrap();
        assert_eq!(
            sqlite
                .try_page_captures(PageCaptureQuery::default())
                .unwrap()
                .captures,
            [capture("one")]
        );
        sqlite
            .connection()
            .unwrap()
            .execute("DELETE FROM crawl_records WHERE storage_key = 'one'", [])
            .unwrap();
        assert_eq!(
            sqlite
                .try_page_captures(PageCaptureQuery::default())
                .unwrap()
                .captures,
            [capture("two")]
        );
        drop(sqlite);
        std::fs::remove_file(path).unwrap();
    }
}
