# `openrust::db`

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：M2 数据层当前实现边界。

## 当前实现

- `DatabaseConfig` 定义在 `openrust::config::database`，旧的 `openrust::database::DatabaseConfig` 导出保持可用。运行 TOML 位于应用根 `config/`；本地 URL 可用 `.env`，生产环境使用进程变量。

- `DatabaseConfig` 从 `[database]` 读取 URL、池大小和连接/获取超时；自定义 `Debug` 会遮蔽 URL。
- `DatabaseBackend` 集中解析官方 PostgreSQL/MySQL URL scheme 与 feature 状态；它只描述 v1 官方支持集合，扩展插件可用 `Db::backend()` 读取 SeaORM 实际 backend。SQLite 不属于官方矩阵，可由自定义插件使用通用 `Db` 连接/事务接口。
- `DatabasePlugin` 验证 pool 参数与 URL 对应的 backend feature，在 `finish` 阶段连接 SeaORM pool；运行时按暂定最低支持接受 PostgreSQL 14+ / MySQL 8.0+。日常开发基线为 PostgreSQL 18 / MySQL 8.4 LTS / Redis 8；数据库常规 CI 使用 PostgreSQL 18 / MySQL 8.4。v1 发布前须复核官方生命周期并完成最低版本能力矩阵验证。
- `DatabasePlugin` 注册 `openrust::core::RateLimiter`，其官方 `DatabaseRateLimitStore` 通过当前 `Db` 实例在 PostgreSQL/MySQL 上原子更新固定窗口记录；`DatabaseRateLimitMigration` 由应用 migrator 显式注册，启动不会自动建表。
- 限流 key 仅存 SHA-256 digest（不抵抗低熵输入猜测）；每个 store 实例每 256 次消费最多清理 512 行已过期记录，窗口长度一并持久化。缺少迁移、数据库故障与不支持 backend 均以错误传播，不作为未命中或放行。
- 自定义 `Plugin` 可在 build 注册 `Db::default()`，finish 建立 SeaORM `DatabaseConnection` 后调用 `Db::initialize()`；`initialize_with_replicas()` 接受同 backend 的主库和副本连接；`Db::backend()` 读取主库 backend。一个应用仅注册一个数据库插件；自定义插件负责自身连接配置及所需能力校验。当前通用初始化与 `DatabaseTableNames` 支持 PostgreSQL、MySQL、SQLite；其他 SeaORM backend 在定义表名 quoting 前会明确拒绝。
- MySQL pool 的每条新连接将会话时区设为 `+00:00`，与 ADR-002 一致。
- `Db` 是可克隆服务句柄；`connection()` / `primary_connection()` 始终返回主库，`read_connection()` 显式轮询只读副本（未配置副本时返回主库）。`with_primary(future)` 让当前 Tokio task scope 内的读入口固定主库；独立 `tokio::spawn` 不继承 scope。插件完成初始化前调用会返回内部错误。
- 读写分离使用显式 API，不解析 SQL；调用方须保证副本入口只执行可接受复制延迟的只读操作。官方插件连接并校验所有配置副本，任一失败即启动失败；不自动重试或回退。每个 pool 使用相同 pool 上限，因此副本数为 N 时总连接上限为 `(N + 1) * max_connections`。
- `Db::transaction()` 将闭包放在 SeaORM 事务中执行；回调成功则派发可选 `TransactionBeforeCommit` 并提交，回调或提交前监听器返回错误则回滚，数据库与回调错误统一映射为框架内部错误。标准 `DatabasePlugin` 同时发现 `EventsPlugin` 时才绑定生命周期事件；自定义数据库 plugin 不自动接入。
- 成功提交后只通知 `TransactionCommitted` 的 direct listeners。其错误仅记录稳定错误码，返回值仍表示数据库已提交；queued listeners 不在此边界执行。需可靠持久派发的副作用须在提交前事件中通过事务队列落库。直接 SeaORM/raw SQL 不经过此 API，不触发事务事件。
- `CoordinationMigration` 需由应用 migrator 显式注册；`SharedLocks` 基于数据库 UTC 时钟提供跨实例 owner token、TTL、单调 fencing token、条件续约/释放及事务内 `assert_current_in_transaction`。过期可接管，但 TTL 不会终止暂停的旧进程或 fence 任意外部副作用。`RequestIdempotency` 由应用显式传入 scope/key/规范化 fingerprint；同键不同 fingerprint 返回冲突，活跃 owner 返回处理中，完成后可重放不超过 64 KiB 的 JSON body。业务写入与 `complete_in_transaction` 必须同事务；lease 到期可恢复，完成记录用有界 `prune_expired(1..=1000)` 清理。表名使用 Db resolver，SHA-256 摘要和 256-bit CSPRNG token 不写日志。真实 PG18/MySQL8.4 双池竞争、旧 owner 防护、scope/冲突/事务回滚/回放/有界清理已通过。
- `DatabaseConfig` 支持 `table_prefix`（默认 `tb_`，空值禁用）和 `table_names` 完整物理名映射；`Db` 持有不可变 `DatabaseTableNames`，提供物理名与 backend quoting。标识符只允许 ASCII 字母/数字/下划线；连接初始化时按 PostgreSQL 63-byte、MySQL 64-byte 限制校验映射和框架核心表名。
- `MigrationPlugin` 在每次 up/down/status 调用中用 Tokio task-local scope 提供 `DatabaseTableNames`；OpenRust AuthMigration 无 scope 时失败。手写 SeaORM migration 可用 `DatabaseTableNames::current()` 获取当前 resolver。`TestDatabase::create_with_table_names()` 用相同 scope 执行隔离迁移。
- 认证迁移、Session/PAT/refresh-token SQL 使用同一 resolver；应用自行书写的 raw SQL 不自动改写，必须使用 `Db::table_name()` / `quoted_table_name()`。表名前缀或映射不会迁移既有 schema 或数据。
- `MigrationPlugin<M>` 注册 `migrate` 命令，调用应用 migrator 的 `up`、`down` 和 `status`；`sea_orm_migration` 从 `openrust::database` re-export。
- `SeedPlugin::new(seed_fn)` 注册 `db seed`；异步回调接收 `Db`，种子数据与幂等性由应用负责。
- `openrust generate model` 与 `generate migration` 可一致传入 `--uuid-id`（`db-uuid` feature，UUIDv7）及 `--soft-delete`（nullable UTC `deleted_at`）；实体生成显式 `Entity::not_deleted()` scope。删除范围不会自动作用于 `find()`、raw SQL 或其他入口，恢复由应用将 `deleted_at` 清空；模型和迁移选项须匹配。
- `dialect::timestamp_utc(name, backend)` 生成 PostgreSQL `TIMESTAMP WITH TIME ZONE` / MySQL `DATETIME(6)`；`dialect::json(name, backend)` 生成 PostgreSQL `JSONB` / MySQL `JSON`；两者返回 `DbErr`，对其他 backend 显式拒绝。
- `db-postgres` 与 `db-mysql` 是可选 feature；未启用 backend feature 时默认不启用具体数据库 driver。
- 数据库连接失败只给出 backend 名，不把连接 URL 或驱动错误写入启动错误。

