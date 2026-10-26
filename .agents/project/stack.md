# 技术栈

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：当前依赖与 v1 规划选型分列。已声明版本以各 `Cargo.toml` 为准，实际解析版本以 `Cargo.lock` 为准；下表标为“待实现 / 待引入”的库、feature 和命令均不是当前可用入口。

## 运行时与 HTTP

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| 异步运行时 | `tokio` 1.x | 唯一运行时 |
| HTTP | `axum` 0.8 + `tower` + `tower-http` | 路由、提取器、中间件；`tower-http` 启用 `cors`、`compression-gzip`、`compression-br` 以支持配置化跨域与 gzip/br 协商，另有 request-id、trace、timeout、catch-panic；门面直接依赖并导出 `tower::ServiceExt` 供单依赖应用测试路由 |
| 限流 | `openrust::core::RateLimiter` + `openrust::database::DatabaseRateLimitStore` | 已实现固定窗口服务与双库共享 store，AuthFlows 已调用；通用 HTTP middleware、请求 key/策略组合仍待实现，不额外依赖 governor |
| 会话 | `tower-sessions` 0.15 | 复用 Axum/Tower 通用 Session 与 store contract；OpenRust 以 SeaORM 实现双库共享 store，Redis adapter 暂留后续 |
| OpenAPI | 规划 `utoipa` + `utoipa-axum`，尚未引入 | 类型 schema、统一响应文档与非生产 `/docs` 均待实现 |
| 出站 HTTP | `reqwest` 0.13（Rustls） | `http-client` 可选 feature；共享连接池、JSON/query/form/multipart/stream、显式超时；禁止默认重定向和应用层重试，响应体由框架限额 |
| 序列化 | `serde` + `serde_json` | `apps/server` 集成测试用 `serde_json` 比对包装字段与请求 ID |
| 验证 | `validator` | M0 评估 `garde` 后定稿 |
| 国际化 | `openrust::i18n` + `serde_json` | 内置中英文框架消息；应用以 JSON resource 扩展；不引入表达式/复数 DSL 依赖 |
| 配置 | `figment` | 已采用；错误信息含键名（测试覆盖） |
| 日志 / 追踪 | 已用 `tracing` + `tracing-subscriber` | `opentelemetry` / `metrics` 接入为 M5 规划，当前无对应依赖或 feature |
| 时间 | `chrono`（UTC） | SeaORM 兼容，生态最广 |
| ID | 规划 UUID v7，尚无框架直接 `uuid` 依赖 | 当前模型生成器使用整数 ID，`--uuid` 选项未实现；传递依赖不代表该生成能力已交付 |

## 数据

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| ORM / 迁移 | `sea-orm` 2.0.4 + `sea-orm-migration` 2.0.4 | 采用 M0 双库迁移 spike 验证的版本；tokio Rustls 默认运行时，数据库连接驱动按 `db-postgres` / `db-mysql` feature 开启 |
| 数据库 URL | `url` 2.5 | `openrust-test` 替换隔离数据库名时保留连接凭据和查询参数 |
| SQLx 支持 | `sqlx` 0.9 | MySQL driver 强制 TLS（Rustls）；不启用存在安全公告的 RSA 非 TLS 认证路径；`db.sqlx()` 尚未实现，若提供需先定义公开 API、feature 与连接生命周期 |
| 数据库 | 暂定最低支持：PostgreSQL 14+、MySQL 8.0+；日常开发/Compose：PostgreSQL 18、MySQL 8.4 LTS；常规 CI：PostgreSQL 18、MySQL 8.4 | 运行时版本检查按暂定最低版本执行；PostgreSQL/MySQL 最低版本的完整兼容性仍须在 v1 发布验收中复核，不把单项历史测试等同于整套框架支持。仅在主版本跨度大或兼容风险显著时专项测试，不为每次日常 CI 追测所有版本。PostgreSQL 官方按主版本提供五年支持，不称 LTS |
| Redis | 暂定最低支持：Redis 6.2+；日常开发/Compose：Redis 8；能力落地后按兼容风险增加专项测试 | Redis driver 和客户端依赖仍待 M5 引入；最低版本须经标准缓存路径与集群共享语义的真实兼容验收，发布前复核版本生命周期 |
| 密码哈希 | `argon2` 0.6（Argon2id / PHC） | RustCrypto 实现；随机 salt；参数有界可配置；哈希与验证移入 Tokio blocking pool |
| 认证与限流 key 摘要 | `sha2` 0.10 + `subtle` 2.6 | Session、PAT、CSRF 与数据库限流 key 存 SHA-256 摘要；CSRF 摘要比较使用常量时间比较。限流摘要仅避免原文持久化，不抵抗低熵 key 猜测 |
| JWT | `jsonwebtoken` 11.1（`aws_lc_rs`） | 复用成熟 JWT 编解码与验证；避免 `rust_crypto` 带入被安全公告覆盖的 `rsa 0.9`；OpenRust 固定 HS256 并自行验证配置、kid、claims、密钥轮换与刷新凭据语义；MSRV 声明为 1.88，须由 workspace 1.97 验证 |
| 应用数据加密 | `aes-gcm-siv` 0.12.1（AES-256-GCM-SIV / RFC 8452）、`getrandom` 0.3、`base64` 0.22、`zeroize` 1.9.1 | `encryption` feature 固定 32-byte key、随机 nonce、认证 envelope 与 key ID 轮换；显式启用 AEAD crate 的 `zeroize` feature；不复用 JWT 签名密钥 |
| 签名链接 | 既有 `jsonwebtoken` 11.1、`base64` 0.22、`getrandom` 0.3、`sha2` 0.10 | 可选 `signing` feature 固定 HS256 和用途专属 key ring；逐条撤销用 PostgreSQL/MySQL 共享存储；未新增依赖 |
| JWT key/token randomness | `base64` 0.22 + `getrandom` 0.3 | Base64 解码配置密钥；操作系统 CSPRNG 生成 jti 与 refresh credential |

