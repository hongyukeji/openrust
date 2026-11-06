//! Session唯一最终配置；不创建第二份Cookie或存储设置。

use serde::{Deserialize, Serialize};

/// Session生命周期及Cookie配置；driver默认database，lifetime默认120分钟。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SessionConfig {
    /// driver 名称；None 明确不启用 Session。自定义 `with_store` 复用同一 Provider。
    pub driver: Option<String>,
    /// 数据库连接名称；None 使用同一数据库管理器的默认连接。
    pub connection: Option<String>,
    /// 数据库 Session 的逻辑表名；默认 sessions，由选定 Db 的表名规则解析。
    pub table: String,
    /// Cookie名称；None按最终app.name推导。
    pub cookie: Option<String>,
    /// Cookie路径，默认`/`。
    pub path: String,
    /// Cookie域；None使用请求主机。
    pub domain: Option<String>,
    /// Secure明确值；None由可信请求传输信息推导。
    pub secure: Option<bool>,
    /// 默认true，禁止脚本读取会话Cookie。
    pub http_only: bool,
    /// 默认Some(lax)，None明确省略该属性。
    pub same_site: Option<String>,
    /// 无活动有效期（分钟）。
    pub lifetime: u64,
    /// 请求启动后触发垃圾回收的抽签比例，默认 `[2, 100]`。
    pub lottery: [u32; 2],
    /// 浏览器关闭时移除Cookie，默认false；持久记录仍有lifetime截止。
    pub expire_on_close: bool,
    /// 是否发送Partitioned标记，默认false。
    pub partitioned: bool,
    /// 整体持久编码字节上限，默认20MiB，只能明确调低。
    pub max_bytes: usize,
    /// 对象保存策略：`native` 拒绝对象，`json` 使用对象显式投影。
    pub serializer: String,
}

impl SessionConfig {
    /// 框架受管Session的持久编码硬上限。
    pub const MAX_BYTES: usize = 20 * 1024 * 1024;

    #[cfg(feature = "session")]
    pub(crate) fn lifetime_seconds(&self) -> crate::Result<i64> {
        self.lifetime
            .checked_mul(60)
            .and_then(|value| i64::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| invalid("lifetime", &self.lifetime.to_string()))
    }

    #[cfg(feature = "session")]
    pub(crate) fn validate(&self) -> crate::Result<Option<cookie::SameSite>> {
        self.lifetime_seconds()?;
        if self.lottery[1] == 0 || self.lottery[0] > self.lottery[1] {
            return Err(invalid("lottery", &format!("{:?}", self.lottery)));
        }
        if self.max_bytes == 0 || self.max_bytes > Self::MAX_BYTES {
            return Err(invalid("max_bytes", &self.max_bytes.to_string()));
        }
        if !matches!(self.serializer.as_str(), "native" | "json") {
            return Err(invalid("serializer", &self.serializer));
        }
        self.validate_cookie_defaults()
    }

    pub(crate) fn validate_cookie_defaults(&self) -> crate::Result<Option<cookie::SameSite>> {
        for (key, value) in [
            ("path", Some(self.path.as_str())),
            ("domain", self.domain.as_deref()),
        ] {
            if value.is_some_and(|value| {
                value.len() > 64 * 1024
                    || value
                        .bytes()
                        .any(|byte| byte <= 0x1f || byte == 0x7f || byte == b';')
            }) {
                return Err(invalid(key, "invalid bounded Cookie attribute"));
            }
        }
        if self.cookie.as_ref().is_some_and(|value| {
            value.is_empty()
                || value.len() > 64 * 1024
                || value
                    .bytes()
                    .any(|byte| byte <= b' ' || matches!(byte, b';' | b'=' | b','))
        }) {
            return Err(invalid("cookie", "invalid Cookie name"));
        }
        match self
            .same_site
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            None => Ok(None),
            Some("lax") => Ok(Some(cookie::SameSite::Lax)),
            Some("strict") => Ok(Some(cookie::SameSite::Strict)),
            Some("none") => Ok(Some(cookie::SameSite::None)),
            Some(value) => Err(invalid("same_site", value)),
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            driver: Some("database".into()),
            connection: None,
            table: "sessions".into(),
            cookie: None,
            path: "/".into(),
            domain: None,
            secure: None,
            http_only: true,
            same_site: Some("lax".into()),
            lifetime: 120,
            lottery: [2, 100],
            expire_on_close: false,
            partitioned: false,
            max_bytes: Self::MAX_BYTES,
            serializer: "native".into(),
        }
    }
}

impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionConfig")
            .field("driver", &self.driver)
            .field("connection", &self.connection)
            .field("table", &self.table)
            .field("cookie", &self.cookie)
            .field("path", &self.path)
            .field("domain", &self.domain)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("same_site", &self.same_site)
            .field("lifetime", &self.lifetime)
            .field("lottery", &self.lottery)
            .field("expire_on_close", &self.expire_on_close)
            .field("partitioned", &self.partitioned)
            .field("max_bytes", &self.max_bytes)
            .field("serializer", &self.serializer)
            .finish()
    }
}
fn invalid(key: &str, value: &str) -> crate::Error {
    crate::Error::Config(crate::config::ConfigError::Invalid(format!(
        "session.{key}: {value}"
    )))
}

#[cfg(all(test, feature = "session"))]
mod tests {
    use super::*;

    #[test]
    fn lottery_defaults_and_final_validation_keep_original_values() -> crate::Result<()> {
        let mut config = SessionConfig::default();
        assert_eq!(config.lottery, [2, 100]);
        assert_eq!(
            serde_json::to_value(&config).map_err(crate::Error::internal)?["lottery"],
            serde_json::json!([2, 100])
        );
        for lottery in [[0, 1], [1, 1], [u32::MAX, u32::MAX]] {
            config.lottery = lottery;
            config.validate()?;
        }
        for lottery in [[0, 0], [3, 2]] {
            config.lottery = lottery;
            let error = config
                .validate()
                .err()
                .ok_or_else(|| invalid("lottery", "预期拒绝无效比例"))?;
            let message = error.to_string();
            assert!(message.contains("session.lottery"));
            assert!(message.contains(&format!("{lottery:?}")));
        }
        Ok(())
    }

    #[test]
    fn database_connection_and_table_keep_defaults_and_explicit_configuration() -> crate::Result<()>
    {
        let defaults = SessionConfig::default();
        assert_eq!(defaults.connection, None);
        assert_eq!(defaults.table, "sessions");
        assert_eq!(defaults.lifetime, 120);
        let inherited: SessionConfig =
            serde_json::from_value(serde_json::json!({"driver":"database"}))
                .map_err(crate::Error::internal)?;
        assert_eq!(inherited.connection, None);
        assert_eq!(inherited.table, "sessions");
        let configured: SessionConfig = serde_json::from_value(serde_json::json!({
            "connection":"reporting", "table":"request_sessions"
        }))
        .map_err(crate::Error::internal)?;
        assert_eq!(configured.connection.as_deref(), Some("reporting"));
        assert_eq!(configured.table, "request_sessions");
        let encoded = serde_json::to_value(&configured).map_err(crate::Error::internal)?;
        assert_eq!(encoded["connection"], "reporting");
        assert_eq!(encoded["table"], "request_sessions");
        let debug = format!("{configured:?}");
        assert!(debug.contains("reporting") && debug.contains("request_sessions"));
        let explicit_empty: SessionConfig = serde_json::from_value(serde_json::json!({
            "connection":"", "table":""
        }))
        .map_err(crate::Error::internal)?;
        assert_eq!(explicit_empty.connection.as_deref(), Some(""));
        assert_eq!(explicit_empty.table, "");
        Ok(())
    }
}
