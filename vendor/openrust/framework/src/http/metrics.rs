//! Prometheus 指标采集端点与低基数 HTTP 观测。

use std::sync::OnceLock;

use axum::{
    body::Body,
    http::{HeaderValue, Method, StatusCode, header},
    response::Response,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use crate::core::Error;
use crate::routing::Router;

static PROMETHEUS: OnceLock<std::result::Result<PrometheusHandle, String>> = OnceLock::new();

/// 初始化进程级 Prometheus recorder；重复调用会复用首次创建的 handle。
pub(crate) fn initialize() -> crate::core::Result<PrometheusHandle> {
    match PROMETHEUS.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .map_err(|error| error.to_string())
    }) {
        Ok(handle) => Ok(handle.clone()),
        Err(message) => Err(Error::custom(
            "metrics_recorder_unavailable",
            format!("Prometheus recorder initialization failed: {message}"),
        )),
    }
}

/// `GET /metrics`：输出 Prometheus 文本格式指标。
pub async fn scrape() -> Response {
    let Ok(handle) = initialize() else {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )
            .body(Body::from("metrics unavailable"))
            .unwrap_or_else(|_| Response::new(Body::empty()));
    };
    handle.run_upkeep();
    let body = handle.render();
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
        )
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

/// 注册 Prometheus scrape 路由。
pub(crate) fn routes(routes: Router) -> Router {
    routes.get("/metrics", scrape)
}

pub(super) fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        Method::HEAD => "HEAD",
        _ => "OTHER",
    }
}

pub(super) fn status_class(status: StatusCode) -> &'static str {
    match status.as_u16() / 100 {
        1 => "1xx",
        2 => "2xx",
        3 => "3xx",
        4 => "4xx",
        5 => "5xx",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::method_label;
    use super::status_class;
    use axum::http::{Method, StatusCode, header};

    #[test]
    fn metric_labels_are_restricted_to_fixed_values() {
        let custom = Method::from_bytes(b"USER-123").unwrap_or(Method::GET);
        assert_eq!(method_label(&custom), "OTHER");
        assert_eq!(method_label(&Method::GET), "GET");
        assert_eq!(status_class(StatusCode::TOO_MANY_REQUESTS), "4xx");
        assert_eq!(status_class(StatusCode::INTERNAL_SERVER_ERROR), "5xx");
    }

    #[tokio::test]
    async fn scrape_route_is_opt_in_and_does_not_export_request_secrets() -> crate::core::Result<()>
    {
        use crate::config::FrameworkConfig;
        use crate::foundation::BootContext;
        use crate::foundation::ProviderBuilder;
        use crate::foundation::http::build_router;
        use crate::support::BoxFuture;
        use crate::support::ServiceProvider;
        use axum::{body::Body, http::Request};
        use figment::{Figment, providers::Serialized};
        use tower::ServiceExt;

        struct MetricsRoutes;
        impl ServiceProvider for MetricsRoutes {
            fn boot<'a>(
                &'a self,
                app: &'a mut BootContext<'_>,
            ) -> BoxFuture<'a, crate::core::Result<()>> {
                Box::pin(async move {
                    app.routes(|routes| routes.get("/users/{id}", || async { "ok" }))?;
                    Ok(())
                })
            }
        }

        let mut config = FrameworkConfig::default();
        let mut disabled = ProviderBuilder::new();
        disabled.provider(crate::http::HttpServiceProvider);
        let disabled = crate::foundation::Application::from_provider_runtime(
            disabled
                .start(Figment::from(Serialized::defaults(config.clone())))
                .await?,
        )?;
        let response = build_router(&disabled)?
            .oneshot(
                Request::get("/metrics")
                    .header(header::ACCEPT, "application/json")
                    .body(Body::empty())
                    .map_err(crate::core::Error::internal)?,
            )
            .await
            .map_err(crate::core::Error::internal)?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );

        config.server.metrics_endpoint_enabled = true;
        let mut builder = ProviderBuilder::new();
        builder
            .provider(crate::http::HttpServiceProvider)
            .provider(MetricsRoutes);
        let app = crate::foundation::Application::from_provider_runtime(
            builder
                .start(Figment::from(Serialized::defaults(config)))
                .await?,
        )?;
        let router = build_router(&app)?;
        let request = Request::get("/users/secret-user?token=secret-token")
            .body(Body::empty())
            .map_err(crate::core::Error::internal)?;
        let response = router
            .clone()
            .oneshot(request)
            .await
            .map_err(crate::core::Error::internal)?;
        assert_eq!(response.status(), StatusCode::OK);
        drop(response);

        let request = Request::get("/metrics")
            .body(Body::empty())
            .map_err(crate::core::Error::internal)?;
        let scrape = router
            .oneshot(request)
            .await
            .map_err(crate::core::Error::internal)?;
        assert_eq!(scrape.status(), StatusCode::OK);
        let body = axum::body::to_bytes(scrape.into_body(), 64 * 1024)
            .await
            .map_err(crate::core::Error::internal)?;
        let body = String::from_utf8(body.to_vec()).map_err(crate::core::Error::internal)?;
        assert!(body.contains("openrust_http_requests_total"));
        assert!(!body.contains("secret-user"));
        assert!(!body.contains("secret-token"));
        assert!(!body.contains("/users/"));
        Ok(())
    }
}
