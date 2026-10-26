//! 基于应用共享 [`RateLimiter`] 的路由级限流。

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

/// 选择路由限流策略使用的请求身份。
#[derive(Clone)]
pub enum RateLimitKey {
    /// 使用 [`ClientIp`] 根据可信代理配置解析出的客户端地址。
    ClientIp,
    /// 使用已认证用户的稳定账户 ID。要求此层之前已安装 [`crate::auth::AuthLayer`]。
    #[cfg(feature = "auth")]
    AuthenticatedUser,
    /// 优先使用已认证账户 ID；匿名请求使用可信客户端地址。
    #[cfg(feature = "auth")]
    UserOrClientIp,
    /// 从请求构造应用自定义 key。闭包必须返回稳定、有界且不含秘密的身份值；该层会在 key 前添加策略作用域。
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

/// 通过已注册的共享限流器实施固定窗口策略的路由层。
#[derive(Clone, Debug)]
pub struct RateLimitLayer {
    scope: Arc<str>,
    limit: u64,
    window: Duration,
    key: RateLimitKey,
}

impl RateLimitLayer {
    /// 创建使用可信客户端 IP 作为身份的策略。
    pub fn by_client_ip(scope: impl Into<Arc<str>>, limit: u64, window: Duration) -> Self {
        Self::new(scope, limit, window, RateLimitKey::ClientIp)
    }

    /// 创建使用已认证账户 ID 作为身份的策略。
    #[cfg(feature = "auth")]
    pub fn by_user(scope: impl Into<Arc<str>>, limit: u64, window: Duration) -> Self {
        Self::new(scope, limit, window, RateLimitKey::AuthenticatedUser)
    }

    /// 创建优先使用账户 ID、否则使用可信客户端 IP 的策略。
    #[cfg(feature = "auth")]
    pub fn by_user_or_client_ip(scope: impl Into<Arc<str>>, limit: u64, window: Duration) -> Self {
        Self::new(scope, limit, window, RateLimitKey::UserOrClientIp)
    }

    /// 创建使用应用提供的身份函数的策略。
    pub fn with_key(
        scope: impl Into<Arc<str>>,
        limit: u64,
        window: Duration,
        key: impl Fn(&Request) -> crate::Result<String> + Send + Sync + 'static,
    ) -> Self {
        Self::new(scope, limit, window, RateLimitKey::Custom(Arc::new(key)))
    }

    /// 根据明确指定的 key 策略创建策略。
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

    /// 返回可与 [`crate::http::Routes::layer`] 配合使用的 Tower layer。
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
