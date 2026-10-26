# 验收口径

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：v1 各里程碑按 [v1 版本计划](../plans/gen1-plan.md) 验收；以下为当前实现与既有测试记录的边界，不将规划计为完成。

- 原 M0、M1、M2 范围已完成：内核与 HTTP、聚合入口、双库连接/事务/迁移/seed、测试数据库隔离与生成项目验证。具体命令与已有通过记录见 [testing.md](testing.md)。
- M3 本地邮件/SMS driver、认证流程、JWT/refresh、可选 Session、PAT、验证码/限流、Policy/RBAC、国际化、出站 HTTP、加密、审计和签名能力均已实现并完成当前本地验收；真实 Twilio 投递仍待授权账号与目标。详见 `core/auth.md`、`core/mail.md` 与 `testing.md`。
- 2026-10-26 认证总验收在 PostgreSQL 18 / MySQL 8.4 完成：每库 `auth_database` 和 `auth_runtime` 各 6 项通过；新生成项目通过格式、check 和真实 TCP HTTP 全流程。Mailpit 邮件实收已完成；Twilio 实发未宣称。
- M2 基础读写分离已按 ADR-029 实现，并在 PostgreSQL 18.6 / MySQL 8.4.13 真实复制拓扑验证复制暂停、主库新读、scope 隔离及追平。完整集群和代表性业务性能验收仍未完成。
- 缓存、队列、事件、存储仍有待实现范围。标准应用 Redis 默认按 ADR-015 执行，但 Redis driver、完整集群和性能基线尚未验收。
- 版本基线：日常本地开发与 Compose 为 PostgreSQL 18、MySQL 8.4 LTS、Redis 8；常规 CI 为 PostgreSQL 18、MySQL 8.4。PostgreSQL 16 与更旧数据库测试仅作历史专项证据；v1 发布前确定并验证最低支持版本。
- 2026-10-26 配置模块与 `.env` 功能簇验证：`cargo fmt --all --check`、default/all-features workspace clippy 与 test、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features`、`cargo deny check` 与 `git diff --check` 通过。测试覆盖模块路径兼容、配置层级/环境选择/脱敏、production 忽略 `.env` 与生成 `.env.example`。默认生成项目在临时目录以本地未发布 `openrust` patch 实际运行 `cargo check` 和 `cargo test`，通过。另在现有 PostgreSQL 18 和 MySQL 8.4 容器上分别通过以下测试（每库 auth 4 项、connection 4 项、migration 1 项）：`cargo test -p openrust-server --features auth,db-postgres --test auth_database --test db_connection --test db_migration`；对应 MySQL 命令使用 `--features auth,db-mysql`。`cargo deny` 输出 `winnow` 重复版本提示，但 advisory、bans、licenses、sources 均通过。
- 2026-10-26 ADR-031 统一出站 HTTP 功能簇完整门禁：default 与 all-features 的 `fmt --check`、workspace clippy、workspace tests、warnings-denied rustdoc 与 `cargo deny check` 全通过；无默认 feature 时 `cargo check -p openrust --no-default-features` 通过且 normal dependency tree 不含 reqwest。CLI `--http-client` 生成项目 check/test、`apps/server` HTTP client 集成和 15 项 fake/local HTTP client tests 均通过。当前外部第三方真实发送不是 HTTP client contract 的前置门禁；SMTP/短信供应商后续按授权与凭据单独验收。
- ADR-033 邮件/SMS：Mailpit v1.32.0 本机 SMTP 实收、Twilio adapter 请求协议/超时/无重试测试及 `--sms-twilio` 生成项目 check/test 通过；缺真实 Twilio 账号与授权目标，未宣称 SMS 实发。

2026-10-26 ADR-034 应用数据加密/密钥轮换已完成本地实现与完整验证：default/all-features workspace tests、all-features clippy、warnings-denied docs、`cargo deny check`、CLI 生成 `--encryption` 项目的 check/test 与配置脱敏运行均通过；RFC 8452 已知向量测试通过。`aes-gcm-siv` v0.12.1 文档当前说明 crate 实现本身无完整独立安全审计，因此把实现审计与目标平台性能复核保留为 v1 发布前事项；未将算法标准化表述为实现审计认证。

2026-10-26 ADR-035 审计实现验收：PostgreSQL 18 / MySQL 8.4 `auth_database` 各 5 项全通过，包含真实表名 resolver、事务提交/回滚、审计写故障回滚和有界清理；两种数据库的生成 `--auth` 项目均通过格式、check 与真实 HTTP auth flow。workspace default/all-features tests/clippy、warnings-denied docs、no-default-features check、`cargo deny check` 均通过，cargo-deny 仅输出已有重复依赖及未使用 MPL allowance 警告。未提供审计查询 API，也未承诺不可篡改或可靠远程 sink 投递。

2026-10-26 ADR-036 签名链接：PostgreSQL 18 / MySQL 8.4 `auth_database` 各 6 项通过；新生成两种 backend `--auth` 项目均通过格式、check 与真实 TCP HTTP auth 流程，覆盖签名插件启动和撤销迁移。workspace default/all-features clippy/test、warnings-denied docs、no-default-features check、`cargo deny check` 通过；双库全量命令与生成项目命令在本轮实际执行，未依赖旧测试记录。

2026-10-26 M3 认证与安全汇总：PG18 与 MySQL 8.4 的 `auth_database`、`auth_runtime` 每库均 6 项通过；新生成 `--auth` 项目各自通过格式、check、真实数据库迁移和 TCP HTTP `auth_flows`。本地默认服务凭据与仓库测试凭据不匹配，因此用隔离的无持久卷 CI Compose 服务实际执行验收并随后清理；未改动本机既有数据库。真实 Twilio 投递仍因未提供授权凭据和目标而未执行。
