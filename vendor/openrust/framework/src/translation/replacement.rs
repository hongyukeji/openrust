//! Laravel 标签回调和按类型的字符串化。
use super::{
    Locale, MissingReplacements, Translator,
    get::{expect_array, expect_string, map_leaves},
    insert_laravel_arg, replace_laravel_args, scalars,
};
use crate::{Error, Result, support::InputNumber};
use axum::http::StatusCode;
use serde_json::Value;
use std::{
    any::{Any, TypeId},
    borrow::Cow,
    collections::BTreeMap,
    fmt::{self, Write as _},
    sync::{Arc, RwLock},
};

/// 翻译替换值。文本、标签回调和类型化对象可在同一有序参数列表中使用。
pub struct Replacement<'a>(Kind<'a>);

enum Kind<'a> {
    Text(Cow<'a, str>),
    Tag(Box<dyn Fn(&str) -> String + 'a>),
    Object {
        value: &'a dyn Any,
        display: Option<&'a dyn fmt::Display>,
    },
}

impl<'a> Replacement<'a> {
    /// 登记一次调用使用的标签回调，接收每个匹配标签的原始内容。
    ///
    /// 参数键作为字面标签名，仅接受 ASCII 字母、数字、下划线、连字符或冒号。
    pub fn tag(render: impl Fn(&str) -> String + 'a) -> Self {
        Self(Kind::Tag(Box::new(render)))
    }

    /// 借用对象，通过共享译器登记的精确类型处理器生成文本。
    pub fn object<T: Any>(value: &'a T) -> Self {
        Self(Kind::Object {
            value,
            display: None,
        })
    }

    /// 借用可显示的对象。已登记的精确类型处理器优先，缺省使用 `Display`。
    pub fn display<T: Any + fmt::Display>(value: &'a T) -> Self {
        Self(Kind::Object {
            value,
            display: Some(value),
        })
    }

    /// 读取已转换为文本的替换值。
    pub fn as_text(&self) -> Option<&str> {
        match &self.0 {
            Kind::Text(text) => Some(text),
            Kind::Tag(_) | Kind::Object { .. } => None,
        }
    }

    /// 按精确类型借用传入的对象。
    pub fn as_object<T: Any>(&self) -> Option<&T> {
        match &self.0 {
            Kind::Object { value, .. } => value.downcast_ref(),
            _ => None,
        }
    }

    /// 借用传入的标签回调。
    pub fn as_tag(&self) -> Option<&dyn Fn(&str) -> String> {
        match &self.0 {
            Kind::Tag(render) => Some(render.as_ref()),
            _ => None,
        }
    }
}

impl<'a> From<&'a str> for Replacement<'a> {
    fn from(text: &'a str) -> Self {
        Self(Kind::Text(Cow::Borrowed(text)))
    }
}

impl From<String> for Replacement<'_> {
    fn from(text: String) -> Self {
        Self(Kind::Text(Cow::Owned(text)))
    }
}

impl From<i64> for Replacement<'_> {
    fn from(value: i64) -> Self {
        value.to_string().into()
    }
}

impl From<f64> for Replacement<'_> {
    fn from(value: f64) -> Self {
        scalars::double_string(value).into()
    }
}

impl From<bool> for Replacement<'_> {
    fn from(value: bool) -> Self {
        Self::from(if value { "1" } else { "" })
    }
}

type Formatter = dyn Fn(&dyn Any) -> Option<String> + Send + Sync;

#[derive(Default)]
pub(super) struct Stringables(RwLock<BTreeMap<TypeId, Arc<Formatter>>>);

impl fmt::Debug for Stringables {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Stringables")
            .finish_non_exhaustive()
    }
}

