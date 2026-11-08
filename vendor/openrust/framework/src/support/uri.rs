//! Laravel 的 URI 值、组件读取和字符串投影。

mod parse;

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
        let parts = parse::parse(input)?;
        let authority = parts.host.as_ref().map(|host| {
            let mut authority = String::new();
            if let Some(user) = &parts.user {
                authority.push_str(user);
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
        Ok(Self {
            parts,
            authority,
            value,
        })
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
