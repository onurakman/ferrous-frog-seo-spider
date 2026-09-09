use crate::now_ms;
use ferrous_frog_crawler_core::{CrawlConfig, CrawlMode};
use ferrous_frog_storage::SqliteStore;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CrawlSession {
    pub id: String,
    pub name: String,
    pub start_url: String,
    pub database_path: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub is_current: bool,
    pub mode: CrawlMode,
    pub status: String,
    pub crawled: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<CrawlConfig>,
}

pub(crate) fn index_connection(dir: &Path) -> Result<Connection, String> {
    fs::create_dir_all(dir)
        .map_err(|error| format!("failed to create app data directory: {error}"))?;
    let conn = Connection::open(dir.join("ferrous-frog-sessions.sqlite3"))
        .map_err(|error| error.to_string())?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    initialize_index(&conn).map_err(|error| error.to_string())?;
    register_legacy_database(&conn, dir)?;
    Ok(conn)
}

pub(crate) fn initialize_index(conn: &Connection) -> rusqlite::Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS crawl_sessions (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, start_url TEXT NOT NULL,
            database_path TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_crawl_sessions_updated_at
            ON crawl_sessions(updated_at_ms DESC);
         CREATE TABLE IF NOT EXISTS config_profiles (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, config_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_config_profiles_updated_at
            ON config_profiles(updated_at_ms DESC);
         CREATE TABLE IF NOT EXISTS integration_settings (
            key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
         );",
    )?;
    for (column, definition) in [
        ("mode", "TEXT NOT NULL DEFAULT 'spider'"),
        ("status", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("crawled", "INTEGER"),
        ("config_json", "TEXT"),
    ] {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('crawl_sessions') WHERE name = ?1)",
            [column],
            |row| row.get(0),
        )?;
        if !exists {
            tx.execute(
                &format!("ALTER TABLE crawl_sessions ADD COLUMN {column} {definition}"),
                [],
            )?;
        }
    }
    tx.commit()
}

