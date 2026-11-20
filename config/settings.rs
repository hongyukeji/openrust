use serde_json::{Value, json};

pub(super) fn load() -> Value {
    /*
    |--------------------------------------------------------------------------
    | 数据库动态设置仓储
    |--------------------------------------------------------------------------
    |
    | 通过 settings Cargo feature 显式启用。默认连接为应用数据库，表名为 settings。
    | 表名或连接变更时须同步登记对应迁移；动态设置值不写入配置缓存。
    |
    */
    json!({"repository":"database","repositories":{"database":{"connection":null,"table":"settings"}}})
}
