//! Headless crawl and export command line.
//!
//! `ferrous-frog-cli crawl <url>` and `ferrous-frog-cli list <urls...|file>` run the same engine as
//! the desktop app, write bulk exports into an output folder and print a JSON summary on stdout.

use anyhow::{Context, Result, anyhow, bail};
use ferrous_frog_crawler_core::{
    BasicCredentials, CrawlConfig, CrawlControl, CrawlMode, CrawlerEvent, crawl,
    validate_crawl_start,
};
use ferrous_frog_export::{ExportKind, export_preset, write_export_files};
#[cfg(test)]
use ferrous_frog_storage::CrawlStore;
use ferrous_frog_storage::{ActiveStore, MemoryStore, SqliteStore};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const APP_IDENTIFIER: &str = "com.ferrousfrog.seospider";
const INDEX_FILE: &str = "ferrous-frog-sessions.sqlite3";

const USAGE: &str = "Ferrous Frog headless crawler

USAGE:
  ferrous-frog-cli crawl <start-url> [options]
  ferrous-frog-cli list <url>... | <file-with-one-url-per-line> [options]
  ferrous-frog-cli profiles [--app-data <dir>]

OPTIONS:
  --config <file.json>     Crawl configuration exported from the app (camelCase JSON)
  --profile <name>         Load a saved configuration profile from the app data directory
  --app-data <dir>         App data directory holding saved profiles (defaults per platform)
  --max-urls <n>           Override the URL budget
  --depth <n>              Override the link depth limit
  --output <dir>           Export folder (default: ./ferrous-frog-exports/<timestamp>)
  --export <kinds>         Comma-separated: csv, xlsx, sitemap, html, workbook, links, redirects
  --preset <name>          Export preset: basic (csv), audit (csv, workbook, html) or full
  --database <file>        Persist the crawl in this SQLite file instead of memory
  --quiet                  Suppress progress lines on stderr
  -h, --help               Show this help

ENVIRONMENT:
  FERROUS_FROG_BASIC_USER / FERROUS_FROG_BASIC_PASSWORD
                           Basic credentials sent only to the starting origin
  FERROUS_FROG_FORM_USER / FERROUS_FROG_FORM_PASSWORD
                           Credentials for the form login configured in the profile/config

The JSON summary printed on stdout lists the status, counts and written files.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Crawl { start_url: String },
    List { sources: Vec<String> },
    Profiles,
    Help,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    config_path: Option<PathBuf>,
    profile: Option<String>,
    app_data: Option<PathBuf>,
    max_urls: Option<usize>,
    max_depth: Option<usize>,
    output: Option<PathBuf>,
    exports: Vec<ExportKind>,
    database: Option<PathBuf>,
    quiet: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    command: Command,
    options: Options,
}

