# `openrust::db`

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：M2 数据层当前实现边界。

## 当前实现

- `DatabaseConfig` 从 `[database]` 读取 URL、池大小和连接/获取超时；自定义 `Debug` 会遮蔽 URL。
- `DatabaseBackend` 集中解析官方 PostgreSQL/MySQL URL scheme 与 feature 状态；它只描述 v1 官方支持集合，扩展插件可用 `Db::backend()` 读取 SeaORM 实际 backend。
- `DatabasePlugin` 验证 pool 参数与 URL 对应的 backend feature，在 `finish` 阶段连接 SeaORM pool；运行时当前接受 PostgreSQL 16+ / MySQL 8.4+。日常开发和常规 CI 使用 PostgreSQL 18 / MySQL 8.4 LTS；运行时门槛不是最终发布支持承诺，v1 发布前须按定稿最低版本调整。
- `DatabasePlugin` 注册 `openrust::core::RateLimiter`，其官方 `DatabaseRateLimitStore` 通过当前 `Db` 实例在 PostgreSQL/MySQL 上原子更新固定窗口记录；`DatabaseRateLimitMigration` 由应用 migrator 显式注册，启动不会自动建表。
- 限流 key 仅存 SHA-256 digest（不抵抗低熵输入猜测）；每个 store 实例每 256 次消费最多清理 512 行已过期记录，窗口长度一并持久化。缺少迁移、数据库故障与不支持 backend 均以错误传播，不作为未命中或放行。
- 自定义 `Plugin` 可在 build 注册 `Db::default()`，finish 建立 SeaORM `DatabaseConnection` 后调用 `Db::initialize()`；`Db::backend()` 读取实际 backend。一个应用仅注册一个数据库插件；自定义插件负责自身连接配置及所需能力校验。
- MySQL pool 的每条新连接将会话时区设为 `+00:00`，与 ADR-002 一致。
- `Db` 是可克隆服务句柄；`connection()` 返回 SeaORM `DatabaseConnection`。插件完成初始化前调用会返回内部错误。
- `Db::transaction()` 将闭包放在 SeaORM 事务中执行；回调成功则提交，回调返回错误则回滚，数据库与回调错误统一映射为框架内部错误。
- `DatabaseConfig` 支持 `table_prefix`（默认 `tb_`，空值禁用）和 `table_names` 完整物理名映射；`Db` 持有不可变 `DatabaseTableNames`，提供物理名与 backend quoting。标识符只允许 ASCII 字母/数字/下划线；连接初始化时按 PostgreSQL 63-byte、MySQL 64-byte 限制校验映射和框架核心表名。
- `MigrationPlugin` 在每次 up/down/status 调用中用 Tokio task-local scope 提供 `DatabaseTableNames`；OpenRust AuthMigration 无 scope 时失败。手写 SeaORM migration 可用 `DatabaseTableNames::current()` 获取当前 resolver。`TestDatabase::create_with_table_names()` 用相同 scope 执行隔离迁移。
- 认证迁移、Session/PAT/refresh-token SQL 使用同一 resolver；应用自行书写的 raw SQL 不自动改写，必须使用 `Db::table_name()` / `quoted_table_name()`。表名前缀或映射不会迁移既有 schema 或数据。
- `MigrationPlugin<M>` 注册 `migrate` 命令，调用应用 migrator 的 `up`、`down` 和 `status`；`sea_orm_migration` 从 `openrust::database` re-export。
- `SeedPlugin::new(seed_fn)` 注册 `db seed`；异步回调接收 `Db`，种子数据与幂等性由应用负责。
- `dialect::timestamp_utc(name, backend)` 生成 PostgreSQL `TIMESTAMP WITH TIME ZONE` / MySQL `DATETIME(6)`；`dialect::json(name, backend)` 生成 PostgreSQL `JSONB` / MySQL `JSON`；两者返回 `DbErr`，对其他 backend 显式拒绝。
- `db-postgres` 与 `db-mysql` 是可选 feature；未启用 backend feature 时默认不启用具体数据库 driver。
- 数据库连接失败只给出 backend 名，不把连接 URL 或驱动错误写入启动错误。

## 后续边界

官方 v1 数据库支持限定 PostgreSQL 与 MySQL。日常开发与常规 CI 使用 PostgreSQL 18 / MySQL 8.4 LTS。PostgreSQL 16 是此前日常基线和已验证版本，保留为历史验证证据，不再作为当前日常基线。只在跨度较大、涉及版本专属行为或出现明确兼容风险时专项验证其他版本。发布前按届时官方生命周期确定并验证最低支持版本；PostgreSQL 14 / MySQL 8.0 仅为历史测试证据。其他 SeaORM backend 可通过自定义连接插件提供 `Db`，但须自行完成该应用使用到的迁移、方言、能力验证和测试隔离适配；不表示认证、队列等所有框架功能兼容。新增官方 backend 需独立完成 feature、能力检查、方言、生成器及双向测试闭环。

## 验证

`cargo test -p openrust` 验证默认配置、URL 遮蔽、未初始化句柄、pool 参数、backend feature 与 table-name resolver。历史 PostgreSQL 14 / MySQL 8.0 隔离库测试覆盖 resolver-aware 迁移、独立 limiter 实例的并发共享配额、SHA-256 存储、固定窗口重置、过期记录清理、迁移 up/status/down、UTC/JSON 列与 seed 命令。MySQL 强制 Rustls TLS，并断言协商了 TLS 且连接会话时区为 `+00:00`。2026-10-26 在当时服务上复跑了 `apps/server/tests/db_connection.rs`、`db_migration.rs` 及认证数据库流程：PostgreSQL 16 与 MySQL 8.4 均通过；这保留为历史验证，当前日常开发与常规 CI 目标为 PostgreSQL 18 / MySQL 8.4 LTS。数据库 URL 的应用配置键为 `DATABASE_URL` 或所选应用前缀（默认 `APP_DATABASE__URL`）；最终最低支持版本待发布前确定。
