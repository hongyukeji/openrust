use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
    time::Duration,
};
use tokio::time::Instant;

/// 应用内任务计时器，读数的单位为毫秒。
#[derive(Default)]
pub struct Stopwatch {
    timers: Mutex<HashMap<String, Timer>>,
}

#[derive(Clone, Copy)]
struct Timer {
    started: Instant,
    attempt: Option<u32>,
}

impl Stopwatch {
    pub fn start(&self, key: &str) {
        self.timers().insert(
            key.into(),
            Timer {
                started: Instant::now(),
                attempt: None,
            },
        );
    }

    pub fn check(&self, key: &str) -> Option<f64> {
        let timer = self.timers().get(key).copied()?;
        Some(round_milliseconds(timer.started.elapsed()))
    }

    pub fn forget(&self, key: &str) {
        self.timers().remove(key);
    }

    pub(crate) fn start_attempt(&self, key: &str, attempt: u32) {
        let mut timers = self.timers();
        if timers
            .get(key)
            .and_then(|timer| timer.attempt)
            .is_none_or(|active| active <= attempt)
        {
            timers.insert(
                key.into(),
                Timer {
                    started: Instant::now(),
                    attempt: Some(attempt),
                },
            );
        }
    }

    pub(crate) fn check_attempt(&self, key: &str, attempt: u32) -> Option<f64> {
        let timer = self.timers().get(key).copied()?;
        timer
            .matches(attempt)
            .then(|| round_milliseconds(timer.started.elapsed()))
    }

    pub(crate) fn forget_attempt(&self, key: &str, attempt: u32) {
        let mut timers = self.timers();
        if timers.get(key).is_some_and(|timer| timer.matches(attempt)) {
            timers.remove(key);
        }
    }

    fn timers(&self) -> MutexGuard<'_, HashMap<String, Timer>> {
        self.timers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Timer {
    fn matches(self, attempt: u32) -> bool {
        self.attempt.is_none_or(|active| active == attempt)
    }
}

pub(crate) fn round_milliseconds(duration: Duration) -> f64 {
    ((duration.as_nanos() + 5_000) / 10_000) as f64 / 100.
}
