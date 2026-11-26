#[path = "../../../../../vendor/openrust-framework/tests/prototypes/workload.rs"]
mod workload;
use openrust::routing::Router;
use std::hint::black_box;

pub struct Work {
    paths: Vec<String>,
    pub queries: Vec<String>,
    expected: Vec<Option<usize>>,
}
impl Work {
    pub fn new(shape: &str, count: usize) -> Self {
        assert!([128, 1000, 10000].contains(&count));
        let paths = (0..count)
            .map(|index| workload::path(shape, index))
            .collect();
        let (queries, expected) = workload::queries(shape, count).into_iter().unzip();
        Self {
            paths,
            queries,
            expected,
        }
    }
    pub fn register(&self, prepare: bool) -> Router {
        let mut router = Router::new();
        for path in &self.paths {
            router.get(black_box(path), || "ok").unwrap();
        }
        if prepare {
            router.__bench_prepare();
        }
        router
    }
    pub fn verify(&self, router: &Router) {
        for (path, expected) in self.queries.iter().zip(&self.expected) {
            assert_eq!(router.__bench_match(path), *expected, "{path}");
        }
    }
    pub fn registration(&self, n: usize) {
        for _ in 0..n {
            black_box(self.register(true));
        }
    }
    pub fn lookups(&self, router: &Router, query: usize, n: usize) {
        for _ in 0..n {
            black_box(router.__bench_match(black_box(&self.queries[query])));
        }
    }
}
