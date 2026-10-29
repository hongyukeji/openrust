//! 默认请求输入转换及请求级覆盖。

use serde_json::Value;
use std::task::{Context, Poll};
use tower::{Layer, Service};

/// 请求输入规范化策略，同时可作为 Tower Layer 安装。
///
/// 默认先递归去除字符串首尾不可见字符，再把空字符串转换为 null。
/// 此策略只修改受管理提取器的 bags，不消费正文；原生 Axum 提取器保持原样。
#[derive(Clone, Debug)]
#[must_use]
pub struct InputNormalization {
    trim: bool,
    null: bool,
    except: Vec<String>,
}

impl Default for InputNormalization {
    fn default() -> Self {
        Self {
            trim: true,
            null: true,
            except: vec![
                "current_password".into(),
                "password".into(),
                "password_confirmation".into(),
            ],
        }
    }
}

impl InputNormalization {
    /// 启用或关闭 `TrimStrings`。
    pub const fn trim_strings(mut self, enabled: bool) -> Self {
        self.trim = enabled;
        self
    }
    /// 启用或关闭 `ConvertEmptyStringsToNull`。
    pub const fn convert_empty_strings_to_null(mut self, enabled: bool) -> Self {
        self.null = enabled;
        self
    }
    /// 增加按完整 dot key 匹配的 trim 排除项；星号匹配任意字符。
    pub fn except_trim<I, T>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        self.except.extend(patterns.into_iter().map(Into::into));
        self
    }
    /// 关闭两项规范化；也可向请求 extensions 插入此策略以条件跳过。
    pub fn disabled() -> Self {
        Self::default()
            .trim_strings(false)
            .convert_empty_strings_to_null(false)
    }

    pub(crate) fn apply(&self, value: &mut Value) {
        self.transform(value, "", true);
    }
    fn transform(&self, value: &mut Value, path: &str, root: bool) {
        match value {
            Value::Object(values) => {
                for (key, value) in values {
                    self.transform(value, &join(path, key, root), false);
                }
            }
            Value::Array(values) => {
                for (index, value) in values.iter_mut().enumerate() {
                    self.transform(value, &join(path, &index.to_string(), root), false);
                }
            }
            Value::String(text) => {
                if self.trim && !self.except.iter().any(|pattern| wildcard(pattern, path)) {
                    *text = text.trim_matches(trim_character).to_owned();
                }
                if self.null && text.is_empty() {
                    *value = Value::Null;
                }
            }
            _ => {}
        }
    }
}
fn join(path: &str, key: &str, root: bool) -> String {
    if root {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}
fn wildcard(pattern: &str, value: &str) -> bool {
    let mut remainder = value;
    let mut pieces = pattern.split('*').peekable();
    if let Some(first) = pieces.next() {
        let Some(rest) = remainder.strip_prefix(first) else {
            return false;
        };
        remainder = rest;
    }
    while let Some(piece) = pieces.next() {
        if pieces.peek().is_none() {
            return remainder.ends_with(piece);
        }
        let Some(index) = remainder.find(piece) else {
            return false;
        };
        remainder = &remainder[index + piece.len()..];
    }
    remainder.is_empty()
}
// Laravel v13.39 Str::trim：PCRE Unicode whitespace 加 INVISIBLE_CHARACTERS，含 NUL。
fn trim_character(value: char) -> bool {
    matches!(value, '\0' | '\u{9}'..='\u{d}' | '\u{20}' | '\u{85}' | '\u{a0}' | '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{115f}'..='\u{1160}' | '\u{1680}' | '\u{17b4}'..='\u{17b5}' | '\u{180e}' | '\u{2000}'..='\u{200f}' | '\u{2028}'..='\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{2060}'..='\u{2065}' | '\u{206a}'..='\u{206f}' | '\u{2800}' | '\u{3000}' | '\u{3164}' | '\u{feff}' | '\u{ffa0}' | '\u{1d159}' | '\u{1d173}'..='\u{1d17a}' | '\u{e0020}')
}

impl<S> Layer<S> for InputNormalization {
    type Service = NormalizationService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        NormalizationService {
            inner,
            policy: self.clone(),
        }
    }
}

/// 将输入规范化策略写入请求 extensions 的中间件服务。
#[derive(Clone, Debug)]
pub struct NormalizationService<S> {
    inner: S,
    policy: InputNormalization,
}
impl<S, B> Service<http::Request<B>> for NormalizationService<S>
where
    S: Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
    fn call(&mut self, mut request: http::Request<B>) -> Self::Future {
        if request.extensions().get::<InputNormalization>().is_none() {
            request.extensions_mut().insert(self.policy.clone());
        }
        self.inner.call(request)
    }
}

