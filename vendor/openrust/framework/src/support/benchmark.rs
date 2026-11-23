use std::{future::Future, num::NonZeroU32, time::Instant};

/// Measures work with a monotonic clock and reports milliseconds.
pub struct Benchmark;

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Benchmark {
    pub fn value<T>(callback: impl FnOnce() -> T) -> (T, f64) {
        let start = Instant::now();
        let output = callback();
        (output, start.elapsed().as_secs_f64() * 1_000.0)
    }

    pub fn measure<T>(mut callback: impl FnMut() -> T, iterations: NonZeroU32) -> f64 {
        let total: f64 = (0..iterations.get())
            .map(|_| Self::value(|| std::hint::black_box(callback())).1)
            .sum();
        total / f64::from(iterations.get())
    }

    pub fn measureMany<'a>(
        callbacks: &mut [(&'a str, &mut dyn FnMut())],
        iterations: NonZeroU32,
    ) -> Vec<(&'a str, f64)> {
        callbacks
            .iter_mut()
            .map(|(name, callback)| (*name, Self::measure(callback, iterations)))
            .collect()
    }

    pub async fn valueAsync<F: Future>(future: F) -> (F::Output, f64) {
        let start = Instant::now();
        let output = future.await;
        (output, start.elapsed().as_secs_f64() * 1_000.0)
    }
}
