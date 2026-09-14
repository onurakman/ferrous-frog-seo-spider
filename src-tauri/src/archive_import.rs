use crate::archive_staging::RecordStage;
use ferrous_frog_crawler_core::CrawlMode;
use ferrous_frog_storage::{
    CrawlFrontierItem, CrawlRecord, ImageAsset, LinkEdge, PageCapture, PageReference,
};
use rusqlite::{Connection, params};
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor,
};
use std::{
    fmt,
    io::Read,
    marker::PhantomData,
    path::{Path, PathBuf},
};

const BATCH_SIZE: usize = 256;

pub(super) struct StagedArchive {
    // The directory is owned until publication; every error drops all temporary files.
    _directory: tempfile::TempDir,
    pub path: PathBuf,
    pub start_url: String,
    pub mode: CrawlMode,
    pub records: usize,
    pub link_edges: usize,
    pub image_assets: usize,
    pub frontier_items: usize,
}

pub(super) fn stage_archive_reader(
    reader: impl Read,
    sessions_directory: &Path,
) -> Result<StagedArchive, String> {
    std::fs::create_dir_all(sessions_directory).map_err(|error| error.to_string())?;
    let directory = tempfile::Builder::new()
        .prefix(".archive-import-")
        .tempdir_in(sessions_directory)
        .map_err(|error| error.to_string())?;
    let path = directory.path().join("crawl.sqlite3");
    let stage = RecordStage::new(&path)?;
    stage.connection().execute_batch("CREATE TABLE archive_pending_edges (ordinal INTEGER PRIMARY KEY, payload TEXT NOT NULL);")
        .map_err(|error| error.to_string())?;
    let mut import = ImportDecoder {
        stage,
        first_record: None,
        first_queued: None,
        list: false,
        records: 0,
        link_edges: 0,
        image_assets: 0,
        frontier_items: 0,
    };
    let mut decoder = serde_json::Deserializer::from_reader(reader);
    (&mut import)
        .deserialize(&mut decoder)
        .map_err(|error| format!("invalid crawl archive: {error}"))?;
    decoder
        .end()
        .map_err(|error| format!("invalid crawl archive: {error}"))?;
    let orphan: bool = import.stage.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM page_captures c LEFT JOIN crawl_records r ON r.storage_key = c.source_storage_key WHERE r.id IS NULL)",
        [], |row| row.get(0)).map_err(|error| error.to_string())?;
    if orphan {
        return Err("crawl archive capture has no matching record occurrence".into());
    }
    let store = import.stage.finish()?;
    let connection = Connection::open(&path).map_err(|error| error.to_string())?;
    connection
        .execute_batch("PRAGMA synchronous = NORMAL;")
        .map_err(|error| error.to_string())?;
    let mut last = 0i64;
    loop {
        // End the read cursor before writing through the store connection.
        let page = {
            let mut statement = connection.prepare("SELECT ordinal,payload FROM archive_pending_edges WHERE ordinal > ?1 ORDER BY ordinal LIMIT ?2")
                .map_err(|error| error.to_string())?;
            statement
                .query_map(params![last, BATCH_SIZE as i64], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        if page.is_empty() {
            break;
        }
        last = page.last().expect("nonempty page").0;
        let edges = page
            .into_iter()
            .map(|(_, payload)| serde_json::from_str::<LinkEdge>(&payload))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        store
            .try_append_archive_link_edges(&edges)
            .map_err(|error| error.to_string())?;
        // Let later writes reuse consumed staging pages instead of publishing a large freelist.
        connection
            .execute(
                "DELETE FROM archive_pending_edges WHERE ordinal <= ?1",
                [last],
            )
            .map_err(|error| error.to_string())?;
    }
    connection
        .execute_batch("DROP TABLE archive_pending_edges;")
        .map_err(|error| error.to_string())?;
    drop(store);
    let busy: i64 = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if busy != 0 {
        return Err("failed to close archive staging checkpoint".into());
    }
    drop(connection);
    Ok(StagedArchive {
        _directory: directory,
        path,
        start_url: import
            .first_record
            .or(import.first_queued)
            .unwrap_or_default(),
        mode: if import.list {
            CrawlMode::List
        } else {
            CrawlMode::Spider
        },
        records: import.records,
        link_edges: import.link_edges,
        image_assets: import.image_assets,
        frontier_items: import.frontier_items,
    })
}

struct ImportDecoder {
    stage: RecordStage,
    first_record: Option<String>,
    first_queued: Option<String>,
    list: bool,
    records: usize,
    link_edges: usize,
    image_assets: usize,
    frontier_items: usize,
}

impl<'de> DeserializeSeed<'de> for &mut ImportDecoder {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for &mut ImportDecoder {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a schema-1 crawl archive")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        let mut seen = [false; 8];
        while let Some(key) = map.next_key::<String>()? {
            let field = match key.as_str() {
                "schemaVersion" => 0,
                "exportedAtMs" => 1,
                "records" => 2,
                "linkEdges" => 3,
                "imageAssets" => 4,
                "pageReferences" => 5,
                "pageCaptures" => 6,
                "frontierState" => 7,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                    continue;
                }
            };
            if std::mem::replace(&mut seen[field], true) {
                return Err(de::Error::custom(format!("duplicate archive field {key}")));
            }
            match field {
                0 => {
                    let version = map.next_value::<u32>()?;
                    if version != crate::CRAWL_ARCHIVE_SCHEMA_VERSION {
                        return Err(de::Error::custom(format!(
                            "unsupported crawl archive schema version {version}"
                        )));
                    }
                }
                1 => {
                    map.next_value::<i64>()?;
                }
                2 => {
                    self.records = map.next_value_seed(Items::new(|record: CrawlRecord| {
                        self.first_record.get_or_insert_with(|| record.url.clone());
                        self.list |= record.list_position.is_some();
                        self.stage.insert(record)
                    }))?;
                }
                3 => {
                    let mut pending = Vec::with_capacity(BATCH_SIZE);
                    self.link_edges = map.next_value_seed(Items::new(|edge: LinkEdge| {
                        pending.push(edge);
                        if pending.len() == BATCH_SIZE {
                            write_pending_edges(&mut self.stage, &pending)?;
                            pending.clear();
                        }
                        Ok(())
                    }))?;
                    write_pending_edges(&mut self.stage, &pending).map_err(de::Error::custom)?;
                }
                4 => {
                    self.image_assets =
                        map.next_value_seed(Batches::new(|rows: &[ImageAsset]| {
                            self.stage
                                .store()
                                .try_append_archive_image_assets(rows)
                                .map_err(|error| error.to_string())
                        }))?;
                }
                5 => {
                    map.next_value_seed(Batches::new(|rows: &[PageReference]| {
                        self.stage
                            .store()
                            .try_append_archive_page_references(rows)
                            .map_err(|error| error.to_string())
                    }))?;
                }
                6 => {
                    map.next_value_seed(Items::new(|capture:PageCapture| {
                    let duplicate:bool = self.stage.connection().query_row("SELECT EXISTS(SELECT 1 FROM page_captures WHERE source_storage_key=?1)",[&capture.source_storage_key],|row|row.get(0)).map_err(|error|error.to_string())?;
                    if duplicate { return Err("crawl archive contains duplicate page captures for one occurrence".into()); }
                    let key = capture.source_storage_key.clone();
                    self.stage.store().try_replace_page_capture(&key,Some(capture)).map_err(|error|error.to_string())
                }))?;
                }
                7 => {
                    map.next_value_seed(Frontier(self))?;
                }
                _ => unreachable!(),
            }
        }
        for (index, name) in [
            "schemaVersion",
            "exportedAtMs",
            "records",
            "linkEdges",
            "imageAssets",
        ]
        .iter()
        .enumerate()
        {
            if !seen[index] {
                return Err(de::Error::custom(format!("missing archive field {name}")));
            }
        }
        Ok(())
    }
}

