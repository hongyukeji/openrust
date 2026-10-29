//! 请求绑定驱动使用的有界 Cookie 快照与本请求响应队列。

use crate::{Result, http::CookieJar};
use serde_json::Value;
use std::io::{self, Write};

/// 不持有 Session、Scope 或请求 body 的请求内驱动上下文。
///
/// `CookieJar` 必须来自当前请求；驱动不得把此上下文保存到应用级单例。
#[derive(Clone)]
pub struct SessionRequestContext {
    cookies: Value,
    jar: CookieJar,
}

impl std::fmt::Debug for SessionRequestContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionRequestContext")
            .finish_non_exhaustive()
    }
}

impl SessionRequestContext {
    /// 使用解密后的 Cookie 快照与同一请求队列构造，校验既有树和 64KiB Cookie 预算。
    pub fn new(cookies: Value, jar: CookieJar) -> Result<Self> {
        if !cookies.is_object() && !cookies.is_array() {
            return Err(super::store::invalid_record());
        }
        crate::support::data::validate_limits(&cookies)?;
        serde_json::to_writer(&mut CookieBudget(0), &cookies)
            .map_err(|_| super::store::limit_error())?;
        Ok(Self { cookies, jar })
    }

    /// 精确名称读取入站 Cookie；不展开点路径。
    pub fn cookie(&self, name: &str) -> Option<Value> {
        let value = match &self.cookies {
            Value::Array(values) => name
                .parse::<usize>()
                .ok()
                .filter(|index| index.to_string() == name)
                .and_then(|index| values.get(index)),
            value => value.get(name),
        };
        value.filter(|value| !value.is_null()).cloned()
    }

    /// 当前请求共享的出站 Cookie 队列。
    pub fn cookie_jar(&self) -> &CookieJar {
        &self.jar
    }
}

struct CookieBudget(usize);
impl Write for CookieBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= 64 * 1024)
            .ok_or_else(|| io::Error::other("Cookie budget exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
