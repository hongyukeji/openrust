# openrust-db

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：M2 数据层当前实现边界。

## 当前实现

- `DatabaseConfig` 从 `[database]` 读取 URL、池大小和连接/获取超时；自定义 `Debug` 会遮蔽 URL。
- `DatabasePlugin` 验证 pool 参数与 URL 对应的 backend feature，在 `finish` 阶段连接 SeaORM pool。
- MySQL pool 的每条新连接将会话时区设为 `+00:00`，与 ADR-002 一致。
- `Db` 是可克隆服务句柄；`connection()` 返回 SeaORM `DatabaseConnection`。插件完成初始化前调用会返回内部错误。
- `Db::transaction()` 将闭包放在 SeaORM 事务中执行；回调成功则提交，回调返回错误则回滚，数据库与回调错误统一映射为框架内部错误。
- `MigrationPlugin<M>` 注册 `migrate` 命令，调用应用 migrator 的 `up`、`down` 和 `status`；`sea_orm_migration` 从 `openrust::database` re-export。
- `SeedPlugin::new(seed_fn)` 注册 `db seed`；异步回调接收 `Db`，种子数据与幂等性由应用负责。
- `dialect::timestamp_utc(name, backend)` 生成 PostgreSQL `TIMESTAMP WITH TIME ZONE` / MySQL `DATETIME(6)`；`dialect::json(name, backend)` 生成 PostgreSQL `JSONB` / MySQL `JSON`。
- `db-postgres` 与 `db-mysql` 是可选 feature；未启用 backend feature 时门面默认不包含 `openrust-db`。
- 数据库连接失败只给出 backend 名，不把连接 URL 或驱动错误写入启动错误。

## 后续边界

每用例独立数据库测试，以及 `openrust new --db` 模板仍在 M2 范围。

## 验证

`cargo test -p openrust-db` 验证默认配置、URL 遮蔽、未初始化句柄、pool 参数和 backend feature 校验。设置 `OPENRUST_TEST_DATABASE_URL` 后，集成测试连接真实服务并验证事务提交/回滚、`SKIP LOCKED`、迁移的重复 up/status/down/重放，以及 UTC 时间与 JSON 列类型；MySQL 强制 Rustls TLS，并断言协商了 TLS 且连接会话时区为 `+00:00`。支持 PostgreSQL 14+ / MySQL 8.0+。
