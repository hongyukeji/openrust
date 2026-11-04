# testing 测试工具

`testing` 是应用测试工具 package，只能依赖根 `openrust`，根框架不反向依赖它。它重导出根框架的 `TestDatabase`，保持应用侧 `testing::TestDatabase` 入口。

`TestDatabase::create::<M>(admin_url)` 为一次测试创建唯一数据库并运行应用迁移。PostgreSQL 先建立临时迁移模板再克隆；MySQL 为每个用例新建数据库并运行迁移。MySQL 强制 TLS，并将 session 时区设为 `+00:00`。隔离 URL 保留原连接凭据和查询参数，只替换数据库路径；管理账号须有创建和删除数据库的权限。

SQLite 应用启用 `testing/db-sqlite`，使用 `TestDatabase::sqlite_file::<M>().await?` 创建自有临时文件库，或使用 `sqlite_memory::<M>()` 创建唯一具名共享内存库；两者都通过同一公开迁移与连接机制，无需管理服务地址。生成认证测试使用文件库，覆盖实际 HTTP 与应用迁移。

测试完成后调用 `close().await` 关闭连接并等待自有数据库或临时目录删除。guard 在 Tokio runtime 中 drop 时也会尝试清理。`Debug` 输出会遮蔽数据库 URL。隔离能力不代表所有数据库业务 SQL 已适配，具体边界仍以各模块验收为准。

在应用 `Cargo.toml` 中添加测试依赖：

```toml
[dev-dependencies]
testing = { workspace = true, features = ["db-postgres"] }
```

```rust,ignore
let database = testing::TestDatabase::create::<migration::Migrator>(
    &std::env::var("OPENRUST_TEST_DATABASE_URL")?,
)
.await?;
```

MySQL/PostgreSQL 数据库测试须使用真实服务；环境变量未设置或测试被跳过时，不构成数据库验收。SQLite 测试使用实际引擎与自有库。`TestApp` 与 HTTP 响应断言使用 `openrust::testing` 的公开入口；本 package 只重导出数据库隔离工具。

使用自定义表前缀或全名映射时，将应用使用的 resolver 一并传给隔离数据库：

```rust,ignore
let names = openrust::db::DatabaseTableNames::new("shop_", Default::default())?;
let database = testing::TestDatabase::create_with_table_names::<migration::Migrator>(
    &std::env::var("OPENRUST_TEST_DATABASE_URL")?,
    names,
)
.await?;
```
