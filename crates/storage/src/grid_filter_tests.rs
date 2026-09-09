use super::*;
use serde_json::{Value, json};

fn query_with_filters(filters: Value) -> GridQuery {
    let mut query = serde_json::to_value(GridQuery::default()).unwrap();
    query["filters"] = filters;
    serde_json::from_value(query).unwrap()
}

fn filtered_query(field: &str, operator: &str, value: &str) -> GridQuery {
    query_with_filters(json!({
        "match": "all",
        "rules": [{ "field": field, "operator": operator, "value": value }]
    }))
}

fn fixtures() -> (MemoryStore, SqliteStore) {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    for (path, status, title, depth, words, time) in [
        ("a", Some(200), Some(" \tCAFÉ  Guide\n"), 1, 100, 11),
        ("b", Some(404), Some(""), 3, 0, 25),
        ("c", None, Some("café Guide"), 0, 200, 7),
        ("d", Some(201), Some("100%_literal"), 2, 125, 12),
    ] {
        let mut record = CrawlRecord::pending(format!("https://example.test/{path}"), depth);
        record.status_code = status;
        record.content_type = Some("text/html".into());
        record.title = title.map(str::to_string);
        record.word_count = words;
        record.response_time_ms = time;
        record.indexability = if path == "b" {
            "Non-indexable"
        } else {
            "Indexable"
        }
        .into();
        if path == "a" {
            record.meta_description = Some("Fresh summary".into());
            record.canonical = Some("https://example.test/target".into());
        } else if path == "b" {
            record.final_url = "https://example.test/landing".into();
        } else if path == "c" {
            record.meta_description = Some(" Summary\tTwo ".into());
            record.canonical = Some("https://example.test/target?x=1".into());
            record.classification = UrlClassification::External;
        } else {
            record.meta_description = Some("\t\u{2003}".into());
        }
        memory.upsert(record.clone());
        sqlite.try_upsert(record).unwrap();
    }
    (memory, sqlite)
}

fn assert_query(memory: &MemoryStore, sqlite: &SqliteStore, query: GridQuery, expected: &[u64]) {
    let actual_memory = memory.query(query.clone());
    let actual_sqlite = sqlite.try_query(query.clone()).unwrap();
    for response in [actual_memory, actual_sqlite] {
        assert_eq!(
            response.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            expected,
            "{query:?}"
        );
        assert_eq!(response.total, expected.len(), "{query:?}");
        assert_eq!(response.summary.total, 4);
    }
}

#[test]
fn advanced_filters_match_typed_fields_with_unicode_and_literal_text_in_both_stores() {
    let (memory, sqlite) = fixtures();
    for (field, operator, value, expected) in [
        ("title", "equals", "cAfÉ\tguide", vec![1, 3]),
        ("title", "notEquals", "café guide", vec![2, 4]),
        ("title", "contains", "FÉ  GU", vec![1, 3]),
        ("title", "notContains", "fé gu", vec![2, 4]),
        ("title", "isEmpty", "", vec![2]),
        ("title", "isNotEmpty", "", vec![1, 3, 4]),
        ("title", "contains", "%_", vec![4]),
        ("title", "equals", "' OR 1=1 --", vec![]),
        ("url", "equals", "HTTPS://EXAMPLE.TEST/B", vec![2]),
        ("finalUrl", "contains", "/landing", vec![2]),
        ("metaDescription", "isEmpty", "", vec![2, 4]),
        ("metaDescription", "equals", "SUMMARY TWO", vec![3]),
        ("canonical", "notContains", "target", vec![2, 4]),
        ("indexability", "equals", "non-indexable", vec![2]),
        ("statusCode", "equals", "200", vec![1]),
        ("statusCode", "notEquals", "200", vec![2, 4]),
        ("statusCode", "lessThan", "201", vec![1]),
        ("statusCode", "greaterThan", "200", vec![2, 4]),
        ("depth", "greaterThan", "1.5", vec![2, 4]),
        ("wordCount", "lessThan", "1.25e2", vec![1, 2]),
        ("responseTimeMs", "equals", " 12.0 ", vec![4]),
    ] {
        assert_query(
            &memory,
            &sqlite,
            filtered_query(field, operator, value),
            &expected,
        );
    }
}

#[test]
fn invalid_advanced_filters_never_return_an_unfiltered_dataset() {
    let (memory, sqlite) = fixtures();
    for query in [
        filtered_query("statusCode", "contains", "200"),
        filtered_query("title", "greaterThan", "1"),
        filtered_query("depth", "equals", ""),
        filtered_query("depth", "equals", "NaN"),
        filtered_query("wordCount", "equals", "inf"),
        filtered_query("responseTimeMs", "lessThan", "1e999"),
        filtered_query("title", "contains", &"x".repeat(2_001)),
        query_with_filters(json!({
            "match": "any",
            "rules": vec![json!({ "field": "url", "operator": "contains", "value": "" }); 21]
        })),
    ] {
        assert!(validate_grid_query(&query).is_err(), "{query:?}");
        assert!(sqlite.try_query(query.clone()).is_err(), "{query:?}");
        let response = memory.query(query.clone());
        assert_eq!(response.total, 0, "{query:?}");
        assert!(response.rows.is_empty(), "{query:?}");
    }
}

