use super::*;

fn reference(kind: PageReferenceKind, target: &str) -> PageReference {
    PageReference {
        id: u64::MAX,
        source_storage_key: "replaced by source argument".into(),
        source_url: "https://example.test/source".into(),
        target_url: format!("https://example.test/{target}"),
        kind,
        rel_nofollow: kind == PageReferenceKind::Hreflang,
    }
}

#[test]
fn reference_queries_preserve_list_occurrences_counts_filters_and_replacement_order() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let kinds = [
            PageReferenceKind::Canonical,
            PageReferenceKind::Hreflang,
            PageReferenceKind::Pagination,
            PageReferenceKind::Amp,
            PageReferenceKind::MetaRefresh,
            PageReferenceKind::Iframe,
        ];
        store.add_page_references(
            "list:1:source",
            kinds
                .iter()
                .enumerate()
                .map(|(index, kind)| reference(*kind, &index.to_string()))
                .collect(),
        );
        store.add_page_references(
            "list:2:source",
            vec![reference(PageReferenceKind::Canonical, "duplicate")],
        );
        let all = store.page_references(PageReferenceQuery::default());
        assert_eq!(all.total, 7);
        assert_eq!(
            all.references.iter().map(|row| row.id).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6, 7]
        );
        assert!(
            all.references[..6]
                .iter()
                .all(|row| row.source_storage_key == "list:1:source")
        );
        assert_eq!(all.references[6].source_storage_key, "list:2:source");
        assert!(all.references[1].rel_nofollow);
        for (index, kind) in kinds.into_iter().enumerate() {
            let result = store.page_references(PageReferenceQuery {
                source_storage_key: Some("list:1:source".into()),
                kind: Some(kind),
                ..PageReferenceQuery::default()
            });
            assert_eq!(result.total, 1);
            assert_eq!(result.references, all.references[index..=index]);
        }
        let page = store.page_references(PageReferenceQuery {
            kind: Some(PageReferenceKind::Canonical),
            offset: 1,
            limit: 1,
            ..PageReferenceQuery::default()
        });
        assert_eq!(page.total, 2);
        assert_eq!(page.references, all.references[6..]);
        for query in [
            PageReferenceQuery {
                limit: 0,
                ..PageReferenceQuery::default()
            },
            PageReferenceQuery {
                offset: usize::MAX,
                ..PageReferenceQuery::default()
            },
        ] {
            let result = store.page_references(query);
            assert_eq!(result.total, 7);
            assert!(result.references.is_empty());
        }
        assert_eq!(
            store
                .page_references(PageReferenceQuery {
                    limit: usize::MAX,
                    ..PageReferenceQuery::default()
                })
                .references,
            all.references
        );
        assert_eq!(
            store
                .page_references(PageReferenceQuery {
                    source_storage_key: Some("list:1:source ".into()),
                    ..PageReferenceQuery::default()
                })
                .total,
            0
        );

        store.add_page_references(
            "list:1:source",
            vec![reference(PageReferenceKind::Iframe, "replacement")],
        );
        let replaced = store.page_references(PageReferenceQuery::default());
        assert_eq!(replaced.total, 2);
        assert_eq!(replaced.references[0], all.references[6]);
        assert_eq!(replaced.references[1].id, 8);
        assert_eq!(
            replaced.references[1].target_url,
            "https://example.test/replacement"
        );
        store.add_page_references("list:1:source", Vec::new());
        assert_eq!(
            store
                .page_references(PageReferenceQuery::default())
                .references,
            all.references[6..]
        );
        assert!(store.records().is_empty());
        assert_eq!(store.link_edges(LinkEdgeQuery::default()).total, 0);
        store.clear();
        let cleared = store.page_references(PageReferenceQuery::default());
        assert_eq!(cleared.total, 0);
        assert!(cleared.references.is_empty());
    }
}

