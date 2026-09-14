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
fn frontier_summary_preserves_recovery_counts_through_updates_and_clear() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        assert_eq!(store.try_frontier_summary().unwrap(), None);
        let mut state = checkpoint(4, 7);
        state.queued[1].url = state.queued[0].url.clone();
        state.queued[1].list_position = Some(7);
        state.queued[1].list_duplicate_index = 2;
        store.save_frontier_state(state.clone());
        assert_eq!(
            store.frontier_summary(),
            Some(CrawlFrontierSummary {
                queued: 4,
                seen: 7,
                crawled: 3
            })
        );
        let mut added = state.queued[0].clone();
        added.storage_key = "list:8:duplicate".into();
        added.list_position = Some(8);
        added.list_duplicate_index = 3;
        store.update_frontier_state(&state.queued[0].storage_key, vec![added], &[], 4);
        assert_eq!(
            store.try_frontier_summary().unwrap(),
            Some(CrawlFrontierSummary {
                queued: 4,
                seen: 8,
                crawled: 4
            })
        );
        for item in store.load_frontier_state().unwrap().queued {
            store.update_frontier_state(&item.storage_key, Vec::new(), &[], 8);
        }
        assert_eq!(
            store.frontier_summary(),
            Some(CrawlFrontierSummary {
                queued: 0,
                seen: 8,
                crawled: 8
            })
        );
        store.clear_frontier_state();
        assert_eq!(store.frontier_summary(), None);
        store.save_frontier_state(CrawlFrontierState {
            crawled: 45,
            ..Default::default()
        });
        // Preserve each existing backend's empty-checkpoint semantics.
        let expected = store.load_frontier_state().map(|_| CrawlFrontierSummary {
            queued: 0,
            seen: 0,
            crawled: 45,
        });
        assert_eq!(store.try_frontier_summary().unwrap(), expected);
    }
}

#[test]
fn sqlite_frontier_summary_does_not_decode_queue_or_seen_payloads() {
    let sqlite = SqliteStore::in_memory().unwrap();
    sqlite.try_save_frontier_state(checkpoint(2, 5)).unwrap();
    sqlite
        .connection()
        .unwrap()
        .execute_batch(
            "UPDATE crawl_frontier_queue SET depth='invalid unused depth';
         UPDATE crawl_frontier_seen SET url=x'FF' WHERE url LIKE '%000004';",
        )
        .unwrap();
    assert!(
        sqlite.try_load_frontier_state().is_err(),
        "The fixture must reject full hydration"
    );
    let expected = Some(CrawlFrontierSummary {
        queued: 2,
        seen: 5,
        crawled: 3,
    });
    assert_eq!(sqlite.try_frontier_summary().unwrap(), expected);
    assert_eq!(sqlite.frontier_summary(), expected);
    let active = ActiveStore::Sqlite(sqlite.clone());
    assert_eq!(active.frontier_summary(), expected);
    assert_eq!(active.try_frontier_summary().unwrap(), expected);
    sqlite
        .connection()
        .unwrap()
        .execute_batch("DROP TABLE crawl_frontier_seen")
        .unwrap();
    assert!(
        active.try_frontier_summary().is_err(),
        "Storage failures must not look like an empty frontier"
    );
}

