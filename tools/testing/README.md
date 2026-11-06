# testing 测试工具

`testing` 是应用测试工具 package，只能依赖根 `openrust`，根框架不反向依赖它。它重导出根框架的 `TestDatabase` 与 `TestDatabaseRecovery`，保持应用侧 `testing::TestDatabase` 入口。

`TestDatabase::create::<M>(admin_url)` 为一次测试创建唯一数据库并运行应用迁移。PostgreSQL 先建立临时迁移模板再克隆；MySQL 为每个用例新建数据库并运行迁移。MySQL 强制 TLS，并将 session 时区设为 `+00:00`。隔离 URL 保留原连接凭据和查询参数，只替换数据库路径；管理账号须有创建和删除数据库的权限。网络隔离只支持直连唯一可写主库；不使用读写分离、多主或可切换到不同实例的代理。MySQL 还须有 CREATE ROUTINE、可读取 routine 元数据及完整 `performance_schema` threads/status 的权限；PostgreSQL 须能读取 `pg_control_system()` 和连接活动。权限不足明确失败，不静默跳过归属检查。

SQLite 应用启用 `testing/db-sqlite`，使用 `TestDatabase::sqlite_file::<M>().await?` 创建自有临时文件库，或使用 `sqlite_memory::<M>()` 创建唯一具名共享内存库；两者都通过同一公开迁移与连接机制，无需管理服务地址。生成认证测试使用文件库，覆盖实际 HTTP 与应用迁移。

测试完成后调用 `close().await` 关闭连接并等待自有数据库或临时目录删除。网络 guard 的 Drop 和未轮询/取消的 close 将清理移交短寿命执行者，不依赖调用方 runtime 继续存活；正常进程退出最多等待30秒。强杀、abort 或断电后的回收由下一次显式 create 或 `TestDatabase::recover(admin_url).await?` 根据持久回执补偿。`Debug` 输出会遮蔽数据库 URL。隔离能力不代表所有数据库业务 SQL 已适配，具体边界仍以各模块验收为准。

网络清理前须关闭使用该 URL 的独立应用池、停止新借用；框架不强制终止连接。仍被持有的库会保留，释放后可显式 recover 重试。MySQL 活动快照不能排斥不合作的新连接或跨库限定名访问；普通管理员并发 DDL、完整复制了标记的同名备份也不具备原子 CAS 保证。

回执默认位于项目 `storage/framework/testing/databases`，可用进程环境变量 `OPENRUST_TEST_DATABASE_STATE_DIR` 指定稳定私有目录。Unix 目录/文件须属当前有效用户且仅本人可访问；本地锁、每资源随机见证、实际服务身份共同核验，模板和目标分别持责。不要删除或搬动活跃回执、永久锁文件和完成墓碑；`cargo clean` 不会移除它们。恢复只读这些回执，不按数据库前缀、PID 或年龄扫描删除。缺失/错配标记、结果未知或活动连接均保留，`TestDatabaseRecovery` 分别返回 reclaimed/active/retained 数量。当前网络持久恢复不提供非 Unix ACL 实现，明确拒绝而不降级检查。

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
let names = openrust::database::DatabaseTableNames::new("shop_", Default::default())?;
let database = testing::TestDatabase::create_with_table_names::<migration::Migrator>(
    &std::env::var("OPENRUST_TEST_DATABASE_URL")?,
    names,
)
.await?;
```
