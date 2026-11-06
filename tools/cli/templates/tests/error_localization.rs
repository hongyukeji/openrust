//! 用真实I18n Provider和公开异常渲染入口验证生成项目的错误语言协议。
//! 不复制翻译器，也不依赖认证、数据库、Cookie或生成应用的可选feature。

use figment::{Figment, providers::Serialized};
use openrust::Error;
use openrust::config::FrameworkConfig;
use openrust::config::HttpStatusMode;
use openrust::foundation::Application;
use openrust::foundation::ExceptionRequest;
use openrust::foundation::ProviderBuilder;
use openrust::core::ResponseContext;
use openrust::core::with_response_context;
use openrust::http::axum::body::to_bytes;
use openrust::http::axum::http::Method;
use openrust::http::axum::http::StatusCode;
use openrust::http::axum::http::header;
use openrust::translation::Translator;
use openrust::translation::TranslationServiceProvider;
use openrust::translation::Locale;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const SECRET: &str = "generated-error-private-source-sentinel";

#[derive(Clone, Copy)]
enum Format {
    Json,
    Html,
    Envelope,
}
const FORMATS: [Format; 3] = [Format::Json, Format::Html, Format::Envelope];

async fn application(default_locale: &str) -> TestResult<Application> {
    let source = Figment::from(Serialized::defaults(FrameworkConfig::default())).merge(
        Serialized::defaults(serde_json::json!({
            "app":{"env":"testing","debug":false},
            "i18n":{"default_locale":default_locale,"fallback_locale":"en"}
        })),
    );
    let mut builder = ProviderBuilder::new();
    builder.provider(TranslationServiceProvider::default());
    Ok(Application::from_provider_runtime(builder.start(source).await?)?)
}

async fn rendered(
    app: &Application,
    format: Format,
    error: Error,
    expected: &str,
    locale: &str,
) -> TestResult {
    let mut handler = app
        .exception_handler()
        .cloned()
        .ok_or("I18n Provider did not install ExceptionHandler")?;
    if matches!(format, Format::Envelope) {
        handler.api_envelope_when(|_, _| true);
    }
    let status = error.status();
    let code = error.code().to_owned();
    let request = ExceptionRequest::new(
        Method::GET,
        "/generated/error-contract",
        !matches!(format, Format::Html),
        "generated-error-localization",
        false,
    );
    let response = with_response_context(
        ResponseContext::new("generated-error-localization", HttpStatusMode::Semantic),
        async { handler.render_for_request(error, &request) },
    )
    .await;
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()[header::VARY], "Accept");
    let bytes = to_bytes(response.into_body(), 64 * 1024).await?;
    let body = String::from_utf8(bytes.to_vec())?;
    assert!(!body.contains(SECRET));
    match format {
        Format::Html => {
            assert!(body.contains(&format!("<html lang=\"{locale}\">")));
            assert!(body.contains(&format!("<p>{expected}</p>")));
        }
        Format::Json | Format::Envelope => {
            let json: serde_json::Value = serde_json::from_str(&body)?;
            assert_eq!(json["message"], expected);
            if matches!(format, Format::Envelope) {
                assert_eq!(json["code"], code);
                assert_eq!(json["request_id"], "generated-error-localization");
            }
        }
    }
    Ok(())
}

async fn verify_locale(app: &Application, locale: &str) -> TestResult {
    let (unauthorized, internal) = if locale == "zh-CN" {
        ("需要身份验证", "服务器内部错误")
    } else {
        ("authentication required", "internal server error")
    };
    for format in FORMATS {
        rendered(app, format, Error::Unauthorized, unauthorized, locale).await?;
        let error = Error::internal(std::io::Error::other(SECRET));
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.code(), "internal_error");
        assert!(error.to_string().contains(SECRET));
        rendered(app, format, error, internal, locale).await?;
    }
    Ok(())
}

#[tokio::test]
async fn configured_default_without_explicit_locale_applies_to_all_error_formats() -> TestResult {
    for default in ["en", "zh-CN"] {
        let app = application(default).await?;
        let outcome = verify_locale(&app, default).await;
        app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        outcome?;
    }
    Ok(())
}

#[tokio::test]
async fn explicit_locale_overrides_default_and_scope_does_not_leak() -> TestResult {
    for default in ["en", "zh-CN"] {
        let app = application(default).await?;
        let outcome = async {
            let i18n = app.state().resolve::<Translator>().await?;
            for locale in ["en", "zh-CN"] {
                i18n.with_locale(Locale::new(locale)?, verify_locale(&app, locale))
                    .await?;
            }
            verify_locale(&app, default).await
        }
        .await;
        app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        outcome?;
    }
    Ok(())
}
