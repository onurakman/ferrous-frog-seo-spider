use ferrous_frog_storage::{CrawlRecord, SqliteStore};
use rusqlite::{Connection, params};
use std::path::Path;

// Keep identity validation and remapping on disk. Upsert's existing column mapping
// preserves complete record payloads as the crawl schema grows.
pub(super) struct RecordStage {
    identities: Connection,
    store: SqliteStore,
}

impl RecordStage {
    pub(super) fn store(&self) -> &SqliteStore {
        &self.store
    }
    pub(super) fn connection(&self) -> &Connection {
        &self.identities
    }
    pub(super) fn connection_mut(&mut self) -> &mut Connection {
        &mut self.identities
    }

    pub(super) fn new(path: &Path) -> Result<Self, String> {
        let store = SqliteStore::open(path).map_err(|error| error.to_string())?;
        let identities = Connection::open(path).map_err(|error| error.to_string())?;
        identities
            .execute_batch(
                "PRAGMA synchronous = NORMAL;
             CREATE TABLE comparison_record_ids (
                 saved_id INTEGER PRIMARY KEY,
                 storage_key TEXT NOT NULL UNIQUE,
                 inserted_id INTEGER UNIQUE
             );",
            )
            .map_err(|error| error.to_string())?;
        Ok(Self { identities, store })
    }

    pub(super) fn insert(&mut self, record: CrawlRecord) -> Result<(), String> {
        let saved = i64::try_from(record.id)
            .map_err(|_| format!("record ID {} exceeds SQLite's range", record.id))?;
        let key = if record.storage_key.trim().is_empty() {
            record.final_url.as_str()
        } else {
            record.storage_key.as_str()
        };
        if let Err(error) = self.identities.execute(
            "INSERT INTO comparison_record_ids (saved_id, storage_key) VALUES (?1, ?2)",
            params![saved, key],
        ) {
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                let duplicate_id: bool = self
                    .identities
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM comparison_record_ids WHERE saved_id = ?1)",
                        [saved],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                return Err(if duplicate_id {
                    format!("duplicate record ID {saved} in crawl source")
                } else {
                    format!("duplicate record storage key {key} in crawl source")
                });
            }
            return Err(error.to_string());
        }
        let inserted = self
            .store
            .try_upsert(record)
            .map_err(|error| error.to_string())?;
        self.identities
            .execute(
                "UPDATE comparison_record_ids SET inserted_id = ?1 WHERE saved_id = ?2",
                params![inserted.id as i64, saved],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<SqliteStore, String> {
        // Private IDs must move out of the way before restoring shuffled originals.
        let transaction = self
            .identities
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute_batch(
                "UPDATE crawl_records SET id = -id;
             UPDATE crawl_records SET id = (
                 SELECT saved_id FROM comparison_record_ids WHERE inserted_id = -crawl_records.id
             );
             DROP TABLE comparison_record_ids;",
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(self.store)
    }
}
