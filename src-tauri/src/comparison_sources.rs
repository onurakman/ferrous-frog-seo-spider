use crate::archive_staging::RecordStage;
use ferrous_frog_storage::{ActiveStore, CrawlRecord, SqliteStore};
use rusqlite::Connection;
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::fs::File;
use std::io::{BufReader, Read};
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
        let file = File::open(archive_path)
            .map_err(|error| format!("failed to read crawl archive: {error}"))?;
        let directory = tempfile::tempdir()
            .map_err(|error| format!("failed to create comparison directory: {error}"))?;
        Self::archive_reader(BufReader::new(file), current, directory)
    }

    fn archive_reader(
        reader: impl Read,
        current: &ActiveStore,
        directory: tempfile::TempDir,
    ) -> Result<Self, String> {
        let baseline = read_archive_records(reader, &directory.path().join("baseline.sqlite3"))?;
        let current_path = directory.path().join("current.sqlite3");
        // ponytail: Memory and in-memory SQLite current sources still hydrate records once;
        // add a stable record visitor if these headless comparison inputs need bounded staging.
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

fn stage_records(records: Vec<CrawlRecord>, path: &Path) -> Result<SqliteStore, String> {
    let mut stage = RecordStage::new(path)?;
    for record in records {
        stage.insert(record)?;
    }
    stage.finish()
}

fn read_archive_records(reader: impl Read, path: &Path) -> Result<SqliteStore, String> {
    let mut stage = RecordStage::new(path)?;
    let mut decoder = serde_json::Deserializer::from_reader(reader);
    ArchiveRecordsSeed(&mut stage)
        .deserialize(&mut decoder)
        .map_err(|error| format!("invalid crawl archive: {error}"))?;
    decoder
        .end()
        .map_err(|error| format!("invalid crawl archive: {error}"))?;
    stage.finish()
}

struct ArchiveRecordsSeed<'a>(&'a mut RecordStage);

impl<'de> DeserializeSeed<'de> for ArchiveRecordsSeed<'_> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for ArchiveRecordsSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a crawl archive object")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        let mut version = None;
        let mut records_seen = false;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "schemaVersion" => {
                    if version.is_some() {
                        return Err(de::Error::duplicate_field("schemaVersion"));
                    }
                    version = Some(map.next_value::<u32>()?);
                }
                "records" => {
                    if records_seen {
                        return Err(de::Error::duplicate_field("records"));
                    }
                    records_seen = true;
                    map.next_value_seed(RecordSequenceSeed(self.0))?;
                }
                // Comparison has always ignored other archive sections, even when their
                // values do not match the full-import schema. Skip without retaining them.
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        let version = version.ok_or_else(|| de::Error::missing_field("schemaVersion"))?;
        if !records_seen {
            return Err(de::Error::missing_field("records"));
        }
        if version != crate::CRAWL_ARCHIVE_SCHEMA_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported crawl archive schema version {version}"
            )));
        }
        Ok(())
    }
}

struct RecordSequenceSeed<'a>(&'a mut RecordStage);

