use openrust::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    support::facades::Facade,
};
pub struct Authorize;
impl Middleware for Authorize {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let telescope = crate::facades::Telescope::getFacadeRoot().map_err(|source| {
                HttpError::Internal {
                    source: Box::new(source),
                }
            })?;
            telescope.flushEntries();
            telescope.stopRecording();
            if telescope.check(request).await? {
                Ok(next.run(request).await)
            } else {
                Ok(Response::empty(StatusCode::FORBIDDEN))
            }
        })
    }
}
