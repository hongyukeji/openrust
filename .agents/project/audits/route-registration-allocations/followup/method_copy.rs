use std::alloc::{GlobalAlloc,Layout,System};
use std::sync::atomic::{AtomicBool,AtomicU64,AtomicI64,Ordering::Relaxed};
static TRACK:AtomicBool=AtomicBool::new(false);
static ALLOCS:AtomicU64=AtomicU64::new(0);static REALLOCS:AtomicU64=AtomicU64::new(0);static FREES:AtomicU64=AtomicU64::new(0);
static BYTES:AtomicU64=AtomicU64::new(0);static LIVE:AtomicI64=AtomicI64::new(0);static PEAK:AtomicI64=AtomicI64::new(0);
struct Counter;
fn add(size:usize) {ALLOCS.fetch_add(1,Relaxed);BYTES.fetch_add(size as u64,Relaxed);let now=LIVE.fetch_add(size as i64,Relaxed)+size as i64;PEAK.fetch_max(now,Relaxed);}
unsafe impl GlobalAlloc for Counter {
 unsafe fn alloc(&self,l:Layout)->*mut u8 {let p=unsafe{System.alloc(l)};if TRACK.load(Relaxed)&&!p.is_null(){add(l.size())}p}
 unsafe fn alloc_zeroed(&self,l:Layout)->*mut u8 {let p=unsafe{System.alloc_zeroed(l)};if TRACK.load(Relaxed)&&!p.is_null(){add(l.size())}p}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout) {if TRACK.load(Relaxed){FREES.fetch_add(1,Relaxed);LIVE.fetch_sub(l.size() as i64,Relaxed);}unsafe{System.dealloc(p,l)}}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,size:usize)->*mut u8 {let q=unsafe{System.realloc(p,l,size)};if TRACK.load(Relaxed)&&!q.is_null(){REALLOCS.fetch_add(1,Relaxed);BYTES.fetch_add(size as u64,Relaxed);let now=LIVE.fetch_add(size as i64-l.size() as i64,Relaxed)+size as i64-l.size() as i64;PEAK.fetch_max(now,Relaxed);}q}
}
#[global_allocator]static GLOBAL:Counter=Counter;
fn main() { let input=[openrust::http::Method::GET,openrust::http::Method::HEAD]; TRACK.store(true,Relaxed); for _ in 0..128 {std::hint::black_box(std::hint::black_box(&input).to_vec());} TRACK.store(false,Relaxed);println!("alloc={} realloc={} bytes={} peak={} live={}",ALLOCS.load(Relaxed),REALLOCS.load(Relaxed),BYTES.load(Relaxed),PEAK.load(Relaxed),LIVE.load(Relaxed)); }
