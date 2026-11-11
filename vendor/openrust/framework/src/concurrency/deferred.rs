use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{foundation::Application, http::HttpFuture};

use super::{Error, Result};

tokio::task_local! {
    static CURRENT: DeferredCallbacks;
}

#[derive(Clone, Default)]
pub(crate) struct DeferredCallbacks(Arc<Mutex<Vec<Callback>>>);

struct Callback {
    always: Arc<AtomicBool>,
    future: HttpFuture<'static, Result<()>>,
}

#[must_use]
pub struct DeferredCallback(Arc<AtomicBool>);

impl DeferredCallback {
    pub fn always(self) -> Self {
        self.0.store(true, Ordering::Release);
        self
    }
}

impl DeferredCallbacks {
    pub(crate) async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    pub(crate) async fn invoke(&self, application: &Application, successful: bool) {
        let callbacks = std::mem::take(
            &mut *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for callback in callbacks {
            if (successful || callback.always.load(Ordering::Acquire))
                && let Err(error) = callback.future.await
            {
                let _ = application.report(&error).await;
            }
        }
    }
}

pub(super) fn enqueue(future: HttpFuture<'static, Result<()>>) -> Result<DeferredCallback> {
    CURRENT
        .try_with(|current| {
            let always = Arc::new(AtomicBool::new(false));
            current
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(Callback {
                    always: always.clone(),
                    future,
                });
            DeferredCallback(always)
        })
        .map_err(|_| Error::OutsideRequest)
}
