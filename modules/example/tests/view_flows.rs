//! 真实 TCP 验证模块视图源、发布副本和安全失败；不依赖数据库。
#![cfg(feature = "view")]

use figment::{Figment, providers::Serialized};
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::http::axum;
use openrust::http::axum::http::StatusCode;
use openrust::support::{BoxFuture, ServiceProvider};
use openrust::testing::TestApp;
use std::{path::PathBuf, time::Duration};
use tokio::sync::oneshot;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const BUDGET: Duration = Duration::from_secs(5);
const PRIVATE: &str = "PRIVATE_TEMPLATE_SENTINEL";

struct ViewApplication;
impl ApplicationDefinition for ViewApplication {
    type Config = <crate::bootstrap::Server as ApplicationDefinition>::Config;

    fn config() -> openrust::config::ConfigManifest {
        crate::bootstrap::Server::config()
    }
    fn default_providers() -> openrust::support::DefaultProviders {
        crate::bootstrap::Server::default_providers()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = crate::bootstrap::Server::providers(config);
        providers.push(Box::new(ViewErrors));
        providers
    }
    fn configure_providers(
        config: &Self::Config,
        builder: &mut openrust::foundation::ProviderBuilder,
    ) {
        crate::bootstrap::Server::configure_providers(config, builder);
    }
}
struct ViewErrors;
impl ServiceProvider for ViewErrors {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.exceptions(|handler| {
                handler.api_envelope_when(|request, _| {
                    request.path() == "/api"
                        || request.path().starts_with("/api/")
                        || request.path() == "/example/pages/about"
                });
            })?;
            Ok(())
        })
    }
}