#[test]
fn advanced_groups_compose_with_views_search_segments_and_server_paging() {
    let (memory, sqlite) = fixtures();
    let rules = json!([
        { "field": "title", "operator": "contains", "value": "café" },
        { "field": "depth", "operator": "greaterThan", "value": "2" }
    ]);
    let all = query_with_filters(json!({ "match": "all", "rules": rules }));
    assert_query(&memory, &sqlite, all, &[]);
    let any = query_with_filters(json!({ "match": "any", "rules": rules }));
    assert_query(&memory, &sqlite, any.clone(), &[1, 2, 3]);

    let mut combined = any.clone();
    combined.view = IssueView::External;
    combined.global_search = Some("guide".into());
    combined.segment_pattern = Some("/[bc]$".into());
    combined.segment_regex = true;
    assert_query(&memory, &sqlite, combined, &[3]);

    let mut page = any;
    page.sort_by = Some("responseTimeMs".into());
    page.sort_dir = SortDirection::Desc;
    page.offset = 1;
    page.limit = 1;
    for response in [memory.query(page.clone()), sqlite.try_query(page).unwrap()] {
        assert_eq!(response.total, 3);
        assert_eq!(
            response.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [1]
        );
        assert_eq!(response.summary.total, 4);
    }

    for mode in ["all", "any"] {
        assert_query(
            &memory,
            &sqlite,
            query_with_filters(json!({ "match": mode, "rules": [] })),
            &[1, 2, 3, 4],
        );
    }
}

#[test]
fn advanced_filter_queries_preserve_old_json_and_export_page_state() {
    let old = serde_json::to_value(GridQuery::default()).unwrap();
    assert!(old.get("filters").is_none());
    assert!(
        serde_json::from_value::<GridQuery>(old.clone())
            .unwrap()
            .filters
            .is_none()
    );

    let mut query = filtered_query("title", "equals", "Café Guide");
    query.global_search = Some("example".into());
    query.segment_pattern = Some("^https:".into());
    query.segment_regex = true;
    query.sort_by = Some("url".into());
    query.sort_dir = SortDirection::Desc;
    let encoded = serde_json::to_value(&query).unwrap();
    let mut next_page: GridQuery = serde_json::from_value(encoded.clone()).unwrap();
    next_page.offset = 200;
    next_page.limit = 200;
    let mut next_encoded = serde_json::to_value(next_page).unwrap();
    next_encoded["offset"] = json!(query.offset);
    next_encoded["limit"] = json!(query.limit);
    assert_eq!(encoded, next_encoded);

    for filters in [
        json!({ "match": "none", "rules": [] }),
        json!({ "match": "all", "rules": [{ "field": "unknown", "operator": "equals", "value": "" }] }),
        json!({ "match": "all", "rules": [{ "field": "title", "operator": "unknown", "value": "" }] }),
        json!({ "match": "all", "rules": [{ "field": "title", "operator": "equals" }] }),
        json!({ "match": "all", "rules": [{ "field": "title", "operator": "equals", "value": "", "typo": true }] }),
    ] {
        let mut encoded = old.clone();
        encoded["filters"] = filters;
        assert!(serde_json::from_value::<GridQuery>(encoded).is_err());
    }
    let valid = filtered_query("title", "contains", &"é".repeat(2_000));
    validate_grid_query(&valid).unwrap();
}

#[test]
fn advanced_filters_only_decode_selected_sqlite_pages() {
    let (memory, sqlite) = fixtures();
    // An unrelated malformed payload must not be hydrated for counting, filtering or
    // sorting other fields. Only selecting that record should expose its error.
    sqlite
        .connection()
        .unwrap()
        .execute(
            "UPDATE crawl_records SET response_time_ms = 'bad-integer' WHERE id = 4",
            [],
        )
        .unwrap();
    assert!(sqlite.try_records().is_err());
    let mut query = filtered_query("title", "isNotEmpty", "");
    query.offset = 1;
    query.limit = 1;
    query.sort_by = Some("url".into());
    let response = sqlite.try_query(query.clone()).unwrap();
    assert_eq!(response.total, 3);
    assert_eq!(response.rows[0].id, 3);
    assert_eq!(response.rows[0].id, memory.query(query.clone()).rows[0].id);
    query.offset = 2;
    assert!(sqlite.try_query(query).is_err());
}

#[test]
fn numeric_filters_keep_integer_precision_and_handle_fractional_thresholds() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    let mut record = CrawlRecord::pending("https://example.test/large".into(), 0);
    record.response_time_ms = 9_007_199_254_740_993;
    memory.upsert(record.clone());
    sqlite.try_upsert(record).unwrap();
    for (operator, value, total) in [
        ("equals", "9007199254740993", 1),
        ("equals", "9007199254740992", 0),
        ("notEquals", "9007199254740992", 1),
        ("greaterThan", "9007199254740992", 1),
        ("lessThan", "9007199254740994", 1),
        ("lessThan", "1e100", 1),
        ("greaterThan", "-1", 1),
    ] {
        let query = filtered_query("responseTimeMs", operator, value);
        assert_eq!(memory.query(query.clone()).total, total, "{query:?}");
        assert_eq!(
            sqlite.try_query(query.clone()).unwrap().total,
            total,
            "{query:?}"
        );
    }
    for (operator, value, total) in [
        ("lessThan", "0.5", 1),
        ("equals", "0.5", 0),
        ("greaterThan", "-0.5", 1),
        ("equals", "-0", 1),
    ] {
        let query = filtered_query("depth", operator, value);
        assert_eq!(memory.query(query.clone()).total, total, "{query:?}");
        assert_eq!(
            sqlite.try_query(query.clone()).unwrap().total,
            total,
            "{query:?}"
        );
    }
}
