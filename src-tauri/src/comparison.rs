use crate::comparison_sources::ComparisonSources;
use crate::{CrawlComparisonResponse, CrawlComparisonRow, sessions};
use ferrous_frog_storage::{ActiveStore, CrawlRecord, SqliteStore};
use rusqlite::{Connection, OptionalExtension, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct ComparisonQuery {
    pub search: String,
    pub change: String,
    pub changed_field: Option<String>,
    pub include_response_only: bool,
    pub sort_by: String,
    pub sort_dir: String,
    pub offset: usize,
    pub limit: usize,
}

impl Default for ComparisonQuery {
    fn default() -> Self {
        Self {
            search: String::new(),
            change: "all".into(),
            changed_field: None,
            include_response_only: false,
            sort_by: "url".into(),
            sort_dir: "asc".into(),
            offset: 0,
            limit: 100,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ComparisonWorkspaceRow {
    pub key: i64,
    #[serde(flatten)]
    pub row: CrawlComparisonRow,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ComparisonPage {
    pub summary: CrawlComparisonResponse,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub rows: Vec<ComparisonWorkspaceRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ComparisonDetail {
    pub row: ComparisonWorkspaceRow,
    pub previous: Option<CrawlRecord>,
    pub current: Option<CrawlRecord>,
}

pub(super) struct ComparisonWorkspace {
    // Close attached database connections before deleting the source directory.
    connection: Connection,
    summary: CrawlComparisonResponse,
    sources: ComparisonSources,
}

impl ComparisonWorkspace {
    pub fn new(sources: ComparisonSources) -> Result<Self, String> {
        let connection = Connection::open(sources.directory.path().join("comparison.sqlite3"))
            .map_err(database_error)?;
        for (schema, store) in [
            ("baseline", &sources.baseline),
            ("current", &sources.current),
        ] {
            let path = store
                .try_database_path()
                .map_err(|error| error.to_string())?
                .ok_or("comparison source must be a private database file")?;
            let mut uri = url::Url::from_file_path(path)
                .map_err(|_| "comparison source path cannot be opened")?;
            uri.set_query(Some("mode=ro"));
            connection
                .execute(&format!("ATTACH DATABASE ?1 AS {schema}"), [uri.as_str()])
                .map_err(database_error)?;
        }
        sessions::register_comparison_functions(&connection).map_err(database_error)?;
        let baseline_sql =
            sessions::comparison_projection(&connection, "baseline").map_err(database_error)?;
        let current_sql =
            sessions::comparison_projection(&connection, "current").map_err(database_error)?;
        let cte = sessions::comparison_cte(&baseline_sql, &current_sql);
        connection.execute_batch("BEGIN").map_err(database_error)?;
        connection
            .execute_batch(&format!(
                "CREATE TABLE comparison_rows AS {cte}
             SELECT ROW_NUMBER() OVER (ORDER BY url, occurrence) AS key, *,
                CASE change_order WHEN 0 THEN 'added' WHEN 1 THEN 'removed'
                    WHEN 2 THEN 'changed' WHEN 3 THEN 'responseOnly' ELSE 'unchanged' END AS change
             FROM classified"
            ))
            .map_err(database_error)?;
        let mut summary = connection
            .query_row(
                &format!(
                    "SELECT (SELECT COUNT(*) FROM baseline.crawl_records),
                (SELECT COUNT(*) FROM current.crawl_records), {}
             FROM comparison_rows",
                    sessions::COMPARISON_COUNTS_SQL,
                ),
                [],
                sessions::comparison_counts_from_row,
            )
            .map_err(database_error)?;
        summary.metric_deltas = crate::comparison_metric_deltas(
            &sessions::comparison_summary(&connection, &baseline_sql).map_err(database_error)?,
            &sessions::comparison_summary(&connection, &current_sql).map_err(database_error)?,
        );
        // Counts include unchanged rows with unavailable evidence; only changes need paging.
        connection
            .execute_batch(
                "DELETE FROM comparison_rows WHERE change_order = 4;
             CREATE UNIQUE INDEX comparison_rows_key ON comparison_rows(key);
             CREATE INDEX comparison_rows_url ON comparison_rows(url, occurrence);
             CREATE INDEX comparison_rows_change ON comparison_rows(change_order, url, occurrence);
             COMMIT;
             DETACH DATABASE baseline;
             DETACH DATABASE current;
             PRAGMA query_only = ON;",
            )
            .map_err(database_error)?;
        Ok(Self {
            connection,
            summary,
            sources,
        })
    }

    pub fn query(&self, query: &ComparisonQuery) -> Result<ComparisonPage, String> {
        let (filter, order, mut parameters) = query_sql(query)?;
        let total = self
            .connection
            .query_row(
                &format!("SELECT COUNT(*) FROM comparison_rows WHERE {filter}"),
                params_from_iter(parameters.iter()),
                |row| row.get::<_, i64>(0),
            )
            .map_err(database_error)? as usize;
        let limit = query.limit.clamp(1, 500);
        parameters.push(Value::Integer(limit as i64));
        parameters.push(Value::Integer(
            i64::try_from(query.offset).unwrap_or(i64::MAX),
        ));
        let rows = self
            .connection
            .prepare(&format!(
                "SELECT * FROM comparison_rows WHERE {filter} ORDER BY {order} LIMIT ? OFFSET ?"
            ))
            .map_err(database_error)?
            .query_map(params_from_iter(parameters.iter()), workspace_row_from_sql)
            .map_err(database_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(database_error)?;
        Ok(ComparisonPage {
            summary: self.summary.clone(),
            total,
            offset: query.offset,
            limit,
            rows,
        })
    }

    pub fn detail(&self, key: i64) -> Result<ComparisonDetail, String> {
        let (row, previous_id, current_id) = self
            .connection
            .query_row(
                "SELECT * FROM comparison_rows WHERE key = ?1",
                [key],
                |row| {
                    Ok((
                        workspace_row_from_sql(row)?,
                        row.get::<_, Option<i64>>("previous_id")?,
                        row.get::<_, Option<i64>>("current_id")?,
                    ))
                },
            )
            .optional()
            .map_err(database_error)?
            .ok_or("comparison row is unavailable")?;
        let record =
            |store: &SqliteStore, id: Option<i64>| -> Result<Option<CrawlRecord>, String> {
                let Some(id) = id else { return Ok(None) };
                let id = u64::try_from(id).map_err(|_| "comparison record ID is invalid")?;
                let mut records = ActiveStore::Sqlite(store.clone())
                    .try_records_by_ids(&[id])
                    .map_err(|error| error.to_string())?;
                records
                    .pop()
                    .map(Some)
                    .ok_or_else(|| "comparison source record is unavailable".into())
            };
        Ok(ComparisonDetail {
            row,
            previous: record(&self.sources.baseline, previous_id)?,
            current: record(&self.sources.current, current_id)?,
        })
    }

    pub fn write_csv(
        &self,
        query: &ComparisonQuery,
        writer: &mut dyn Write,
    ) -> Result<usize, String> {
        let (filter, order, parameters) = query_sql(query)?;
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT * FROM comparison_rows WHERE {filter} ORDER BY {order}"
            ))
            .map_err(database_error)?;
        let rows = statement
            .query_map(
                params_from_iter(parameters.iter()),
                sessions::comparison_row_from_sql,
            )
            .map_err(database_error)?;
        let mut writer = csv::Writer::from_writer(writer);
        writer
            .write_record([
                "URL",
                "Identity Key",
                "Occurrence",
                "Change",
                "Changed Fields",
                "Content Comparison",
                "Previous URL",
                "Current URL",
                "Previous Final URL",
                "Current Final URL",
                "Previous List Position",
                "Current List Position",
                "Previous Status Code",
                "Current Status Code",
                "Previous Title",
                "Current Title",
                "Previous Meta Description",
                "Current Meta Description",
                "Previous Indexability",
                "Current Indexability",
                "Previous Response Hash",
                "Current Response Hash",
            ])
            .map_err(|error| error.to_string())?;
        let mut count = 0;
        for row in rows {
            let row = row.map_err(database_error)?;
            let cells = [
                row.url,
                row.identity_key,
                row.occurrence.to_string(),
                row.change,
                row.changed_fields.join("; "),
                row.content_comparison,
                row.previous_url.unwrap_or_default(),
                row.current_url.unwrap_or_default(),
                row.previous_final_url.unwrap_or_default(),
                row.current_final_url.unwrap_or_default(),
                row.previous_list_position
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.current_list_position
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.previous_status_code
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.current_status_code
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.previous_title.unwrap_or_default(),
                row.current_title.unwrap_or_default(),
                row.previous_meta_description.unwrap_or_default(),
                row.current_meta_description.unwrap_or_default(),
                row.previous_indexability.unwrap_or_default(),
                row.current_indexability.unwrap_or_default(),
                row.previous_response_hash.unwrap_or_default(),
                row.current_response_hash.unwrap_or_default(),
            ]
            .map(|value| {
                if value.trim_start().starts_with(['=', '+', '-', '@']) {
                    format!("'{value}")
                } else {
                    value
                }
            });
            writer
                .write_record(cells)
                .map_err(|error| error.to_string())?;
            count += 1;
        }
        writer.flush().map_err(|error| error.to_string())?;
        Ok(count)
    }
}

fn database_error(error: rusqlite::Error) -> String {
    format!("comparison database failed: {error}")
}

fn workspace_row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<ComparisonWorkspaceRow> {
    Ok(ComparisonWorkspaceRow {
        key: row.get("key")?,
        row: sessions::comparison_row_from_sql(row)?,
    })
}

fn query_sql(query: &ComparisonQuery) -> Result<(String, String, Vec<Value>), String> {
    let change = match query.change.as_str() {
        "all" => None,
        "added" => Some(0),
        "removed" => Some(1),
        "changed" => Some(2),
        "responseOnly" => Some(3),
        _ => return Err("unsupported comparison change filter".into()),
    };
    let mut clauses = Vec::new();
    if let Some(change) = change {
        clauses.push(format!("change_order = {change}"));
    } else if !query.include_response_only {
        clauses.push("change_order != 3".into());
    }
    if let Some(field) = query.changed_field.as_deref() {
        let column = match field {
            "finalUrl" => "final_url_diff",
            "statusCode" => "status_diff",
            "title" => "title_diff",
            "metaDescription" => "meta_diff",
            "indexability" => "indexability_diff",
            "headings" => "headings_diff",
            "canonical" => "canonical_diff",
            "robotsDirectives" => "robots_diff",
            "content" => "content_diff",
            "responseHash" => "hash_diff",
            _ => return Err("unsupported comparison changed-field filter".into()),
        };
        clauses.push(format!("{column} = 1"));
    }
    let mut parameters = Vec::new();
    if !query.search.trim().is_empty() {
        let search = [
            "url",
            "previous_url",
            "current_url",
            "previous_final_url",
            "current_final_url",
            "previous_title",
            "current_title",
            "previous_meta_description",
            "current_meta_description",
        ]
        .map(|column| {
            format!("instr(ff_comparison_text_key({column}), ff_comparison_text_key(?1)) > 0")
        })
        .join(" OR ");
        clauses.push(format!("({search})"));
        parameters.push(Value::Text(query.search.clone()));
    }
    let sort = match query.sort_by.as_str() {
        "url" => "url",
        "change" => "change_order",
        "previousStatusCode" => "previous_status_code",
        "currentStatusCode" => "current_status_code",
        "previousTitle" => "previous_title",
        "currentTitle" => "current_title",
        _ => return Err("unsupported comparison sort field".into()),
    };
    let direction = match query.sort_dir.as_str() {
        "asc" => "ASC",
        "desc" => "DESC",
        _ => return Err("unsupported comparison sort direction".into()),
    };
    let filter = if clauses.is_empty() {
        "1".into()
    } else {
        clauses.join(" AND ")
    };
    let order = if sort == "url" {
        format!("url {direction}, occurrence ASC")
    } else {
        format!("{sort} {direction}, url ASC, occurrence ASC")
    };
    Ok((filter, order, parameters))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{CustomExtractionValue, SqliteStore};

    fn sources() -> ComparisonSources {
        let directory = tempfile::tempdir().unwrap();
        ComparisonSources {
            baseline: SqliteStore::open(directory.path().join("baseline.sqlite3")).unwrap(),
            current: SqliteStore::open(directory.path().join("current.sqlite3")).unwrap(),
            directory,
        }
    }

    fn record(path: &str) -> CrawlRecord {
        let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 0);
        record.status_code = Some(200);
        record.content_type = Some("text/html".into());
        record.indexability = "Indexable".into();
        record.indexability_status = "Indexable".into();
        record.title = Some("Previous title".into());
        record.meta_description = Some("Captured description".into());
        record.response_hash = Some("raw-before".into());
        record.content_hash = Some("stable-content".into());
        record.content_hash_context = Some("html-text-v1:http".into());
        record
    }

    #[test]
    fn pages_filters_and_csv_reach_every_change_without_response_noise() {
        let sources = sources();
        for position in 0..1_105 {
            let old = record(&format!("change-{position:04}"));
            let mut new = old.clone();
            new.title = Some(format!("Updated title {position:04}"));
            sources.baseline.try_upsert(old).unwrap();
            sources.current.try_upsert(new).unwrap();
        }
        for position in 0..1_005 {
            let old = record(&format!("noise-{position:04}"));
            let mut new = old.clone();
            new.response_hash = Some("new-nonce".into());
            sources.baseline.try_upsert(old).unwrap();
            sources.current.try_upsert(new).unwrap();
        }
        let mut legacy = record("legacy-unchanged");
        legacy.content_hash = None;
        sources.baseline.try_upsert(legacy.clone()).unwrap();
        sources.current.try_upsert(legacy).unwrap();
        let workspace = ComparisonWorkspace::new(sources).unwrap();
        let first = workspace.query(&ComparisonQuery::default()).unwrap();
        assert_eq!((first.total, first.rows.len()), (1_105, 100));
        assert_eq!(
            (
                first.summary.changed,
                first.summary.response_only,
                first.summary.content_unavailable
            ),
            (1_105, 1_005, 1)
        );
        assert!(first.summary.rows.is_empty());
        let last = workspace
            .query(&ComparisonQuery {
                offset: 1_000,
                limit: 999,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            (last.offset, last.limit, last.rows.len()),
            (1_000, 500, 105)
        );
        assert!(last.rows[0].row.url.ends_with("change-1000"));
        let query = ComparisonQuery {
            search: "UPDATED TITLE 10".into(),
            changed_field: Some("title".into()),
            sort_by: "currentTitle".into(),
            sort_dir: "desc".into(),
            offset: 11,
            limit: 7,
            ..Default::default()
        };
        let filtered = workspace.query(&query).unwrap();
        assert_eq!((filtered.total, filtered.rows.len()), (100, 7));
        assert!(filtered.rows[0].row.url.ends_with("change-1088"));
        assert_eq!(filtered.summary.changed, 1_105);
        let mut csv = Vec::new();
        assert_eq!(workspace.write_csv(&query, &mut csv).unwrap(), 100);
        let mut reader = csv::Reader::from_reader(csv.as_slice());
        let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(rows.len(), 100);
        assert!(rows[0][0].ends_with("change-1099"));
        assert!(rows[99][0].ends_with("change-1000"));
        let noise = workspace
            .query(&ComparisonQuery {
                change: "responseOnly".into(),
                offset: 1_000,
                ..Default::default()
            })
            .unwrap();
        assert_eq!((noise.total, noise.rows.len()), (1_005, 5));
        assert!(
            noise
                .rows
                .iter()
                .all(|row| row.row.change == "responseOnly")
        );
    }

    #[test]
    fn prepared_results_preserve_hash_and_unavailable_semantics() {
        let sources = sources();
        for path in [
            "unchanged",
            "nonce",
            "content",
            "metadata",
            "status",
            "raw-gained",
            "context-mismatch",
            "legacy",
            "counts-gained",
        ] {
            let mut old = record(path);
            let mut new = old.clone();
            match path {
                "nonce" => new.response_hash = Some("new-nonce".into()),
                "content" => new.content_hash = Some("edited-content".into()),
                "metadata" => {
                    new.meta_description = Some("Edited description".into());
                    new.h1 = Some("Edited heading".into());
                    new.canonical = Some("https://example.test/canonical".into());
                    new.meta_robots = Some("nofollow".into());
                }
                "status" => new.status_code = Some(404),
                "raw-gained" => old.response_hash = None,
                "context-mismatch" => {
                    new.content_hash = Some("different-selection".into());
                    new.content_hash_context = Some("another-context".into());
                    new.response_hash = Some("different-response".into());
                }
                "legacy" => {
                    old.content_hash = None;
                    new.content_hash = None;
                    old.content_hash_context = None;
                    new.content_hash_context = None;
                }
                "counts-gained" => {
                    new.title_count = Some(1);
                    new.meta_description_count = Some(1);
                }
                _ => {}
            }
            sources.baseline.try_upsert(old).unwrap();
            sources.current.try_upsert(new).unwrap();
        }
        sources.baseline.try_upsert(record("removed")).unwrap();
        sources.current.try_upsert(record("added")).unwrap();
        let old = sources.baseline.try_records().unwrap();
        let new = sources.current.try_records().unwrap();
        // Paging must not decode unrelated captured evidence.
        Connection::open(sources.directory.path().join("baseline.sqlite3"))
            .unwrap()
            .execute("UPDATE crawl_records SET page_speed = 'invalid json'", [])
            .unwrap();
        let workspace = ComparisonWorkspace::new(sources).unwrap();
        for include_response_only in [false, true] {
            let page = workspace
                .query(&ComparisonQuery {
                    include_response_only,
                    sort_by: "change".into(),
                    ..Default::default()
                })
                .unwrap();
            let mut result = page.summary;
            result.rows = page.rows.into_iter().map(|row| row.row).collect();
            assert_eq!(
                serde_json::to_value(result).unwrap(),
                serde_json::to_value(crate::compare_records(&old, &new, include_response_only))
                    .unwrap()
            );
        }
    }

    #[test]
    fn detail_uses_the_compared_occurrence_and_preserves_captured_evidence() {
        let sources = sources();
        let mut representative = record("same-final-url");
        representative.storage_key = "list:9".into();
        representative.list_position = Some(9);
        representative.title_count = Some(2);
        representative.custom_extractions = vec![CustomExtractionValue {
            name: "Product".into(),
            values: vec!["A & B".into()],
        }];
        let previous = sources.baseline.try_upsert(representative.clone()).unwrap();
        representative.h1 = Some("Edited heading".into());
        let current = sources.current.try_upsert(representative).unwrap();
        let mut earlier = record("same-final-url");
        earlier.storage_key = "list:1".into();
        earlier.list_position = Some(1);
        sources.baseline.try_upsert(earlier.clone()).unwrap();
        sources.current.try_upsert(earlier).unwrap();
        sources.baseline.try_upsert(record("removed")).unwrap();
        sources.current.try_upsert(record("added")).unwrap();
        let workspace = ComparisonWorkspace::new(sources).unwrap();
        let page = workspace.query(&ComparisonQuery::default()).unwrap();
        assert_eq!(page.summary.baseline_records, 3);
        assert_eq!(page.total, 3);
        for row in page.rows {
            let detail = workspace.detail(row.key).unwrap();
            assert_eq!(detail.row.key, row.key);
            match row.row.change.as_str() {
                "added" => assert!(detail.previous.is_none() && detail.current.is_some()),
                "removed" => assert!(detail.previous.is_some() && detail.current.is_none()),
                _ => {
                    let old = detail.previous.unwrap();
                    let new = detail.current.unwrap();
                    assert_eq!((old.id, new.id), (previous.id, current.id));
                    assert_eq!((old.list_position, new.list_position), (Some(9), Some(9)));
                    assert_eq!(detail.row.row.occurrence, 2);
                    assert_eq!(old.title_count, Some(2));
                    assert_eq!(old.custom_extractions, previous.custom_extractions);
                    assert_eq!(new.h1, current.h1);
                }
            }
        }
        assert!(workspace.detail(-1).is_err());
        assert!(workspace.detail(i64::MAX).is_err());
    }

    #[test]
    fn search_is_literal_csv_is_escaped_and_query_inputs_are_validated() {
        let sources = sources();
        let old = record("literal");
        let mut new = old.clone();
        new.title = Some("  =SUM(1,2)".into());
        new.meta_description = Some("Ünicode %_ marker, \"quoted\"\nnext line".into());
        sources.baseline.try_upsert(old).unwrap();
        sources.current.try_upsert(new).unwrap();
        let old = record("other");
        let mut new = old.clone();
        new.title = Some("Edited".into());
        sources.baseline.try_upsert(old).unwrap();
        sources.current.try_upsert(new).unwrap();
        let workspace = ComparisonWorkspace::new(sources).unwrap();
        let defaults: ComparisonQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(
            (
                defaults.limit,
                defaults.change.as_str(),
                defaults.sort_by.as_str()
            ),
            (100, "all", "url")
        );
        let query = ComparisonQuery {
            search: "ÜNICODE %_".into(),
            limit: 0,
            ..Default::default()
        };
        let page = workspace.query(&query).unwrap();
        assert_eq!((page.total, page.limit, page.rows.len()), (1, 1, 1));
        let mut output = Vec::new();
        assert_eq!(workspace.write_csv(&query, &mut output).unwrap(), 1);
        let mut reader = csv::Reader::from_reader(output.as_slice());
        let headers = reader.headers().unwrap().clone();
        let values = reader.records().next().unwrap().unwrap();
        assert_eq!(
            &values[headers
                .iter()
                .position(|value| value == "Current Title")
                .unwrap()],
            "'  =SUM(1,2)"
        );
        assert_eq!(
            &values[headers
                .iter()
                .position(|value| value == "Current Meta Description")
                .unwrap()],
            "Ünicode %_ marker, \"quoted\"\nnext line"
        );
        for invalid in [
            ComparisonQuery {
                change: "unknown".into(),
                ..Default::default()
            },
            ComparisonQuery {
                changed_field: Some("unknown".into()),
                ..Default::default()
            },
            ComparisonQuery {
                sort_by: "url; DROP TABLE comparison_rows".into(),
                ..Default::default()
            },
            ComparisonQuery {
                sort_dir: "invalid".into(),
                ..Default::default()
            },
        ] {
            assert!(workspace.query(&invalid).is_err());
            assert!(workspace.write_csv(&invalid, &mut Vec::new()).is_err());
        }
        let beyond = workspace
            .query(&ComparisonQuery {
                offset: usize::MAX,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(beyond.total, 2);
        assert!(beyond.rows.is_empty());
    }

    #[test]
    fn request_aliases_and_list_occurrences_match_across_all_comparison_paths() {
        let directory = tempfile::tempdir().unwrap();
        let index = sessions::index_connection(directory.path()).unwrap();
        let (baseline, old_store) =
            sessions::create_session(&index, directory.path(), "Old", "", None, "ready").unwrap();
        let (current, new_store) =
            sessions::create_session(&index, directory.path(), "New", "", None, "ready").unwrap();
        let list = |path: &str, position: u32, occurrence: u32| {
            let mut row = record(path);
            row.storage_key = format!("list:{position}:{}", row.url);
            row.list_position = Some(position);
            row.list_duplicate_index = occurrence;
            row
        };
        let mut alias_a = record("alias-a");
        alias_a.final_url = "https://example.test/shared".into();
        let mut alias_b = record("alias-b");
        alias_b.final_url = alias_a.final_url.clone();
        let mut redirected = alias_a.clone();
        redirected.final_url = "https://example.test/new-destination".into();
        old_store.try_upsert(alias_a).unwrap();
        old_store.try_upsert(alias_b.clone()).unwrap();
        new_store.try_upsert(alias_b).unwrap();
        new_store.try_upsert(redirected).unwrap();
        for occurrence in [3, 1, 2] {
            old_store
                .try_upsert(list("repeat", occurrence * 2, occurrence))
                .unwrap();
        }
        // Global positions and completion order change. Occurrence 2 is removed;
        // occurrence 3 must retain its identity instead of shifting down to 2.
        for occurrence in [4, 3, 1] {
            let mut row = list("repeat", occurrence * 3 + 1, occurrence);
            if occurrence == 3 {
                row.title = Some("Edited third occurrence".into());
            }
            new_store.try_upsert(row).unwrap();
        }
        for (old_position, new_position) in [(20, 50), (40, 80)] {
            old_store
                .try_upsert(list("legacy", old_position, 0))
                .unwrap();
            let mut new = list("legacy", new_position, 0);
            if old_position == 40 {
                new.status_code = Some(404);
            }
            new_store.try_upsert(new).unwrap();
        }
        // Legacy archives can retain the List position only in their storage key.
        for position in [70, 60] {
            let mut old = list("legacy-key", position, 0);
            old.list_position = None;
            let mut new = old.clone();
            new.storage_key = format!("list:{}:{}", position + 100, new.url);
            if position == 70 {
                new.meta_description = Some("Edited second occurrence".into());
            }
            old_store.try_upsert(old).unwrap();
            new_store.try_upsert(new).unwrap();
        }
        let mut previous = old_store.try_records().unwrap();
        let mut next = new_store.try_records().unwrap();
        previous.reverse();
        next.rotate_left(3);
        let expected = crate::compare_records(&previous, &next, false);
        assert_eq!(
            (expected.added, expected.removed, expected.changed),
            (1, 1, 4)
        );
        assert_eq!(expected.rows.len(), 6);
        let redirect = expected
            .rows
            .iter()
            .find(|row| row.url.ends_with("alias-a"))
            .unwrap();
        assert_eq!(redirect.changed_fields, ["finalUrl"]);
        assert_eq!(
            redirect.previous_final_url.as_deref(),
            Some("https://example.test/shared")
        );
        assert_eq!(
            redirect.current_final_url.as_deref(),
            Some("https://example.test/new-destination")
        );
        assert!(!expected.rows.iter().any(|row| row.url.ends_with("alias-b")));
        let repeated = expected
            .rows
            .iter()
            .filter(|row| row.url.ends_with("repeat"))
            .collect::<Vec<_>>();
        assert_eq!(
            repeated
                .iter()
                .map(|row| (row.change.as_str(), row.occurrence))
                .collect::<Vec<_>>(),
            [("added", 4), ("removed", 2), ("changed", 3)]
        );
        let serialized = serde_json::to_value(&expected).unwrap();
        let saved =
            sessions::compare_sessions(&index, &baseline.id, &current.id, None, false).unwrap();
        assert_eq!(serde_json::to_value(saved).unwrap(), serialized);
        previous.rotate_left(2);
        next.reverse();
        assert_eq!(
            serde_json::to_value(crate::compare_records(&previous, &next, false)).unwrap(),
            serialized
        );
        let archive_path = directory.path().join("baseline.json");
        std::fs::write(
            &archive_path,
            serde_json::to_vec(&serde_json::json!({"schemaVersion": 1, "records": previous}))
                .unwrap(),
        )
        .unwrap();
        for sources in [
            ComparisonSources::saved(
                std::path::Path::new(&baseline.database_path),
                std::path::Path::new(&current.database_path),
            )
            .unwrap(),
            ComparisonSources::archive(&archive_path, &ActiveStore::Sqlite(new_store.clone()))
                .unwrap(),
        ] {
            let workspace = ComparisonWorkspace::new(sources).unwrap();
            let page = workspace
                .query(&ComparisonQuery {
                    sort_by: "change".into(),
                    ..Default::default()
                })
                .unwrap();
            for row in &page.rows {
                let detail = workspace.detail(row.key).unwrap();
                assert_eq!(detail.row.row.identity_key, row.row.identity_key);
                if row.row.url.ends_with("repeat") && row.row.change == "changed" {
                    assert_eq!(detail.previous.unwrap().list_duplicate_index, 3);
                    assert_eq!(detail.current.unwrap().list_duplicate_index, 3);
                }
            }
            let mut response = page.summary;
            response.rows = page.rows.into_iter().map(|row| row.row).collect();
            assert_eq!(serde_json::to_value(response).unwrap(), serialized);
            let filtered = ComparisonQuery {
                changed_field: Some("finalUrl".into()),
                search: "new-destination".into(),
                ..Default::default()
            };
            assert_eq!(workspace.query(&filtered).unwrap().total, 1);
            let mut csv = Vec::new();
            assert_eq!(workspace.write_csv(&filtered, &mut csv).unwrap(), 1);
            let mut reader = csv::Reader::from_reader(csv.as_slice());
            let headers = reader.headers().unwrap().clone();
            let row = reader.records().next().unwrap().unwrap();
            assert_eq!(
                &row[headers
                    .iter()
                    .position(|value| value == "Current Final URL")
                    .unwrap()],
                "https://example.test/new-destination"
            );
        }
    }
}
