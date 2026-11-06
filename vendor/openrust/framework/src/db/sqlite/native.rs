//! 通过同一 C 引擎的 fresh 连接读取 SQLx 初始化前的外键默认。

use std::sync::OnceLock;

static FOREIGN_KEYS_DEFAULT: OnceLock<bool> = OnceLock::new();

pub(in crate::db) async fn foreign_keys_default() -> crate::Result<bool> {
    if let Some(value) = FOREIGN_KEYS_DEFAULT.get() {
        return Ok(*value);
    }
    let value = tokio::task::spawn_blocking(probe)
        .await
        .map_err(|_| invalid_default())?
        .map_err(|()| invalid_default())?;
    // 默认构建状态不变时各 fresh 连接结果一致；失败不缓存，避免暂态失败永久污染。
    let _ = FOREIGN_KEYS_DEFAULT.set(value);
    Ok(value)
}

fn probe() -> Result<bool, ()> {
    let connection = rusqlite::Connection::open_in_memory().map_err(|_| ())?;
    let value: bool = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .map_err(|_| ())?;
    connection.close().map_err(|_| ())?;
    Ok(value)
}

fn invalid_default() -> crate::Error {
    crate::config::ConfigError::diagnostic("database", "sqlite_native_default_unavailable").into()
}
