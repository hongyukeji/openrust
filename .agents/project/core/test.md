# testing

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：测试工具 crate 当前实现边界。

## 当前实现

- `TestDatabase::create::<M>(admin_url)` 为一次测试创建唯一数据库并运行应用迁移。
- PostgreSQL 用临时迁移模板执行数据库克隆；MySQL 每次新建数据库并运行迁移。MySQL 连接强制 TLS，session 时区设为 `+00:00`。
- 隔离 URL 保留原 URL 凭据与查询参数，只替换 database path。管理用户必须有 CREATE/DROP DATABASE 权限。
- `close().await` 关闭连接并等待数据库删除；guard 在 Tokio runtime 内 drop 时安排 best-effort 清理。
- `Debug` 遮蔽 database URL。
- 独立 `testing` package 位于 `tools/testing/`，保持 `testing::TestDatabase` 应用入口；实现由根 `openrust::testing` 提供。
- 隔离保证仅覆盖已实现的 PostgreSQL 与 MySQL；其他 SeaORM backend 需要独立隔离策略，不因 `Db::initialize()` 可接入而推断兼容。

## 后续边界

`TestApp` 与 HTTP response assertions 是 M6 待实现项，按 [ADR-023](../decisions/adr-023-loco-application-experience-evaluation.md) 复用应用 finalize、公开 router、Tower oneshot 与本 crate 数据库隔离工具。Factory 与 fake driver assertions 仅随具体能力 contract 逐项补齐，不属于当前已交付 surface。

## 验证

`tests/database.rs` 历史验证曾在 PostgreSQL 14 和 MySQL 8.0 上分别创建两个实例，验证迁移表存在、数据彼此隔离并显式删除数据库。CI 的 MySQL 隔离步骤使用 root admin URL；其他普通集成测试使用最低权限 `openrust` 用户。
