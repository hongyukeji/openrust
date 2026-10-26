# 验收口径

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：v1 各里程碑按 [v1 版本计划](../plans/gen1-plan.md) 验收；以下为当前实现与既有测试记录的边界，不将规划计为完成。

- 原 M0、M1、M2 范围已完成：内核与 HTTP、聚合入口、双库连接/事务/迁移/seed、测试数据库隔离与生成项目验证。具体命令与已有通过记录见 [testing.md](testing.md)。
- M3 邮件 driver、认证流程、JWT/refresh、可选 Session、PAT、验证码/限流与基础 Policy/RBAC 已实现部分能力；完整 M3 覆盖和真实邮件/短信供应商交付仍未验收。具体边界以 `core/auth.md`、`core/mail.md` 和当前功能簇记录为准。
- M2 基础读写分离已按 ADR-029 实现，并在 PostgreSQL 18.6 / MySQL 8.4.13 真实复制拓扑验证复制暂停、主库新读、scope 隔离及追平。完整集群和代表性业务性能验收仍未完成。
- 缓存、队列、事件、存储仍有待实现范围。标准应用 Redis 默认按 ADR-015 执行，但 Redis driver、完整集群和性能基线尚未验收。
- 版本基线：日常本地开发与 Compose 为 PostgreSQL 18、MySQL 8.4 LTS、Redis 8；常规 CI 为 PostgreSQL 18、MySQL 8.4。PostgreSQL 16 与更旧数据库测试仅作历史专项证据；v1 发布前确定并验证最低支持版本。
- 2026-10-26 配置模块与 `.env` 功能簇验证：`cargo fmt --all --check`、default/all-features workspace clippy 与 test、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features`、`cargo deny check` 与 `git diff --check` 通过。测试覆盖模块路径兼容、配置层级/环境选择/脱敏、production 忽略 `.env` 与生成 `.env.example`。默认生成项目在临时目录以本地未发布 `openrust` patch 实际运行 `cargo check` 和 `cargo test`，通过。另在现有 PostgreSQL 18 和 MySQL 8.4 容器上分别通过以下测试（每库 auth 4 项、connection 4 项、migration 1 项）：`cargo test -p openrust-server --features auth,db-postgres --test auth_database --test db_connection --test db_migration`；对应 MySQL 命令使用 `--features auth,db-mysql`。`cargo deny` 输出 `winnow` 重复版本提示，但 advisory、bans、licenses、sources 均通过。
