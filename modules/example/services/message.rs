//! 示例问候内容，展示模块配置与同一容器中的业务依赖替换。

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ExampleConfig {
    pub(crate) greeting: String,
    pub(crate) article_summary_origin: Option<String>,
    pub(crate) feature: ExampleFeatureConfig,
    #[cfg(all(
        feature = "auth-oidc",
        any(feature = "db-postgres", feature = "db-mysql")
    ))]
    pub(crate) oidc: Option<super::oidc::ExampleOidcConfig>,
}

impl Default for ExampleConfig {
    fn default() -> Self {
        Self {
            greeting: "Hello from modules/example".to_owned(),
            article_summary_origin: None,
            feature: ExampleFeatureConfig::default(),
            #[cfg(all(
                feature = "auth-oidc",
                any(feature = "db-postgres", feature = "db-mysql")
            ))]
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
        #[cfg(all(
            feature = "auth-oidc",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
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
