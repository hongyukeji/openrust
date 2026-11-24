mod probe_support;
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
fn main() {
 let mut args=std::env::args().skip(1);let case=args.next().unwrap();if case=="verify"{probe_support::verify();return}
 let n=args.next().unwrap().parse::<u64>().unwrap();
 if case=="allocator_self_test" {
  TRACK.store(true,Relaxed);
  unsafe {
   let layout=Layout::from_size_align(64,8).unwrap();
   let p=std::alloc::alloc(layout);assert!(!p.is_null());
   let q=std::alloc::realloc(std::hint::black_box(p),layout,128);assert!(!q.is_null());
   std::alloc::dealloc(std::hint::black_box(q),Layout::from_size_align(128,8).unwrap());
  }
  TRACK.store(false,Relaxed);
  assert_eq!(ALLOCS.load(Relaxed),1);assert_eq!(REALLOCS.load(Relaxed),1);assert_eq!(FREES.load(Relaxed),1);
  assert_eq!(BYTES.load(Relaxed),192);assert_eq!(PEAK.load(Relaxed),128);assert_eq!(LIVE.load(Relaxed),0);
  println!("{{\"self_test\":true}}");return;
 }
 let work=probe_support::Work::new(case.clone());
 TRACK.store(true,Relaxed);work.run(n);TRACK.store(false,Relaxed);
 println!("{{\"case\":\"{case}\",\"iterations\":{n},\"allocs\":{},\"reallocs\":{},\"deallocs\":{},\"requested_bytes\":{},\"peak_live_bytes\":{},\"retained_bytes\":{}}}",ALLOCS.load(Relaxed),REALLOCS.load(Relaxed),FREES.load(Relaxed),BYTES.load(Relaxed),PEAK.load(Relaxed),LIVE.load(Relaxed));
}
