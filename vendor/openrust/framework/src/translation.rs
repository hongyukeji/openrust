//! 请求作用域的消息目录与 locale 选择。

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

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

use crate::config::{AppConfig, ConfigError};
use crate::core::{Error, Result};
use crate::support::{InputNumber, scalars};

mod choice;
mod choice_case;
mod composite_loader;
mod file_loader;
mod get;
mod loader;
mod lookup;
mod replacement;
mod source_refresh;
pub use composite_loader::CompositeTranslationLoader;
pub use file_loader::FileTranslationLoader;
pub use loader::{
    TranslationLines, TranslationLoadSession, TranslationLoader, TranslationResource,
};
pub use lookup::{MissingReplacements, MissingTranslation};
pub use replacement::Replacement;
pub use source_refresh::TranslationRefreshOutcome;
pub use source_refresh::TranslationRefreshStatus;
pub use source_refresh::parse_translation_lines;
pub use source_refresh::translation_session;
use source_refresh::{PublishedCatalog, RefreshRuntime};

/// 应用消息目录使用的规范语言标签。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Locale(String);

impl Locale {
    pub(crate) fn english() -> Self {
        Self("en".to_owned())
    }

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
pub enum TranslationFormatError {
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
    Data(serde_json::Value),
    Plural {
        argument: String,
        forms: BTreeMap<PluralCategory, String>,
    },
    Select {
        argument: String,
        cases: BTreeMap<String, String>,
    },
}

#[derive(Clone, Copy)]
enum CatalogLine<'a> {
    Message(&'a CatalogMessage),
    Text(&'a str),
    Collection(&'a serde_json::Value),
    Group(&'a BTreeMap<String, CatalogMessage>),
}

impl<'a> CatalogLine<'a> {
    fn grouped(message: &'a CatalogMessage) -> Option<Self> {
        match message {
            CatalogMessage::Data(value) => Self::from_value(value),
            _ => Some(Self::Message(message)),
        }
    }

    fn from_value(value: &'a serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::String(text) => Some(Self::Text(text)),
            serde_json::Value::Object(items) if !items.is_empty() => Some(Self::Collection(value)),
            serde_json::Value::Array(items) if !items.is_empty() => Some(Self::Collection(value)),
            _ => None,
        }
    }

    fn text(self) -> Option<&'a str> {
        match self {
            Self::Message(CatalogMessage::Text(text)) => Some(text),
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    fn format(self, locale: &Locale, key: &str, args: &MessageArgs) -> String {
        match self {
            Self::Message(message) => format_catalog_message(locale, message, key, args),
            Self::Text(text) => interpolate(text, args),
            Self::Collection(_) | Self::Group(_) => interpolate(key, args),
        }
    }
}

fn grouped_line<'a>(
    lines: &'a BTreeMap<String, CatalogMessage>,
    item: &str,
) -> Option<CatalogLine<'a>> {
    if let Some(message) = lines.get(item) {
        return CatalogLine::grouped(message);
    }
    let (head, tail) = item.split_once('.')?;
    let CatalogMessage::Data(value) = lines.get(head)? else {
        return None;
    };
    let mut value = value;
    for segment in tail.split('.') {
        value = match value {
            serde_json::Value::Object(items) => items.get(segment)?,
            serde_json::Value::Array(items) => {
                if !segment.bytes().all(|byte| byte.is_ascii_digit())
                    || (segment.starts_with('0') && segment.len() > 1)
                {
                    return None;
                }
                items.get(segment.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    CatalogLine::from_value(value)
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
    resource_messages: BTreeMap<TranslationResource, BTreeMap<String, CatalogMessage>>,
    source_global_keys: BTreeMap<Locale, std::collections::BTreeSet<String>>,
}

impl Default for MessageCatalog {
    fn default() -> Self {
        Self {
            resource_messages: BTreeMap::new(),
            source_global_keys: BTreeMap::new(),
            messages: BTreeMap::from([
                (Locale::english(), framework_messages(EN_MESSAGES, false)),
                (
                    Locale("zh-CN".to_owned()),
                    framework_messages(ZH_CN_MESSAGES, true),
                ),
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

    fn message(&self, locale: &Locale, key: &str) -> Option<(&Locale, CatalogLine<'_>)> {
        self.message_at(locale, key)
            .or_else(|| self.message_at(&Locale(locale.language().to_owned()), key))
            .or_else(|| {
                if self.messages.contains_key(locale) {
                    return None;
                }
                self.messages
                    .keys()
                    .find(|tag| tag.language() == locale.language())
                    .and_then(|tag| self.message_at(tag, key))
            })
    }

    fn message_at(&self, locale: &Locale, key: &str) -> Option<(&Locale, CatalogLine<'_>)> {
        let (locale, global) = self.messages.get_key_value(locale)?;
        // 固定Translator::get先查完整JSON字面键，再解析group/namespace；不可平铺覆盖。
        if let Some(message) = global.get(key) {
            return Some((locale, CatalogLine::Message(message)));
        }
        self.group_message_at(locale, key)
    }

    fn group_message_at(&self, locale: &Locale, key: &str) -> Option<(&Locale, CatalogLine<'_>)> {
        self.group_line_at(locale, key, false)
    }

    fn group_line_at(
        &self,
        locale: &Locale,
        key: &str,
        whole_group: bool,
    ) -> Option<(&Locale, CatalogLine<'_>)> {
        let (locale, _) = self.messages.get_key_value(locale)?;
        let (namespace, item) = key
            .split_once("::")
            .map_or((None, key), |(namespace, key)| (Some(namespace), key));
        if let Some(namespace) = namespace {
            let resource = TranslationResource {
                locale: locale.clone(),
                group: "*".into(),
                namespace: Some(namespace.into()),
            };
            if let Some(message) = self
                .resource_messages
                .get(&resource)
                .and_then(|lines| lines.get(item))
            {
                return Some((locale, CatalogLine::Message(message)));
            }
        }
        let (group, item) = item
            .split_once('.')
            .map_or((item, None), |(group, item)| (group, Some(item)));
        let resource = TranslationResource {
            locale: locale.clone(),
            group: group.into(),
            namespace: namespace.map(str::to_owned),
        };
        let lines = self.resource_messages.get(&resource)?;
        let line = match item {
            Some(item) => grouped_line(lines, item)?,
            None if whole_group && !lines.is_empty() => CatalogLine::Group(lines),
            None => return None,
        };
        Some((locale, line))
    }

    fn json_line_at(&self, locale: &Locale, key: &str) -> Option<(&Locale, CatalogLine<'_>)> {
        let (locale, global) = self.messages.get_key_value(locale)?;
        Some((locale, CatalogLine::Message(global.get(key)?)))
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

fn framework_messages(entries: &[(&str, &str)], chinese: bool) -> BTreeMap<String, CatalogMessage> {
    let mut result = messages(entries);
    let (one, other) = if chinese {
        (
            "{first}（另有 {count} 项错误）",
            "{first}（另有 {count} 项错误）",
        )
    } else {
        (
            "{first} (and {count} more error)",
            "{first} (and {count} more errors)",
        )
    };
    result.insert(
        "openrust.validation.summary.more".into(),
        CatalogMessage::Plural {
            argument: "count".into(),
            forms: BTreeMap::from([
                (PluralCategory::One, one.into()),
                (PluralCategory::Other, other.into()),
            ]),
        },
    );
    result
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

/// 显式构造翻译服务时使用的默认和回退语言。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationConfig {
    /// 请求中没有受支持的语言时使用的 locale。
    pub default_locale: Locale,
    /// 标准查找在请求语言之后使用最终回退。目录视图可先查语言级资源。
    pub fallback_locale: Locale,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            default_locale: Locale::english(),
            fallback_locale: Locale::english(),
        }
    }
}

/// 标准模式优先匹配 JSON 字面键。目录视图提供额外的回退策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TranslationLookup {
    /// JSON仅查请求locale；空串或"0"命中后返回原key，group再按locale回退。
    #[default]
    Standard,
    /// 显式保留全局Catalog跨locale回退及空串/"0"原样返回的增强。
    Catalog,
}

/// 可在多个请求间安全共享的应用国际化服务。
#[derive(Debug, Clone)]
pub struct Translator {
    config: TranslationConfig,
    lookup: TranslationLookup,
    catalog: Arc<RwLock<PublishedCatalog>>,
    source: Option<Arc<RefreshRuntime>>,
    stringables: Arc<replacement::Stringables>,
    lookup_hooks: Arc<lookup::LookupHooks>,
}

impl Translator {
    /// 创建服务，并校验两个已配置的默认 locale 均存在资源。
    pub fn new(config: TranslationConfig, catalog: MessageCatalog) -> Result<Self, ConfigError> {
        for (key, locale) in [
            ("app.locale", &config.default_locale),
            ("app.fallback_locale", &config.fallback_locale),
        ] {
            if !catalog.supports(locale) {
                return Err(ConfigError::Invalid(format!(
                    "{key}: locale `{locale}` has no message resource"
                )));
            }
        }
        Ok(Self {
            config,
            lookup: TranslationLookup::Standard,
            catalog: Arc::new(RwLock::new(PublishedCatalog::new(catalog))),
            source: None,
            stringables: Arc::default(),
            lookup_hooks: Arc::default(),
        })
    }

    /// 在模块 boot 时登记语言资源；发布目录存在时完全取代模块源目录。
    ///
    /// 资源格式为目录下的 `<locale>.json`，键既可用 `namespace::`，也可作为 JSON 全局键查询。
    pub fn add_namespace(
        &self,
        namespace: &str,
        source_dir: impl AsRef<Path>,
        published_dir: impl AsRef<Path>,
    ) -> Result<()> {
        if namespace.is_empty()
            || !namespace
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(Error::custom(
                "i18n_namespace_invalid",
                "语言资源命名空间无效",
            ));
        }
        let selected_dir = match published_dir.as_ref().symlink_metadata() {
            Ok(_) => published_dir.as_ref(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => source_dir.as_ref(),
            Err(_) => {
                return Err(Error::custom(
                    "i18n_namespace_source_invalid",
                    "语言资源目录不可读取",
                ));
            }
        };
        let selected = read_namespace_messages(selected_dir)?;
        let mut overlay = source_refresh::empty_catalog();
        for (locale, content) in selected {
            let entries = content.as_object().ok_or_else(|| {
                Error::custom("i18n_namespace_json_invalid", "语言资源JSON必须是对象")
            })?;
            let mut merged = entries.clone();
            for (key, value) in entries {
                merged.insert(format!("{namespace}::{key}"), value.clone());
            }
            let content = serde_json::to_string(&merged)
                .map_err(|_| Error::custom("i18n_namespace_json_invalid", "语言资源JSON无效"))?;
            overlay.merge_json(locale, &content)?;
        }
        self.merge_namespace(namespace, &overlay)
    }

    /// 根据 `Accept-Language` 请求头选择最匹配的受支持 locale。
    pub fn select(&self, accept_language: Option<&str>) -> Locale {
        let catalog = self
            .catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.select_from_catalog(&catalog, accept_language)
    }

    fn select_from_catalog(
        &self,
        catalog: &MessageCatalog,
        accept_language: Option<&str>,
    ) -> Locale {
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
                if candidate == self.config.default_locale && catalog.supports(&candidate) {
                    return Some(candidate);
                }
                catalog
                    .messages
                    .get_key_value(&candidate)
                    .map(|(locale, _)| locale.clone())
                    .or_else(|| {
                        catalog
                            .messages
                            .keys()
                            .find(|supported| supported.as_str() == candidate.language())
                            .cloned()
                    })
                    .or_else(|| {
                        catalog
                            .messages
                            .keys()
                            .find(|supported| supported.language() == candidate.language())
                            .cloned()
                    })
            })
            .unwrap_or_else(|| self.config.default_locale.clone())
    }

    /// 为当前服务句柄显式选择查找增强；不改变共享目录、来源或默认语言。
    #[must_use]
    pub fn with_lookup(mut self, lookup: TranslationLookup) -> Self {
        self.lookup = lookup;
        self
    }

    /// 按传入locale与所选lookup策略解析key，再替换`{name}`参数；HTTP协商在请求入口完成。
    pub fn translate(&self, locale: &Locale, key: &str, args: &MessageArgs) -> String {
        self.try_translate(locale, key, args)
            .unwrap_or_else(|| interpolate(key, args))
    }

    /// 按数值区间和语言的复数规则选择字符串资源的一个分支。
    ///
    /// 先查指定语言，缺少键时改用配置的回退语言及其复数规则。未指定语言时，
    /// 优先使用同一译器的请求或任务语言，其次使用应用默认语言。
    /// 选定分支后替换 `:count` 与具名参数。参数按迭代顺序登记。
    /// 非有限数、无法执行复数规则的数值或结构化消息返回错误。
    pub fn choice<K: AsRef<str>, V: AsRef<str>>(
        &self,
        key: &str,
        count: InputNumber,
        args: impl IntoIterator<Item = (K, V)>,
        locale: Option<&Locale>,
    ) -> Result<String> {
        let (selected, count_text) = self.select_choice(key, count, locale)?;
        if selected.contains(':') {
            let replacements = named_replacements(args, Some(&count_text));
            Ok(replace_named_placeholders(selected, &replacements))
        } else {
            Ok(selected)
        }
    }

    fn select_choice(
        &self,
        key: &str,
        count: InputNumber,
        locale: Option<&Locale>,
    ) -> Result<(String, String)> {
        let count_text = match count {
            InputNumber::Integer(number) => number.to_string(),
            InputNumber::Float(number) if number.is_finite() => scalars::double_string(number),
            InputNumber::Float(_) => {
                return Err(Error::custom(
                    "translation_choice_count_invalid",
                    "翻译数量必须是有限数值",
                ));
            }
        };
        let locale = self.lookup_locale(locale);
        let catalog = self.lookup_snapshot();
        // 先按原语言的精确存在性选择复数语言，再在该语言执行一次普通读取。
        let locale = if self
            .lookup_value(&catalog, &locale, key, false)
            .is_some_and(|(_, line, global)| global || line.text() != Some(key))
        {
            locale.as_ref()
        } else {
            &self.config.fallback_locale
        };
        let line = match self.lookup_value(&catalog, locale, key, true) {
            Some((_, message, global)) => {
                let text = message.text().ok_or_else(|| {
                    Error::custom_with_status(
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "translation_choice_message_invalid",
                        "复数选择要求字符串消息",
                    )
                })?;
                if global && (text.is_empty() || text == "0") {
                    Cow::Borrowed(key)
                } else {
                    Cow::Borrowed(text)
                }
            }
            None => self.missing_text(key, locale, true, MissingReplacements::Text(&[])),
        };
        let selected = choice::select(&line, count, locale)?.to_owned();
        Ok((selected, count_text))
    }

    fn try_translate(&self, locale: &Locale, key: &str, args: &MessageArgs) -> Option<String> {
        let catalog = self
            .catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // 显式translator调用保留原请求locale；HTTP入口已负责Accept-Language协商。
        let selected;
        let locale = match self.lookup {
            TranslationLookup::Standard => locale,
            TranslationLookup::Catalog => {
                selected = self.select_from_catalog(&catalog, Some(locale.as_str()));
                &selected
            }
        };
        let message = match self.lookup {
            TranslationLookup::Catalog => catalog
                .message(locale, key)
                .or_else(|| catalog.message(&self.config.fallback_locale, key)),
            TranslationLookup::Standard => {
                if let Some((message_locale, global)) = catalog.messages.get_key_value(locale)
                    && let Some(message) = global.get(key)
                {
                    if matches!(message,CatalogMessage::Text(text) if text.is_empty() || text=="0")
                    {
                        return Some(interpolate(key, args));
                    }
                    return Some(format_catalog_message(message_locale, message, key, args));
                }
                catalog
                    .group_message_at(locale, key)
                    .or_else(|| catalog.group_message_at(&self.config.fallback_locale, key))
            }
        };
        message.map(|(message_locale, message)| message.format(message_locale, key, args))
    }

    /// 按 locale 格式化精确十进制字符串，不经过浮点数或舍入。
    pub fn format_number(
        &self,
        locale: &Locale,
        decimal: &str,
    ) -> std::result::Result<String, TranslationFormatError> {
        let value =
            Decimal::from_str(decimal).map_err(|_| TranslationFormatError::InvalidNumber {
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
    ) -> std::result::Result<String, TranslationFormatError> {
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
    ) -> std::result::Result<String, TranslationFormatError> {
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
        let locale = self.select(Some(locale.as_str()));
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

    /// 在后台任务或无 HTTP 请求的代码中使用此应用配置的默认语言。
    pub async fn with_default_locale<F: Future>(&self, future: F) -> F::Output {
        self.with_locale(self.select(None), future).await
    }

    pub(crate) fn locale_for_request(&self, accept_language: Option<&str>) -> Locale {
        self.select(accept_language)
    }
}

fn format_catalog_message(
    message_locale: &Locale,
    message: &CatalogMessage,
    key: &str,
    args: &MessageArgs,
) -> String {
    match message {
        CatalogMessage::Text(template) => interpolate(template, args),
        CatalogMessage::Data(_) => interpolate(key, args),
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
    }
}

fn read_namespace_messages(directory: &Path) -> Result<Vec<(String, serde_json::Value)>> {
    let Ok(metadata) = directory.symlink_metadata() else {
        return Err(Error::custom(
            "i18n_namespace_source_invalid",
            "语言资源目录不可读取",
        ));
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::custom(
            "i18n_namespace_source_invalid",
            "语言资源路径不是目录",
        ));
    }
    let mut files = std::fs::read_dir(directory)
        .map_err(|_| Error::custom("i18n_namespace_source_invalid", "语言资源目录不可读取"))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|_| Error::custom("i18n_namespace_source_invalid", "语言资源目录项不可读取"))?;
    files.sort_by_key(std::fs::DirEntry::file_name);
    let mut result = Vec::new();
    for file in files {
        let path = file.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let metadata = path
            .symlink_metadata()
            .map_err(|_| Error::custom("i18n_namespace_source_invalid", "语言资源文件不可读取"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Error::custom(
                "i18n_namespace_source_invalid",
                "语言资源文件无效",
            ));
        }
        let locale = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| Error::custom("i18n_namespace_locale_invalid", "语言资源文件名无效"))?;
        let locale = Locale::new(locale)?.to_string();
        let content = std::fs::read_to_string(&path)
            .map_err(|_| Error::custom("i18n_namespace_source_invalid", "语言资源文件不可读取"))?;
        let raw: serde_json::Value = serde_json::from_str(&content)
            .map_err(|_| Error::custom("i18n_namespace_json_invalid", "语言资源 JSON 无效"))?;
        raw.as_object().ok_or_else(|| {
            Error::custom("i18n_namespace_json_invalid", "语言资源 JSON 必须是对象")
        })?;
        result.push((locale, raw));
    }
    Ok(result)
}

fn icu_locale(locale: &Locale) -> std::result::Result<IcuLocale, TranslationFormatError> {
    IcuLocale::from_str(locale.as_str()).map_err(|error| format_error(locale, error))
}

fn format_error(locale: &Locale, error: impl std::fmt::Display) -> TranslationFormatError {
    TranslationFormatError::Format {
        locale: locale.to_string(),
        reason: error.to_string(),
    }
}

type LoaderFactory = Arc<
    dyn Fn(
            crate::container::Resolver,
        ) -> crate::support::BoxFuture<'static, Result<Arc<dyn TranslationLoader>>>
        + Send
        + Sync,
>;

/// 注册配置驱动的国际化服务；应用可选择静态目录或显式异步Loader工厂。
#[derive(Default)]
pub struct TranslationServiceProvider {
    catalog: MessageCatalog,
    loader_factory: Option<LoaderFactory>,
}

impl TranslationServiceProvider {
    /// 使用应用的消息目录；语言配置始终从当前配置仓储读取。
    pub fn new(catalog: MessageCatalog) -> Self {
        Self {
            catalog,
            loader_factory: None,
        }
    }

    /// 显式选择异步Loader工厂；配置和来源在同一I18n单例首次构造时读取。
    ///
    /// 工厂及初始快照失败直接沿启动返回，不静默切换来源，不登记全局Loader或后台任务。
    /// Provider类型身份与new/default相同，可用于`DefaultProviders::replace`。
    /// 单例通常在本Provider的boot首次解析；此后其他boot对配置的覆盖不追溯重建已有单例，
    /// 工厂读取的是首次构造时的视图，不承诺所有Provider启动完成后的最终视图。
    pub fn from_loader_factory<F>(factory: F) -> Self
    where
        F: Fn(
                crate::container::Resolver,
            )
                -> crate::support::BoxFuture<'static, Result<Arc<dyn TranslationLoader>>>
            + Send
            + Sync
            + 'static,
    {
        Self {
            catalog: MessageCatalog::default(),
            loader_factory: Some(Arc::new(factory)),
        }
    }
}

impl crate::support::ServiceProvider for TranslationServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let catalog = self.catalog.clone();
            let loader_factory = self.loader_factory.clone();
            app.singleton::<Translator, _>(move |resolver| {
                let catalog = catalog.clone();
                let loader_factory = loader_factory.clone();
                Box::pin(async move {
                    let application = match resolver.section::<AppConfig>("app") {
                        Ok(application) => application,
                        Err(ConfigError::Source(error)) if error.missing() => AppConfig::default(),
                        Err(error) => return Err(error.into()),
                    };
                    let config = TranslationConfig {
                        default_locale: application.locale,
                        fallback_locale: application.fallback_locale,
                    };
                    let service = match loader_factory {
                        Some(factory) => {
                            Translator::from_loader(config, factory(resolver).await?).await?
                        }
                        None => Translator::new(config, catalog)?,
                    };
                    Ok(Arc::new(service))
                })
            })
            .await?;
            app.require::<Translator>();
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let service = app.resolve::<Translator>().await?;
            app.exceptions(move |handler| handler.set_i18n((*service).clone()))?;
            Ok(())
        })
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
    pub fn translate(&self, i18n: &Translator, key: &str, args: &MessageArgs) -> String {
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
    i18n: Translator,
    locale: Locale,
}

tokio::task_local! {
    static I18N_CONTEXT: I18nContext;
}

#[cfg(test)]
pub(crate) async fn with_request_locale<F: Future>(
    i18n: Translator,
    accept_language: Option<&str>,
    future: F,
) -> F::Output {
    // 异步边界有界核对来源；失败状态已记录，继续使用上次已验证目录。
    let _ = i18n.refresh_if_due().await;
    let locale = i18n.locale_for_request(accept_language);
    i18n.with_locale(locale, future).await
}

// 验证入口使用目录视图。公开文本入口默认使用标准查找。
pub(crate) fn translate_current(key: &str, args: &MessageArgs) -> Option<String> {
    I18N_CONTEXT
        .try_with(|context| {
            context
                .i18n
                .clone()
                .with_lookup(TranslationLookup::Catalog)
                .translate(&context.locale, key, args)
        })
        .ok()
}

// 仅为服务尚未初始化时使用的安全内置目录，不解析容器或建立第二个应用服务。
static BOOTSTRAP_MESSAGES: std::sync::LazyLock<MessageCatalog> =
    std::sync::LazyLock::new(MessageCatalog::default);

pub(crate) fn bootstrap_translation(key: &str, args: &MessageArgs) -> Option<String> {
    let locale = Locale::english();
    BOOTSTRAP_MESSAGES
        .message(&locale, key)
        .map(|(locale, message)| message.format(locale, key, args))
}

pub(crate) fn translate_for_rendering(
    service: Option<&Translator>,
    key: &str,
    args: &MessageArgs,
) -> Option<String> {
    // ADR统一错误协议显式保留平面Catalog的语言级/最终回退与合法空串；不改公开translator默认。
    match I18N_CONTEXT.try_with(|context| {
        context
            .i18n
            .clone()
            .with_lookup(TranslationLookup::Catalog)
            .try_translate(&context.locale, key, args)
    }) {
        Ok(message) => message,
        Err(_) => service.map_or_else(
            || bootstrap_translation(key, args),
            |service| {
                service
                    .clone()
                    .with_lookup(TranslationLookup::Catalog)
                    .try_translate(&service.select(None), key, args)
            },
        ),
    }
}

pub(crate) fn rendering_locale(service: Option<&Translator>) -> Locale {
    I18N_CONTEXT
        .try_with(|context| context.locale.clone())
        .unwrap_or_else(|_| service.map_or_else(Locale::english, |service| service.select(None)))
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

fn named_replacements<K: AsRef<str>, V: AsRef<str>>(
    args: impl IntoIterator<Item = (K, V)>,
    count: Option<&str>,
) -> BTreeMap<String, String> {
    let mut replacements = BTreeMap::new();
    let mut has_count = false;
    for (key, value) in args {
        let key = key.as_ref();
        has_count |= key == "count";
        insert_named_replacement(&mut replacements, key, value.as_ref());
    }
    if let Some(count) = count.filter(|_| !has_count) {
        insert_named_replacement(&mut replacements, "count", count);
    }
    replacements
}

fn insert_named_replacement(replacements: &mut BTreeMap<String, String>, key: &str, value: &str) {
    replacements.insert(
        format!(":{}", choice_case::titlecase_first(key)),
        choice_case::titlecase_first(value),
    );
    replacements.insert(format!(":{}", key.to_uppercase()), value.to_uppercase());
    replacements.insert(format!(":{key}"), value.to_owned());
}

fn replace_named_placeholders(template: String, replacements: &BTreeMap<String, String>) -> String {
    if replacements.is_empty() || !template.contains(':') {
        return template;
    }
    let mut output = String::with_capacity(template.len());
    let mut remaining = template.as_str();
    while let Some(start) = remaining.find(':') {
        output.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        // 在同一位置可匹配的两个键必为前缀关系。逆序先匹配较长键，替换值不再扫描。
        if let Some((key, value)) = replacements
            .iter()
            .rev()
            .find(|(key, _)| remaining.starts_with(key.as_str()))
        {
            output.push_str(value);
            remaining = &remaining[key.len()..];
        } else {
            output.push(':');
            remaining = &remaining[1..];
        }
    }
    output.push_str(remaining);
    output
}

#[derive(Debug, thiserror::Error)]
#[error("request locale was not initialized by HTTP middleware")]
struct MissingRequestLocale;

const EN_MESSAGES: &[(&str, &str)] = &[
    ("openrust.response.ok", "ok"),
    ("openrust.error.title", "Error"),
    (
        "openrust.validation.summary.empty",
        "The given data was invalid.",
    ),
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
    ("openrust.error.title", "错误"),
    ("openrust.validation.summary.empty", "输入数据无效。"),
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
        let service = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
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
        let service = Translator::new(TranslationConfig::default(), catalog)?
            .with_lookup(TranslationLookup::Catalog);
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
        assert_eq!(
            service.translate(&locale, "fallback.only", &args),
            "fallback"
        );
        assert_eq!(interpolate("{missing} {name}", &args), "{missing} 小明");
        Ok(())
    }

    #[test]
    fn request_locale_isolated_between_concurrent_scopes() -> Result<()> {
        let service = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
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
        let default_service = Translator::new(TranslationConfig::default(), catalog)?;
        let args = |count: &str| BTreeMap::from([("count".to_owned(), count.to_owned())]);
        // 官方JSON查找仅请求locale；ICU格式化与跨locale Catalog增强是两层独立能力。
        assert_eq!(
            default_service.translate(&Locale::new("en")?, "items", &args("1")),
            "1 item"
        );
        assert_eq!(
            default_service.translate(&Locale::new("zh-CN")?, "items", &args("1")),
            "items"
        );
        // 复用同一目录显式选择既有增强，保留全部plural/select断言。
        let service = default_service
            .clone()
            .with_lookup(TranslationLookup::Catalog);
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

        let service = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
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
}

#[cfg(test)]
#[path = "../tests/unit/translation/rendering_source.rs"]
mod rendering_source_tests;

#[cfg(test)]
#[path = "../tests/unit/translation/loader_provider.rs"]
mod loader_provider_tests;

#[cfg(test)]
#[path = "../tests/unit/translation/application_locales.rs"]
mod provider_tests;

#[cfg(test)]
#[path = "../tests/unit/translation/choice_translator.rs"]
mod choice_translator_tests;
