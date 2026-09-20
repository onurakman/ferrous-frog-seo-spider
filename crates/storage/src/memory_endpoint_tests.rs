use super::tests::test_edge;
use super::*;

fn record(path: &str, depth: usize, status: Option<u16>) -> CrawlRecord {
    let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), depth);
    row.status_code = status;
    row
}

// Preserve the previous full-scan semantics as an independent oracle for endpoint updates.
fn scan_edges(edges: &mut [LinkEdge], record: &CrawlRecord) {
    let aliases = record_url_aliases(record);
    for edge in edges {
        if !url_aliases(&edge.source_url).is_disjoint(&aliases) {
            edge.source_status_code = record.status_code;
            edge.source_depth = record.depth;
        }
        if !url_aliases(&edge.target_url).is_disjoint(&aliases) {
            edge.target_status_code = record.status_code;
            edge.target_depth = Some(record.depth);
        }
    }
}

#[test]
fn memory_endpoint_updates_visit_only_related_edges() {
    for edge_count in [50, 500] {
        let store = MemoryStore::new();
        for index in 0..edge_count {
            store.add_link_edge(test_edge(
                &format!("https://example.test/source/{index}"),
                &format!("https://example.test/target/{index}"),
                LinkType::Internal,
            ));
        }
        URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
        store.upsert(record("target/0", 3, Some(404)));
        let aliases = URL_ALIAS_EXPANSIONS.with(|count| count.get());
        eprintln!("endpoint update: {edge_count} edges; {aliases} alias expansions");
        assert!(
            aliases <= 24,
            "upsert reparsed unrelated edge endpoints: {aliases}"
        );
        let inner = store.inner.read().unwrap();
        assert_eq!(inner.link_edges[0].target_status_code, Some(404));
        assert_eq!(inner.link_edges[0].target_depth, Some(3));
        assert!(
            inner.link_edges[1..]
                .iter()
                .all(|edge| edge.target_status_code.is_none())
        );
    }
}

#[test]
fn memory_endpoint_updates_preserve_latest_upsert_aliases_none_and_clear() {
    let store = MemoryStore::new();
    let mut first = record("original", 1, Some(200));
    first.storage_key = "list:1:https://example.test/original".into();
    first.final_url = "https://example.test/shared".into();
    store.upsert(first.clone());
    let mut later = record("shared", 2, Some(404));
    later.storage_key = "list:2:https://example.test/shared".into();
    store.upsert(later.clone());
    let mut expected = Vec::new();
    for (source, target) in [
        (
            "https://example.test/shared",
            "https://example.test/shared#fragment",
        ),
        ("https://example.test/moved", "https://example.test/shared"),
        (
            "list:2:https://example.test/shared",
            "https://example.test/unrelated",
        ),
        ("HTTPS://EXAMPLE.TEST/#fragment", "https://example.test"),
    ] {
        expected.push(store.add_link_edge(test_edge(source, target, LinkType::Internal)));
    }
    assert_eq!(
        expected[0].target_status_code,
        Some(200),
        "append uses earliest matching occurrence"
    );
    later.status_code = Some(503);
    later.depth = 3;
    scan_edges(&mut expected, &later);
    store.upsert(later.clone());
    assert_eq!(store.inner.read().unwrap().link_edges, expected);
    assert_eq!(
        expected[0].target_status_code,
        Some(503),
        "refresh uses the latest matching upsert"
    );
    let appended = store.add_link_edge(test_edge(
        "https://example.test/shared",
        "https://example.test/shared",
        LinkType::Internal,
    ));
    assert_eq!(
        appended.target_status_code,
        Some(200),
        "a new edge still uses earliest lookup"
    );
    expected.push(appended);
    later.url = "https://example.test/moved".into();
    later.final_url = later.url.clone();
    later.status_code = Some(201);
    later.depth = 4;
    first.status_code = None;
    first.depth = 9;
    for row in [later, first, record("", 5, Some(202))] {
        scan_edges(&mut expected, &row);
        store.upsert(row);
        assert_eq!(store.inner.read().unwrap().link_edges, expected);
    }
    assert_eq!(expected[0].target_status_code, None);
    assert_eq!(expected[0].target_depth, Some(9));
    store.clear();
    let appended = store.add_link_edge(test_edge(
        "https://example.test/fresh",
        "https://example.test/moved",
        LinkType::Internal,
    ));
    assert_eq!(appended.id, 1);
    assert_eq!(appended.source_status_code, None);
    assert_eq!(appended.target_status_code, None);
    store.upsert(record("shared", 10, Some(418)));
    assert_eq!(store.inner.read().unwrap().link_edges, vec![appended]);
    store.upsert(record("moved", 11, Some(204)));
    assert_eq!(
        store.inner.read().unwrap().link_edges[0].target_status_code,
        Some(204)
    );
}