pub(crate) fn mark_interrupted(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "UPDATE crawl_sessions SET status = 'interrupted'
         WHERE status IN ('starting', 'running', 'paused', 'importing')",
        [],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn register_legacy_database(conn: &Connection, dir: &Path) -> Result<(), String> {
    let path = dir.join("ferrous-frog-current.sqlite3");
    if !path.is_file() {
        return Ok(());
    }
    let path_text = path.to_string_lossy();
    let known: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM crawl_sessions WHERE database_path = ?1)",
            [path_text.as_ref()],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if known {
        return Ok(());
    }
    // Only the one legacy current database is inspected; history reads index metadata.
    let metadata = read_database_metadata(&path);
    let (start_url, mode, crawled, status) = match metadata {
        Ok((url, mode, count)) => (url, mode, Some(count), "unknown"),
        Err(_) => (String::new(), CrawlMode::Spider, None, "unavailable"),
    };
    let now = now_ms();
    conn.execute(
        "INSERT OR IGNORE INTO crawl_sessions
         (id, name, start_url, database_path, created_at_ms, updated_at_ms, mode, status, crawled)
         VALUES ('legacy-current', 'Previous crawl', ?1, ?2, ?3, ?3, ?4, ?5, ?6)",
        params![
            start_url,
            path_text.as_ref(),
            now,
            mode_name(mode),
            status,
            crawled.map(|value| value as i64)
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn query_sessions(
    conn: &Connection,
    current_session_id: Option<&str>,
) -> Result<Vec<CrawlSession>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, start_url, database_path, created_at_ms, updated_at_ms,
                    mode, status, crawled
             FROM crawl_sessions ORDER BY updated_at_ms DESC, name ASC",
        )
        .map_err(|error| error.to_string())?;
    stmt.query_map([], |row| session_from_row(row, current_session_id))
        .map_err(|error| error.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn get_session(
    conn: &Connection,
    id: &str,
    current_session_id: Option<&str>,
) -> Result<CrawlSession, String> {
    conn.query_row(
        "SELECT id, name, start_url, database_path, created_at_ms, updated_at_ms,
                mode, status, crawled FROM crawl_sessions WHERE id = ?1",
        [id],
        |row| session_from_row(row, current_session_id),
    )
    .optional()
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "crawl session not found; refresh the crawl library".to_string())
}

fn session_from_row(
    row: &rusqlite::Row<'_>,
    current_session_id: Option<&str>,
) -> rusqlite::Result<CrawlSession> {
    let id: String = row.get(0)?;
    let mode: String = row.get(6)?;
    let database_path: String = row.get(3)?;
    let status = if Path::new(&database_path).is_file() {
        row.get(7)?
    } else {
        "unavailable".to_string()
    };
    Ok(CrawlSession {
        is_current: current_session_id == Some(id.as_str()),
        id,
        name: row.get(1)?,
        start_url: row.get(2)?,
        database_path,
        created_at_ms: row.get(4)?,
        updated_at_ms: row.get(5)?,
        mode: if mode == "list" {
            CrawlMode::List
        } else {
            CrawlMode::Spider
        },
        status,
        crawled: row
            .get::<_, Option<i64>>(8)?
            .map(|value| value.max(0) as usize),
        config: None,
    })
}

pub(crate) fn load_config(conn: &Connection, session: &mut CrawlSession) -> Result<(), String> {
    let json: Option<String> = conn
        .query_row(
            "SELECT config_json FROM crawl_sessions WHERE id = ?1",
            [&session.id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    session.config = json
        .map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|error| format!("saved crawl configuration is invalid: {error}"))?;
    Ok(())
}

pub(crate) fn mode_name(mode: CrawlMode) -> &'static str {
    match mode {
        CrawlMode::Spider => "spider",
        CrawlMode::List => "list",
    }
}

pub(crate) fn seed_url(config: &CrawlConfig) -> &str {
    if config.mode == CrawlMode::List {
        config
            .list_urls
            .iter()
            .chain(&config.list_sitemap_urls)
            .map(|url| url.trim())
            .find(|url| !url.is_empty())
            .unwrap_or(config.start_url.trim())
    } else {
        config.start_url.trim()
    }
}

pub(crate) fn create_session(
    conn: &Connection,
    dir: &Path,
    name: &str,
    start_url: &str,
    config: Option<&CrawlConfig>,
    status: &str,
) -> Result<(CrawlSession, SqliteStore), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("session name is required".to_string());
    }
    let config_json = config
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    let sessions_dir = dir.join("sessions");
    fs::create_dir_all(&sessions_dir)
        .map_err(|error| format!("failed to create sessions directory: {error}"))?;
    let suffix: String = conn
        .query_row("SELECT lower(hex(randomblob(8)))", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let now = now_ms();
    let id = format!("session-{now}-{suffix}");
    let path = sessions_dir.join(format!("{id}.sqlite3"));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("failed to create crawl database: {error}"))?;
    let create = || -> Result<(CrawlSession, SqliteStore), String> {
        let store = SqliteStore::open(&path)
            .map_err(|error| format!("failed to create crawl database: {error}"))?;
        conn.execute(
            "INSERT INTO crawl_sessions
             (id, name, start_url, database_path, created_at_ms, updated_at_ms, mode, status, crawled, config_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?8, 0, ?7)",
            params![id, name, start_url.trim(), path.to_string_lossy().as_ref(), now,
                mode_name(config.map(|config| config.mode).unwrap_or_default()), config_json, status],
        ).map_err(|error| error.to_string())?;
        Ok((get_session(conn, &id, Some(&id))?, store))
    };
    match create() {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = conn.execute("DELETE FROM crawl_sessions WHERE id = ?1", [&id]);
            let _ = fs::remove_file(path);
            Err(error)
        }
    }
}

pub(crate) fn open_existing_database(path: &Path) -> Result<SqliteStore, String> {
    let _existing = read_connection(path)?;
    let absolute = fs::canonicalize(path).map_err(|error| error.to_string())?;
    let mut uri = url::Url::from_file_path(&absolute)
        .map_err(|_| "crawl database path cannot be opened".to_string())?;
    // SQLite enforces no creation even if the file disappears after the availability check.
    uri.set_query(Some("mode=rw"));
    SqliteStore::open(uri.as_str())
        .map_err(|error| format!("failed to open crawl database {}: {error}", path.display()))
}

fn read_connection(path: &Path) -> Result<Connection, String> {
    if !path.is_file() {
        return Err(format!(
            "crawl database is unavailable at {}. Restore the file to this location or remove its saved card.",
            path.display()
        ));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("failed to read crawl database {}: {error}", path.display()))?;
    conn.query_row("SELECT 1 FROM crawl_records LIMIT 1", [], |_| Ok(()))
        .optional()
        .map_err(|error| format!("invalid crawl database {}: {error}", path.display()))?;
    Ok(conn)
}

