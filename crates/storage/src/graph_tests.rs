use super::*;

fn page(path: &str, position: Option<u32>, status: Option<u16>) -> CrawlRecord {
    let mut row = CrawlRecord::pending(format!("https://example.test/{path}"), 2);
    row.list_position = position;
    row.status_code = status;
    row
}

fn edge(source: &str, target: &str, link_type: LinkType, order: u64) -> LinkEdge {
    LinkEdge {
        id: 0,
        source_url: format!("https://example.test/{source}"),
        target_url: format!("https://example.test/{target}"),
        anchor_text: target.into(),
        rel: String::new(),
        rel_nofollow: false,
        link_type,
        source_status_code: None,
        target_status_code: None,
        source_depth: 2,
        target_depth: None,
        source_position: 1,
        discovery_order: order,
    }
}

fn populate(store: &ActiveStore) {
    let mut external = page("external", None, Some(200));
    external.classification = UrlClassification::External;
    store.upsert(external);
    let mut first = page("original", Some(40), Some(200));
    first.final_url = "https://example.test/shared".into();
    first.depth = 3;
    store.upsert(first);
    store.upsert(page("b", Some(2), Some(404)));
    store.upsert(page("pending", Some(3), None));
    let mut blocked = page("blocked", Some(4), None);
    blocked.status_text = "Blocked by robots.txt".into();
    blocked.error = Some("Blocked by robots.txt".into());
    store.upsert(blocked);
    let mut failed = page("failed", Some(5), None);
    failed.error = Some(String::new());
    store.upsert(failed);
    let mut last = page("duplicate", Some(1), Some(503));
    last.final_url = "https://example.test/shared".into();
    last.depth = 7;
    store.upsert(last);
    let mut excluded_duplicate = page("external-duplicate", Some(50), Some(302));
    excluded_duplicate.final_url = "https://example.test/shared".into();
    excluded_duplicate.classification = UrlClassification::External;
    store.upsert(excluded_duplicate);
    store.add_link_edge(edge("b", "original", LinkType::Internal, 30));
    store.add_link_edge(edge("shared", "edge-only", LinkType::Internal, 10));
    store.add_link_edge(edge(
        "edge-source",
        "external-target",
        LinkType::External,
        20,
    ));
    store.save_frontier_state(CrawlFrontierState {
        queued: vec![CrawlFrontierItem {
            url: "https://example.test/queue-only".into(),
            depth: 1,
            from_sitemap: true,
            storage_key: "https://example.test/queue-only".into(),
            list_position: None,
            list_duplicate_index: 0,
        }],
        ..CrawlFrontierState::default()
    });
}

