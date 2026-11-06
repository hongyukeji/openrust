//! 验证生成的 JWT 应用路由，并为独立 Web 协议用例显式安装测试 Provider。
{{seed_module}}
#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
{{grpc_module}}#[path = "support/message_bags.rs"]
mod message_bags;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "support/request_cookie.rs"]
mod request_cookie;
#[path = "support/request_input.rs"]
mod request_input;
#[path = "support/request_redirect.rs"]
mod request_redirect;
#[path = "support/request_session.rs"]
mod request_session;
#[path = "../routes/mod.rs"]
mod routes;

use bootstrap::OpenRustGeneratedApplication as GeneratedApplication;

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::config::ConfigSources;
use openrust::core::{Application, BoxFuture, RegisterContext, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::TestApp;
use std::time::Duration;

// 通用 Web 协议用例显式安装自己的 Provider，生产 JWT 宿主继续用于 API 协议用例。
struct TestWebHost<const SESSION: bool>;

impl<const SESSION: bool> Application for TestWebHost<SESSION> {
    type Config = <GeneratedApplication as Application>::Config;

    fn config() -> openrust::config::ConfigManifest {
        GeneratedApplication::config()
    }

    fn config_namespace() -> &'static str {
        GeneratedApplication::config_namespace()
    }

    fn default_providers() -> openrust::core::DefaultProviders {
        let mut extra: Vec<Box<dyn ServiceProvider>> =
            vec![Box::new(openrust::http::CookieServiceProvider::new())];
        if SESSION {
            extra.push(Box::new(openrust::session::SessionServiceProvider::new()));
        }
        GeneratedApplication::default_providers().merge(extra)
    }

    fn package_providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        GeneratedApplication::package_providers(config)
    }

    fn additional_providers() -> Vec<Box<dyn ServiceProvider>> {
        GeneratedApplication::additional_providers()
    }

    fn with_bootstrap_providers() -> bool {
        GeneratedApplication::with_bootstrap_providers()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        GeneratedApplication::providers(config)
    }

    fn configure_providers(config: &Self::Config, builder: &mut openrust::core::ProviderBuilder) {
        GeneratedApplication::configure_providers(config, builder);
    }
}

fn test_configuration() -> openrust::config::FrameworkConfig {
    let mut config = openrust::config::FrameworkConfig::default();
    config.app.env = openrust::config::Environment::Testing;
    config
}

