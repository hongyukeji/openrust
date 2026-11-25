use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use crate::{
    container::Container, contracts::container::Container as _, foundation::Application,
    http::HttpFuture,
};

use super::{Error, Manager, Result, facade::DeferredRun};

tokio::task_local! {
    static CURRENT: DeferredCallbacks;
}

#[derive(Clone, Default)]
pub(crate) struct DeferredCallbacks {
    callbacks: Arc<Mutex<Vec<Callback>>>,
    transfer: bool,
    after_http: Option<Arc<AfterHttp>>,
}

struct Callback {
    name: Option<String>,
    always: Arc<AtomicBool>,
    action: Action,
}

type DeferredFuture =
    HttpFuture<'static, std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>>;
type AfterHttpCallback = Box<dyn FnOnce(Duration) -> DeferredFuture + Send>;

struct AfterHttp {
    started_at: Instant,
    callbacks: Mutex<Vec<AfterHttpCallback>>,
    terminating: AtomicBool,
}

enum Action {
    Local {
        future: DeferredFuture,
        skipped: Option<DeferredFuture>,
    },
    Transfer(Box<dyn FnOnce() -> DeferredRun + Send>),
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct TransferredRun {
    run: DeferredRun,
    always: bool,
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
    pub(crate) fn for_http(started_at: Instant) -> Self {
        Self {
            after_http: Some(Arc::new(AfterHttp {
                started_at,
                callbacks: Mutex::default(),
                terminating: AtomicBool::new(false),
            })),
            ..Self::default()
        }
    }

    pub(crate) fn is_active() -> bool {
        CURRENT.try_with(|_| ()).is_ok()
    }

    pub(crate) fn for_isolated_job() -> Self {
        Self {
            transfer: true,
            ..Self::default()
        }
    }

    pub(crate) fn take_transfers(&self) -> Vec<TransferredRun> {
        let pending = std::mem::take(
            &mut *self
                .callbacks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        pending
            .into_iter()
            .filter_map(|callback| match callback.action {
                Action::Transfer(snapshot) => Some(TransferredRun {
                    run: snapshot(),
                    always: callback.always.load(Ordering::Acquire),
                }),
                Action::Local { .. } => None,
            })
            .collect()
    }

    pub(crate) async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.after_http.as_ref().is_none_or(|http| {
            http.callbacks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        }) && self
            .callbacks
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

    pub(crate) fn begin_http_termination(&self) -> bool {
        self.after_http
            .as_ref()
            .is_none_or(|http| !http.terminating.swap(true, Ordering::AcqRel))
    }

    pub(crate) async fn invoke_after_http(&self, application: &Application) {
        let Some(http) = &self.after_http else {
            return;
        };
        let pending = std::mem::take(
            &mut *http
                .callbacks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let duration = http.started_at.elapsed();
        for callback in pending {
            if let Err(error) = callback(duration).await {
                let _ = application.report(error.as_ref()).await;
            }
        }
    }

    pub(crate) async fn invoke_with<F>(
        &self,
        successful: bool,
        report: impl Fn(Box<dyn std::error::Error + Send + Sync>) -> F,
    ) where
        F: Future<Output = ()>,
    {
        self.scope(async {
            loop {
                let callbacks = {
                    let mut pending = self
                        .callbacks
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    pending
                        .extract_if(.., |callback| {
                            matches!(callback.action, Action::Local { .. })
                        })
                        .collect::<Vec<_>>()
                };
                if callbacks.is_empty() {
                    break;
                }
                for callback in callbacks {
                    let Action::Local { future, skipped } = callback.action else {
                        continue;
                    };
                    let future = if successful || callback.always.load(Ordering::Acquire) {
                        Some(future)
                    } else {
                        skipped
                    };
                    if let Some(future) = future
                        && let Err(error) = future.await
                    {
                        report(error).await;
                    }
                }
            }
        })
        .await;
    }
}

pub(crate) fn enqueue_after_http<F, E>(
    callback: impl FnOnce(Duration) -> F + Send + 'static,
) -> Result<()>
where
    F: Future<Output = std::result::Result<(), E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    CURRENT
        .try_with(|current| {
            let http = current.after_http.as_ref().ok_or(Error::OutsideRequest)?;
            http.callbacks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(Box::new(move |duration| {
                    boxed(Box::pin(callback(duration)))
                }));
            Ok(())
        })
        .map_err(|_| Error::OutsideRequest)?
}

pub(crate) fn enqueue<E>(
    future: HttpFuture<'static, std::result::Result<(), E>>,
) -> Result<DeferredCallback>
where
    E: std::error::Error + Send + Sync + 'static,
{
    enqueue_with_skip(future, None)
}

pub(crate) fn enqueue_with_skip<E>(
    future: HttpFuture<'static, std::result::Result<(), E>>,
    skipped: Option<HttpFuture<'static, std::result::Result<(), E>>>,
) -> Result<DeferredCallback>
where
    E: std::error::Error + Send + Sync + 'static,
{
    register(None, |_| Action::Local {
        future: boxed(future),
        skipped: skipped.map(boxed),
    })
}

pub(crate) fn enqueue_named<E>(
    name: String,
    future: HttpFuture<'static, std::result::Result<(), E>>,
) -> Result<DeferredCallback>
where
    E: std::error::Error + Send + Sync + 'static,
{
    register(Some(name), |_| Action::Local {
        future: boxed(future),
        skipped: None,
    })
}

fn boxed<E>(future: HttpFuture<'static, std::result::Result<(), E>>) -> DeferredFuture
where
    E: std::error::Error + Send + Sync + 'static,
{
    Box::pin(async move { future.await.map_err(Into::into) })
}

pub(super) fn enqueue_run(
    snapshot: impl FnOnce() -> DeferredRun + Send + 'static,
    execute: impl FnOnce(DeferredRun) -> HttpFuture<'static, Result<()>> + Send + 'static,
) -> Result<DeferredCallback> {
    register(None, |transfer| {
        if transfer {
            Action::Transfer(Box::new(snapshot))
        } else {
            Action::Local {
                future: boxed(Box::pin(async move { execute(snapshot()).await })),
                skipped: None,
            }
        }
    })
}

pub(crate) fn restore(owner: Arc<Container>, transferred: TransferredRun) -> Result<()> {
    let TransferredRun { run, always } = transferred;
    let callback = enqueue(Box::pin(async move {
        let manager = owner.make::<Manager>()?;
        manager.resume(run).await
    }))?;
    if always {
        let _ = callback.always();
    }
    Ok(())
}

fn register(name: Option<String>, action: impl FnOnce(bool) -> Action) -> Result<DeferredCallback> {
    CURRENT
        .try_with(|current| {
            let always = Arc::new(AtomicBool::new(false));
            let action = action(current.transfer);
            let mut callbacks = current
                .callbacks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let duplicate = name
                .as_ref()
                .and_then(|name| {
                    callbacks
                        .iter()
                        .position(|item| item.name.as_ref() == Some(name))
                })
                .map(|index| callbacks.remove(index));
            callbacks.push(Callback {
                name,
                always: always.clone(),
                action,
            });
            drop(callbacks);
            drop(duplicate);
            DeferredCallback(always)
        })
        .map_err(|_| Error::OutsideRequest)
}