impl<'de> DeserializeSeed<'de> for RecordSequenceSeed<'_> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for RecordSequenceSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an array of crawl records")
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<(), S::Error> {
        while let Some(record) = sequence.next_element::<CrawlRecord>()? {
            self.0.insert(record).map_err(de::Error::custom)?;
        }
        Ok(())
    }
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
    fn archive_records_reach_sqlite_before_eof_and_accept_metadata_after_rows() {
        use std::io::Read;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        struct TinyReader {
            bytes: std::io::Cursor<Vec<u8>>,
            probe_after: u64,
            database: std::path::PathBuf,
            observed: Arc<AtomicBool>,
        }
        impl Read for TinyReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.bytes.position() >= self.probe_after
                    && !self.observed.load(Ordering::Relaxed)
                {
                    let conn = Connection::open(&self.database).unwrap();
                    let count: i64 = conn
                        .query_row("SELECT COUNT(*) FROM crawl_records", [], |row| row.get(0))
                        .unwrap();
                    assert_eq!(
                        count, 1,
                        "first row must be persisted while later JSON is unread"
                    );
                    assert!(self.bytes.position() < self.bytes.get_ref().len() as u64);
                    self.observed.store(true, Ordering::Relaxed);
                }
                let size = buffer.len().min(7);
                self.bytes.read(&mut buffer[..size])
            }
        }
        let mut first = page(7);
        first.id = 9;
        let mut second = page(1);
        second.id = 13;
        let prefix = format!("{{\"records\":[{},", serde_json::to_string(&first).unwrap());
        let bytes = format!(
            "{prefix}{}],\"schemaVersion\":1,\"unused\":{{\"nested\":[1,2,3]}}}}",
            serde_json::to_string(&second).unwrap()
        )
        .into_bytes();
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("baseline.sqlite3");
        let observed = Arc::new(AtomicBool::new(false));
        let store = read_archive_records(
            TinyReader {
                bytes: std::io::Cursor::new(bytes),
                probe_after: prefix.len() as u64 + 40,
                database: database.clone(),
                observed: observed.clone(),
            },
            &database,
        )
        .unwrap();
        assert!(observed.load(Ordering::Relaxed));
        assert_eq!(
            records(&store),
            serde_json::to_value([second, first]).unwrap()
        );
        let connection = Connection::open(database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'comparison_record_ids'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn late_archive_failures_remove_private_stage_and_leave_current_unchanged() {
        let current = ActiveStore::memory();
        current.upsert(page(8));
        let before = serde_json::to_value(current.records()).unwrap();
        let mut first = page(1);
        first.id = 99;
        let record = serde_json::to_string(&first).unwrap();
        for document in [
            format!("{{\"schemaVersion\":1,\"records\":[{record},"),
            format!("{{\"schemaVersion\":1,\"records\":[{record}]}} trailing"),
            format!("{{\"schemaVersion\":2,\"records\":[{record}]}}"),
            format!("{{\"schemaVersion\":1,\"records\":[{record}],\"records\":[]}}"),
            format!("{{\"schemaVersion\":1,\"records\":[{record}],\"schemaVersion\":1}}"),
            format!("{{\"records\":[{record}]}}"),
            "{\"schemaVersion\":1}".into(),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().to_path_buf();
            assert!(
                ComparisonSources::archive_reader(document.as_bytes(), &current, directory)
                    .is_err()
            );
            assert!(!path.exists(), "failed private stage must be removed");
            assert_eq!(serde_json::to_value(current.records()).unwrap(), before);
        }
        struct FailedReader<R>(R);
        impl<R: Read> Read for FailedReader<R> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                match self.0.read(buffer)? {
                    0 => Err(std::io::Error::other("archive input failed before EOF")),
                    count => Ok(count),
                }
            }
        }
        let document = format!("{{\"schemaVersion\":1,\"records\":[{record}]}}");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_path_buf();
        let error = ComparisonSources::archive_reader(
            FailedReader(document.as_bytes()),
            &current,
            directory,
        )
        .err()
        .unwrap();
        assert!(error.contains("archive input failed before EOF"), "{error}");
        assert!(!path.exists());
        assert_eq!(serde_json::to_value(current.records()).unwrap(), before);
    }

    #[test]
    fn archive_identity_extremes_and_late_storage_failure_keep_private_staging_safe() {
        let current = ActiveStore::memory();
        current.upsert(page(8));
        let original = serde_json::to_value(current.records()).unwrap();
        let mut first = page(7);
        first.id = i64::MAX as u64;
        let mut second = page(1);
        second.id = 0;
        let json = serde_json::to_vec(
            &serde_json::json!({"schemaVersion": 1, "records": [first.clone(), second.clone()]}),
        )
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let sources =
            ComparisonSources::archive_reader(json.as_slice(), &current, directory).unwrap();
        assert_eq!(
            records(&sources.baseline),
            serde_json::to_value([second, first]).unwrap()
        );
        drop(sources);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_path_buf();
        fs::write(path.join("current.sqlite3"), "not a database").unwrap();
        assert!(ComparisonSources::archive_reader(json.as_slice(), &current, directory).is_err());
        assert!(
            !path.exists(),
            "late current-store failure must remove the completed baseline too"
        );
        assert_eq!(serde_json::to_value(current.records()).unwrap(), original);
        let mut overflow = page(1);
        overflow.id = u64::MAX;
        let json =
            serde_json::to_vec(&serde_json::json!({"schemaVersion": 1, "records": [overflow]}))
                .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_path_buf();
        let error = ComparisonSources::archive_reader(json.as_slice(), &current, directory)
            .err()
            .unwrap();
        assert!(error.contains("exceeds SQLite's range"), "{error}");
        assert!(!path.exists());
    }

    #[test]
    #[ignore = "isolated archive-staging RSS workload; run optimized outside browser/crawler benchmarks"]
    fn archive_record_stream_workload() {
        use serde::Deserialize;
        use std::io::Write;
        use std::time::Instant;
        let count: usize = std::env::var("FF_ARCHIVE_RECORDS")
            .ok()
            .map(|value| value.parse().unwrap())
            .unwrap_or(50_000);
        assert!(count > 0 && count < u32::MAX as usize);
        let mode = std::env::var("FF_ARCHIVE_MODE").unwrap_or_else(|_| "stream".into());
        assert!(matches!(mode.as_str(), "snapshot" | "stream"));
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("source.ffcrawl.json");
        let mut output = std::io::BufWriter::new(File::create(&archive).unwrap());
        output
            .write_all(b"{\"schemaVersion\":1,\"records\":[")
            .unwrap();
        for index in 0..count {
            if index > 0 {
                output.write_all(b",").unwrap();
            }
            let mut record = page(index as u32);
            record.id = ((count - index) * 3) as u64;
            serde_json::to_writer(&mut output, &record).unwrap();
        }
        output.write_all(b"]}").unwrap();
        output.flush().unwrap();
        drop(output);
        let bytes = fs::metadata(&archive).unwrap().len();
        let database = directory.path().join("baseline.sqlite3");
        let started = Instant::now();
        let reader = BufReader::new(File::open(&archive).unwrap());
        let store = if mode == "snapshot" {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ArchiveRecords {
                schema_version: u32,
                records: Vec<CrawlRecord>,
            }
            let decoded: ArchiveRecords = serde_json::from_reader(reader).unwrap();
            assert_eq!(decoded.schema_version, 1);
            // Matched baseline isolates whole-array hydration; identity staging is shared.
            stage_records(decoded.records, &database).unwrap()
        } else {
            read_archive_records(reader, &database).unwrap()
        };
        let elapsed = started.elapsed();
        let connection = Connection::open(&database).unwrap();
        let actual: i64 = connection
            .query_row("SELECT COUNT(*) FROM crawl_records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(actual, count as i64);
        let selected = ActiveStore::Sqlite(store)
            .try_records_by_ids(&[3, (count * 3) as u64])
            .unwrap();
        assert_eq!(selected.len(), if count == 1 { 1 } else { 2 });
        assert_eq!(selected[0].list_position, Some((count - 1) as u32));
        assert_eq!(selected.last().unwrap().list_position, Some(0));
        assert_eq!(selected[0].custom_extractions, page(0).custom_extractions);
        eprintln!(
            "archive_records mode={mode} rows={count} archive_bytes={bytes} stage_ms={:.3}",
            elapsed.as_secs_f64() * 1000.0
        );
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
