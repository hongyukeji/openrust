use std::{
    any::TypeId,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use axum::body::Bytes;
use http_body::{Frame, SizeHint};
use tokio_util::task::task_tracker::TaskTrackerToken;

use super::Kernel;
use crate::{
    contracts::http::{Kernel as _, Middleware, TerminableMiddleware},
    http::{Body, HttpError, HttpFuture, Next, Request, RequestParts, Response, ResponseParts},
};

struct Terminable<M>(Arc<M>);

pub(crate) fn terminable<M: TerminableMiddleware>(middleware: Arc<M>) -> Arc<dyn Middleware> {
    Arc::new(Terminable(middleware))
}

impl<M: TerminableMiddleware> Middleware for Terminable<M> {
    fn middleware_type_id(&self) -> TypeId {
        TypeId::of::<M>()
    }

    fn terminable(&self) -> Option<&dyn TerminableMiddleware> {
        Some(self.0.as_ref())
    }

    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        self.0.handle(request, next)
    }
}

/// Request metadata shared by the kernel and the HTTP transport.
#[derive(Clone)]
pub(super) struct Termination(Arc<Mutex<Vec<Arc<dyn Middleware>>>>);

impl Termination {
    pub(super) fn new(middleware: Vec<Arc<dyn Middleware>>) -> Self {
        Self(Arc::new(Mutex::new(middleware)))
    }

    pub(super) fn take(&self) -> Vec<Arc<dyn Middleware>> {
        std::mem::take(&mut *self.0.lock().expect("termination lock poisoned"))
    }
}

pub(super) struct ResponseBody {
    body: Body,
    callback: Option<Callback>,
}

struct Callback {
    runtime: tokio::runtime::Handle,
    task: TaskTrackerToken,
    kernel: Arc<Kernel>,
    request: RequestParts,
    response: ResponseParts,
}

impl ResponseBody {
    pub(super) fn wrap(
        body: Body,
        kernel: Arc<Kernel>,
        request: RequestParts,
        response: ResponseParts,
    ) -> Body {
        Body::new(Self {
            body,
            callback: Some(Callback {
                runtime: tokio::runtime::Handle::current(),
                task: kernel.termination_tasks.token(),
                kernel,
                request,
                response,
            }),
        })
    }
}

impl http_body::Body for ResponseBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.body).poll_frame(context)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

impl Drop for ResponseBody {
    fn drop(&mut self) {
        if let Some(Callback {
            runtime,
            task,
            kernel,
            request,
            response,
        }) = self.callback.take()
        {
            runtime.spawn(async move {
                kernel.terminate(&request, &response).await;
                drop(task);
            });
        }
    }
}
