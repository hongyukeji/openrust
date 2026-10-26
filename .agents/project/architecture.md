# 架构

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：根 `openrust` package 以 Rust modules 承载框架能力；CLI、proc-macro、测试工具与聚合应用保留独立 workspace package。详细设计见 [框架设计](plans/framework-design.md)。

## 已确认的交付方式

整套源码 workspace 交付，业务放在 `apps/<name>/`。内部包目标为 `cli`、`macros`、`testing`、`server`，根 `openrust` 与公共 API 保留；所有 package 禁用发布。代码目前仍是下文列出的旧包名，迁移清单与验收见 [ADR-039](decisions/adr-039-source-workspace-distribution.md)。

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
├── src/queue.rs     # 数据库任务与 worker（queue feature）
├── src/schedule.rs  # UTC 调度（queue feature）
├── tests/           # 框架公共 API 集成测试
└── examples/        # 可运行示例
cli/                     # 独立 `openrust` 命令行与模板（package: cli）
macros/                  # proc-macro package（macros）
testing/                 # 用户 dev-dependency 测试工具（testing）
apps/server/             # 同仓库集成宿主；通过验收替代后才可移除
```

框架源码 Rust 模块与运行时配置文件分开：源码位于根 `src/config/`，每个应用（含生成项目）在自身项目根 `config/` 放 TOML/YAML。仓库 `apps/` 是业务应用入口；CLI 通过 Cargo metadata 识别源码 workspace，默认生成到 `apps/<name>/`，并把应用及可选 migration 登记为 workspace member。应用使用本地 workspace/path 依赖；`dev`、`serve` 与代理命令按目标 app 的 manifest 显式选包，并从应用工作目录加载配置。外部独立目录不是生成目标，详细边界见 ADR-039。

当前 manifest 的五个 workspace package 为：根 `openrust`（框架），`cli`（命令分发），`macros`（proc-macro 技术边界），`testing`（用户测试 dev-dependency），`server`（聚合启动与验收宿主）。原能力空壳已移除；cache/events/storage 等剩余能力直接在根 package 实现，不预建空模块。

## 目录职责与验证入口

按 [ADR-021](decisions/adr-021-loco-style-module-layout.md)，CLI、proc-macro 和测试工具的独立编译/依赖边界保留在 `cli/`、`macros/`、`testing/`；包名按 ADR-039 迁移为目录同名。根框架保留唯一 `openrust` package。

示例按应用分组，例如 `examples/hello/main.rs` 和后续 `examples/demo/`；根与 `apps/server` 测试按能力组织，通过明确 `[[test]]` targets 保持测试名称、feature 门控和 Cargo 发现。queue/schedule 的 PostgreSQL 与 MySQL 集成命令已进入 CI；迁移已完成五包/265 条测试清单、workspace、双库和生成项目验证。旧 `crates/` 已移除。

`apps/server` 保留为集成宿主：`tests/server/aggregate.rs` 依赖其二进制与配置目录，双库、认证、队列、调度测试由其 feature forwarding 和隔离测试数据库驱动。根公共 API 测试与 hello 示例没有替代这些职责；移除它仍须另行提供等价启动进程和数据库验收。`apps/` 目录约定保留。

## 依赖规则

- 所有框架能力属于同一个 `openrust` package，内部引用沿 `core/config` 等 module boundary；公共稳定入口保持 `openrust::...`。
- 根 package 可选数据库 driver、SMTP 等依赖由 Cargo features 控制；默认构建无需数据库驱动或外部服务。
- PostgreSQL 与 MySQL 的 schema/SQL 差异集中在 `db::dialect` 及明确的数据库适配入口，需为两库提供分支和真实测试。
- 生成应用只依赖 `openrust`；`testing` 单向依赖 `openrust`，根框架不得反向依赖测试工具。
- `apps/server` 只依赖公开 facade；继续承载尚未由根 `tests/` / `examples/` 等价接替的启动、feature 组合、生成项目和双库验收。
- 只有存在编译、工具或测试依赖边界时才保留 package；不得为规划中的能力机械增设空 crate 或空 module。

`testing` 当前提供 `TestDatabase` 隔离工具；公共 `TestApp` / HTTP response assertions 是计划中的 M6 测试体验补齐，不应按 crate 描述或历史计划文字误判为已实现。

## 当前状态

| 边界 | 当前实现与事实 |
| --- | --- |
| `openrust::core`、`openrust::config` | Application/AppBuilder/AppState/Plugin/Command、错误与生命周期、分层配置；见 [core/core.md](core/core.md) |
| `openrust::http`、`openrust::i18n` | 路由、提取器、响应、中间件、health/serve、可选出站 HTTP 与请求级国际化；通用 HTTP 限流层、OpenAPI、ready 和实时通信仍待补；见 [core/http.md](core/http.md)、[core/i18n.md](core/i18n.md) |
| `openrust::db` | SeaORM 双库、连接池、事务、migration/seed、方言与表名解析；显式只读副本路由和 task-local 主库 scope 已实现，完整集群验收仍未完成；见 [core/db.md](core/db.md) |
| `openrust::audit` | 显式事件、详情白名单/脱敏、数据库与自定义 sink、事务内写入和有界清理；见 [core/audit.md](core/audit.md) |
| `openrust::auth` | JWT-first、可选 Session、refresh、PAT、验证码、AuthFlows、Policy/RBAC；现有运行时双库及生成 HTTP 验收有记录，流程扩展与其余补交项另行验收；见 [core/auth.md](core/auth.md) |
| `openrust::mail`、`openrust::sms` | log/fake、可选 SMTP/Twilio、显式 locale；Mailpit 本地实收有记录，Twilio 实发待授权；队列通知编排未完成；见 [core/mail.md](core/mail.md) |
| `openrust::security`、`openrust::signing` | 可选本地应用加密/密钥轮换、用途隔离签名链接及共享撤销；见 [core/security.md](core/security.md)、[core/signing.md](core/signing.md) |
| `openrust::queue`、`openrust::schedule` | 数据库队列/worker 与 UTC 调度已有实现；任务编排、事件、完整故障/集群验收的提交及 WIP 边界以 Git、[core/queue.md](core/queue.md)、[core/schedule.md](core/schedule.md) 为准，不能据此关闭 M4 |
| `cli` | new/dev/generate 与模板，见 [core/cli.md](core/cli.md) |
| `macros` | `#[openrust::resource]` proc-macro，见 [core/macros.md](core/macros.md) |
| `testing` | PostgreSQL/MySQL `TestDatabase`，见 [core/test.md](core/test.md) |
| `apps/server` | 聚合入口及启动/数据库/认证集成验证，见 [core/server.md](core/server.md) |
| cache/events/storage 等剩余能力 | 尚未完整实现；按 [当前计划](plan.md) 与 [能力覆盖](plans/v1-capability-coverage.md) 逐簇推进 |

## 扩展模型

应用模块与第三方包都通过 `Plugin` 接入：`build` 阶段同步注册服务、命令、路由、迁移、配置段等；所有插件构建完成并冻结后按注册序依次调用异步 `finish`。HTTP 路由复用 `RouteRegistry` 与 Tower middleware。worker/scheduler 由命令 supervisor 持有任务、取消与停机，不新增全局 Hooks 或 Plugin shutdown（[ADR-037](decisions/adr-037-queue-worker-lifecycle.md)）；这不取消事务、认证、上传等类型化业务扩展要求。不提供动态插件加载。

能力驱动遵循 [ADR-009](decisions/adr-009-extensible-capability-drivers.md)：提供可靠默认实现与可由第三方接入的公开契约，集中后端差异并明确能力边界。数据库扩展沿用 SeaORM `DatabaseConnection`、`Db` 和 Plugin 生命周期；v1 官方支持 PostgreSQL/MySQL，不把其他 backend 的连接成功视为完整框架支持。集群部署跨里程碑推进，memory、本地 file、单节点磁盘不能充当共享权威状态，详见 [集群计划](plans/v1-cluster-deployment.md)。
