//! 存活探针端点。

use crate::core::{ApiResponse, Readiness};
use crate::routing::router::Router;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// HTTP readiness 服务类型别名，HTTP 与 gRPC 共用同一进程状态。
pub(crate) type ServerReadiness = Readiness;

/// `GET /health` 的响应正文。
#[derive(Debug, Serialize)]
pub struct Health {
    /// 进程能够响应时始终为 `"ok"`。
    pub status: &'static str,
}

/// 存活探针：只要进程能够处理请求就返回成功，不检查外部依赖。
pub async fn live() -> ApiResponse<Health> {
    ApiResponse::success(Health { status: "ok" })
}

/// 就绪探针：listener 正在接收流量且全部依赖探针成功时返回 200，否则返回 503。
pub async fn ready(State(state): State<crate::foundation::ApplicationState>) -> Response {
    let is_ready = match state.resolve::<Readiness>().await {
        Ok(readiness) => readiness.is_ready() && readiness.dependencies_ready().await,
        Err(_) => false,
    };
    let mut response = ApiResponse::success(Health {
        status: if is_ready { "ready" } else { "not_ready" },
    })
    .into_response();
    if !is_ready {
        *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    }
    response
}

/// 注册 `GET /health` 和 `GET /ready`。
pub fn routes(routes: Router) -> Router {
    routes.get("/health", live).get("/ready", ready)
}

#[cfg(test)]
mod tests {
    use super::ready;
    use crate::core::Error;
    use crate::core::Readiness;
    use crate::foundation::ProviderBuilder;
    use crate::foundation::RegisterContext;
    use crate::support::ServiceProvider;
    use axum::{extract::State, http::StatusCode};
    use figment::Figment;

    struct TestReadiness(Readiness);

    impl ServiceProvider for TestReadiness {
        fn register<'a>(
            &'a self,
            app: &'a mut RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, crate::core::Result<()>> {
            Box::pin(async move {
                let readiness = self.0.clone();
                app.singleton::<Readiness, _>(move |_| {
                    let readiness = readiness.clone();
                    Box::pin(async move { Ok(std::sync::Arc::new(readiness)) })
                })
                .await?;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn ready_returns_503_without_leaking_dependency_errors() -> crate::core::Result<()> {
        let readiness = Readiness::default();
        readiness.register_probe("database.primary", || async {
            Err(Error::custom(
                "database_secret",
                "private connection detail",
            ))
        })?;
        let mut builder = ProviderBuilder::new();
        builder.provider(TestReadiness(readiness.clone()));
        let app = crate::foundation::Application::from_provider_runtime(
            builder.start(Figment::new()).await?,
        )?;
        let _listener = readiness.begin_listening();

        let response = ready(State(app.state().clone())).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .map_err(Error::internal)?;
        let body = String::from_utf8(body.to_vec()).map_err(Error::internal)?;
        assert!(!body.contains("database_secret"));
        assert!(!body.contains("private connection detail"));
        Ok(())
    }
}
