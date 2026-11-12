use std::time::Duration;

use super::{JobMiddleware, JobNext};
use crate::queue::{JobContext, Payload, QueueFuture, QueueResult};

/// A condition evaluated when a job's middleware is constructed.
pub trait MiddlewareCondition {
    fn evaluate(self) -> bool;
}

impl MiddlewareCondition for bool {
    fn evaluate(self) -> bool {
        self
    }
}

impl<F: FnOnce() -> bool> MiddlewareCondition for F {
    fn evaluate(self) -> bool {
        self()
    }
}

/// Acknowledges a matching job without running its handler.
#[derive(Default)]
pub struct Skip {
    skip: bool,
}

impl Skip {
    pub fn new(skip: bool) -> Self {
        Self { skip }
    }

    pub fn when(condition: impl MiddlewareCondition) -> Self {
        Self::new(condition.evaluate())
    }

    pub fn unless(condition: impl MiddlewareCondition) -> Self {
        Self::new(!condition.evaluate())
    }
}

impl JobMiddleware for Skip {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        if self.skip {
            Box::pin(async { Ok(()) })
        } else {
            next.run(context, payload)
        }
    }
}

/// Releases a matching job to the queue without running its handler.
#[derive(Default)]
pub struct Release {
    release: bool,
    delay: Duration,
}

impl Release {
    pub fn new(release: bool, delay: Duration) -> Self {
        Self { release, delay }
    }

    pub fn when(condition: impl MiddlewareCondition, delay: Duration) -> Self {
        Self::new(condition.evaluate(), delay)
    }

    pub fn unless(condition: impl MiddlewareCondition, delay: Duration) -> Self {
        Self::new(!condition.evaluate(), delay)
    }
}

impl JobMiddleware for Release {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        if self.release {
            context.release(self.delay);
            Box::pin(async { Ok(()) })
        } else {
            next.run(context, payload)
        }
    }
}