pub(crate) fn read_database_metadata(path: &Path) -> Result<(String, CrawlMode, usize), String> {
    let conn = read_connection(path)?;
    let count = conn
        .query_row("SELECT COUNT(*) FROM crawl_records", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| error.to_string())?;
    let mut url: String = conn
        .query_row(
            "SELECT url FROM crawl_records ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let has_list_position: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('crawl_records') WHERE name = 'list_position')", [], |row| row.get(0)).map_err(|error| error.to_string())?;
    let mut list: bool = has_list_position
        && conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM crawl_records WHERE list_position IS NOT NULL)",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
    if count == 0 {
        let has_frontier: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'crawl_frontier_queue')",
            [], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        if has_frontier {
            url = conn
                .query_row(
                    "SELECT url FROM crawl_frontier_queue ORDER BY position LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?
                .unwrap_or_default();
            let has_list_position: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('crawl_frontier_queue') WHERE name = 'list_position')",
                [], |row| row.get(0),
            ).map_err(|error| error.to_string())?;
            list = has_list_position && conn.query_row("SELECT EXISTS(SELECT 1 FROM crawl_frontier_queue WHERE list_position IS NOT NULL)", [], |row| row.get(0))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok((
        url,
        if list {
            CrawlMode::List
        } else {
            CrawlMode::Spider
        },
        count as usize,
    ))
}

pub(crate) fn save_progress(
    conn: &Connection,
    id: &str,
    status: &str,
    crawled: usize,
) -> Result<(), String> {
    conn.execute(
        "UPDATE crawl_sessions SET status = ?1, crawled = ?2, updated_at_ms = ?3 WHERE id = ?4",
        params![status, crawled as i64, now_ms(), id],
    )
    .map_err(|error| format!("failed to save crawl progress: {error}"))?;
    Ok(())
}

pub(crate) fn save_config(conn: &Connection, id: &str, config: &CrawlConfig) -> Result<(), String> {
    let json = serde_json::to_string(config).map_err(|error| error.to_string())?;
    conn.execute(
        "UPDATE crawl_sessions SET config_json = ?1, mode = ?2, start_url = ?3 WHERE id = ?4",
        params![json, mode_name(config.mode), seed_url(config), id],
    )
    .map_err(|error| format!("failed to save crawl configuration: {error}"))?;
    Ok(())
}

pub(crate) fn save_status(conn: &Connection, id: &str, status: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE crawl_sessions SET status = ?1, updated_at_ms = ?2 WHERE id = ?3",
        params![status, now_ms(), id],
    )
    .map_err(|error| format!("failed to save crawl status: {error}"))?;
    Ok(())
}

pub(crate) fn validate_resume(session: &CrawlSession, config: &CrawlConfig) -> Result<(), String> {
    if session.mode != config.mode
        || (!session.start_url.is_empty() && session.start_url != seed_url(config))
        || session.config.as_ref().is_some_and(|saved| {
            saved.start_url.trim() != config.start_url.trim()
                || [
                    (&saved.list_urls, &config.list_urls),
                    (&saved.list_sitemap_urls, &config.list_sitemap_urls),
                ]
                .into_iter()
                .any(|(previous, current)| {
                    previous
                        .iter()
                        .map(|url| url.trim())
                        .filter(|url| !url.is_empty())
                        .ne(current
                            .iter()
                            .map(|url| url.trim())
                            .filter(|url| !url.is_empty()))
                })
        })
    {
        return Err("resume must use the saved crawl mode and target URLs; start a new crawl for different targets".to_string());
    }
    Ok(())
}

pub(crate) fn resume_count(path: &Path) -> Result<usize, String> {
    let conn = read_connection(path)?;
    let queued: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM crawl_frontier_queue)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !queued {
        return Err("this saved crawl has no queued URLs to resume; start a new crawl".to_string());
    }
    conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM crawl_frontier_meta WHERE key = 'crawled'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map(|count| count.unwrap_or_default().max(0) as usize)
    .map_err(|error| error.to_string())
}