// 生成应用在公开 server 配置上按精确环境启用主机信任列表。
#[tokio::test]
async fn generated_application_applies_trusted_hosts() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = test_configuration();
    config.app.env = openrust::config::Environment::Development;
    config.server.trusted_hosts = Some(openrust::config::TrustedHostsConfig::default());
    let source = jwt_profile_source(Figment::from(Serialized::defaults(config)).merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    )));
    let mut app = TestApp::<GeneratedApplication>::builder(source)
        .build()
        .await?;
    app.request(
        Request::get("/api")
            .header("host", "localhost")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    app.request(
        Request::get("/api")
            .header("host", "attacker.test")
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::BAD_REQUEST)?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

struct RuntimeGreeting(&'static str);

struct RuntimeGreetingProvider(&'static str);

impl ServiceProvider for RuntimeGreetingProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::new(RuntimeGreeting(self.0)))
                .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn generated_application_exposes_runtime_provider_registration()
-> Result<(), Box<dyn std::error::Error>> {
    let source = jwt_profile_source(Figment::from(Serialized::defaults(test_configuration()))
        .merge(Toml::string("[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n")));
    let configuration: bootstrap::Configuration = source.extract()?;
    let builder =
        openrust::core::ProviderBuilder::for_application::<GeneratedApplication>(&configuration);
    let mut app = openrust::App::from_provider_runtime(builder.start(source).await?)?;
    assert_eq!(app.environment().as_str(), "testing");
    assert!(app.running_unit_tests());
    assert!(!app.is_production());
    let first = app
        .register_provider(RuntimeGreetingProvider("初次登记"))
        .await?;
    let old = app.state().resolve::<RuntimeGreeting>().await?;
    assert_eq!(old.0, "初次登记");
    assert_eq!(
        app.register_provider(RuntimeGreetingProvider("忽略重复"))
            .await?,
        first
    );
    assert_eq!(
        app.state().resolve::<RuntimeGreeting>().await?.0,
        "初次登记"
    );
    assert_eq!(
        app.force_register_provider(RuntimeGreetingProvider("强制更新"))
            .await?,
        first
    );
    assert_eq!(old.0, "初次登记");
    assert_eq!(
        app.state().resolve::<RuntimeGreeting>().await?.0,
        "强制更新"
    );
    app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_serves_api_and_health() -> Result<(), Box<dyn std::error::Error>> {
    let source = jwt_profile_source(Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[app]\nname = 'configured-application'\n[server]\nroot_target = 'view:index.html'\n[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    )));
    let root: serde_json::Value = source.extract()?;
    let mut entries = root
        .as_object()
        .ok_or("生成配置根必须是对象")?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    entries.push((
        "template_order".to_owned(),
        serde_json::json!({"z_label": "先声明", "a_label": "后声明"}),
    ));
    let sources = ConfigSources::from_ordered_entries(entries)?;
    let mut app = TestApp::<GeneratedApplication>::builder(sources)
        .build()
        .await?;
    let keys = app.app()?.config_repository().all()?["template_order"]
        .as_object()
        .ok_or("有序测试区段必须是对象")?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(keys, ["z_label", "a_label"]);
    let request = Request::builder().uri("/api").body(Body::empty())?;
    app.request(request)
        .await?
        .assert_status(StatusCode::OK)?
        .assert_json(&serde_json::json!({
            "success": true,
            "code": "ok",
            "message": "ok",
            "data": {"name": "configured-application"}
        }))?;
    let startup_name = app.app()?.config().app.name.clone();
    app.app()?
        .config_repository()
        .set("app.name", serde_json::json!("运行期名称"))?;
    app.app()?
        .config_repository()
        .set("template_order.z_label", serde_json::json!("已复核"))?;
    let keys = app.app()?.config_repository().all()?["template_order"]
        .as_object()
        .ok_or("有序测试区段必须是对象")?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(keys, ["z_label", "a_label"]);
    assert_eq!(
        app.app()?.section::<serde_json::Value>("app")?["name"],
        "运行期名称"
    );
    assert_eq!(app.app()?.config().app.name, startup_name);
    let api = app
        .request(Request::get("/api").body(Body::empty())?)
        .await?;
    assert!(api.headers().get("set-cookie").is_none());
    api.assert_json(&serde_json::json!({
        "success": true,
        "code": "ok",
        "message": "ok",
        "data": {"name": "configured-application"}
    }))?;
    let health = Request::builder().uri("/health").body(Body::empty())?;
    app.request(health).await?.assert_status(StatusCode::OK)?;
    let page = app
        .request(Request::builder().uri("/").body(Body::empty())?)
        .await?;
    page.assert_status(StatusCode::OK)?;
    let html = std::str::from_utf8(page.body())?;
    assert!(html.contains("configured-application"));
    assert!(html.contains("<p id=\"default-error\"></p>"));
    assert!(html.contains("<p id=\"other-error\"></p>"));
    let missing = Request::builder().uri("/api/missing").body(Body::empty())?;
    let fallback = app.request(missing).await?;
    fallback.assert_status(StatusCode::NOT_FOUND)?;
    assert_eq!(fallback.body(), "未找到路由：/api/missing".as_bytes());
    app.shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_exposes_request_input_to_business_provider()
-> Result<(), Box<dyn std::error::Error>> {
    let source = jwt_profile_source(Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    )));
    request_input::assert_request_input::<GeneratedApplication>(source).await
}

#[tokio::test]
async fn generated_application_consumes_message_bags_through_business_provider()
-> Result<(), Box<dyn std::error::Error>> {
    let source = jwt_profile_source(Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    )));
    message_bags::assert_message_bags::<GeneratedApplication>(source).await
}

/// 复用公开 helper 验证概览路由覆盖/静态顺序、动作跳转、确认回跳及错误闪存。
#[tokio::test]
async fn generated_application_redirects_to_named_route_and_restores_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let source = jwt_profile_source(Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    )));
    request_redirect::assert_request_redirect::<TestWebHost<false>>(source).await
}

#[tokio::test]
async fn generated_application_exposes_cookie_contract() -> Result<(), Box<dyn std::error::Error>> {
    let source = jwt_profile_source(Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    )));
    request_cookie::assert_request_cookie::<TestWebHost<false>>(source, false).await?;
    Ok(())
}

#[tokio::test]
async fn generated_application_exposes_session_lifecycle() -> Result<(), Box<dyn std::error::Error>>
{
    let source = jwt_profile_source(Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    )));
    request_session::assert_request_session::<TestWebHost<false>>(source, false).await?;
    Ok(())
}

#[tokio::test]
async fn test_web_host_supports_explicit_cookie_session_without_database()
-> Result<(), Box<dyn std::error::Error>> {
    let source = jwt_profile_source(Figment::from(Serialized::defaults(
        test_configuration(),
    ))
    .merge(Toml::string(
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[view]\ndirectory = 'resources/views'\n",
    )));
    let source = source.merge(Serialized::defaults(
        serde_json::json!({"session":{"lottery":[1,1]}}),
    ));
    request_session::assert_original_host_cookie_session::<TestWebHost<true>>(source).await
}

// 这些通用Router测试仍使用真实API profile；只补足其独立Figment的显式认证配置。
fn jwt_profile_source(source: Figment) -> Figment {
    source.merge(Serialized::defaults(serde_json::json!({
        "auth":{"defaults":{"guard":"api","passwords":"users","provider":null},"guards":{"api":{"driver":"jwt-api","provider":"users"}},"providers":{"users":{"driver":"app-users","table":"users"}},"jwt":{"issuer":"http://localhost","audience":"generated-common-tests","active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}},"signed_links":{"issuer":"http://localhost","active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
        "fortify-api":{"guard":"api","prefix":"/auth"}
    })))
}