#[test]
fn sqlite_frontier_summary_tracks_external_commits_rollback_and_reopen() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frontier-summary-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let sqlite = SqliteStore::open(&path).unwrap();
        sqlite.try_save_frontier_state(checkpoint(2, 5)).unwrap();
        let external = Connection::open(&path).unwrap();
        let previous = Some(CrawlFrontierSummary {
            queued: 2,
            seen: 5,
            crawled: 3,
        });
        for commit in [false, true] {
            external
                .execute_batch(
                    "BEGIN IMMEDIATE;
                DELETE FROM crawl_frontier_queue WHERE position=0;
                INSERT INTO crawl_frontier_seen(url) VALUES ('https://example.test/new');
                UPDATE crawl_frontier_meta SET value='4' WHERE key='crawled';",
                )
                .unwrap();
            assert_eq!(sqlite.try_frontier_summary().unwrap(), previous);
            external
                .execute_batch(if commit { "COMMIT" } else { "ROLLBACK" })
                .unwrap();
            assert_eq!(
                sqlite.try_frontier_summary().unwrap(),
                if commit {
                    Some(CrawlFrontierSummary {
                        queued: 1,
                        seen: 6,
                        crawled: 4,
                    })
                } else {
                    previous
                }
            );
        }
    }
    {
        let sqlite = SqliteStore::open(&path).unwrap();
        assert_eq!(
            sqlite.try_frontier_summary().unwrap(),
            Some(CrawlFrontierSummary {
                queued: 1,
                seen: 6,
                crawled: 4
            })
        );
        for value in ["invalid", "-1", "18446744073709551616"] {
            sqlite
                .connection()
                .unwrap()
                .execute("UPDATE crawl_frontier_meta SET value=?1", [value])
                .unwrap();
            assert_eq!(sqlite.try_frontier_summary().unwrap().unwrap().crawled, 0);
        }
        sqlite
            .connection()
            .unwrap()
            .execute("DELETE FROM crawl_frontier_meta", [])
            .unwrap();
        assert_eq!(sqlite.try_frontier_summary().unwrap().unwrap().crawled, 0);
    }
    std::fs::remove_file(path).unwrap();
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
fn frontier_updates_preserve_order_list_identity_sitemap_flags_and_seen_keys() {
    for store in [
        ActiveStore::memory(),
        ActiveStore::Sqlite(SqliteStore::in_memory().unwrap()),
    ] {
        let mut previous = checkpoint(4, 7);
        previous.queued[1].url = previous.queued[0].url.clone();
        let mut added = previous.queued[0].clone();
        added.storage_key = "list:8:duplicate".into();
        added.list_position = Some(8);
        added.list_duplicate_index = 3;
        added.depth = 42;
        added.from_sitemap = false;
        store.save_frontier_state(previous.clone());
        store.update_frontier_state(
            &previous.queued[0].storage_key,
            vec![added.clone()],
            &[added.url.clone()],
            4,
        );
        let mut retained = previous.queued[1].clone();
        retained.from_sitemap = true;
        added.from_sitemap = true;
        let mut seen = previous.seen;
        seen.push(added.storage_key.clone());
        seen.sort();
        let mut expected = CrawlFrontierState {
            queued: vec![
                retained,
                previous.queued[2].clone(),
                previous.queued[3].clone(),
                added,
            ],
            seen,
            crawled: 4,
        };
        assert_eq!(store.load_frontier_state(), Some(expected.clone()));
        for item in expected.queued.clone().into_iter().rev() {
            expected.crawled += 1;
            store.update_frontier_state(&item.storage_key, Vec::new(), &[], expected.crawled);
            expected.queued.pop();
            assert_eq!(store.load_frontier_state(), Some(expected.clone()));
        }
        store.clear_frontier_state();
        assert!(store.load_frontier_state().is_none());
    }
}

#[test]
fn sqlite_frontier_updates_write_only_changed_entries() {
    for size in [100, 10_000] {
        let sqlite = SqliteStore::in_memory().unwrap();
        let previous = checkpoint(size / 2, size);
        sqlite.try_save_frontier_state(previous.clone()).unwrap();
        let revision = crawl_audit_revision(&sqlite.connection().unwrap()).unwrap();
        let changes = sqlite.connection().unwrap().total_changes();
        let mut added = previous.queued[0].clone();
        added.url = "https://example.test/new".into();
        added.storage_key = added.url.clone();
        sqlite
            .try_update_frontier_state(
                &previous.queued[0].storage_key,
                vec![added.clone()],
                &[previous.queued[1].url.clone()],
                previous.crawled + 1,
            )
            .unwrap();
        let work = sqlite.connection().unwrap().total_changes() - changes;
        assert!(
            work <= 5,
            "Updating one completion/discovery wrote {work} rows with {size} seen URLs"
        );
        assert_eq!(
            crawl_audit_revision(&sqlite.connection().unwrap()).unwrap(),
            revision
        );
        let saved = sqlite.try_load_frontier_state().unwrap().unwrap();
        assert_eq!(saved.queued.len(), previous.queued.len());
        assert_eq!(saved.seen.len(), size + 1);
        assert_eq!(saved.queued.last(), Some(&added));
        assert!(saved.queued[0].from_sitemap);
    }
}

