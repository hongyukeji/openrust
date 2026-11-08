//! 示例问候内容，展示模块配置与同一容器中的业务依赖替换。

use chrono::{DateTime, Utc};
use openrust::container::{ClassDescriptor, ConstructorParam, ParameterValue};
use std::sync::Arc;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ExampleConfig {
    pub(crate) greeting: String,
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
}

pub(crate) const EXAMPLE_PRESENTER_CLASS: &str = "Modules/Example/ExamplePresenter";
pub(crate) const EXAMPLE_PRESENTER_ALIAS: &str = "example.greeting.presenter";

/// 控制器读取问候语所需的呈现视图。
pub(crate) trait GreetingPresenter: Send + Sync {
    fn text(&self) -> &str;
    fn served_at(&self) -> DateTime<Utc>;
}

impl GreetingPresenter for ExamplePresenter {
    fn text(&self) -> &str {
        self.message.text()
    }

    fn served_at(&self) -> DateTime<Utc> {
        *self.served_at
    }
}

/// 提供问候呈现器的编译类描述。实际问候语仍来自模块 Provider 的绑定。
pub(crate) fn class_descriptors() -> [ClassDescriptor; 1] {
    [ClassDescriptor::concrete::<ExamplePresenter>(
        EXAMPLE_PRESENTER_CLASS,
        vec![
            ConstructorParam::class::<ExampleMessage>("message"),
            ConstructorParam::scalar::<DateTime<Utc>>("served_at")
                .with_default(|| Ok(ParameterValue::new(Utc::now()))),
        ],
        |args| {
            Ok(Arc::new(ExamplePresenter {
                message: args.service::<ExampleMessage>(0)?,
                served_at: args.value(1)?.downcast::<DateTime<Utc>>()?,
            }))
        },
    )
    .with_projection(openrust::class_projection!(ExamplePresenter => dyn GreetingPresenter))]
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