pub(crate) fn register_database(conn: &Connection, path: &Path) -> Result<CrawlSession, String> {
    let (start_url, mode, crawled) = read_database_metadata(path)?;
    let suffix: String = conn
        .query_row("SELECT lower(hex(randomblob(8)))", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let now = now_ms();
    let id = format!("session-{now}-{suffix}");
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Imported database");
    conn.execute(
        "INSERT INTO crawl_sessions
        (id, name, start_url, database_path, created_at_ms, updated_at_ms, mode, status, crawled)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, 'unknown', ?7)",
        params![
            id,
            name,
            start_url,
            path.to_string_lossy().as_ref(),
            now,
            mode_name(mode),
            crawled as i64
        ],
    )
    .map_err(|error| error.to_string())?;
    get_session(conn, &id, Some(&id))
}

pub(crate) fn compare_sessions(
    index: &Connection,
    baseline_id: &str,
    current_id: &str,
    active_id: Option<&str>,
) -> Result<crate::CrawlComparisonResponse, String> {
    if baseline_id == current_id {
        return Err("choose two different saved crawls to compare".to_string());
    }
    if active_id.is_some_and(|id| id == baseline_id || id == current_id) {
        return Err("stop the selected active crawl before comparing it".to_string());
    }
    let baseline = get_session(index, baseline_id, None)?;
    let current = get_session(index, current_id, None)?;
    let baseline_path = Path::new(&baseline.database_path);
    let current_path = Path::new(&current.database_path);
    let conn = read_connection(baseline_path)?;
    let _current = read_connection(current_path)?;
    let baseline_path = fs::canonicalize(baseline_path).map_err(|error| error.to_string())?;
    let current_path = fs::canonicalize(current_path).map_err(|error| error.to_string())?;
    if baseline_path == current_path {
        return Err("these saved cards refer to the same crawl database".to_string());
    }
    let mut current_uri = url::Url::from_file_path(&current_path)
        .map_err(|_| "crawl database path cannot be opened".to_string())?;
    current_uri.set_query(Some("mode=ro"));
    conn.execute("ATTACH DATABASE ?1 AS comparison", [current_uri.as_str()])
        .map_err(|error| format!("failed to open comparison database: {error}"))?;
    conn.execute_batch("PRAGMA query_only = ON; BEGIN;")
        .map_err(|error| error.to_string())?;
    compare_databases(&conn).map_err(|error| format!("failed to compare saved crawls: {error}"))
}

