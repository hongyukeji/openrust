//! Laravel 的 URI 值、组件读取、不可变变更和字符串投影。

mod parse;
mod query;
mod response;
pub use query::UriQueryString;

use super::{Collection, DataObject, Htmlable, Stringable};
use crate::{Error, Result};
use parse::Components;
use serde::{Serialize, Serializer};
use std::{any::Any, borrow::Cow, fmt, str::FromStr};

/// 按 URI 组件解析的值，可表示绝对地址、相对路径或空引用。
///
/// ```
/// use openrust::support::Uri;
///
/// let uri = Uri::of("../orders?state=open#details")?;
/// assert_eq!(uri.path(), "../orders");
/// assert_eq!(uri.query_string(), Some("state=open"));
/// assert_eq!(uri.fragment(), Some("details"));
/// # Ok::<(), openrust::Error>(())
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Uri {
    parts: Components,
    authority: Option<String>,
    value: String,
}

impl Uri {
    /// 解析并规范化各 URI 组件。
    pub fn of(input: &str) -> Result<Self> {
        parse::parse(input).map(Self::from_parts)
    }

    fn checked_parts(parts: Components) -> Result<Self> {
        if parse::validate_state(&parts) {
            Ok(Self::from_parts(parts))
        } else {
            Err(invalid_uri())
        }
    }

    fn from_parts(parts: Components) -> Self {
        let authority = parts.host.as_ref().map(|host| {
            let mut authority = String::new();
            if parts.user.is_some() || parts.password.is_some() {
                authority.push_str(parts.user.as_deref().unwrap_or_default());
                if let Some(password) = &parts.password {
                    authority.push(':');
                    authority.push_str(password);
                }
                authority.push('@');
            }
            authority.push_str(host);
            if let Some(port) = parts.port {
                authority.push(':');
                authority.push_str(&port.to_string());
            }
            authority
        });
        let mut value = String::new();
        if let Some(scheme) = &parts.scheme {
            value.push_str(scheme);
            value.push(':');
        }
        if let Some(authority) = &authority {
            value.push_str("//");
            value.push_str(authority);
        }
        value.push_str(&parts.path);
        if let Some(query) = &parts.query {
            value.push('?');
            value.push_str(query);
        }
        if let Some(fragment) = &parts.fragment {
            value.push('#');
            value.push_str(fragment);
        }
        Self {
            parts,
            authority,
            value,
        }
    }

    /// 借用规范化后的 URI 文本。
    pub fn value(&self) -> &str {
        &self.value
    }

    /// 判断 URI 是否为空引用。
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// 判断 URI 是否包含内容。
    pub fn is_not_empty(&self) -> bool {
        !self.is_empty()
    }

    /// 取得规范化后的 scheme。
    pub fn scheme(&self) -> Option<&str> {
        self.parts.scheme.as_deref()
    }

    /// 取得完整 authority。其内容可能包括凭据。
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// 取得编码后的用户名。
    pub fn user(&self) -> Option<&str> {
        self.parts.user.as_deref()
    }

    /// 取得编码后的用户信息，包含显式密码。
    pub fn user_info(&self) -> Option<&str> {
        self.authority()?
            .split_once('@')
            .map(|(user_info, _)| user_info)
    }

    /// 取得编码后的密码。
    pub fn password(&self) -> Option<&str> {
        self.parts.password.as_deref()
    }

    /// 取得规范化后的主机，包括 IPv6 字面值的方括号。
    pub fn host(&self) -> Option<&str> {
        self.parts.host.as_deref()
    }

    /// 取得显式非默认端口。URI 本身不限制为 TCP 端口范围。
    pub const fn port(&self) -> Option<i64> {
        self.parts.port
    }

    /// 取得编码后的路径。去除两端斜线，空路径用 `/` 表示。
    pub fn path(&self) -> &str {
        let path = self.parts.path.trim_matches('/');
        if path.is_empty() { "/" } else { path }
    }

    /// 按出现顺序返回路径段，保留内部空段及百分号编码。
    pub fn path_segments(&self) -> Result<Collection<&str>> {
        let path = self.path();
        if path == "/" {
            return Ok(Collection::default());
        }
        Collection::try_from_entries(
            (0_i64..)
                .zip(path.split('/'))
                .map(|(key, part)| (key.into(), part)),
        )
    }

