# openrust-test

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：测试工具 crate 当前实现边界。

## 当前实现

- `TestDatabase::create::<M>(admin_url)` 为一次测试创建唯一数据库并运行应用迁移。
- PostgreSQL 用临时迁移模板执行数据库克隆；MySQL 每次新建数据库并运行迁移。MySQL 连接强制 TLS，session 时区设为 `+00:00`。
- 隔离 URL 保留原 URL 凭据与查询参数，只替换 database path。管理用户必须有 CREATE/DROP DATABASE 权限。
- `close().await` 关闭连接并等待数据库删除；guard 在 Tokio runtime 内 drop 时安排 best-effort 清理。
- `Debug` 遮蔽 database URL。

## 后续边界

`TestApp`、HTTP response assertions、Factory 与 fake driver assertions 仍在 M3-M5 范围。

## 验证

`tests/database.rs` 在 PostgreSQL 14 和 MySQL 8.0 上分别创建两个实例，验证迁移表存在、数据彼此隔离并显式删除数据库。CI 的 MySQL 隔离步骤使用 root admin URL；其他普通集成测试使用最低权限 `openrust` 用户。
