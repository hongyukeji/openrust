//! 响应 Cookie 值对象与 Laravel 风格工厂。
//!
//! 依据 Laravel d52c3b3129e4c8e213c97cdc68f94b0b7d27f181 `CookieJar`，
//! 以及 Symfony `HttpFoundation` v8.0.0（19f925ff62437970e56fcdf793aa93607622d8be）
//! Cookie 的默认值、编码、空值删除与属性模型。未知扩展不属于该对象模型。

/*
Symfony 派生语义的许可声明：
Copyright (c) 2004-present Fabien Potencier

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is furnished
to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/

use std::{fmt::Write as _, sync::Arc};

use crate::support::calendar::{
    CalendarClock, CalendarContext, CalendarInstant, DateOptions, parse_date,
};

use axum::http::{HeaderValue, StatusCode};
use cookie::{
    SameSite,
    time::{Duration, OffsetDateTime},
};

pub(super) const HEADER_LIMIT: usize = 64 * 1024;
const RESERVED_NAME: &[u8] = b"=,; \t\r\n\x0b\x0c";

/// 单次创建 Cookie 的属性；空选项继承工厂默认值。
#[derive(Clone, Debug)]
pub struct CookieOptions {
    /// Cookie 路径；空串或 `"0"` 按 Laravel 规则继承默认路径。
    pub path: Option<String>,
    /// Cookie 域；空串或 `"0"` 按 Laravel 规则继承默认域。
    pub domain: Option<String>,
    /// 是否仅通过 HTTPS 发送；未设置时继承工厂与请求默认值。
    pub secure: Option<bool>,
    /// 是否禁止 JavaScript 读取，默认开启。
    pub http_only: bool,
    /// 是否直接输出值而不执行 URL 编码，默认关闭。
    pub raw: bool,
    /// 跨站策略；未设置时继承工厂默认值。
    pub same_site: Option<SameSite>,
}

impl Default for CookieOptions {
    fn default() -> Self {
        Self {
            path: None,
            domain: None,
            secure: None,
            http_only: true,
            raw: false,
            same_site: None,
        }
    }
}

/// 不可变响应 Cookie；调试输出不包含名称、值或其它业务属性。
#[derive(Clone)]
pub struct Cookie {
    name: String,
    value: Option<String>,
    path: String,
    domain: Option<String>,
    secure: Option<bool>,
    http_only: bool,
    raw: bool,
    same_site: Option<SameSite>,
    expires: Option<OffsetDateTime>,
    max_age: Option<i64>,
    partitioned: bool,
}

impl std::fmt::Debug for Cookie {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Cookie").finish_non_exhaustive()
    }
}

impl Cookie {
    /// 返回 Cookie 名称。
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// 返回原始值；删除 Cookie 为 `None`，空串保持为 `Some("")`。
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
    /// 返回 Cookie 路径。
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// 返回 Cookie 域。
    #[must_use]
    pub fn domain(&self) -> Option<&str> {
        self.domain.as_deref()
    }
    /// 返回显式安全属性；`None` 表示使用请求默认值。
    #[must_use]
    pub fn secure(&self) -> Option<bool> {
        self.secure
    }
    /// 是否禁止 JavaScript 读取。
    #[must_use]
    pub fn http_only(&self) -> bool {
        self.http_only
    }
    /// 是否直接输出值。
    #[must_use]
    pub fn raw(&self) -> bool {
        self.raw
    }
    /// 返回跨站策略。
    #[must_use]
    pub fn same_site(&self) -> Option<SameSite> {
        self.same_site
    }
    /// 返回到期时间；会话 Cookie 为 `None`。
    #[must_use]
    pub fn expires(&self) -> Option<OffsetDateTime> {
        self.expires
    }

    /// 返回设置Partitioned标记的Cookie，保留其余属性。
    #[must_use]
    pub fn with_partitioned(mut self, partitioned: bool) -> Self {
        self.partitioned = partitioned;
        self
    }

    /// 复制 Cookie 并替换值，保留全部属性；非法应用参数返回安全 500 错误。
    pub fn with_value(&self, value: String) -> crate::Result<Self> {
        self.replace_value(value, invalid)
    }

    #[cfg(feature = "encryption")]
    pub(crate) fn with_response_value(&self, value: String) -> crate::Result<Self> {
        self.replace_value(value, super::cookie_limits_error)
    }

    fn replace_value(
        &self,
        value: String,
        limit_error: fn() -> crate::Error,
    ) -> crate::Result<Self> {
        let mut result = self.clone();
        result.value = Some(value);
        result.validate_with_limit(limit_error)?;
        Ok(result)
    }

    /// 生成有界 `Set-Cookie` 头；自动安全属性由当前请求提供。
    ///
    /// 空值按 Symfony 规则输出删除 Cookie。应用参数或输出超限返回安全 500 错误。
    pub fn to_header(&self, secure_default: bool) -> crate::Result<HeaderValue> {
        self.render_header(secure_default, invalid)
    }

    pub(crate) fn to_response_header(&self, secure_default: bool) -> crate::Result<HeaderValue> {
        self.render_header(secure_default, super::cookie_limits_error)
    }

    fn render_header(
        &self,
        secure_default: bool,
        limit_error: fn() -> crate::Error,
    ) -> crate::Result<HeaderValue> {
        self.validate_with_limit(limit_error)?;
        let now = now_utc()?;
        let name = if self.raw {
            self.name.clone()
        } else {
            encode(&self.name, true)
        };
        let mut rendered = name;
        rendered.push('=');
        if self.value.as_deref().is_none_or(str::is_empty) {
            rendered.push_str("deleted");
            let past = now
                .checked_sub(Duration::seconds(31_536_001))
                .ok_or_else(invalid)?;
            append_expiry(&mut rendered, past, 0)?;
        } else {
            if let Some(value) = &self.value {
                rendered.push_str(&if self.raw {
                    value.clone()
                } else {
                    encode(value, false)
                });
            }
            if let Some(expires) = self.expires {
                let age = self
                    .max_age
                    .unwrap_or_else(|| (expires - now).whole_seconds().max(0));
                append_expiry(&mut rendered, expires, age)?;
            } else if let Some(age) = self.max_age {
                write!(rendered, "; Max-Age={age}").map_err(|_| invalid())?;
            }
        }
        rendered.push_str("; path=");
        rendered.push_str(&self.path);
        if let Some(domain) = self.domain.as_deref().filter(|value| php_truthy(value)) {
            rendered.push_str("; domain=");
            rendered.push_str(domain);
        }
        if self.secure.unwrap_or(secure_default) {
            rendered.push_str("; secure");
        }
        if self.http_only {
            rendered.push_str("; httponly");
        }
        if let Some(site) = self.same_site {
            rendered.push_str("; samesite=");
            rendered.push_str(match site {
                SameSite::Lax => "lax",
                SameSite::Strict => "strict",
                SameSite::None => "none",
            });
        }
        if self.partitioned {
            rendered.push_str("; partitioned");
        }
        if rendered.len() > HEADER_LIMIT {
            return Err(limit_error());
        }
        HeaderValue::from_str(&rendered).map_err(|_| invalid())
    }

    /// 解析应用生成的 `Set-Cookie`，保留 Symfony 对象可表示的属性。
    ///
    /// 名称和值保持原始编码（对应 `Cookie::fromString` 默认 `decode=false`）；
    /// 未知扩展按上游对象模型忽略；到期时间默认使用 UTC 与系统时钟。
    /// 已知属性无效、非法头或超过 64 KiB 返回安全 500。
    pub fn from_header(header: &str) -> crate::Result<Self> {
        Self::from_header_with_context(header, &CalendarContext::default())
    }

    /// 使用显式应用时区和一次捕获的生产时钟语义解析到期时间。
    pub(crate) fn from_header_with_context(
        header: &str,
        context: &CalendarContext,
    ) -> crate::Result<Self> {
        if header.len() > HEADER_LIMIT || HeaderValue::from_str(header).is_err() {
            return Err(invalid());
        }
        let instant = context.now().map_err(|_| invalid())?;
        let now =
            OffsetDateTime::from_unix_timestamp(instant.epoch_seconds()).map_err(|_| invalid())?;
        // strtotime 不读取 Carbon test-now；只复用 calendar 的时区、词法和计算内核。
        let context =
            CalendarContext::new(context.timezone().clone(), Arc::new(ExpiryClock(instant)));
        let parts = split_header(header)?;
        let first = parts.first().ok_or_else(invalid)?;
        let (name, value) = first
            .split_once('=')
            .map_or((*first, None), |(name, value)| (name, Some(value)));
        let mut result = Self {
            name: name.trim().to_owned(),
            value: value.map(unquote).transpose()?,
            path: "/".into(),
            domain: None,
            secure: Some(false),
            http_only: false,
            raw: true,
            same_site: None,
            expires: None,
            max_age: None,
            partitioned: false,
        };
        for part in parts.iter().skip(1) {
            let (key, value) = part
                .split_once('=')
                .map_or((part.trim(), None), |(key, value)| {
                    (key.trim(), Some(value.trim()))
                });
            match key.to_ascii_lowercase().as_str() {
                "path" => {
                    let path = unquote(value.ok_or_else(invalid)?)?;
                    result.path = if php_truthy(&path) { path } else { "/".into() };
                }
                "domain" => result.domain = Some(unquote(value.ok_or_else(invalid)?)?),
                "secure" => result.secure = Some(true),
                "httponly" => result.http_only = true,
                "partitioned" => result.partitioned = true,
                "samesite" => {
                    result.same_site = match unquote(value.ok_or_else(invalid)?)?
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "" => None,
                        "lax" => Some(SameSite::Lax),
                        "strict" => Some(SameSite::Strict),
                        "none" => Some(SameSite::None),
                        _ => return Err(invalid()),
                    };
                }
                "expires" => {
                    result.expires = parse_expiry(&unquote(value.ok_or_else(invalid)?)?, &context)?;
                }
                "max-age" => {
                    result.max_age = Some(
                        value
                            .ok_or_else(invalid)?
                            .parse::<i64>()
                            .map_err(|_| invalid())?,
                    );
                }
                _ => {}
            }
        }
        if let Some(age) = result.max_age
            && (age > 0 || result.expires.is_some_and(|expires| expires > now))
        {
            result.expires = Some(
                now.checked_add(Duration::seconds(age))
                    .ok_or_else(invalid)?,
            );
        }
        result.max_age = result.max_age.map(|age| age.max(0));
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> crate::Result<()> {
        self.validate_with_limit(invalid)
    }

    fn validate_with_limit(&self, limit_error: fn() -> crate::Error) -> crate::Result<()> {
        if self.name.len() > HEADER_LIMIT
            || self.path.len() > HEADER_LIMIT
            || self
                .domain
                .as_ref()
                .is_some_and(|value| value.len() > HEADER_LIMIT)
            || self
                .value
                .as_ref()
                .is_some_and(|value| value.len() > HEADER_LIMIT)
        {
            return Err(limit_error());
        }
        if !php_truthy(&self.name)
            || self.name.bytes().any(|byte| {
                (byte.is_ascii_control() && !RESERVED_NAME.contains(&byte))
                    || (self.raw && RESERVED_NAME.contains(&byte))
            })
            || invalid_attribute(&self.path)
            || self
                .domain
                .as_ref()
                .is_some_and(|value| invalid_attribute(value))
            || (self.raw
                && self
                    .value
                    .as_ref()
                    .is_some_and(|value| invalid_attribute(value)))
        {
            return Err(invalid());
        }
        let size = encoded_len(&self.name, self.raw, true)
            .checked_add(
                self.value
                    .as_deref()
                    .map_or(0, |value| encoded_len(value, self.raw, false)),
            )
            .and_then(|size| size.checked_add(self.path.len()))
            .and_then(|size| size.checked_add(self.domain.as_ref().map_or(0, String::len)))
            .ok_or_else(limit_error)?;
        if size > HEADER_LIMIT {
            return Err(limit_error());
        }
        Ok(())
    }
}

/// Laravel 风格 Cookie 工厂；默认路径 `/`、自动安全属性与 `SameSite=Lax`。
#[derive(Clone, Debug)]
pub struct CookieFactory {
    path: String,
    domain: Option<String>,
    secure: Option<bool>,
    same_site: Option<SameSite>,
}

impl Default for CookieFactory {
    fn default() -> Self {
        Self {
            path: "/".into(),
            domain: None,
            secure: None,
            same_site: Some(SameSite::Lax),
        }
    }
}

impl CookieFactory {
    /// 创建 Cookie；零分钟为会话 Cookie，非零分钟使用受检到期时间计算。
    pub fn make(
        &self,
        name: impl Into<String>,
        value: impl Into<String>,
        minutes: i64,
        options: CookieOptions,
    ) -> crate::Result<Cookie> {
        self.build(name.into(), Some(value.into()), minutes, options)
    }
    /// 创建有效期为 576000 分钟（400 天）的 Cookie。
    pub fn forever(
        &self,
        name: impl Into<String>,
        value: impl Into<String>,
        options: CookieOptions,
    ) -> crate::Result<Cookie> {
        self.make(name, value, 576_000, options)
    }
    /// 创建删除 Cookie，值为 `None`，头包含过去的到期时间和 `Max-Age=0`。
    pub fn forget(&self, name: impl Into<String>, options: CookieOptions) -> crate::Result<Cookie> {
        self.build(name.into(), None, -2_628_000, options)
    }
    /// 修改工厂默认值；非法属性不改变原默认值并返回安全 500。
    pub fn set_default_path_and_domain(
        &mut self,
        path: String,
        domain: Option<String>,
        secure: Option<bool>,
        same_site: Option<SameSite>,
    ) -> crate::Result<&mut Self> {
        if path.len() > HEADER_LIMIT
            || invalid_attribute(&path)
            || domain
                .as_ref()
                .is_some_and(|value| value.len() > HEADER_LIMIT || invalid_attribute(value))
        {
            return Err(invalid());
        }
        self.path = path;
        self.domain = domain;
        self.secure = secure;
        self.same_site = same_site;
        Ok(self)
    }
    fn build(
        &self,
        name: String,
        value: Option<String>,
        minutes: i64,
        options: CookieOptions,
    ) -> crate::Result<Cookie> {
        let seconds = minutes.checked_mul(60).ok_or_else(invalid)?;
        let expires = if minutes == 0 {
            None
        } else {
            let expires = now_utc()?
                .checked_add(Duration::seconds(seconds))
                .ok_or_else(invalid)?;
            (expires.unix_timestamp() > 0).then_some(expires)
        };
        let path = options
            .path
            .filter(|path| php_truthy(path))
            .unwrap_or_else(|| self.path.clone());
        let result = Cookie {
            name,
            value,
            path: if php_truthy(&path) { path } else { "/".into() },
            domain: options
                .domain
                .filter(|domain| php_truthy(domain))
                .or_else(|| self.domain.clone()),
            secure: options.secure.or(self.secure),
            http_only: options.http_only,
            raw: options.raw,
            same_site: options.same_site.or(self.same_site),
            expires,
            max_age: None,
            partitioned: false,
        };
        result.validate()?;
        Ok(result)
    }
}

// 捕获的 clock 不启用 Carbon test-now，保持 PHP strtotime 的生产缺项语义。
struct ExpiryClock(CalendarInstant);
impl CalendarClock for ExpiryClock {
    fn now(&self) -> crate::Result<CalendarInstant> {
        Ok(self.0)
    }
}

fn parse_expiry(source: &str, context: &CalendarContext) -> crate::Result<Option<OffsetDateTime>> {
    // Symfony is_numeric 分支转换为 int，而 Carbon 会把短数字文本作为日期。
    if let Some(timestamp) = numeric_expiry(source) {
        return positive_expiry(timestamp);
    }
    // 标准 HTTP 日期包含 GMT，直接解析既快也不受应用默认时区影响。
    let parsed = cookie::Cookie::parse(format!("x=x; Expires={source}"))
        .ok()
        .and_then(|cookie| cookie.expires_datetime());
    if let Some(expires) = parsed {
        return positive_expiry(expires.unix_timestamp());
    }
    let value = serde_json::Value::String(source.to_owned());
    let date = parse_date(&value, DateOptions::default(), context)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    positive_expiry(date.timestamp())
}

fn positive_expiry(timestamp: i64) -> crate::Result<Option<OffsetDateTime>> {
    if timestamp <= 0 {
        return Ok(None);
    }
    OffsetDateTime::from_unix_timestamp(timestamp)
        .map(Some)
        .map_err(|_| invalid())
}

// PHP 数字字符串显式截断小数并饱和至整数边界；非有限指数转换为零。
#[allow(clippy::cast_possible_truncation)]
fn numeric_expiry(source: &str) -> Option<i64> {
    let source = source.trim_matches(|character: char| character.is_ascii_whitespace());
    if let Ok(integer) = source.parse::<i64>() {
        return Some(integer);
    }
    if source
        .bytes()
        .any(|byte| byte.is_ascii_alphabetic() && !matches!(byte, b'e' | b'E'))
    {
        return None;
    }
    let number = source.parse::<f64>().ok()?;
    Some(if number.is_finite() { number as i64 } else { 0 })
}

fn now_utc() -> crate::Result<OffsetDateTime> {
    OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .map_err(|_| invalid())
}

fn invalid() -> crate::Error {
    crate::Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "cookie_parameters_invalid",
        "Cookie 参数或响应头无效",
    )
}
fn php_truthy(value: &str) -> bool {
    !value.is_empty() && value != "0"
}
fn invalid_attribute(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b';')
}
fn encoded_len(value: &str, raw: bool, name: bool) -> usize {
    value
        .bytes()
        .map(|byte| {
            if raw || !needs_encoding(byte, name) {
                1
            } else {
                3
            }
        })
        .sum()
}
fn needs_encoding(byte: u8, name: bool) -> bool {
    if name {
        RESERVED_NAME.contains(&byte)
    } else {
        !(byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte))
    }
}
fn encode(value: &str, name: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut result = String::with_capacity(encoded_len(value, false, name));
    if name {
        for character in value.chars() {
            if let Some(byte) = u8::try_from(character)
                .ok()
                .filter(|byte| RESERVED_NAME.contains(byte))
            {
                result.push('%');
                result.push(char::from(HEX[usize::from(byte >> 4)]));
                result.push(char::from(HEX[usize::from(byte & 15)]));
            } else {
                result.push(character);
            }
        }
    } else {
        for &byte in value.as_bytes() {
            if needs_encoding(byte, false) {
                result.push('%');
                result.push(char::from(HEX[usize::from(byte >> 4)]));
                result.push(char::from(HEX[usize::from(byte & 15)]));
            } else {
                result.push(char::from(byte));
            }
        }
    }
    result
}
fn append_expiry(output: &mut String, expires: OffsetDateTime, age: i64) -> crate::Result<()> {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    write!(
        output,
        "; expires={}, {:02} {} {:04} {:02}:{:02}:{:02} GMT; Max-Age={age}",
        DAYS[usize::from(expires.weekday().number_days_from_monday())],
        expires.day(),
        MONTHS[usize::from(u8::from(expires.month()) - 1)],
        expires.year(),
        expires.hour(),
        expires.minute(),
        expires.second()
    )
    .map_err(|_| invalid())
}
fn split_header(header: &str) -> crate::Result<Vec<&str>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in header.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b';' && !quoted {
            result.push(header[start..index].trim());
            start = index + 1;
        }
    }
    if quoted || escaped {
        return Err(invalid());
    }
    result.push(header[start..].trim());
    Ok(result)
}
fn unquote(value: &str) -> crate::Result<String> {
    let value = value.trim();
    if !value.starts_with('"') {
        return Ok(value.to_owned());
    }
    let quoted = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(invalid)?;
    let mut result = String::new();
    let mut escaped = false;
    for character in quoted.chars() {
        if escaped {
            result.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            result.push(character);
        }
    }
    if escaped {
        return Err(invalid());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cookie_defaults_and_secure_request() -> crate::Result<()> {
        let cookie =
            CookieFactory::default().make("session", "a /+é", 0, CookieOptions::default())?;
        assert_eq!(cookie.value(), Some("a /+é"));
        assert_eq!(cookie.path(), "/");
        assert_eq!(cookie.domain(), None);
        assert_eq!(cookie.secure(), None);
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert!(cookie.http_only());
        assert!(!cookie.raw());
        assert_eq!(cookie.expires(), None);
        assert_eq!(
            cookie.to_header(false)?.to_str().ok(),
            Some("session=a%20%2F%2B%C3%A9; path=/; httponly; samesite=lax")
        );
        assert!(
            cookie
                .to_header(true)?
                .to_str()
                .is_ok_and(|header| header.contains("; secure"))
        );
        assert!(!format!("{cookie:?}").contains("session"));
        assert!(!format!("{cookie:?}").contains("a /+é"));
        Ok(())
    }

    #[test]
    fn forever_uses_400_days_and_forget_is_real_deletion() -> crate::Result<()> {
        let before = OffsetDateTime::now_utc();
        let factory = CookieFactory::default();
        let cookie = factory.forever("persistent", "value", CookieOptions::default())?;
        assert!(
            cookie
                .expires()
                .is_some_and(|expires| (expires - before).whole_seconds() >= 34_559_999)
        );
        assert!(
            cookie
                .expires()
                .is_some_and(|expires| (expires - before).whole_seconds() <= 34_560_001)
        );
        let forgotten = factory.forget("persistent", CookieOptions::default())?;
        assert_eq!(forgotten.value(), None);
        assert!(forgotten.expires().is_some_and(|expires| expires < before));
        let header = forgotten.to_header(false)?;
        assert!(
            header
                .to_str()
                .is_ok_and(|header| header.starts_with("persistent=deleted; expires=")
                    && header.contains("; Max-Age=0"))
        );
        let empty = factory.make("empty", "", 0, CookieOptions::default())?;
        assert_eq!(empty.value(), Some(""));
        assert!(
            empty
                .to_header(false)?
                .to_str()
                .is_ok_and(|header| header.contains("=deleted;") && header.contains("Max-Age=0"))
        );
        Ok(())
    }

    #[test]
    fn options_inherit_defaults_and_with_value_preserves_attributes() -> crate::Result<()> {
        let mut factory = CookieFactory::default();
        factory.set_default_path_and_domain(
            "foo".into(),
            Some("/domain".into()),
            Some(true),
            Some(SameSite::Strict),
        )?;
        let cookie = factory.make(
            "name",
            "value",
            1,
            CookieOptions {
                path: Some("0".into()),
                secure: Some(false),
                http_only: false,
                raw: true,
                ..CookieOptions::default()
            },
        )?;
        assert_eq!(cookie.path(), "foo");
        assert_eq!(cookie.domain(), Some("/domain"));
        assert_eq!(cookie.secure(), Some(false));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
        let replaced = cookie.with_value("ciphertext".into())?;
        assert_eq!(replaced.value(), Some("ciphertext"));
        assert_eq!(replaced.expires(), cookie.expires());
        assert_eq!(replaced.path(), cookie.path());
        assert_eq!(replaced.domain(), cookie.domain());
        assert_eq!(replaced.secure(), cookie.secure());
        assert_eq!(replaced.same_site(), cookie.same_site());
        assert_eq!(replaced.raw(), cookie.raw());
        assert_eq!(replaced.http_only(), cookie.http_only());
        Ok(())
    }

    #[test]
    fn existing_header_preserves_known_attributes_without_automatic_secure() -> crate::Result<()> {
        let cookie = Cookie::from_header(
            "name=raw%20value==; Path=/x; Domain=.example.test; Max-Age=3600; Secure; HttpOnly; SameSite=None; Partitioned; Priority=High",
        )?;
        assert_eq!(cookie.value(), Some("raw%20value=="));
        assert_eq!(cookie.domain(), Some(".example.test"));
        assert!(cookie.raw());
        let replacement = cookie.with_value("encrypted".into())?;
        let header = replacement.to_header(false)?;
        let header = header.to_str().map_err(|_| invalid())?;
        for attribute in [
            "path=/x",
            "domain=.example.test",
            "Max-Age=3600",
            "secure",
            "httponly",
            "samesite=none",
            "partitioned",
        ] {
            assert!(header.contains(attribute));
        }
        assert!(!header.contains("Priority"));
        let insecure = Cookie::from_header("name=value; SameSite=None; Partitioned")?;
        assert!(
            !insecure
                .to_header(true)?
                .to_str()
                .is_ok_and(|header| header.contains("; secure"))
        );
        assert_eq!(Cookie::from_header("name")?.value(), None);
        Ok(())
    }

    #[test]
    fn invalid_application_parameters_and_minute_overflow_fail_safely() -> crate::Result<()> {
        let factory = CookieFactory::default();
        for name in ["", "0", "bad\0name"] {
            let result = factory.make(name, "SECRET", 0, CookieOptions::default());
            assert!(result.is_err());
            if let Err(error) = result {
                assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
                assert_eq!(error.code(), "cookie_parameters_invalid");
                assert!(!format!("{error:?}").contains("SECRET"));
            }
        }
        for minutes in [i64::MIN, i64::MAX, i64::MAX / 60] {
            assert!(
                factory
                    .make("name", "value", minutes, CookieOptions::default())
                    .is_err()
            );
        }
        assert!(
            factory
                .make(
                    "name",
                    "value",
                    0,
                    CookieOptions {
                        path: Some("/; Secure".into()),
                        ..CookieOptions::default()
                    }
                )
                .is_err()
        );
        assert!(
            factory
                .make(
                    "bad name",
                    "value",
                    0,
                    CookieOptions {
                        raw: true,
                        ..CookieOptions::default()
                    }
                )
                .is_err()
        );
        assert!(
            factory
                .make(
                    "name",
                    "secret; injected=1",
                    0,
                    CookieOptions {
                        raw: true,
                        ..CookieOptions::default()
                    }
                )
                .is_err()
        );
        assert!(
            factory
                .make(
                    "name",
                    "x".repeat(HEADER_LIMIT),
                    0,
                    CookieOptions::default()
                )
                .is_err()
        );
        assert!(Cookie::from_header("name=value; SameSite=invalid").is_err());
        assert!(Cookie::from_header("name=value\r\nInjected: secret").is_err());
        assert!(Cookie::from_header("name=value; Max-Age=9223372036854775808").is_err());
        assert!(Cookie::from_header("name=value; Expires=invalid").is_err());
        let encoded_name = factory.make("a b=é", "value", 0, CookieOptions::default())?;
        assert!(
            encoded_name
                .to_header(false)?
                .as_bytes()
                .starts_with("a%20b%3Dé=value".as_bytes())
        );
        Ok(())
    }

    // 固定参考：PHP 8.5.0 -n，Symfony v8.0.0 Cookie::expiresTimestamp；
    // strtotime(text, 1705322096)，date_default_timezone_set("America/New_York")。
    // PHP 的显式 base 只用于参考脚本，生产实现不修改进程时区或全局时钟。
    #[test]
    fn expires_relative_and_overflow_reuse_calendar_with_production_clock_semantics()
    -> crate::Result<()> {
        let context = CalendarContext::new(
            crate::support::calendar::ResolvedTimezone::parse("America/New_York")?,
            Arc::new(crate::support::calendar::FixedClock::new(
                CalendarInstant::new(1_705_322_096, 123_456)?,
            )),
        );
        for (source, expected) in [
            ("tomorrow", 1_705_381_200),
            ("-1 day", 1_705_235_696),
            ("2020-02-31", 1_583_125_200),
        ] {
            let cookie = Cookie::from_header_with_context(
                &format!("name=value; Expires={source}"),
                &context,
            )?;
            assert_eq!(
                cookie.expires().map(OffsetDateTime::unix_timestamp),
                Some(expected)
            );
            assert_eq!(cookie.expires().map(OffsetDateTime::nanosecond), Some(0));
        }
        // 公共入口默认 UTC，日期溢出与 PHP strtotime 的归一化一致。
        assert_eq!(
            Cookie::from_header("name=value; Expires=2020-02-31")?
                .expires()
                .map(OffsetDateTime::unix_timestamp),
            Some(1_583_107_200)
        );
        Ok(())
    }

    #[test]
    fn expires_numeric_cast_and_invalid_strtotime_have_distinct_results() -> crate::Result<()> {
        for (source, expected) in [
            ("0", None),
            ("-1.9", None),
            ("1.9", Some(1)),
            ("1e3", Some(1000)),
            ("1e1000", None),
        ] {
            let cookie = Cookie::from_header(&format!("name=value; Expires={source}"))?;
            assert_eq!(
                cookie.expires().map(OffsetDateTime::unix_timestamp),
                expected
            );
        }
        for source in ["definitely-invalid", ""] {
            let result = Cookie::from_header(&format!("name=value; Expires={source}"));
            assert!(result.is_err());
            if let Err(error) = result {
                assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
                assert_eq!(error.code(), "cookie_parameters_invalid");
            }
        }
        Ok(())
    }

    #[test]
    fn expires_context_clock_is_read_once_and_http_date_keeps_gmt() -> crate::Result<()> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct CountingClock(Arc<AtomicUsize>);
        impl CalendarClock for CountingClock {
            fn now(&self) -> crate::Result<CalendarInstant> {
                self.0.fetch_add(1, Ordering::SeqCst);
                CalendarInstant::new(1_705_322_096, 0)
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let context = CalendarContext::new(
            crate::support::calendar::ResolvedTimezone::parse("America/New_York")?,
            Arc::new(CountingClock(calls.clone())),
        );
        let cookie = Cookie::from_header_with_context(
            "name=value; Expires=Wed, 01 Jan 2020 00:00:00 GMT",
            &context,
        )?;
        assert_eq!(
            cookie.expires().map(OffsetDateTime::unix_timestamp),
            Some(1_577_836_800)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        Ok(())
    }
}
