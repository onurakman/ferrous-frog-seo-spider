use super::*;
use ferrous_frog_storage::{
    CrawlStore, LinkEdgeQuery, LinkType, MemoryStore, RedirectHop, SqliteStore,
};
use std::process::{Command, Stdio};

type Sheets = Vec<(String, Vec<Vec<String>>)>;

// Python's standard library inspects the actual XLSX package without a test-only crate.
fn inspect_workbook(bytes: &[u8]) -> Sheets {
    let script = r#"
import io, json, sys, zipfile, xml.etree.ElementTree as ET
with zipfile.ZipFile(io.BytesIO(sys.stdin.buffer.read())) as archive:
    ns = {'s': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
    strings = []
    if 'xl/sharedStrings.xml' in archive.namelist():
        strings = [''.join(node.itertext()) for node in ET.fromstring(archive.read('xl/sharedStrings.xml'))]
    relationships = {node.attrib['Id']: node.attrib['Target'] for node in ET.fromstring(archive.read('xl/_rels/workbook.xml.rels'))}
    sheets = []
    for sheet in ET.fromstring(archive.read('xl/workbook.xml')).find('s:sheets', ns):
        target = relationships[sheet.attrib['{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id']]
        path = target.lstrip('/') if target.startswith('/') else 'xl/' + target
        rows = []
        for row in ET.fromstring(archive.read(path)).find('s:sheetData', ns):
            values = []
            for cell in row:
                column = 0
                for char in cell.attrib['r'].rstrip('0123456789'):
                    column = column * 26 + ord(char) - ord('A') + 1
                values.extend([''] * (column - len(values)))
                value = cell.findtext('s:v', '', ns)
                if cell.attrib.get('t') == 's':
                    value = strings[int(value)]
                elif cell.attrib.get('t') == 'inlineStr':
                    value = ''.join(cell.find('s:is', ns).itertext())
                values[column - 1] = value
            rows.append(values)
        sheets.append([sheet.attrib['name'], rows])
    print(json.dumps(sheets))
"#;
    let mut child = ["python3", "python"]
        .into_iter()
        .find_map(|program| {
            Command::new(program)
                .args(["-c", script])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .ok()
        })
        .expect("workbook inspection requires Python 3");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn page(path: &str, title: &str, description: &str) -> CrawlRecord {
    let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), 1);
    record.status_code = Some(200);
    record.status_text = "OK".into();
    record.content_type = Some("text/html".into());
    record.indexability = "Indexable".into();
    record.title = Some(title.into());
    record.title_len = title.chars().count();
    record.title_pixel_width = 400;
    record.meta_description = Some(description.into());
    record.meta_description_len = description.chars().count();
    record.meta_description_pixel_width = 800;
    record.canonical = Some(record.final_url.clone());
    record.canonical_count = 1;
    record
}

#[test]
fn metadata_counts_keep_unknown_cells_empty_and_export_multiple_tag_evidence() {
    let mut measured = page("measured", "Measured page", "Measured description");
    measured.title_count = Some(3);
    measured.meta_description_count = Some(2);
    let mut absent = page("absent", "", "");
    absent.title_count = Some(0);
    absent.meta_description_count = Some(0);
    let legacy = page("legacy", "Legacy page", "Legacy description");
    let records = [measured, absent, legacy];

    let html = records_to_html_report(&records, &[]).unwrap();
    assert!(html.contains("Multiple titles (3 tags)"));
    assert!(html.contains("Multiple meta descriptions (2 tags)"));
    assert!(!html.contains("Multiple titles (0 tags)"));

    let csv = records_to_csv_string(&records).unwrap();
    let mut reader = csv::Reader::from_reader(csv.as_bytes());
    let headers = reader.headers().unwrap().clone();
    let rows = reader.records().collect::<Result<Vec<_>, _>>().unwrap();
    let xlsx = inspect_workbook(&records_to_xlsx_bytes(&records).unwrap());
    for (name, expected) in [
        ("title_count", ["3", "0", ""]),
        ("meta_description_count", ["2", "0", ""]),
    ] {
        let column = headers.iter().position(|header| header == name).unwrap();
        assert_eq!(xlsx[0].1[0][column], name);
        for (index, value) in expected.iter().enumerate() {
            assert_eq!(&rows[index][column], *value);
            assert_eq!(
                xlsx[0].1[index + 1]
                    .get(column)
                    .map(String::as_str)
                    .unwrap_or(""),
                *value
            );
        }
    }

    let store = MemoryStore::new();
    for record in records {
        store.upsert(record);
    }
    let mut bytes = Vec::new();
    audit_workbook_to_writer(|query| Ok(store.query(query)), &mut bytes).unwrap();
    let sheets = inspect_workbook(&bytes)
        .into_iter()
        .collect::<HashMap<_, _>>();
    for (sheet, issue, field, value) in [
        ("Titles", "Multiple titles", "title_count", "3"),
        (
            "Descriptions",
            "Multiple descriptions",
            "meta_description_count",
            "2",
        ),
    ] {
        let rows = &sheets[sheet];
        let column = rows[0].iter().position(|header| header == field).unwrap();
        let matches = rows
            .iter()
            .skip(1)
            .filter(|row| row[4] == issue)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0][1], "https://example.test/measured");
        assert_eq!(matches[0][column], value);
    }
}

