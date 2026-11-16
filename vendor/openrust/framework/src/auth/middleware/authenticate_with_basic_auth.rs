use crate::{
    auth::Auth,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};

/// Requires HTTP Basic authentication through a session guard.
#[derive(Default)]
pub struct AuthenticateWithBasicAuth {
    guard: Option<String>,
    field: Option<String>,
}

impl AuthenticateWithBasicAuth {
    pub fn new(guard: Option<&str>, field: Option<&str>) -> Self {
        Self {
            guard: guard.filter(|guard| !guard.is_empty()).map(str::to_owned),
            field: field.filter(|field| !field.is_empty()).map(str::to_owned),
        }
    }

    pub(crate) fn parse(parameters: &str) -> Option<Self> {
        let mut parts = parameters.split(',');
        let middleware = Self::new(parts.next(), parts.next());
        parts.next().is_none().then_some(middleware)
    }
}

impl Middleware for AuthenticateWithBasicAuth {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            Auth::sessionGuard(request, self.guard.as_deref())?
                .basic(self.field.as_deref().unwrap_or("email"), &[])
                .await?;
            Ok(next.run(request).await)
        })
    }
}
