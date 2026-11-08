//! 示例问候内容，展示模块配置与同一容器中的业务依赖替换。

use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use openrust::container::attributes::{Config, Give, Tag, Tagged};
use openrust::container::{
    AttributeParameter, AttributeValue, ClassAttribute, ClassDescriptor, ConstructorParam,
    ContextualAttribute, PhpBytes, PhpTarget, Resolver, ServiceKey,
};
use openrust::support::BoxFuture;
use std::sync::Arc;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ExampleConfig {
    pub(crate) greeting: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    greeting_suffix: Option<String>,
    pub(crate) article_summary_origin: Option<String>,
    pub(crate) feature: ExampleFeatureConfig,
    pub(crate) trusted_hosts: Option<openrust::config::TrustedHostsConfig>,
    #[cfg(feature = "auth-oidc")]
    pub(crate) oidc: Option<super::oidc::ExampleOidcConfig>,
}

impl Default for ExampleConfig {
    fn default() -> Self {
        Self {
            greeting: "Hello from modules/example".to_owned(),
            greeting_suffix: None,
            article_summary_origin: None,
            feature: ExampleFeatureConfig::default(),
            trusted_hosts: None,
            #[cfg(feature = "auth-oidc")]
            oidc: None,
        }
    }
}

/// 模块文件提供的固定业务开关。
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ExampleFeatureConfig {
    pub(crate) enabled: bool,
}

impl ExampleConfig {
    pub(crate) fn validate(&self) -> openrust::Result<()> {
        #[cfg(feature = "auth-oidc")]
        if let Some(oidc) = &self.oidc {
            oidc.validate()?;
        }
        let Some(origin) = &self.article_summary_origin else {
            return Ok(());
        };
        let invalid = || {
            openrust::Error::custom(
                "example_article_summary_origin_invalid",
                "example.article_summary_origin 必须是无凭据、查询和片段的 HTTP(S) 根地址",
            )
        };
        let url = url::Url::parse(origin).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(invalid());
        }
        Ok(())
    }
}

pub(crate) struct ExampleMessage {
    text: String,
    binding_extension: Option<&'static str>,
}

impl ExampleMessage {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            text: message.into(),
            binding_extension: None,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn with_binding_extension(&self) -> Self {
        Self {
            text: self.text.clone(),
            binding_extension: Some("module-provider"),
        }
    }

    pub(crate) fn binding_extension(&self) -> Option<&'static str> {
        self.binding_extension
    }
}

pub(crate) struct ExamplePresenter {
    message: Arc<ExampleMessage>,
    served_at: Arc<DateTime<Utc>>,
    configured_suffix: Arc<String>,
    suffixes: Arc<Tagged<GreetingSuffix>>,
}

pub(crate) const EXAMPLE_MESSAGE_ALIAS: &str = "example.greeting.message";
pub(crate) const EXAMPLE_PRESENTER_CLASS: &str = "Modules/Example/ExamplePresenter";
pub(crate) const EXAMPLE_PRESENTER_ALIAS: &str = "example.greeting.presenter";
pub(crate) const EXAMPLE_GREETING_SUFFIXES: &str = "example.greeting.suffixes";

/// 可由模块扩展登记的问候语后缀。
pub(crate) struct GreetingSuffix(pub(crate) &'static str);

/// 控制器读取问候语所需的呈现视图。
pub(crate) trait GreetingPresenter: Send + Sync {
    fn text(&self) -> &str;
    fn served_at(&self) -> DateTime<Utc>;
    fn rendered_text(&self) -> BoxFuture<'_, openrust::Result<String>>;
}

impl GreetingPresenter for ExamplePresenter {
    fn text(&self) -> &str {
        self.message.text()
    }

    fn served_at(&self) -> DateTime<Utc> {
        *self.served_at
    }

    fn rendered_text(&self) -> BoxFuture<'_, openrust::Result<String>> {
        Box::pin(async move {
            let mut text = self.text().to_owned();
            text.push_str(&self.configured_suffix);
            let mut suffixes = self.suffixes.stream();
            while let Some(suffix) = suffixes.next().await {
                text.push_str(suffix?.0);
            }
            Ok(text)
        })
    }
}

