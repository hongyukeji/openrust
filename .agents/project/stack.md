# 技术栈

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：当前依赖与 v1 规划选型分列。已声明版本以各 `Cargo.toml` 为准，实际解析版本以 `Cargo.lock` 为准；下表标为“待实现 / 待引入”的库、feature 和命令均不是当前可用入口。

## 运行时与 HTTP

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| 异步运行时 | `tokio` 1.x | 唯一运行时；启用 `io-util` 支持 HTTP 验收与异步 I/O、`fs` 支持本地缓存异步文件操作 |
| HTTP | `axum` 0.8 + `tower` + `tower-http` | 路由、提取器、中间件；`tower-http` 启用 `cors`、`compression-gzip`、`compression-br` 以支持配置化跨域与 gzip/br 协商，另有 request-id、trace、timeout、catch-panic；门面直接依赖并导出 `tower::ServiceExt` 供单依赖应用测试路由 |
| 限流 | `openrust::core::RateLimiter` + `openrust::database::DatabaseRateLimitStore` | 已实现固定窗口服务与双库共享 store，AuthFlows 与通用 HTTP RateLimitLayer/key 策略已接入；gRPC 适配待实现，不额外依赖 governor |
| 会话 | `tower-sessions` 0.15；可选 Redis adapter 复用 `redis` 1.8.0 | 默认使用 SeaORM 数据库 store；`session-redis` feature 提供显式 `RedisSessionStore`，通过 `SessionPlugin::with_store` 选择；Redis 键与 TTL 语义见 ADR-017 |
| gRPC | 规划 `tonic` / `prost`、`tonic-prost-build` 及官方 health/reflection 组件，尚未引入 | [ADR-040](decisions/adr-040-grpc-integration.md) 已纳入 v1/M5；实施时核实版本、MSRV、TLS provider 与代码生成工具。可选 `grpc` feature，默认构建不要求 `protoc`；不自研 RPC 栈 |
| OpenAPI | 规划 `utoipa` + `utoipa-axum`，尚未引入 | 类型 schema、统一响应文档与非生产 `/docs` 均待实现 |
| 出站 HTTP | `reqwest` 0.13（Rustls）、`ipnet` 2.11、`httpdate` 1.x | `http-client` 可选 feature；共享连接池、JSON/query/form/multipart/stream、显式超时；默认禁重定向和代理继承；`HttpClient::restricted` 可选执行目标主机/端口、DNS 全结果及公网地址策略，内网例外由主机 + CIDR + 端口显式登记；命名依赖隔离及可选进程内熔断复用此客户端 |
| 外部 OIDC 客户端 | `oauth2` 5.0（禁用 crate 默认 feature）+ 现有 `jsonwebtoken` 11.1（aws-lc-rs） | 可选 `auth-oidc` feature 依赖 `auth`、`http-client`、`encryption`；授权码/PKCE/令牌交换由 oauth2 提供，OIDC Discovery/JWKS 与 ID Token 必需声明校验由框架 glue 完成并通过受限命名 `DependencyClient` 出站；避免引入 `openidconnect` 的受影响 `rsa` 依赖，见 ADR-053 |
| 序列化 | `serde` + `serde_json` | `apps/server` 集成测试用 `serde_json` 比对包装字段与请求 ID |
| 验证 | `validator` | M0 评估 `garde` 后定稿 |
| 国际化 | `openrust::i18n`、`serde_json` 与 ICU4X `icu_datetime`/`icu_decimal`/`icu_locale`/`icu_plurals` 2.3.x | 同一 catalog 支持字符串插值、CLDR plural/select；编译数据用于 locale 数字与公历日期格式化。仅启用所需组件；数据随二进制构建，不使用运行时网络。日期时区转换由调用方显式完成；最终二进制尺寸增量纳入 M6 构建基线测量 |
| 配置 | `figment` | 已采用；错误信息含键名（测试覆盖） |
| 日志 / 追踪 | 已用 `tracing` + `tracing-subscriber` | `opentelemetry` / `metrics` 接入为 M5 规划，当前无对应依赖或 feature |
| 时间 | `chrono` + `chrono-tz` 0.10.4（IANA tzdb），队列 cron 使用 | SeaORM 兼容；`queue` feature 启用静态 IANA 时区表，构建/产物增加相应数据；集群节点须运行相同构建产物 |
| cron 解析 | `cron-parser` 0.12（`queue` feature） | 只接受五字段表达式；可复用解析结果计算日历触发，DST gap 跳过且 fold 的两个绝对时刻有序返回，框架以本地 wall-slot 唯一键折叠重复分钟 |
| ID | 已提供可选 UUIDv7，详见下表 `db-uuid` | 默认整数 ID；生成选项为 `--uuid-id`，不使用不存在的 `--uuid` |

