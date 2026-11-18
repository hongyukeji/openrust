use std::{
    cell::RefCell,
    future::Future,
    sync::{Arc, Weak},
};

use crate::{
    container::Container,
    contracts::auth::Authenticatable,
    http::{Request, RequestAttributes},
    support::facades::current_container,
};

/// Read-only HTTP metadata associated with the current log execution scope.
#[derive(Clone)]
pub struct Http {
    method: String,
    path: String,
    attributes: RequestAttributes,
    default_guard: String,
}

impl Http {
    pub fn method(&self) -> &str {
        &self.method
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn user(&self) -> Option<Arc<dyn Authenticatable>> {
        crate::auth::logged_user(&self.attributes, &self.default_guard)
    }
}

/// Identifies the execution that produced a log message.
#[derive(Clone)]
pub enum Origin {
    Http(Http),
    Console {
        command: Option<String>,
    },
    Queue {
        command: Option<String>,
        queue: String,
        job: String,
    },
}

struct Frame {
    owner: Weak<Container>,
    origin: RefCell<Origin>,
}

tokio::task_local! { static CURRENT: Frame; }

impl Origin {
    pub(crate) fn enabled(owner: &Container) -> bool {
        use crate::contracts::container::Container as _;
        owner
            .make::<super::super::events::Listeners>()
            .is_ok_and(|listeners| listeners.has_listeners())
    }

    pub fn current() -> Option<Self> {
        CURRENT
            .try_with(|frame| Self::matches(frame).then(|| frame.origin.borrow().clone()))
            .ok()
            .flatten()
    }

    /// Updates metadata inside the current execution scope only.
    pub fn set(origin: Self) -> bool {
        CURRENT
            .try_with(|frame| {
                if !Self::matches(frame) {
                    return false;
                }
                *frame.origin.borrow_mut() = origin;
                true
            })
            .unwrap_or(false)
    }

    fn matches(frame: &Frame) -> bool {
        current_container()
            .is_some_and(|active| Weak::ptr_eq(&frame.owner, &Arc::downgrade(&active)))
    }

    pub(crate) fn job(queue: &str, job: &str) -> Option<Self> {
        match Self::current() {
            Some(Self::Console { command }) | Some(Self::Queue { command, .. }) => {
                Some(Self::Queue {
                    command,
                    queue: queue.into(),
                    job: job.into(),
                })
            }
            _ => None,
        }
    }

    pub(crate) fn http(request: &Request, default_guard: String) -> Self {
        Self::Http(Http {
            method: request.method().to_string(),
            path: request.path().into(),
            attributes: request.attributes().clone(),
            default_guard,
        })
    }

    pub(crate) async fn scope<F: Future>(
        owner: Arc<Container>,
        origin: Option<Self>,
        future: F,
    ) -> F::Output {
        let Some(origin) = origin else {
            return future.await;
        };
        CURRENT
            .scope(
                Frame {
                    owner: Arc::downgrade(&owner),
                    origin: RefCell::new(origin),
                },
                future,
            )
            .await
    }
}
