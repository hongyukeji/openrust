# ADR-049 数据库迁移执行互斥

- 状态：Accepted（已实现并完成双库验收）
- 日期：2026-10-27
- 依据：[ADR-042](adr-042-shared-locks-and-request-idempotency.md)、[M5 运行生命周期与迁移协调](../plans/v1-capability-coverage.md#能力表)

## 背景

同一数据库可由多个应用节点或运维进程同时执行 `migrate up/down/status`。SeaORM 的 migration runner 不负责跨进程互斥；并发首次建迁移表、检查待迁移项和应用 DDL 会导致竞争。现有 `SharedLocks` 依赖应用显式登记的 `CoordinationMigration`，而被迁移集合中的该 migration 可能创建或删除共享锁表，不能覆盖首次迁移及全部 down 路径。

## 决策

- `MigrationPlugin` 的 `up`、`down`、`status` 命令均先在所选 `Db` 上获取排他执行锁；具名实例各自使用所选数据库连接。
- 迁移锁使用专用的 `migration_locks` 内部表及固定单行主键。命令在主库按 `DatabaseTableNames` resolver 惰性创建表和锁行，后续应用 migration 不管理或删除该表。
- 每个竞争者以独立 SeaORM 事务执行 `SELECT ... FOR UPDATE NOWAIT` 并持有该行锁；获得后才运行 SeaORM migration runner。PostgreSQL/MySQL 的锁冲突按驱动错误码识别后短暂重试，非锁冲突错误立即传播。
- 默认等待上限为 30 秒；应用可通过 `MigrationPlugin::with_lock_wait_timeout` 覆盖。超时返回 `migration_lock_timeout`，不执行 migration。执行期间 future 取消或进程/连接退出时，事务回滚释放行锁，等待者可继续竞争。
- DDL 仍由 SeaORM runner 在其自身连接和事务边界执行；互斥不让 MySQL DDL 变成原子事务，也不承诺部分 schema 自动回滚。迁移失败后仍须按数据库现状检查并恢复。
- 锁仅保护同一物理数据库且使用相同逻辑表名解析配置的迁移命令；直接调用 `MigratorTrait`、原生 SQL、外部工具或绕过 `MigrationPlugin` 的路径不受保护。
- `status` 也取得排他锁，以避免它在 schema DDL 进行中读取半完成状态。只读副本不参与锁和迁移。

## 验收

- PostgreSQL 18 与 MySQL 8.4 真实隔离库验证两个独立进程的并发首次迁移只有一个执行者、第二个在锁释放后读取最新状态；另验证等待超时、owner 强制退出后的锁释放与再迁移恢复、具名数据库隔离、`down/status` 路径和表名 resolver。
- 默认/all-features workspace tests、fmt、clippy、warnings-denied Rustdoc、no-default-features check 与 `cargo deny check` 通过。

实现与测试入口见 [`src/db/migration_lock.rs`](../../../framework/src/db/migration_lock.rs) 和 [`tests/server/database/migration.rs`](../../../tests/server/database/migration.rs)。

## 非目标

本 ADR 不取代应用 migration 集、自动运行 migration、提供业务级分布式锁、接管手工执行的 SeaORM runner，也不自动修复非事务 DDL 失败留下的部分 schema。
