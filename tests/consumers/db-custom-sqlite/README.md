# 自定义 SQLite 数据库 Provider

此独立示例展示如何在应用 package 中开启 SeaORM 的 SQLite driver，并通过 OpenRust 无官方驱动也可用的 `prelude::Db` 与 `database` 公共导出使用自定义连接、backend 查询、表名解析和事务。

本 consumer 刻意不启用 OpenRust 的 `db-sqlite`，验证自定义 Provider 的通用连接、事务、查询与应用自有迁移。框架 `migrate` 命令应先明确返回 `database_feature_disabled`，不因缺少标准连接的锁 URL 改报其他错误。需要内置 SQLite 迁移时启用 `db-sqlite` 并使用标准 Provider；这与本 consumer 的外部驱动契约分别验证。内置连接、配置及迁移的限定范围见[数据库指南](../../../docs/src/database/connections.md)，认证/Session/队列等业务 SQL 不由此示例证明。

从源码仓库根目录运行：

```sh
cargo run --manifest-path tests/consumers/db-custom-sqlite/Cargo.toml
```

`openrust` 显式关闭标准默认 feature，并保留原有 `cache-redis`；未启用 `db-postgres`、`db-mysql` 或队列。示例通过自身 `sea-orm` 依赖开启 `sqlx-sqlite`。自定义 Provider在 register 登记共享 `Db` 工厂，解析时创建 SeaORM `DatabaseConnection`、登记异步关闭回调并调用 `Db::initialize()`。多个连接可使用 `Db::initialize_with_replicas()`，但需由应用自行验证只读和复制语义。
