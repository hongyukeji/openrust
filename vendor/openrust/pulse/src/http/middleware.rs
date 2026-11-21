use openrust::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    support::facades::{Config, Facade, Gate},
};
pub struct Authorize;
impl Middleware for Authorize {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let local = Config::getFacadeRoot()
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?
                .get("app.env")
                .and_then(|v| v.as_str().map(str::to_owned))
                .as_deref()
                == Some("local");
            if !local && !Gate::allows(request, "viewPulse", &[]).await? {
                return Ok(Response::empty(StatusCode::FORBIDDEN));
            }
            Ok(next.run(request).await)
        })
    }
}