#[test]
fn reference_replacement_rolls_back_on_insert_failure() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .try_add_page_references(
            "source",
            vec![reference(PageReferenceKind::Canonical, "original")],
        )
        .unwrap();
    store
        .try_add_page_references("other", vec![reference(PageReferenceKind::Iframe, "other")])
        .unwrap();
    let before = store
        .try_page_references(PageReferenceQuery::default())
        .unwrap();
    store
        .connection()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_reference BEFORE INSERT ON page_references
         WHEN NEW.target_url = 'https://example.test/reject'
         BEGIN SELECT RAISE(ABORT, 'injected reference failure'); END;",
        )
        .unwrap();
    let error = store
        .try_add_page_references(
            "source",
            vec![
                reference(PageReferenceKind::Amp, "first"),
                reference(PageReferenceKind::Amp, "reject"),
            ],
        )
        .unwrap_err();
    assert!(error.to_string().contains("injected reference failure"));
    assert_eq!(
        store
            .try_page_references(PageReferenceQuery::default())
            .unwrap()
            .references,
        before.references
    );
}

#[test]
fn reference_query_decodes_only_its_page_without_hydrating_crawl_records() {
    let store = SqliteStore::in_memory().unwrap();
    let plan: String = store
        .connection()
        .unwrap()
        .query_row(
            "EXPLAIN QUERY PLAN SELECT * FROM page_references
             WHERE source_storage_key = ?1 AND kind = ?2 ORDER BY id LIMIT 100",
            params!["source", "canonical"],
            |row| row.get(3),
        )
        .unwrap();
    assert!(
        plan.contains("source_storage_key=? AND kind=?"),
        "Combined filters must seek within one source and kind: {plan}"
    );
    store
        .try_add_page_references(
            "source",
            vec![reference(PageReferenceKind::Canonical, "good")],
        )
        .unwrap();
    store
        .try_add_page_references("other", vec![reference(PageReferenceKind::Iframe, "bad")])
        .unwrap();
    store
        .try_upsert(CrawlRecord::pending(
            "https://example.test/unrelated".into(),
            0,
        ))
        .unwrap();
    store
        .connection()
        .unwrap()
        .execute_batch(
            "UPDATE page_references SET kind = 'invalid' WHERE source_storage_key = 'other';
         UPDATE crawl_records SET status_code = 'invalid';",
        )
        .unwrap();
    let page = store
        .try_page_references(PageReferenceQuery {
            limit: 1,
            ..PageReferenceQuery::default()
        })
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.references[0].target_url, "https://example.test/good");
    assert_eq!(
        store
            .try_page_references(PageReferenceQuery {
                source_storage_key: Some("source".into()),
                ..PageReferenceQuery::default()
            })
            .unwrap()
            .references,
        page.references
    );
    assert_eq!(
        store
            .try_page_references(PageReferenceQuery {
                limit: 0,
                ..PageReferenceQuery::default()
            })
            .unwrap()
            .total,
        2
    );
    assert!(
        store
            .try_page_references(PageReferenceQuery {
                offset: 1,
                limit: 1,
                ..PageReferenceQuery::default()
            })
            .is_err()
    );
}

#[test]
fn references_initialize_older_databases_and_survive_reopening() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-references-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = SqliteStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE page_references")
            .unwrap();
    }
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        store
            .try_page_references(PageReferenceQuery::default())
            .unwrap()
            .total,
        0
    );
    store
        .try_add_page_references(
            "list:1:source",
            vec![reference(PageReferenceKind::MetaRefresh, "next")],
        )
        .unwrap();
    let saved = store
        .try_page_references(PageReferenceQuery::default())
        .unwrap();
    drop(store);
    let reopened = SqliteStore::open(&path).unwrap();
    let query: PageReferenceQuery =
        serde_json::from_value(serde_json::json!({"kind": "metaRefresh"})).unwrap();
    assert_eq!(query.limit, 100);
    assert_eq!(
        reopened.try_page_references(query).unwrap().references,
        saved.references
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
