use crate::*;

fn image(page_url: &str, image_url: &str, position: u32, alt: Option<&str>) -> ImageAsset {
    ImageAsset {
        id: 0,
        page_url: page_url.into(),
        image_url: image_url.into(),
        alt_text: alt.map(str::to_string),
        alt_len: alt.map_or(0, |text| text.chars().count() as u32),
        missing_alt: alt.is_none(),
        alt_too_long: false,
        width: Some(640),
        height: Some(480),
        source_position: position,
        size_bytes: None,
        oversized: false,
    }
}

fn populate(store: &impl CrawlStore) {
    for (position, size) in [(1, 10), (2, 300_000)] {
        let mut record = CrawlRecord::pending("https://example.test/old.png#download".into(), 0);
        record.final_url = "https://example.test/hero.png".into();
        record.storage_key = format!("list:{position}:{}", record.url);
        record.list_position = Some(position);
        record.content_type = Some("image/png".into());
        record.status_code = Some(200);
        record.size_bytes = size;
        store.upsert(record);
    }
    let mut html = CrawlRecord::pending("https://example.test/not-image".into(), 0);
    html.content_type = Some("text/html".into());
    html.size_bytes = 500_000;
    store.upsert(html);
    store.add_image_assets(
        "https://example.test/z",
        vec![
            image(
                "https://example.test/z",
                "https://example.test/old.png#view",
                8,
                None,
            ),
            image(
                "https://example.test/z",
                "https://example.test/hero.png",
                3,
                Some("Résumé 🐸"),
            ),
            image(
                "https://example.test/z",
                "https://example.test/hero.png",
                9,
                Some(""),
            ),
        ],
    );
    let mut non_image = image(
        "https://example.test/",
        "https://example.test/not-image",
        4,
        Some("Logo"),
    );
    non_image.width = None;
    non_image.height = None;
    store.add_image_assets("https://example.test/", vec![non_image]);
}

#[test]
fn image_queries_page_filter_and_sort_occurrences_with_list_redirect_size_aliases() {
    let memory = MemoryStore::new();
    let sqlite = SqliteStore::in_memory().unwrap();
    populate(&memory);
    populate(&sqlite);
    let oversized = sqlite
        .try_image_assets(ImageAssetQuery {
            oversized_only: true,
            sort_by: Some("sourcePosition".into()),
            ..ImageAssetQuery::default()
        })
        .unwrap();
    assert_eq!(oversized.total, 3);
    assert_eq!(
        oversized
            .images
            .iter()
            .map(|row| row.source_position)
            .collect::<Vec<_>>(),
        [3, 8, 9]
    );
    assert!(
        oversized
            .images
            .iter()
            .all(|row| row.size_bytes == Some(300_000) && row.oversized)
    );

    for sort_by in [
        None,
        Some("id"),
        Some("pageUrl"),
        Some("imageUrl"),
        Some("altText"),
        Some("altLen"),
        Some("missingAlt"),
        Some("altTooLong"),
        Some("width"),
        Some("height"),
        Some("sourcePosition"),
        Some("sizeBytes"),
        Some("oversized"),
        Some("unknown"),
    ] {
        for sort_dir in [SortDirection::Asc, SortDirection::Desc] {
            for query in [
                ImageAssetQuery::default(),
                ImageAssetQuery {
                    offset: 1,
                    limit: 2,
                    ..ImageAssetQuery::default()
                },
                ImageAssetQuery {
                    page_url: Some(" https://example.test ".into()),
                    ..ImageAssetQuery::default()
                },
                ImageAssetQuery {
                    global_search: Some(" RÉSUMÉ ".into()),
                    ..ImageAssetQuery::default()
                },
                ImageAssetQuery {
                    oversized_only: true,
                    missing_alt_only: true,
                    ..ImageAssetQuery::default()
                },
                ImageAssetQuery {
                    offset: 4,
                    ..ImageAssetQuery::default()
                },
                ImageAssetQuery {
                    limit: 0,
                    ..ImageAssetQuery::default()
                },
            ] {
                let query = ImageAssetQuery {
                    sort_by: sort_by.map(str::to_string),
                    sort_dir: sort_dir.clone(),
                    ..query
                };
                let expected = memory.image_assets(query.clone());
                let actual = sqlite.try_image_assets(query.clone()).unwrap();
                assert_eq!(actual.total, expected.total, "{query:?}");
                assert_eq!(actual.images, expected.images, "{query:?}");
            }
        }
    }
}