## 异步能力与 driver

| 能力 | 当前实现 | 待实现边界 |
| --- | --- | --- |
| 队列 | `queue` feature 下的自研数据库 driver（[ADR-003](decisions/adr-003-database-queue-default.md)，不依赖 `apalis`） | 当前无 `queue-redis` feature 或 Redis queue driver；M4 剩余范围见 [队列事实](core/queue.md) |
| 调度 | 同属 `queue` feature；`Scheduler` + `ScheduleFrequency::{Interval, DailyAtUtc}`，仅 UTC | 无 `cron` 依赖、cron 表达式或 IANA 时区/DST 支持；cron/IANA/DST 不纳入首版调度 contract；生产资源/集群验收仍待补齐 |
| 缓存 | 尚未实现；contract 要求标准应用默认 Redis、memory/file 显式选择（ADR-015、[ADR-014](decisions/adr-014-cache-driver-and-store-contract.md)） | M5 引入统一 Cache、命名 stores 与 drivers；`cache-redis` 是规划 feature，当前 manifest 不含它 |
| 存储 | 尚未实现；规划以 `opendal` 提供本地和 S3 兼容路径 | `opendal` 依赖与 `storage-s3` feature 均待引入 |
| 邮件 / 短信 | `lettre` 0.11 + log/fake；`mail-smtp` 启用 Tokio/Rustls STARTTLS，仅本机 loopback 可显式关闭；`sms-twilio` 复用 `http-client` 与 `base64` | 本机 Mailpit SMTP 实收已有验收记录；真实 Twilio 投递待授权账号和目标，log/fake 不能代替送达证据 |
| 模板 | `LocalizedEmail` 与 `I18n` 消息参数插值；SMS 可传供应商 template SID | `minijinja` 尚未引入，也没有 `templates` feature；邮件按 ADR-033 复用 catalog，不在 v1 新增第二套引擎；不能将参数插值视为完整页面模板系统 |

## 工具链

| 用途 | 当前状态 |
| --- | --- |
| CLI 解析 | 已用 `clap`（derive） |
| 模板渲染（生成器） | 当前为内置模板文件与 Rust 字符串生成；未使用 `minijinja` |
| 文件监听（`openrust dev`） | 尚未实现；当前仅以 development 环境启动应用，`watchexec` 是规划选型 |
| 快照测试 | 已用 `insta` |
| 流式请求测试 | 已用 `tokio-stream` 0.1（dev dependency）构造出站 HTTP 一次性 stream；不进入默认运行时依赖 |
| 数据库隔离测试 | 已用 `openrust-test::TestDatabase`，提供 PostgreSQL 模板克隆与 MySQL 独立库迁移 |
| 过程宏 | 已用 `syn` + `quote` 解析控制器 impl 项并生成 `Resource` 实现 |
| 假数据 | `fake` 尚未引入；已有 Mail/SMS/Audit fake 是各能力自身测试实现，不依赖该 crate |
| 依赖审计 | 当前验证使用 `cargo-deny`；`cargo-audit` 为规划工具，不等于已执行 |
| 语义化版本检查 | `cargo-semver-checks` 属于 M6 发布门禁规划 |
| 变更日志 | `git-cliff` 为规划工具 |
| 文档站 | M6 规划 `mdbook`；当前尚无 mdBook 用户文档站 |
| CI | GitHub Actions；PostgreSQL 与 MySQL 以 service 容器提供 |

## 版本政策

v2 端侧规划：Admin Web 使用 pure-admin 精简国际化版 + Element Plus（Vue 3 / TypeScript / Vite 路线），App 使用 uni-app；PC 端仅设计，暂不引入运行时。B1 锁定具体兼容版本、App 实现代际/渲染模式与目标平台，记录实际依赖和锁文件；目前这些前端依赖尚未安装，不属于 v1 Rust 核心依赖。范围、选型依据及验收见 [v2 计划](plans/v2-admin-platform.md)。

Edition 2024；MSRV 为 stable 减两个小版本；0.x 期间破坏性变更只在次版本号；依赖升级随 Dependabot，升级主版本写 ADR。
