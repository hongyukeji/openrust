# 技术栈

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：v1 版本选型。已用依赖的版本以根 `Cargo.toml` 的 `[workspace.dependencies]` 为准（2026-10-25 锁定：tokio 1.54、axum 0.8.9、axum-core 0.5、tower 0.5、tower-http 0.7、figment 0.10、validator 0.21、clap 4.6、thiserror 2、serde 1、tracing 0.1）；未引入的只列主版本线与选型理由。

## 运行时与 HTTP

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| 异步运行时 | `tokio` 1.x | 唯一运行时 |
| HTTP | `axum` 0.8 + `tower` + `tower-http` | 路由、提取器、中间件；`tower-http` 启用 `cors`、`compression-gzip`、`compression-br` 以支持配置化跨域与 gzip/br 协商，另有 request-id、trace、timeout、catch-panic；门面直接依赖并导出 `tower::ServiceExt` 供单依赖应用测试路由 |
| 限流 | `openrust::core::RateLimiter` + `openrust::database::DatabaseRateLimitStore` | 不额外依赖 governor；固定窗口 service/store contract 与 PostgreSQL/MySQL 跨节点共享实现，具体 HTTP middleware 按后续 M3 策略集成 |
| 会话 | `tower-sessions` 0.15 | 复用 Axum/Tower 通用 Session 与 store contract；OpenRust 以 SeaORM 实现双库共享 store，Redis adapter 暂留后续 |
| OpenAPI | `utoipa` + `utoipa-axum` | 从类型生成文档，`/docs` 非生产默认开启 |
| 序列化 | `serde` + `serde_json` | `apps/server` 集成测试用 `serde_json` 比对包装字段与请求 ID |
| 验证 | `validator` | M0 评估 `garde` 后定稿 |
| 配置 | `figment` | 已采用；错误信息含键名（测试覆盖） |
| 日志 / 追踪 | `tracing` + `tracing-subscriber`；可选 `opentelemetry` / `metrics` | — |
| 时间 | `chrono`（UTC） | SeaORM 兼容，生态最广 |
| ID | `uuid`（v7） | `--uuid` 生成器选项 |

## 数据

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| ORM / 迁移 | `sea-orm` 2.0.4 + `sea-orm-migration` 2.0.4 | 采用 M0 双库迁移 spike 验证的版本；tokio Rustls 默认运行时，数据库连接驱动按 `db-postgres` / `db-mysql` feature 开启 |
| 数据库 URL | `url` 2.5 | `openrust-test` 替换隔离数据库名时保留连接凭据和查询参数 |
| SQLx 支持 | `sqlx` 0.9 | MySQL driver 强制 TLS（Rustls）；不启用存在安全公告的 RSA 非 TLS 认证路径；`db.sqlx()` 尚未实现，若提供需先定义公开 API、feature 与连接生命周期 |
| 数据库 | 日常开发/Compose：PostgreSQL 18、MySQL 8.4、Redis 8；常规 CI 数据库：PostgreSQL 18、MySQL 8.4；额外版本仅在主版本跨度或兼容风险显著时专项测试 | PostgreSQL 官方按主版本提供五年支持，不称 LTS；运行时当前接受 PostgreSQL 16+，这不等于最终发布支持承诺。PostgreSQL 16 为历史验证版本，不再是日常基线。v1 发布前按届时生命周期确定并验证最低支持版本；不为每次日常 CI 追测所有版本 |
| Redis | 日常开发目标：Redis 8；能力落地后按兼容风险决定是否专项覆盖最新版本 | Redis 官方版本类别称 Extended，不称 LTS；发布前核实届时版本生命周期，并验证标准缓存路径与共享语义 |
| 密码哈希 | `argon2` 0.6（Argon2id / PHC） | RustCrypto 实现；随机 salt；参数有界可配置；哈希与验证移入 Tokio blocking pool |
| 认证与限流 key 摘要 | `sha2` 0.10 + `subtle` 2.6 | Session、PAT、CSRF 与数据库限流 key 存 SHA-256 摘要；CSRF 摘要比较使用常量时间比较。限流摘要仅避免原文持久化，不抵抗低熵 key 猜测 |
| JWT | `jsonwebtoken` 11.1（`aws_lc_rs`） | 复用成熟 JWT 编解码与验证；避免 `rust_crypto` 带入被安全公告覆盖的 `rsa 0.9`；OpenRust 固定 HS256 并自行验证配置、kid、claims、密钥轮换与刷新凭据语义；MSRV 声明为 1.88，须由 workspace 1.97 验证 |
| JWT key/token randomness | `base64` 0.22 + `getrandom` 0.3 | Base64 解码配置密钥；操作系统 CSPRNG 生成 jti 与 refresh credential |

## 异步能力与 driver

| 能力 | 默认 | 可选 feature |
| --- | --- | --- |
| 队列 | 自研数据库队列（[ADR-003](decisions/adr-003-database-queue-default.md)，不依赖 `apalis`） | `queue-redis`（`redis` crate） |
| 调度 | 自研 `Schedule` + `cron` 解析 | — |
| 缓存 | 标准应用默认 Redis（ADR-015）；memory/file 显式选择，contract 见 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md) | 库层 `cache-redis` 可选，标准应用显式启用；各 driver 在 M5 实现 |
| 存储 | `opendal`（fs） | `storage-s3` |
| 邮件 | `lettre` 0.11（SMTP）+ log + fake | `mail-smtp` 用 Tokio + Rustls 强制 STARTTLS；log 为默认 driver；新增的 `quoted_printable` 传递依赖为 0BSD，已加入许可证允许清单 |
| 模板 | `minijinja`（仅邮件） | `templates` |

## 工具链

| 用途 | 工具 |
| --- | --- |
| CLI 解析 | `clap`（derive） |
| 模板渲染（生成器） | `minijinja` |
| 文件监听（`openrust dev`） | `watchexec` 库 |
| 快照测试 | `insta` |
| 数据库隔离测试 | `openrust-test::TestDatabase` | PostgreSQL 模板克隆、MySQL 独立库迁移 |
| 过程宏 | `syn` + `quote` | 解析控制器 impl 项并生成 `Resource` 实现；避免手工拼装 token 与语法错误 |
| 假数据 | `fake` |
| 依赖审计 | `cargo-deny`、`cargo-audit` |
| 语义化版本检查 | `cargo-semver-checks` |
| 变更日志 | `git-cliff` |
| 文档站 | `mdbook` |
| CI | GitHub Actions；PostgreSQL 与 MySQL 以 service 容器提供 |

## 版本政策

Edition 2024；MSRV 为 stable 减两个小版本；0.x 期间破坏性变更只在次版本号；依赖升级随 Dependabot，升级主版本写 ADR。
