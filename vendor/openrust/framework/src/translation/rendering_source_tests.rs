//! 真实DefaultProviders/Handler链保留ADR错误Catalog增强；公开translator仍为Laravel默认。
use super::*;
use crate::foundation::Application;
use crate::foundation::ExceptionHandler;
use crate::foundation::ExceptionRequest;
use crate::foundation::ProviderBuilder;
use crate::foundation::ProviderId;
use crate::support::DefaultProviders;

async fn render(
    handler: &ExceptionHandler,
    code: &str,
    expected: &str,
    locale: &str,
) -> Result<()> {
    for format in 0..3 {
        let mut handler = handler.clone();
        if format == 2 {
            handler.api_envelope_when(|_, _| true);
        }
        let request = ExceptionRequest::new(
            axum::http::Method::GET,
            "/i18n-source",
            format != 1,
            "i18n-source",
            false,
        );
        let response = crate::core::error::with_response_context(
            crate::core::error::ResponseContext::new(
                "i18n-source",
                crate::config::HttpStatusMode::Semantic,
            ),
            async {
                handler.render_for_request(
                    Error::custom(code, "raw diagnostic must not override translation"),
                    &request,
                )
            },
        )
        .await;
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .map_err(Error::internal)?;
        let body = String::from_utf8(bytes.to_vec()).map_err(Error::internal)?;
        if format == 1 {
            assert!(body.contains(&format!("<html lang=\"{locale}\">")));
            assert!(body.contains(&format!("<p>{expected}</p>")));
        } else {
            let json: serde_json::Value = serde_json::from_str(&body).map_err(Error::internal)?;
            assert_eq!(json["message"], expected);
        }
    }
    Ok(())
}
fn rendering_config() -> figment::Figment {
    use figment::providers::Serialized;
    use serde_json::json;

    let config = figment::Figment::new();
    #[cfg(feature = "encryption")]
    let config = config.merge(Serialized::defaults(json!({
        "security": {"encryption": {"active_key_id": "fixture", "keys": {
            "fixture": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
        }}}
    })));
    config.merge(Serialized::defaults(json!({
        "i18n": {"default_locale": "zh-CN", "fallback_locale": "en"},
        "cache": {"default": "memory", "stores": {"memory": {"driver": "memory"}}}
    })))
}

#[tokio::test]
async fn default_provider_handler_preserves_catalog_fallback_empty_and_scope_without_changing_translator()
-> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert(
        "en",
        "openrust.error.application_missing",
        "fallback application message",
    )?;
    catalog.insert("zh-CN", "openrust.error.application_empty", "")?;
    catalog.insert("en-GB", "locale.marker", "registered")?;
    catalog.insert("en", "openrust.error.language_only", "language resource")?;
    let defaults = DefaultProviders::for_runtime(false, false).replace(
        ProviderId::of::<TranslationServiceProvider>(),
        Box::new(TranslationServiceProvider::new(catalog)),
    );
    let mut builder = ProviderBuilder::new();
    builder.default_providers(defaults);
    let config = rendering_config();
    // 本地磁盘在 boot 时建立暂存目录，生命周期覆盖应用 shutdown。
    #[cfg(feature = "storage-local")]
    let storage = tempfile::tempdir().map_err(Error::internal)?;
    #[cfg(feature = "storage-local")]
    let config = config.merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "storage": {"default_disk": "local", "disks": {"local": {
                "driver": "fs", "root": storage.path()
            }}}
        }),
    ));
    let runtime = builder.start(config).await?;
    let app = Application::from_provider_runtime(runtime)?;
    let outcome: Result<()> = async {
        let service = app.state().container().resolve::<Translator>().await?;
        let handler = app
            .exception_handler()
            .ok_or_else(|| Error::custom("test_handler_missing", "异常策略未安装"))?;
        assert!(
            handler
                .i18n_service()
                .is_some_and(|rendering| Arc::ptr_eq(&rendering.catalog, &service.catalog))
        );
        // 无HTTP/无locale上下文的Handler也使用同一已发布目录，允许空字符串和跨语言错误回退。
        render(
            handler,
            "application_missing",
            "fallback application message",
            "zh-CN",
        )
        .await?;
        render(handler, "application_empty", "", "zh-CN").await?;
        service
            .with_locale(Locale::new("en-GB")?, async {
                render(handler, "language_only", "language resource", "en-GB").await?;
                render(
                    handler,
                    "application_missing",
                    "fallback application message",
                    "en-GB",
                )
                .await?;
                Ok::<(), Error>(())
            })
            .await?;
        service
            .with_locale(Locale::new("zh-CN")?, async {
                render(handler, "application_empty", "", "zh-CN").await?;
                assert_eq!(
                    Error::custom("application_empty", "raw")
                        .to_api_response()
                        .message,
                    ""
                );
                Ok::<(), Error>(())
            })
            .await?;
        // 渲染侧clone的policy只影响该临时句柄，未把公共singleton悄悄换回Catalog默认。
        assert_eq!(
            service.translate(
                &Locale::new("zh-CN")?,
                "openrust.error.application_missing",
                &MessageArgs::new()
            ),
            "openrust.error.application_missing"
        );
        assert_eq!(
            service.translate(
                &Locale::new("zh-CN")?,
                "openrust.error.application_empty",
                &MessageArgs::new()
            ),
            "openrust.error.application_empty"
        );
        assert!(!service.refresh_status()?.configured);
        Ok(())
    }
    .await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await;
    outcome?;
    cleanup
}
