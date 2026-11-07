//! 验证生成的认证路由端到端行为。

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
{{grpc_module}}
#[cfg(feature = "auth-fortify")]
#[path = "support/fortify_http.rs"]
mod fortify_http;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "support/request_cookie.rs"]
mod request_cookie;
#[path = "support/request_input.rs"]
mod request_input;
#[path = "support/request_session.rs"]
mod request_session;
#[path = "../routes/mod.rs"]
mod routes;
#[path = "../database/seeders.rs"]
mod seed;

use bootstrap::OpenRustGeneratedApplication as GeneratedApplication;

use std::error::Error as StdError;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::http::axum::http::StatusCode;
use openrust::mail::{MailDriver, Mailer};
use openrust::prelude::*;
use testing::TestDatabase;

type TestResult<T = ()> = std::result::Result<T, Box<dyn StdError>>;

fn test_configuration() -> openrust::config::FrameworkConfig {
    let mut config = openrust::config::FrameworkConfig::default();
    config.app.env = openrust::config::Environment::Testing;
    config.app.key = Some("0123456789abcdef0123456789abcdef".into());
    config
}

// 测试覆盖与真实启动共用代码配置；不能遗漏应用声明的具名账户 Provider。
fn application_test_source(overrides: Figment) -> TestResult<openrust::config::ConfigSources> {
    let entries = overrides.extract::<std::collections::BTreeMap<String, serde_json::Value>>()?;
    Ok(
        openrust::config::ConfigLoader::application_code_sources_with_overrides(
            ".",
            GeneratedApplication::config_namespace(),
            Some("testing"),
            &GeneratedApplication::config(),
            entries,
        )?,
    )
}

#[derive(Clone, Default)]
struct CaptureMailer {
    messages: Arc<Mutex<Vec<String>>>,
}

impl CaptureMailer {
    fn latest_code(&self) -> std::result::Result<String, Box<dyn StdError>> {
        let messages = self
            .messages
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let message = messages.last().ok_or("expected a generated email")?;
        let tail = message
            .split("Your OpenRust verification code is: ")
            .nth(1)
            .ok_or("generated email did not contain a verification code")?;
        let code = tail
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>();
        if code.len() != 6 {
            return Err("generated email did not contain a six-digit code".into());
        }
        Ok(code)
    }
}

impl MailDriver for CaptureMailer {
    fn send(
        &self,
        message: openrust::mail::lettre::Message,
    ) -> BoxFuture<'_, openrust::Result<()>> {
        Box::pin(async move {
            let formatted = String::from_utf8_lossy(&message.formatted()).into_owned();
            self.messages
                .lock()
                .map_err(|error| {
                    openrust::Error::internal(std::io::Error::other(error.to_string()))
                })?
                .push(formatted);
            Ok(())
        })
    }
}

