//! Request-scoped message catalogs and locale selection.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};

use crate::config::ConfigError;
use crate::core::{AppBuilder, Error, Plugin, Result};

/// Canonical language tag used by an application message catalog.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Locale(String);

impl Locale {
    /// Parses and canonicalizes a language tag such as `en` or `zh-CN`.
    pub fn new(value: impl AsRef<str>) -> Result<Self, ConfigError> {
        normalize_locale(value.as_ref()).map(Self)
    }

    /// Returns the canonical language tag.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn language(&self) -> &str {
        self.0.split('-').next().unwrap_or(&self.0)
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for Locale {
    type Error = ConfigError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Locale> for String {
    fn from(value: Locale) -> Self {
        value.0
    }
}

/// Named string arguments for message templates.
pub type MessageArgs = BTreeMap<String, String>;

/// Immutable locale-to-message map. Application resources overlay built-in messages by key.
#[derive(Debug, Clone)]
pub struct MessageCatalog {
    messages: BTreeMap<Locale, BTreeMap<String, String>>,
}

impl Default for MessageCatalog {
    fn default() -> Self {
        Self {
            messages: BTreeMap::from([
                (Locale("en".to_owned()), messages(EN_MESSAGES)),
                (Locale("zh-CN".to_owned()), messages(ZH_CN_MESSAGES)),
            ]),
        }
    }
}

impl MessageCatalog {
    /// Creates a catalog containing framework English and Simplified Chinese messages.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a JSON object of `message_key: template` entries and merges it into `locale`.
    pub fn merge_json(
        &mut self,
        locale: impl AsRef<str>,
        json: &str,
    ) -> std::result::Result<(), ConfigError> {
        let locale = Locale::new(locale)?;
        let messages: BTreeMap<String, String> = serde_json::from_str(json).map_err(|error| {
            ConfigError::Invalid(format!(
                "i18n.{locale}: invalid JSON message resource: {error}"
            ))
        })?;
        self.messages.entry(locale).or_default().extend(messages);
        Ok(())
    }

    /// Adds or replaces one message template.
    pub fn insert(
        &mut self,
        locale: impl AsRef<str>,
        key: impl Into<String>,
        template: impl Into<String>,
    ) -> std::result::Result<(), ConfigError> {
        let locale = Locale::new(locale)?;
        self.messages
            .entry(locale)
            .or_default()
            .insert(key.into(), template.into());
        Ok(())
    }

    /// Returns all locale tags that contain resources.
    pub fn locales(&self) -> impl Iterator<Item = &Locale> {
        self.messages.keys()
    }

    fn template(&self, locale: &Locale, key: &str) -> Option<&str> {
        let localized = self
            .messages
            .get(locale)
            .and_then(|messages| messages.get(key));
        if localized.is_some() {
            return localized.map(String::as_str);
        }
        self.messages
            .iter()
            .find(|(tag, _)| tag.language() == locale.language())
            .and_then(|(_, messages)| messages.get(key))
            .map(String::as_str)
    }

    fn supports(&self, locale: &Locale) -> bool {
        self.messages
            .keys()
            .any(|supported| supported == locale || supported.language() == locale.language())
    }
}

fn messages(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, message)| ((*key).to_owned(), (*message).to_owned()))
        .collect()
}

/// Language defaults used when an incoming request has no supported language preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct I18nConfig {
    /// Locale used when no supported request language is present.
    pub default_locale: Locale,
    /// Final locale used when neither the selected locale nor its language has a message.
    pub fallback_locale: Locale,
}

impl Default for I18nConfig {
    fn default() -> Self {
        Self {
            default_locale: Locale("en".to_owned()),
            fallback_locale: Locale("en".to_owned()),
        }
    }
}

/// Application internationalization service, safe to share across requests.
#[derive(Debug, Clone)]
pub struct I18n {
    config: I18nConfig,
    catalog: Arc<MessageCatalog>,
}

impl I18n {
    /// Creates the service and validates that both configured defaults have resources.
    pub fn new(config: I18nConfig, catalog: MessageCatalog) -> Result<Self, ConfigError> {
        for (key, locale) in [
            ("i18n.default_locale", &config.default_locale),
            ("i18n.fallback_locale", &config.fallback_locale),
        ] {
            if !catalog.supports(locale) {
                return Err(ConfigError::Invalid(format!(
                    "{key}: locale `{locale}` has no message resource"
                )));
            }
        }
        Ok(Self {
            config,
            catalog: Arc::new(catalog),
        })
    }

