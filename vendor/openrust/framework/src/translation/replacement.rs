use std::{
    any::{Any, TypeId},
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    fmt::{self, Write as _},
    sync::{Arc, RwLock},
};

use super::{PluralCount, TranslationError, TranslationResult, choice_case::titlecase_first};

/// A scalar, tag callback, or typed object used by a translation.
pub struct Replacement<'a>(Kind<'a>);

enum Kind<'a> {
    Text(Cow<'a, str>),
    Tag(Box<dyn Fn(&str) -> String + 'a>),
    Object {
        value: &'a dyn Any,
        display: Option<&'a dyn fmt::Display>,
        name: &'static str,
    },
}

impl<'a> Replacement<'a> {
    /// Receives the original content of each matching tag.
    ///
    /// A tag name uses ASCII letters, digits, underscores, hyphens, or colons.
    pub fn tag(render: impl Fn(&str) -> String + 'a) -> Self {
        Self(Kind::Tag(Box::new(render)))
    }

    /// Uses the formatter registered for the concrete type.
    pub fn object<T: Any>(value: &'a T) -> Self {
        Self(Kind::Object {
            value,
            display: None,
            name: std::any::type_name::<T>(),
        })
    }

    /// Uses the registered formatter when present and otherwise uses `Display`.
    pub fn display<T: Any + fmt::Display>(value: &'a T) -> Self {
        Self(Kind::Object {
            value,
            display: Some(value),
            name: std::any::type_name::<T>(),
        })
    }

    /// Returns a scalar replacement as text.
    pub fn as_text(&self) -> Option<&str> {
        match &self.0 {
            Kind::Text(text) => Some(text),
            Kind::Tag(_) | Kind::Object { .. } => None,
        }
    }

    /// Borrows an object of the specified concrete type.
    pub fn as_object<T: Any>(&self) -> Option<&T> {
        match &self.0 {
            Kind::Object { value, .. } => value.downcast_ref(),
            _ => None,
        }
    }

    /// Borrows a tag callback.
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
        PluralCount::Float(value).to_string().into()
    }
}

impl From<bool> for Replacement<'_> {
    fn from(value: bool) -> Self {
        Self::from(if value { "1" } else { "" })
    }
}

type Formatter = dyn Fn(&dyn Any) -> Option<String> + Send + Sync;

#[derive(Default)]
pub(super) struct Stringables(RwLock<HashMap<TypeId, Arc<Formatter>>>);

impl Stringables {
    pub(super) fn register<T: Any>(&self, render: impl Fn(&T) -> String + Send + Sync + 'static) {
        let previous = self.0.write().unwrap().insert(
            TypeId::of::<T>(),
            Arc::new(move |value| value.downcast_ref::<T>().map(&render)),
        );
        drop(previous);
    }

    pub(super) fn replace(
        &self,
        mut line: String,
        args: &[(&str, Replacement<'_>)],
        count: Option<String>,
    ) -> TranslationResult<String> {
        let mut tokens = BTreeMap::new();
        for (key, replacement) in args {
            match &replacement.0 {
                Kind::Text(text) => insert_tokens(&mut tokens, key, text),
                Kind::Tag(render) => line = replace_tag(line, key, render.as_ref())?,
                Kind::Object {
                    value,
                    display,
                    name,
                } => {
                    let text = self.stringify(*value, *display, name)?;
                    insert_tokens(&mut tokens, key, &text);
                }
            }
        }
        if let Some(count) = count.filter(|_| !args.iter().any(|(key, _)| *key == "count")) {
            insert_tokens(&mut tokens, "count", &count);
        }
        if tokens.is_empty() {
            return Ok(line);
        }
        Ok(replace_tokens(&line, tokens))
    }

    fn stringify(
        &self,
        value: &dyn Any,
        display: Option<&dyn fmt::Display>,
        name: &'static str,
    ) -> TranslationResult<String> {
        let render = self.0.read().unwrap().get(&value.type_id()).cloned();
        if let Some(text) = render.and_then(|render| render(value)) {
            return Ok(text);
        }
        let display = display.ok_or(TranslationError::MissingStringable(name))?;
        let mut text = String::new();
        write!(&mut text, "{display}").map_err(TranslationError::StringableFormat)?;
        Ok(text)
    }
}

pub(super) fn replace(text: &str, replacements: &BTreeMap<String, String>) -> String {
    if replacements.is_empty() {
        return text.to_owned();
    }
    let mut tokens = BTreeMap::new();
    for (key, value) in replacements {
        insert_tokens(&mut tokens, key, value);
    }
    replace_tokens(text, tokens)
}

fn insert_tokens(tokens: &mut BTreeMap<String, String>, key: &str, value: &str) {
    if !key.is_empty() {
        tokens.insert(format!(":{}", titlecase_first(key)), titlecase_first(value));
        tokens.insert(format!(":{}", key.to_uppercase()), value.to_uppercase());
        tokens.insert(format!(":{key}"), value.to_owned());
    }
}

fn replace_tokens(text: &str, tokens: BTreeMap<String, String>) -> String {
    let mut tokens: Vec<_> = tokens.into_iter().collect();
    tokens.sort_by_key(|(key, _)| std::cmp::Reverse(key.len()));
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(':') {
        output.push_str(&rest[..index]);
        rest = &rest[index..];
        if let Some((token, value)) = tokens.iter().find(|(token, _)| rest.starts_with(token)) {
            output.push_str(value);
            rest = &rest[token.len()..];
        } else {
            output.push(':');
            rest = &rest[1..];
        }
    }
    output.push_str(rest);
    output
}

fn replace_tag(
    text: String,
    name: &str,
    render: &dyn Fn(&str) -> String,
) -> TranslationResult<String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
    {
        return Err(TranslationError::InvalidTag);
    }
    let open = format!("<{name}>");
    if !text.contains(&open) {
        return Ok(text);
    }
    let close = format!("</{name}>");
    let mut result = String::with_capacity(text.len());
    // Match the nearest closing tag on the same line, as in the framework callback.
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
