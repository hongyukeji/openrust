# testing

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：测试工具 crate 与根公开测试宿主当前实现边界。

## 当前实现

- `TestDatabase::create::<M>(admin_url)` 为一次测试创建唯一数据库并运行应用迁移。
- PostgreSQL 用临时迁移模板执行数据库克隆；MySQL 每次新建数据库并运行迁移。MySQL 连接强制 TLS，session 时区设为 `+00:00`。
- 隔离 URL 保留原 URL 凭据与查询参数，只替换 database path。管理用户必须有 CREATE/DROP DATABASE 权限。
- `close().await` 关闭连接并等待数据库删除；guard 在 Tokio runtime 内 drop 时安排 best-effort 清理。
- `Debug` 遮蔽 database URL。
- `openrust::testing::{TestApp, TestAppBuilder, TestResponse}` 由根框架公开；TestApp 使用生产 Application/Provider/Router 构建路径，支持 Provider 移除、全 register 后 typed 覆盖、重建、有界 oneshot 请求和 HTTP 状态/header/JSON 断言。它不替代真实 TCP、浏览器或多进程验收。
- 独立 `testing` package 位于 `tools/testing/`，保持 `testing::TestDatabase` 应用入口；实现由根 `openrust::testing` 提供。
- 隔离保证仅覆盖已实现的 PostgreSQL 与 MySQL；其他 SeaORM backend 需要独立隔离策略，不因 `Db::initialize()` 可接入而推断兼容。

## 后续边界

Factory/model factory states 仍是 [R6](../plans/laravel-rebuild.md#5-按依赖实施) 待实现能力；不把 SeaORM Entity 或 TestDatabase 冒充 Laravel Factory。Fake driver assertions 随具体能力的公开 contract 实现，当前应使用模块 TestApp 显式安装捕获 driver 并拒绝 production。

## 验证

`tests/database.rs` 历史验证曾在 PostgreSQL 14 和 MySQL 8.0 上分别创建两个实例，验证迁移表存在、数据彼此隔离并显式删除数据库。CI 的 MySQL 隔离步骤使用 root admin URL；其他普通集成测试使用最低权限 `openrust` 用户。
