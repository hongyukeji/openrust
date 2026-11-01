//! 实时连接与可选 Redis transport 的应用默认值；仅 realtime feature 编译。

use openrust::config::{ConfigContext, ConfigError};
use serde::Serialize;

// 框架类型的私有 redis_url 不参与 Serialize；应用 DTO 必须显式保留部署入口。
/// 实时通信的可序列化应用配置；字段对应 `RealtimeConfig` 与 Provider schema。
#[derive(Serialize)]
pub struct Realtime {
    max_connections: usize,
    max_queue_messages: usize,
    max_queue_bytes: usize,
    max_total_queue_bytes: usize,
    max_message_bytes: usize,
    heartbeat_interval_secs: u64,
    max_connection_age_secs: u64,
    namespace: String,
    allowed_origins: Vec<String>,
    redis_url: Option<String>,
}

/// 配置不安装 Provider；Redis transport 还需 realtime-redis feature。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<Realtime, ConfigError> {
    Ok(Realtime {
        max_connections: 10_000,
        max_queue_messages: 64,
        max_queue_bytes: 1024 * 1024,
        max_total_queue_bytes: 256 * 1024 * 1024,
        max_message_bytes: 64 * 1024,
        heartbeat_interval_secs: 20,
        max_connection_age_secs: 900,
        namespace: "openrust".to_owned(),
        // 精确 http(s) Origin，不接受通配、路径或末尾斜线。
        allowed_origins: Vec::new(),
        // None 不指定共享 transport URL；Some 或部署来源可提供 redis(s) URL。
        redis_url: None,
    })
}
