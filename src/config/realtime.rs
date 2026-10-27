//! WebSocket 与 SSE 的连接、消息和共享 transport 限额。

use serde::{Deserialize, Serialize};

/// `[realtime]` 区段：活动连接与每连接缓冲区的资源上限。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RealtimeConfig {
    /// 每个 API 节点允许的最大并发连接数（1..=100000）。
    pub max_connections: usize,
    /// 每连接排队消息数（1..=4096）。
    pub max_queue_messages: usize,
    /// 每连接排队消息字节数（1 KiB..=64 MiB）。
    pub max_queue_bytes: usize,
    /// 当前节点所有连接共享的排队消息字节上限（1 MiB..=4 GiB）。
    pub max_total_queue_bytes: usize,
    /// 单条消息最大字节数（1..=1 MiB）。
    pub max_message_bytes: usize,
    /// WebSocket/SSE 心跳间隔（1..=300 秒）。
    pub heartbeat_interval_secs: u64,
    /// 单条连接的最长存活时间；到期后客户端须重新认证（1..=86400 秒）。
    pub max_connection_age_secs: u64,
    /// Redis channel 隔离名称；只能使用 ASCII 字母、数字、`_`、`-` 和 `.`。
    pub namespace: String,
    /// WebSocket 与 `EventSource` 的精确 Origin allowlist；不支持通配符。
    pub allowed_origins: Vec<String>,
    /// 可选 Redis URL。仅在启用 `realtime-redis` 且安装 `RealtimePlugin` 时使用。
    #[serde(skip_serializing)]
    redis_url: Option<String>,
}

impl RealtimeConfig {
    /// 创建使用默认资源上限的配置。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置 Redis URL；Debug 和序列化输出不会暴露其内容。
    #[must_use]
    pub fn with_redis_url(mut self, url: impl Into<String>) -> Self {
        self.redis_url = Some(url.into());
        self
    }

    /// 设置浏览器实时连接的精确 Origin allowlist。
    #[must_use]
    pub fn with_allowed_origins(mut self, origins: impl IntoIterator<Item = String>) -> Self {
        self.allowed_origins = origins.into_iter().collect();
        self
    }

    /// 返回已配置的 Redis URL。
    pub fn redis_url(&self) -> Option<&str> {
        self.redis_url.as_deref()
    }

    /// 检查资源上限、namespace 和可选 Redis URL。
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=100_000).contains(&self.max_connections) {
            return Err("realtime.max_connections must be 1..=100000".into());
        }
        if !(1..=4096).contains(&self.max_queue_messages) {
            return Err("realtime.max_queue_messages must be 1..=4096".into());
        }
        if !(1024..=64 * 1024 * 1024).contains(&self.max_queue_bytes) {
            return Err("realtime.max_queue_bytes must be 1024..=67108864".into());
        }
        if !(1024 * 1024..=u32::MAX as usize).contains(&self.max_total_queue_bytes) {
            return Err("realtime.max_total_queue_bytes must be 1048576..=4294967295".into());
        }
        if !(1..=1024 * 1024).contains(&self.max_message_bytes) {
            return Err("realtime.max_message_bytes must be 1..=1048576".into());
        }
        if !(1..=300).contains(&self.heartbeat_interval_secs) {
            return Err("realtime.heartbeat_interval_secs must be 1..=300".into());
        }
        if !(1..=86_400).contains(&self.max_connection_age_secs) {
            return Err("realtime.max_connection_age_secs must be 1..=86400".into());
        }
        if self.namespace.is_empty()
            || self.namespace.len() > 128
            || !self
                .namespace
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        {
            return Err("realtime.namespace must be 1..=128 safe ASCII characters".into());
        }
        if self.allowed_origins.iter().any(|origin| {
            origin == "*"
                || url::Url::parse(origin).is_err_and(|_| true)
                || !origin.starts_with("https://") && !origin.starts_with("http://")
                || origin.ends_with('/')
                || origin.contains('?')
                || origin.contains('#')
                || url::Url::parse(origin).is_ok_and(|parsed| {
                    parsed.host().is_none()
                        || !parsed.username().is_empty()
                        || parsed.password().is_some()
                        || parsed.path() != "/"
                        || parsed.query().is_some()
                        || parsed.fragment().is_some()
                })
        }) {
            return Err("realtime.allowed_origins must contain exact http(s) origins without paths or wildcards".into());
        }
        if let Some(url) = &self.redis_url
            && !url.starts_with("redis://")
            && !url.starts_with("rediss://")
        {
            return Err("realtime.redis_url must use redis:// or rediss://".into());
        }
        Ok(())
    }
}

impl Default for RealtimeConfig {
    fn default() -> Self {
        Self {
            max_connections: 10_000,
            max_queue_messages: 64,
            max_queue_bytes: 1024 * 1024,
            max_total_queue_bytes: 256 * 1024 * 1024,
            max_message_bytes: 64 * 1024,
            heartbeat_interval_secs: 20,
            max_connection_age_secs: 900,
            namespace: "openrust".into(),
            allowed_origins: Vec::new(),
            redis_url: None,
        }
    }
}

impl std::fmt::Debug for RealtimeConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RealtimeConfig")
            .field("max_connections", &self.max_connections)
            .field("max_queue_messages", &self.max_queue_messages)
            .field("max_queue_bytes", &self.max_queue_bytes)
            .field("max_total_queue_bytes", &self.max_total_queue_bytes)
            .field("max_message_bytes", &self.max_message_bytes)
            .field("heartbeat_interval_secs", &self.heartbeat_interval_secs)
            .field("max_connection_age_secs", &self.max_connection_age_secs)
            .field("namespace", &self.namespace)
            .field("allowed_origins", &self.allowed_origins)
            .field("redis_url", &self.redis_url.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::RealtimeConfig;

    #[test]
    fn config_bounds_and_redacts_redis_credentials() {
        let mut config = RealtimeConfig::new().with_redis_url("redis://user:secret@localhost/0");
        assert!(config.validate().is_ok());
        assert!(!format!("{config:?}").contains("secret"));
        config.max_queue_bytes = 1;
        assert!(config.validate().is_err());
    }
}
