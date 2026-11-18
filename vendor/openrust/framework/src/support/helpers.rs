use std::{future::Future, num::NonZeroUsize, time::Duration};

use serde_json::Value;

use super::{Sleep, SleepFake};

/// Resolves the current application's storage directory without creating it.
pub fn storage_path(
    path: &str,
) -> Result<std::path::PathBuf, crate::contracts::container::BindingResolutionError> {
    use crate::contracts::container::{BindingResolutionError, Container as _};
    super::facades::current_container()
        .ok_or(BindingResolutionError::FacadeNotSet)?
        .make::<crate::foundation::Paths>()
        .map(|paths| paths.storagePath(path))
}

/// Tests Laravel's blank-value rules for JSON values.
pub fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text
            .trim_matches([' ', '\t', '\n', '\r', '\x0b', '\0'])
            .is_empty(),
        Value::Array(values) => values.is_empty(),
        Value::Object(values) => values.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

pub fn filled(value: &Value) -> bool {
    !blank(value)
}

enum Schedule {
    Fixed {
        attempts: NonZeroUsize,
        delay: Duration,
    },
    Backoff(Vec<Duration>),
}

/// An explicit asynchronous retry policy. Attempt numbers start at one.
pub struct Retry {
    schedule: Schedule,
    fake: Option<SleepFake>,
}

#[allow(non_snake_case, reason = "Matches Laravel-style helper names")]
impl Retry {
    pub fn times(attempts: NonZeroUsize, delay: Duration) -> Self {
        Self {
            schedule: Schedule::Fixed { attempts, delay },
            fake: None,
        }
    }

    /// The delay list determines how many retries to attempt after the first call.
    pub fn backoff(delays: impl Into<Vec<Duration>>) -> Self {
        Self {
            schedule: Schedule::Backoff(delays.into()),
            fake: None,
        }
    }

    pub fn using(mut self, fake: &SleepFake) -> Self {
        self.fake = Some(fake.clone());
        self
    }

    pub async fn run<T, E, F: Future<Output = Result<T, E>>>(
        self,
        callback: impl FnMut(usize) -> F,
    ) -> Result<T, E> {
        self.runWhen(callback, |_| true).await
    }

    pub async fn runWhen<T, E, F: Future<Output = Result<T, E>>>(
        self,
        mut callback: impl FnMut(usize) -> F,
        mut predicate: impl FnMut(&E) -> bool,
    ) -> Result<T, E> {
        let mut attempt = 1;
        loop {
            let error = match callback(attempt).await {
                Ok(value) => return Ok(value),
                Err(error) => error,
            };
            let delay = match &self.schedule {
                Schedule::Fixed { attempts, delay } => (attempt < attempts.get()).then_some(*delay),
                Schedule::Backoff(delays) => delays.get(attempt - 1).copied(),
            };
            let Some(delay) = delay else {
                return Err(error);
            };
            if !predicate(&error) {
                return Err(error);
            }
            if !delay.is_zero() {
                let mut wait = Sleep::r#for(delay);
                if let Some(fake) = &self.fake {
                    wait = wait.using(fake);
                }
                wait.wait().await;
            }
            attempt += 1;
        }
    }
}