fn fixture() -> MemoryStore {
    let store = MemoryStore::new();
    let title = "Résumé 🐸 — a useful descriptive title";
    let description = "An informative shared description that explains the page and gives readers enough useful context.";
    let mut redirected = page("old", title, description);
    redirected.final_url = "https://example.test/résumé?x=1&y=2".into();
    redirected.redirect_target = Some(redirected.final_url.clone());
    redirected.redirect_chain.push(RedirectHop {
        url: redirected.url.clone(),
        status_code: 301,
        location: Some(redirected.final_url.clone()),
        dns_lookup_time_ms: None,
        tcp_connect_time_ms: None,
        tls_handshake_time_ms: None,
        ttfb_ms: None,
        elapsed_ms: None,
    });
    store.upsert(redirected);
    let mut duplicate = page("duplicate", title, description);
    duplicate.canonical_count = 2;
    store.upsert(duplicate);
    let mut missing = page("missing-metadata", "", "");
    missing.title = None;
    missing.meta_description = None;
    missing.canonical = None;
    missing.canonical_count = 0;
    store.upsert(missing);
    let mut short = page("short", "Tiny", "Short");
    short.title_pixel_width = 100;
    short.meta_description_pixel_width = 100;
    store.upsert(short);
    let mut long = page("long", &"T".repeat(61), &"D".repeat(161));
    long.title_pixel_width = 600;
    long.meta_description_pixel_width = 1000;
    long.h1 = long.title.clone();
    store.upsert(long);
    let mut broken = page("missing", title, description);
    broken.status_code = Some(404);
    broken.status_text = "Not Found".into();
    store.upsert(broken);
    store.add_link_edge(LinkEdge {
        id: 0,
        source_url: "https://example.test/source".into(),
        target_url: "https://example.test/missing".into(),
        anchor_text: "Missing résumé 🐸".into(),
        rel: String::new(),
        rel_nofollow: false,
        link_type: LinkType::Internal,
        source_status_code: Some(200),
        target_status_code: Some(404),
        source_depth: 0,
        target_depth: Some(1),
        source_position: 1,
        discovery_order: 1,
    });
    let mut offline = CrawlRecord::pending("https://example.test/offline".into(), 0);
    offline.error = Some("Connection refused".into());
    store.upsert(offline);
    let mut blocked = CrawlRecord::pending("https://example.test/robots".into(), 0);
    blocked.status_text = "Blocked by robots.txt".into();
    blocked.error = Some("Blocked by robots.txt".into());
    store.upsert(blocked);
    let mut image = page("image.png", title, description);
    image.content_type = Some("image/png".into());
    store.upsert(image);
    store.upsert(CrawlRecord::pending(
        "https://example.test/pending".into(),
        0,
    ));
    let mut unfollowed = CrawlRecord::pending("https://example.test/unfollowed".into(), 0);
    unfollowed.status_code = Some(302);
    unfollowed.redirect_target = Some("https://elsewhere.test/".into());
    store.upsert(unfollowed);
    store
}