## 数据

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| ORM / 迁移 | `sea-orm` 2.0.4 + `sea-orm-migration` 2.0.4 | 采用 M0 双库迁移 spike 验证的版本；tokio Rustls 默认运行时，OpenRust 官方连接驱动按 `db-postgres` / `db-mysql` feature 开启。外部自定义插件可通过同版本 SeaORM 依赖的 feature 统一选择其他 backend；SQLite 通用 `Db` 与应用自有迁移示例见 `examples/db-custom-sqlite/`，不代表 OpenRust 框架迁移、认证/队列或方言的完整支持 |
| 数据库 URL | `url` 2.5 | `testing` 替换隔离数据库名时保留连接凭据和查询参数 |
| SQLx 支持 | `sqlx` 0.9 | MySQL driver 强制 TLS（Rustls）；不启用存在安全公告的 RSA 非 TLS 认证路径；`db.sqlx()` 尚未实现，若提供需先定义公开 API、feature 与连接生命周期 |
| 数据库 | 暂定最低支持：PostgreSQL 14+、MySQL 8.0+；日常开发/Compose：PostgreSQL 18、MySQL 8.4 LTS；常规 CI：PostgreSQL 18、MySQL 8.4 | 运行时版本检查按暂定最低版本执行；PostgreSQL/MySQL 最低版本的完整兼容性仍须在 v1 发布验收中复核，不把单项历史测试等同于整套框架支持。仅在主版本跨度大或兼容风险显著时专项测试，不为每次日常 CI 追测所有版本。PostgreSQL 官方按主版本提供五年支持，不称 LTS |
| UUIDv7 主键 | `uuid` 1.x（`v7`, `serde`），通过 `db-uuid` feature 同步启用 SeaORM/SeaQuery UUID codec | 仅在模型生成显式使用 `--uuid-id` 时启用；默认仍为自增整数。UUIDv7 由生成实体的 SeaORM `before_save` 在应用进程生成 |
| Redis | 暂定最低支持：Redis 6.2+；日常开发/Compose：Redis 8；当前真实服务验收使用 Redis 8.10.2 | 缓存与可选 Session adapter 已通过 Redis 8.10.2；Redis 6.2 最低版本及最终集群兼容仍需发布前专项验收 |
| 密码哈希 | `argon2` 0.6（Argon2id / PHC） | RustCrypto 实现；随机 salt；参数有界可配置；哈希与验证移入 Tokio blocking pool |
| 认证与限流 key 摘要 | `sha2` 0.10 + `subtle` 2.6 | Session、PAT、CSRF 与数据库限流 key 存 SHA-256 摘要；CSRF 摘要比较使用常量时间比较。限流摘要仅避免原文持久化，不抵抗低熵 key 猜测 |
| JWT | `jsonwebtoken` 11.1（`aws_lc_rs`） | 复用成熟 JWT 编解码与验证；避免 `rust_crypto` 带入被安全公告覆盖的 `rsa 0.9`；OpenRust 固定 HS256 并自行验证配置、kid、claims、密钥轮换与刷新凭据语义；MSRV 声明为 1.88，须由 workspace 1.97 验证 |
| TOTP | `hmac` 0.13 + `sha1` 0.11 + `subtle` 2.6（均仅由 `auth-mfa` 启用） | 按 RFC 6238 实现 HMAC-SHA1 动态截断与常量时间码比较；锁文件中已有对应版本，新增直接依赖用于显式协议实现与 RFC 已知向量测试；seed 继续由现有 `Encryptor` 加密 |
| 应用数据加密 | `aes-gcm-siv` 0.12.1（AES-256-GCM-SIV / RFC 8452）、`getrandom` 0.3、`base64` 0.22、`zeroize` 1.9.1 | `encryption` feature 固定 32-byte key、随机 nonce、认证 envelope 与 key ID 轮换；显式启用 AEAD crate 的 `zeroize` feature；不复用 JWT 签名密钥 |
| 签名链接 | 既有 `jsonwebtoken` 11.1、`base64` 0.22、`getrandom` 0.3、`sha2` 0.10 | 可选 `signing` feature 固定 HS256 和用途专属 key ring；逐条撤销用 PostgreSQL/MySQL 共享存储；未新增依赖 |
| JWT、锁与幂等 owner randomness | `base64` 0.22 + 必需依赖 `getrandom` 0.3 | 操作系统 CSPRNG 生成 JWT jti、refresh credential、共享锁与请求幂等 owner token；共享锁令该依赖不再按认证/队列/加密 feature 可选 |

