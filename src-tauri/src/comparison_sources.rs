use ferrous_frog_storage::{ActiveStore, CrawlRecord, SqliteStore};
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub(super) struct ComparisonSources {
    pub baseline: SqliteStore,
    pub current: SqliteStore,
    pub directory: tempfile::TempDir,
}

impl ComparisonSources {
    pub fn saved(baseline_path: &Path, current_path: &Path) -> Result<Self, String> {
        let directory = tempfile::tempdir()
            .map_err(|error| format!("failed to create comparison directory: {error}"))?;
        let baseline = copy_records(baseline_path, &directory.path().join("baseline.sqlite3"))?;
        let current = copy_records(current_path, &directory.path().join("current.sqlite3"))?;
        Ok(Self {
            baseline,
            current,
            directory,
        })
    }

    pub fn archive(archive_path: &Path, current: &ActiveStore) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ArchiveRecords {
            schema_version: u32,
            records: Vec<CrawlRecord>,
        }
        let file = File::open(archive_path)
            .map_err(|error| format!("failed to read crawl archive: {error}"))?;
        let archive: ArchiveRecords = serde_json::from_reader(BufReader::new(file))
            .map_err(|error| format!("invalid crawl archive: {error}"))?;
        if archive.schema_version != crate::CRAWL_ARCHIVE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported crawl archive schema version {}",
                archive.schema_version
            ));
        }
        let directory = tempfile::tempdir()
            .map_err(|error| format!("failed to create comparison directory: {error}"))?;
        let baseline = stage_records(archive.records, &directory.path().join("baseline.sqlite3"))?;
        let current_path = directory.path().join("current.sqlite3");
        // ponytail: archives and memory sources hydrate records once; use a streaming
        // archive reader if those headless inputs grow beyond available memory.
        let current = match current {
            ActiveStore::Memory(store) => stage_records(store.records(), &current_path)?,
            ActiveStore::Sqlite(store) => match store
                .try_database_path()
                .map_err(|error| error.to_string())?
            {
                Some(path) => copy_records(&path, &current_path)?,
                None => stage_records(
                    store.try_records().map_err(|error| error.to_string())?,
                    &current_path,
                )?,
            },
        };
        Ok(Self {
            baseline,
            current,
            directory,
        })
    }
}

fn attach_source(connection: &Connection, path: &Path) -> Result<(), String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("failed to find crawl database {}: {error}", path.display()))?;
    if !path.is_file() {
        return Err(format!("crawl database is not a file: {}", path.display()));
    }
    let mut uri = url::Url::from_file_path(&path)
        .map_err(|_| "crawl database path cannot be opened".to_string())?;
    uri.set_query(Some("mode=ro"));
    connection
        .execute("ATTACH DATABASE ?1 AS source", [uri.as_str()])
        .map_err(|error| format!("failed to read crawl database {}: {error}", path.display()))?;
    Ok(())
}

