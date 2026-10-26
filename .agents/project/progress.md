# 当前进度

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：根 `openrust` 模块迁移已独立提交；M2 读写分离已实现，并在 PostgreSQL 18.6 / MySQL 8.4.13 真实复制拓扑验证。模块化配置、`.env`、统一出站 HTTP、请求级国际化、邮件/SMS 驱动、应用加密、审计、签名链接及 M3 认证双库/生成 HTTP 总验收已完成；Twilio 真实投递仍待授权账号与目标，完整集群验收未关闭。日常开发基线为 PostgreSQL 18 / MySQL 8.4 LTS / Redis 8；PostgreSQL 16 测试仅作历史专项证据。

| 项 | 状态 |
| --- | --- |
| v1 版本计划与框架设计 | 已定稿，见 `plans/` |
| M3 统一出站 HTTP | 已完成独立功能簇：ADR-031、可选 `http-client`/CLI `--http-client`、reqwest 0.13 Rustls client、transport fake、受限 request/response stream、超时/取消、显式有界幂等重试。15 项 client tests、`apps/server` 集成、生成项目 check/test 和 default/all-features workspace fmt/clippy/test/doc、`cargo deny check` 通过；随本功能簇独立本地提交，不推送。真实第三方供应商不属于本 client 本地验收，后续 M3 供应商发送仍需已授权目标与凭据。 |
| M3 请求级国际化 | 实现 `openrust::i18n`、中英框架消息、JSON 应用资源、TOML locale 默认/回退、Accept-Language q 值选择、请求 task-local 隔离、错误与 validator rule 本地化、显式 locale scope，并接入 `apps/server` 与新项目模板。6 项模块测试、5 项聚合 HTTP 测试、生成项目 check/test 与真实进程中文 404 smoke 通过；default/all-features workspace fmt/clippy/test、warnings-denied docs、`cargo deny check` 通过。已提交 `dc41812`。 |
| M3 邮件与短信驱动 | ADR-033 实现 `Mailer`/`SmsSender` 可扩展入口、SMTP/log/fake、Twilio REST adapter、短信文本/模板内容、显式 locale、production 禁止 log/明文远端 SMTP、CLI `--sms-twilio` 与默认模板注册。Twilio 请求/认证/错误/timeout/无重试通过 fake transport 测试；Mailpit v1.32.0 本机 TCP 实收测试通过；生成 Twilio 项目 check/test、workspace default/all-features tests、clippy、warnings-denied docs、cargo deny 通过。真实 Twilio 号码投递未授权/未配置，仍待外部凭据与目标。 |
| M3 应用数据加密与密钥轮换 | ADR-034 实现可选 `encryption` feature、`openrust::security::{Encryptor,EncryptionPlugin}`、AES-256-GCM-SIV/AAD envelope、历史 key 解密、配置脱敏、`openrust key generate` 与 `openrust new --encryption`。8 项加密测试、11 项 CLI 测试、workspace default/all-features tests/clippy/docs、cargo deny 与生成项目本地 patch check/test 通过；已提交 `e5e86f2`。所用 crate 文档声明当前无完整独立审计，已记为发布前复核项。 |
| M3 审计记录 | ADR-035 实现 `openrust::audit`：显式事件与 sink、details allowlist/redaction、`test-util` fake、数据库迁移/持久化/有界清理、事务内写入；`Policy::grant_permission_in_transaction` 与审计在 PostgreSQL 18/MySQL 8.4 上验证同事务提交、回滚和 sink 失败回滚。`--auth` 模板注册 audit plugin/migration；生成项目在两库通过真实 HTTP auth 流程。workspace default/all-features tests/clippy、warnings-denied docs、cargo deny 与双库完整 `auth_database` 均通过。 |
| M3 签名与限时链接 | ADR-036 实现独立 `signing` feature、用途专属 key ring、purpose/subject/TTL/JTI 签名验证、key rotation 与可选 PostgreSQL/MySQL 共享撤销 store/迁移/有界清理；4 项 signer 单测和两库各 6 项 `auth_database` 通过。`--auth` 生成项目注册 service 与 migration；两种数据库生成项目通过 fmt/check/真实 HTTP auth flow。workspace default/all-features clippy/tests、warnings-denied docs、无默认 feature check、cargo deny 与 diff check 通过。 |
| M3 认证与安全总验收 | PostgreSQL 18 / MySQL 8.4 的 `auth_database` 与 `auth_runtime` 各 6 项通过；新生成 `--auth` 项目在两库分别通过 fmt、`cargo check` 和真实 TCP `auth_flows`。覆盖注册/验证、JWT/refresh、Session/CSRF、PAT、限流、Policy/RBAC、可信代理、审计和签名撤销。Twilio 协议测试不等于实发；实发等待授权目标与凭据。 |
| M4 数据库队列与 worker 基础簇 | 已实现显式迁移、handler allowlist、事务派发、SKIP LOCKED、失败重试、worker 并发/租约/心跳、超时/合作取消和强杀恢复。PG18/MySQL8.4 真实服务测试通过。M4 尚未完成：旧 token 接管竞态、任务链/批次/唯一键、调度器、事件、多实例调度验收。详见 `core/queue.md`。 |
| 架构决策 | 以 `decisions/` 中有效状态为准；缓存标准应用默认 Redis（ADR-015），driver/store contract 见 ADR-014，均不代表缓存已实现 |
| 仓库基线（许可证、README、工具链声明、Agent 契约、CHANGELOG、贡献与安全政策） | 已就位 |
| workspace、lint 基线、CI 工作流 | 已就位；CI 双库 job 已启用，MSRV 为 Rust 1.97 |
| `openrust::core` / `openrust::config` | 已迁入根 package 并测试：`APP_` 默认及自定义配置命名空间、配置分层与 `.env` 引导、错误和响应包装、服务注册表、Plugin 级联、嵌套命令与固定窗口限流契约 |
| 配置模块整理 | `src/config/{app,api,auth,database,logger,mailer,server}.rs`；Auth/Database/Mail 旧路径 re-export；生成 `.env.example` 含本地覆盖示例，不含可用密钥。production 跳过 `.env`，进程环境变量优先；workspace default/all-features 门禁通过 |
| `openrust::http` | 已迁入根 package 并测试：`Routes`、`Resource` / `ResourceActions`、统一响应与分页、OpenRust 提取器、`Inject`、`Validated`、404/405/408/413 包装、中间件、`/health` 与 `serve` |
| `openrust-macros` | `#[openrust::resource]` 从控制器动作关联函数生成 `Resource` 实现，并由 `apps/server` 集成测试覆盖 |
| `openrust` / `openrust-cli` | facade `run` 与数据库/auth/audit/signing plugin facade；`new` 支持无数据库、PostgreSQL、MySQL 和 `--auth` 模板；认证模板生成 Auth flows routes、audit 与 signed-link migrations/plugins；`generate model` / `generate migration`、`dev` / `serve` 与运行命令代理可用 |
| 生成认证项目验收 | CLI 包名测试模板已改为从生成 manifest 与 `CARGO_PKG_NAME` 比对；PG18 与 MySQL 8.4 新生成 `--auth` 项目均通过 fmt、check 和真实 HTTP/database 流程 |
| `openrust::db` | `DatabaseConfig` / `DatabasePlugin` / `Db`、主库与显式只读副本池、轮询选择、task-local primary scope、自定义 SeaORM 初始化、事务、迁移/seed 命令、UTC/JSON 方言 helper、table resolver、双库、MySQL Rustls TLS、共享固定窗口限流存储 |
| `openrust-test` | `TestDatabase` 为每个用例创建并迁移独立库；支持 `create_with_table_names` 以相同 resolver scope 执行迁移；PostgreSQL 迁移模板克隆，MySQL 独立建库；`close().await` 清理；基线两库集成测试通过 |
| `openrust::mail` | `Email` / `Mailer`，log、fake、可选 SMTP Rustls driver；已接入 facade、server 与项目模板 |
| `apps/server` | M1 聚合入口；`auth` feature 集成测试验证 facade PAT 与 JWT 路由、JWT 不依赖 SessionPlugin，并与 `Inject<T>` 共存。生成 AuthFlows HTTP 真机测试现由 CLI 模板提供；默认数据库认证器的 PAT 入口在生成模板增加，MySQL e2e 通过 |
| `openrust::auth` | AuthPlugin/custom Authenticator、Session/PAT/JWT 路由认证、CurrentUser/OptionalUser、共享 Session/CSRF、refresh/verification/Policy migrations、Argon2id、AuthFlows 与基础 Policy/RBAC 已实现；PostgreSQL 18 / MySQL 8.4 数据库、runtime 与生成项目 TCP HTTP 矩阵通过。真实 Twilio 投递待授权账号与目标 |
| 配置与数据命名 | `APP_` 默认、自定义单一命名空间、环境/`.env` 引导、配置目录和前缀覆盖已实现并通过 core/CLI/workspace 与双库生成项目 check/test；数据库表前缀/映射实现已接入 Db、MigrationPlugin、TestDatabase 与 Auth SQL；PostgreSQL/MySQL 真实隔离库矩阵与迁移 runner 验证通过，工作区全量 fmt/clippy/test/doc 通过 |
| 后续能力模块 | `cache` / `queue` / `events` / `storage` 尚未实现；按 M4/M5 计划直接在根 package 创建非空模块，不保留空壳 package |
| v1 新增范围 | 读写分离已按 ADR-029 实现并通过复制延迟验收；流程钩子、缓存、可观测性及最终集群按专题计划补做 |
| 验证证据 | 根迁移与 M2 相关默认/all-features 检查通过；PG18.6 / MySQL8.4.13 真实复制测试覆盖暂停/追平、主库新读和两个独立 App scope；500 次串行 `SELECT 1` 路由微基准已记录。真实 SMTP 送达和完整多节点集群仍无验收证据 |
| M2 验收 | 双库数据库测试、model/migration 生成项目编译与测试、生成迁移 up/status/down、默认与 all-features workspace 验证通过；详见 `memory/testing.md` |
