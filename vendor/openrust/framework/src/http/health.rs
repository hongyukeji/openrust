//! 应用健康诊断、进程存活与服务就绪端点。

use crate::core::{ApiResponse, Readiness, Result};
use crate::events::Dispatcher;
use crate::foundation::ApplicationState;
use crate::foundation::events::DiagnosingHealth;
use crate::foundation::exceptions::escape_html;
use crate::http::RequestInfo;
use crate::routing::router::Router;
use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
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

/// 派发应用健康诊断事件，根据请求返回 HTML 或含单一 `status` 字段的 JSON。
///
/// 在应用启动装配中用 `routes.get("/up", diagnose)` 注册，也可指定其他路径。
/// 生产诊断失败时返回 HTTP 500，并使用应用的异常报告策略记录错误。
pub async fn diagnose(
    State(state): State<ApplicationState>,
    request: RequestInfo,
) -> Result<Response> {
    let diagnostic = async {
        state
            .resolve::<Dispatcher>()
            .await?
            .dispatch(&DiagnosingHealth)
            .await
    }
    .await;
    let (status, health, heading) = match diagnostic {
        Ok(_) => (StatusCode::OK, "up", "Application up"),
        Err(error) => {
            if state.config().app.debug {
                return Err(error);
            }
            let context = crate::core::error::response_context();
            if let Some(handler) = context.exception_handler {
                handler.report_error(&error);
            } else {
                crate::foundation::ExceptionHandler::default().report_error(&error);
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "down",
                "Application experiencing problems",
            )
        }
    };
    let mut response = if request.expects_json()? {
        (status, Json(Health { status: health })).into_response()
    } else {
        let name = escape_html(&state.config().app.name);
        let html = format!(
            r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><title>{name}</title></head><body><h1>{heading}</h1><p>HTTP request received.</p></body></html>"#
        );
        (status, Html(html)).into_response()
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, private"),
    );
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        response
            .extensions_mut()
            .insert(crate::http::middleware::RenderedErrorResponse);
    }
    Ok(response)
}

/// 就绪探针：listener 正在接收流量且全部依赖探针成功时返回 200，否则返回 503。
pub async fn ready(State(state): State<ApplicationState>) -> Response {
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