    /// 取得编码后的查询原文，区分缺失与显式空查询。
    pub fn query_string(&self) -> Option<&str> {
        self.parts.query.as_deref()
    }

    /// 取得编码后的片段，区分缺失与显式空片段。
    pub fn fragment(&self) -> Option<&str> {
        self.parts.fragment.as_deref()
    }

    /// 设置协议，并按新协议重新校验路径、默认端口和其余组件。
    pub fn with_scheme(&self, scheme: &str) -> Result<Self> {
        let mut parts = self.parts.clone();
        parts.scheme = Some(parse::normalize_scheme(scheme)?);
        parts.port = parse::format_port(parts.port, parts.scheme.as_deref())?;
        parts.path = parse::format_path(&parts.path, &parts)?;
        Self::checked_parts(parts)
    }

    /// 设置主机，按 URI 规则规范化主机名并为相对路径补齐 authority 分隔。
    pub fn with_host(&self, host: &str) -> Result<Self> {
        parse::validate_component(host)?;
        let mut parts = self.parts.clone();
        parts.host = Some(parse::normalize_host(host)?);
        parts.path = parse::format_path(&parts.path, &parts)?;
        Self::checked_parts(parts)
    }

    /// 设置或清除用户信息。密码会进入 URI，调用方应避免将它写入日志。
    pub fn with_user(&self, user: Option<&str>, password: Option<&str>) -> Result<Self> {
        if let Some(user) = user {
            parse::validate_component(user)?;
        }
        if let Some(password) = password {
            parse::validate_component(password)?;
        }
        let mut parts = self.parts.clone();
        parts.user = user.map(parse::encode_user);
        parts.password = password.map(parse::encode_password);
        Self::checked_parts(parts)
    }

    /// 设置或清除端口，省略与当前协议默认值相同的端口。
    pub fn with_port(&self, port: Option<i64>) -> Result<Self> {
        let mut parts = self.parts.clone();
        parts.port = parse::format_port(port, parts.scheme.as_deref())?;
        Self::checked_parts(parts)
    }

    /// 设置绝对路径组件，按 Laravel 规则折叠重复的前导斜线。
    pub fn with_path(&self, path: &str) -> Result<Self> {
        parse::validate_component(path)?;
        let mut parts = self.parts.clone();
        let path = format!("/{}", path.trim_start_matches('/'));
        parts.path = parse::format_path(&path, &parts)?;
        Self::checked_parts(parts)
    }

    /// 设置编码后的片段，空字符串保留显式的 `#` 分隔符。
    pub fn with_fragment(&self, fragment: &str) -> Result<Self> {
        parse::validate_component(fragment)?;
        let mut parts = self.parts.clone();
        parts.fragment = Some(parse::encode_query_or_fragment(fragment));
        Self::checked_parts(parts)
    }

    /// 移除片段及其分隔符。
    pub fn without_fragment(&self) -> Result<Self> {
        let mut parts = self.parts.clone();
        parts.fragment = None;
        Self::checked_parts(parts)
    }

    /// 转为 Support 的链式字符串。
    pub fn to_stringable(&self) -> Stringable {
        Stringable::from(self.value())
    }
}

impl FromStr for Uri {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self> {
        Self::of(input)
    }
}

impl fmt::Display for Uri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.value())
    }
}

impl fmt::Debug for Uri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Uri([REDACTED])")
    }
}

impl Serialize for Uri {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.value())
    }
}

impl Htmlable for Uri {
    type Raw = str;

    fn to_html(&self) -> &str {
        self.value()
    }

    fn to_html_text(&self) -> Result<Cow<'_, str>> {
        Ok(Cow::Borrowed(self.value()))
    }
}

impl DataObject for Uri {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn json_projection(&self) -> Option<serde_json::Value> {
        Some(serde_json::Value::String(self.value.clone()))
    }

    fn string_projection(&self) -> Option<String> {
        Some(self.value.clone())
    }
}

fn invalid_uri() -> Error {
    Error::custom("support_uri_invalid", "URI 语法无效")
}

#[cfg(test)]
mod tests;
