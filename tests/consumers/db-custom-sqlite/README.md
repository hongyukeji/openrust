# 自定义 SQLite 数据库 Provider

此独立示例展示如何在应用 package 中开启 SeaORM 的 SQLite driver，并通过 OpenRust 无官方驱动也可用的 `prelude::Db` 与 `database` 公共导出使用自定义连接、backend 查询、表名解析和事务。

SQLite 在此验证自定义 Provider的通用 SeaORM 连接、事务和应用自有迁移，不属于 OpenRust 官方数据库支持矩阵。示例通过公开 `DatabaseTableNames::scope` 与应用自己的 SeaORM `Migrator` 执行单机迁移并查询验证，再验证框架 `MigrationServiceProvider` 对 SQLite 的迁移互斥明确返回 `unsupported_database_backend`；不把它当作框架官方迁移支持。框架自带的数据库连接配置、方言 helper、认证/队列/协调迁移、共享锁、数据库限流及 `TestDatabase` 仍仅面向 PostgreSQL/MySQL；使用其他 backend 时，应用必须自行提供并验证对应能力。

从源码仓库根目录运行：

```sh
cargo run --manifest-path tests/consumers/db-custom-sqlite/Cargo.toml
```

`openrust` 显式关闭标准默认 feature，并保留原有 `cache-redis`；未启用 `db-postgres`、`db-mysql` 或队列。示例通过自身 `sea-orm` 依赖开启 `sqlx-sqlite`。自定义 Provider在 register 登记共享 `Db` 工厂，解析时创建 SeaORM `DatabaseConnection`、登记异步关闭回调并调用 `Db::initialize()`。多个连接可使用 `Db::initialize_with_replicas()`，但需由应用自行验证只读和复制语义。