    /// Selects the best supported locale from an `Accept-Language` header.
    pub fn select(&self, accept_language: Option<&str>) -> Locale {
        let Some(header) = accept_language else {
            return self.config.default_locale.clone();
        };
        let mut candidates = header
            .split(',')
            .enumerate()
            .filter_map(|(order, part)| {
                let mut fields = part.trim().split(';');
                let tag = fields.next()?.trim();
                let quality = match fields.find_map(|field| field.trim().strip_prefix("q=")) {
                    Some(value) => value.parse::<f32>().ok()?,
                    None => 1.0,
                };
                if tag == "*"
                    || !quality.is_finite()
                    || quality == 0.0
                    || !(0.0..=1.0).contains(&quality)
                {
                    return None;
                }
                Locale::new(tag).ok().map(|locale| (order, quality, locale))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        candidates
            .into_iter()
            .find_map(|(_, _, candidate)| {
                self.catalog
                    .messages
                    .keys()
                    .find(|supported| {
                        *supported == &candidate || supported.language() == candidate.language()
                    })
                    .cloned()
            })
            .unwrap_or_else(|| self.config.default_locale.clone())
    }

    /// Resolves a key using the selected locale and configured fallback, then replaces `{name}` arguments.
    pub fn translate(&self, locale: &Locale, key: &str, args: &MessageArgs) -> String {
        let template = self.catalog.template(locale, key).or_else(|| {
            self.catalog
                .messages
                .get(&self.config.fallback_locale)?
                .get(key)
                .map(String::as_str)
        });
        template.map_or_else(|| key.to_owned(), |value| interpolate(value, args))
    }

    /// Runs a future with an explicit locale, overriding the locale selected from the request header.
    pub async fn with_locale<F: Future>(&self, locale: Locale, future: F) -> F::Output {
        I18N_CONTEXT
            .scope(
                I18nContext {
                    i18n: self.clone(),
                    locale,
                },
                future,
            )
            .await
    }

    fn locale_for_request(&self, accept_language: Option<&str>) -> Locale {
        self.select(accept_language)
    }
}

/// Installs a configured internationalization service into application state.
#[derive(Default)]
pub struct I18nPlugin {
    config: Option<I18nConfig>,
    catalog: MessageCatalog,
}

impl I18nPlugin {
    /// Creates a plugin from an application-specific catalog and language defaults.
    pub fn new(config: I18nConfig, catalog: MessageCatalog) -> Result<Self, ConfigError> {
        I18n::new(config.clone(), catalog.clone())?;
        Ok(Self {
            config: Some(config),
            catalog,
        })
    }
}

impl Plugin for I18nPlugin {
    fn name(&self) -> &'static str {
        "i18n"
    }

    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config = self
            .config
            .clone()
            .unwrap_or_else(|| app.config().i18n.clone());
        app.insert(I18n::new(config, self.catalog.clone())?);
        Ok(())
    }
}

/// The locale selected for the current HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLocale(Locale);

impl RequestLocale {
    /// Returns the selected canonical locale tag.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the selected locale.
    pub fn locale(&self) -> &Locale {
        &self.0
    }

    /// Translates a message using the current application's catalog.
    pub fn translate(&self, i18n: &I18n, key: &str, args: &MessageArgs) -> String {
        i18n.translate(&self.0, key, args)
    }
}

