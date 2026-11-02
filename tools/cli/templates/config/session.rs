//! Session 持久化和 Cookie 的应用默认值。

use openrust::config::{ConfigContext, ConfigError, SessionConfig};

/// Provider 注册与每条路由选择 session 中间件仍为独立步骤。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(ctx: &ConfigContext) -> Result<SessionConfig, ConfigError> {
    Ok(SessionConfig {
        // None 明确不启用 Session；database 仍须数据库及迁移。
        driver: Some("database".to_owned()),
        // None 选默认数据库连接；部署可指定同一数据库管理器中的具名连接。
        connection: ctx.nullable_string_or("SESSION_CONNECTION", None),
        // 逻辑表名仍通过选定连接的 table_prefix / table_names 解析。
        table: ctx.string_or("SESSION_TABLE", "sessions")?,
        // None 从最终 app.name 推导 Cookie 名称。
        cookie: None,
        path: "/".to_owned(),
        // None 不指定 Cookie Domain，使用请求主机。
        domain: None,
        // None 依可信请求传输信息推导 Secure；Some 显式固定该属性。
        secure: None,
        http_only: true,
        // None 省略 SameSite；可选 lax、strict、none。
        same_site: Some("lax".to_owned()),
        lifetime: 120,
        lottery: [2, 100],
        expire_on_close: false,
        partitioned: false,
        // 20 MiB 为框架硬上限；应用只能调低。
        max_bytes: 20 * 1024 * 1024,
        // native 拒绝对象；json 仅保存对象显式投影。
        serializer: "native".to_owned(),
    })
}
