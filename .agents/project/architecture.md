# 架构

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 的 crate 分层与依赖方向。详细设计见 [框架设计](plans/framework-design.md)，代码落地后各 crate 事实写入 `core/<crate>.md`。

## 分层

```text
apps/server ── openrust（仓库聚合入口、集成验证宿主）
openrust-cli ───────────────┐           （全局工具：new / dev / generate <kind>，代理运行命令）
                            ▼
openrust（门面：prelude、feature、re-export）
   ├── openrust-http ─── openrust-core
   ├── openrust-db ──── openrust-core
   ├── openrust-auth ── openrust-http + openrust-db + openrust-mail
   ├── openrust-queue ─ openrust-db（database driver）
   ├── openrust-events  openrust-core（+ openrust-queue 可选）
   ├── openrust-cache / openrust-storage / openrust-mail ── openrust-core
   └── openrust-macros（被 queue / events / http 使用）
openrust-test ── 依赖门面，提供 TestApp 与 fake driver 断言
```

## 依赖规则

- 所有能力 crate 只依赖 `openrust-core` 与所需的同层 crate，不反向依赖门面 `openrust`。
- `openrust-core` 只依赖 tokio / serde / tracing / figment 与 `axum-core`（仅为让 `Error` 实现 `IntoResponse`，避免用户面对两种 `Result`），不依赖 axum 路由层或任何数据库驱动。
- 数据库驱动、Redis、S3、SMTP、OpenTelemetry 一律通过 feature 开启；门面 crate 默认不含数据库驱动。
- 单库专属 SQL 只能出现在 `openrust-db::dialect`，并同时提供 PostgreSQL 与 MySQL 分支。
- 生成的应用只依赖门面 `openrust`（及 `openrust-test` 作为 dev-dependency），不直接依赖子 crate。
- `apps/server` 同样只依赖门面，随已实现能力扩展并补集成测试；它不发布。

## 当前状态

| crate | 状态 |
| --- | --- |
| `openrust-core` | M0 已实现：`Application` / `AppBuilder` / `App` / `AppState` / `Plugin` / `Command` / `Error` / 配置分层 / 生命周期，见 [core/core.md](core/core.md) |
| `openrust-http` | M0 已实现：`HttpPlugin` / `Routes` / `Validated` / `Inject` / 中间件栈 / `/health` / `serve`，见 [core/http.md](core/http.md) |
| `openrust` | 门面与 `run`，见 [core/facade.md](core/facade.md) |
| `openrust-cli` | 生成项目、启动开发服务并代理运行命令，见 [core/cli.md](core/cli.md) |
| `apps/server` | M1 聚合入口：HTTP、默认中间件、`/health` 与根路由，见 [core/server.md](core/server.md) |
| 其余 crate | 空壳，README 列出规划 surface |

## 扩展模型

应用模块与第三方包都通过实现 `Plugin` 接入：`build` 阶段向 `AppBuilder` 插入服务并追加路由、迁移、配置段、命令与调度；所有插件构建完成并冻结后依次调用 `finish`。不提供动态插件加载。
