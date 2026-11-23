use super::EnvironmentFileError as Error;
use adotenv::{Dialect, Document, DoubleEscapes, QuotedTail, ReferenceRules, UnknownEscapes};
use std::{collections::BTreeMap, ops::Range};
use zeroize::Zeroizing;

fn dialect() -> Dialect {
    Dialect::PORTABLE
        .with_double_escapes(
            DoubleEscapes::LF
                .union(DoubleEscapes::BACKSLASH)
                .union(DoubleEscapes::DOUBLE_QUOTE)
                .union(DoubleEscapes::DOLLAR)
                .union(DoubleEscapes::SINGLE_QUOTE),
        )
        .with_unknown_escapes(UnknownEscapes::Error)
        .with_quoted_tail(QuotedTail::Append)
}
fn parse(text: &str) -> Result<Document<'_>, Error> {
    adotenv::parse_strict_with(text, dialect()).map_err(|_| Error::InvalidDocument)
}
pub(super) fn values(text: &str) -> Result<BTreeMap<String, String>, Error> {
    dotenvy::from_read_iter(text.trim_start_matches('\u{feff}').as_bytes())
        .map(|entry| entry.map_err(|_| Error::InvalidDocument))
        .collect()
}
pub(super) fn validate(text: &str) -> Result<(), Error> {
    if text.len() > 8 * 1024 * 1024 {
        return Err(Error::TooLarge);
    }
    if text.contains('\0') {
        return Err(Error::InvalidDocument);
    }
    parse(text)?;
    values(text)?;
    Ok(())
}
fn key(key: &str) -> Result<(), Error> {
    let mut chars = key.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
    {
        return Err(Error::InvalidKey { key: key.into() });
    }
    Ok(())
}
pub(super) fn unique(text: &str, name: &str) -> Result<Option<Range<usize>>, Error> {
    key(name)?;
    let doc = parse(text)?;
    let mut entries = doc.entries().filter(|entry| entry.key() == name);
    let first = entries.next().map(|entry| entry.line_span());
    if entries.next().is_some() {
        return Err(Error::DuplicateKey { key: name.into() });
    }
    Ok(first)
}
pub(super) fn raw(text: &str, name: &str) -> Result<Option<String>, Error> {
    unique(text, name)?;
    let doc = parse(text)?;
    Ok(doc
        .entries()
        .find(|entry| entry.key() == name)
        .and_then(|entry| entry.raw_value().map(str::to_owned)))
}
fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}
fn encode(value: &str, expression: bool) -> String {
    if value.is_empty() {
        return String::new();
    }
    if !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_./:+@%=-,".contains(&c))
    {
        return value.into();
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' if !expression => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
pub(super) fn set(
    text: &str,
    name: &str,
    value: &str,
    expression: bool,
) -> Result<Zeroizing<String>, Error> {
    unique(text, name)?;
    if value.contains('\0') {
        return Err(Error::InvalidValue { key: name.into() });
    }
    let doc = parse(text)?;
    let entry = doc.entries().find(|entry| entry.key() == name);
    if let Some(entry) = entry {
        let expands = entry
            .references()
            .iter()
            .any(|reference| reference.is_eligible(ReferenceRules::PORTABLE));
        if !expression && !expands && values(text)?.get(name).is_some_and(|old| old == value) {
            return Ok(Zeroizing::new(text.into()));
        }
    }
    let encoded = Zeroizing::new(encode(value, expression));
    let mut out = Zeroizing::new(text.to_owned());
    if let Some(entry) = entry {
        let span = entry.value_span().ok_or(Error::InvalidDocument)?;
        out.replace_range(span, &encoded);
    } else {
        let nl = newline(text);
        if !out.trim_start_matches('\u{feff}').is_empty() && !out.ends_with('\n') {
            out.push_str(nl);
        }
        out.push_str(name);
        out.push('=');
        out.push_str(&encoded);
        out.push_str(nl);
    }
    validate(&out)?;
    if !expression && values(&out)?.get(name).map(String::as_str) != Some(value) {
        return Err(Error::InvalidValue { key: name.into() });
    }
    Ok(out)
}
pub(super) fn remove(text: &str, name: &str) -> Result<Zeroizing<String>, Error> {
    unique(text, name)?;
    let doc = parse(text)?;
    let mut out = Zeroizing::new(text.to_owned());
    if let Some(entry) = doc.entries().find(|entry| entry.key() == name) {
        let mut span = entry.line_span();
        span.start = span.start.max(bom_len(text));
        let line = &text[span.clone()];
        let replacement = if let Some(comment) = entry.inline_comment() {
            let indent = &text[entry.tokens().indent.clone()];
            let nl = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            format!("{indent}{comment}{nl}")
        } else {
            String::new()
        };
        out.replace_range(span, &replacement);
    }
    validate(&out)?;
    Ok(out)
}

/// 独立的内存环境文档，不依赖应用容器、文件系统或环境文件名。
#[derive(Clone, Default)]
pub struct EnvironmentDocument {
    pub(super) text: Zeroizing<String>,
}
impl std::fmt::Debug for EnvironmentDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvironmentDocument")
            .finish_non_exhaustive()
    }
}
impl EnvironmentDocument {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn parse(text: impl AsRef<str>) -> Result<Self, Error> {
        validate(text.as_ref())?;
        Ok(Self {
            text: Zeroizing::new(text.as_ref().to_owned()),
        })
    }
    pub fn content(&self) -> &str {
        &self.text
    }
    pub fn get(&self, name: &str) -> Result<Option<String>, Error> {
        unique(&self.text, name)?;
        Ok(values(&self.text)?.remove(name))
    }
    pub fn get_raw(&self, name: &str) -> Result<Option<String>, Error> {
        raw(&self.text, name)
    }
    pub fn contains(&self, name: &str) -> Result<bool, Error> {
        Ok(unique(&self.text, name)?.is_some())
    }
    pub fn all(&self) -> Result<BTreeMap<String, String>, Error> {
        values(&self.text)
    }
    pub fn set(&mut self, name: &str, value: impl AsRef<str>) -> Result<&mut Self, Error> {
        self.text = set(&self.text, name, value.as_ref(), false)?;
        Ok(self)
    }
    pub fn set_expression(
        &mut self,
        name: &str,
        value: impl AsRef<str>,
    ) -> Result<&mut Self, Error> {
        self.text = set(&self.text, name, value.as_ref(), true)?;
        Ok(self)
    }
    pub fn set_with_options(
        &mut self,
        name: &str,
        value: impl AsRef<str>,
        options: &super::EnvironmentWriteOptions,
    ) -> Result<&mut Self, Error> {
        self.text = set_with_options(&self.text, name, value.as_ref(), false, options)?;
        Ok(self)
    }
    pub fn set_expression_with_options(
        &mut self,
        name: &str,
        value: impl AsRef<str>,
        options: &super::EnvironmentWriteOptions,
    ) -> Result<&mut Self, Error> {
        self.text = set_with_options(&self.text, name, value.as_ref(), true, options)?;
        Ok(self)
    }
    pub fn set_many<K: AsRef<str>, V: AsRef<str>>(
        &mut self,
        items: impl IntoIterator<Item = (K, V)>,
    ) -> Result<&mut Self, Error> {
        let mut text = self.text.clone();
        let mut keys = std::collections::BTreeSet::new();
        for (key, value) in items {
            let key = key.as_ref();
            if !keys.insert(key.to_owned()) {
                return Err(Error::DuplicateKey { key: key.into() });
            }
            text = set(&text, key, value.as_ref(), false)?;
        }
        self.text = text;
        Ok(self)
    }
    pub fn remove(&mut self, name: &str) -> Result<&mut Self, Error> {
        self.text = remove(&self.text, name)?;
        Ok(self)
    }
}