#[test]
fn audit_workbook_contains_complete_crawl_and_typed_issue_tabs() {
    let store = fixture();
    let mut bytes = Vec::new();
    let mut page_count = 0;
    let row_count = audit_workbook_to_writer(
        |mut query| {
            assert!(query.limit <= 10_000);
            if query.limit > 0 {
                page_count += 1;
                query.limit = query.limit.min(2);
            }
            Ok(store.query(query))
        },
        &mut bytes,
    )
    .unwrap();
    assert_eq!(row_count, 11);
    assert!(page_count > 6);
    let sheets = inspect_workbook(&bytes);
    assert_eq!(
        sheets
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "Summary",
            "URLs",
            "Broken Links",
            "Redirects",
            "Titles",
            "Descriptions",
            "Canonicals",
            "Content"
        ]
    );
    let sheets = sheets.into_iter().collect::<HashMap<_, _>>();
    let summary = sheets["Summary"]
        .iter()
        .map(|row| (row[0].as_str(), row[1].as_str()))
        .collect::<HashMap<_, _>>();
    assert_eq!(summary["Crawl records"], "11");
    assert_eq!(summary["Broken URLs"], "2");
    assert_eq!(summary["Redirected URLs"], "2");
    assert_eq!(summary["Titles rows"], "8");
    assert_eq!(summary["Descriptions rows"], "7");
    assert_eq!(summary["Canonicals rows"], "2");
    assert_eq!(summary["Exact duplicate records"], "0");
    assert_eq!(summary["Content rows"], "0");
    assert_eq!(sheets["URLs"].len(), 12);
    assert_eq!(
        sheets["URLs"][0],
        [
            "id",
            "url",
            "final_url",
            "status_code",
            "content_type",
            "indexability",
            "indexability_status",
            "title",
            "meta_description",
            "h1",
            "canonical",
            "depth",
            "inlink_count",
            "outlink_count",
            "response_time_ms",
            "error"
        ]
    );
    assert_eq!(
        &sheets["URLs"][1][1..4],
        [
            "https://example.test/old",
            "https://example.test/résumé?x=1&y=2",
            "200"
        ]
    );
    assert_eq!(
        sheets["URLs"][1][7],
        "Résumé 🐸 — a useful descriptive title"
    );
    assert_eq!(sheets["Broken Links"].len(), 3);
    assert_eq!(
        sheets["Broken Links"][0],
        [
            "id",
            "url",
            "final_url",
            "status_code",
            "issue",
            "status_text",
            "error",
            "inlink_count",
            "first_inlink_source_url",
            "first_inlink_anchor_text"
        ]
    );
    assert_eq!(sheets["Broken Links"][1][9], "Missing résumé 🐸");
    assert_eq!(sheets["Broken Links"][2][6], "Connection refused");
    assert_eq!(sheets["Redirects"].len(), 3);
    assert_eq!(
        sheets["Redirects"][0],
        [
            "id",
            "url",
            "final_url",
            "status_code",
            "issue",
            "redirect_target",
            "redirect_chain",
            "error"
        ]
    );
    assert!(sheets["Redirects"][1][6].contains("301 https://example.test/old"));
    assert!(sheets["Redirects"][1][6].contains("https://example.test/résumé?x=1&y=2"));
    assert_eq!(
        sheets["Titles"][0],
        [
            "id",
            "url",
            "final_url",
            "status_code",
            "issue",
            "title",
            "title_len",
            "title_pixel_width",
            "h1",
            "title_count"
        ]
    );
    assert_eq!(
        sheets["Descriptions"][0],
        [
            "id",
            "url",
            "final_url",
            "status_code",
            "issue",
            "meta_description",
            "meta_description_len",
            "meta_description_pixel_width",
            "meta_description_count"
        ]
    );
    assert_eq!(
        sheets["Canonicals"][0],
        [
            "id",
            "url",
            "final_url",
            "status_code",
            "issue",
            "canonical",
            "canonical_count",
            "indexability",
            "indexability_status"
        ]
    );
    for (sheet, expected) in [
        (
            "Titles",
            vec![
                "Duplicate title",
                "Duplicate title",
                "Missing title",
                "Title matches H1",
                "Title too long",
                "Title too short",
                "Title too wide",
                "Title too narrow",
            ],
        ),
        (
            "Descriptions",
            vec![
                "Duplicate description",
                "Duplicate description",
                "Missing description",
                "Description too long",
                "Description too short",
                "Description too wide",
                "Description too narrow",
            ],
        ),
        (
            "Canonicals",
            vec!["Missing canonical", "Multiple canonicals"],
        ),
    ] {
        let mut issues = sheets[sheet]
            .iter()
            .skip(1)
            .map(|row| row[4].as_str())
            .collect::<Vec<_>>();
        issues.sort_unstable();
        let mut expected = expected;
        expected.sort_unstable();
        assert_eq!(issues, expected, "{sheet}");
    }
}

