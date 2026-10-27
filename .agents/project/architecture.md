# 架构

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：根 `openrust` package 同时承载框架 modules 与 `server` 集成 bin；四个独立工具 package 集中位于 `tools/`。详细设计见 [框架设计](plans/framework-design.md)。

## 已确认的交付方式

整套源码 workspace 交付，业务放在 `apps/<name>/`。根 `openrust` package 提供 library 与 `server` bin；其余内部 package 为 `tools/cli`、`tools/macros`、`tools/testing`、`tools/performance`。全部 package 禁用发布；源码交付约定见 [ADR-039](decisions/adr-039-source-workspace-distribution.md)，目录收纳见 [ADR-021](decisions/adr-021-loco-style-module-layout.md)。

## Workspace 与源码布局

```text
openrust (根 package + workspace)
├── src/main.rs     # 集成宿主 bin：server，根 cargo run 默认启动
├── src/routes.rs   # 框架路由注册、装配与应用扩展入口
├── src/config/      # Rust 配置加载、类型与校验
├── src/audit.rs     # 显式审计与可替换 sink
├── src/core/        # Application、AppBuilder、Plugin、Command、Error
├── src/http/        # Axum routes、extractors、Tower middleware、serve
├── src/db/          # SeaORM 连接、迁移、seed、方言及 table resolver
├── src/auth/        # JWT、Session、refresh、PAT、认证流程与 Policy
├── src/mail/        # Email/Mailer 与可选 SMTP
├── src/view.rs      # 可选服务端 HTML 视图（view feature）
├── src/data/exchange/ # 数据导入/导出与任务化工作流
├── src/queue.rs     # 数据库任务与 worker（queue feature）
├── src/schedule.rs  # UTC 调度（queue feature）
├── tests/           # 框架公共 API、server 集成测试及独立 consumer 验收
├── config/          # 根宿主运行配置
└── apps/example/    # 唯一面向开发者的贯穿式示例应用
tools/                   # 独立的仓库维护/开发 package
├── cli/                 # `openrust` 命令行与模板（package: cli）
├── macros/              # proc-macro package（macros）
├── testing/             # 用户 dev-dependency 测试工具（testing）
└── performance/         # 独立性能验证 package，含 benches/
apps/                    # 根宿主模块、综合示例应用及独立生成应用
├── mod.rs               # Server Application 与模块/插件组合
├── <module>.rs          # 根 server 编译的业务模块（显式声明）
└── example/             # 唯一综合示例，采用正常生成应用结构
tests/consumers/          # 独立 consumer 与真实拓扑验收夹具
```

框架源码 Rust 模块与运行时配置文件分开：源码位于根 `src/`，包括 `src/config/` 与集中路由装配的 `src/routes.rs`；每个应用（含生成项目）在自身项目根 `config/` 放 TOML/YAML。HTTP 注册扩展复用 `RouteRegistry`、`AppBuilderHttpExt::routes` 和 Tower layers；应用在 `Application::build` 或插件 `build` 阶段按顺序声明路由，不另建 Hooks。仓库 `apps/mod.rs` 是根宿主的静态模块注册表，业务模块需显式声明并通过 `Application` / `AppBuilder` 接入；不扫描文件系统或运行时加载 Rust 源码。CLI 通过 Cargo metadata 识别源码 workspace，生成的 `apps/<name>/` 是独立 package，并把应用及可选 migration 登记为 workspace member。应用使用本地 workspace/path 依赖；`dev`、`serve` 与代理命令按目标 app 的 manifest 显式选包，并从应用工作目录加载配置。外部独立目录不是生成目标，详细边界见 ADR-039 与 [ADR-063](decisions/adr-063-explicit-app-module-registry.md)。数据交换模块按 `src/data/exchange/` 分层；`openrust::data_exchange` 保留兼容导出。

当前 workspace 有七个 package：根 `openrust`（框架与聚合启动宿主），`tools/cli`（package `cli`，命令分发），`tools/macros`（package `macros`，proc-macro 技术边界），`tools/testing`（package `testing`，用户测试 dev-dependency），`tools/performance`（package `performance`，框架性能基准），以及 `apps/example` 与其 `migration` package（贯穿式示例应用及迁移）。原能力空壳已移除；cache/events/storage 等剩余能力直接在根 package 实现，不预建空模块。

## 目录职责与验证入口

