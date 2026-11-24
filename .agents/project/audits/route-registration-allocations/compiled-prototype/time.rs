#[path = "support.rs"]
mod support;
use std::{hint::black_box, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args[1] == "cache" {
        let work = support::Work::new(&args[2], args[3].parse().unwrap());
        println!("{}", work.register(true).__bench_cache_dump());
        return;
    }
    let count: usize = args[2].parse().unwrap();
    let work = support::Work::new(&args[1], count);
    let n: usize = args[3].parse().unwrap();
    work.registration(1);
    let start = Instant::now();
    work.registration(n);
    let registration = start.elapsed().as_nanos() as f64 / n as f64;
    let mut router = work.register(false);
    let start = Instant::now();
    router.__bench_prepare();
    let compile_cold = start.elapsed().as_nanos();
    router.__bench_reset();
    let start = Instant::now();
    router.__bench_prepare();
    let compile = start.elapsed().as_nanos();
    let start = Instant::now();
    work.lookups(&router, 0, 1);
    let cold_first = start.elapsed().as_nanos();
    drop(router);
    let router = work.register(true);
    let start = Instant::now();
    work.lookups(&router, 2, 1);
    let cold_last = start.elapsed().as_nanos();
    work.verify(&router);
    let lookup_n = if args[1] == "static" { 1000 } else { 100 };
    let mut timings = Vec::new();
    for query in 0..work.queries.len() {
        work.lookups(&router, query, 10);
        let start = Instant::now();
        work.lookups(&router, query, lookup_n);
        timings.push(start.elapsed().as_nanos() as f64 / lookup_n as f64);
    }
    let stats = router.__bench_stats();
    black_box(router);
    println!(
        "{}",
        serde_json::json!({"registration_ns":registration,"compile_ns":compile,"compile_cold_ns":compile_cold,"lookup_cold_first_ns":cold_first,"lookup_cold_last_ns":cold_last,"lookup_ns":timings,"stats":stats,"router_size":std::mem::size_of::<openrust::routing::Router>()})
    );
}
