# 验收口径

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：v1 各里程碑按 [v1 版本计划](../plans/gen1-plan.md) 验收；以下为当前实现与既有测试记录的边界，不将规划计为完成。

- 原 M0、M1、M2 范围已完成：内核与 HTTP、聚合入口、双库连接/事务/迁移/seed、测试数据库隔离与生成项目验证。具体命令与已有通过记录见 [testing.md](testing.md)。
- M3 邮件 driver 已实现并有 fake、配置、集成接入及生成项目检查；没有真实 SMTP 投递验收记录。认证 ADR-012 为 contract，认证 crate 仍是骨架，身份/会话/PAT/授权不能算已交付。
- 基础读写分离、业务流程扩展点等新增数据层范围待实现，原 M2 完成标签不覆盖这些新增要求。
- 缓存、队列、事件、存储仍为骨架。标准应用 Redis 默认按 ADR-015 执行，但 Redis driver、完整集群、真实主副本复制拓扑与性能基线尚未验收。
- 本次进度校准依据代码和既有测试记录，未重跑全量 Rust 门禁；未来验收必须更新实际命令与结果，不能将文档修改或接口契约当作测试通过。
- 2026-10-26 配置模块与 `.env` 功能簇验证：`cargo fmt --all --check`、default/all-features workspace clippy 与 test、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features`、`cargo deny check` 与 `git diff --check` 通过。测试覆盖模块路径兼容、配置层级/环境选择/脱敏、production 忽略 `.env` 与生成 `.env.example`。另在现有 PostgreSQL 18 和 MySQL 8.4 容器上分别通过以下测试（每库 auth 4 项、connection 4 项、migration 1 项）：`cargo test -p openrust-server --features auth,db-postgres --test auth_database --test db_connection --test db_migration`；对应 MySQL 命令使用 `--features auth,db-mysql`。`cargo deny` 输出 `winnow` 重复版本提示，但 advisory、bans、licenses、sources 均通过。