#[test]
#[ignore = "isolated Memory endpoint-update workload; run without browser or crawler workloads"]
fn memory_endpoint_update_workload() {
    for unrelated in [50, 5_000] {
        let store = MemoryStore::new();
        for index in 0..unrelated {
            store.add_link_edge(test_edge(
                &format!("https://example.test/source/{index}"),
                &format!("https://example.test/target/{index}"),
                LinkType::Internal,
            ));
        }
        for _ in 0..4 {
            store.add_link_edge(test_edge(
                "https://example.test/watched",
                "https://example.test/watched",
                LinkType::Internal,
            ));
        }
        store.upsert(record("watched", 1, Some(200)));
        let mut samples = Vec::new();
        for pass in 0..7 {
            URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
            let started = std::time::Instant::now();
            store.upsert(record("watched", pass, Some(200 + pass as u16)));
            samples.push(started.elapsed());
            let work = URL_ALIAS_EXPANSIONS.with(|count| count.get());
            let inner = store.inner.read().unwrap();
            assert!(
                inner.link_edges[..unrelated]
                    .iter()
                    .all(|edge| edge.source_status_code.is_none()
                        && edge.target_status_code.is_none())
            );
            assert!(
                inner.link_edges[unrelated..]
                    .iter()
                    .all(|edge| edge.source_status_code == Some(200 + pass as u16)
                        && edge.target_status_code == Some(200 + pass as u16))
            );
            if pass == 6 {
                samples.sort();
                eprintln!(
                    "endpoint_update unrelated={unrelated} matched=4 samples=7 median_ms={:.6} min_ms={:.6} max_ms={:.6} alias_expansions={work}",
                    samples[3].as_secs_f64() * 1000.0,
                    samples[0].as_secs_f64() * 1000.0,
                    samples[6].as_secs_f64() * 1000.0
                );
            }
        }
    }
}

#[test]
fn memory_endpoint_updates_match_sqlite_for_shared_list_occurrences() {
    let stores = [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ];
    let mut first = record("first", 1, Some(200));
    first.storage_key = "list:1:https://example.test/first".into();
    first.final_url = "https://example.test/shared".into();
    let mut later = record("shared", 2, Some(404));
    later.storage_key = "list:2:https://example.test/shared".into();
    for store in &stores {
        store.upsert(first.clone());
        store.upsert(later.clone());
        let edge = store.add_link_edge(test_edge(
            "https://example.test/shared",
            "https://example.test/shared",
            LinkType::Internal,
        ));
        assert_eq!(edge.source_status_code, Some(200));
        assert_eq!(edge.target_status_code, Some(200));
    }
    for status in [Some(503), None, Some(204)] {
        later.status_code = status;
        later.depth += 1;
        for store in &stores {
            store.upsert(later.clone());
            let edges = store.link_edges(LinkEdgeQuery::default()).edges;
            assert_eq!(edges[0].source_status_code, status);
            assert_eq!(edges[0].target_status_code, status);
            assert_eq!(edges[0].source_depth, later.depth);
            assert_eq!(edges[0].target_depth, Some(later.depth));
        }
        assert_eq!(
            stores[0].link_edges(LinkEdgeQuery::default()).edges,
            stores[1].link_edges(LinkEdgeQuery::default()).edges
        );
    }
    later.url = "https://example.test/moved".into();
    later.final_url = later.url.clone();
    later.status_code = Some(418);
    for store in &stores {
        store.upsert(later.clone());
        assert_eq!(
            store.link_edges(LinkEdgeQuery::default()).edges[0].target_status_code,
            Some(204),
            "moving the record does not reset old endpoint evidence"
        );
    }
}