按 [ADR-021](decisions/adr-021-loco-style-module-layout.md)，CLI、proc-macro、测试工具和性能验证的独立编译/运行边界保留为独立 package，并集中放在 `tools/` 下；包名仍按 ADR-039 使用职责名。根框架保留唯一 `openrust` package。

面向开发者的综合应用位于 `apps/example/`，按 HTTP、数据库、认证、缓存、队列、存储和通信等能力随 v1 实际进展逐步完善；不再维护多个平行入门工程。独立下游 consumer 与真实多进程/容器验收位于 `tests/consumers/<capability>/`，不得误作通用测试 API。根 `tests/` 按能力组织，通过明确 `[[test]]` targets 保持测试名称、feature 门控和 Cargo 发现。目录与示例决策见 [ADR-064](decisions/adr-064-canonical-example-application.md)。

根 package 内的 `server` bin 是框架开发/集成宿主；`apps/mod.rs` 是其应用模块组合入口，`src/main.rs` 只负责启动。框架路由注册、装配默认中间件及对应用开放的路由/layer 扩展入口位于 `src/routes.rs`；`src/http/routes.rs` 保留 Axum 路由构建器。`tests/server/` 与根 `config/` 保留真实进程、双库、认证、队列和调度验收；独立生成应用继续位于 `apps/<name>/`。

## 依赖规则

- 所有通用框架能力属于同一个 `openrust` package，内部引用沿 `core/config` 等 module boundary；公共稳定入口保持 `openrust::...`。
- 根 package 可选数据库 driver、SMTP 等依赖由 Cargo features 控制；默认构建无需数据库驱动或外部服务。
- PostgreSQL 与 MySQL 的 schema/SQL 差异集中在 `db::dialect` 及明确的数据库适配入口，需为两库提供分支和真实测试。
- 生成应用只依赖 `openrust`；`testing` 单向依赖 `openrust`，根框架不得反向依赖测试工具。
- 根 `apps/mod.rs` 只通过公开 facade 组装默认宿主，并显式声明/注册业务模块；`src/main.rs` 不承担模块装配。根 `tests/server/` 覆盖 feature 组合、真实进程和双库验收。
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
| 根 `server` bin | 聚合入口及启动/数据库/认证集成验证，见 [core/server.md](core/server.md) |
| cache/events/storage 等剩余能力 | 尚未完整实现；按 [当前计划](plan.md) 与 [能力覆盖](plans/v1-capability-coverage.md) 逐簇推进 |

## 后续领域插件边界

[AI模型平台](plans/ai-model-platform.md) 是后续可选领域插件案例，目前仅规划，须用户明确要求开发后才启动。通用插件边界由 [ADR-009](decisions/adr-009-extensible-capability-drivers.md#可选领域插件) 维护，项目细节仅在该计划维护；现有装配入口不代表领域能力已实现。

## 扩展模型

应用模块与第三方包都通过 `Plugin` 接入：`build` 阶段同步注册服务、命令、路由、迁移、配置段等；所有插件构建完成并冻结后按注册序依次调用异步 `finish`。HTTP 路由复用 `RouteRegistry` 与 Tower middleware。worker/scheduler 由命令 supervisor 持有任务、取消与停机，不新增全局 Hooks 或 Plugin shutdown（[ADR-037](decisions/adr-037-queue-worker-lifecycle.md)）；这不取消事务、认证、上传等类型化业务扩展要求。不提供动态插件加载。

HTTP 业务 API 使用 `/api/*`，实时 WebSocket 使用 `/ws/*`，健康/就绪/metrics 保持独立运维路径。公开站点使用 `[server].public_dir`（默认 `public/`）并映射到 URL 根；`[server].root_target` 配置首页文件、内部转发、HTTP(S) 重定向或启用 `view` 后的服务端 HTML 模板，遵循 Figment 环境覆盖优先级，详见 [ADR-068](decisions/adr-068-api-namespace-and-static-site-root.md) 与 [ADR-069](decisions/adr-069-server-rendered-views.md)。

能力驱动遵循 [ADR-009](decisions/adr-009-extensible-capability-drivers.md)：提供可靠默认实现与可由第三方接入的公开契约，集中后端差异并明确能力边界。数据库扩展沿用 SeaORM `DatabaseConnection`、`Db` 和 Plugin 生命周期；v1 官方支持 PostgreSQL/MySQL，不把其他 backend 的连接成功视为完整框架支持。集群部署跨里程碑推进，memory、本地 file、单节点磁盘不能充当共享权威状态，详见 [集群计划](plans/v1-cluster-deployment.md)。