/// TestApp 使用相同 Provider 清单，覆盖 Mailer 后验证真实认证路由。
#[tokio::test]
async fn generated_test_app_overrides_mailer_for_registration() -> TestResult {
    use openrust::http::axum::{body::Body, http::Request};
    {{auth_testapp_database_setup}}

    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x42_u8; 32]);
    let link_key =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0x24_u8; 32]);
    let config = format!(
        "[session]\ndriver = 'database'\n[database]\nurl = {:?}\n[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n[auth.jwt]\nissuer = 'http://openrust.test'\naudience = 'generated-auth-tests'\nactive_key_id = 'test'\n[auth.jwt.keys]\ntest = {:?}\n[security.signed_links]\nissuer = 'http://openrust.test'\nactive_key_id = 'test-link'\n[security.signed_links.keys]\ntest-link = {:?}\n{{encryption_config}}
\n[view]\ndirectory = 'resources/views'\n",
        database.url(),
        key,
        link_key
    );
    let source =
        Figment::from(Serialized::defaults(test_configuration())).merge(Toml::string(&config));
    let source = application_test_source(source)?;
    let mail = CaptureMailer::default();
    let captured = mail.clone();
    let input_source = source.figment().clone();
    let mut builder = openrust::testing::TestApp::<GeneratedApplication>::builder(source);
    builder.configure_after_register(move |bindings| {
        let captured = captured.clone();
        Box::pin(async move {
            bindings.instance(Arc::new(Mailer::new(captured))).await?;
            Ok(())
        })
    });
    let mut app = builder.build().await?;
    let result = bounded_auth_business(async {
        #[cfg(feature = "auth-fortify")]
        fortify_http::exercise::<GeneratedApplication>(input_source.clone()).await?;
        request_input::assert_request_input::<GeneratedApplication>(input_source.clone()).await?;
        #[cfg(feature = "encryption")]
        let check_cookie_wire = assert_default_cookie_wire;
        #[cfg(not(feature = "encryption"))]
        let check_cookie_wire = |_| Ok(());
        request_cookie::assert_request_cookie::<GeneratedApplication>(
            input_source.clone(),
            true,
            check_cookie_wire,
        )
        .await?;
        request_session::assert_request_session::<GeneratedApplication>(input_source, true).await?;
        let payload =
            serde_json::json!({"email": "testapp@example.test", "password": "initial-password"});
        let request = Request::builder()
            .method("POST")
            .uri("/api/auth/register")
            .header("content-type", "application/json")
            .extension(openrust::http::axum::extract::ConnectInfo(
                SocketAddr::from(([127, 0, 0, 1], 12345)),
            ))
            .body(Body::from(serde_json::to_vec(&payload)?))?;
        app.request(request).await?.assert_status(StatusCode::OK)?;
        assert_eq!(mail.latest_code()?.len(), 6);
        assert_generated_named_user_provider(
            app.app()?,
            "testapp@example.test",
            "initial-password",
        )
        .await?;
        Ok(())
    })
    .await;
    let app_result = app
        .shutdown(openrust::tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    drop(app);
    let database_result =
        openrust::tokio::time::timeout(std::time::Duration::from_secs(10), database.close()).await;
    result?;
    app_result?;
    database_result??;
    Ok(())
}

/// 解析生成应用实际注册的工厂，避免JWT成功掩盖默认账户Provider失联。
async fn assert_generated_named_user_provider(
    app: &openrust::Application,
    email: &str,
    plain: &str,
) -> TestResult {
    use openrust::auth::{AuthManager, Credentials};
    use openrust::database::sea_orm::{ConnectionTrait, DbBackend, Statement};

    let scope = app.state().container().scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    assert!(Arc::ptr_eq(
        &manager,
        &scope.resolve::<AuthManager>().await?
    ));
    assert_eq!(manager.default_driver()?, "web");
    // Session Guard需要HTTP请求槽；这里仅解析同scope生产注册的账户Provider。
    let provider = manager
        .create_user_provider(Some("users"))
        .await?
        .ok_or("生成应用未注册默认users Provider")?;
    // 配置故障沿生产工厂返回500；未知Provider名的None语义仍独立保留。
    let repository = app.config_repository();
    let providers = repository.section::<serde_json::Value>("auth.providers")?;
    repository.set(
        "auth.providers.users",
        serde_json::json!({"driver":"app-users"}),
    )?;
    let misconfigured = manager.create_user_provider(Some("users")).await;
    repository.set("auth.providers", providers)?;
    let Err(error) = misconfigured else {
        return Err("缺失table的应用Provider意外成功".into());
    };
    assert_eq!(error.code(), "app_user_table_required");
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let credentials = Credentials::new(serde_json::Map::from_iter([
        ("email".into(), serde_json::json!(email)),
        ("password".into(), serde_json::json!(plain)),
    ]));
    let user = provider
        .retrieve_by_credentials(&credentials)
        .await?
        .ok_or("生成应用Provider无法检索真实注册账户")?;
    let identifier = user.auth_identifier()?;
    let numeric_id = identifier
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or("真实数据库数字主键被账户Provider改变类型")?;
    assert_eq!(user.auth_identifier_name(), "id");
    assert_eq!(user.auth_password_name(), "password_hash");
    assert!(user.auth_password()?.is_some_and(|hash| !hash.is_empty()));
    assert!(
        provider
            .validate_credentials(user.as_ref(), &credentials)
            .await?
    );
    let wrong = Credentials::new(serde_json::Map::from_iter([
        ("email".into(), serde_json::json!(email)),
        (
            "password".into(),
            serde_json::json!("incorrect-generated-password"),
        ),
    ]));
    assert!(!provider.validate_credentials(user.as_ref(), &wrong).await?);
    let restored = provider
        .retrieve_by_id(&identifier)
        .await?
        .ok_or("生成应用Provider未保留数字账户标识")?;
    assert_eq!(restored.auth_identifier()?, identifier);
    assert_eq!(restored.auth_password_name(), "password_hash");

    let token = "generated-provider-remember-probe";
    assert_eq!(user.remember_token_name(), "remember_token");
    provider.update_remember_token(user.as_ref(), token).await?;
    let remembered = provider
        .retrieve_by_token(&identifier, token)
        .await?
        .ok_or("生成应用Provider未持久化记住我token")?;
    assert_eq!(remembered.auth_identifier()?, identifier);

    let db = scope.resolve::<openrust::database::Db>().await?;
    let table = db.quoted_table_name("users")?;
    let placeholder = match db.backend()? {
        DbBackend::Postgres => "$1",
        DbBackend::MySql | DbBackend::Sqlite => "?",
        _ => return Err("生成认证测试不支持当前数据库后端".into()),
    };
    // 只有表名进入SQL文本，并复用Db标识符校验；账户主键始终绑定。
    db.connection()?
        .execute_raw(Statement::from_sql_and_values(
            db.backend()?,
            format!("UPDATE {table} SET disabled_at=CURRENT_TIMESTAMP WHERE id={placeholder}"),
            [numeric_id.into()],
        ))
        .await?;
    assert!(matches!(
        provider.retrieve_by_id(&identifier).await,
        Err(openrust::Error::Unauthorized)
    ));
    assert!(matches!(
        provider.retrieve_by_credentials(&credentials).await,
        Err(openrust::Error::Unauthorized)
    ));
    assert!(matches!(
        provider.retrieve_by_token(&identifier, token).await,
        Err(openrust::Error::Unauthorized)
    ));
    Ok(())
}

