use std::{future::Future, num::NonZeroUsize, time::Duration};

use serde_json::Value;

use super::{Sleep, SleepFake};

/// Defers an owned future until the current HTTP, Artisan or queue-attempt boundary.
///
/// A name replaces a previously registered callback of the same name in this
/// lifecycle. Set `always` to include unsuccessful responses, commands and jobs.
/// Capture scoped services when registering: their original scope may have
/// closed by the time this future runs. Cancellation before the boundary or a
/// killed process does not guarantee execution. Outside a lifecycle, this
/// returns `concurrency::Error::OutsideRequest`.
pub fn defer<F, E>(
    callback: F,
    name: Option<&str>,
    always: bool,
) -> crate::concurrency::Result<crate::concurrency::DeferredCallback>
where
    F: Future<Output = Result<(), E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    let callback = match name {
        Some(name) => crate::concurrency::enqueue_named(name.to_owned(), Box::pin(callback)),
        None => crate::concurrency::enqueue_deferred(Box::pin(callback)),
    }?;
    Ok(if always { callback.always() } else { callback })
}

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

/// 读取当前应用的语言目录，不创建目录。
pub fn lang_path(
    path: &str,
) -> Result<std::path::PathBuf, crate::contracts::container::BindingResolutionError> {
    use crate::contracts::container::{BindingResolutionError, Container as _};
    super::facades::current_container()
        .ok_or(BindingResolutionError::FacadeNotSet)?
        .make::<crate::foundation::Paths>()
        .map(|paths| paths.langPath(path))
}

/// Tests blank-value rules for JSON values.
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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
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

#[cfg(test)]
#[path = "../../tests/unit/deferred_lifecycle.rs"]
mod deferred_lifecycle;