struct TcpServer {
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl TcpServer {
    async fn close(&mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut task) = self.task.take() {
            if let Ok(result) = tokio::time::timeout(BUDGET, &mut task).await {
                result??;
            } else {
                task.abort();
                let _ = tokio::time::timeout(BUDGET, task).await;
                return Err("view fixture server did not stop before deadline".into());
            }
        }
        Ok(())
    }
}
impl Drop for TcpServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
struct Fixture {
    app: TestApp<ViewApplication>,
    server: TcpServer,
    client: reqwest::Client,
    origin: String,
    source: PathBuf,
    published: PathBuf,
    directory: tempfile::TempDir,
}
struct Page {
    status: StatusCode,
    content_type: String,
    text: String,
}
impl Fixture {
    async fn start() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        crate::modules::example::write_discovery_fixture(directory.path())?;
        let module = directory.path().join("modules/example");
        let source = module.join("resources/views/pages/about.html");
        let published = directory
            .path()
            .join("resources/views/vendor/example/pages/about.html");
        let app_views = directory.path().join("resources/views");
        std::fs::create_dir_all(module.join("config"))?;
        std::fs::create_dir_all(module.join("lang"))?;
        std::fs::create_dir_all(source.parent().ok_or("source parent missing")?)?;
        std::fs::create_dir_all(published.parent().ok_or("published parent missing")?)?;
        std::fs::create_dir_all(app_views.join("pages"))?;
        std::fs::write(
            directory.path().join("modules_statuses.json"),
            r#"{"example":true}"#,
        )?;
        std::fs::write(
            &source,
            "<main>SOURCE {{ title }} {{ '<script>module-value</script>' }}</main>",
        )?;
        // 根视图同名文件不能替代 example:: 命名空间的缺失源。
        std::fs::write(
            app_views.join("pages/about.html"),
            "ROOT_VIEW_MUST_NOT_REPLACE_MODULE",
        )?;
        let mut framework = openrust::config::FrameworkConfig::default();
        framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let source_config = Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(serde_json::json!({
            "modules":{"activator":"file","activators":{"file":{"statuses-file":"modules_statuses.json"}}},
            "app":{"env":"test","key":"base64:KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="},"application_root":directory.path(),
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
            "view":{"directory":app_views,"auto_reload":true},
            "session":{"driver":"array","secure":false}
        })));
        let app = TestApp::<ViewApplication>::builder(crate::modules::example::test_sources(
            &(source_config),
        )?)
        .build()
        .await?;
        let router = openrust::foundation::http::build_router(app.app()?)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(BUDGET)
            .build()?;
        Ok(Self {
            app,
            server: TcpServer {
                stop: Some(stop),
                task: Some(task),
            },
            client,
            origin,
            source,
            published,
            directory,
        })
    }
    async fn page(&self) -> TestResult<Page> {
        self.page_accept("text/html").await
    }
    async fn page_accept(&self, accept: &str) -> TestResult<Page> {
        self.request_page("/example/pages/about", accept).await
    }
    async fn request_page(&self, path: &str, accept: &str) -> TestResult<Page> {
        let mut response = self
            .client
            .get(format!("{}{path}", self.origin))
            .header("accept", accept)
            .send()
            .await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > 16 * 1024 {
                return Err("view fixture response exceeded budget".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Page {
            status,
            content_type,
            text: String::from_utf8(bytes)?,
        })
    }
    async fn close(mut self) -> TestResult {
        let result = self
            .app
            .shutdown(tokio::time::Instant::now() + BUDGET)
            .await;
        self.server.close().await?;
        result?;
        Ok(())
    }
}
#[tokio::test]
async fn real_tcp_module_view_prefers_published_copy_and_falls_back_to_source() -> TestResult {
    let fixture = Fixture::start().await?;
    let source = fixture.page().await?;
    assert_eq!(source.status, StatusCode::OK);
    assert!(source.content_type.starts_with("text/html"));
    assert!(source.text.contains("SOURCE 关于示例应用"));
    assert!(source.text.contains("&lt;script&gt;"));
    assert!(!source.text.contains("<script>"));
    // 资源发布副本只在本测试临时应用根内创建，生产 namespace loader 决定优先级。
    std::fs::write(
        &fixture.published,
        "<main>PUBLISHED {{ title }} {{ '<script>published-value</script>' }}</main>",
    )?;
    let published = fixture.page().await?;
    assert_eq!(published.status, StatusCode::OK);
    assert!(published.text.contains("PUBLISHED 关于示例应用"));
    assert!(!published.text.contains("SOURCE"));
    assert!(published.text.contains("&lt;script&gt;"));
    assert!(!published.text.contains("<script>"));
    std::fs::remove_file(&fixture.published)?;
    let fallback = fixture.page().await?;
    assert_eq!(fallback.status, StatusCode::OK);
    assert!(fallback.text.contains("SOURCE 关于示例应用"));
    assert!(!fallback.text.contains("PUBLISHED"));
    fixture.close().await
}
#[tokio::test]
async fn real_tcp_invalid_published_or_missing_source_view_fails_without_disclosing_paths()
-> TestResult {
    let fixture = Fixture::start().await?;
    // 启用业务模块后，宿主的 API 边界和额外网页错误策略仍须保留。
    let api = fixture
        .request_page("/api/nope", "application/json")
        .await?;
    assert_eq!(api.status, StatusCode::NOT_FOUND);
    let api_body: serde_json::Value = serde_json::from_str(&api.text)?;
    assert_eq!(api_body["code"], "not_found");
    let outside = fixture.request_page("/apifoo", "application/json").await?;
    assert_eq!(outside.status, StatusCode::NOT_FOUND);
    let outside_body: serde_json::Value = serde_json::from_str(&outside.text)?;
    assert!(outside_body.get("code").is_none());
    assert!(outside_body["message"].is_string());
    std::fs::write(&fixture.published, format!("{PRIVATE} {{% if %}}"))?;
    let invalid = fixture.page_accept("application/json").await?;
    assert_safe_error(&fixture, &invalid)?;
    // 坏副本必须报错，不能静默回退到存在的源；删除两者后也不能借用根同名模板。
    std::fs::remove_file(&fixture.published)?;
    std::fs::remove_file(&fixture.source)?;
    let missing = fixture.page_accept("application/json").await?;
    assert_safe_error(&fixture, &missing)?;
    assert!(!missing.text.contains("ROOT_VIEW_MUST_NOT_REPLACE_MODULE"));
    fixture.close().await
}
fn assert_safe_error(fixture: &Fixture, response: &Page) -> TestResult {
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    let body: serde_json::Value = serde_json::from_str(&response.text)?;
    assert_eq!(body["code"], "internal_error");
    assert!(!response.text.contains(PRIVATE));
    assert!(
        !response
            .text
            .contains(&fixture.directory.path().to_string_lossy().into_owned())
    );
    assert!(!response.text.contains("SOURCE"));
    Ok(())
}
