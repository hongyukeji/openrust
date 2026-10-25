# 架构

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 的 crate 分层与依赖方向。详细设计见 [框架设计](plans/framework-design.md)，代码落地后各 crate 事实写入 `core/<crate>.md`。

## 分层

```text
openrust-cli ───────────────┐           （全局工具：new / dev / make:*，代理运行命令）
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
- `openrust-core` 只依赖 tokio / serde / tracing / 配置与错误基础库，不依赖 axum 或任何数据库驱动。
- 数据库驱动、Redis、S3、SMTP、OpenTelemetry 一律通过 feature 开启；门面 crate 默认不含数据库驱动。
- 单库专属 SQL 只能出现在 `openrust-db::dialect`，并同时提供 PostgreSQL 与 MySQL 分支。
- 生成的应用只依赖门面 `openrust`（及 `openrust-test` 作为 dev-dependency），不直接依赖子 crate。

## 扩展模型

业务模块与第三方包都通过实现 `ServiceProvider` 接入：注册服务到 `AppState`、追加路由、迁移、配置段、命令与调度。不提供动态插件加载。
