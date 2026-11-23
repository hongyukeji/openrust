//! URI references used by Eloquent casts.

mod parse;
mod query;

pub use query::UriQueryString;

use std::{fmt, str::FromStr, time::SystemTime};

use crate::{
    http::RedirectResponse,
    routing::{RedirectOptions, RouteParameter, SignatureMode, UrlError, UrlResult},
    support::facades::URL,
};

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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
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

    /// 设置协议，并按新协议重新校验路径、默认端口和其余组件。
    pub fn withScheme(&self, scheme: &str) -> Result<Self, InvalidUri> {
        let mut parts = self.parts.clone();
        parts.scheme = Some(parse::normalize_scheme(scheme)?);
        parts.port = parse::format_port(parts.port, parts.scheme.as_deref())?;
        parts.path = parse::format_path(&parts.path, &parts)?;
        Self::checked_parts(parts)
    }

    /// 设置主机，按 URI 规则规范化主机名并为相对路径补齐 authority 分隔。
    pub fn withHost(&self, host: &str) -> Result<Self, InvalidUri> {
        parse::validate_component(host)?;
        let mut parts = self.parts.clone();
        parts.host = Some(parse::normalize_host(host)?);
        parts.path = parse::format_path(&parts.path, &parts)?;
        Self::checked_parts(parts)
    }

    /// 设置或清除用户信息。密码会进入 URI，调用方应避免将它写入日志。
    pub fn withUser(&self, user: Option<&str>, password: Option<&str>) -> Result<Self, InvalidUri> {
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
    pub fn withPort(&self, port: Option<i64>) -> Result<Self, InvalidUri> {
        let mut parts = self.parts.clone();
        parts.port = parse::format_port(port, parts.scheme.as_deref())?;
        Self::checked_parts(parts)
    }

    /// 设置绝对路径组件并折叠重复的前导斜线。
    pub fn withPath(&self, path: &str) -> Result<Self, InvalidUri> {
        parse::validate_component(path)?;
        let mut parts = self.parts.clone();
        let path = format!("/{}", path.trim_start_matches('/'));
        parts.path = parse::format_path(&path, &parts)?;
        Self::checked_parts(parts)
    }

    /// 设置编码后的片段，空字符串保留显式的 `#` 分隔符。
    pub fn withFragment(&self, fragment: &str) -> Result<Self, InvalidUri> {
        parse::validate_component(fragment)?;
        let mut parts = self.parts.clone();
        parts.fragment = Some(parse::encode_query_or_fragment(fragment));
        Self::checked_parts(parts)
    }

    /// 移除片段及其分隔符。
    pub fn withoutFragment(&self) -> Result<Self, InvalidUri> {
        let mut parts = self.parts.clone();
        parts.fragment = None;
        Self::checked_parts(parts)
    }

    /// 转为 Support 的链式字符串。
    pub fn toStringable(&self) -> super::Stringable {
        super::Stringable::from(self.value())
    }

    pub fn pathSegments(&self) -> Vec<&str> {
        let path = self.path();
        if path == "/" {
            Vec::new()
        } else {
            path.split('/').collect()
        }
    }

    /// Generates a URI using the current application's URL generator.
    pub fn to(path: &str, parameters: &[RouteParameter], secure: Option<bool>) -> UrlResult<Self> {
        Self::generated(URL::to(path, parameters, secure)?)
    }

    pub fn route(name: &str, parameters: &[RouteParameter], absolute: bool) -> UrlResult<Self> {
        Self::generated(URL::route(name, parameters, absolute)?)
    }

    pub fn action(action: &str, parameters: &[RouteParameter], absolute: bool) -> UrlResult<Self> {
        Self::generated(URL::action(action, parameters, absolute)?)
    }

    pub fn signedRoute(
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        mode: SignatureMode,
    ) -> UrlResult<Self> {
        Self::generated(URL::signedRoute(name, parameters, expiration, mode)?)
    }

    pub fn temporarySignedRoute(
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        mode: SignatureMode,
    ) -> UrlResult<Self> {
        Self::generated(URL::temporarySignedRoute(
            name, expiration, parameters, mode,
        )?)
    }

    /// Builds an explicit redirect using the same target validation as other redirects.
    pub fn redirect(&self, options: RedirectOptions) -> UrlResult<RedirectResponse> {
        crate::routing::redirect_response(self.value(), options)
    }

    fn generated(url: String) -> UrlResult<Self> {
        Self::of(&url).map_err(|_| UrlError::InvalidOrigin)
    }

    fn checked_parts(parts: Components) -> Result<Self, InvalidUri> {
        if !parse::validate_state(&parts) {
            return Err(invalid_uri());
        }
        Ok(Self::from_parts(parts))
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
