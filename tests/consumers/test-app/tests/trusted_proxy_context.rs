//! 独立应用只用公开配置和请求 API 消费可信代理上下文。

use figment::{Figment, providers::Serialized};
use openrust::config::{FrameworkConfig, NoConfig};
use openrust::core::{Application, BootContext, BoxFuture, Result, ServiceProvider};
use openrust::http::{
    HttpServiceProvider, RequestInfo, TrustedProxyHeaders,
    axum::{
        Json,
        body::Body,
        extract::connect_info::ConnectInfo,
        http::{Request, StatusCode},
    },
};
use openrust::testing::TestApp;
use std::{net::SocketAddr, time::Duration};

struct ProxyRoutes;

impl ServiceProvider for ProxyRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes.get("/context", |request: RequestInfo| async move {
                    Ok::<_, openrust::Error>(Json(serde_json::json!({
                        "client_ip": request.client_ip()?,
                        "client_ips": request.client_ips()?,
                        "base_url": request.base_url()?,
                        "scheme": request.scheme()?,
                        "host": request.host()?,
                        "port": request.port()?,
                        "root": request.root()?,
                        "full_url": request.full_url()?,
                    })))
                })
            })?;
            Ok(())
        })
    }
}

struct ProxyApplication;

impl Application for ProxyApplication {
    type Config = NoConfig;

    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(HttpServiceProvider), Box::new(ProxyRoutes)]
    }
}

// 发现公开策略没有接入 RequestInfo，或非可信 peer 的伪造头进入相同快照。
#[tokio::test]
async fn public_proxy_configuration_controls_one_request_snapshot()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut config = FrameworkConfig::default();
    config.server.trusted_proxy_ranges = Some(vec!["10.0.0.0/8".to_owned()]);
    config.server.trusted_proxy_headers = TrustedProxyHeaders::FOR
        | TrustedProxyHeaders::HOST
        | TrustedProxyHeaders::PORT
        | TrustedProxyHeaders::PROTO
        | TrustedProxyHeaders::PREFIX;
    assert_eq!(
        config.server.trusted_proxy_headers,
        TrustedProxyHeaders::DEFAULT
    );
    let mut app = TestApp::<ProxyApplication>::builder(Figment::from(Serialized::defaults(config)))
        .build()
        .await?;
    for (peer, trusted) in [("10.0.0.2:42000", true), ("192.0.2.7:42000", false)] {
        let response = app
            .request(
                Request::get("/context")
                    .header("host", "internal.test:8080")
                    .header("x-forwarded-for", "198.51.100.8")
                    .header("x-forwarded-host", "public.test")
                    .header("x-forwarded-port", "8443")
                    .header("x-forwarded-proto", "https")
                    .header("x-forwarded-prefix", "/gateway")
                    .extension(ConnectInfo(peer.parse::<SocketAddr>()?))
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?;
        let json: serde_json::Value = serde_json::from_slice(response.body())?;
        let data = &json;
        let expected_ip = if trusted { "198.51.100.8" } else { "192.0.2.7" };
        assert_eq!(data["client_ip"], expected_ip);
        assert_eq!(data["client_ips"], serde_json::json!([expected_ip]));
        assert_eq!(data["base_url"], if trusted { "/gateway" } else { "" });
        assert_eq!(data["scheme"], if trusted { "https" } else { "http" });
        assert_eq!(
            data["host"],
            if trusted {
                "public.test"
            } else {
                "internal.test"
            }
        );
        assert_eq!(data["port"], if trusted { 8443 } else { 8080 });
        let root = if trusted {
            "https://public.test:8443/gateway"
        } else {
            "http://internal.test:8080"
        };
        assert_eq!(data["root"], root);
        assert_eq!(data["full_url"], format!("{root}/context"));
    }
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}
