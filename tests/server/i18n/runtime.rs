//! 通过聚合 HTTP 应用验证国际化行为。

use std::collections::BTreeMap;

use openrust::ServiceExt;
use openrust::config::FrameworkConfig;
use openrust::http::axum::body::{Body, to_bytes};
use openrust::http::axum::http::{Request, StatusCode};
use openrust::http::axum::response::{IntoResponse, Response};
use openrust::http::routing::get;
use openrust::http::{
    HttpServiceProvider, Inject, Json, RequestLocale, Routes, Validated, build_router,
};
use openrust::i18n::{I18n, I18nConfig, I18nServiceProvider, Locale, MessageArgs, MessageCatalog};
use openrust::validation::{Rule, Rules, Validate};
use openrust::{Error, Result};
use serde::Deserialize;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn request_locale(locale: RequestLocale) -> Json<String> {
    Json(locale.as_str().to_owned())
}

async fn app_greeting(locale: RequestLocale, Inject(i18n): Inject<I18n>) -> Json<String> {
    let args = MessageArgs::from([("name".to_owned(), "Ada".to_owned())]);
    Json(locale.translate(&i18n, "greeting", &args))
}

async fn plural_message(locale: RequestLocale, Inject(i18n): Inject<I18n>) -> Json<String> {
    let args = BTreeMap::from([("count".to_owned(), "2".to_owned())]);
    Json(locale.translate(&i18n, "items", &args))
}

async fn unauthorized() -> Result<Json<&'static str>> {
    Err(Error::Unauthorized)
}

async fn custom_error() -> Result<Json<&'static str>> {
    Err(Error::custom("quota_exceeded", "limit reached"))
}

#[derive(Deserialize, serde::Serialize)]
struct ValidatedInput {
    email: String,
}
impl Validate for ValidatedInput {
    fn rules() -> Rules {
        Rules::new().field("email", [Rule::required(), Rule::string(), Rule::email()])
    }
}

async fn validated(Validated(Json(input)): Validated<Json<ValidatedInput>>) -> Json<String> {
    Json(input.email)
}

async fn explicit_locale(Inject(i18n): Inject<I18n>) -> Result<Response> {
    let locale = Locale::new("zh-CN")?;
    Ok(i18n
        .with_locale(locale, async { Json("forced").into_response() })
        .await)
}

fn i18n_routes(routes: Routes) -> Routes {
    routes
        .route("/greeting", get(app_greeting))
        .route("/plural", get(plural_message))
        .route("/locale", get(request_locale))
        .route("/unauthorized", get(unauthorized))
        .route("/custom-error", get(custom_error))
        .route("/validated", get(validated))
        .route("/explicit", get(explicit_locale))
}

async fn router() -> Result<openrust::http::axum::Router> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    let mut catalog = MessageCatalog::new();
    catalog.insert_plural(
        "en",
        "items",
        "count",
        BTreeMap::from([
            (
                openrust::i18n::PluralCategory::One,
                "{count} item".to_owned(),
            ),
            (
                openrust::i18n::PluralCategory::Other,
                "{count} items".to_owned(),
            ),
        ]),
    )?;
    catalog.insert_plural(
        "pl",
        "items",
        "count",
        BTreeMap::from([
            (
                openrust::i18n::PluralCategory::One,
                "{count} plik".to_owned(),
            ),
            (
                openrust::i18n::PluralCategory::Few,
                "{count} pliki".to_owned(),
            ),
            (
                openrust::i18n::PluralCategory::Many,
                "{count} plików".to_owned(),
            ),
            (
                openrust::i18n::PluralCategory::Other,
                "{count} pliku".to_owned(),
            ),
        ]),
    )?;
    builder
        .provider(HttpServiceProvider)
        .provider(I18nServiceProvider::new(catalog))
        .provider(TestRoutes::new(i18n_routes));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    build_router(&app)
}

#[tokio::test]
async fn aggregate_server_selects_plural_forms_per_request_locale() -> TestResult {
    let router = router().await?;
    let english = router.clone().oneshot(
        Request::get("/plural")
            .header("accept-language", "en")
            .body(Body::empty())?,
    );
    let polish = router.clone().oneshot(
        Request::get("/plural")
            .header("accept-language", "pl")
            .body(Body::empty())?,
    );
    let (english_response, polish_response) = tokio::join!(english, polish);
    let english_body = to_bytes(english_response?.into_body(), usize::MAX).await?;
    let polish_body = to_bytes(polish_response?.into_body(), usize::MAX).await?;
    let english_json: serde_json::Value = serde_json::from_slice(&english_body)?;
    let polish_json: serde_json::Value = serde_json::from_slice(&polish_body)?;
    assert_eq!(english_json["data"], "2 items");
    assert_eq!(polish_json["data"], "2 pliki");
    Ok(())
}

