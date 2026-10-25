# openrust-db

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：M2 数据层当前实现边界。

## 当前实现

- `DatabaseConfig` 从 `[database]` 读取 URL、池大小和连接/获取超时；自定义 `Debug` 会遮蔽 URL。
- `DatabaseBackend` 集中解析官方 PostgreSQL/MySQL URL scheme 与 feature 状态；它只描述 v1 官方支持集合，扩展插件可用 `Db::backend()` 读取 SeaORM 实际 backend。
- `DatabasePlugin` 验证 pool 参数与 URL 对应的 backend feature，在 `finish` 阶段连接 SeaORM pool，并拒绝低于 PostgreSQL 14 / MySQL 8.0 的服务版本。
- 自定义 `Plugin` 可在 build 注册 `Db::default()`，finish 建立 SeaORM `DatabaseConnection` 后调用 `Db::initialize()`；`Db::backend()` 读取实际 backend。一个应用仅注册一个数据库插件；自定义插件负责自身连接配置及所需能力校验。
- MySQL pool 的每条新连接将会话时区设为 `+00:00`，与 ADR-002 一致。
- `Db` 是可克隆服务句柄；`connection()` 返回 SeaORM `DatabaseConnection`。插件完成初始化前调用会返回内部错误。
- `Db::transaction()` 将闭包放在 SeaORM 事务中执行；回调成功则提交，回调返回错误则回滚，数据库与回调错误统一映射为框架内部错误。
- `MigrationPlugin<M>` 注册 `migrate` 命令，调用应用 migrator 的 `up`、`down` 和 `status`；`sea_orm_migration` 从 `openrust::database` re-export。
- `SeedPlugin::new(seed_fn)` 注册 `db seed`；异步回调接收 `Db`，种子数据与幂等性由应用负责。
- `dialect::timestamp_utc(name, backend)` 生成 PostgreSQL `TIMESTAMP WITH TIME ZONE` / MySQL `DATETIME(6)`；`dialect::json(name, backend)` 生成 PostgreSQL `JSONB` / MySQL `JSON`；两者返回 `DbErr`，对其他 backend 显式拒绝。
- `db-postgres` 与 `db-mysql` 是可选 feature；未启用 backend feature 时门面默认不包含 `openrust-db`。
- 数据库连接失败只给出 backend 名，不把连接 URL 或驱动错误写入启动错误。

## 后续边界

官方 v1 数据库支持仍限定 PostgreSQL 14+ / MySQL 8.0+。其他 SeaORM backend 可通过自定义连接插件提供 `Db`，但须自行完成该应用使用到的迁移、方言、能力验证和测试隔离适配；不表示认证、队列等所有框架功能兼容。新增官方 backend 需独立完成 feature、能力检查、方言、生成器及双向测试闭环。

## 验证

`cargo test -p openrust-db` 验证默认配置、URL 遮蔽、未初始化句柄、pool 参数和 backend feature 校验。设置 `OPENRUST_TEST_DATABASE_URL` 后，迁移集成测试使用独立自定义连接插件验证公开 `Db::initialize()` 与现有 MigrationPlugin/SeedPlugin 协作；同时验证事务提交/回滚、`SKIP LOCKED`、迁移重复 up/status/down/重放，以及 UTC 时间与 JSON 列类型。MySQL 强制 Rustls TLS，并断言协商了 TLS 且连接会话时区为 `+00:00`。真实支持 PostgreSQL 14+ / MySQL 8.0+。