fn parse_args(args: &[String]) -> Result<Cli> {
    let mut positional = Vec::new();
    let mut options = Options::default();
    let mut iter = args.iter();
    let value = |flag: &str, iter: &mut std::slice::Iter<'_, String>| {
        iter.next()
            .cloned()
            .ok_or_else(|| anyhow!("{flag} requires a value"))
    };
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Ok(Cli {
                    command: Command::Help,
                    options,
                });
            }
            "--config" => options.config_path = Some(PathBuf::from(value(arg, &mut iter)?)),
            "--profile" => options.profile = Some(value(arg, &mut iter)?),
            "--app-data" => options.app_data = Some(PathBuf::from(value(arg, &mut iter)?)),
            "--output" => options.output = Some(PathBuf::from(value(arg, &mut iter)?)),
            "--database" => options.database = Some(PathBuf::from(value(arg, &mut iter)?)),
            "--max-urls" => {
                options.max_urls = Some(value(arg, &mut iter)?.parse().context("--max-urls")?)
            }
            "--depth" => {
                options.max_depth = Some(value(arg, &mut iter)?.parse().context("--depth")?)
            }
            "--export" => {
                for name in value(arg, &mut iter)?.split(',') {
                    let kind = ExportKind::parse(name)
                        .ok_or_else(|| anyhow!("unknown export kind: {name}"))?;
                    if !options.exports.contains(&kind) {
                        options.exports.push(kind);
                    }
                }
            }
            "--preset" => {
                let name = value(arg, &mut iter)?;
                for kind in
                    export_preset(&name).ok_or_else(|| anyhow!("unknown export preset: {name}"))?
                {
                    if !options.exports.contains(&kind) {
                        options.exports.push(kind);
                    }
                }
            }
            "--quiet" => options.quiet = true,
            other if other.starts_with('-') => bail!("unknown option: {other}"),
            _ => positional.push(arg.clone()),
        }
    }
    if options.config_path.is_some() && options.profile.is_some() {
        bail!("--config and --profile cannot be combined");
    }
    let command = match positional.first().map(String::as_str) {
        None => Command::Help,
        Some("crawl") => match positional.as_slice() {
            [_, url] => Command::Crawl {
                start_url: url.clone(),
            },
            _ => bail!("crawl expects exactly one start URL"),
        },
        Some("list") => {
            if positional.len() < 2 {
                bail!("list expects URLs or a file with one URL per line");
            }
            Command::List {
                sources: positional[1..].to_vec(),
            }
        }
        Some("profiles") => Command::Profiles,
        Some(other) => bail!("unknown command: {other}"),
    };
    Ok(Cli { command, options })
}

fn default_app_data_dir() -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let base = if cfg!(target_os = "windows") {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library").join("Application Support")
    } else {
        env("XDG_DATA_HOME").unwrap_or(env("HOME")?.join(".local").join("share"))
    };
    Some(base.join(APP_IDENTIFIER))
}

fn app_data_dir(options: &Options) -> Result<PathBuf> {
    options
        .app_data
        .clone()
        .or_else(default_app_data_dir)
        .ok_or_else(|| anyhow!("pass --app-data because no platform data directory was found"))
}

fn open_index(dir: &Path) -> Result<rusqlite::Connection> {
    let path = dir.join(INDEX_FILE);
    if !path.is_file() {
        bail!("no saved profiles: {} does not exist", path.display());
    }
    rusqlite::Connection::open(&path).with_context(|| format!("open {}", path.display()))
}

fn list_profiles(dir: &Path) -> Result<Vec<String>> {
    let conn = open_index(dir)?;
    let mut statement =
        conn.prepare("SELECT name FROM config_profiles ORDER BY updated_at_ms DESC, name ASC")?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

fn load_profile(dir: &Path, name: &str) -> Result<CrawlConfig> {
    let conn = open_index(dir)?;
    let json: String = conn
        .query_row(
            "SELECT config_json FROM config_profiles WHERE name = ?1
             ORDER BY updated_at_ms DESC LIMIT 1",
            [name],
            |row| row.get(0),
        )
        .map_err(|_| anyhow!("profile not found: {name}"))?;
    serde_json::from_str(&json).with_context(|| format!("profile {name} is not readable"))
}

fn load_config_file(path: &Path) -> Result<CrawlConfig> {
    let json = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("parse {}", path.display()))
}

/// A single existing file argument is read as one URL per line; anything else is a URL.
fn list_sources(sources: &[String]) -> Result<Vec<String>> {
    if let [single] = sources
        && Path::new(single).is_file()
    {
        let text = fs::read_to_string(single).with_context(|| format!("read {single}"))?;
        return Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
            .collect());
    }
    Ok(sources.to_vec())
}

