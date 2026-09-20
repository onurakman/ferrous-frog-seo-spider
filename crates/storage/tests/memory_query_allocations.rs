use ferrous_frog_storage::{
    CrawlRecord, CustomExtractionValue, GridQuery, LinkEdge, LinkEdgeQuery, LinkType, MemoryStore,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct CountAllocations;
static TRACKING: AtomicBool = AtomicBool::new(false);
static BYTES: AtomicUsize = AtomicUsize::new(0);

fn count(size: usize) {
    if TRACKING.load(Ordering::Relaxed) {
        BYTES.fetch_add(size, Ordering::Relaxed);
    }
}

// This integration-test executable contains one test; measurements exclude fixture setup.
unsafe impl GlobalAlloc for CountAllocations {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(size);
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountAllocations = CountAllocations;

fn allocated<T>(work: impl FnOnce() -> T) -> (T, usize) {
    BYTES.store(0, Ordering::Relaxed);
    TRACKING.store(true, Ordering::Relaxed);
    let result = work();
    TRACKING.store(false, Ordering::Relaxed);
    (result, BYTES.load(Ordering::Relaxed))
}

#[test]
fn memory_summary_and_query_windows_do_not_copy_unselected_large_payloads() {
    let store = MemoryStore::new();
    for index in 0..64 {
        let mut record = CrawlRecord::pending(format!("https://example.test/{index:02}"), 0);
        record.custom_extractions.push(CustomExtractionValue {
            name: "Large retained value".into(),
            values: vec!["x".repeat(128 * 1024)],
        });
        store.upsert(record);
        store.add_link_edge(LinkEdge {
            id: 0,
            source_url: "https://example.test/source".into(),
            target_url: format!("https://example.test/{index:02}"),
            anchor_text: "x".repeat(128 * 1024),
            rel: String::new(),
            rel_nofollow: false,
            link_type: LinkType::Internal,
            source_status_code: Some(200),
            target_status_code: None,
            source_depth: 0,
            target_depth: None,
            source_position: index,
            discovery_order: 0,
        });
    }
    let (summary, bytes) = allocated(|| store.summary());
    eprintln!("Memory summary allocated {bytes} bytes");
    assert_eq!(summary.total, 64);
    assert!(
        bytes < 1024 * 1024,
        "summary allocated {bytes} bytes for 8 MiB of unrelated payloads"
    );
    for limit in [0, 1] {
        let (page, bytes) = allocated(|| {
            store.query(GridQuery {
                global_search: Some("example.test".into()),
                sort_by: Some("firstInlinkSourceUrl".into()),
                offset: 63,
                limit,
                ..GridQuery::default()
            })
        });
        eprintln!("Memory {limit}-row query allocated {bytes} bytes");
        assert_eq!(page.total, 64);
        assert_eq!(page.rows.len(), limit);
        if let Some(row) = page.rows.first() {
            assert_eq!(row.url, "https://example.test/63");
            assert_eq!(row.custom_extractions[0].values[0].len(), 128 * 1024);
        }
        assert!(
            bytes < 1024 * 1024,
            "{limit}-row window allocated {bytes} bytes"
        );
    }
    let (page, bytes) = allocated(|| {
        store.link_edges(LinkEdgeQuery {
            global_search: Some("example.test".into()),
            sort_by: Some("targetUrl".into()),
            offset: 63,
            limit: 1,
            ..LinkEdgeQuery::default()
        })
    });
    eprintln!("Memory one-link query allocated {bytes} bytes");
    assert_eq!(page.total, 64);
    assert_eq!(page.edges.len(), 1);
    assert_eq!(page.edges[0].target_url, "https://example.test/63");
    assert_eq!(page.edges[0].anchor_text.len(), 128 * 1024);
    assert!(
        bytes < 1024 * 1024,
        "one-link window allocated {bytes} bytes"
    );
}
