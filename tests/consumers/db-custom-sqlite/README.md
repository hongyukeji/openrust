# 自定义 SQLite 数据库插件

此独立示例展示如何在应用 package 中开启 SeaORM 的 SQLite driver，并通过 OpenRust 公共 `Db` 服务使用自定义连接、backend 查询、表名解析和事务。

SQLite 在此验证自定义插件的通用 SeaORM 连接、事务和应用自有迁移，不属于 OpenRust 官方数据库支持矩阵。示例先验证框架 `MigrationPlugin` 对 SQLite 的迁移互斥明确返回 `unsupported_database_backend`，再通过公开 `DatabaseTableNames::scope` 与应用自己的 SeaORM `Migrator` 执行单机迁移并查询验证；不把它当作框架官方迁移支持。框架自带的数据库连接配置、方言 helper、认证/队列/协调迁移、共享锁、数据库限流及 `TestDatabase` 仍仅面向 PostgreSQL/MySQL；使用其他 backend 时，应用必须自行提供并验证对应能力。

从源码仓库根目录运行：

```sh
cargo run --manifest-path tests/consumers/db-custom-sqlite/Cargo.toml
```

`openrust` 未启用 `db-postgres` 或 `db-mysql` feature；示例通过自身 `sea-orm` 依赖开启 `sqlx-sqlite`。自定义插件应在 `build` 注册唯一 `Db`，在 `finish` 创建 SeaORM `DatabaseConnection` 后调用 `Db::initialize()`。多个连接可使用 `Db::initialize_with_replicas()`，但需由应用自行验证只读和复制语义。
