use super::*;

fn checkpoint(queued: usize, seen: usize) -> CrawlFrontierState {
    let urls = (0..seen)
        .map(|index| format!("https://example.test/page/{index:06}"))
        .collect::<Vec<_>>();
    CrawlFrontierState {
        queued: urls[seen - queued..]
            .iter()
            .enumerate()
            .map(|(position, url)| CrawlFrontierItem {
                url: url.clone(),
                depth: position % 8,
                from_sitemap: position % 17 == 0,
                storage_key: url.clone(),
                list_position: None,
                list_duplicate_index: 0,
            })
            .collect(),
        seen: urls,
        crawled: seen - queued,
    }
}

#[test]
fn sqlite_frontier_replacement_preserves_order_list_metadata_and_seen_deduplication() {
    let sqlite = SqliteStore::in_memory().unwrap();
    let active = ActiveStore::Sqlite(sqlite.clone());
    active.save_frontier_state(checkpoint(4, 7));

    let mut replacement = checkpoint(3, 3);
    let first = &mut replacement.queued[0];
    first.list_position = Some(u32::MAX);
    first.list_duplicate_index = u32::MAX;
    first.depth = 7;
    first.from_sitemap = true;
    first.storage_key = format!("list:{}:{}", u32::MAX, first.url);
    replacement.queued[1].url = replacement.queued[0].url.clone();
    replacement.queued[1].storage_key = format!("list:2:{}", replacement.queued[1].url);
    replacement.queued[1].list_position = Some(2);
    replacement.queued[1].list_duplicate_index = 1;
    replacement.queued[1].depth = 0;
    replacement.queued[1].from_sitemap = false;
    replacement.seen = replacement
        .queued
        .iter()
        .rev()
        .map(|item| item.storage_key.clone())
        .collect();
    replacement.seen.push(replacement.seen[0].clone());
    replacement.crawled = 42;
    active.save_frontier_state(replacement.clone());
    replacement.seen.sort();
    replacement.seen.dedup();
    assert_eq!(active.load_frontier_state(), Some(replacement.clone()));

    replacement.queued.drain(..2);
    replacement.seen = vec![replacement.queued[0].storage_key.clone()];
    replacement.crawled = 44;
    sqlite.try_save_frontier_state(replacement.clone()).unwrap();
    assert_eq!(
        sqlite.try_load_frontier_state().unwrap(),
        Some(replacement.clone())
    );

    replacement.queued.clear();
    sqlite.try_save_frontier_state(replacement.clone()).unwrap();
    assert_eq!(sqlite.try_load_frontier_state().unwrap(), Some(replacement));

    sqlite
        .try_save_frontier_state(CrawlFrontierState {
            crawled: 45,
            ..CrawlFrontierState::default()
        })
        .unwrap();
    assert_eq!(sqlite.try_load_frontier_state().unwrap(), None);
    assert_eq!(
        sqlite
            .connection()
            .unwrap()
            .query_row(
                "SELECT value FROM crawl_frontier_meta WHERE key = 'crawled'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "45"
    );
}

#[test]
fn sqlite_frontier_insert_failures_roll_back_the_complete_checkpoint() {
    for (table, condition) in [
        ("crawl_frontier_queue", "NEW.position = 1"),
        (
            "crawl_frontier_seen",
            "NEW.url = 'https://example.test/page/000004'",
        ),
        ("crawl_frontier_meta", "NEW.value = '8'"),
    ] {
        let sqlite = SqliteStore::in_memory().unwrap();
        let previous = checkpoint(2, 3);
        sqlite.try_save_frontier_state(previous.clone()).unwrap();
        sqlite
            .connection()
            .unwrap()
            .execute_batch(&format!(
                "CREATE TRIGGER reject_checkpoint BEFORE INSERT ON {table}
                 WHEN {condition}
                 BEGIN SELECT RAISE(ABORT, 'injected checkpoint failure'); END;"
            ))
            .unwrap();
        let mut replacement = checkpoint(3, 6);
        replacement.crawled = 8;
        let error = sqlite
            .try_save_frontier_state(replacement.clone())
            .unwrap_err();
        assert!(error.to_string().contains("injected checkpoint failure"));
        assert_eq!(
            sqlite.try_load_frontier_state().unwrap(),
            Some(previous),
            "{table}"
        );

        sqlite
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_checkpoint")
            .unwrap();
        sqlite.try_save_frontier_state(replacement.clone()).unwrap();
        assert_eq!(sqlite.try_load_frontier_state().unwrap(), Some(replacement));
    }
}

#[test]
#[ignore = "measures complete SQLite frontier checkpoint replacement"]
fn sqlite_frontier_checkpoint_workload() {
    for (queued, seen) in [(1_000, 2_000), (5_000, 10_000), (500, 10_500)] {
        let sqlite = SqliteStore::in_memory().unwrap();
        let mut state = checkpoint(queued, seen);
        sqlite.try_save_frontier_state(state.clone()).unwrap();
        let mut samples = Vec::new();
        for _ in 0..7 {
            state.queued.rotate_left(1);
            state.crawled += 1;
            let snapshot = state.clone();
            let previous_changes = sqlite.connection().unwrap().total_changes();
            let started = std::time::Instant::now();
            sqlite.try_save_frontier_state(snapshot).unwrap();
            samples.push(started.elapsed());
            assert_eq!(
                sqlite.connection().unwrap().total_changes() - previous_changes,
                (2 * (queued + seen + 1)) as u64
            );
        }
        assert_eq!(sqlite.try_load_frontier_state().unwrap(), Some(state));
        samples.sort();
        eprintln!(
            "frontier pending={queued} seen={seen} inserts_per_save={} samples={} min_ms={:.3} median_ms={:.3} max_ms={:.3}",
            queued + seen,
            samples.len(),
            samples[0].as_secs_f64() * 1_000.0,
            samples[samples.len() / 2].as_secs_f64() * 1_000.0,
            samples[samples.len() - 1].as_secs_f64() * 1_000.0,
        );
    }
}