#[test]
fn sqlite_frontier_update_failures_roll_back_and_reopen() {
    let path = std::env::temp_dir().join(format!(
        "ferrous-frog-frontier-update-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let previous = checkpoint(4, 7);
    {
        let sqlite = SqliteStore::open(&path).unwrap();
        sqlite.try_save_frontier_state(previous.clone()).unwrap();
        let mut added = previous.queued[0].clone();
        added.url = "https://example.test/new".into();
        added.storage_key = added.url.clone();
        for (event, table, condition) in [
            (
                "INSERT",
                "crawl_frontier_queue",
                "NEW.storage_key = 'https://example.test/new'",
            ),
            (
                "INSERT",
                "crawl_frontier_seen",
                "NEW.url = 'https://example.test/new'",
            ),
            ("UPDATE", "crawl_frontier_queue", "NEW.from_sitemap = 1"),
            ("INSERT", "crawl_frontier_meta", "NEW.value = '42'"),
        ] {
            sqlite.connection().unwrap().execute_batch(&format!(
                "CREATE TRIGGER reject_update BEFORE {event} ON {table}
                 WHEN {condition} BEGIN SELECT RAISE(ABORT, 'injected frontier update failure'); END;"
            )).unwrap();
            let error = sqlite
                .try_update_frontier_state(
                    &previous.queued[0].storage_key,
                    vec![added.clone()],
                    &[previous.queued[1].url.clone()],
                    42,
                )
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("injected frontier update failure")
            );
            assert_eq!(
                sqlite.try_load_frontier_state().unwrap(),
                Some(previous.clone())
            );
            sqlite
                .connection()
                .unwrap()
                .execute_batch("DROP TRIGGER reject_update")
                .unwrap();
        }
        sqlite
            .try_update_frontier_state(&previous.queued[0].storage_key, vec![added.clone()], &[], 4)
            .unwrap();
    }
    {
        let sqlite = SqliteStore::open(&path).unwrap();
        let saved = sqlite.try_load_frontier_state().unwrap().unwrap();
        assert_eq!(saved.crawled, 4);
        assert_eq!(saved.queued.len(), 4);
        assert_eq!(saved.queued.last().unwrap().url, "https://example.test/new");
        assert_eq!(saved.seen.len(), 8);
        assert!(
            !saved
                .queued
                .iter()
                .any(|item| item.storage_key == previous.queued[0].storage_key)
        );
    }
    std::fs::remove_file(path).unwrap();
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

#[test]
#[ignore = "compares recovery counts with full frontier hydration at 100k and one million seen URLs"]
fn frontier_recovery_summary_workload() {
    for seen in [100_000, 1_000_000] {
        for sqlite in [false, true] {
            let store = if sqlite {
                ActiveStore::Sqlite(SqliteStore::in_memory().unwrap())
            } else {
                ActiveStore::memory()
            };
            store.save_frontier_state(checkpoint(500, seen));
            let expected = Some(CrawlFrontierSummary {
                queued: 500,
                seen,
                crawled: seen - 500,
            });
            // Warm both paths. Setup, verification and source insertion are not timed.
            assert_eq!(
                store.load_frontier_state().map(|state| state.summary()),
                expected
            );
            assert_eq!(store.try_frontier_summary().unwrap(), expected);
            let mut hydration = Vec::new();
            let mut counts = Vec::new();
            for pass in 0..7 {
                // Alternate pair order to avoid always giving one path the warmer cache.
                for summary_only in if pass % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let started = std::time::Instant::now();
                    let actual = std::hint::black_box(if summary_only {
                        store.try_frontier_summary().unwrap()
                    } else {
                        store.load_frontier_state().map(|state| state.summary())
                    });
                    let elapsed = started.elapsed();
                    assert_eq!(actual, expected);
                    if summary_only {
                        counts.push(elapsed);
                    } else {
                        hydration.push(elapsed);
                    }
                }
            }
            hydration.sort();
            counts.sort();
            eprintln!(
                "recovery backend={} pending=500 seen={seen} samples=7 hydration_median_ms={:.6} counts_median_ms={:.6} hydration_range_ms={:.6}..{:.6} counts_range_ms={:.6}..{:.6}",
                if sqlite { "sqlite" } else { "memory" },
                hydration[3].as_secs_f64() * 1_000.0,
                counts[3].as_secs_f64() * 1_000.0,
                hydration[0].as_secs_f64() * 1_000.0,
                hydration[6].as_secs_f64() * 1_000.0,
                counts[0].as_secs_f64() * 1_000.0,
                counts[6].as_secs_f64() * 1_000.0,
            );
        }
    }
}

#[test]
#[ignore = "compares complete and incremental SQLite frontier checkpoints"]
fn sqlite_incremental_frontier_workload() {
    for (pending, seen) in [(1_000, 2_000), (5_000, 10_000), (500, 100_000)] {
        for incremental in [false, true] {
            let sqlite = SqliteStore::in_memory().unwrap();
            let mut state = checkpoint(pending, seen);
            sqlite.try_save_frontier_state(state.clone()).unwrap();
            let mut samples = Vec::new();
            let mut writes = Vec::new();
            for pass in 0..7 {
                let completed = state.queued.remove(0);
                let mut added = completed.clone();
                added.url = format!("https://example.test/new/{pass}");
                added.storage_key = added.url.clone();
                state.queued.push(added.clone());
                state.seen.push(added.storage_key.clone());
                state.seen.sort();
                state.crawled += 1;
                let snapshot = state.clone();
                let changes = sqlite.connection().unwrap().total_changes();
                let started = std::time::Instant::now();
                if incremental {
                    sqlite
                        .try_update_frontier_state(
                            &completed.storage_key,
                            vec![added],
                            &[],
                            state.crawled,
                        )
                        .unwrap();
                } else {
                    sqlite.try_save_frontier_state(snapshot).unwrap();
                }
                samples.push(started.elapsed());
                writes.push(sqlite.connection().unwrap().total_changes() - changes);
            }
            assert_eq!(sqlite.try_load_frontier_state().unwrap(), Some(state));
            samples.sort();
            eprintln!(
                "frontier pending={pending} seen={seen} incremental={incremental} median_ms={:.3} writes_min={} writes_max={}",
                samples[3].as_secs_f64() * 1_000.0,
                writes.iter().min().unwrap(),
                writes.iter().max().unwrap(),
            );
        }
    }
}