fn write_pending_edges(stage: &mut RecordStage, rows: &[LinkEdge]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let tx = stage
        .connection_mut()
        .transaction()
        .map_err(|error| error.to_string())?;
    {
        let mut insert = tx
            .prepare_cached("INSERT INTO archive_pending_edges(payload) VALUES (?1)")
            .map_err(|error| error.to_string())?;
        for edge in rows {
            insert
                .execute([serde_json::to_string(edge).map_err(|error| error.to_string())?])
                .map_err(|error| error.to_string())?;
        }
    }
    tx.commit().map_err(|error| error.to_string())
}

struct Items<T, F> {
    write: F,
    item: PhantomData<T>,
}
impl<T, F> Items<T, F> {
    fn new(write: F) -> Self {
        Self {
            write,
            item: PhantomData,
        }
    }
}
impl<'de, T: DeserializeOwned, F: FnMut(T) -> Result<(), String>> DeserializeSeed<'de>
    for Items<T, F>
{
    type Value = usize;
    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<usize, D::Error> {
        decoder.deserialize_seq(self)
    }
}
impl<'de, T: DeserializeOwned, F: FnMut(T) -> Result<(), String>> Visitor<'de> for Items<T, F> {
    type Value = usize;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array")
    }
    fn visit_seq<S: SeqAccess<'de>>(mut self, mut seq: S) -> Result<usize, S::Error> {
        let mut count = 0;
        while let Some(item) = seq.next_element::<T>()? {
            (self.write)(item).map_err(de::Error::custom)?;
            count += 1;
        }
        Ok(count)
    }
}