#[test]
fn image_query_decodes_only_the_requested_sqlite_page() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .try_add_image_assets(
            "https://example.test/",
            vec![
                image(
                    "https://example.test/",
                    "https://example.test/a.png",
                    1,
                    None,
                ),
                image(
                    "https://example.test/",
                    "https://example.test/b.png",
                    2,
                    None,
                ),
            ],
        )
        .unwrap();
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE image_assets SET alt_len = 'invalid' WHERE source_position = 2",
            [],
        )
        .unwrap();
    let first = store
        .try_image_assets(ImageAssetQuery {
            limit: 1,
            ..ImageAssetQuery::default()
        })
        .unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.images[0].source_position, 1);
    assert!(
        store
            .try_image_assets(ImageAssetQuery {
                offset: 1,
                limit: 1,
                ..ImageAssetQuery::default()
            })
            .is_err()
    );
    assert_eq!(
        store
            .try_image_assets(ImageAssetQuery {
                limit: 0,
                ..ImageAssetQuery::default()
            })
            .unwrap()
            .total,
        2
    );
}

fn count_record_alias_work(store: &SqliteStore) -> Arc<std::sync::atomic::AtomicUsize> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let record_calls = calls.clone();
    store
        .connection()
        .unwrap()
        .create_scalar_function(
            "ff_url_aliases",
            3,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            move |context| {
                if !context.get_raw(1).as_str()?.is_empty() {
                    record_calls.fetch_add(1, Ordering::Relaxed);
                }
                let aliases = sorted_aliases(url_aliases_many([
                    context.get_raw(0).as_str()?,
                    context.get_raw(1).as_str()?,
                    context.get_raw(2).as_str()?,
                ]));
                serde_json::to_string(&aliases)
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
            },
        )
        .unwrap();
    calls
}