impl<S> FromRequestParts<S> for RequestLocale
where
    S: Send + Sync,
{
    type Rejection = Error;

    fn from_request_parts(
        _parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = std::result::Result<Self, Self::Rejection>> + Send {
        std::future::ready(
            I18N_CONTEXT
                .try_with(|context| Self(context.locale.clone()))
                .map_err(|_| Error::internal(MissingRequestLocale)),
        )
    }
}

#[derive(Clone)]
struct I18nContext {
    i18n: I18n,
    locale: Locale,
}

tokio::task_local! {
    static I18N_CONTEXT: I18nContext;
}

pub(crate) async fn with_request_locale<F: Future>(
    i18n: I18n,
    accept_language: Option<&str>,
    future: F,
) -> F::Output {
    let locale = i18n.locale_for_request(accept_language);
    i18n.with_locale(locale, future).await
}

pub(crate) fn translate_current(key: &str, args: &MessageArgs) -> Option<String> {
    I18N_CONTEXT
        .try_with(|context| context.i18n.translate(&context.locale, key, args))
        .ok()
}

fn normalize_locale(value: &str) -> std::result::Result<String, ConfigError> {
    let mut parts = value.trim().split('-');
    let Some(language) = parts.next() else {
        return Err(invalid_locale(value));
    };
    if !(2..=3).contains(&language.len())
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return Err(invalid_locale(value));
    }
    let mut normalized = language.to_ascii_lowercase();
    for (index, part) in parts.enumerate() {
        if part.is_empty()
            || part.len() > 8
            || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(invalid_locale(value));
        }
        normalized.push('-');
        if part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_alphabetic())
            || part.len() == 3 && part.bytes().all(|byte| byte.is_ascii_digit())
        {
            normalized.push_str(&part.to_ascii_uppercase());
        } else if index == 0 && part.len() == 4 {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                normalized.push(first.to_ascii_uppercase());
                normalized.extend(chars.map(|character| character.to_ascii_lowercase()));
            }
        } else {
            normalized.push_str(&part.to_ascii_lowercase());
        }
    }
    Ok(normalized)
}

fn invalid_locale(value: &str) -> ConfigError {
    ConfigError::Invalid(format!("invalid locale tag {value:?}"))
}

fn interpolate(template: &str, args: &MessageArgs) -> String {
    let mut output = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some(start) = remaining.find('{') {
        output.push_str(&remaining[..start]);
        let after_start = &remaining[start + 1..];
        let Some(end) = after_start.find('}') else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let name = &after_start[..end];
        if let Some(value) = args.get(name) {
            output.push_str(value);
        } else {
            output.push('{');
            output.push_str(name);
            output.push('}');
        }
        remaining = &after_start[end + 1..];
    }
    output.push_str(remaining);
    output
}

#[derive(Debug, thiserror::Error)]
#[error("request locale was not initialized by HTTP middleware")]
struct MissingRequestLocale;

const EN_MESSAGES: &[(&str, &str)] = &[
    ("openrust.response.ok", "ok"),
    ("openrust.error.bad_request", "bad request"),
    ("openrust.error.unauthorized", "authentication required"),
    ("openrust.error.forbidden", "forbidden"),
    ("openrust.error.not_found", "not found"),
    ("openrust.error.conflict", "conflict"),
    ("openrust.error.validation_failed", "validation failed"),
    ("openrust.error.rate_limited", "too many requests"),
    (
        "openrust.error.configuration_error",
        "internal server error",
    ),
    ("openrust.error.internal_error", "internal server error"),
    ("openrust.error.request_timeout", "request timed out"),
    ("openrust.error.payload_too_large", "request body too large"),
    ("openrust.error.method_not_allowed", "method not allowed"),
    (
        "openrust.error.unsupported_media_type",
        "unsupported media type",
    ),
    ("openrust.validation.email", "must be a valid email address"),
    ("openrust.validation.length", "has an invalid length"),
    ("openrust.validation.required", "is required"),
    ("openrust.validation.range", "is outside the allowed range"),
    ("openrust.validation.url", "must be a valid URL"),
];