fn copy_records(source: &Path, destination: &Path) -> Result<SqliteStore, String> {
    {
        let mut connection = Connection::open(destination).map_err(|error| error.to_string())?;
        attach_source(&connection, source)?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        // Keep the source table's constraints, especially its integer primary key.
        // CTAS loses them, and old final_url uniqueness must reach existing migrations.
        let definition: String = transaction
            .query_row(
                "SELECT sql FROM source.sqlite_schema WHERE type = 'table' AND name = 'crawl_records'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("invalid crawl database {}: {error}", source.display()))?;
        transaction
            .execute(&definition, [])
            .and_then(|_| {
                transaction.execute(
                    "INSERT INTO main.crawl_records SELECT * FROM source.crawl_records",
                    [],
                )
            })
            .map_err(|error| format!("failed to copy comparison records: {error}"))?;
        transaction.commit().map_err(|error| error.to_string())?;
    }
    SqliteStore::open(destination)
        .map_err(|error| format!("failed to initialize comparison records: {error}"))
}

fn stage_records(mut records: Vec<CrawlRecord>, path: &Path) -> Result<SqliteStore, String> {
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for record in &records {
        if i64::try_from(record.id).is_err() {
            return Err(format!("record ID {} exceeds SQLite's range", record.id));
        }
        if !ids.insert(record.id) {
            return Err(format!(
                "duplicate record ID {} in comparison source",
                record.id
            ));
        }
        let key = if record.storage_key.trim().is_empty() {
            record.final_url.as_str()
        } else {
            record.storage_key.as_str()
        };
        if !keys.insert(key) {
            return Err(format!(
                "duplicate record storage key {key} in comparison source"
            ));
        }
    }
    records.sort_by_key(|record| {
        (
            record.list_position.map(u64::from).unwrap_or(record.id),
            record.id,
        )
    });
    let store = SqliteStore::open(path).map_err(|error| error.to_string())?;
    let mut identities = Vec::with_capacity(records.len());
    for record in records {
        let saved_id = record.id as i64;
        let inserted = store
            .try_upsert(record)
            .map_err(|error| error.to_string())?;
        identities.push((inserted.id as i64, saved_id));
    }
    // Upsert allocates IDs. Move those private IDs out of the way before restoring
    // originals, so shuffled IDs cannot collide or change List representatives.
    let mut connection = Connection::open(path).map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute("UPDATE crawl_records SET id = -id", [])
        .map_err(|error| error.to_string())?;
    {
        let mut update = transaction
            .prepare("UPDATE crawl_records SET id = ?1 WHERE id = ?2")
            .map_err(|error| error.to_string())?;
        for (inserted, saved) in identities {
            update
                .execute(params![saved, -inserted])
                .map_err(|error| error.to_string())?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{CrawlRecord, CrawlStore, CustomExtractionValue};
    use rusqlite::Connection;
    use std::fs;

    fn page(position: u32) -> CrawlRecord {
        let mut record = CrawlRecord::pending("https://example.test/same".into(), 3);
        record.storage_key = format!("list:{position}:{}", record.url);
        record.list_position = Some(position);
        record.list_duplicate_index = position;
        record.status_code = Some(200);
        record.title = Some(format!("Title {position}"));
        record.meta_description = Some("Captured description".into());
        record.content_type = Some("text/html".into());
        record.response_hash = Some(format!("raw-{position}"));
        record.content_hash = Some(format!("content-{position}"));
        record.content_hash_context = Some("text-v1:http:selection".into());
        record.title_count = Some(2);
        record.inlink_count = 7;
        record.backlink_authority = Some(23.5);
        record.analytics_revenue = Some(41.25);
        record.custom_extractions = vec![CustomExtractionValue {
            name: "Product".into(),
            values: vec!["A & B".into(), "Unicode: İ".into()],
        }];
        record
    }

    fn records(store: &SqliteStore) -> serde_json::Value {
        serde_json::to_value(store.try_records().unwrap()).unwrap()
    }

    fn write_archive(path: &Path, records: &[CrawlRecord]) {
        fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "records": records,
                "linkEdges": {"deliberately": "not a decoded edge array"},
                "imageAssets": null,
                "frontierState": ["unused"]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn saved_snapshots_include_wal_and_preserve_sources_and_record_payloads() {
        let source_directory = tempfile::tempdir().unwrap();
        let baseline_path = source_directory.path().join("baseline ?#İ.sqlite3");
        let current_path = source_directory.path().join("current.sqlite3");
        let baseline = SqliteStore::open(&baseline_path).unwrap();
        let current = SqliteStore::open(&current_path).unwrap();
        let source_connection = Connection::open(&baseline_path).unwrap();
        source_connection
            .execute_batch(
                "CREATE TABLE library_sentinel (value TEXT);
                 INSERT INTO library_sentinel VALUES ('unchanged');
                 PRAGMA wal_autocheckpoint = 0;
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        baseline.try_upsert(page(3)).unwrap();
        baseline.try_upsert(page(1)).unwrap();
        source_connection
            .execute("UPDATE crawl_records SET id = id + 10", [])
            .unwrap();
        current.try_upsert(page(5)).unwrap();
        assert_eq!(
            baseline.try_database_path().unwrap(),
            Some(baseline_path.clone())
        );
        let original = records(&baseline);
        let database_bytes = fs::read(&baseline_path).unwrap();
        let wal_path = baseline_path.with_file_name("baseline ?#İ.sqlite3-wal");
        let wal_bytes = fs::read(&wal_path).unwrap();
        assert!(!wal_bytes.is_empty());

        let readonly = Connection::open_in_memory().unwrap();
        attach_source(&readonly, &baseline_path).unwrap();
        assert!(readonly.is_readonly("source").unwrap());
        assert!(
            readonly
                .execute("DELETE FROM source.crawl_records", [])
                .is_err()
        );

        let snapshots = ComparisonSources::saved(&baseline_path, &current_path).unwrap();
        assert_eq!(records(&snapshots.baseline), original);
        assert_eq!(records(&snapshots.current), records(&current));
        assert_eq!(fs::read(&baseline_path).unwrap(), database_bytes);
        assert_eq!(fs::read(&wal_path).unwrap(), wal_bytes);
        let private_connection =
            Connection::open(snapshots.directory.path().join("baseline.sqlite3")).unwrap();
        assert_eq!(
            private_connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'library_sentinel'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            source_connection
                .query_row("SELECT value FROM library_sentinel", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "unchanged"
        );
        let private_directory = snapshots.directory.path().to_path_buf();
        drop(private_connection);
        drop(snapshots);
        assert!(!private_directory.exists());
        assert_eq!(records(&baseline), original);
    }

    #[test]
    fn legacy_sources_migrate_only_inside_private_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.sqlite3");
        let source = SqliteStore::open(&path).unwrap();
        source.try_upsert(page(1)).unwrap();
        drop(source);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "ALTER TABLE crawl_records DROP COLUMN content_hash;
             ALTER TABLE crawl_records DROP COLUMN content_hash_context;
             ALTER TABLE crawl_records DROP COLUMN title_count;",
            )
            .unwrap();
        let definition: String = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name = 'crawl_records'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute_batch(
                &definition
                    .replace("CREATE TABLE crawl_records", "CREATE TABLE legacy_records")
                    .replace(
                        "final_url TEXT NOT NULL,",
                        "final_url TEXT NOT NULL UNIQUE,",
                    ),
            )
            .unwrap();
        connection
            .execute_batch(
                "INSERT INTO legacy_records SELECT * FROM crawl_records;
             DROP TABLE crawl_records;
             ALTER TABLE legacy_records RENAME TO crawl_records;",
            )
            .unwrap();
        drop(connection);
        let before = fs::read(&path).unwrap();
        let snapshots = ComparisonSources::saved(&path, &path).unwrap();
        let record = snapshots.baseline.try_records().unwrap().remove(0);
        assert_eq!(record.response_hash.as_deref(), Some("raw-1"));
        assert!(record.content_hash.is_none());
        assert!(record.content_hash_context.is_none());
        assert!(record.title_count.is_none());
        // The old UNIQUE final_url constraint is removed only from the private copy.
        snapshots.baseline.try_upsert(page(2)).unwrap();
        assert_eq!(snapshots.baseline.try_records().unwrap().len(), 2);
        assert_eq!(fs::read(&path).unwrap(), before);
        let source = Connection::open(&path).unwrap();
        assert_eq!(source.query_row("SELECT COUNT(*) FROM pragma_table_info('crawl_records') WHERE name = 'content_hash'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn archive_snapshots_keep_ids_list_order_and_payloads_after_sources_change() {
        for current in [
            ActiveStore::memory(),
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ] {
            if let ActiveStore::Sqlite(store) = &current {
                assert!(store.try_database_path().unwrap().is_none());
            }
            let directory = tempfile::tempdir().unwrap();
            let archive_path = directory.path().join("capture.ffcrawl.json");
            let mut later = page(7);
            later.id = 9;
            let mut first = page(1);
            first.id = 13;
            write_archive(&archive_path, &[later.clone(), first.clone()]);
            current.upsert(page(8));
            let expected_current = serde_json::to_value(current.records()).unwrap();
            let snapshots = ComparisonSources::archive(&archive_path, &current).unwrap();
            fs::write(&archive_path, "changed after preparation").unwrap();
            fs::remove_file(&archive_path).unwrap();
            current.clear();
            assert_eq!(
                records(&snapshots.baseline),
                serde_json::to_value([first, later]).unwrap()
            );
            assert_eq!(records(&snapshots.current), expected_current);
            let private_directory = snapshots.directory.path().to_path_buf();
            drop(snapshots);
            assert!(!private_directory.exists());
            assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
        }
    }

    #[test]
    fn archive_file_source_copies_records_without_decoding_the_full_dataset() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("current.sqlite3");
        let current = SqliteStore::open(&source_path).unwrap();
        current.try_upsert(page(1)).unwrap();
        let connection = Connection::open(&source_path).unwrap();
        connection
            .execute(
                "UPDATE crawl_records SET response_time_ms = 'invalid-integer'",
                [],
            )
            .unwrap();
        let archive_path = directory.path().join("baseline.ffcrawl.json");
        let mut baseline = page(2);
        baseline.id = 5;
        write_archive(&archive_path, &[baseline]);
        let snapshots =
            ComparisonSources::archive(&archive_path, &ActiveStore::Sqlite(current)).unwrap();
        assert_eq!(snapshots.baseline.try_records().unwrap().len(), 1);
        // The invalid payload survives SQL copying and fails only when requested.
        assert!(snapshots.current.try_records().is_err());
        let private = Connection::open(snapshots.directory.path().join("current.sqlite3")).unwrap();
        assert_eq!(
            private
                .query_row("SELECT response_time_ms FROM crawl_records", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "invalid-integer"
        );
    }

    #[test]
    fn invalid_sources_are_rejected_without_creating_or_replacing_source_files() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing.sqlite3");
        assert!(ComparisonSources::saved(&missing, &missing).is_err());
        assert!(!missing.exists());
        let unrelated = directory.path().join("unrelated.sqlite3");
        let connection = Connection::open(&unrelated).unwrap();
        connection
            .execute("CREATE TABLE unrelated (id INTEGER)", [])
            .unwrap();
        drop(connection);
        let before = fs::read(&unrelated).unwrap();
        assert!(ComparisonSources::saved(&unrelated, &unrelated).is_err());
        assert_eq!(fs::read(&unrelated).unwrap(), before);
        let invalid = directory.path().join("invalid.sqlite3");
        fs::write(&invalid, "not a database").unwrap();
        assert!(ComparisonSources::saved(&invalid, &invalid).is_err());
        assert_eq!(fs::read_to_string(&invalid).unwrap(), "not a database");

        let archive = directory.path().join("invalid.ffcrawl.json");
        let current = ActiveStore::memory();
        fs::write(&archive, r#"{"schemaVersion":2,"records":[]}"#).unwrap();
        assert!(
            ComparisonSources::archive(&archive, &current)
                .err()
                .unwrap()
                .contains("schema version 2")
        );
        let mut duplicate = page(2);
        duplicate.id = 1;
        let mut first = page(1);
        first.id = 1;
        write_archive(&archive, &[first, duplicate]);
        assert!(
            ComparisonSources::archive(&archive, &current)
                .err()
                .unwrap()
                .contains("duplicate record ID")
        );
        let mut first = page(1);
        first.id = 1;
        let mut duplicate_key = first.clone();
        duplicate_key.id = 2;
        write_archive(&archive, &[first, duplicate_key]);
        assert!(
            ComparisonSources::archive(&archive, &current)
                .err()
                .unwrap()
                .contains("duplicate record storage key")
        );
        assert!(current.records().is_empty());
    }
}
