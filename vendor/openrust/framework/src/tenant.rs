#![doc = include_str!("tenant/README.md")]
//! 可信租户解析与显式异步作用域。
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{future::Future, sync::Arc};

use axum::{
    extract::{Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::Route,
};
use tower::{Layer, Service};

use crate::{Error, Result};

const MAX_TENANT_ID_BYTES: usize = 128;

/// 已校验且不透明的租户标识符。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct TenantId(String);

impl TenantId {
    /// 校验并创建租户标识符。
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_TENANT_ID_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        {
            return Err(Error::custom(
                "tenant_invalid_id",
                "tenant ID must contain 1 to 128 safe ASCII characters",
            ));
        }
        Ok(Self(value))
    }

    /// 返回租户标识符文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for TenantId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("TenantId")
            .field(&"[REDACTED]")
            .finish()
    }
}

tokio::task_local! {
    static CURRENT_TENANT: Option<TenantId>;
}

/// 读取并显式建立当前异步调用的租户作用域。
pub struct TenantContext;

impl TenantContext {
    /// 返回当前 task-local 租户；未启用隔离时为 `None`。
    pub fn current() -> Option<TenantId> {
        CURRENT_TENANT.try_with(Clone::clone).ok().flatten()
    }

    /// 要求当前调用已绑定租户；缺失时拒绝操作。
    pub fn require_current() -> Result<TenantId> {
        Self::current().ok_or_else(|| {
            Error::custom_with_status(
                axum::http::StatusCode::FORBIDDEN,
                "tenant_context_missing",
                "a trusted tenant context is required",
            )
        })
    }

    /// 在指定租户的异步作用域运行 future。
    ///
    /// 已有不同租户的父作用域不能被覆盖；task-local 不会自动传播到 `tokio::spawn`。
    pub async fn scope<F: Future>(tenant: TenantId, future: F) -> Result<F::Output> {
        if Self::current().is_some_and(|current| current != tenant) {
            return Err(Error::custom_with_status(
                axum::http::StatusCode::FORBIDDEN,
                "tenant_scope_switch_denied",
                "a nested call cannot change its tenant context",
            ));
        }
        Ok(CURRENT_TENANT.scope(Some(tenant), future).await)
    }

    /// 在全局调度触发内隔离外层请求租户；仅框架可使用此边界。
    #[cfg(any(feature = "queue", test))]
    pub(crate) async fn without_current<F: Future>(future: F) -> F::Output {
        CURRENT_TENANT.scope(None, future).await
    }
}

type Resolver = dyn Fn(&Request) -> Result<TenantId> + Send + Sync;

/// 由应用提供可信身份到租户标识的 HTTP 解析层。
#[derive(Clone)]
pub struct TenantLayer {
    resolver: Arc<Resolver>,
}

impl TenantLayer {
    /// 创建解析层；resolver 必须从已认证身份或其他明确可信的请求状态得出租户。
    ///
    /// 框架不内置读取任意 header/query 的 resolver。resolver 错误会失败关闭。
    pub fn new(resolver: impl Fn(&Request) -> Result<TenantId> + Send + Sync + 'static) -> Self {
        Self {
            resolver: Arc::new(resolver),
        }
    }

    /// 返回可与 [`crate::routing::Router::layer`] 配合使用的 Tower layer。
    ///
    /// 需要认证时，应让 `AuthLayer` 处于外层，使身份先完成验证。
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
        middleware::from_fn_with_state(self.clone(), resolve_tenant)
    }
}

async fn resolve_tenant(
    State(layer): State<TenantLayer>,
    request: Request,
    next: Next,
) -> Response {
    let Ok(tenant) = (layer.resolver)(&request) else {
        return Error::custom_with_status(
            axum::http::StatusCode::FORBIDDEN,
            "tenant_resolution_denied",
            "trusted tenant resolution failed",
        )
        .into_response();
    };
    match TenantContext::scope(tenant, next.run(request)).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;

    use super::{TenantContext, TenantId, TenantLayer};

    #[test]
    fn validates_tenant_identifiers_without_disclosing_them() -> crate::Result<()> {
        let tenant = TenantId::new("tenant.example-1")?;
        assert_eq!(tenant.as_str(), "tenant.example-1");
        assert!(format!("{tenant:?}").contains("[REDACTED]"));
        for invalid in ["", "../tenant", "tenant/name", "tenant name", "租户"] {
            assert!(TenantId::new(invalid).is_err());
        }
        assert!(TenantId::new("a".repeat(129)).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn scopes_are_isolated_and_cannot_switch_nested_tenant() -> crate::Result<()> {
        assert!(TenantContext::current().is_none());
        let tenant_a = TenantId::new("tenant-a")?;
        TenantContext::scope(tenant_a.clone(), async {
            assert_eq!(TenantContext::require_current()?.as_str(), "tenant-a");
            let switched = TenantContext::scope(TenantId::new("tenant-b")?, async {
                Ok::<_, crate::Error>(())
            })
            .await;
            assert_eq!(
                switched.err().map(|error| error.code().to_owned()),
                Some("tenant_scope_switch_denied".to_owned())
            );
            Ok::<_, crate::Error>(())
        })
        .await??;
        assert!(TenantContext::current().is_none());
        assert_eq!(
            TenantContext::require_current()
                .err()
                .map(|error| error.code().to_owned()),
            Some("tenant_context_missing".to_owned())
        );
        Ok(())
    }

    #[tokio::test]
    async fn global_trigger_temporarily_clears_outer_tenant() -> crate::Result<()> {
        TenantContext::scope(TenantId::new("tenant-a")?, async {
            assert_eq!(TenantContext::require_current()?.as_str(), "tenant-a");
            TenantContext::without_current(async {
                assert!(TenantContext::current().is_none());
                TenantContext::scope(TenantId::new("job-tenant")?, async {
                    assert_eq!(TenantContext::require_current()?.as_str(), "job-tenant");
                    Ok::<_, crate::Error>(())
                })
                .await??;
                assert!(TenantContext::current().is_none());
                Ok::<_, crate::Error>(())
            })
            .await?;
            assert_eq!(TenantContext::require_current()?.as_str(), "tenant-a");
            Ok::<_, crate::Error>(())
        })
        .await??;
        Ok(())
    }

    #[tokio::test]
    async fn resolver_failure_rejects_without_running_the_handler() -> crate::Result<()> {
        let calls = Arc::new(AtomicUsize::new(0));
        let handler_calls = Arc::clone(&calls);
        let app = axum::Router::new()
            .route(
                "/",
                get(move || {
                    let calls = Arc::clone(&handler_calls);
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        StatusCode::NO_CONTENT
                    }
                }),
            )
            .layer(TenantLayer::new(|_| Err(crate::Error::Unauthorized)).layer());
        let response = match app.oneshot(Request::new(Body::empty())).await {
            Ok(response) => response,
            Err(never) => match never {},
        };
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .map_err(crate::Error::internal)?;
        let body: serde_json::Value =
            serde_json::from_slice(&body).map_err(crate::Error::internal)?;
        assert_eq!(body["success"], false);
        assert_eq!(body["code"], "tenant_resolution_denied");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        Ok(())
    }
}
