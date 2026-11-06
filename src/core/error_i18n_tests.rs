//! 默认三种错误格式共用应用翻译目录；诊断 Display/source 不随用户语言变化。

use super::*;
use crate::{
    core::{ExceptionHandler, ExceptionRequest},
    i18n::{I18n, I18nConfig, I18nServiceProvider, Locale, MessageCatalog},
};

#[derive(Clone, Copy)]
enum Format {
    Json,
    Html,
    Envelope,
}
const FORMATS: [Format; 3] = [Format::Json, Format::Html, Format::Envelope];

fn service(default_locale: &str, catalog: MessageCatalog) -> Result<I18n> {
    I18n::new(
        I18nConfig {
            default_locale: Locale::new(default_locale)?,
            fallback_locale: Locale::new("en")?,
        },
        catalog,
    )
    .map_err(Error::from)
}

async fn body(
    handler: &ExceptionHandler,
    format: Format,
    error: Error,
    debug: bool,
) -> Result<String> {
    let mut handler = handler.clone();
    if matches!(format, Format::Envelope) {
        handler.api_envelope_when(|_, _| true);
    }
    let status = error.status();
    let request = ExceptionRequest::new(
        http::Method::GET,
        "/i18n",
        !matches!(format, Format::Html),
        "i18n-e01",
        debug,
    );
    let response = with_response_context(
        ResponseContext::new("i18n-e01", HttpStatusMode::Semantic),
        async { handler.render_for_request(error, &request) },
    )
    .await;
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()[header::VARY], "Accept");
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .map_err(Error::internal)?;
    String::from_utf8(bytes.to_vec()).map_err(Error::internal)
}

fn message(body: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(Error::internal)?;
    value["message"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::custom("test_message_missing", "响应缺少用户消息"))
}

fn assert_message(body: &str, format: Format, expected: &str, locale: &str) -> Result<()> {
    match format {
        Format::Html => {
            assert!(body.contains(&format!("<html lang=\"{locale}\">")));
            assert!(body.contains(&format!("<p>{expected}</p>")));
        }
        Format::Json | Format::Envelope => assert_eq!(message(body)?, expected),
    }
    Ok(())
}

#[tokio::test]
async fn explicit_http_details_survive_generic_catalog_entries_in_every_format() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    for code in [
        "bad_request",
        "forbidden",
        "not_found",
        "conflict",
        "source_detail",
    ] {
        catalog.insert(
            "zh-CN",
            format!("openrust.error.{code}"),
            "不能覆盖显式详情",
        )?;
    }
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(service("zh-CN", catalog)?);
    for format in FORMATS {
        for error in [
            Error::BadRequest("explicit safe detail".into()),
            Error::Forbidden("explicit safe detail".into()),
            Error::NotFound("explicit safe detail".into()),
            Error::Conflict("explicit safe detail".into()),
            Error::http_with_source(
                StatusCode::NOT_FOUND,
                "source_detail",
                "explicit safe detail",
                std::io::Error::other("private source"),
            ),
        ] {
            let rendered = body(&handler, format, error, false).await?;
            assert_message(&rendered, format, "explicit safe detail", "zh-CN")?;
            assert!(!rendered.contains("private source"));
        }
    }
    Ok(())
}