struct GreetingMessage;

impl ContextualAttribute for GreetingMessage {
    fn resolve(
        self: Arc<Self>,
        parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, openrust::Result<AttributeValue>> {
        Arc::new(Give::<ExampleMessage>::new(ServiceKey::named(
            EXAMPLE_MESSAGE_ALIAS,
        )))
        .resolve(parameter, resolver)
    }
}

/// 由示例 Provider 提供时间的参数属性。
pub(crate) struct ExampleServedAt;

impl ContextualAttribute for ExampleServedAt {}

/// 在具体呈现器构造后触发的类属性。
pub(crate) struct ExampleGreetingConstructed;

/// 提供问候呈现器的编译类描述。实际问候语仍来自模块 Provider 的绑定。
pub(crate) fn presenter_descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete::<ExamplePresenter>(
        EXAMPLE_PRESENTER_CLASS,
        vec![
            ConstructorParam::class::<ExampleMessage>("message")
                .with_attribute(ClassAttribute::contextual(|| Ok(GreetingMessage))),
            ConstructorParam::php("served_at", PhpTarget::Bytes)
                .with_attribute(ClassAttribute::contextual(|| Ok(ExampleServedAt))),
            ConstructorParam::scalar::<String>("configured_suffix").with_attribute(
                ClassAttribute::contextual(|| {
                    Ok(
                        Config::<String>::new("example.greeting_suffix")
                            .with_default(String::new()),
                    )
                }),
            ),
            ConstructorParam::scalar::<Tagged<GreetingSuffix>>("suffixes").with_attribute(
                ClassAttribute::contextual(|| {
                    Ok(Tag::<GreetingSuffix>::new(EXAMPLE_GREETING_SUFFIXES))
                }),
            ),
        ],
        |args| {
            let served_at = args.value(1)?.downcast::<PhpBytes>()?;
            let invalid_timestamp = || {
                openrust::Error::custom(
                    "example_presenter_time_invalid",
                    "问候呈现器需要 RFC 3339 时间",
                )
            };
            let timestamp =
                std::str::from_utf8(served_at.as_bytes()).map_err(|_| invalid_timestamp())?;
            let served_at = DateTime::parse_from_rfc3339(timestamp)
                .map_err(|_| invalid_timestamp())?
                .with_timezone(&Utc);
            Ok(Arc::new(ExamplePresenter {
                message: args.service::<ExampleMessage>(0)?,
                served_at: Arc::new(served_at),
                configured_suffix: args.value(2)?.downcast::<String>()?,
                suffixes: args.value(3)?.downcast::<Tagged<GreetingSuffix>>()?,
            }))
        },
    )
    .with_projection(openrust::class_projection!(ExamplePresenter => dyn GreetingPresenter))
    .with_attribute(ClassAttribute::marker(|| Ok(ExampleGreetingConstructed)))
}

#[cfg(test)]
mod tests {
    use super::ExampleConfig;

    #[test]
    fn article_summary_origin_rejects_untrusted_shapes_without_echoing_values()
    -> openrust::Result<()> {
        for value in [
            "http://user:secret-sentinel@example.test/",
            "file:///etc/passwd",
            "https://example.test/other",
            "https://example.test/?token=secret-sentinel",
        ] {
            let config = ExampleConfig {
                article_summary_origin: Some(value.to_owned()),
                ..ExampleConfig::default()
            };
            let Some(error) = config.validate().err() else {
                return Err(openrust::Error::BadRequest(
                    "origin unexpectedly accepted".into(),
                ));
            };
            assert_eq!(error.code(), "example_article_summary_origin_invalid");
            assert!(!format!("{error:?}").contains("secret-sentinel"));
        }
        Ok(())
    }
}
