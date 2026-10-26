# 架构

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：根 `openrust` package 以 Rust modules 承载框架能力；CLI、proc-macro、测试工具与聚合应用保留独立 workspace package。详细设计见 [框架设计](plans/framework-design.md)。

## Workspace 与源码布局

```text
openrust (根 package + workspace)
├── src/config/      # Rust 配置加载、类型与校验
├── src/audit.rs     # 显式审计与可替换 sink
├── src/core/        # Application、AppBuilder、Plugin、Command、Error
├── src/http/        # Axum routes、extractors、Tower middleware、serve
├── src/db/          # SeaORM 连接、迁移、seed、方言及 table resolver
├── src/auth/        # JWT、Session、refresh、PAT、认证流程与 Policy
├── src/mail/        # Email/Mailer 与可选 SMTP
├── tests/           # 框架公共 API 集成测试
└── examples/        # 可运行示例
crates/openrust-cli/     # 独立 `openrust` 命令行与模板
crates/openrust-macros/  # proc-macro package
crates/openrust-test/    # 用户 dev-dependency 测试工具
apps/server/             # 同仓库集成宿主；通过验收替代后才可移除
```

框架源码 Rust 模块与运行时配置文件分开：源码位于根 `src/config/`，每个应用（含生成项目）在自身项目根 `config/` 放 TOML/YAML。仓库 `apps/` 是 workspace 内应用目录约定；`openrust new` 创建的是独立项目，具有自己的 `src/`、`config/` 和 `Cargo.toml`。

`cargo metadata --no-deps` 的五个 workspace package 为：根 `openrust`（框架），`openrust-cli`（命令分发），`openrust-macros`（proc-macro 技术边界），`openrust-test`（用户测试 dev-dependency），`openrust-server`（聚合启动与验收宿主）。未实现的 cache/events/queue/storage 空壳已移除；计划中的能力在对应里程碑作为根 package modules 实现，不预建空模块。

## 依赖规则

- 所有框架能力属于同一个 `openrust` package，内部引用沿 `core/config` 等 module boundary；公共稳定入口保持 `openrust::...`。
- 根 package 可选数据库 driver、SMTP 等依赖由 Cargo features 控制；默认构建无需数据库驱动或外部服务。
- PostgreSQL 与 MySQL 的 schema/SQL 差异集中在 `db::dialect` 及明确的数据库适配入口，需为两库提供分支和真实测试。
- 生成应用只依赖 `openrust`；`openrust-test` 单向依赖 `openrust`，根框架不得反向依赖测试工具。
- `apps/server` 只依赖公开 facade；继续承载尚未由根 `tests/` / `examples/` 等价接替的启动、feature 组合、生成项目和双库验收。
- 只有存在技术或分发理由时才保留 package；不得为规划中的能力机械增设空 crate 或空 module。

`openrust-test` 当前提供 `TestDatabase` 隔离工具；公共 `TestApp` / HTTP response assertions 是计划中的 M6 测试体验补齐，不应按 crate 描述或历史计划文字误判为已实现。

## 当前状态

| 边界 | 当前实现与事实 |
| --- | --- |
| `openrust::core`、`openrust::config` | Application/AppBuilder/AppState/Plugin/Command、错误与生命周期、分层配置；见 [core/core.md](core/core.md) |
| `openrust::http` | HttpPlugin、Routes、Validated、Inject、中间件、health/serve；根 `tests/http_*.rs` 覆盖 HTTP 行为；见 [core/http.md](core/http.md) |
| `openrust::db` | SeaORM 双库、连接池、事务、migration/seed、方言与表名解析；显式只读副本路由和 task-local 主库 scope 已实现，完整集群验收仍未完成；见 [core/db.md](core/db.md) |
| `openrust::audit` | 显式事件、详情白名单/脱敏、数据库与自定义 sink、事务内写入和有界清理；见 [core/audit.md](core/audit.md) |
| `openrust::auth` | JWT、Session、refresh、PAT、验证码、AuthFlows、Policy/RBAC 的现有 WIP 已随源码迁移；继续独立按 M3 验收，不把路径迁移算成功能完成；见 [core/auth.md](core/auth.md) |
| `openrust::mail` | Email/Mailer 的 log、fake、可选 SMTP；见 [core/mail.md](core/mail.md) |
| `openrust-cli` | new/dev/generate 与模板，见 [core/cli.md](core/cli.md) |
| `openrust-macros` | `#[openrust::resource]` proc-macro，见 [core/macros.md](core/macros.md) |
| `openrust-test` | PostgreSQL/MySQL `TestDatabase`，见 [core/test.md](core/test.md) |
| `apps/server` | 聚合入口及启动/数据库/认证集成验证，见 [core/server.md](core/server.md) |
| cache/events/queue/storage | 尚未实现；按 v1 计划后续在 root package 按能力实现 |

## 扩展模型

应用模块与第三方包都通过 `Plugin` 接入：`build` 阶段同步注册服务、命令、路由、迁移、配置段等；所有插件构建完成并冻结后按注册序依次调用异步 `finish`。HTTP 路由复用 `RouteRegistry` 与 Tower middleware。当前没有并列的全局 Hooks API，也没有插件 shutdown callback；生命周期扩展边界见 [ADR-023](decisions/adr-023-loco-application-experience-evaluation.md)。不提供动态插件加载。

能力驱动遵循 [ADR-009](decisions/adr-009-extensible-capability-drivers.md)：提供可靠默认实现与可由第三方接入的公开契约，集中后端差异并明确能力边界。数据库扩展沿用 SeaORM `DatabaseConnection`、`Db` 和 Plugin 生命周期；v1 官方支持 PostgreSQL/MySQL，不把其他 backend 的连接成功视为完整框架支持。集群部署跨里程碑推进，memory、本地 file、单节点磁盘不能充当共享权威状态，详见 [集群计划](plans/v1-cluster-deployment.md)。
