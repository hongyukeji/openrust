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
    future: HttpFuture<'static, std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>>,
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
        self.invoke_with(successful, |error| async move {
            let _ = application.report(error.as_ref()).await;
        })
        .await;
    }

    pub(crate) async fn invoke_with<F>(
        &self,
        successful: bool,
        report: impl Fn(Box<dyn std::error::Error + Send + Sync>) -> F,
    ) where
        F: Future<Output = ()>,
    {
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
                report(error).await;
            }
        }
    }
}

pub(crate) fn enqueue<E>(
    future: HttpFuture<'static, std::result::Result<(), E>>,
) -> Result<DeferredCallback>
where
    E: std::error::Error + Send + Sync + 'static,
{
    CURRENT
        .try_with(|current| {
            let always = Arc::new(AtomicBool::new(false));
            current
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(Callback {
                    always: always.clone(),
                    future: Box::pin(async move { future.await.map_err(Into::into) }),
                });
            DeferredCallback(always)
        })
        .map_err(|_| Error::OutsideRequest)
}