#[test]
fn empty_audit_workbook_keeps_all_headers_and_zero_counts() {
    let store = MemoryStore::new();
    let mut bytes = Vec::new();
    assert_eq!(
        audit_workbook_to_writer(|query| Ok(store.query(query)), &mut bytes).unwrap(),
        0
    );
    let sheets = inspect_workbook(&bytes);
    assert_eq!(sheets.len(), 8);
    assert_eq!(sheets[0].1[0], ["Metric", "Value"]);
    assert!(sheets[0].1.iter().any(|row| row == &["Crawl records", "0"]));
    for (_, rows) in &sheets[1..] {
        assert_eq!(rows.len(), 1);
        assert_eq!(&rows[0][..3], ["id", "url", "final_url"]);
    }
}

#[test]
fn content_workbook_sheet_keeps_exact_duplicate_hash_evidence_and_matching_occurrences() {
    let hash = "ab".repeat(32);
    let mut first = page("résumé-original", "Résumé 🐸", "Description");
    first.final_url = "https://example.test/résumé?x=1&y=2".into();
    first.response_hash = Some(hash.clone());
    first.size_bytes = 12_345;
    first.word_count = 123;
    first.indexability = "Non-Indexable".into();
    let mut second = first.clone();
    second.url = "https://outside.test/different".into();
    second.final_url = second.url.clone();
    second.storage_key = second.url.clone();
    second.classification = ferrous_frog_storage::UrlClassification::External;
    let mut list_duplicate = first.clone();
    list_duplicate.storage_key = format!("list:2:{}", first.url);
    list_duplicate.list_position = Some(2);
    list_duplicate.final_url.push_str("#fragment");
    let mut records = vec![first, second, list_duplicate];
    for (path, final_url) in [
        ("alias-one", "https://alias.test"),
        ("alias-two", "https://alias.test/#fragment"),
    ] {
        let mut record = page(path, "Alias only", "Description");
        record.final_url = final_url.into();
        record.response_hash = Some("cd".repeat(32));
        records.push(record);
    }
    let mut incomplete = page("incomplete", "Incomplete", "Description");
    incomplete.response_hash = Some(hash.clone());
    incomplete.indexability_status = "Response body incomplete".into();
    let mut error = page("error", "Error", "Description");
    error.response_hash = Some(hash.clone());
    error.status_code = Some(404);
    let mut image = page("image", "Image", "Description");
    image.response_hash = Some(hash.clone());
    image.content_type = Some("image/png".into());
    let mut unique = page("unique", "Unique", "Description");
    unique.response_hash = Some("ef".repeat(32));
    let mut blank = page("blank", "Blank", "Description");
    blank.response_hash = Some(String::new());
    records.extend([
        incomplete,
        error,
        image,
        unique,
        blank,
        page("no-hash", "No hash", "Description"),
    ]);

    let mut inspected = Vec::new();
    for store in [
        ferrous_frog_storage::ActiveStore::memory(),
        ferrous_frog_storage::ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        for record in &records {
            store.upsert(record.clone());
        }
        let mut bytes = Vec::new();
        let source_count = audit_workbook_to_writer(
            |mut query| {
                query.limit = query.limit.min(2);
                Ok(store.query(query))
            },
            &mut bytes,
        )
        .unwrap();
        assert_eq!(source_count, records.len());
        let sheets = inspect_workbook(&bytes);
        assert_eq!(sheets[7].0, "Content");
        let content = &sheets[7].1;
        assert_eq!(
            content[0],
            [
                "id",
                "url",
                "final_url",
                "status_code",
                "issue",
                "response_hash",
                "size_bytes",
                "content_type",
                "word_count",
                "indexability"
            ]
        );
        assert_eq!(content.len(), 4);
        assert_eq!(
            content
                .iter()
                .skip(1)
                .map(|row| row[0].as_str())
                .collect::<Vec<_>>(),
            ["1", "2", "3"]
        );
        assert!(
            content
                .iter()
                .skip(1)
                .all(|row| row[4] == "Exact duplicate response body"
                    && row[5] == hash
                    && row[6] == "12345"
                    && row[8] == "123")
        );
        assert_eq!(content[1][1], "https://example.test/résumé-original");
        assert_eq!(content[1][2], "https://example.test/résumé?x=1&y=2");
        assert_eq!(
            content[3][2],
            "https://example.test/résumé?x=1&y=2#fragment"
        );
        let summary = sheets[0]
            .1
            .iter()
            .map(|row| (row[0].as_str(), row[1].as_str()))
            .collect::<HashMap<_, _>>();
        assert_eq!(summary["Exact duplicate records"], "3");
        assert_eq!(summary["Content rows"], "3");
        assert!(summary["Response hashes"].contains("decoded response-body bytes"));
        inspected.push(sheets);
    }
    assert_eq!(inspected[0], inspected[1]);
}