impl Translator {
    /// 为精确 Rust 类型登记或替换字符串化处理器。
    ///
    /// 处理器由同一译器的克隆共享。处理器执行及旧处理器析构均在锁外。
    pub fn stringable<T: Any>(&self, render: impl Fn(&T) -> String + Send + Sync + 'static) {
        let previous = self
            .stringables
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                TypeId::of::<T>(),
                Arc::new(move |value| value.downcast_ref::<T>().map(&render)),
            );
        drop(previous);
    }

    /// 读取翻译值，按参数顺序应用标签回调、类型化对象与文本占位符。
    ///
    /// 替换递归访问列表和关联映射的叶值。未登记且未显式提供 `Display` 的对象返回错误。
    /// 读取先取得目录快照，再执行用户回调。
    pub fn get_with<'a, K: AsRef<str>>(
        &self,
        key: &str,
        args: impl IntoIterator<Item = (K, Replacement<'a>)>,
        locale: Option<&Locale>,
        fallback: bool,
    ) -> Result<Value> {
        let locale = self.lookup_locale(locale);
        let args: Vec<_> = args
            .into_iter()
            .map(|(key, value)| (key.as_ref().to_owned(), value))
            .collect();
        let icu_args = args
            .iter()
            .filter_map(|(key, value)| Some((key.clone(), value.as_text()?.to_owned())))
            .collect();
        let value = self.read_value(
            key,
            &locale,
            fallback,
            &icu_args,
            MissingReplacements::Mixed(&args),
        );
        if args.is_empty() {
            Ok(value)
        } else {
            map_leaves(value, &mut |line| {
                self.replace_line(line, args.iter().map(|(key, value)| (key, value)), None)
            })
        }
    }

    /// 使用标签或对象替换读取文本，并校验返回类型。
    pub fn string_with<'a, K: AsRef<str>>(
        &self,
        key: &str,
        args: impl IntoIterator<Item = (K, Replacement<'a>)>,
        locale: Option<&Locale>,
        fallback: bool,
    ) -> Result<String> {
        expect_string(self.get_with(key, args, locale, fallback)?)
    }

    /// 使用标签或对象替换读取列表或关联映射，并校验返回类型。
    pub fn array_with<'a, K: AsRef<str>>(
        &self,
        key: &str,
        args: impl IntoIterator<Item = (K, Replacement<'a>)>,
        locale: Option<&Locale>,
        fallback: bool,
    ) -> Result<Value> {
        expect_array(self.get_with(key, args, locale, fallback)?)
    }

    /// 先选择复数分支，再应用标签、对象及具名参数。可显式覆盖 `count`。
    pub fn choice_with<'a, K: AsRef<str>>(
        &self,
        key: &str,
        count: InputNumber,
        args: impl IntoIterator<Item = (K, Replacement<'a>)>,
        locale: Option<&Locale>,
    ) -> Result<String> {
        let (line, count) = self.select_choice(key, count, locale)?;
        self.replace_line(line, args, Some(&count))
    }

    fn replace_line<'a, K, V>(
        &self,
        mut line: String,
        args: impl IntoIterator<Item = (K, V)>,
        count: Option<&str>,
    ) -> Result<String>
    where
        K: AsRef<str>,
        V: std::borrow::Borrow<Replacement<'a>>,
    {
        let mut replacements = BTreeMap::new();
        let mut has_count = false;
        for (key, replacement) in args {
            let key = key.as_ref();
            has_count |= key == "count";
            match &replacement.borrow().0 {
                Kind::Text(text) => insert_laravel_arg(&mut replacements, key, text),
                Kind::Tag(render) => line = replace_tag(line, key, render.as_ref())?,
                Kind::Object { value, display } => {
                    let text = self.stringify(*value, *display)?;
                    insert_laravel_arg(&mut replacements, key, &text);
                }
            }
        }
        if let Some(count) = count.filter(|_| !has_count) {
            insert_laravel_arg(&mut replacements, "count", count);
        }
        Ok(replace_laravel_args(line, &replacements))
    }

    fn stringify(&self, value: &dyn Any, display: Option<&dyn fmt::Display>) -> Result<String> {
        let render = self
            .stringables
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&value.type_id())
            .cloned();
        if let Some(text) = render.and_then(|render| render(value)) {
            return Ok(text);
        }
        let Some(display) = display else {
            return Err(Error::custom_with_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "translation_object_not_stringable",
                "翻译对象缺少字符串化处理器",
            ));
        };
        let mut text = String::new();
        write!(&mut text, "{display}").map_err(|_| {
            Error::custom_with_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "translation_object_format_failed",
                "翻译对象字符串化失败",
            )
        })?;
        Ok(text)
    }
}

fn replace_tag(text: String, name: &str, render: &dyn Fn(&str) -> String) -> Result<String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
    {
        return Err(Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "translation_tag_invalid",
            "翻译标签名称无效",
        ));
    }
    let open = format!("<{name}>");
    if !text.contains(&open) {
        return Ok(text);
    }
    let close = format!("</{name}>");
    let mut result = String::with_capacity(text.len());
    // PHP 默认点匹配不越过 LF。最近的闭标签结束当前非重叠匹配。
    for segment in text.split_inclusive('\n') {
        let mut remaining = segment;
        while let Some(start) = remaining.find(&open) {
            let content = &remaining[start + open.len()..];
            let Some(end) = content.find(&close) else {
                break;
            };
            result.push_str(&remaining[..start]);
            result.push_str(&render(&content[..end]));
            remaining = &content[end + close.len()..];
        }
        result.push_str(remaining);
    }
    Ok(result)
}

#[cfg(test)]
#[path = "../../tests/unit/translation/replacements.rs"]
mod tests;