#[tokio::test]
async fn aggregate_server_localizes_errors_and_scopes_concurrent_requests() -> TestResult {
    let router = router().await?;
    let english = router.clone().oneshot(
        Request::get("/unauthorized")
            .header("accept-language", "en")
            .body(Body::empty())?,
    );
    let chinese = router.clone().oneshot(
        Request::get("/unauthorized")
            .header("accept-language", "zh-CN, en;q=0.3")
            .body(Body::empty())?,
    );
    let (english_response, chinese_response) = tokio::join!(english, chinese);
    let english_response = english_response?;
    let chinese_response = chinese_response?;
    assert_eq!(english_response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(chinese_response.status(), StatusCode::UNAUTHORIZED);
    let english_body = to_bytes(english_response.into_body(), usize::MAX).await?;
    let chinese_body = to_bytes(chinese_response.into_body(), usize::MAX).await?;
    let english_json: serde_json::Value = serde_json::from_slice(&english_body)?;
    let chinese_json: serde_json::Value = serde_json::from_slice(&chinese_body)?;
    assert_eq!(english_json["code"], "unauthorized");
    assert_eq!(english_json["message"], "authentication required");
    assert_eq!(chinese_json["code"], "unauthorized");
    assert_eq!(chinese_json["message"], "需要身份验证");
    Ok(())
}

#[tokio::test]
async fn aggregate_server_exposes_request_locale_and_explicit_override() -> TestResult {
    let router = router().await?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/locale")
                .header("accept-language", "fr, zh;q=0.8, en;q=0.2")
                .body(Body::empty())?,
        )
        .await?;
    let locale_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(locale_json["data"], "zh-CN");

    let response = router
        .oneshot(
            Request::get("/explicit")
                .header("accept-language", "en")
                .body(Body::empty())?,
        )
        .await?;
    let explicit_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(explicit_json["message"], "成功");
    assert_eq!(explicit_json["data"], "forced");
    Ok(())
}

#[tokio::test]
async fn validator_rule_messages_localize_without_changing_code_or_field_path() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::get("/validated")
                .header("accept-language", "zh-CN")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"email":"invalid"}"#))?,
        )
        .await?;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["code"], "validation_failed");
    assert_eq!(body["errors"]["email"][0], "必须是有效的电子邮件地址");
    Ok(())
}

#[tokio::test]
async fn untranslated_application_error_keeps_its_safe_message() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::get("/custom-error")
                .header("accept-language", "zh-CN")
                .body(Body::empty())?,
        )
        .await?;
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["code"], "quota_exceeded");
    assert_eq!(body["message"], "limit reached");
    Ok(())
}

#[tokio::test]
async fn app_catalog_can_override_builtin_messages_with_parameters() -> TestResult {
    let mut catalog = MessageCatalog::new();
    catalog.merge_json("en", r#"{"greeting":"Hello, {name}!"}"#)?;
    let plugin = I18nServiceProvider::new(catalog);
    let app = I18n::new(I18nConfig::default(), {
        let mut messages = MessageCatalog::new();
        messages.merge_json("en", r#"{"greeting":"Hello, {name}!"}"#)?;
        messages
    })?;
    let mut args = MessageArgs::new();
    args.insert("name".to_owned(), "Ada".to_owned());
    assert_eq!(
        app.translate(&Locale::new("en")?, "greeting", &args),
        "Hello, Ada!"
    );

    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(HttpServiceProvider).provider(plugin);
    builder.provider(TestRoutes::new(i18n_routes));
    let assembled = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    assert!(assembled.state().resolve::<I18n>().await.is_ok());
    let response = build_router(&assembled)?
        .oneshot(
            Request::get("/greeting")
                .header("accept-language", "en-US")
                .body(Body::empty())?,
        )
        .await?;
    let response_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(response_json["data"], "Hello, Ada!");
    assert_eq!(
        app.translate(&Locale::new("en")?, "unknown", &BTreeMap::new()),
        "unknown"
    );
    Ok(())
}

struct TestRoutes<F>(std::sync::Arc<F>);
impl<F: Fn(openrust::http::Routes) -> openrust::http::Routes> TestRoutes<F> {
    fn new(routes: F) -> Self {
        Self(std::sync::Arc::new(routes))
    }
}
impl<F> openrust::core::ServiceProvider for TestRoutes<F>
where
    F: Fn(openrust::http::Routes) -> openrust::http::Routes + Send + Sync + 'static,
{
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        let routes = std::sync::Arc::clone(&self.0);
        Box::pin(async move {
            app.routes(move |value| routes(value))?;
            Ok(())
        })
    }
}
