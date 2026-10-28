//! 示例问候内容，展示模块配置与同一容器中的业务依赖替换。

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ExampleConfig {
    pub(crate) greeting: String,
    pub(crate) article_summary_origin: Option<String>,
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
            #[cfg(all(
                feature = "auth-oidc",
                any(feature = "db-postgres", feature = "db-mysql")
            ))]
            oidc: None,
        }
    }
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

pub(crate) struct ExampleMessage(String);

impl ExampleMessage {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn text(&self) -> &str {
        &self.0
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