#[test]
fn image_alias_cache_tracks_record_evidence_and_reads_image_references_live() {
    use std::sync::atomic::Ordering;
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-image-aliases-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sqlite = SqliteStore::open(&path).unwrap();
    let memory = MemoryStore::new();
    populate(&sqlite);
    populate(&memory);
    let calls = count_record_alias_work(&sqlite);
    let query = ImageAssetQuery {
        limit: 1,
        oversized_only: true,
        sort_by: Some("sourcePosition".into()),
        ..ImageAssetQuery::default()
    };
    let first = sqlite.try_image_assets(query.clone()).unwrap();
    assert_eq!(first.total, 3);
    assert_eq!(first.images[0].size_bytes, Some(300_000));
    let initial_work = calls.load(Ordering::Relaxed);
    assert!(initial_work > 0);
    for offset in [1, 2] {
        let page_query = ImageAssetQuery {
            offset,
            ..query.clone()
        };
        let actual = sqlite.try_image_assets(page_query.clone()).unwrap();
        assert_eq!(actual.images, memory.image_assets(page_query).images);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            initial_work,
            "Paging must reuse record aliases"
        );
    }

    sqlite
        .try_save_frontier_state(CrawlFrontierState::default())
        .unwrap();
    let new_images = vec![image(
        "https://example.test/z",
        "https://example.test/old.png#new",
        7,
        None,
    )];
    sqlite
        .try_add_image_assets("https://example.test/z", new_images.clone())
        .unwrap();
    memory.add_image_assets("https://example.test/z", new_images);
    let updated = sqlite.try_image_assets(query.clone()).unwrap();
    assert_eq!(updated.total, 1);
    assert_eq!(updated.images[0].source_position, 7);
    assert_eq!(updated.images, memory.image_assets(query.clone()).images);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        initial_work,
        "Image-reference and frontier writes must not rebuild record aliases"
    );

    {
        let mut conn = sqlite.connection().unwrap();
        let transaction = conn.transaction().unwrap();
        transaction
            .execute("UPDATE crawl_records SET size_bytes = 1", [])
            .unwrap();
        transaction.rollback().unwrap();
    }
    assert_eq!(
        sqlite.try_image_assets(query.clone()).unwrap().images[0].size_bytes,
        Some(300_000)
    );
    assert_eq!(
        calls.load(Ordering::Relaxed),
        initial_work,
        "Rollback must preserve cached evidence"
    );

    // Updating an earlier List occurrence must not override the highest record ID.
    let mut earlier = memory
        .records()
        .into_iter()
        .find(|row| row.list_position == Some(1))
        .unwrap();
    earlier.size_bytes = 500_000;
    memory.upsert(earlier.clone());
    sqlite.try_upsert(earlier).unwrap();
    let updated = sqlite.try_image_assets(query.clone()).unwrap();
    assert_eq!(updated.images[0].size_bytes, Some(300_000));
    assert_eq!(updated.images, memory.image_assets(query.clone()).images);
    assert!(calls.load(Ordering::Relaxed) > initial_work);

    let writer = Connection::open(&path).unwrap();
    writer
        .execute(
            "UPDATE crawl_records SET size_bytes = 100 WHERE list_position = 2",
            [],
        )
        .unwrap();
    assert_eq!(sqlite.try_image_assets(query.clone()).unwrap().total, 0);
    writer
        .execute(
            "UPDATE crawl_records SET content_type = 'text/html' WHERE list_position = 2",
            [],
        )
        .unwrap();
    assert_eq!(
        sqlite.try_image_assets(query.clone()).unwrap().images[0].size_bytes,
        Some(500_000)
    );
    let before_image_write = calls.load(Ordering::Relaxed);
    writer.execute("UPDATE image_assets SET image_url = 'https://example.test/unknown' WHERE source_position = 7", []).unwrap();
    assert_eq!(sqlite.try_image_assets(query.clone()).unwrap().total, 0);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        before_image_write,
        "External image-reference changes must remain live without rebuilding aliases"
    );
    writer.execute("UPDATE image_assets SET image_url = 'https://example.test/hero.png' WHERE source_position = 7", []).unwrap();
    assert_eq!(sqlite.try_image_assets(query.clone()).unwrap().total, 1);
    writer
        .execute("DELETE FROM crawl_records WHERE list_position = 1", [])
        .unwrap();
    assert_eq!(sqlite.try_image_assets(query.clone()).unwrap().total, 0);
    drop(writer);
    drop(sqlite);
    let reopened = SqliteStore::open(&path).unwrap();
    assert_eq!(reopened.try_image_assets(query).unwrap().total, 0);
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "measures repeated image pages over 50,000 records and 50,000 image occurrences"]
fn image_alias_repeated_page_workload() {
    use std::sync::atomic::Ordering;
    let store = SqliteStore::in_memory().unwrap();
    for index in 0..50_000 {
        let is_image = index < 10_000;
        let mut record = CrawlRecord::pending(format!("https://example.test/source/{index}"), 0);
        record.final_url = format!("https://example.test/final/{index}");
        record.content_type = Some(if is_image { "image/png" } else { "text/html" }.into());
        record.status_code = Some(200);
        record.size_bytes = 300_000;
        store.try_upsert(record).unwrap();
    }
    for page in 0..5_000 {
        let page_url = format!("https://example.test/page/{page:05}");
        store
            .try_add_image_assets(
                &page_url,
                (0..10)
                    .map(|position| {
                        image(
                            &page_url,
                            &format!(
                                "https://example.test/source/{}#image",
                                (page * 10 + position) % 10_000
                            ),
                            position,
                            Some("Image"),
                        )
                    })
                    .collect(),
            )
            .unwrap();
    }
    let calls = count_record_alias_work(&store);
    for (label, page_size, page_count, oversized_only) in [
        ("first page", 100, 1, false),
        ("export", 10_000, 5, false),
        ("oversized UI", 100, 10, true),
    ] {
        calls.store(0, Ordering::Relaxed);
        let started = std::time::Instant::now();
        for page in 0..page_count {
            let response = store
                .try_image_assets(ImageAssetQuery {
                    limit: page_size,
                    offset: page * page_size,
                    oversized_only,
                    ..ImageAssetQuery::default()
                })
                .unwrap();
            assert_eq!(response.total, 50_000);
            assert_eq!(response.images.len(), page_size);
            assert!(
                response
                    .images
                    .iter()
                    .all(|image| image.size_bytes == Some(300_000))
            );
        }
        eprintln!(
            "Image alias {label}: {page_count} x {page_size} rows, {:.3}s, {} record alias normalizations",
            started.elapsed().as_secs_f64(),
            calls.load(Ordering::Relaxed)
        );
    }
}