#[test]
fn audit_workbook_preflights_url_and_combined_issue_row_limits() {
    for (source_count, title_count, sheet) in [(1_048_576, 0, "URLs"), (400_000, 400_000, "Titles")]
    {
        let mut bytes = Vec::new();
        let error = audit_workbook_to_writer(
            |query| {
                assert_eq!(
                    query.limit, 0,
                    "oversized workbooks must fail before reading data pages"
                );
                let total = match query.view {
                    IssueView::All => source_count,
                    IssueView::TitleMissing
                    | IssueView::TitleDuplicate
                    | IssueView::TitleTooShort => title_count,
                    _ => 0,
                };
                Ok(GridResponse {
                    rows: Vec::new(),
                    total,
                    summary: CrawlSummary {
                        total: source_count,
                        ..CrawlSummary::default()
                    },
                })
            },
            &mut bytes,
        )
        .unwrap_err();
        assert!(
            error.contains(sheet) && error.contains("1,048,575"),
            "{error}"
        );
        assert!(bytes.is_empty());
    }
}

#[test]
fn audit_workbook_refuses_changed_or_incomplete_pages_without_writing() {
    for change_total in [false, true] {
        let store = MemoryStore::new();
        store.upsert(CrawlRecord::pending("https://example.test/".into(), 0));
        let mut bytes = Vec::new();
        let error = audit_workbook_to_writer(
            |query| {
                let has_rows = query.limit > 0;
                let mut response = store.query(query);
                if has_rows {
                    if change_total {
                        response.total += 1;
                    } else {
                        response.rows.clear();
                    }
                }
                Ok(response)
            },
            &mut bytes,
        )
        .unwrap_err();
        assert!(error.contains("crawl changed"), "{error}");
        assert!(bytes.is_empty());
    }
}

#[test]
fn audit_workbook_sqlite_contents_match_memory_store() {
    let memory = fixture();
    let sqlite = SqliteStore::open(":memory:").unwrap();
    for record in memory.records() {
        sqlite.upsert(record);
    }
    for edge in memory.link_edges(LinkEdgeQuery::default()).edges {
        sqlite.add_link_edge(edge);
    }
    let mut memory_bytes = Vec::new();
    let mut sqlite_bytes = Vec::new();
    audit_workbook_to_writer(|query| Ok(memory.query(query)), &mut memory_bytes).unwrap();
    audit_workbook_to_writer(
        |mut query| {
            query.limit = query.limit.min(2);
            sqlite.try_query(query).map_err(|error| error.to_string())
        },
        &mut sqlite_bytes,
    )
    .unwrap();
    assert_eq!(
        inspect_workbook(&sqlite_bytes),
        inspect_workbook(&memory_bytes)
    );
}

#[test]
fn filtered_xlsx_stream_matches_legacy_columns_and_preserves_filters_and_order() {
    let memory = fixture();
    let store = SqliteStore::in_memory().unwrap();
    for mut record in memory.records() {
        if record.id <= 2 {
            record.list_position = Some(record.id as u32);
        }
        if record.id == 1 {
            record.title_count = Some(2);
            record.meta_description_count = Some(3);
        }
        store.upsert(record);
    }
    for edge in memory.link_edges(LinkEdgeQuery::default()).edges {
        store.add_link_edge(edge);
    }
    for sort_by in [None, Some("url".to_string())] {
        let query = GridQuery {
            offset: 99,
            limit: 1,
            global_search: Some("résumé".into()),
            segment_pattern: Some("old|duplicate".into()),
            segment_regex: true,
            sort_by: sort_by.clone(),
            view: IssueView::TitleDuplicate,
            ..GridQuery::default()
        };
        let mut bytes = Vec::new();
        let count = query_to_xlsx_writer(
            query.clone(),
            |mut page| {
                assert!(page.limit <= 10_000);
                page.limit = page.limit.min(1);
                Ok(store.query(page))
            },
            &mut bytes,
        )
        .unwrap();
        assert_eq!(count, 2);
        let sheets = inspect_workbook(&bytes);
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].0, "Crawl Results");
        let rows = &sheets[0].1;
        assert_eq!(rows[0].len(), 91);
        assert_eq!(&rows[0][89..], ["title_count", "meta_description_count"]);
        assert_eq!(
            &rows[0][..5],
            ["id", "url", "final_url", "classification", "status_code"]
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1][24], "Résumé 🐸 — a useful descriptive title");
        let expected_urls = if sort_by.is_some() {
            ["https://example.test/duplicate", "https://example.test/old"]
        } else {
            ["https://example.test/old", "https://example.test/duplicate"]
        };
        assert_eq!([rows[1][1].as_str(), rows[2][1].as_str()], expected_urls);
        assert!(
            rows.iter()
                .skip(1)
                .any(|row| row[2] == "https://example.test/résumé?x=1&y=2")
        );
        let expected_records = store
            .query(GridQuery {
                offset: 0,
                limit: 100,
                ..query
            })
            .rows;
        assert_eq!(
            sheets,
            inspect_workbook(&records_to_xlsx_bytes(&expected_records).unwrap())
        );
    }
}

