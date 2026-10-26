//! 请求作用域的消息目录与 locale 选择。

use std::collections::BTreeMap;
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Timelike};
use icu_datetime::DateTimeFormatter;
use icu_datetime::fieldsets::YMD;
use icu_datetime::input::{Date as IcuDate, DateTime as IcuDateTime, Time as IcuTime};
use icu_decimal::DecimalFormatter;
use icu_decimal::input::Decimal;
use icu_decimal::options::DecimalFormatterOptions;
use icu_locale::Locale as IcuLocale;
use icu_plurals::{PluralOperands, PluralRules};
use serde::{Deserialize, Serialize};

use crate::config::ConfigError;
use crate::core::{AppBuilder, Error, Plugin, Result};

/// 应用消息目录使用的规范语言标签。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Locale(String);

impl Locale {
    /// 解析并规范化语言标签，例如 `en` 或 `zh-CN`。
    pub fn new(value: impl AsRef<str>) -> Result<Self, ConfigError> {
        normalize_locale(value.as_ref()).map(Self)
    }

    /// 返回规范化后的语言标签。
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

/// 消息模板使用的具名字符串参数。
pub type MessageArgs = BTreeMap<String, String>;

/// ICU/CLDR 计数复数类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PluralCategory {
    /// 某些语言用于零。
    Zero,
    /// 语言规则定义的单数类别。
    One,
    /// 语言规则定义的双数类别。
    Two,
    /// 波兰语、俄语等语言的少数类别。
    Few,
    /// 某些语言用于大量或特定数值的类别。
    Many,
    /// 必须提供的通用回退类别。
    Other,
}

/// 日期或日期时间的地区显示长度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateFormatStyle {
    /// 紧凑数值格式。
    Short,
    /// 常用格式。
    Medium,
    /// 带完整月份名称的格式。
    Long,
}

/// 日期与时间合并显示的地区格式长度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTimeFormatStyle {
    /// 紧凑数值格式。
    Short,
    /// 常用格式。
    Medium,
    /// 带完整月份名称的格式。
    Long,
}

/// 数字或日期地区格式化失败。
#[derive(Debug, thiserror::Error)]
pub enum I18nFormatError {
    /// 输入不是有效的精确十进制数。
    #[error("invalid decimal value `{value}`")]
    InvalidNumber {
        /// 无法解析的十进制输入。
        value: String,
    },
    /// locale 数据或日期字段无法由格式化器处理。
    #[error("cannot format locale `{locale}`: {reason}")]
    Format {
        /// 请求的 locale。
        locale: String,
        /// 数据或日期构造错误。
        reason: String,
    },
}