fn build_config(command: &Command, options: &Options) -> Result<CrawlConfig> {
    let mut config = if let Some(path) = &options.config_path {
        load_config_file(path)?
    } else if let Some(name) = &options.profile {
        load_profile(&app_data_dir(options)?, name)?
    } else {
        CrawlConfig::default()
    };
    config.resume_from_state = false;
    match command {
        Command::Crawl { start_url } => {
            config.mode = CrawlMode::Spider;
            config.start_url = start_url.clone();
        }
        Command::List { sources } => {
            config.mode = CrawlMode::List;
            config.list_urls = list_sources(sources)?;
            config.list_sitemap_urls.clear();
            config.start_url = config.list_urls.first().cloned().unwrap_or_default();
        }
        Command::Profiles | Command::Help => {}
    }
    if let Some(max_urls) = options.max_urls {
        config.max_urls = max_urls;
    }
    if let Some(max_depth) = options.max_depth {
        config.max_depth = max_depth;
    }
    config.basic_credentials = basic_credentials_from_env(
        std::env::var("FERROUS_FROG_BASIC_USER").ok(),
        std::env::var("FERROUS_FROG_BASIC_PASSWORD").ok(),
    )?;
    config.http_auth.enabled = config.basic_credentials.is_some();
    config.form_credentials = basic_credentials_from_env(
        std::env::var("FERROUS_FROG_FORM_USER").ok(),
        std::env::var("FERROUS_FROG_FORM_PASSWORD").ok(),
    )?;
    if config.form_login.enabled && config.form_credentials.is_none() {
        bail!(
            "this configuration enables form login; set FERROUS_FROG_FORM_USER and FERROUS_FROG_FORM_PASSWORD"
        );
    }
    validate_crawl_start(&config)?;
    Ok(config)
}

fn basic_credentials_from_env(
    user: Option<String>,
    password: Option<String>,
) -> Result<Option<BasicCredentials>> {
    match (user, password) {
        (None, None) => Ok(None),
        (Some(username), Some(password)) if !username.trim().is_empty() && !password.is_empty() => {
            Ok(Some(BasicCredentials {
                username: username.trim().to_string(),
                password,
            }))
        }
        _ => bail!("set both FERROUS_FROG_BASIC_USER and FERROUS_FROG_BASIC_PASSWORD, or neither"),
    }
}

fn open_store(options: &Options) -> Result<ActiveStore> {
    Ok(match &options.database {
        Some(path) => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            ActiveStore::Sqlite(SqliteStore::open(path)?)
        }
        None => ActiveStore::Memory(MemoryStore::new()),
    })
}

fn output_dir(options: &Options) -> PathBuf {
    options.output.clone().unwrap_or_else(|| {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        PathBuf::from("ferrous-frog-exports").join(stamp.to_string())
    })
}