// 输入已通过 Axum JSON 语法及结构深度校验；这里只定位 token 并累计限额，不定义第二套语法。
struct JsonRewrite<'a> {
    raw: &'a [u8],
    cursor: usize,
    copied: usize,
    output: Vec<u8>,
    policy: &'a InputNormalization,
    fields: usize,
    expanded: usize,
}
impl InputNormalization {
    pub(crate) fn rewrite_json(&self, raw: &[u8]) -> crate::Result<Vec<u8>> {
        let mut rewrite = JsonRewrite {
            raw,
            cursor: 0,
            copied: 0,
            output: Vec::with_capacity(raw.len()),
            policy: self,
            fields: 0,
            expanded: 0,
        };
        rewrite.walk("", true, 0)?;
        rewrite.output.extend_from_slice(&raw[rewrite.copied..]);
        Ok(rewrite.output)
    }
}
impl JsonRewrite<'_> {
    fn walk(&mut self, path: &str, root: bool, depth: usize) -> crate::Result<()> {
        if depth > 64 {
            return Err(crate::Error::custom_with_status(
                http::StatusCode::PAYLOAD_TOO_LARGE,
                "input_limits_exceeded",
                "JSON input exceeds inspection limit",
            ));
        }
        self.whitespace();
        match self.raw.get(self.cursor) {
            Some(b'"') => {
                let start = self.cursor;
                let mut value = Value::String(self.string()?);
                self.policy.transform(&mut value, path, root);
                self.output.extend_from_slice(&self.raw[self.copied..start]);
                serde_json::to_writer(&mut self.output, &value).map_err(crate::Error::internal)?;
                self.copied = self.cursor;
            }
            Some(b'{') => {
                self.cursor += 1;
                self.whitespace();
                if self.raw.get(self.cursor) == Some(&b'}') {
                    self.cursor += 1;
                    return Ok(());
                }
                loop {
                    self.whitespace();
                    let key = self.string()?;
                    self.take(b':')?;
                    let path = self.child_path(path, &key, root)?;
                    self.walk(&path, false, depth + 1)?;
                    self.whitespace();
                    if self.raw.get(self.cursor) != Some(&b',') {
                        break;
                    }
                    self.cursor += 1;
                }
                self.take(b'}')?;
            }
            Some(b'[') => {
                self.cursor += 1;
                self.whitespace();
                if self.raw.get(self.cursor) == Some(&b']') {
                    self.cursor += 1;
                    return Ok(());
                }
                let mut index = 0_usize;
                loop {
                    let path = self.child_path(path, &index.to_string(), root)?;
                    self.walk(&path, false, depth + 1)?;
                    index += 1;
                    self.whitespace();
                    if self.raw.get(self.cursor) != Some(&b',') {
                        break;
                    }
                    self.cursor += 1;
                }
                self.take(b']')?;
            }
            Some(_) => {
                while self.raw.get(self.cursor).is_some_and(|byte| {
                    !byte.is_ascii_whitespace() && !matches!(byte, b',' | b'}' | b']')
                }) {
                    self.cursor += 1;
                }
            }
            None => return Err(rewrite_error()),
        }
        Ok(())
    }
    fn child_path(&mut self, parent: &str, key: &str, root: bool) -> crate::Result<String> {
        let length = if root {
            key.len()
        } else {
            parent.len().saturating_add(1).saturating_add(key.len())
        };
        self.fields = self.fields.saturating_add(1);
        self.expanded = self.expanded.saturating_add(length);
        if self.fields > 1024 || self.expanded > 20 * 1024 * 1024 {
            return Err(crate::Error::custom_with_status(
                http::StatusCode::PAYLOAD_TOO_LARGE,
                "input_limits_exceeded",
                "JSON input exceeds inspection limit",
            ));
        }
        Ok(join(parent, key, root))
    }
    fn whitespace(&mut self) {
        while self
            .raw
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }
    fn take(&mut self, byte: u8) -> crate::Result<()> {
        self.whitespace();
        if self.raw.get(self.cursor) != Some(&byte) {
            return Err(rewrite_error());
        }
        self.cursor += 1;
        Ok(())
    }
    fn string(&mut self) -> crate::Result<String> {
        let start = self.cursor;
        if self.raw.get(start) != Some(&b'"') {
            return Err(rewrite_error());
        }
        self.cursor += 1;
        while let Some(byte) = self.raw.get(self.cursor) {
            match byte {
                b'\\' => self.cursor += 2,
                b'"' => {
                    self.cursor += 1;
                    return serde_json::from_slice(&self.raw[start..self.cursor])
                        .map_err(|_| rewrite_error());
                }
                _ => self.cursor += 1,
            }
        }
        Err(rewrite_error())
    }
}
fn rewrite_error() -> crate::Error {
    crate::Error::BadRequest("JSON input could not be normalized".into())
}

#[cfg(test)]
mod tests {
    use super::InputNormalization;

    #[test]
    fn lexical_json_rewrite_preserves_numbers_keys_duplicates_and_context() -> crate::Result<()> {
        let raw = br#" { " n " : 1.2300e+2, "negative":-0, "wide":340282366920938463463374607431768211455, "text":" first ","text":" last ", "\u0070assword":" secret ", "user":{"password":" nested "},"arr":[" x ",""],"literal":" { [ \"numbers\" ] } " } "#;
        let expected = br#" { " n " : 1.2300e+2, "negative":-0, "wide":340282366920938463463374607431768211455, "text":"first","text":"last", "\u0070assword":" secret ", "user":{"password":"nested"},"arr":["x",null],"literal":"{ [ \"numbers\" ] }" } "#;
        let actual = InputNormalization::default().rewrite_json(raw)?;
        assert_eq!(actual, expected);
        Ok(())
    }
}
