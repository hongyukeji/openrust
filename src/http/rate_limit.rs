//! Route-level rate limiting backed by the application's shared [`RateLimiter`].

use std::{fmt, sync::Arc, time::Duration};

use axum::{
    extract::{Extension, Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::Route,
};
use tower::{Layer, Service};

use crate::{
    core::{AppState, RateLimiter},
    http::ClientIp,
};

type KeyFn = dyn Fn(&Request) -> crate::Result<String> + Send + Sync;

/// Selects the request identity used by a route rate-limit policy.
#[derive(Clone)]
pub enum RateLimitKey {
    /// Use the client address resolved by [`ClientIp`] and trusted proxy configuration.
    ClientIp,
    /// Use an authenticated user's stable account ID. Requires an [`crate::auth::AuthLayer`]
    /// to run before this layer.
    #[cfg(feature = "auth")]
    AuthenticatedUser,
    /// Use the authenticated account ID, or the trusted client address for anonymous requests.
    #[cfg(feature = "auth")]
    UserOrClientIp,
    /// Build an application-defined key from the request. The closure must return a stable,
    /// bounded, non-secret identity value; the policy scope is prepended by the layer.
    Custom(Arc<KeyFn>),
}

impl fmt::Debug for RateLimitKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientIp => formatter.write_str("ClientIp"),
            #[cfg(feature = "auth")]
            Self::AuthenticatedUser => formatter.write_str("AuthenticatedUser"),
            #[cfg(feature = "auth")]
            Self::UserOrClientIp => formatter.write_str("UserOrClientIp"),
            Self::Custom(_) => formatter.write_str("Custom(..)"),
        }
    }
}

/// A route layer applying a fixed-window policy through the registered shared limiter.
#[derive(Clone, Debug)]
pub struct RateLimitLayer {
    scope: Arc<str>,
    limit: u64,
    window: Duration,
    key: RateLimitKey,
}

impl RateLimitLayer {
    /// Creates a policy using the trusted client IP as its identity.
    pub fn by_client_ip(scope: impl Into<Arc<str>>, limit: u64, window: Duration) -> Self {
        Self::new(scope, limit, window, RateLimitKey::ClientIp)
    }

    /// Creates a policy using an authenticated account ID.
    #[cfg(feature = "auth")]
    pub fn by_user(scope: impl Into<Arc<str>>, limit: u64, window: Duration) -> Self {
        Self::new(scope, limit, window, RateLimitKey::AuthenticatedUser)
    }

    /// Creates a policy using account ID when present, otherwise trusted client IP.
    #[cfg(feature = "auth")]
    pub fn by_user_or_client_ip(scope: impl Into<Arc<str>>, limit: u64, window: Duration) -> Self {
        Self::new(scope, limit, window, RateLimitKey::UserOrClientIp)
    }

    /// Creates a policy using an application-supplied identity function.
    pub fn with_key(
        scope: impl Into<Arc<str>>,
        limit: u64,
        window: Duration,
        key: impl Fn(&Request) -> crate::Result<String> + Send + Sync + 'static,
    ) -> Self {
        Self::new(scope, limit, window, RateLimitKey::Custom(Arc::new(key)))
    }

    /// Creates a policy from an explicit key strategy.
    pub fn new(
        scope: impl Into<Arc<str>>,
        limit: u64,
        window: Duration,
        key: RateLimitKey,
    ) -> Self {
        Self {
            scope: scope.into(),
            limit,
            window,
            key,
        }
    }

    /// Returns a Tower layer for use with [`crate::http::Routes::layer`].
    pub fn layer(
        &self,
    ) -> impl Layer<
        Route,
        Service: Service<
            Request,
            Response = Response,
            Error = std::convert::Infallible,
            Future: Send + 'static,
        > + Clone
                     + Send
                     + Sync
                     + 'static,
    > + Clone
    + Send
    + Sync
    + 'static {
        middleware::from_fn_with_state::<_, _, (State<RateLimitLayer>, Extension<AppState>, Request)>(
            self.clone(),
            enforce,
        )
    }
}

async fn enforce(
    axum::extract::State(policy): axum::extract::State<RateLimitLayer>,
    Extension(state): Extension<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let identity = match request_key(&request, &state, &policy.key) {
        Ok(identity) => identity,
        Err(error) => return error.into_response(),
    };
    let key = format!("{}:{identity}", policy.scope);
    let limiter = match state.try_get::<RateLimiter>() {
        Ok(limiter) => limiter,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = limiter.check(&key, policy.limit, policy.window).await {
        return error.into_response();
    }
    next.run(request).await
}

fn request_key(request: &Request, state: &AppState, key: &RateLimitKey) -> crate::Result<String> {
    match key {
        RateLimitKey::ClientIp => client_ip_key(request, state),
        #[cfg(feature = "auth")]
        RateLimitKey::AuthenticatedUser => request
            .extensions()
            .get::<crate::auth::CurrentUser>()
            .map(|user| format!("user:{}", user.id()))
            .ok_or(crate::Error::Unauthorized),
        #[cfg(feature = "auth")]
        RateLimitKey::UserOrClientIp => {
            match request.extensions().get::<crate::auth::CurrentUser>() {
                Some(user) => Ok(format!("user:{}", user.id())),
                None => client_ip_key(request, state),
            }
        }
        RateLimitKey::Custom(build) => build(request),
    }
}

fn client_ip_key(request: &Request, state: &AppState) -> crate::Result<String> {
    let ip = ClientIp::from_request(request, state)?;
    Ok(format!("ip:{}", ip.ip()))
}
