# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## 后续里程碑

- OpenAPI：后续里程碑用 `utoipa` 生成 schema，并在非生产环境挂载 `/docs`（ADR-008 §6）。

## M2 数据层（进行中）

- `openrust-db` 连接池 / plugin、`Db::transaction`、`MigrationPlugin` 与运行期迁移命令已实现；Rust 1.97 MSRV、MySQL 强制 TLS，以及 PostgreSQL 14 / MySQL 8.0 的连接、事务、`SKIP LOCKED`、迁移 up/down/status 验收已完成。
- 后续 M2 数据层功能簇：UTC/JSON dialect helper 与 seed，再接入 `openrust-test` 数据库隔离和 `openrust new --db` 模板。

## M3 邮件、认证与授权（M2 收口后领取）

- 确认 `CurrentUser` 能与 `Inject` 共存于同一 handler，在 `openrust-auth` 测试中覆盖。

## M4/M5 Redis 驱动（对应里程碑时领取）

- Redis 驱动建立配置段时，将 `REDIS_URL` 映射到连接配置，并保证同键的 `OPENRUST_` 变量优先。

## 阻塞

无。