async fn run(cli: Cli) -> Result<serde_json::Value> {
    let config = build_config(&cli.command, &cli.options)?;
    let store = open_store(&cli.options)?;
    let quiet = cli.options.quiet;
    let last_report = Mutex::new(Instant::now() - Duration::from_secs(1));
    let on_event = move |event: CrawlerEvent| {
        if quiet {
            return;
        }
        let Some(progress) = event.progress else {
            return;
        };
        let mut last = last_report
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if event.kind != "finished" && last.elapsed() < Duration::from_secs(1) {
            return;
        }
        *last = Instant::now();
        eprintln!(
            "{}: crawled {} queued {} discovered {} ({:.1} pages/s)",
            progress.status,
            progress.crawled,
            progress.queued,
            progress.discovered,
            progress.pages_per_second
        );
    };
    let progress = crawl(
        config.clone(),
        store.clone(),
        CrawlControl::default(),
        on_event,
    )
    .await?;
    let dir = output_dir(&cli.options);
    let files = write_export_files(&store, &config.thresholds, &cli.options.exports, &dir)
        .map_err(|error| anyhow!(error))?;
    Ok(serde_json::json!({
        "status": progress.status,
        "crawled": progress.crawled,
        "queued": progress.queued,
        "discovered": progress.discovered,
        "elapsedMs": progress.elapsed_ms,
        "broken": progress.summary.broken,
        "outputDir": if files.is_empty() { serde_json::Value::Null } else { dir.display().to_string().into() },
        "files": files.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "database": cli.options.database.as_ref().map(|path| path.display().to_string()),
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("error: {error:#}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let outcome = match &cli.command {
        Command::Help => {
            println!("{USAGE}");
            return;
        }
        Command::Profiles => app_data_dir(&cli.options)
            .and_then(|dir| list_profiles(&dir))
            .map(|names| serde_json::json!({ "profiles": names })),
        Command::Crawl { .. } | Command::List { .. } => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("start async runtime")
            .and_then(|runtime| runtime.block_on(run(cli))),
    };
    match outcome {
        Ok(summary) => println!("{summary}"),
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_commands_options_and_presets() {
        let cli = parse_args(&args(&[
            "crawl",
            "https://example.test/",
            "--max-urls",
            "50",
            "--depth",
            "2",
            "--preset",
            "audit",
            "--export",
            "csv,links",
            "--quiet",
            "--output",
            "out",
        ]))
        .unwrap();
        assert_eq!(
            cli.command,
            Command::Crawl {
                start_url: "https://example.test/".into()
            }
        );
        assert_eq!(cli.options.max_urls, Some(50));
        assert_eq!(cli.options.max_depth, Some(2));
        assert_eq!(
            cli.options.exports,
            vec![
                ExportKind::Csv,
                ExportKind::AuditWorkbook,
                ExportKind::HtmlReport,
                ExportKind::LinksCsv
            ]
        );
        assert!(cli.options.quiet);
        assert_eq!(cli.options.output, Some(PathBuf::from("out")));
        assert_eq!(
            parse_args(&args(&["list", "https://a.test/", "https://b.test/"]))
                .unwrap()
                .command,
            Command::List {
                sources: args(&["https://a.test/", "https://b.test/"])
            }
        );
        assert_eq!(parse_args(&[]).unwrap().command, Command::Help);
        assert_eq!(
            parse_args(&args(&["profiles", "--app-data", "/tmp/x"]))
                .unwrap()
                .command,
            Command::Profiles
        );
        for invalid in [
            vec!["crawl"],
            vec!["crawl", "a", "b"],
            vec!["list"],
            vec!["crawl", "a", "--export", "pdf"],
            vec!["crawl", "a", "--preset", "everything"],
            vec!["crawl", "a", "--bogus"],
            vec!["crawl", "a", "--config", "x", "--profile", "y"],
            vec!["crawl", "a", "--max-urls"],
        ] {
            assert!(parse_args(&args(&invalid)).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn basic_credentials_require_both_environment_values() {
        assert_eq!(basic_credentials_from_env(None, None).unwrap(), None);
        assert!(basic_credentials_from_env(Some("u".into()), None).is_err());
        assert!(basic_credentials_from_env(Some(" ".into()), Some("p".into())).is_err());
        assert_eq!(
            basic_credentials_from_env(Some(" frog ".into()), Some("p".into())).unwrap(),
            Some(BasicCredentials {
                username: "frog".into(),
                password: "p".into()
            })
        );
    }

    #[test]
    fn list_sources_read_files_or_urls_and_profiles_come_from_the_app_index() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("urls.txt");
        fs::write(&file, "# comment\nhttps://a.test/\n\n https://b.test/ \n").unwrap();
        assert_eq!(
            list_sources(&[file.display().to_string()]).unwrap(),
            args(&["https://a.test/", "https://b.test/"])
        );
        assert_eq!(
            list_sources(&args(&["https://c.test/"])).unwrap(),
            args(&["https://c.test/"])
        );

        assert!(list_profiles(dir.path()).is_err());
        let conn = rusqlite::Connection::open(dir.path().join(INDEX_FILE)).unwrap();
        conn.execute_batch(
            "CREATE TABLE config_profiles (id TEXT PRIMARY KEY, name TEXT NOT NULL,
             config_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);",
        )
        .unwrap();
        let config = CrawlConfig {
            max_urls: 77,
            ..CrawlConfig::default()
        };
        conn.execute(
            "INSERT INTO config_profiles VALUES ('p1', 'Docs', ?1, 1, 1)",
            [serde_json::to_string(&config).unwrap()],
        )
        .unwrap();
        assert_eq!(list_profiles(dir.path()).unwrap(), args(&["Docs"]));
        let options = Options {
            profile: Some("Docs".into()),
            app_data: Some(dir.path().to_path_buf()),
            max_depth: Some(1),
            ..Options::default()
        };
        let command = Command::Crawl {
            start_url: "https://example.test/".into(),
        };
        let built = build_config(&command, &options).unwrap();
        assert_eq!(built.max_urls, 77);
        assert_eq!(built.max_depth, 1);
        assert_eq!(built.start_url, "https://example.test/");
        assert!(
            build_config(
                &command,
                &Options {
                    profile: Some("Missing".into()),
                    ..options
                }
            )
            .is_err()
        );
    }

    async fn spawn_site() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 2048];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (status, body) = match path {
                        "/" => (
                            "200 OK",
                            "<html><head><title>Home</title></head><body><a href=\"/a\">A</a><a href=\"/missing\">M</a></body></html>",
                        ),
                        "/a" => (
                            "200 OK",
                            "<html><head><title>A</title></head><body>a</body></html>",
                        ),
                        _ => ("404 Not Found", "<h1>missing</h1>"),
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (base_url, handle)
    }

    #[tokio::test]
    async fn crawl_and_list_commands_export_into_the_output_folder() {
        let (base_url, server) = spawn_site().await;
        let dir = tempfile::tempdir().unwrap();
        let mut config = CrawlConfig {
            respect_robots: false,
            requests_per_second: 50,
            request_delay_ms: 0,
            ..CrawlConfig::default()
        };
        config.sitemap.enabled = false;
        let config_path = dir.path().join("config.json");
        fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();

        let out = dir.path().join("out");
        let summary = run(Cli {
            command: Command::Crawl {
                start_url: base_url.clone(),
            },
            options: Options {
                config_path: Some(config_path.clone()),
                output: Some(out.clone()),
                exports: ExportKind::ALL.to_vec(),
                database: Some(dir.path().join("crawl.sqlite3")),
                quiet: true,
                max_depth: Some(1),
                ..Options::default()
            },
        })
        .await
        .unwrap();
        assert_eq!(summary["status"], "finished");
        assert_eq!(summary["crawled"], 3);
        assert_eq!(summary["broken"], 1);
        for kind in ExportKind::ALL {
            let path = out.join(kind.file_name());
            assert!(path.is_file(), "{}", path.display());
            assert!(fs::metadata(&path).unwrap().len() > 0, "{}", path.display());
        }
        let csv = fs::read_to_string(out.join("crawl.csv")).unwrap();
        assert_eq!(csv.lines().count(), 4);
        assert!(
            fs::read_to_string(out.join("seo-report.html"))
                .unwrap()
                .contains("missing")
        );
        let reopened = SqliteStore::open(dir.path().join("crawl.sqlite3")).unwrap();
        assert_eq!(reopened.records().len(), 3);

        let list_out = dir.path().join("list");
        let summary = run(Cli {
            command: Command::List {
                sources: vec![format!("{base_url}a"), format!("{base_url}missing")],
            },
            options: Options {
                config_path: Some(config_path),
                output: Some(list_out.clone()),
                exports: vec![ExportKind::Csv],
                quiet: true,
                ..Options::default()
            },
        })
        .await
        .unwrap();
        assert_eq!(summary["crawled"], 2);
        assert_eq!(summary["files"].as_array().unwrap().len(), 1);
        assert!(list_out.join("crawl.csv").is_file());
        assert_eq!(summary["database"], serde_json::Value::Null);
        server.abort();
    }
}
