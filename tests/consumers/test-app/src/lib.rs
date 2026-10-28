//! 独立应用通过公开 TestApp API 替换服务并测试 Provider 移除。

#[cfg(test)]
mod tests {
    use figment::{Figment, providers::Serialized};
    use openrust::config::{FrameworkConfig, NoConfig};
    use openrust::core::{
        Application, BootContext, BoxFuture, RegisterContext, Result, ServiceProvider,
        provider::ProviderId,
    };
    use openrust::http::{
        HttpServiceProvider,
        axum::{
            Json,
            body::Body,
            extract::State,
            http::{Request, StatusCode},
        },
    };
    use openrust::testing::TestApp;
    use std::{sync::Arc, time::Duration};

    trait Greeting: Send + Sync {
        fn message(&self) -> &'static str;
    }
    struct FixedGreeting(&'static str);
    impl Greeting for FixedGreeting {
        fn message(&self) -> &'static str {
            self.0
        }
    }
    struct BusinessProvider;
    impl ServiceProvider for BusinessProvider {
        fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
            app.instance::<dyn Greeting>(Arc::new(FixedGreeting("production")));
            Ok(())
        }
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.routes(|routes| {
                    routes.get(
                        "/api/greeting",
                        |State(state): State<openrust::core::AppState>| async move {
                            let value = state.resolve::<dyn Greeting>().await?;
                            Ok::<_, openrust::Error>(Json(
                                serde_json::json!({"message": value.message()}),
                            ))
                        },
                    )
                })?;
                Ok(())
            })
        }
    }
    struct Example;
    impl Application for Example {
        type Config = NoConfig;
        fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
            vec![Box::new(HttpServiceProvider), Box::new(BusinessProvider)]
        }
    }
    fn source() -> Figment {
        let mut config = FrameworkConfig::default();
        // 本用例断言实际 HTTP 404，显式选择语义化状态码。
        config.api.http_status = openrust::config::HttpStatusMode::Semantic;
        Figment::from(Serialized::defaults(config))
    }
    fn request(
        path: &str,
    ) -> std::result::Result<Request<Body>, openrust::http::axum::http::Error> {
        Request::builder().uri(path).body(Body::empty())
    }
    #[tokio::test]
    async fn override_is_isolated_and_provider_removal_preserves_health()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut first_builder = TestApp::<Example>::builder(source());
        first_builder.configure_after_register(|bindings| {
            bindings.instance::<dyn Greeting>(Arc::new(FixedGreeting("first")));
            Ok(())
        });
        let mut first = first_builder.build().await?;
        let mut second_builder = TestApp::<Example>::builder(source());
        second_builder.configure_after_register(|bindings| {
            bindings.instance::<dyn Greeting>(Arc::new(FixedGreeting("second")));
            Ok(())
        });
        let mut second = second_builder.build().await?;
        for (app, message) in [(&first, "first"), (&second, "second")] {
            app.request(request("/api/greeting")?)
                .await?
                .assert_status(StatusCode::OK)?
                .assert_header("content-type", "application/json")?
                .assert_json(&serde_json::json!({"message": message}))?;
        }
        let mut removed_builder = TestApp::<Example>::builder(source());
        removed_builder.remove_provider(ProviderId::of::<BusinessProvider>());
        let mut removed = removed_builder.build().await?;
        removed
            .request(request("/api/greeting")?)
            .await?
            .assert_status(StatusCode::NOT_FOUND)?;
        removed
            .request(request("/health")?)
            .await?
            .assert_status(StatusCode::OK)?;
        let deadline = openrust::tokio::time::Instant::now() + Duration::from_secs(5);
        first.shutdown(deadline).await?;
        second.shutdown(deadline).await?;
        removed.shutdown(deadline).await?;
        Ok(())
    }
}