const ZH_CN_MESSAGES: &[(&str, &str)] = &[
    ("openrust.response.ok", "成功"),
    ("openrust.error.bad_request", "请求参数错误"),
    ("openrust.error.unauthorized", "需要身份验证"),
    ("openrust.error.forbidden", "没有权限执行此操作"),
    ("openrust.error.not_found", "请求的资源不存在"),
    ("openrust.error.conflict", "请求与当前状态冲突"),
    ("openrust.error.validation_failed", "输入验证失败"),
    ("openrust.error.rate_limited", "请求过于频繁"),
    ("openrust.error.configuration_error", "服务器内部错误"),
    ("openrust.error.internal_error", "服务器内部错误"),
    ("openrust.error.request_timeout", "请求超时"),
    ("openrust.error.payload_too_large", "请求正文过大"),
    ("openrust.error.method_not_allowed", "不支持此请求方法"),
    ("openrust.error.unsupported_media_type", "不支持此媒体类型"),
    ("openrust.validation.email", "必须是有效的电子邮件地址"),
    ("openrust.validation.length", "长度不符合要求"),
    ("openrust.validation.required", "此项为必填项"),
    ("openrust.validation.range", "超出允许范围"),
    ("openrust.validation.url", "必须是有效的 URL"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use figment::providers::Format;

    #[test]
    fn canonicalizes_language_and_region() {
        let normalized = Locale::new("ZH-cn");
        assert!(normalized.is_ok());
        if let Ok(locale) = normalized {
            assert_eq!(locale.as_str(), "zh-CN");
        }
        assert!(Locale::new("x").is_err());
        assert!(Locale::new("zh--CN").is_err());
    }

    #[test]
    fn chooses_highest_quality_supported_language_and_skips_invalid_values() -> Result<()> {
        let service = I18n::new(I18nConfig::default(), MessageCatalog::new())?;
        assert_eq!(
            service.select(Some("fr;q=1, zh;q=0.7, en;q=0.3")).as_str(),
            "zh-CN"
        );
        assert_eq!(service.select(Some("zh;q=0, en;q=0.2")).as_str(), "en");
        assert_eq!(service.select(Some("zh;q=bogus, en")).as_str(), "en");
        Ok(())
    }

    #[test]
    fn merges_application_json_and_falls_back_by_language_then_default() -> Result<()> {
        let mut catalog = MessageCatalog::new();
        catalog.merge_json("zh-CN", r#"{"hello":"你好，{name}","override":"应用覆盖"}"#)?;
        catalog.insert("en", "fallback.only", "fallback")?;
        let service = I18n::new(I18nConfig::default(), catalog)?;
        let locale = Locale::new("zh")?;
        let args = BTreeMap::from([("name".to_owned(), "小明".to_owned())]);
        assert_eq!(service.translate(&locale, "hello", &args), "你好，小明");
        assert_eq!(
            service.translate(&locale, "openrust.error.unauthorized", &BTreeMap::new()),
            "需要身份验证"
        );
        assert_eq!(
            service.translate(&locale, "missing.key", &BTreeMap::new()),
            "missing.key"
        );
        assert_eq!(interpolate("{missing} {name}", &args), "{missing} 小明");
        Ok(())
    }

    #[test]
    fn request_locale_isolated_between_concurrent_scopes() -> Result<()> {
        let service = I18n::new(I18nConfig::default(), MessageCatalog::new())?;
        let english = service.clone();
        let chinese = service.clone();
        let english_locale = Locale::new("en")?;
        let chinese_locale = Locale::new("zh-CN")?;
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::internal)?
            .block_on(async move {
                tokio::join!(
                    english.with_locale(english_locale, async {
                        tokio::task::yield_now().await;
                        translate_current("openrust.error.unauthorized", &BTreeMap::new())
                    }),
                    chinese.with_locale(chinese_locale, async {
                        tokio::task::yield_now().await;
                        translate_current("openrust.error.unauthorized", &BTreeMap::new())
                    })
                )
            });
        assert_eq!(result.0.as_deref(), Some("authentication required"));
        assert_eq!(result.1.as_deref(), Some("需要身份验证"));
        Ok(())
    }

    #[test]
    fn rejects_malformed_json_resources() {
        let mut catalog = MessageCatalog::new();
        assert!(catalog.merge_json("en", "[]").is_err());
        assert!(catalog.merge_json("not_a_tag", "{}").is_err());
    }

    #[test]
    fn i18n_config_deserializes_from_the_standard_toml_section() -> Result<()> {
        let figment = figment::Figment::new().merge(figment::providers::Toml::string(
            "[i18n]\ndefault_locale = \"zh-CN\"\nfallback_locale = \"en\"",
        ));
        let config: I18nConfig = figment.extract_inner("i18n").map_err(ConfigError::from)?;
        assert_eq!(config.default_locale.as_str(), "zh-CN");
        assert_eq!(config.fallback_locale.as_str(), "en");
        Ok(())
    }
}