#[test]
fn filtered_xlsx_stream_rejects_excel_overflow_before_writing() {
    let mut bytes = Vec::new();
    let error = query_to_xlsx_writer(
        GridQuery::default(),
        |query| {
            assert_eq!(query.limit, 0);
            Ok(GridResponse {
                rows: Vec::new(),
                total: 1_048_576,
                summary: CrawlSummary::default(),
            })
        },
        &mut bytes,
    )
    .unwrap_err();
    assert!(error.contains("1,048,575"), "{error}");
    assert!(bytes.is_empty());
}

#[test]
fn filtered_xlsx_keeps_advanced_groups_across_pages_and_rejects_invalid_rules_before_fetching() {
    let store = SqliteStore::in_memory().unwrap();
    for (path, title, depth) in [
        ("a", "Alpha", 1),
        ("b", "Beta", 3),
        ("c", "Alpha", 4),
        ("d", "Beta", 1),
    ] {
        let mut record = page(path, title, "Description");
        record.depth = depth;
        store.upsert(record);
    }
    for (mode, expected) in [
        ("all", vec!["https://example.test/c"]),
        (
            "any",
            vec![
                "https://example.test/c",
                "https://example.test/b",
                "https://example.test/a",
            ],
        ),
    ] {
        let query = GridQuery {
            offset: 99,
            limit: 1,
            sort_by: Some("depth".into()),
            sort_dir: ferrous_frog_storage::SortDirection::Desc,
            filters: Some(
                serde_json::from_value(serde_json::json!({
                    "match": mode,
                    "rules": [
                        {"field":"title", "operator":"contains", "value":"alpha"},
                        {"field":"depth", "operator":"greaterThan", "value":"2"}
                    ]
                }))
                .unwrap(),
            ),
            ..GridQuery::default()
        };
        let mut bytes = Vec::new();
        let mut offsets = Vec::new();
        let count = query_to_xlsx_writer(
            query.clone(),
            |mut page| {
                assert_eq!(page.filters, query.filters);
                assert_eq!(page.sort_by, query.sort_by);
                if page.limit > 0 {
                    offsets.push(page.offset);
                    page.limit = 1;
                }
                store.try_query(page).map_err(|error| error.to_string())
            },
            &mut bytes,
        )
        .unwrap();
        assert_eq!(count, expected.len());
        assert_eq!(offsets, (0..expected.len()).collect::<Vec<_>>());
        let sheets = inspect_workbook(&bytes);
        assert_eq!(
            sheets[0]
                .1
                .iter()
                .skip(1)
                .map(|row| row[1].as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }

    let query = GridQuery {
        filters: Some(
            serde_json::from_value(serde_json::json!({
                "match":"all", "rules":[{"field":"depth", "operator":"contains", "value":"2"}]
            }))
            .unwrap(),
        ),
        ..GridQuery::default()
    };
    let mut bytes = Vec::new();
    let mut fetched = false;
    let result = query_to_xlsx_writer(
        query,
        |_| {
            fetched = true;
            Ok(GridResponse {
                rows: Vec::new(),
                total: 0,
                summary: CrawlSummary::default(),
            })
        },
        &mut bytes,
    );
    assert!(
        result.is_err(),
        "invalid filters must fail before writing a workbook"
    );
    assert!(!fetched);
    assert!(bytes.is_empty());
}