## 异步能力与 driver

| 能力 | 当前实现 | 待实现边界 |
| --- | --- | --- |
| 队列 | `queue` feature 下默认使用数据库 driver（[ADR-003](decisions/adr-003-database-queue-default.md)，不依赖 `apalis`）；可选 `queue-redis` 复用已有 `redis` 1.8.0 | Redis Queue 已通过 Redis 8.10.2 多连接、唯一派发、失败重试、租约恢复与 Redis-only server 集成验收；至少一次及事务/任务组限制见 ADR-046，Redis 持久性/复制与最低版本仍由部署和 v1 发布门禁负责 |
| 调度 | `queue` feature；`Scheduler` + interval/UTC daily/五字段 cron，`cron-parser` 0.12 与 `chrono-tz` 0.10.4 | F1 已实现；cron 使用 IANA zoneinfo 表，DST gap 跳过、重复 wall minute 去重；有界补跑及双库验收见 ADR-037 |
| 缓存 | 根 `openrust::cache` 提供 Cache、命名 stores、memory/file 与公开 custom driver contract；Redis 由 `redis` 1.8.0、`connection-manager` + `tokio-rustls-comp` feature 提供 | `cache-redis` 可选 feature；标准生成应用启用 Redis 默认 store；Redis 6.2 最低版兼容、F4 tag/SWR/singleflight 尚待验收 |
| 存储 | OpenDAL 0.59.x（当前锁定 0.59.5）提供 filesystem/S3-compatible I/O；`bytes` 1.12 与 `futures-util` 0.3 承载有界流 | 根模块 `openrust::storage` 提供配置、流检查和 driver contract；默认不启用，`storage-local` 启用文件系统，`storage-s3` 增加 S3 服务和 Rustls HTTP transport；OpenDAL facade 关闭默认 feature，仅开启相应服务。S3 签名依赖链使用 `tiny-keccak`（CC0-1.0），许可在 `deny.toml` 中显式允许 |
| 邮件 / 短信 | `lettre` 0.11 + log/fake；`mail-smtp` 启用 Tokio/Rustls STARTTLS，仅本机 loopback 可显式关闭；`sms-twilio` 复用 `http-client` 与 `base64` | 本机 Mailpit SMTP 实收已有验收记录；真实 Twilio 投递待授权账号和目标，log/fake 不能代替送达证据 |
| 模板 | 可选 `mail-templates` 使用 MiniJinja `~2.24.0`（关闭默认 feature，仅 `builtins`、`fuel`、`multi_template`、`serde`、`unicode`）；`LocalizedEmail` 与 `I18n` 仍负责普通消息和主题翻译 | 仅受信任编译期应用资源、严格变量与自动 HTML 转义；不提供文件/网络 loader；邮件模板不扩展为通用页面模板系统 |

