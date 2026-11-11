use crate::{
    contracts::{http::Middleware, routing::UrlGenerator as _},
    http::{HttpError, HttpFuture, Next, Request, Response},
    routing::{SignatureMode, SignatureQueryFilter},
    support::facades::URL,
};

/// Rejects requests with invalid or expired URL signatures before the route handler.
pub struct ValidateSignature {
    mode: SignatureMode,
    ignored: Vec<String>,
}

impl ValidateSignature {
    pub fn absolute() -> Self {
        Self {
            mode: SignatureMode::Absolute,
            ignored: Vec::new(),
        }
    }

    pub fn relative() -> Self {
        Self {
            mode: SignatureMode::Relative,
            ignored: Vec::new(),
        }
    }

    pub fn except(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.ignored.extend(names.into_iter().map(Into::into));
        self
    }

    pub(super) fn parse(parameters: &str) -> Option<Self> {
        let mut names = parameters.split(',').peekable();
        let middleware = match names.peek() {
            Some(&"relative") => {
                names.next();
                Self::relative()
            }
            Some(&"absolute") => {
                names.next();
                Self::absolute()
            }
            _ => Self::absolute(),
        };
        let names = names.collect::<Vec<_>>();
        (!names.contains(&"")).then(|| middleware.except(names))
    }
}

impl Middleware for ValidateSignature {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let urls = URL::service()?;
            let ignore = |name: &str| self.ignored.iter().any(|ignored| ignored == name);
            if !urls.hasValidSignature(
                request,
                self.mode,
                SignatureQueryFilter::Predicate(&ignore),
            )? {
                return Err(HttpError::InvalidSignature);
            }
            Ok(next.run(request).await)
        })
    }
}
