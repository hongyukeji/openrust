//! URI references used by Eloquent casts.

mod parse;

use std::{fmt, str::FromStr};

use parse::Components;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidUri;

impl fmt::Display for InvalidUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid URI reference")
    }
}

impl std::error::Error for InvalidUri {}

fn invalid_uri() -> InvalidUri {
    InvalidUri
}

/// 按 URI 组件解析的值，可表示绝对地址、相对路径或空引用。
#[derive(Clone, PartialEq, Eq)]
pub struct Uri {
    parts: Components,
    authority: Option<String>,
    value: String,
}

#[allow(non_snake_case, reason = "Matches Laravel's URI API")]
impl Uri {
    /// 解析并规范化各 URI 组件。
    pub fn of(input: &str) -> Result<Self, InvalidUri> {
        parse::parse(input).map(Self::from_parts)
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
    pub fn isEmpty(&self) -> bool {
        self.value.is_empty()
    }

    /// 判断 URI 是否包含内容。
    pub fn isNotEmpty(&self) -> bool {
        !self.isEmpty()
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
    pub fn userInfo(&self) -> Option<&str> {
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

    /// Returns the encoded query, including an explicitly empty query.
    pub fn queryString(&self) -> Option<&str> {
        self.parts.query.as_deref()
    }

    pub fn fragment(&self) -> Option<&str> {
        self.parts.fragment.as_deref()
    }
}

impl fmt::Display for Uri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.value())
    }
}

impl FromStr for Uri {
    type Err = InvalidUri;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::of(input)
    }
}

impl serde::Serialize for Uri {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.value())
    }
}
