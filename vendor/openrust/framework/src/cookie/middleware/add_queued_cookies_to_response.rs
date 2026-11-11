use std::cell::RefCell;

use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
    support::facades::Cookie,
};

use crate::cookie::CookieJar;

tokio::task_local! {
    static QUEUE: Option<RefCell<CookieJar>>;
}

/// Adds request-local queued cookies after the downstream handler runs.
pub struct AddQueuedCookiesToResponse;

impl Middleware for AddQueuedCookiesToResponse {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let jar = Cookie::factory()?.for_request();
            QUEUE
                .scope(Some(RefCell::new(jar)), async move {
                    let mut response = next.run(request).await;
                    for cookie in with_queue(CookieJar::drain)? {
                        response.setCookie(&cookie, request.is_secure())?;
                    }
                    Ok(response)
                })
                .await
        })
    }
}

pub(in crate::cookie) fn with_queue<T>(
    f: impl FnOnce(&mut CookieJar) -> T,
) -> Result<T, crate::cookie::CookieError> {
    QUEUE
        .try_with(|jar| jar.as_ref().map(|jar| f(&mut jar.borrow_mut())))
        .ok()
        .flatten()
        .ok_or(crate::cookie::CookieError)
}

pub(crate) async fn request_scope<F: std::future::Future>(future: F) -> F::Output {
    QUEUE.scope(None, future).await
}
