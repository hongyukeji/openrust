//! Laravel 值读取复用已发布的翻译快照。
use super::{
    CatalogLine, CatalogMessage, I18N_CONTEXT, Locale, MessageArgs, MessageCatalog, Translator,
    format_catalog_message, laravel_replacements, replace_laravel_args, scalars,
};
use crate::{Error, Result};
use axum::http::StatusCode;
use serde_json::{Map, Value};
use std::{borrow::Cow, convert::Infallible, sync::Arc};

impl Translator {
    /// 读取字符串、列表或关联映射。缺少资源时返回键本身。
    ///
    /// 未传语言时使用当前请求或任务语言，再使用应用默认语言。全局 JSON 字面键
    /// 仅在请求语言查找。启用 `fallback` 时，缺少分组资源可回退到配置语言。
    /// 参数按传入顺序处理，并递归替换分组值中的 `:name`、`:Name` 和 `:NAME`。
    pub fn get<K: AsRef<str>, V: AsRef<str>>(
        &self,
        key: &str,
        args: impl IntoIterator<Item = (K, V)>,
        locale: Option<&Locale>,
        fallback: bool,
    ) -> Value {
        let locale = self.lookup_locale(locale);
        // 参数来自调用方，求值不能占用共享目录读锁。
        let args = args
            .into_iter()
            .map(|(key, value)| (key.as_ref().to_owned(), value.as_ref().to_owned()))
            .collect::<Vec<_>>();
        let icu_args = args.iter().cloned().collect();
        let replacements = laravel_replacements(args, None);
        let value = self.read_value(key, &locale, fallback, &icu_args);
        if replacements.is_empty() {
            value
        } else {
            map_leaves(value, &mut |leaf| {
                Ok::<_, Infallible>(replace_laravel_args(leaf, &replacements))
            })
            .unwrap_or_else(|never| match never {})
        }
    }

    pub(super) fn read_value(
        &self,
        key: &str,
        locale: &Locale,
        fallback: bool,
        icu_args: &MessageArgs,
    ) -> Value {
        let catalog = self
            .catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.lookup_value(&catalog, locale, key, fallback)
            .map_or_else(
                || Value::String(key.to_owned()),
                |(locale, line, global)| {
                    if global && matches!(line.text(), Some("" | "0")) {
                        Value::String(key.to_owned())
                    } else {
                        line.value(locale, key, icu_args)
                    }
                },
            )
    }

    /// 读取文本；值为列表或关联映射时返回翻译类型错误。
    pub fn string<K: AsRef<str>, V: AsRef<str>>(
        &self,
        key: &str,
        args: impl IntoIterator<Item = (K, V)>,
        locale: Option<&Locale>,
        fallback: bool,
    ) -> Result<String> {
        expect_string(self.get(key, args, locale, fallback))
    }

    /// 读取列表或关联映射；值为文本或资源缺失时返回翻译类型错误。
    pub fn array<K: AsRef<str>, V: AsRef<str>>(
        &self,
        key: &str,
        args: impl IntoIterator<Item = (K, V)>,
        locale: Option<&Locale>,
        fallback: bool,
    ) -> Result<Value> {
        expect_array(self.get(key, args, locale, fallback))
    }

    /// 判断资源是否存在，按 `fallback` 决定是否查询回退语言的分组。
    ///
    /// 精确 JSON 键的空字符串和字符串 `0` 仍算存在。分组文本等于传入键时视为缺失。
    pub fn has(&self, key: &str, locale: Option<&Locale>, fallback: bool) -> bool {
        let locale = self.lookup_locale(locale);
        let catalog = self
            .catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.lookup_value(&catalog, &locale, key, fallback)
            .is_some_and(|(locale, line, global)| {
                global
                    || match line {
                        CatalogLine::Message(message) => {
                            format_catalog_message(locale, message, key, &MessageArgs::new()) != key
                        }
                        CatalogLine::Text(text) => text != key,
                        CatalogLine::Collection(_) | CatalogLine::Group(_) => true,
                    }
            })
    }

    /// 仅判断指定语言或当前语言是否具有该资源。
    pub fn has_for_locale(&self, key: &str, locale: Option<&Locale>) -> bool {
        self.has(key, locale, false)
    }

    pub(super) fn lookup_locale<'a>(&'a self, locale: Option<&'a Locale>) -> Cow<'a, Locale> {
        if let Some(locale) = locale {
            return Cow::Borrowed(locale);
        }
        I18N_CONTEXT
            .try_with(|context| {
                Arc::ptr_eq(&context.i18n.catalog, &self.catalog).then(|| context.locale.clone())
            })
            .ok()
            .flatten()
            .map_or(Cow::Borrowed(&self.config.default_locale), Cow::Owned)
    }

    fn lookup_value<'a>(
        &self,
        catalog: &'a MessageCatalog,
        locale: &Locale,
        key: &str,
        fallback: bool,
    ) -> Option<(&'a Locale, CatalogLine<'a>, bool)> {
        catalog.laravel_line_at(locale, key, true).or_else(|| {
            (fallback && locale != &self.config.fallback_locale)
                .then(|| catalog.laravel_line_at(&self.config.fallback_locale, key, false))
                .flatten()
        })
    }
}

impl CatalogLine<'_> {
    fn value(self, locale: &Locale, key: &str, args: &MessageArgs) -> Value {
        match self {
            Self::Message(message) => message_value(message, locale, key, args),
            Self::Text(text) => Value::String(text.to_owned()),
            Self::Collection(value) => value.clone(),
            Self::Group(lines) => {
                // 加载器的公开资源映射以规范十进制键承载根数组。
                if (0..lines.len()).all(|index| lines.contains_key(&index.to_string())) {
                    return Value::Array(
                        (0..lines.len())
                            .filter_map(|index| lines.get(&index.to_string()))
                            .map(|line| message_value(line, locale, key, args))
                            .collect(),
                    );
                }
                Value::Object(
                    lines
                        .iter()
                        .map(|(item, line)| (item.clone(), message_value(line, locale, key, args)))
                        .collect::<Map<_, _>>(),
                )
            }
        }
    }
}

fn message_value(
    message: &CatalogMessage,
    locale: &Locale,
    key: &str,
    args: &MessageArgs,
) -> Value {
    match message {
        CatalogMessage::Text(text) => Value::String(text.clone()),
        CatalogMessage::Data(value) => value.clone(),
        _ => Value::String(format_catalog_message(locale, message, key, args)),
    }
}

pub(super) fn expect_string(value: Value) -> Result<String> {
    match value {
        Value::String(text) => Ok(text),
        _ => Err(Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "translation_value_not_string",
            "翻译值必须为字符串",
        )),
    }
}

pub(super) fn expect_array(value: Value) -> Result<Value> {
    match value {
        value @ (Value::Array(_) | Value::Object(_)) => Ok(value),
        _ => Err(Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "translation_value_not_array",
            "翻译值必须为数组或关联映射",
        )),
    }
}

pub(super) fn map_leaves<E>(
    value: Value,
    transform: &mut impl FnMut(String) -> std::result::Result<String, E>,
) -> std::result::Result<Value, E> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| map_leaves(item, transform))
            .collect::<std::result::Result<_, _>>()
            .map(Value::Array),
        Value::Object(items) => items
            .into_iter()
            .map(|(key, item)| map_leaves(item, transform).map(|item| (key, item)))
            .collect::<std::result::Result<_, _>>()
            .map(Value::Object),
        Value::String(text) => transform(text).map(Value::String),
        leaf => transform(scalars::string(&leaf)).map(Value::String),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/translation/get_translator.rs"]
mod tests;