fn bom_len(text: &str) -> usize {
    if text.starts_with('\u{feff}') { 3 } else { 0 }
}

// 只识别定位边界相邻的完整空白行，不跨越声明或注释，也不触碰多行值内部。
fn blank_suffix_start(text: &str) -> usize {
    let bom = bom_len(text);
    let mut end = text.len();
    while end > bom {
        let body_end = if text[..end].ends_with('\n') {
            end - 1
        } else {
            end
        };
        let start = text[..body_end].rfind('\n').map_or(bom, |p| p + 1);
        if text[start..body_end]
            .chars()
            .all(|c| matches!(c, ' ' | '\t' | '\r'))
        {
            end = start;
        } else {
            break;
        }
    }
    end
}
fn blank_prefix_end(text: &str) -> usize {
    let mut end = 0;
    for line in text.split_inclusive('\n') {
        if !line.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
            break;
        }
        end += line.len();
    }
    end
}
// 沿用字符串匹配的整键、区分大小写及仅 * 通配语义；独立文档不依赖应用服务。
// 匹配到多项时返回首尾跨度，供 before 取第一项、after 取最后一项。
fn anchor_span(text: &str, anchor: &str, moving: &str) -> Result<Range<usize>, Error> {
    if anchor == moving {
        return Err(Error::InvalidPosition);
    }
    if !anchor.contains('*') {
        return unique(text, anchor)?.ok_or_else(|| Error::MissingAnchor { key: anchor.into() });
    }
    if anchor.len() > 8 * 1024 * 1024 {
        return Err(Error::TooLarge);
    }
    key(&anchor.replace('*', "A")).map_err(|_| Error::InvalidKey { key: anchor.into() })?;
    let pattern = anchor
        .split('*')
        .map(regex::escape)
        .collect::<Vec<_>>()
        .join(".*");
    let matcher = regex::RegexBuilder::new(&format!(r"\A{pattern}\z"))
        .dot_matches_new_line(true)
        .build()
        .map_err(|_| Error::TooLarge)?;
    let document = parse(text)?;
    let mut keys = std::collections::BTreeSet::new();
    let mut matched: Option<Range<usize>> = None;
    for entry in document.entries() {
        // 移动已有变量时排除自身，否则它会不断成为自己的首项或末项。
        if entry.key() == moving || !matcher.is_match(entry.key()) {
            continue;
        }
        if !keys.insert(entry.key()) {
            return Err(Error::DuplicateKey {
                key: entry.key().into(),
            });
        }
        let span = entry.line_span();
        match &mut matched {
            Some(range) => range.end = span.end,
            None => matched = Some(span),
        }
    }
    matched.ok_or_else(|| Error::MissingAnchor { key: anchor.into() })
}
// 已在锚点旁时直接整理原位置的间距，避免移动操作反复遗留自己的分隔空行。
fn is_positioned(
    text: &str,
    name: &str,
    span: &Range<usize>,
    position: &super::EnvironmentPosition,
) -> Result<bool, Error> {
    use super::EnvironmentPosition as Position;
    match position {
        Position::Start => Ok(text[..span.start]
            .trim_start_matches('\u{feff}')
            .trim()
            .is_empty()),
        Position::End => Ok(text[span.end..].trim().is_empty()),
        Position::Before(anchor) | Position::After(anchor) => {
            let other = anchor_span(text, anchor, name)?;
            if matches!(position, Position::Before(_)) {
                Ok(span.end <= other.start && text[span.end..other.start].trim().is_empty())
            } else {
                Ok(other.end <= span.start && text[other.end..span.start].trim().is_empty())
            }
        }
    }
}
pub(super) fn set_with_options(
    text: &str,
    name: &str,
    value: &str,
    expression: bool,
    options: &super::EnvironmentWriteOptions,
) -> Result<Zeroizing<String>, Error> {
    use super::EnvironmentPosition as Position;
    // 在分配换行字符串前检查整数溢出和大小上限。
    let spacing = options
        .blank_lines_before
        .unwrap_or(0)
        .checked_add(options.blank_lines_after.unwrap_or(0))
        .ok_or(Error::TooLarge)?;
    if spacing > 8 * 1024 * 1024 {
        return Err(Error::TooLarge);
    }
    let existing = unique(text, name)?;
    if existing.is_some()
        && !options.move_existing
        && options.blank_lines_before.is_none()
        && options.blank_lines_after.is_none()
    {
        return set(text, name, value, expression);
    }
    let retain_position = !options.move_existing
        || existing
            .as_ref()
            .map(|span| is_positioned(text, name, span, &options.position))
            .transpose()?
            .unwrap_or(false);
    let mut source;
    let declaration;
    let original_position;
    if existing.is_some() {
        source = set(text, name, value, expression)?;
        let mut span = unique(&source, name)?.ok_or(Error::InvalidDocument)?;
        span.start = span.start.max(bom_len(&source));
        declaration = Zeroizing::new(source[span.clone()].to_owned());
        original_position = Some(span.start);
        source.replace_range(span, "");
    } else {
        source = Zeroizing::new(text.to_owned());
        declaration = set("", name, value, expression)?;
        original_position = None;
    }
    let position = if let Some(position) = original_position.filter(|_| retain_position) {
        position
    } else {
        match &options.position {
            Position::Start => bom_len(&source),
            Position::End => source.len(),
            Position::Before(anchor) | Position::After(anchor) => {
                let span = anchor_span(&source, anchor, name)?;
                if matches!(options.position, Position::Before(_)) {
                    span.start.max(bom_len(&source))
                } else {
                    span.end
                }
            }
        }
    };
    let (left, right) = source.split_at(position);
    let left = if options.blank_lines_before.is_some() {
        &left[..blank_suffix_start(left)]
    } else {
        left
    };
    let right = if options.blank_lines_after.is_some() {
        &right[blank_prefix_end(right)..]
    } else {
        right
    };
    let nl = newline(text);
    let body = declaration
        .strip_suffix("\r\n")
        .or_else(|| declaration.strip_suffix('\n'))
        .unwrap_or(&declaration);
    let ends_line = declaration.ends_with('\n');
    let mut out = Zeroizing::new(left.to_owned());
    if left.len() > bom_len(left) && !left.ends_with('\n') {
        out.push_str(nl);
    }
    for _ in 0..options.blank_lines_before.unwrap_or(0) {
        out.push_str(nl);
    }
    out.push_str(body);
    if !right.is_empty() || ends_line || options.blank_lines_after.is_some() {
        out.push_str(nl);
    }
    for _ in 0..options.blank_lines_after.unwrap_or(0) {
        out.push_str(nl);
    }
    out.push_str(right);
    validate(&out)?;
    if !expression && values(&out)?.get(name).map(String::as_str) != Some(value) {
        return Err(Error::InvalidValue { key: name.into() });
    }
    Ok(out)
}
