use super::*;

fn page(path: &str, value: Option<&str>) -> CrawlRecord {
    let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), 1);
    row.status_code = Some(200);
    row.content_type = Some("text/html".into());
    row.indexability = "Indexable".into();
    row.indexability_status = "Indexable".into();
    row.title = value.map(str::to_string);
    row.meta_description = row.title.clone();
    row.h1 = row.title.clone();
    row.h2 = row.title.clone();
    row.canonical = row.title.clone();
    row.title_count = Some(1);
    row.meta_description_count = Some(1);
    row.h1_count = 1;
    row.h2_count = 1;
    row.title_len = 5;
    row.meta_description_len = 5;
    row.title_pixel_width = 30;
    row.meta_description_pixel_width = 30;
    row
}

fn fixtures() -> (MemoryStore, SqliteStore) {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut rows = Vec::new();
    for (path, value) in [
        ("null", None),
        ("empty", Some("")),
        ("spaces", Some("   ")),
        ("ascii-whitespace", Some("\t\r\n\u{b}\u{c}")),
        ("nbsp", Some("\u{a0}")),
        ("em-space", Some("\u{2003}")),
        (
            "mixed-whitespace",
            Some(" \t\u{85}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\n "),
        ),
        ("zero-width-space", Some("\u{200b}")),
        ("bom", Some("\u{feff}")),
        ("populated", Some("\u{a0} Short \u{2003}")),
    ] {
        rows.push(page(path, value));
    }
    for position in [1, 0] {
        let mut row = page("list", Some("\u{3000}"));
        row.storage_key = format!("list:{position}:{}", row.url);
        row.list_position = Some(position);
        row.list_duplicate_index = position;
        rows.push(row);
    }
    for kind in [
        "error",
        "no-response",
        "robots",
        "non-html",
        "no-content-type",
        "incomplete",
        "noindex",
        "external",
    ] {
        let mut row = page(kind, Some("\t\u{a0}"));
        match kind {
            "error" => row.status_code = Some(404),
            "no-response" => {
                row.status_code = None;
                row.error = Some("Connection failed".into());
            }
            "robots" => {
                row.status_code = None;
                row.status_text = "Blocked by robots.txt".into();
                row.error = Some(row.status_text.clone());
            }
            "non-html" => row.content_type = Some("image/png".into()),
            "no-content-type" => row.content_type = None,
            "incomplete" => row.indexability_status = "Response body incomplete".into(),
            "noindex" => {
                row.indexability = "Non-indexable".into();
                row.indexability_status = "Meta robots noindex".into();
            }
            "external" => row.classification = UrlClassification::External,
            _ => unreachable!(),
        }
        rows.push(row);
    }
    for row in rows {
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    (memory, sqlite)
}

const MISSING_IDS: &[u64] = &[1, 2, 3, 4, 5, 6, 7, 11, 12, 19, 20];

fn assert_query(memory: &MemoryStore, sqlite: &SqliteStore, query: GridQuery, expected: &[u64]) {
    let memory = memory.query(query.clone());
    let sqlite = sqlite.try_query(query.clone()).unwrap();
    for (backend, response) in [("Memory", &memory), ("SQLite", &sqlite)] {
        let mut ids: Vec<_> = response.rows.iter().map(|row| row.id).collect();
        ids.sort_unstable();
        assert_eq!(ids, expected, "{backend}: {query:?}");
        assert_eq!(response.total, expected.len(), "{backend}: {query:?}");
    }
    assert_eq!(
        memory.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        sqlite.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        "Backend order differs: {query:?}"
    );
}

#[test]
fn metadata_whitespace_missing_views_and_summaries_match_both_backends() {
    let (memory, sqlite) = fixtures();
    for view in [
        IssueView::TitleMissing,
        IssueView::MetaMissing,
        IssueView::H1Missing,
        IssueView::H2Missing,
        IssueView::CanonicalMissing,
    ] {
        assert_query(
            &memory,
            &sqlite,
            GridQuery {
                view,
                ..GridQuery::default()
            },
            MISSING_IDS,
        );
    }
    for (actual, expected) in [
        (sqlite.progress_summary(), memory.progress_summary()),
        (sqlite.summary(), memory.summary()),
        (
            sqlite.try_query(GridQuery::default()).unwrap().summary,
            memory.query(GridQuery::default()).summary,
        ),
    ] {
        assert_eq!(expected.total, 20);
        assert_eq!(expected.title_missing, MISSING_IDS.len());
        assert_eq!(expected.meta_missing, MISSING_IDS.len());
        assert_eq!(expected.h1_missing, MISSING_IDS.len());
        assert_eq!(expected.h2_missing, MISSING_IDS.len());
        assert_eq!(expected.canonical_missing, MISSING_IDS.len());
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    // Queries classify whitespace without rewriting the retained evidence.
    let rows = sqlite.try_records().unwrap();
    assert_eq!(
        rows.iter()
            .find(|row| row.id == 5)
            .unwrap()
            .title
            .as_deref(),
        Some("\u{a0}")
    );
}

#[test]
fn metadata_whitespace_short_and_narrow_views_require_nonblank_text() {
    let (memory, sqlite) = fixtures();
    for view in [
        IssueView::TitleTooShort,
        IssueView::TitlePixelTooNarrow,
        IssueView::MetaTooShort,
        IssueView::MetaPixelTooNarrow,
    ] {
        assert_query(
            &memory,
            &sqlite,
            GridQuery {
                view,
                ..GridQuery::default()
            },
            &[8, 9, 10],
        );
    }
}

#[test]
fn metadata_whitespace_views_compose_with_advanced_empty_filters_and_list_rows() {
    let (memory, sqlite) = fixtures();
    for (view, field) in [
        (IssueView::TitleMissing, GridFilterField::Title),
        (IssueView::MetaMissing, GridFilterField::MetaDescription),
        (IssueView::CanonicalMissing, GridFilterField::Canonical),
    ] {
        for (match_mode, operator, expected) in [
            (
                GridFilterMatch::All,
                GridFilterOperator::IsEmpty,
                MISSING_IDS,
            ),
            (
                GridFilterMatch::All,
                GridFilterOperator::IsNotEmpty,
                &[][..],
            ),
            (
                GridFilterMatch::Any,
                GridFilterOperator::IsEmpty,
                MISSING_IDS,
            ),
        ] {
            let mut query = GridQuery {
                view: view.clone(),
                filters: Some(GridFilterGroup {
                    match_mode,
                    rules: vec![GridFilterRule {
                        field,
                        operator,
                        value: String::new(),
                    }],
                }),
                ..GridQuery::default()
            };
            assert_query(&memory, &sqlite, query.clone(), expected);
            if operator == GridFilterOperator::IsEmpty && match_mode == GridFilterMatch::All {
                query.filters.as_mut().unwrap().rules.push(GridFilterRule {
                    field: GridFilterField::Url,
                    operator: GridFilterOperator::Contains,
                    value: "/list".into(),
                });
                assert_query(&memory, &sqlite, query.clone(), &[11, 12]);
                let rows = sqlite.try_query(query).unwrap().rows;
                assert_eq!(rows.iter().map(|row| row.id).collect::<Vec<_>>(), [12, 11]);
                assert_eq!(rows[0].url, rows[1].url);
                assert_ne!(rows[0].storage_key, rows[1].storage_key);
            }
        }
    }
}

#[test]
fn metadata_whitespace_title_h1_comparison_keeps_ascii_case_and_interior_spacing() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for (index, (title, h1)) in [
        ("\u{a0} Café GUIDE\u{2003}", "\tCafé guide\n"),
        ("Café guide", "CAFÉ GUIDE"),
        ("Two  spaces", "two spaces"),
        ("Two\twords", "two words"),
        ("\u{200b}", "\u{200b}"),
        ("\u{feff}", "\u{feff}"),
        ("\u{a0}", "\u{a0}"),
        ("\u{a0} Same  gap\n", "same  GAP\u{2003}"),
        ("", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = page(&format!("pair-{index}"), Some(title));
        row.h1 = Some(h1.into());
        memory.upsert(row.clone());
        sqlite.try_upsert(row).unwrap();
    }
    assert_query(
        &memory,
        &sqlite,
        GridQuery {
            view: IssueView::TitleSameAsH1,
            ..GridQuery::default()
        },
        &[1, 5, 6, 8],
    );
}
