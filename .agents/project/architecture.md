# 架构

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：应用职责目录与独立 framework library；目录决定见 [ADR-071](decisions/adr-071-application-framework-layout.md)。公共语义后继阶段逐项重构。

## Workspace 与职责

根 `application` package 只有 `src/main.rs` 的 `server` bin，default-members 仅根应用；默认 `cargo run` 无需选择 package/bin。`bootstrap/app.rs` 装配应用，`routes/web.rs` 注册路由，`app/` 放业务，config/database/resources/public/storage/tests 分别承担配置、数据代码、资源、公开文件、运行数据与集成测试。顶层职责目录通过编译期 `#[path]` 接入，Cargo 静态发现，无运行时源码扫描。

`framework/` 是唯一 `openrust` library package，通用实现位于 framework/src，原公开路径 `openrust::...` 在 R1 位置迁移期间保持；Container/Provider 等语义按后继 contract 重写。tools/cli、tools/macros、tools/testing、tools/performance 保持独立编译边界，不为每个领域拆空 crate。工具与应用单向依赖 framework。

综合示例位于 examples/example，迁移 package 位于其 database/migrations。公开扩展 consumer 保留 tests/consumers；根 tests 是默认应用的公开 API/真实服务集成回归，framework/src 单元测试随库发现。数据方言与协议驱动仍按既有边界，R1 不改变事务、认证和队列语义。

生成应用拥有独立 workspace 和 Cargo.lock；从框架源码执行 new 指定目标目录，生成 manifest 复制必要 workspace 配置并将本地 framework/tool path 明确解析，不修改源 workspace，也不依赖尚未发布的 crate。源目录需保持可达。

## 验证边界

根 CLI 启动、应用/框架测试、生成应用 check/test/HTTP、双库与 IDE 各有独立证据；具体见 testing/verification。IDE Command=run 的 UI 调试在实际执行前保留缺口，Cargo 编译不能代替。

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