#[test]
fn graph_caps_preserve_backend_order_last_occurrences_and_literal_aliases() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        populate(&store);
        for max_nodes in [0, 1, 2, 4, 20] {
            for max_edges in [0, 1, 2, 20] {
                for internal_only in [false, true] {
                    let query = CrawlGraphQuery {
                        max_nodes,
                        max_edges,
                        internal_only,
                    };
                    let edges = store.link_edges(LinkEdgeQuery {
                        limit: max_edges.max(1),
                        internal_only,
                        ..LinkEdgeQuery::default()
                    });
                    let expected =
                        build_crawl_graph(store.records(), edges.edges, edges.total, query.clone());
                    let actual = store.crawl_graph(query);
                    assert_eq!(
                        serde_json::to_value(actual).unwrap(),
                        serde_json::to_value(expected).unwrap()
                    );
                }
            }
        }
        let capped = store.crawl_graph(CrawlGraphQuery {
            max_nodes: 2,
            max_edges: 1,
            internal_only: true,
        });
        assert_eq!(capped.nodes.len(), 2);
        assert!(
            capped
                .nodes
                .iter()
                .all(|node| node.classification == Some(UrlClassification::Internal))
        );
        let shared = capped
            .nodes
            .iter()
            .find(|node| node.url.ends_with("/shared"))
            .unwrap();
        assert_eq!(
            shared.status_code,
            Some(if matches!(store, ActiveStore::Memory(_)) {
                503
            } else {
                200
            })
        );
        let whole = store.crawl_graph(CrawlGraphQuery::default());
        assert_eq!(
            whole
                .nodes
                .iter()
                .find(|node| node.url.ends_with("/shared"))
                .unwrap()
                .status_code,
            Some(302)
        );
        assert_eq!(
            whole.edges.iter().map(|edge| edge.id).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(whole.total_edges, 3);
        for (path, crawled) in [
            ("original", false),
            ("edge-only", false),
            ("edge-source", false),
            ("pending", false),
            ("blocked", false),
            ("failed", true),
        ] {
            assert_eq!(
                whole
                    .nodes
                    .iter()
                    .find(|node| node.url.ends_with(&format!("/{path}")))
                    .unwrap()
                    .crawled,
                crawled,
                "{path}"
            );
        }
        assert!(
            !whole
                .nodes
                .iter()
                .any(|node| node.url.ends_with("/queue-only"))
        );
        let zero = store.crawl_graph(CrawlGraphQuery {
            max_nodes: 0,
            max_edges: 0,
            internal_only: false,
        });
        assert_eq!(zero.nodes.len(), 1);
        assert_eq!(zero.total_edges, 3);
    }
}

#[test]
fn memory_graph_does_not_rebuild_unrequested_record_alias_annotations() {
    let store = ActiveStore::memory();
    populate(&store);
    URL_ALIAS_EXPANSIONS.with(|count| count.set(0));
    let graph = store.crawl_graph(CrawlGraphQuery {
        max_nodes: 2,
        max_edges: 1,
        internal_only: true,
    });
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(URL_ALIAS_EXPANSIONS.with(|count| count.get()), 0);
}

#[test]
fn sqlite_graph_decodes_only_selected_graph_columns_and_rows() {
    let sqlite = SqliteStore::in_memory().unwrap();
    for index in 0..10 {
        sqlite
            .try_upsert(page(&format!("page/{index}"), None, Some(200)))
            .unwrap();
    }
    sqlite.connection().unwrap().execute_batch(
        "UPDATE crawl_records SET response_time_ms = 'invalid unrelated integer', custom_extractions = '{malformed JSON';
         UPDATE crawl_records SET depth = 'invalid unselected depth' WHERE id = 10;",
    ).unwrap();
    assert!(sqlite.try_records().is_err());
    let query = CrawlGraphQuery {
        max_nodes: 2,
        max_edges: 1,
        internal_only: false,
    };
    let active = ActiveStore::Sqlite(sqlite.clone());
    for graph in [
        sqlite.try_crawl_graph(query.clone()).unwrap(),
        sqlite.crawl_graph(query.clone()),
        active.try_crawl_graph(query.clone()).unwrap(),
        active.crawl_graph(query),
    ] {
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.nodes[0].url, "https://example.test/page/0");
        assert_eq!(graph.nodes[1].url, "https://example.test/page/1");
    }
    assert!(sqlite.try_crawl_graph(CrawlGraphQuery::default()).is_err());
    assert!(active.try_crawl_graph(CrawlGraphQuery::default()).is_err());
}