#[derive(Debug, Clone)]
enum CatalogMessage {
    Text(String),
    Plural {
        argument: String,
        forms: BTreeMap<PluralCategory, String>,
    },
    Select {
        argument: String,
        cases: BTreeMap<String, String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawMessage {
    Text(String),
    Structured(StructuredMessage),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StructuredMessage {
    plural: Option<RawPluralMessage>,
    select: Option<RawSelectMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPluralMessage {
    argument: String,
    forms: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSelectMessage {
    argument: String,
    cases: BTreeMap<String, String>,
}

/// 不可变的 locale 到消息映射。应用资源按 key 覆盖内置消息。
#[derive(Debug, Clone)]
pub struct MessageCatalog {
    messages: BTreeMap<Locale, BTreeMap<String, CatalogMessage>>,
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
    /// 创建包含框架英文和简体中文消息的目录。
    pub fn new() -> Self {
        Self::default()
    }

    /// 合并 `locale` 的 JSON 消息资源；普通字符串兼容原模板，`plural` 和 `select` 使用有限分支对象。
    pub fn merge_json(
        &mut self,
        locale: impl AsRef<str>,
        json: &str,
    ) -> std::result::Result<(), ConfigError> {
        let locale = Locale::new(locale)?;
        let raw_messages: BTreeMap<String, RawMessage> =
            serde_json::from_str(json).map_err(|error| {
                ConfigError::Invalid(format!(
                    "i18n.{locale}: invalid JSON message resource: {error}"
                ))
            })?;
        let mut messages = BTreeMap::new();
        for (key, raw_message) in raw_messages {
            let message = CatalogMessage::try_from(raw_message)
                .map_err(|error| ConfigError::Invalid(format!("i18n.{locale}.{key}: {error}")))?;
            messages.insert(key, message);
        }
        self.messages.entry(locale).or_default().extend(messages);
        Ok(())
    }

    /// 添加或替换一条消息模板。
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
            .insert(key.into(), CatalogMessage::Text(template.into()));
        Ok(())
    }

    /// 添加按 ICU/CLDR 计数规则选择的消息；`Other` 是必需回退分支。
    pub fn insert_plural(
        &mut self,
        locale: impl AsRef<str>,
        key: impl Into<String>,
        argument: impl Into<String>,
        forms: BTreeMap<PluralCategory, String>,
    ) -> std::result::Result<(), ConfigError> {
        let locale = Locale::new(locale)?;
        let argument = argument.into();
        validate_argument(&argument)?;
        validate_templates(forms.values(), forms.contains_key(&PluralCategory::Other))?;
        self.messages
            .entry(locale)
            .or_default()
            .insert(key.into(), CatalogMessage::Plural { argument, forms });
        Ok(())
    }

    /// 添加有限值选择消息；`other` 是必需回退分支。
    pub fn insert_select(
        &mut self,
        locale: impl AsRef<str>,
        key: impl Into<String>,
        argument: impl Into<String>,
        cases: BTreeMap<String, String>,
    ) -> std::result::Result<(), ConfigError> {
        let locale = Locale::new(locale)?;
        let argument = argument.into();
        validate_argument(&argument)?;
        validate_templates(cases.values(), cases.contains_key("other"))?;
        if cases.keys().any(|case| case.is_empty() || case.len() > 64) {
            return Err(ConfigError::Invalid(
                "i18n select cases must use non-empty keys up to 64 bytes".to_owned(),
            ));
        }
        self.messages
            .entry(locale)
            .or_default()
            .insert(key.into(), CatalogMessage::Select { argument, cases });
        Ok(())
    }

    /// 返回包含资源的全部 locale 标签。
    pub fn locales(&self) -> impl Iterator<Item = &Locale> {
        self.messages.keys()
    }

    fn message(&self, locale: &Locale, key: &str) -> Option<(&Locale, &CatalogMessage)> {
        self.messages
            .get_key_value(locale)
            .and_then(|(tag, messages)| messages.get(key).map(|message| (tag, message)))
            .or_else(|| {
                self.messages
                    .iter()
                    .find(|(tag, _)| tag.language() == locale.language())
                    .and_then(|(tag, messages)| messages.get(key).map(|message| (tag, message)))
            })
    }

    fn supports(&self, locale: &Locale) -> bool {
        self.messages
            .keys()
            .any(|supported| supported == locale || supported.language() == locale.language())
    }
}

impl TryFrom<RawMessage> for CatalogMessage {
    type Error = String;

    fn try_from(raw: RawMessage) -> std::result::Result<Self, Self::Error> {
        match raw {
            RawMessage::Text(text) => Ok(Self::Text(text)),
            RawMessage::Structured(StructuredMessage {
                plural: Some(plural),
                select: None,
            }) => {
                validate_argument(&plural.argument).map_err(|error| error.to_string())?;
                let mut forms = BTreeMap::new();
                for (name, template) in plural.forms {
                    let category = parse_plural_category(&name)
                        .ok_or_else(|| format!("unsupported plural category `{name}`"))?;
                    if forms.insert(category, template).is_some() {
                        return Err(format!("duplicate plural category `{name}`"));
                    }
                }
                validate_templates(forms.values(), forms.contains_key(&PluralCategory::Other))
                    .map_err(|error| error.to_string())?;
                Ok(Self::Plural {
                    argument: plural.argument,
                    forms,
                })
            }
            RawMessage::Structured(StructuredMessage {
                plural: None,
                select: Some(select),
            }) => {
                validate_argument(&select.argument).map_err(|error| error.to_string())?;
                validate_templates(select.cases.values(), select.cases.contains_key("other"))
                    .map_err(|error| error.to_string())?;
                if select
                    .cases
                    .keys()
                    .any(|case| case.is_empty() || case.len() > 64)
                {
                    return Err("select cases must use non-empty keys up to 64 bytes".to_owned());
                }
                Ok(Self::Select {
                    argument: select.argument,
                    cases: select.cases,
                })
            }
            RawMessage::Structured(_) => Err(
                "a structured message must contain exactly one of `plural` or `select`".to_owned(),
            ),
        }
    }
}

fn validate_argument(argument: &str) -> std::result::Result<(), ConfigError> {
    if argument.is_empty()
        || !argument
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(ConfigError::Invalid(
            "i18n argument names must contain only ASCII letters, digits, and underscores"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_templates<'a>(
    templates: impl Iterator<Item = &'a String>,
    has_other: bool,
) -> std::result::Result<(), ConfigError> {
    let templates = templates.collect::<Vec<_>>();
    if !has_other || templates.is_empty() || templates.len() > 32 {
        return Err(ConfigError::Invalid(
            "i18n message branches require `other` and must contain 1 to 32 entries".to_owned(),
        ));
    }
    if templates.iter().any(|template| template.len() > 16 * 1024) {
        return Err(ConfigError::Invalid(
            "i18n message branches must not exceed 16384 bytes".to_owned(),
        ));
    }
    Ok(())
}

fn parse_plural_category(value: &str) -> Option<PluralCategory> {
    match value {
        "zero" => Some(PluralCategory::Zero),
        "one" => Some(PluralCategory::One),
        "two" => Some(PluralCategory::Two),
        "few" => Some(PluralCategory::Few),
        "many" => Some(PluralCategory::Many),
        "other" => Some(PluralCategory::Other),
        _ => None,
    }
}

fn plural_category(value: icu_plurals::PluralCategory) -> PluralCategory {
    match value {
        icu_plurals::PluralCategory::Zero => PluralCategory::Zero,
        icu_plurals::PluralCategory::One => PluralCategory::One,
        icu_plurals::PluralCategory::Two => PluralCategory::Two,
        icu_plurals::PluralCategory::Few => PluralCategory::Few,
        icu_plurals::PluralCategory::Many => PluralCategory::Many,
        icu_plurals::PluralCategory::Other => PluralCategory::Other,
    }
}

fn messages(entries: &[(&str, &str)]) -> BTreeMap<String, CatalogMessage> {
    entries
        .iter()
        .map(|(key, message)| {
            (
                (*key).to_owned(),
                CatalogMessage::Text((*message).to_owned()),
            )
        })
        .collect()
}

/// 传入请求没有受支持的语言偏好时使用的默认语言设置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct I18nConfig {
    /// 请求中没有受支持的语言时使用的 locale。
    pub default_locale: Locale,
    /// 所选 locale 及其语言都没有对应消息时使用的最终回退 locale。
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

/// 可在多个请求间安全共享的应用国际化服务。
#[derive(Debug, Clone)]
pub struct I18n {
    config: I18nConfig,
    catalog: Arc<MessageCatalog>,
}

impl I18n {
    /// 创建服务，并校验两个已配置的默认 locale 均存在资源。
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

    /// 根据 `Accept-Language` 请求头选择最匹配的受支持 locale。
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

    /// 使用所选 locale 和已配置的回退规则解析 key，然后替换 `{name}` 参数。
    pub fn translate(&self, locale: &Locale, key: &str, args: &MessageArgs) -> String {
        let message = self.catalog.message(locale, key).or_else(|| {
            self.catalog
                .messages
                .get_key_value(&self.config.fallback_locale)?
                .1
                .get(key)
                .map(|message| (&self.config.fallback_locale, message))
        });
        message.map_or_else(
            || key.to_owned(),
            |(message_locale, message)| match message {
                CatalogMessage::Text(template) => interpolate(template, args),
                CatalogMessage::Select { argument, cases } => {
                    let selected = args
                        .get(argument)
                        .and_then(|value| cases.get(value))
                        .or_else(|| cases.get("other"));
                    selected.map_or_else(|| key.to_owned(), |template| interpolate(template, args))
                }
                CatalogMessage::Plural { argument, forms } => {
                    let category = args
                        .get(argument)
                        .and_then(|value| Decimal::from_str(value).ok())
                        .map(|value| PluralOperands::from(&value))
                        .and_then(|value| {
                            let locale = IcuLocale::from_str(message_locale.as_str()).ok()?;
                            let rules = PluralRules::try_new_cardinal(locale.into()).ok()?;
                            Some(plural_category(rules.category_for(value)))
                        })
                        .unwrap_or(PluralCategory::Other);
                    let template = forms
                        .get(&category)
                        .or_else(|| forms.get(&PluralCategory::Other));
                    template.map_or_else(|| key.to_owned(), |template| interpolate(template, args))
                }
            },
        )
    }

    /// 按 locale 格式化精确十进制字符串，不经过浮点数或舍入。
    pub fn format_number(
        &self,
        locale: &Locale,
        decimal: &str,
    ) -> std::result::Result<String, I18nFormatError> {
        let value = Decimal::from_str(decimal).map_err(|_| I18nFormatError::InvalidNumber {
            value: decimal.to_owned(),
        })?;
        let preferences = icu_locale(locale)?;
        let formatter =
            DecimalFormatter::try_new(preferences.into(), DecimalFormatterOptions::default())
                .map_err(|error| format_error(locale, error))?;
        Ok(formatter.format(&value).to_string())
    }

    /// 按 locale 格式化公历日期；日期必须已按应用选择的时区确定。
    pub fn format_date(
        &self,
        locale: &Locale,
        date: NaiveDate,
        style: DateFormatStyle,
    ) -> std::result::Result<String, I18nFormatError> {
        let month = u8::try_from(date.month()).map_err(|error| format_error(locale, error))?;
        let day = u8::try_from(date.day()).map_err(|error| format_error(locale, error))?;
        let date = IcuDate::try_new_iso(date.year(), month, day)
            .map_err(|error| format_error(locale, error))?;
        let preferences = icu_locale(locale)?;
        let field_set = match style {
            DateFormatStyle::Short => YMD::short(),
            DateFormatStyle::Medium => YMD::medium(),
            DateFormatStyle::Long => YMD::long(),
        };
        let formatter = DateTimeFormatter::try_new(preferences.into(), field_set)
            .map_err(|error| format_error(locale, error))?;
        Ok(formatter.format(&date).to_string())
    }

    /// 按 locale 格式化日期时间的本地墙上时间。
    ///
    /// `datetime` 必须已经转换到调用方选择的时区；方法不会推断进程默认时区，
    /// 也不会把该时间重新转换为其他时区。
    pub fn format_datetime<Tz: TimeZone>(
        &self,
        locale: &Locale,
        datetime: &DateTime<Tz>,
        style: DateTimeFormatStyle,
    ) -> std::result::Result<String, I18nFormatError> {
        let month = u8::try_from(datetime.month()).map_err(|error| format_error(locale, error))?;
        let day = u8::try_from(datetime.day()).map_err(|error| format_error(locale, error))?;
        let date = IcuDate::try_new_iso(datetime.year(), month, day)
            .map_err(|error| format_error(locale, error))?;
        let hour = u8::try_from(datetime.hour()).map_err(|error| format_error(locale, error))?;
        let minute =
            u8::try_from(datetime.minute()).map_err(|error| format_error(locale, error))?;
        let second =
            u8::try_from(datetime.second()).map_err(|error| format_error(locale, error))?;
        let time = IcuTime::try_new(hour, minute, second, datetime.nanosecond())
            .map_err(|error| format_error(locale, error))?;
        let datetime = IcuDateTime { date, time };
        let preferences = icu_locale(locale)?;
        let field_set = match style {
            DateTimeFormatStyle::Short => YMD::short().with_time_hm(),
            DateTimeFormatStyle::Medium => YMD::medium().with_time_hm(),
            DateTimeFormatStyle::Long => YMD::long().with_time_hm(),
        };
        let formatter = DateTimeFormatter::try_new(preferences.into(), field_set)
            .map_err(|error| format_error(locale, error))?;
        Ok(formatter.format(&datetime).to_string())
    }

    /// 使用显式 locale 执行 future，并覆盖从请求头中选择的 locale。
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

fn icu_locale(locale: &Locale) -> std::result::Result<IcuLocale, I18nFormatError> {
    IcuLocale::from_str(locale.as_str()).map_err(|error| format_error(locale, error))
}

fn format_error(locale: &Locale, error: impl std::fmt::Display) -> I18nFormatError {
    I18nFormatError::Format {
        locale: locale.to_string(),
        reason: error.to_string(),
    }
}

/// 将已配置的国际化服务安装到应用状态中。
#[derive(Default)]
pub struct I18nPlugin {
    config: Option<I18nConfig>,
    catalog: MessageCatalog,
}

impl I18nPlugin {
    /// 使用应用专属消息目录和默认语言创建插件。
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

/// 为当前 HTTP 请求选择的 locale。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLocale(Locale);

impl RequestLocale {
    /// 返回所选的规范 locale 标签。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// 返回所选 locale。
    pub fn locale(&self) -> &Locale {
        &self.0
    }

    /// 使用当前应用的消息目录翻译消息。
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
    fn formats_plural_and_select_messages_from_validated_json() -> Result<()> {
        let mut catalog = MessageCatalog::new();
        catalog.merge_json(
            "en",
            r#"{
                "items": {"plural": {"argument": "count", "forms": {
                    "one": "{count} item", "other": "{count} items"
                } }},
                "choice": {"select": {"argument": "role", "cases": {
                    "admin": "Administrator", "other": "Member"
                } }}
            }"#,
        )?;
        catalog.merge_json(
            "pl",
            r#"{"files": {"plural": {"argument": "count", "forms": {
                "one": "{count} plik", "few": "{count} pliki", "many": "{count} plików", "other": "{count} pliku"
            }}}}"#,
        )?;
        let service = I18n::new(I18nConfig::default(), catalog)?;
        let args = |count: &str| BTreeMap::from([("count".to_owned(), count.to_owned())]);
        assert_eq!(
            service.translate(&Locale::new("en")?, "items", &args("1")),
            "1 item"
        );
        assert_eq!(
            service.translate(&Locale::new("zh-CN")?, "items", &args("1")),
            "1 item"
        );
        assert_eq!(
            service.translate(&Locale::new("en")?, "items", &args("0")),
            "0 items"
        );
        assert_eq!(
            service.translate(&Locale::new("en")?, "items", &args("1.5")),
            "1.5 items"
        );
        assert_eq!(
            service.translate(&Locale::new("en")?, "items", &args("not-a-number")),
            "not-a-number items"
        );
        assert_eq!(
            service.translate(&Locale::new("en")?, "items", &BTreeMap::new()),
            "{count} items"
        );
        assert_eq!(
            service.translate(&Locale::new("pl")?, "files", &args("2")),
            "2 pliki"
        );
        assert_eq!(
            service.translate(&Locale::new("pl")?, "files", &args("5")),
            "5 plików"
        );
        assert_eq!(
            service.translate(
                &Locale::new("en")?,
                "choice",
                &BTreeMap::from([("role".to_owned(), "admin".to_owned())])
            ),
            "Administrator"
        );
        assert_eq!(
            service.translate(&Locale::new("en")?, "choice", &BTreeMap::new()),
            "Member"
        );
        assert_eq!(
            service.translate(
                &Locale::new("en")?,
                "choice",
                &BTreeMap::from([("role".to_owned(), "viewer".to_owned())])
            ),
            "Member"
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_structured_message_resources_atomically() -> Result<()> {
        let mut catalog = MessageCatalog::new();
        assert!(
            catalog
                .merge_json(
                    "en",
                    r#"{"good":"ok","bad":{"plural":{"argument":"n","forms":{"one":"one"}}}}"#
                )
                .is_err()
        );
        assert!(!catalog.messages[&Locale::new("en")?].contains_key("good"));
        assert!(
            catalog
                .merge_json(
                    "en",
                    r#"{"bad":{"select":{"argument":"","cases":{"other":"x"}}}}"#
                )
                .is_err()
        );
        assert!(
            catalog
                .merge_json(
                    "en",
                    r#"{"bad":{"plural":{"argument":"n","forms":{"unknown":"x","other":"y"}}}}"#
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn formats_decimal_and_dates_with_explicit_locale_and_timezone() -> Result<()> {
        use chrono::{FixedOffset, NaiveDate, TimeZone};

        let service = I18n::new(I18nConfig::default(), MessageCatalog::new())?;
        let en = Locale::new("en")?;
        let zh = Locale::new("zh-CN")?;
        let pl = Locale::new("pl")?;
        assert_eq!(
            service
                .format_number(&en, "1234567.50")
                .map_err(Error::internal)?,
            "1,234,567.50"
        );
        assert_eq!(
            service
                .format_number(&zh, "1234567.50")
                .map_err(Error::internal)?,
            "1,234,567.50"
        );
        assert_eq!(
            service
                .format_number(&pl, "1234567.50")
                .map_err(Error::internal)?,
            "1 234 567,50"
        );
        assert!(service.format_number(&en, "1.2.3").is_err());
        let date = NaiveDate::from_ymd_opt(2024, 2, 29)
            .ok_or_else(|| Error::internal(std::io::Error::other("invalid test date")))?;
        let utc = FixedOffset::east_opt(0)
            .ok_or_else(|| Error::internal(std::io::Error::other("invalid test offset")))?;
        let plus_eight = FixedOffset::east_opt(8 * 60 * 60)
            .ok_or_else(|| Error::internal(std::io::Error::other("invalid test offset")))?;
        let instant = utc
            .with_ymd_and_hms(2024, 2, 28, 20, 30, 0)
            .single()
            .ok_or_else(|| Error::internal(std::io::Error::other("invalid test instant")))?;
        let local = instant.with_timezone(&plus_eight);
        assert_ne!(
            service
                .format_date(&en, instant.date_naive(), DateFormatStyle::Long)
                .map_err(Error::internal)?,
            service
                .format_date(&en, local.date_naive(), DateFormatStyle::Long)
                .map_err(Error::internal)?
        );
        assert_ne!(
            service
                .format_datetime(&en, &instant, DateTimeFormatStyle::Medium)
                .map_err(Error::internal)?,
            service
                .format_datetime(&en, &local, DateTimeFormatStyle::Medium)
                .map_err(Error::internal)?
        );
        assert_ne!(
            service
                .format_date(&en, date, DateFormatStyle::Long)
                .map_err(Error::internal)?,
            service
                .format_date(&zh, date, DateFormatStyle::Long)
                .map_err(Error::internal)?
        );
        assert!(service.format_number(&en, "not-a-number").is_err());
        Ok(())
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
