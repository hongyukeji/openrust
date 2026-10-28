# 架构

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：同一 workspace 内的框架 library 与应用；目录决定见 [ADR-072](decisions/adr-072-framework-and-application-projects.md)。公共语义后继阶段逐项重构。

## Workspace 与职责

工程分工见 [ADR-072](decisions/adr-072-framework-and-application-projects.md)：本仓库对应 laravel/framework，根 openrust package/lib 的领域源码位于 src/；工具位于 tools/，框架公开测试与独立 consumers 位于 tests/。工具和应用单向依赖框架，根 package 无业务 bin，根 workspace 的 default-members=["examples/demo"] 默认启动应用。

examples/demo 是根 workspace 的聚合回归应用成员，共用根 Cargo.lock，保留原根 server、app/bootstrap/routes/config/database/resources/public/storage 与进程/数据测试。CI workspace 门禁包含 demo，并显式执行其真实服务 targets。examples/example 保留完整能力演示及其迁移 package，职责不等于第二份应用模板。

应用骨架唯一来源是 tools/cli/templates 和生成器；生成应用使用自身名称、独立 workspace/lock、本地明确解析的框架/tool 依赖，不继承开发 workspace，也不修改源工程。应用用静态模块声明连接职责目录，保留 Rust 类型/async/Cargo。

## 验证边界

框架 workspace、workspace 成员 demo、生成应用 check/test/HTTP、consumer、双库分别验收。根 workspace 默认启动 demo，根与 demo 目录无参数 cargo run 及真实 HTTP 须执行。目录迁移中，不将旧 ADR-071 验收记作新布局通过。

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
| workspace 成员 demo 的 `server` bin | 聚合入口及启动/数据库/认证集成验证，见 [core/server.md](core/server.md) |
| cache/events/storage 等剩余能力 | 尚未完整实现；按 [当前计划](plan.md) 与 [能力覆盖](plans/v1-capability-coverage.md) 逐簇推进 |