#[tokio::test]
async fn configured_default_without_accept_language_is_shared_by_all_formats() -> Result<()> {
    for (locale, expected) in [("en", "authentication required"), ("zh-CN", "需要身份验证")] {
        let mut handler = ExceptionHandler::default();
        handler.set_i18n(service(locale, MessageCatalog::new())?);
        for format in FORMATS {
            let rendered = body(&handler, format, Error::Unauthorized, false).await?;
            assert_message(&rendered, format, expected, locale)?;
            if matches!(format, Format::Envelope) {
                let value: serde_json::Value =
                    serde_json::from_str(&rendered).map_err(Error::internal)?;
                assert_eq!(value["code"], "unauthorized");
                assert_eq!(value["request_id"], "i18n-e01");
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn explicit_scope_wins_and_unsupported_language_uses_configured_default() -> Result<()> {
    let i18n = service("zh-CN", MessageCatalog::new())?;
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(i18n.clone());
    for (requested, expected, locale) in [
        ("en", "authentication required", "en"),
        ("fr", "需要身份验证", "zh-CN"),
    ] {
        i18n.with_locale(Locale::new(requested)?, async {
            for format in FORMATS {
                assert_message(
                    &body(&handler, format, Error::Unauthorized, false).await?,
                    format,
                    expected,
                    locale,
                )?;
            }
            Ok::<_, Error>(())
        })
        .await?;
    }
    assert_eq!(i18n.select(None).as_str(), "zh-CN");
    assert_eq!(i18n.select(Some("fr, de;q=0.5")).as_str(), "zh-CN");
    Ok(())
}

#[tokio::test]
async fn exact_locale_precedes_language_and_missing_keys_use_same_catalog_fallback() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert(
        "en-GB",
        "openrust.error.not_found",
        "British resource missing",
    )?;
    catalog.insert("en", "openrust.error.not_found", "English resource missing")?;
    catalog.insert(
        "en",
        "openrust.error.application_missing",
        "fallback application message",
    )?;
    catalog.insert("zh-CN", "openrust.error.application_empty", "")?;
    let i18n = service("en-GB", catalog.clone())?;
    assert_eq!(i18n.select(Some("en-GB")).as_str(), "en-GB");
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(i18n);
    for format in FORMATS {
        assert_message(
            &body(
                &handler,
                format,
                Error::custom("not_found", "diagnostic unchanged"),
                false,
            )
            .await?,
            format,
            "British resource missing",
            "en-GB",
        )?;
    }
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(service("zh-CN", catalog)?);
    for format in FORMATS {
        assert_message(
            &body(
                &handler,
                format,
                Error::custom("application_missing", "raw safe fallback"),
                false,
            )
            .await?,
            format,
            "fallback application message",
            "zh-CN",
        )?;
        assert_message(
            &body(
                &handler,
                format,
                Error::custom("application_empty", "must not replace empty translation"),
                false,
            )
            .await?,
            format,
            "",
            "zh-CN",
        )?;
        assert_message(
            &body(
                &handler,
                format,
                Error::custom("unregistered_code", "explicit safe detail"),
                false,
            )
            .await?,
            format,
            "explicit safe detail",
            "zh-CN",
        )?;
    }
    Ok(())
}

#[tokio::test]
async fn validation_rules_fields_and_plural_summary_share_locale_without_changing_diagnostics()
-> Result<()> {
    use crate::validation::{InputContext, Rule, Rules};
    let i18n = service("zh-CN", MessageCatalog::new())?;
    let error = i18n
        .with_default_locale(async {
            Rules::new()
                .field("email", [Rule::required()])
                .field("name", [Rule::required()])
                .validate(&serde_json::json!({}), InputContext::Json)
                .err()
                .ok_or_else(|| Error::custom("test_validation_missing", "预期验证失败"))
        })
        .await?;
    assert_eq!(error.to_string(), "此项为必填项 (and 1 more error)");
    let Error::Validation(errors) = error else {
        return Err(Error::custom(
            "test_validation_variant",
            "验证错误未保留字段结构",
        ));
    };
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(i18n);
    for format in FORMATS {
        let rendered = body(
            &handler,
            format,
            Error::validation_in_bag("profile", errors.clone()),
            false,
        )
        .await?;
        assert_message(&rendered, format, "此项为必填项（另有 1 项错误）", "zh-CN")?;
        if !matches!(format, Format::Html) {
            let value: serde_json::Value =
                serde_json::from_str(&rendered).map_err(Error::internal)?;
            assert_eq!(value["errors"]["email"][0], "此项为必填项");
            assert_eq!(value["errors"]["name"][0], "此项为必填项");
        }
        assert_message(
            &body(
                &handler,
                format,
                Error::Validation(ValidationErrors::new()),
                false,
            )
            .await?,
            format,
            "输入数据无效。",
            "zh-CN",
        )?;
    }
    Ok(())
}

#[tokio::test]
async fn server_messages_hide_sources_and_debug_diagnostics_remain_explicit_and_untranslated()
-> Result<()> {
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(service("zh-CN", MessageCatalog::new())?);
    for format in FORMATS {
        let error = Error::internal(std::io::Error::other("credential-private-sentinel"));
        let rendered = body(&handler, format, error, false).await?;
        assert_message(&rendered, format, "服务器内部错误", "zh-CN")?;
        assert!(!rendered.contains("credential-private-sentinel"));
        let custom = Error::custom_with_status(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "DSN-private-sentinel",
        );
        let rendered = body(&handler, format, custom, false).await?;
        assert_message(&rendered, format, "服务器内部错误", "zh-CN")?;
        assert!(!rendered.contains("DSN-private-sentinel"));
    }
    let debug = body(
        &handler,
        Format::Json,
        Error::internal(std::io::Error::other("original-debug-source")),
        true,
    )
    .await?;
    assert_eq!(message(&debug)?, "服务器内部错误");
    let payload: serde_json::Value = serde_json::from_str(&debug).map_err(Error::internal)?;
    assert_eq!(payload["sources"][0], "original-debug-source");
    Ok(())
}

#[tokio::test]
async fn background_default_scope_and_early_bootstrap_do_not_resolve_global_services() -> Result<()>
{
    assert_eq!(
        Error::Unauthorized.to_api_response().message,
        "authentication required"
    );
    let i18n = service("zh-CN", MessageCatalog::new())?;
    i18n.with_default_locale(async {
        assert_eq!(
            Error::Unauthorized.to_api_response().message,
            "需要身份验证"
        );
        assert_eq!(ApiResponse::success(()).message, "成功");
        assert_eq!(
            Error::internal(std::io::Error::other("private"))
                .to_api_response()
                .message,
            "服务器内部错误"
        );
    })
    .await;
    assert_eq!(
        Error::Unauthorized.to_api_response().message,
        "authentication required"
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_app_catalogs_and_locales_never_share_mutable_default_state() -> Result<()> {
    let mut a_catalog = MessageCatalog::new();
    a_catalog.insert("zh-CN", "openrust.error.unauthorized", "应用甲需要验证")?;
    let mut b_catalog = MessageCatalog::new();
    b_catalog.insert(
        "en",
        "openrust.error.unauthorized",
        "application B authentication",
    )?;
    let a = service("zh-CN", a_catalog)?;
    let b = service("en", b_catalog)?;
    let (left, right) = tokio::join!(
        a.with_default_locale(async {
            for _ in 0..12 {
                tokio::task::yield_now().await;
                assert_eq!(
                    Error::Unauthorized.to_api_response().message,
                    "应用甲需要验证"
                );
            }
            Ok::<_, Error>(())
        }),
        b.with_default_locale(async {
            for _ in 0..12 {
                tokio::task::yield_now().await;
                assert_eq!(
                    Error::Unauthorized.to_api_response().message,
                    "application B authentication"
                );
            }
            Ok::<_, Error>(())
        }),
    );
    left?;
    right?;
    Ok(())
}

#[tokio::test]
async fn provider_installs_the_resolved_app_catalog_without_http_dependency() -> Result<()> {
    let mut builder = crate::core::ProviderBuilder::new();
    builder.provider(I18nServiceProvider::default());
    let runtime = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(
                serde_json::json!({"i18n":{"default_locale":"zh-CN","fallback_locale":"en"}}),
            ),
        ))
        .await?;
    let app = crate::core::App::from_provider_runtime(runtime)?;
    let outcome: Result<()> = async {
        let handler = app
            .exception_handler()
            .ok_or_else(|| Error::custom("test_handler_missing", "应用异常策略未安装"))?;
        for format in FORMATS {
            assert_message(
                &body(handler, format, Error::Unauthorized, false).await?,
                format,
                "需要身份验证",
                "zh-CN",
            )?;
        }
        Ok(())
    }
    .await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn response_serialization_failure_keeps_localized_safe_envelope() -> Result<()> {
    struct PrivateSerialization;
    impl serde::Serialize for PrivateSerialization {
        fn serialize<S: serde::Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("private-serialization-sentinel"))
        }
    }
    let i18n = service("zh-CN", MessageCatalog::new())?;
    let response = i18n
        .with_default_locale(async { ApiResponse::success(PrivateSerialization).into_response() })
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .map_err(Error::internal)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(Error::internal)?;
    assert_eq!(value["code"], "internal_error");
    assert_eq!(value["message"], "服务器内部错误");
    assert!(!String::from_utf8_lossy(&bytes).contains("private-serialization-sentinel"));
    Ok(())
}

#[tokio::test]
async fn protocol_headers_and_authorized_typed_renderer_are_not_translated() -> Result<()> {
    #[derive(Debug)]
    struct ProtocolSource;
    impl std::fmt::Display for ProtocolSource {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("original protocol source")
        }
    }
    impl std::error::Error for ProtocolSource {}
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(service("zh-CN", MessageCatalog::new())?);
    handler.render::<ProtocolSource, _>(|_, _| {
        let mut response = (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"error":"invalid_client","error_description":"Client authentication failed"}))).into_response();
        response.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"OAuth\""));
        Some(response)
    });
    let request =
        ExceptionRequest::new(http::Method::POST, "/oauth/token", true, "protocol", false);
    let response = handler.render_for_request(
        Error::http_with_source(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "Client authentication failed",
            ProtocolSource,
        ),
        &request,
    );
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers()[header::WWW_AUTHENTICATE],
        "Basic realm=\"OAuth\""
    );
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .map_err(Error::internal)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(Error::internal)?;
    assert_eq!(value["error"], "invalid_client");
    assert_eq!(value["error_description"], "Client authentication failed");
    let response = handler.render_for_request(Error::rate_limited(7), &request);
    assert_eq!(response.headers()[header::RETRY_AFTER], "7");
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .map_err(Error::internal)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(Error::internal)?;
    assert_eq!(value["message"], "请求过于频繁");
    Ok(())
}