#[test]
fn graph_queries_observe_record_url_edge_updates_and_clear() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let mut record = store.upsert(page("original", None, Some(200)));
        let query = CrawlGraphQuery::default();
        let first = store.try_crawl_graph(query.clone()).unwrap();
        assert_eq!(first.nodes[0].status_code, Some(200));
        assert!(first.edges.is_empty());
        record.final_url = "https://example.test/moved".into();
        record.status_code = Some(404);
        record.depth = 7;
        store.upsert(record);
        store.add_link_edge(edge("original", "pending-target", LinkType::Internal, 0));
        let updated = store.try_crawl_graph(query.clone()).unwrap();
        let moved = updated
            .nodes
            .iter()
            .find(|node| node.url.ends_with("/moved"))
            .unwrap();
        assert_eq!(moved.status_code, Some(404));
        assert_eq!(moved.depth, Some(7));
        assert!(
            !updated
                .nodes
                .iter()
                .find(|node| node.url.ends_with("/original"))
                .unwrap()
                .crawled
        );
        assert_eq!(updated.total_edges, 1);
        assert_eq!(updated.edges[0].source_status_code, Some(404));
        store.clear();
        let empty = store.try_crawl_graph(query).unwrap();
        assert!(empty.nodes.is_empty());
        assert!(empty.edges.is_empty());
        assert_eq!(empty.total_edges, 0);
    }
}

#[test]
fn sqlite_graph_errors_reach_direct_and_active_callers() {
    for table in ["crawl_records", "link_edges"] {
        let sqlite = SqliteStore::in_memory().unwrap();
        sqlite
            .connection()
            .unwrap()
            .execute_batch(&format!("DROP TABLE {table}"))
            .unwrap();
        assert!(sqlite.try_crawl_graph(CrawlGraphQuery::default()).is_err());
        assert!(
            ActiveStore::Sqlite(sqlite)
                .try_crawl_graph(CrawlGraphQuery::default())
                .is_err()
        );
    }
}

#[test]
fn sqlite_graph_nodes_and_edges_share_a_snapshot_during_external_writes() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-graph-snapshot-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sqlite = SqliteStore::open(&path).unwrap();
    sqlite.try_upsert(page("source", None, Some(200))).unwrap();
    sqlite.try_upsert(page("target", None, Some(200))).unwrap();
    sqlite
        .try_add_link_edge(edge("source", "target", LinkType::Internal, 0))
        .unwrap();
    sqlite
        .connection()
        .unwrap()
        .execute_batch(
            "ALTER TABLE link_edges RENAME TO graph_test_edges;
         CREATE VIEW link_edges AS SELECT * FROM graph_test_edges WHERE ff_graph_snapshot_probe();",
        )
        .unwrap();
    let writer = Connection::open(&path).unwrap();
    let wrote = std::sync::atomic::AtomicBool::new(false);
    sqlite
        .connection()
        .unwrap()
        .create_scalar_function(
            "ff_graph_snapshot_probe",
            0,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            move |_| {
                if !wrote.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    writer.execute_batch(
                        "UPDATE crawl_records SET status_code = 503;
                    UPDATE graph_test_edges SET target_status_code = 503;",
                    )?;
                }
                Ok(true)
            },
        )
        .unwrap();
    let before = sqlite.try_crawl_graph(CrawlGraphQuery::default()).unwrap();
    assert!(
        before
            .nodes
            .iter()
            .all(|node| node.status_code == Some(200))
    );
    assert_eq!(before.edges[0].target_status_code, Some(200));
    let after = sqlite.try_crawl_graph(CrawlGraphQuery::default()).unwrap();
    assert!(after.nodes.iter().all(|node| node.status_code == Some(503)));
    assert_eq!(after.edges[0].target_status_code, Some(503));
    drop(sqlite);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_graph_winner_selection_handles_shared_finals_and_list_ties() {
    let sqlite = SqliteStore::in_memory().unwrap();
    for index in 0..500 {
        let mut record = page(
            &format!("occurrence/{index}"),
            Some((index % 5) + 1),
            Some(200),
        );
        record.final_url = if index == 499 {
            "https://example.test/last"
        } else {
            "https://example.test/shared"
        }
        .into();
        record.depth = index as usize;
        sqlite.try_upsert(record).unwrap();
    }
    let one = sqlite
        .try_crawl_graph(CrawlGraphQuery {
            max_nodes: 1,
            max_edges: 0,
            internal_only: true,
        })
        .unwrap();
    assert_eq!(one.nodes.len(), 1);
    assert_eq!(one.nodes[0].url, "https://example.test/shared");
    assert_eq!(one.nodes[0].depth, Some(494));
    let all = sqlite
        .try_crawl_graph(CrawlGraphQuery {
            max_nodes: usize::MAX,
            max_edges: usize::MAX,
            internal_only: true,
        })
        .unwrap();
    assert_eq!(all.nodes.len(), 2);
}

