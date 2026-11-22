use crate::Feature;
use openrust::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    support::facades::{Config, Facade},
};
use std::sync::Arc;

type Respond = Arc<
    dyn for<'a> Fn(&'a mut Request, &'a [String]) -> HttpFuture<'a, Result<Response, HttpError>>
        + Send
        + Sync,
>;
pub struct EnsureFeaturesAreActive {
    features: Vec<String>,
    respondUsing: Option<Respond>,
}
impl EnsureFeaturesAreActive {
    pub fn using(features: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            features: features.into_iter().map(Into::into).collect(),
            respondUsing: None,
        }
    }
    pub fn whenInactive(
        mut self,
        callback: impl for<'a> Fn(
            &'a mut Request,
            &'a [String],
        ) -> HttpFuture<'a, Result<Response, HttpError>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.respondUsing = Some(Arc::new(callback));
        self
    }
}
impl Middleware for EnsureFeaturesAreActive {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let features: Vec<_> = self.features.iter().map(String::as_str).collect();
            if Feature::forRequest(request)
                .await?
                .someAreInactive(&features)
                .await?
            {
                if let Some(respond) = &self.respondUsing {
                    return respond(request, &self.features).await;
                }
                let debug = Config::getFacadeRoot()?
                    .get("app.debug")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                return Err(openrust::http::abort(
                    StatusCode::BAD_REQUEST,
                    if debug {
                        format!(
                            "Required features [{}] not enabled.",
                            self.features.join(", ")
                        )
                    } else {
                        String::new()
                    },
                    Default::default(),
                ));
            }
            Ok(next.run(request).await)
        })
    }
}

impl From<crate::Error> for HttpError {
    fn from(source: crate::Error) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}