fn comparison_projection(conn: &Connection, schema: &str) -> rusqlite::Result<String> {
    let columns = conn
        .prepare(&format!("PRAGMA {schema}.table_info(crawl_records)"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
    let fields = [
        "id",
        "final_url",
        "status_code",
        "title",
        "meta_description",
        "indexability",
        "indexability_status",
        "response_hash",
        "list_position",
        "content_type",
        "h1",
        "error",
        "status_text",
        "near_duplicate_cluster_id",
    ]
    .into_iter()
    .map(|field| {
        if columns.contains(field) {
            field.to_string()
        } else {
            format!("NULL AS {field}")
        }
    })
    .collect::<Vec<_>>()
    .join(", ");
    Ok(format!("SELECT {fields} FROM {schema}.crawl_records"))
}

fn compare_databases(conn: &Connection) -> rusqlite::Result<crate::CrawlComparisonResponse> {
    conn.create_scalar_function(
        "ff_comparison_text_key",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            Ok(context
                .get::<Option<String>>(0)?
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase())
        },
    )?;
    let baseline_sql = comparison_projection(conn, "main")?;
    let current_sql = comparison_projection(conn, "comparison")?;
    let cte = comparison_cte(&baseline_sql, &current_sql);
    let count = |row: &rusqlite::Row<'_>, index| {
        row.get::<_, i64>(index).map(|value| value.max(0) as usize)
    };
    let mut result = conn.query_row(
        &format!(
            "{cte} SELECT
            (SELECT COUNT(*) FROM baseline_rows), (SELECT COUNT(*) FROM current_rows),
            COALESCE(SUM(change_order = 0), 0), COALESCE(SUM(change_order = 1), 0),
            COALESCE(SUM(change_order = 2), 0), COALESCE(SUM(status_diff), 0),
            COALESCE(SUM(title_diff), 0), COALESCE(SUM(meta_diff), 0),
            COALESCE(SUM(indexability_diff), 0), COALESCE(SUM(hash_diff), 0)
         FROM changes"
        ),
        [],
        |row| {
            Ok(crate::CrawlComparisonResponse {
                baseline_records: count(row, 0)?,
                current_records: count(row, 1)?,
                added: count(row, 2)?,
                removed: count(row, 3)?,
                changed: count(row, 4)?,
                status_changed: count(row, 5)?,
                title_changed: count(row, 6)?,
                meta_description_changed: count(row, 7)?,
                indexability_changed: count(row, 8)?,
                hash_changed: count(row, 9)?,
                rows: Vec::new(),
                metric_deltas: Vec::new(),
            })
        },
    )?;
    result.rows = conn
        .prepare(&format!(
            "{cte} SELECT url, change, previous_status_code, current_status_code,
            previous_title, current_title, previous_indexability, current_indexability,
            previous_response_hash, current_response_hash
         FROM changes ORDER BY change_order, url LIMIT 1000"
        ))?
        .query_map([], |row| {
            Ok(crate::CrawlComparisonRow {
                url: row.get(0)?,
                change: row.get(1)?,
                previous_status_code: row.get(2)?,
                current_status_code: row.get(3)?,
                previous_title: row.get(4)?,
                current_title: row.get(5)?,
                previous_indexability: row.get(6)?,
                current_indexability: row.get(7)?,
                previous_response_hash: row.get(8)?,
                current_response_hash: row.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    result.metric_deltas = crate::comparison_metric_deltas(
        &comparison_summary(conn, &baseline_sql)?,
        &comparison_summary(conn, &current_sql)?,
    );
    Ok(result)
}

fn comparison_cte(baseline_sql: &str, current_sql: &str) -> String {
    let fields = "COALESCE(p.final_url, c.final_url) AS url,
                CASE WHEN p.id IS NULL THEN 0 WHEN c.id IS NULL THEN 1 ELSE 2 END AS change_order,
                p.status_code AS previous_status_code, c.status_code AS current_status_code,
                p.title AS previous_title, c.title AS current_title,
                p.indexability_status AS previous_indexability, c.indexability_status AS current_indexability,
                p.response_hash AS previous_response_hash, c.response_hash AS current_response_hash,
                (p.id IS NOT NULL AND c.id IS NOT NULL AND p.status_code IS NOT c.status_code) AS status_diff,
                (p.id IS NOT NULL AND c.id IS NOT NULL AND p.title IS NOT c.title) AS title_diff,
                (p.id IS NOT NULL AND c.id IS NOT NULL AND p.meta_description IS NOT c.meta_description) AS meta_diff,
                (p.id IS NOT NULL AND c.id IS NOT NULL AND (p.indexability IS NOT c.indexability OR p.indexability_status IS NOT c.indexability_status)) AS indexability_diff,
                (p.id IS NOT NULL AND c.id IS NOT NULL AND p.response_hash IS NOT c.response_hash) AS hash_diff";
    format!(
        "WITH baseline_rows AS ({baseline_sql}), current_rows AS ({current_sql}),
         baseline_ranked AS (
            SELECT *, ROW_NUMBER() OVER (PARTITION BY final_url ORDER BY COALESCE(list_position, id) DESC, id DESC) AS position
            FROM baseline_rows
         ), current_ranked AS (
            SELECT *, ROW_NUMBER() OVER (PARTITION BY final_url ORDER BY COALESCE(list_position, id) DESC, id DESC) AS position
            FROM current_rows
         ), previous AS MATERIALIZED (SELECT * FROM baseline_ranked WHERE position = 1),
         current AS MATERIALIZED (SELECT * FROM current_ranked WHERE position = 1),
         joined AS (
            SELECT {fields} FROM previous p LEFT JOIN current c ON p.final_url = c.final_url
            UNION ALL
            SELECT {fields} FROM current c LEFT JOIN previous p ON p.final_url = c.final_url
            WHERE p.id IS NULL
         ), changes AS (
            SELECT *, CASE change_order WHEN 0 THEN 'added' WHEN 1 THEN 'removed' ELSE 'changed' END AS change
            FROM joined WHERE change_order != 2 OR status_diff OR title_diff OR meta_diff OR indexability_diff OR hash_diff
         )"
    )
}

fn comparison_summary(
    conn: &Connection,
    projection: &str,
) -> rusqlite::Result<ferrous_frog_storage::CrawlSummary> {
    let cte = format!(
        "WITH records AS ({projection}), html AS (
        SELECT * FROM records WHERE {}
    )",
        ferrous_frog_storage::SUCCESS_HTML_SQL
    );
    let count = |sql: &str| {
        conn.query_row(&format!("{cte} {sql}"), [], |row| {
            row.get::<_, i64>(0).map(|value| value.max(0) as usize)
        })
    };
    let missing = |column| {
        count(&format!(
            "SELECT COUNT(*) FROM html WHERE ff_comparison_text_key({column}) = ''"
        ))
    };
    let duplicates = |column| {
        count(&format!(
            "SELECT COALESCE(SUM(matches), 0) FROM (
        SELECT COUNT(*) AS matches FROM html WHERE ff_comparison_text_key({column}) != ''
        GROUP BY ff_comparison_text_key({column}) HAVING COUNT(*) > 1
    )"
        ))
    };
    Ok(ferrous_frog_storage::CrawlSummary {
        total: count("SELECT COUNT(*) FROM records")?,
        broken: count("SELECT COUNT(*) FROM records WHERE status_code >= 400
            OR (status_code >= 300 AND status_code < 400 AND error IS NOT NULL)
            OR (status_code IS NULL AND error IS NOT NULL AND status_text != 'Blocked by robots.txt' AND error != 'Blocked by robots.txt')")?,
        indexable: count("SELECT COUNT(*) FROM records WHERE indexability = 'Indexable'")?,
        non_indexable: count("SELECT COUNT(*) FROM records WHERE indexability = 'Non-indexable'")?,
        title_missing: missing("title")?, title_duplicate: duplicates("title")?,
        meta_missing: missing("meta_description")?, meta_duplicate: duplicates("meta_description")?,
        h1_missing: missing("h1")?,
        near_duplicates: count("SELECT COALESCE(SUM(matches), 0) FROM (
            SELECT COUNT(*) AS matches FROM html WHERE near_duplicate_cluster_id IS NOT NULL
            GROUP BY near_duplicate_cluster_id HAVING COUNT(*) > 1
        )")?,
        ..ferrous_frog_storage::CrawlSummary::default()
    })
}

pub(crate) fn refresh_legacy_metadata(
    conn: &Connection,
    session: &mut CrawlSession,
) -> Result<(), String> {
    if session.config.is_some() || session.crawled.is_some() {
        return Ok(());
    }
    let (url, mode, count) = read_database_metadata(Path::new(&session.database_path))?;
    if session.start_url.is_empty() {
        session.start_url = url;
    }
    session.mode = mode;
    session.crawled = Some(count);
    if session.status == "unavailable" {
        session.status = "unknown".to_string();
    }
    conn.execute(
        "UPDATE crawl_sessions SET start_url = ?1, mode = ?2, crawled = ?3, status = ?5 WHERE id = ?4",
        params![session.start_url, mode_name(mode), count as i64, session.id, session.status],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod comparison_scaling_tests {
    use super::*;

    #[test]
    fn comparison_work_grows_with_records_instead_of_record_pairs() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("ATTACH DATABASE ':memory:' AS comparison;
            CREATE TABLE main.crawl_records (id INTEGER PRIMARY KEY, final_url TEXT NOT NULL);
            CREATE TABLE comparison.crawl_records (id INTEGER PRIMARY KEY, final_url TEXT NOT NULL);").unwrap();
        let steps = |size: usize| {
            conn.execute_batch(&format!(
                "DELETE FROM main.crawl_records; DELETE FROM comparison.crawl_records;
                INSERT INTO main.crawl_records WITH RECURSIVE numbers(n) AS (
                    VALUES(1) UNION ALL SELECT n + 1 FROM numbers WHERE n < {size}
                ) SELECT n, 'https://example.test/' || n FROM numbers;
                INSERT INTO comparison.crawl_records SELECT * FROM main.crawl_records;"
            ))
            .unwrap();
            let cte = comparison_cte(
                &comparison_projection(&conn, "main").unwrap(),
                &comparison_projection(&conn, "comparison").unwrap(),
            );
            let mut statement = conn
                .prepare(&format!("{cte} SELECT COUNT(*) FROM changes"))
                .unwrap();
            assert_eq!(
                statement.query_row([], |row| row.get::<_, i64>(0)).unwrap(),
                0
            );
            statement.get_status(rusqlite::StatementStatus::VmStep)
        };
        let small = steps(500);
        let large = steps(2_000);
        eprintln!("comparison VM steps: 500={small}, 2000={large}");
        assert!(
            large < small * 8,
            "comparison should not perform work for every pair: {small} -> {large}"
        );
    }
}
