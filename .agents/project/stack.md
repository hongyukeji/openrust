# 技术栈

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：第一代选型。已用依赖的版本以根 `Cargo.toml` 的 `[workspace.dependencies]` 为准（2026-10-25 锁定：tokio 1.54、axum 0.8.9、axum-core 0.5、tower 0.5、tower-http 0.7、figment 0.10、validator 0.21、clap 4.6、thiserror 2、serde 1、tracing 0.1）；未引入的只列主版本线与选型理由。

## 运行时与 HTTP

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| 异步运行时 | `tokio` 1.x | 唯一运行时 |
| HTTP | `axum` 0.8 + `tower` + `tower-http` | 路由、提取器、中间件；`tower-http` 提供 request-id、trace、cors、compression、timeout、catch-panic |
| 限流 | `tower_governor` | 登录等端点限流 |
| 会话 | `tower-sessions` | 数据库 store 默认，Redis store 可选 |
| OpenAPI | `utoipa` + `utoipa-axum` | 从类型生成文档，`/docs` 非生产默认开启 |
| 序列化 | `serde` + `serde_json` | — |
| 验证 | `validator` | M0 评估 `garde` 后定稿 |
| 配置 | `figment` | 已采用；错误信息含键名（测试覆盖） |
| 日志 / 追踪 | `tracing` + `tracing-subscriber`；可选 `opentelemetry` / `metrics` | — |
| 时间 | `chrono`（UTC） | SeaORM 兼容，生态最广 |
| ID | `uuid`（v7） | `--uuid` 生成器选项 |

## 数据

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| ORM / 迁移 | `sea-orm` 2.x + `sea-orm-migration` | 2026-10 已有 2.0，基于 `sqlx` 0.9 |
| 直连逃生口 | `sqlx` 0.9 | 仅通过 `db.sqlx()` 显式使用 |
| 数据库 | PostgreSQL 14+、MySQL 8.0+ | 均支持 `SKIP LOCKED`、CTE、JSON |
| 密码哈希 | `argon2`（Argon2id） | 参数可配置 |

## 异步能力与 driver

| 能力 | 默认 | 可选 feature |
| --- | --- | --- |
| 队列 | 自研数据库队列（[ADR-003](decisions/adr-003-database-queue-default.md)，不依赖 `apalis`） | `queue-redis`（`redis` crate） |
| 调度 | 自研 `Schedule` + `cron` 解析 | — |
| 缓存 | `moka`（进程内） | `cache-redis` |
| 存储 | `opendal`（fs） | `storage-s3` |
| 邮件 | `lettre`（SMTP）+ log + fake | — |
| 模板 | `minijinja`（仅邮件） | `templates` |

## 工具链

| 用途 | 工具 |
| --- | --- |
| CLI 解析 | `clap`（derive） |
| 模板渲染（生成器） | `minijinja` |
| 文件监听（`openrust dev`） | `watchexec` 库 |
| 快照测试 | `insta` |
| 假数据 | `fake` |
| 依赖审计 | `cargo-deny`、`cargo-audit` |
| 语义化版本检查 | `cargo-semver-checks` |
| 变更日志 | `git-cliff` |
| 文档站 | `mdbook` |
| CI | GitHub Actions；PostgreSQL 与 MySQL 以 service 容器提供 |

## 版本政策

Edition 2024；MSRV 为 stable 减两个小版本；0.x 期间破坏性变更只在次版本号；依赖升级随 Dependabot，升级主版本写 ADR。
