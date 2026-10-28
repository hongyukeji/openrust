use openrust::http::AppBuilderHttpExt;
use openrust::http::{
    HttpPlugin,
    axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    },
};
use tower::ServiceExt;

#[tokio::test]
async fn module_registration_controls_only_its_business_routes()
-> Result<(), Box<dyn std::error::Error>> {
    for registered in [false, true] {
        let mut builder = openrust::AppBuilder::new(
            openrust::config::FrameworkConfig::default(),
            figment::Figment::new(),
        );
        builder
            .plugin(HttpPlugin)
            .routes(openrust::routes::framework_routes);
        if registered {
            builder.plugin(super::ExamplePlugin);
        }
        let app = openrust::lifecycle::finalize(builder).await?;
        let router = openrust::http::build_router(&app)?;
        for path in ["/health", "/api"] {
            let response = router
                .clone()
                .oneshot(Request::get(path).body(Body::empty())?)
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = router
            .oneshot(Request::get("/api/example").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let response: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(response["success"], registered);
        if registered {
            assert_eq!(response["data"]["message"], "Hello from app/example");
        } else {
            // 默认 AlwaysOk envelope 保持 HTTP 200，机器码仍明确没有此业务路由。
            assert_eq!(response["code"], "not_found");
        }
    }
    Ok(())
}