## 工具链

| 用途 | 当前状态 |
| --- | --- |
| CLI 解析 | 已用 `clap`（derive） |
| 模板渲染（生成器） | CLI 继续使用内置模板文件与 Rust 字符串生成；应用邮件的可选模板渲染由根 `openrust` 的 `mail-templates` feature 提供 |
| 文件监听（`openrust dev`） | 尚未实现；当前仅以 development 环境启动应用，`watchexec` 是规划选型 |
| 快照测试 | 已用 `insta` |
| TOML 配置编辑 | CLI 使用 `toml_edit` 在整套源码 workspace 中合并应用成员，并保留根 `Cargo.toml` 的其余内容与注释；不引入额外 manifest 重写器 |
| 流式请求测试 | 已用 `tokio-stream` 0.1（dev dependency）构造出站 HTTP 一次性 stream；不进入默认运行时依赖 |
| 数据库隔离测试 | 已用 `testing::TestDatabase`，提供 PostgreSQL 模板克隆与 MySQL 独立库迁移 |
| 过程宏 | 已用 `syn` + `quote` 解析控制器 impl 项并生成 `Resource` 实现 |
| 假数据 | `fake` 尚未引入；已有 Mail/SMS/Audit fake 是各能力自身测试实现，不依赖该 crate |
| 依赖审计 | 当前验证使用 `cargo-deny`；`cargo-audit` 为规划工具，不等于已执行 |
| 语义化版本检查 | M6 对已有源码版本做 API 兼容性检查；使用 `cargo-semver-checks` 时明确源码 baseline，不以 crates.io 已发布包为前提 |
| 变更日志 | `git-cliff` 为规划工具 |
| 文档站 | M6 规划 `mdbook`；当前尚无 mdBook 用户文档站 |
| CI | GitHub Actions；PostgreSQL 与 MySQL 以 service 容器提供 |

## 版本政策

v2 端侧规划：Admin Web 使用 pure-admin 精简国际化版 + Element Plus（Vue 3 / TypeScript / Vite 路线），App 使用 uni-app；PC 端仅设计，暂不引入运行时。B1 锁定具体兼容版本、App 实现代际/渲染模式与目标平台，记录实际依赖和锁文件；目前这些前端依赖尚未安装，不属于 v1 Rust 核心依赖。范围、选型依据及验收见 [v2 计划](plans/v2-admin-platform.md)。

Edition 2024；MSRV 为 stable 减两个小版本；0.x 期间破坏性变更只在次版本号；依赖升级随 Dependabot，升级主版本写 ADR。

## 已确认但尚未引入的底层补强

[底层补齐计划](plans/v1-foundation-completion.md) F1–F10 是目标范围；具体版本、MSRV、许可证、安全维护和构建成本在实施 Spec 核实后登记。F1 使用 `cron-parser` 0.12 与 `chrono-tz` 0.10.4；F2 使用 ICU4X 2.3.x 编译数据组件，cargo feature 只启用日期、数字、locale 与复数能力，不引入运行时数据服务。

- cron/时区、i18n 复数/地区格式化、邮件与生成模板优先成熟组件；候选名称仅供评估，不能当作已锁定选择。
- OAuth2/OIDC、TOTP、CSV/XLSX 使用成熟协议/格式组件；复用本地认证/密钥、Queue/Storage，不自研协议和文件格式。
- cache tag/SWR/加载合并、查询规则、租户作用域与 fixtures 优先在既有能力上整合；只在有明确附加价值时引入依赖，不建立新 ORM/容器。