## 后续边界

嵌套事务与 savepoint、重试、直接 SeaORM/原生 SQL 逃生口均不受事务事件覆盖；提交前与提交后失败边界见 [ADR-041](../decisions/adr-041-transaction-and-auth-flow-events.md)。

官方 v1 数据库支持限定 PostgreSQL 与 MySQL，暂定最低版本为 PostgreSQL 14+ / MySQL 8.0+。日常开发基线为 PostgreSQL 18 / MySQL 8.4 LTS / Redis 8，数据库 CI 使用 PostgreSQL 18 / MySQL 8.4。只在跨度较大、涉及版本专属行为或出现明确兼容风险时专项验证其他版本。v1 发布前按届时官方生命周期复核并验证最低版本下框架承诺的能力；已有 PostgreSQL 14 / MySQL 8.0 历史集成结果不单独代表当前完整支持。

SQLite 自定义插件通过 SeaORM 自行创建连接后，可使用 `Db::initialize()`、`Db::transaction()`、通用表名解析与应用自有 SeaORM `MigrationPlugin`。该连接不使 SQLite 成为 OpenRust 官方支持后端：`DatabasePlugin`、官方 URL/feature 检查、方言 helper、框架认证/队列/协调迁移与 `TestDatabase` 仍限定 PostgreSQL/MySQL，调用不支持的能力时应显式失败。独立 consumer 示例见 `examples/db-custom-sqlite/`。其他 SeaORM backend 还须先实现受控的表名 quoting，再适配实际使用的迁移、方言与测试隔离；不能由连接成功推断认证/队列兼容。新增官方 backend 仍需独立完成 feature、能力检查、方言、生成器及真实测试闭环。

## 验证

`cargo test -p openrust` 验证默认配置、URL 遮蔽、未初始化句柄、pool 参数、backend feature 与 table-name resolver。历史 PostgreSQL 14 / MySQL 8.0 隔离库测试覆盖 resolver-aware 迁移、独立 limiter 实例的并发共享配额、SHA-256 存储、固定窗口重置、过期记录清理、迁移 up/status/down、UTC/JSON 列与 seed 命令。MySQL 强制 Rustls TLS，并断言协商了 TLS 且连接会话时区为 `+00:00`。2026-10-26 根模块迁移后的认证数据库与生成项目测试已在 PostgreSQL 16 / MySQL 8.4 通过。2026-10-26 在 PostgreSQL 18.6 / MySQL 8.4.13 真实复制拓扑分别运行 `OPENRUST_TEST_VERIFY_REPLICA_LAG=1 cargo test -p openrust-server --features db-postgres --test db_connection` 与 `OPENRUST_TEST_VERIFY_REPLICA_LAG=1 cargo test -p openrust-server --features db-mysql --test db_connection` 并通过：确认独立副本身份、暂停复制时旧读、主库 scope 新读、两个 App 的 scope 隔离、spawn 不继承 scope、恢复后追平；`initialize_with_replicas` 的自定义接入也通过。双库 release `db_routing` 500 次 `SELECT 1` 微基准已记录于 `memory/testing.md`，不代表业务负载性能提升。数据库 URL 的应用配置键为 `DATABASE_URL` 或所选应用前缀（默认 `APP_DATABASE__URL`）；暂定最低支持版本见 [ADR-002](../decisions/adr-002-seaorm-dual-database.md)，完整兼容矩阵待 v1 发布前复核。