struct Batches<T, F> {
    write: F,
    item: PhantomData<T>,
}
impl<T, F> Batches<T, F> {
    fn new(write: F) -> Self {
        Self {
            write,
            item: PhantomData,
        }
    }
}
impl<'de, T: DeserializeOwned, F: FnMut(&[T]) -> Result<(), String>> DeserializeSeed<'de>
    for Batches<T, F>
{
    type Value = usize;
    fn deserialize<D: de::Deserializer<'de>>(mut self, decoder: D) -> Result<usize, D::Error> {
        let mut batch = Vec::with_capacity(BATCH_SIZE);
        let count = Items::new(|item: T| {
            batch.push(item);
            if batch.len() == BATCH_SIZE {
                (self.write)(&batch)?;
                batch.clear();
            }
            Ok(())
        })
        .deserialize(decoder)?;
        if !batch.is_empty() {
            (self.write)(&batch).map_err(de::Error::custom)?;
        }
        Ok(count)
    }
}

struct Frontier<'a>(&'a mut ImportDecoder);
impl<'de> DeserializeSeed<'de> for Frontier<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_option(self)
    }
}
impl<'de> Visitor<'de> for Frontier<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a frontier object or null")
    }
    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_some<D: de::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_map(self)
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        let mut seen = [false; 3];
        while let Some(key) = map.next_key::<String>()? {
            let field = match key.as_str() {
                "queued" => 0,
                "seen" => 1,
                "crawled" => 2,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                    continue;
                }
            };
            if std::mem::replace(&mut seen[field], true) {
                return Err(de::Error::custom(format!("duplicate frontier field {key}")));
            }
            match field {
                0 => {
                    let mut position = 0;
                    self.0.frontier_items =
                        map.next_value_seed(Batches::new(|rows: &[CrawlFrontierItem]| {
                            if self.0.first_queued.is_none() {
                                self.0.first_queued = rows.first().map(|row| row.url.clone());
                            }
                            self.0.list |= rows.iter().any(|row| row.list_position.is_some());
                            self.0
                                .stage
                                .store()
                                .try_append_archive_frontier_queue(position, rows)
                                .map_err(|error| error.to_string())?;
                            position += rows.len();
                            Ok(())
                        }))?;
                }
                1 => {
                    map.next_value_seed(Batches::new(|rows: &[String]| {
                        self.0
                            .stage
                            .store()
                            .try_append_archive_seen(rows)
                            .map_err(|error| error.to_string())
                    }))?;
                }
                2 => {
                    let crawled = map.next_value::<usize>()?;
                    self.0
                        .stage
                        .store()
                        .try_set_archive_crawled(crawled)
                        .map_err(de::Error::custom)?;
                }
                _ => unreachable!(),
            }
        }
        for (index, name) in ["queued", "seen", "crawled"].iter().enumerate() {
            if !seen[index] {
                return Err(de::Error::custom(format!("missing frontier field {name}")));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::{CrawlRecord, GridQuery, SortDirection, SqliteStore};
    use serde_json::json;

    fn empty_archive() -> serde_json::Value {
        json!({"schemaVersion":1,"exportedAtMs":0,"records":[],"linkEdges":[],"imageAssets":[]})
    }

    #[test]
    fn archive_import_streams_complete_schema_and_restores_record_identity() {
        let directory = tempfile::tempdir().unwrap();
        let mut first = CrawlRecord::pending("https://example.test/first".into(), 0);
        first.id = 90;
        first.storage_key = "occurrence:first".into();
        first.list_position = Some(1);
        first.inlink_count = 77;
        let mut second = first.clone();
        second.id = 7;
        second.url = "https://example.test/second".into();
        second.storage_key = "occurrence:second".into();
        second.list_position = Some(0);
        let mut input = empty_archive();
        input["records"] = json!([first, second]);
        input["frontierState"] = json!({"queued":[],"seen":["z","a","z"],"crawled":2});
        let bytes = serde_json::to_vec(&input).unwrap();
        let staged = stage_archive_reader(&bytes[..], directory.path()).unwrap();
        assert_eq!(staged.records, 2);
        assert_eq!(staged.start_url, "https://example.test/first");
        assert_eq!(staged.mode, ferrous_frog_crawler_core::CrawlMode::List);
        let store = SqliteStore::open(&staged.path).unwrap();
        let rows = store
            .try_query(GridQuery {
                sort_dir: SortDirection::Asc,
                ..Default::default()
            })
            .unwrap()
            .rows;
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [7, 90]);
        assert_eq!(rows[1].inlink_count, 77);
        let frontier = store.try_load_frontier_state().unwrap().unwrap();
        assert_eq!(frontier.seen, ["a", "z"]);
        assert_eq!(frontier.crawled, 2);
    }

    #[test]
    fn archive_import_rejects_late_invalid_json_and_cleans_staging() {
        let directory = tempfile::tempdir().unwrap();
        let valid = serde_json::to_string(&empty_archive()).unwrap();
        for input in [
            format!("{valid} true"),
            valid.trim_end_matches('}').to_owned(),
            valid.replace("\"schemaVersion\":1", "\"schemaVersion\":2"),
            valid.replace("\"exportedAtMs\":0,", ""),
            valid.replace(
                "\"schemaVersion\":1",
                "\"schemaVersion\":1,\"schemaVersion\":1",
            ),
            valid.replace("\"imageAssets\":[]", "\"imageAssets\":null"),
            valid.replace("\"records\":[]", "\"records\":[],\"records\":[]"),
        ] {
            assert!(
                stage_archive_reader(input.as_bytes(), directory.path()).is_err(),
                "{input}"
            );
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }
    #[test]
    fn archive_import_preserves_links_before_records_and_capture_ownership() {
        use ferrous_frog_storage::{LinkEdge, LinkEdgeQuery, LinkType, PageCapture};
        let directory = tempfile::tempdir().unwrap();
        let mut record = CrawlRecord::pending("https://example.test/source".into(), 1);
        record.id = 80;
        record.status_code = Some(200);
        record.storage_key = "list:80".into();
        let edge = LinkEdge {
            id: 900,
            source_url: record.url.clone(),
            target_url: record.url.clone(),
            anchor_text: "Unicode <script>İ 🐸".into(),
            rel: "nofollow".into(),
            rel_nofollow: true,
            link_type: LinkType::Internal,
            source_status_code: Some(301),
            target_status_code: Some(404),
            source_depth: 17,
            target_depth: Some(19),
            source_position: 27,
            discovery_order: 400,
        };
        let capture = PageCapture {
            source_storage_key: record.storage_key.clone(),
            source_url: record.url.clone(),
            final_url: record.final_url.clone(),
            raw_html: Some(String::new()),
            visible_text: Some("İ 🐸".into()),
            raw_html_truncated: true,
            ..Default::default()
        };
        let input = format!(
            r#"{{"linkEdges":[{}],"pageCaptures":[{}],"records":[{}],"imageAssets":[],"schemaVersion":1,"exportedAtMs":0}}"#,
            serde_json::to_string(&edge).unwrap(),
            serde_json::to_string(&capture).unwrap(),
            serde_json::to_string(&record).unwrap()
        );
        let staged = stage_archive_reader(input.as_bytes(), directory.path()).unwrap();
        let store = SqliteStore::open(&staged.path).unwrap();
        assert_eq!(
            store
                .try_link_edges(LinkEdgeQuery::default())
                .unwrap()
                .edges
                .as_slice(),
            std::slice::from_ref(&edge)
        );
        assert_eq!(
            store
                .try_page_captures(ferrous_frog_storage::PageCaptureQuery {
                    source_storage_key: Some(capture.source_storage_key.clone()),
                    ..Default::default()
                })
                .unwrap()
                .captures
                .pop(),
            Some(capture.clone())
        );
        drop(store);
        drop(staged);
        let duplicate = input.replace(
            "\"pageCaptures\":[",
            &format!(
                "\"pageCaptures\":[{},",
                serde_json::to_string(&capture).unwrap()
            ),
        );
        assert!(stage_archive_reader(duplicate.as_bytes(), directory.path()).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        let duplicate_edge = input.replace(
            "\"linkEdges\":[",
            &format!("\"linkEdges\":[{},", serde_json::to_string(&edge).unwrap()),
        );
        assert!(stage_archive_reader(duplicate_edge.as_bytes(), directory.path()).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        let mut orphan = empty_archive();
        orphan["pageCaptures"] = json!([capture]);
        assert!(
            stage_archive_reader(
                serde_json::to_vec(&orphan).unwrap().as_slice(),
                directory.path()
            )
            .is_err()
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    struct ProbeReader<'a> {
        input: &'a [u8],
        position: usize,
        probe_after: usize,
        fail_after: Option<usize>,
        directory: &'a Path,
        observed: &'a std::cell::Cell<bool>,
    }
    impl Read for ProbeReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.position > self.probe_after && !self.observed.get() {
                let child = std::fs::read_dir(self.directory)?
                    .next()
                    .unwrap()?
                    .path()
                    .join("crawl.sqlite3");
                let conn = Connection::open(child).unwrap();
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM crawl_records", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(
                    count, 1,
                    "the first record must reach SQLite before the second is decoded"
                );
                self.observed.set(true);
            }
            if self.fail_after.is_some_and(|limit| self.position >= limit) {
                return Err(std::io::Error::other("injected late read failure"));
            }
            let count = buffer.len().min(7).min(self.input.len() - self.position);
            buffer[..count].copy_from_slice(&self.input[self.position..self.position + count]);
            self.position += count;
            Ok(count)
        }
    }

    #[test]
    fn archive_import_inserts_before_eof_and_cleans_late_io_failures() {
        let directory = tempfile::tempdir().unwrap();
        let mut first = CrawlRecord::pending("https://example.test/first".into(), 0);
        first.id = 101;
        let prefix = format!("{{\"records\":[{},", serde_json::to_string(&first).unwrap());
        let mut second = CrawlRecord::pending("https://example.test/second".into(), 0);
        second.id = 303;
        second.title = Some("x".repeat(8192));
        let input = format!(
            "{prefix}{}],\"schemaVersion\":1,\"exportedAtMs\":0,\"linkEdges\":[],\"imageAssets\":[]}}",
            serde_json::to_string(&second).unwrap()
        );
        for fail in [false, true] {
            let observed = std::cell::Cell::new(false);
            let reader = ProbeReader {
                input: input.as_bytes(),
                position: 0,
                probe_after: prefix.len() + 128,
                fail_after: fail.then_some(prefix.len() + 2048),
                directory: directory.path(),
                observed: &observed,
            };
            let result = stage_archive_reader(reader, directory.path());
            assert!(observed.get());
            assert_eq!(result.is_err(), fail);
            drop(result);
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn archive_import_preserves_relation_batches_and_frontier_order() {
        use ferrous_frog_storage::{
            CrawlFrontierItem, ImageAsset, ImageAssetQuery, PageReference, PageReferenceKind,
            PageReferenceQuery,
        };
        let directory = tempfile::tempdir().unwrap();
        let mut input = empty_archive();
        let images: Vec<_> = (0..300)
            .rev()
            .map(|id| ImageAsset {
                id,
                page_url: "https://example.test/page".into(),
                image_url: format!("https://example.test/image-{id}"),
                alt_text: Some(format!("Alt {id} 🐸")),
                alt_len: 12,
                missing_alt: false,
                alt_too_long: false,
                width: Some(40),
                height: Some(50),
                source_position: id as u32,
                size_bytes: None,
                oversized: false,
            })
            .collect();
        let references: Vec<_> = (0..300)
            .rev()
            .map(|id| PageReference {
                id,
                source_storage_key: "legacy-source".into(),
                source_url: "https://example.test/page".into(),
                target_url: format!("https://example.test/ref-{id}"),
                kind: PageReferenceKind::Pagination,
                rel_nofollow: id % 2 == 0,
            })
            .collect();
        let queued: Vec<_> = (0..300)
            .rev()
            .map(|position| CrawlFrontierItem {
                url: format!("https://example.test/queued-{position}"),
                depth: position as usize,
                from_sitemap: position % 2 == 0,
                storage_key: format!("occurrence:{position}"),
                list_position: Some(position),
                list_duplicate_index: 2,
            })
            .collect();
        input["imageAssets"] = json!(images);
        input["pageReferences"] = json!(references);
        input["frontierState"] =
            json!({"queued":queued,"seen":["second","first","second"],"crawled":47});
        input["unknownFutureField"] = json!({"ignored":[true,1,"future"]});
        let bytes = serde_json::to_vec(&input).unwrap();
        let staged = stage_archive_reader(bytes.as_slice(), directory.path()).unwrap();
        assert_eq!(staged.start_url, queued[0].url);
        assert_eq!(staged.mode, CrawlMode::List);
        assert_eq!(
            (staged.records, staged.image_assets, staged.frontier_items),
            (0, 300, 300)
        );
        let store = SqliteStore::open(&staged.path).unwrap();
        let mut images = images;
        images.sort_by_key(|row| row.id);
        assert_eq!(
            store
                .try_image_assets(ImageAssetQuery::default())
                .unwrap()
                .images,
            images
        );
        let mut references = references;
        references.sort_by_key(|row| row.id);
        assert_eq!(
            store
                .try_page_references(PageReferenceQuery {
                    limit: 1000,
                    ..Default::default()
                })
                .unwrap()
                .references,
            references
        );
        let frontier = store.try_load_frontier_state().unwrap().unwrap();
        assert_eq!(frontier.queued, queued);
        assert_eq!(frontier.crawled, 47);
        assert_eq!(frontier.seen, ["first", "second"]);
    }

    #[test]
    fn archive_import_rejects_ambiguous_identity_and_malformed_frontier() {
        let directory = tempfile::tempdir().unwrap();
        let mut record = CrawlRecord::pending("https://example.test/legacy".into(), 0);
        record.id = 0;
        record.storage_key = String::new();
        let mut valid = empty_archive();
        valid["records"] = json!([record]);
        let bytes = serde_json::to_vec(&valid).unwrap();
        let staged = stage_archive_reader(bytes.as_slice(), directory.path()).unwrap();
        let store = SqliteStore::open(&staged.path).unwrap();
        assert_eq!(
            store.try_records().unwrap()[0].storage_key,
            record.final_url
        );
        drop(store);
        drop(staged);
        let mut duplicate_id = valid.clone();
        let mut other = record.clone();
        other.storage_key = "other".into();
        duplicate_id["records"] = json!([record, other]);
        let mut duplicate_key = valid.clone();
        other = record.clone();
        other.id = 99;
        duplicate_key["records"] = json!([record, other]);
        let mut overflow = valid.clone();
        other.id = u64::MAX;
        overflow["records"] = json!([other]);
        let mut missing_frontier = valid.clone();
        missing_frontier["frontierState"] = json!({"queued":[],"seen":[]});
        let mut wrong_optional = valid.clone();
        wrong_optional["pageReferences"] = json!(false);
        for input in [
            duplicate_id,
            duplicate_key,
            overflow,
            missing_frontier,
            wrong_optional,
        ] {
            assert!(
                stage_archive_reader(
                    serde_json::to_vec(&input).unwrap().as_slice(),
                    directory.path()
                )
                .is_err()
            );
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }

    #[test]
    #[ignore = "isolated full archive import RSS workload"]
    fn archive_import_full_stream_workload() {
        use ferrous_frog_storage::{CrawlFrontierItem, LinkType, PageReferenceKind};
        use std::{
            fs::File,
            io::{BufReader, BufWriter, Write},
            time::Instant,
        };
        fn array<T: serde::Serialize>(
            writer: &mut impl Write,
            count: usize,
            mut item: impl FnMut(usize) -> T,
        ) {
            for index in 0..count {
                if index > 0 {
                    writer.write_all(b",").unwrap();
                }
                serde_json::to_writer(&mut *writer, &item(index)).unwrap();
            }
        }
        let count: usize = std::env::var("FF_IMPORT_RECORDS")
            .ok()
            .map(|v| v.parse().unwrap())
            .unwrap_or(50_000);
        let edges: usize = std::env::var("FF_IMPORT_EDGES")
            .ok()
            .map(|v| v.parse().unwrap())
            .unwrap_or(1_000_001);
        assert!(count > 0 && edges > 0 && count < u32::MAX as usize);
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("full.ffcrawl.json");
        let mut output = BufWriter::new(File::create(&input).unwrap());
        output
            .write_all(b"{\"schemaVersion\":1,\"exportedAtMs\":0,\"records\":[")
            .unwrap();
        array(&mut output, count, |index| {
            let mut record = CrawlRecord::pending(format!("https://example.test/page-{index}"), 1);
            record.id = ((count - index) * 3) as u64;
            record.storage_key = format!("list:{index}");
            record.list_position = Some(index as u32);
            record.status_code = Some(200);
            record.content_type = Some("text/html".into());
            record.title = Some(format!("Page {index} İ 🐸"));
            record.inlink_count = 71;
            record
        });
        output.write_all(b"],\"linkEdges\":[").unwrap();
        array(&mut output, edges, |index| LinkEdge {
            id: index as u64,
            source_url: format!("https://example.test/page-{}", index % count),
            target_url: format!("https://example.test/page-{}", (index + 1) % count),
            anchor_text: format!("Evidence {index} İ 🐸"),
            rel: "nofollow".into(),
            rel_nofollow: true,
            link_type: LinkType::Internal,
            source_status_code: Some(301),
            target_status_code: Some(404),
            source_depth: 17,
            target_depth: Some(19),
            source_position: (index % 100) as u32,
            discovery_order: (index * 2) as u64,
        });
        output.write_all(b"],\"imageAssets\":[").unwrap();
        array(&mut output, count, |index| ImageAsset {
            id: (index * 3 + 11) as u64,
            page_url: format!("https://example.test/page-{index}"),
            image_url: format!("https://example.test/image-{index}.png"),
            alt_text: Some(format!("Image {index}")),
            alt_len: 12,
            missing_alt: false,
            alt_too_long: false,
            width: Some(100),
            height: Some(50),
            source_position: 3,
            size_bytes: None,
            oversized: false,
        });
        output.write_all(b"],\"pageReferences\":[").unwrap();
        array(&mut output, count, |index| PageReference {
            id: (index * 3 + 11) as u64,
            source_storage_key: format!("list:{index}"),
            source_url: format!("https://example.test/page-{index}"),
            target_url: format!("https://example.test/page-{}", (index + 1) % count),
            kind: PageReferenceKind::Pagination,
            rel_nofollow: false,
        });
        output.write_all(b"],\"pageCaptures\":[").unwrap();
        array(&mut output, count, |index| PageCapture {
            source_storage_key: format!("list:{index}"),
            source_url: format!("https://example.test/page-{index}"),
            final_url: format!("https://example.test/page-{index}"),
            raw_html: Some(format!("<html><title>Page {index} İ 🐸</title></html>")),
            visible_text: Some(format!("Page {index}")),
            ..Default::default()
        });
        output
            .write_all(b"],\"frontierState\":{\"queued\":[")
            .unwrap();
        array(&mut output, count, |index| CrawlFrontierItem {
            url: format!("https://example.test/queued-{index}"),
            depth: 2,
            from_sitemap: false,
            storage_key: format!("queued:{index}"),
            list_position: Some((count - index) as u32),
            list_duplicate_index: 1,
        });
        output.write_all(b"],\"seen\":[").unwrap();
        array(&mut output, count * 2, |index| format!("seen:{index}"));
        write!(&mut output, "],\"crawled\":{count}}}}}").unwrap();
        output.flush().unwrap();
        drop(output);
        let bytes = std::fs::metadata(&input).unwrap().len();
        let started = Instant::now();
        let staged = stage_archive_reader(
            BufReader::new(File::open(&input).unwrap()),
            &directory.path().join("sessions"),
        )
        .unwrap();
        let index = crate::sessions::index_connection(directory.path()).unwrap();
        let (session, store) = crate::sessions::publish_imported_session(
            &index,
            directory.path(),
            &staged.path,
            &staged.start_url,
            staged.mode,
            staged.records,
        )
        .unwrap();
        let elapsed = started.elapsed();
        let conn = Connection::open(&session.database_path).unwrap();
        for (table, expected) in [
            ("crawl_records", count),
            ("link_edges", edges),
            ("image_assets", count),
            ("page_references", count),
            ("page_captures", count),
            ("crawl_frontier_queue", count),
            ("crawl_frontier_seen", count * 2),
        ] {
            let actual: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(actual, expected as i64, "{table}");
        }
        let last: (i64, i64, String) = conn
            .query_row(
                "SELECT id,target_status_code,anchor_text FROM link_edges ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            last,
            (
                (edges - 1) as i64,
                404,
                format!("Evidence {} İ 🐸", edges - 1)
            )
        );
        let captured: i64 = conn
            .query_row(
                "SELECT inlink_count FROM crawl_records WHERE id=3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(captured, 71);
        assert_eq!(session.start_url, "https://example.test/page-0");
        assert_eq!(session.status, "imported");
        eprintln!(
            "archive_import records={count} edges={edges} images={count} references={count} captures={count} queued={count} seen={} archive_bytes={bytes} import_publish_ms={:.3}",
            count * 2,
            elapsed.as_secs_f64() * 1000.0
        );
        drop(conn);
        drop(store);
        drop(staged);
    }
}
