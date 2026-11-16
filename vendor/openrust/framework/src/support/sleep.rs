use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

/// Builds a delay for the current async task.
#[must_use = "call wait, then, or while to run the delay"]
pub struct Sleep {
    duration: Duration,
    enabled: bool,
    fake: Option<SleepFake>,
}

impl Sleep {
    pub fn r#for(duration: Duration) -> Self {
        Self {
            duration,
            enabled: true,
            fake: None,
        }
    }

    pub fn until(timestamp: SystemTime) -> Self {
        Self::r#for(
            timestamp
                .duration_since(SystemTime::now())
                .unwrap_or_default(),
        )
    }

    #[allow(
        clippy::self_named_constructors,
        reason = "Matches Laravel Sleep::sleep"
    )]
    pub fn sleep(seconds: u64) -> Self {
        Self::r#for(Duration::from_secs(seconds))
    }

    pub fn usleep(microseconds: u64) -> Self {
        Self::r#for(Duration::from_micros(microseconds))
    }

    pub fn and(mut self, duration: Duration) -> Self {
        self.duration = self.duration.saturating_add(duration);
        self
    }

    pub fn when(mut self, condition: bool) -> Self {
        self.enabled = condition;
        self
    }

    pub fn unless(self, condition: bool) -> Self {
        self.when(!condition)
    }

    pub fn fake() -> SleepFake {
        SleepFake::default()
    }

    pub fn using(mut self, fake: &SleepFake) -> Self {
        self.fake = Some(fake.clone());
        self
    }

    pub async fn wait(self) {
        if self.enabled {
            match self.fake {
                Some(fake) => fake.record(self.duration),
                None if !self.duration.is_zero() => tokio::time::sleep(self.duration).await,
                None => {}
            }
        }
    }

    /// Repeats the delay while the predicate remains true.
    pub async fn r#while(self, mut predicate: impl FnMut() -> bool) {
        if self.fake.is_some() {
            self.wait().await;
        } else {
            while self.enabled && predicate() {
                if self.duration.is_zero() {
                    tokio::task::yield_now().await;
                } else {
                    tokio::time::sleep(self.duration).await;
                }
            }
        }
    }

    pub async fn then<T>(self, callback: impl FnOnce() -> T) -> T {
        self.wait().await;
        callback()
    }
}

type Observer = Arc<dyn Fn(Duration) + Send + Sync>;

#[derive(Default)]
struct SleepLog {
    durations: Vec<Duration>,
    observers: Vec<Observer>,
}

/// Records waits explicitly assigned to this fake. Clones share its log.
#[derive(Clone, Default)]
pub struct SleepFake(Arc<Mutex<SleepLog>>);

#[allow(non_snake_case, reason = "Matches Laravel's Sleep assertions")]
impl SleepFake {
    pub fn sequence(&self) -> Vec<Duration> {
        self.0.lock().expect("sleep log mutex").durations.clone()
    }

    pub fn whenFakingSleep(&self, callback: impl Fn(Duration) + Send + Sync + 'static) {
        self.0
            .lock()
            .expect("sleep log mutex")
            .observers
            .push(Arc::new(callback));
    }

    pub fn assertSlept(&self, predicate: impl FnMut(&Duration) -> bool, times: usize) {
        assert_eq!(self.sequence().into_iter().filter(predicate).count(), times);
    }

    pub fn assertSleptTimes(&self, expected: usize) {
        assert_eq!(self.sequence().len(), expected);
    }

    pub fn assertSequence(&self, expected: &[Option<Duration>]) {
        let actual = self.sequence();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.into_iter().zip(expected) {
            if let Some(expected) = expected {
                assert_eq!(actual, *expected);
            }
        }
    }

    pub fn assertNeverSlept(&self) {
        self.assertSleptTimes(0);
    }

    pub fn assertInsomniac(&self) {
        assert!(self.sequence().iter().all(Duration::is_zero));
    }

    fn record(&self, duration: Duration) {
        let observers = {
            let mut log = self.0.lock().expect("sleep log mutex");
            log.durations.push(duration);
            log.observers.clone()
        };
        for observer in observers {
            observer(duration);
        }
    }
}