#[tokio::test]
async fn language_resource_precedes_configured_fallback_and_html_escapes_translations() -> Result<()>
{
    let mut catalog = MessageCatalog::new();
    catalog.insert("en-GB", "locale.marker", "registered")?;
    catalog.insert("en", "openrust.error.language_only", "language resource")?;
    catalog.insert("zh-CN", "openrust.error.language_only", "不应抢先使用回退")?;
    catalog.insert("zh-CN", "openrust.error.fallback_only", "回退语言")?;
    catalog.insert(
        "en-GB",
        "openrust.error.html_message",
        "<script>not markup</script>",
    )?;
    let i18n = I18n::new(
        I18nConfig {
            default_locale: Locale::new("en-GB")?,
            fallback_locale: Locale::new("zh-CN")?,
        },
        catalog,
    )?;
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(i18n);
    for format in FORMATS {
        assert_message(
            &body(
                &handler,
                format,
                Error::custom("language_only", "raw"),
                false,
            )
            .await?,
            format,
            "language resource",
            "en-GB",
        )?;
        assert_message(
            &body(
                &handler,
                format,
                Error::custom("fallback_only", "raw"),
                false,
            )
            .await?,
            format,
            "回退语言",
            "en-GB",
        )?;
        let rendered = body(
            &handler,
            format,
            Error::custom("html_message", "raw"),
            false,
        )
        .await?;
        match format {
            Format::Html => {
                assert!(rendered.contains("<p>&lt;script&gt;not markup&lt;/script&gt;</p>"));
                assert!(!rendered.contains("<script>"));
            }
            Format::Json | Format::Envelope => {
                assert_eq!(message(&rendered)?, "<script>not markup</script>");
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn regional_configured_default_keeps_its_tag_with_language_only_resources() -> Result<()> {
    let i18n = service("en-US", MessageCatalog::new())?;
    let mut handler = ExceptionHandler::default();
    handler.set_i18n(i18n.clone());
    for requested in ["en-US", "fr"] {
        i18n.with_locale(Locale::new(requested)?, async {
            for format in FORMATS {
                assert_message(
                    &body(&handler, format, Error::Unauthorized, false).await?,
                    format,
                    "authentication required",
                    "en-US",
                )?;
            }
            Ok::<_, Error>(())
        })
        .await?;
    }
    Ok(())
}