#[test]
#[ignore = "measures capped graph snapshots over 50,000 records and 150,000 edges"]
fn graph_snapshot_storage_workload() {
    let count = 50_000;
    let url = |index: usize| {
        format!(
            "https://{}.example/pages/{index:06}",
            if index > 0 && index.is_multiple_of(37) {
                "external"
            } else {
                "crawl"
            }
        )
    };
    for (name, store) in [
        ("Memory", ActiveStore::memory()),
        (
            "SQLite",
            ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
        ),
    ] {
        for index in 0..count {
            let mut record = CrawlRecord::pending(url(index), index / 1_000);
            record.classification = if index > 0 && index.is_multiple_of(37) {
                UrlClassification::External
            } else {
                UrlClassification::Internal
            };
            record.status_code = Some(if index.is_multiple_of(97) { 404 } else { 200 });
            record.status_text = "OK".into();
            record.content_type = Some("text/html; charset=utf-8".into());
            record.title = Some(format!(
                "Synthetic crawl page {index}: {}",
                "title ".repeat(8)
            ));
            record.meta_description =
                Some("A generated description for the graph storage workload. ".repeat(3));
            record.h1 = Some(format!("Synthetic page heading {index}"));
            record.canonical = Some(record.final_url.clone());
            record.indexability = "Indexable".into();
            record.indexability_status = "Indexable".into();
            record.word_count = 600;
            record.inlink_count = 3;
            record.outlink_count = 3;
            record.custom_extractions = vec![CustomExtractionValue {
                name: "Synthetic category".into(),
                values: vec![
                    format!("Category {}", index % 100),
                    "Generated fixture content".repeat(3),
                ],
            }];
            store.upsert(record);
        }
        for index in 0..count {
            for (position, delta) in [1, 7, 101].into_iter().enumerate() {
                let target = (index + delta) % count;
                let mut link = edge("", "", LinkType::Internal, 0);
                link.source_url = url(index);
                link.target_url = if position == 2 && index.is_multiple_of(100) {
                    url(count + index)
                } else {
                    url(target)
                };
                link.anchor_text = format!("Page {target}");
                link.source_depth = index / 1_000;
                link.source_position = position as u32 + 1;
                link.link_type = if target > 0 && target.is_multiple_of(37) {
                    LinkType::External
                } else {
                    LinkType::Internal
                };
                store.add_link_edge(link);
            }
        }
        for (label, nodes, edges, internal_only, expected_edges) in [
            ("ui-1", 700, 1_500, false, 1_495),
            ("ui-2", 700, 1_500, false, 1_495),
            ("ui-3", 700, 1_500, false, 1_495),
            ("ui-internal", 700, 1_500, true, 1_455),
            ("export", 5_000, 10_000, false, 9_966),
        ] {
            let started = std::time::Instant::now();
            let graph = store
                .try_crawl_graph(CrawlGraphQuery {
                    max_nodes: nodes,
                    max_edges: edges,
                    internal_only,
                })
                .unwrap();
            println!(
                "{name} {label}: records={count} nodes={} edges={} total_edges={} elapsed={:.3}ms",
                graph.nodes.len(),
                graph.edges.len(),
                graph.total_edges,
                started.elapsed().as_secs_f64() * 1_000.
            );
            assert_eq!(graph.nodes.len(), nodes);
            assert_eq!(graph.edges.len(), expected_edges);
            assert_eq!(
                graph.total_edges,
                if internal_only { 145_947 } else { 150_000 }
            );
        }
    }
}
