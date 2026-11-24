use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed};
static TRACK: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static FREES: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicI64 = AtomicI64::new(0);
static PEAK: AtomicI64 = AtomicI64::new(0);
struct Counter;
fn add(size: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size as u64, Relaxed);
    let now = LIVE.fetch_add(size as i64, Relaxed) + size as i64;
    PEAK.fetch_max(now, Relaxed);
}
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if TRACK.load(Relaxed) && !p.is_null() {
            add(l.size())
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if TRACK.load(Relaxed) && !p.is_null() {
            add(l.size())
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if TRACK.load(Relaxed) {
            FREES.fetch_add(1, Relaxed);
            LIVE.fetch_sub(l.size() as i64, Relaxed);
        }
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, size) };
        if TRACK.load(Relaxed) && !q.is_null() {
            REALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(size as u64, Relaxed);
            let now = LIVE.fetch_add(size as i64 - l.size() as i64, Relaxed) + size as i64
                - l.size() as i64;
            PEAK.fetch_max(now, Relaxed);
        }
        q
    }
}
#[global_allocator]
static GLOBAL: Counter = Counter;

#[path = "support.rs"]
mod support;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args[1] == "self_test" {
        TRACK.store(true, Relaxed);
        unsafe {
            let layout = Layout::from_size_align(64, 8).unwrap();
            let p = std::alloc::alloc(layout);
            assert!(!p.is_null());
            let q = std::alloc::realloc(std::hint::black_box(p), layout, 128);
            assert!(!q.is_null());
            std::alloc::dealloc(
                std::hint::black_box(q),
                Layout::from_size_align(128, 8).unwrap(),
            );
        }
        TRACK.store(false, Relaxed);
        assert_eq!(ALLOCS.load(Relaxed), 1);
        assert_eq!(REALLOCS.load(Relaxed), 1);
        assert_eq!(FREES.load(Relaxed), 1);
        assert_eq!(BYTES.load(Relaxed), 192);
        assert_eq!(PEAK.load(Relaxed), 128);
        assert_eq!(LIVE.load(Relaxed), 0);
        println!("{{\"self_test\":true}}");
        return;
    }
    let work = support::Work::new(&args[1], args[2].parse().unwrap());
    let phase = &args[3];
    let n: usize = args[4].parse().unwrap();
    let mut router = if phase == "compile" {
        Some(work.register(false))
    } else if phase.starts_with("lookup") {
        Some(work.register(true))
    } else {
        None
    };
    if phase == "register" {
        work.registration(1);
    }
    if phase == "lookup_warm" {
        let router = router.as_ref().unwrap();
        work.verify(router);
        for query in 0..work.queries.len() {
            work.lookups(router, query, 10);
        }
    }
    if !["register", "register_cold", "lookup_warm"].contains(&phase.as_str()) {
        assert_eq!(n, 1);
    }
    TRACK.store(true, Relaxed);
    let held = match phase.as_str() {
        "register" | "register_cold" => {
            work.registration(n);
            None
        }
        "lookup_cold_first" | "lookup_cold_last" => {
            work.lookups(
                router.as_ref().unwrap(),
                if phase == "lookup_cold_first" { 0 } else { 2 },
                1,
            );
            None
        }
        "resident" => Some(work.register(true)),
        "resident_warm" => {
            let router = work.register(true);
            work.verify(&router);
            for query in 0..work.queries.len() {
                work.lookups(&router, query, 10);
            }
            Some(router)
        }
        "compile" => {
            router.as_mut().unwrap().__bench_reset();
            router.as_ref().unwrap().__bench_prepare();
            None
        }
        "lookup_warm" => {
            for query in 0..work.queries.len() {
                work.lookups(router.as_ref().unwrap(), query, n);
            }
            None
        }
        _ => panic!("未知测量阶段"),
    };
    TRACK.store(false, Relaxed);
    let stats = held.as_ref().or(router.as_ref()).map(|r| r.__bench_stats());
    println!(
        "{}",
        serde_json::json!({"phase":phase,"iterations":n,"query_count":if phase=="resident_warm" {work.queries.len()*11} else if phase.starts_with("lookup_cold") {1} else if phase=="lookup_warm" {n*work.queries.len()} else {0},"allocs":ALLOCS.load(Relaxed),"deallocs":FREES.load(Relaxed),"reallocs":REALLOCS.load(Relaxed),"requested_bytes":BYTES.load(Relaxed),"peak_live_bytes":PEAK.load(Relaxed),"retained_bytes":LIVE.load(Relaxed),"stats":stats,"router_size":std::mem::size_of::<openrust::routing::Router>()})
    );
}
