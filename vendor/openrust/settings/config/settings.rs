use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(_env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | 数据库动态设置仓储
    |--------------------------------------------------------------------------
    |
    | 通过 settings Cargo feature 显式启用。默认连接为应用数据库，表名为 settings。
    | 表名或连接变更时须同步登记对应迁移；动态设置值不写入配置缓存。
    | 每五秒核对数据库版本，缓存沿用默认 store，失效时回退数据库，无需集群开关。
    | namespace 默认按环境、数据库及表隔离；独立应用共用数据库时需独立表或连接。
    |
    */
    Ok(
        json!({"repository":"database","repositories":{"database":{"connection":null,"table":"settings"}},"namespace":null,"refresh_interval":5,"cache":{"enabled":true,"store":null,"ttl":3600}}),
    )
}
