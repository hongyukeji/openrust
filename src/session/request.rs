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
    protection: Option<crate::http::cookie::CookieProtection>,
    database_metadata: super::DatabaseSessionMetadata,
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
        Ok(Self {
            cookies,
            jar,
            protection: None,
            database_metadata: super::DatabaseSessionMetadata::default(),
        })
    }

    pub(crate) fn from_request(
        cookies: Value,
        jar: CookieJar,
        protection: Option<crate::http::cookie::CookieProtection>,
    ) -> Result<Self> {
        let mut context = Self::new(cookies, jar)?;
        context.protection = protection;
        Ok(context)
    }

    pub(super) fn protects_session(&self, cookie_name: &str) -> bool {
        self.protection.as_ref().is_some_and(|protection| {
            protection.encrypted
                && !protection
                    .except
                    .iter()
                    .any(|name| name == cookie_name || super::SessionId::parse(name).is_ok())
        })
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

    /// 安装本请求真实观测的数据库元数据；缺省列保持省略语义。
    #[must_use]
    pub fn with_database_metadata(mut self, metadata: super::DatabaseSessionMetadata) -> Self {
        self.database_metadata = metadata;
        self
    }

    /// 数据库 driver 的请求内元数据快照，不包含请求体或容器作用域。
    pub fn database_metadata(&self) -> &super::DatabaseSessionMetadata {
        &self.database_metadata
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_context_has_no_authentication_proof_and_exact_exceptions_are_conservative()
    -> Result<()> {
        let name = "session_id";
        let jar = CookieJar::new(crate::http::CookieFactory::default());
        assert!(
            !SessionRequestContext::new(serde_json::json!({}), jar.clone())?.protects_session(name)
        );
        for (encrypted, except, expected) in [
            (false, vec![], false),
            (true, vec![name.to_owned()], false),
            (true, vec!["A".repeat(40)], false),
            (true, vec!["*".to_owned()], true),
            (true, vec![], true),
        ] {
            let context = SessionRequestContext::from_request(
                serde_json::json!({}),
                jar.clone(),
                Some(crate::http::cookie::CookieProtection { encrypted, except }),
            )?;
            assert_eq!(context.protects_session(name), expected);
        }
        Ok(())
    }
}
