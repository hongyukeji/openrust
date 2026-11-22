use crate::{
    contracts::{
        container::Container as _, http::Middleware, routing::UrlGenerator as _, session::Session,
    },
    http::{HttpError, HttpFuture, Method, Next, Request, Response},
    session::{SessionError, SessionManager, session_manager::RequestSession},
    support::facades::{Auth, URL, current_container},
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
                .get::<SessionManager>("session")?;
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
            session.store.setId(id)?;
            let lock = manager
                .lock(request.session_block, session.store.getId())
                .await?;
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
    RequestSession {
        store: mut session,
        database_user,
    }: RequestSession,
) -> Result<Response, HttpError> {
    session.start().await?;
    manager.collect_garbage(&mut session).await?;
    request.setLaravelSession(session);
    let mut response = next.run(request).await;
    // Login and logout can change the identity. The guard still needs the active store.
    if !request.isPrecognitive()
        && let Some(database_user) = database_user
        && let Some(user) = Auth::manager()?.database_session_user(request).await?
    {
        let _ = database_user.set(user);
    }
    let mut session = request.take_session().ok_or(SessionError::Configuration)?;
    if request.matched_route
        && request.method() == Method::GET
        && !request.ajax()
        && !request.prefetch()
        && !request.isPrecognitive()
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
    if !request.isPrecognitive() {
        session.save().await?;
    }
    // Laravel 请求在响应完成后仍可访问 Session，供终止回调及 HTTP 断言读取。
    request.setLaravelSession(session);
    Ok(response)
}
