use crate::{
    contracts::{
        container::Container as _, http::Middleware, routing::UrlGenerator as _, session::Session,
    },
    http::{HttpError, HttpFuture, Method, Next, Request, Response},
    session::{SessionError, SessionManager, Store},
    support::facades::{URL, current_container},
};

/// Loads the request session and persists it on the response path.
pub struct StartSession;

impl Middleware for StartSession {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let manager = current_container()
                .ok_or(SessionError::Configuration)?
                .get::<SessionManager>("session")
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
            if !manager.is_configured() || request.hasSession() {
                return Ok(next.run(request).await);
            }
            if !request.cookie_is_protected(manager.cookie_name()) {
                return Err(SessionError::UnprotectedCookie.into());
            }
            let id = request.cookie(Some(manager.cookie_name()))?;
            let id = id.as_ref().and_then(serde_json::Value::as_str);
            let Some(mut session) = manager.build(request, id)? else {
                return Ok(next.run(request).await);
            };
            session.setId(id)?;
            let lock = manager.lock(request.session_block, session.getId()).await?;
            let response = handle_session(&manager, request, next, session).await;
            if let Some(lock) = lock {
                let release = lock.release().await;
                if response.is_ok() {
                    release?;
                }
            }
            response
        })
    }
}

async fn handle_session(
    manager: &SessionManager,
    request: &mut Request,
    next: Next<'_>,
    mut session: Store,
) -> Result<Response, HttpError> {
    session.start().await?;
    manager.collect_garbage(&mut session).await?;
    request.setLaravelSession(session);
    let mut response = next.run(request).await;
    let mut session = request.take_session().ok_or(SessionError::Configuration)?;
    if request.matched_route
        && request.method() == Method::GET
        && request
            .headers()
            .get("x-requested-with")
            .is_none_or(|value| value != "XMLHttpRequest")
        && !["purpose", "sec-purpose", "x-purpose"]
            .into_iter()
            .any(|name| {
                request
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.contains("prefetch"))
            })
        && request
            .headers()
            .get("precognition")
            .is_none_or(|value| value != "true")
        && let Some(path) = request.uri().path_and_query()
        && path.as_str().starts_with('/')
        && !path.as_str().starts_with("//")
    {
        session.setPreviousUrl(&URL::forRequest(request)?.full()?)?;
        session.setPreviousRoute(request.route_name.as_deref())?;
    }
    #[cfg(feature = "sanctum")]
    let stateful_frontend = crate::sanctum::StatefulFrontend::is_active(request);
    #[cfg(not(feature = "sanctum"))]
    let stateful_frontend = false;
    let cookie = manager.cookie(&session, stateful_frontend)?;
    response.setCookie(&cookie, request.is_secure())?;
    session.save().await?;
    Ok(response)
}