#[cfg(feature = "encryption")]
fn assert_default_cookie_wire(pairs: &[String]) -> openrust::Result<()> {
    for pair in pairs {
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let error = || {
            openrust::Error::custom(
                "generated_cookie_payload_mismatch",
                "生成 Web 应用未发出默认 Laravel CBC Cookie",
            )
        };
        let (_, wire) = pair.split_once('=').ok_or_else(error)?;
        // Cookie 编码仅转义 Base64 字母表中的三个符号。
        let wire = wire
            .replace("%2B", "+")
            .replace("%2F", "/")
            .replace("%3D", "=");
        let json = STANDARD.decode(wire).map_err(|_| error())?;
        let payload: serde_json::Value = serde_json::from_slice(&json).map_err(|_| error())?;
        let iv = payload["iv"].as_str().ok_or_else(error)?;
        let value = payload["value"].as_str().ok_or_else(error)?;
        let mac = payload["mac"].as_str().ok_or_else(error)?;
        if STANDARD.decode(iv).map_err(|_| error())?.len() != 16
            || STANDARD.decode(value).map_err(|_| error())?.is_empty()
            || mac.len() != 64
            || !mac.bytes().all(|byte| byte.is_ascii_hexdigit())
            || payload["tag"] != ""
        {
            return Err(error());
        }
    }
    Ok(())
}

/// 捕获每次 poll 中的业务 panic；取消和超时先释放借用，再由调用方统一回收资源。
async fn bounded_auth_business(
    future: impl std::future::Future<Output = TestResult>,
) -> TestResult {
    let mut future = std::pin::pin!(future);
    let caught = std::future::poll_fn(|context| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            future.as_mut().poll(context)
        })) {
            Ok(result) => result,
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("业务验收 panic（非字符串 payload）");
                std::task::Poll::Ready(Err(std::io::Error::other(message.to_owned()).into()))
            }
        }
    });
    openrust::tokio::time::timeout(std::time::Duration::from_secs(120), caught).await?
}
