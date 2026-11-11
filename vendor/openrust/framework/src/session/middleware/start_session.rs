use crate::{
    contracts::{container::Container as _, http::Middleware, session::Session},
    http::{HttpError, HttpFuture, Next, Request, Response},
    session::{SessionError, SessionManager},
    support::facades::current_container,
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
            let Some(mut session) = manager.build(request)? else {
                return Ok(next.run(request).await);
            };
            let id = request.cookie(Some(manager.cookie_name()))?;
            session.setId(id.as_ref().and_then(serde_json::Value::as_str))?;
            session.start().await?;
            manager.collect_garbage(&mut session).await?;
            request.setLaravelSession(session);
            let mut response = next.run(request).await;
            let mut session = request.take_session().ok_or(SessionError::Configuration)?;
            let cookie = manager.cookie(&session)?;
            response.setCookie(&cookie, request.is_secure())?;
            session.save().await?;
            Ok(response)
        })
    }
}
