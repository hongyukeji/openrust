# 项目上下文

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：OpenRust 是全新的开源 Rust 应用开发框架，当前进入 v1 版本 M4 队列、调度与事件能力。

## 当前状态

| 项 | 值 |
| --- | --- |
| 版本 | 0.0.0（未发布，crates.io 名称未预留） |
| 阶段 | v1 版本 M0-M3 本地框架能力及认证/安全总验收已完成；M2 读写分离通过 PostgreSQL 18 / MySQL 8.4 真实复制拓扑验证；当前按依赖顺序开始 M4 队列、调度与事件。真实 Twilio 发送仍待授权账号与目标，属于外部验收条件 |
| 仓库 | `https://github.com/hongyukeji/openrust`（公开，默认分支 `main`，已推送）；本地 `/Users/Home/Codes/Rust/openrust` |
| 协作模式 | 单开发者 + AI Agent；Claude 负责方案与设计，Codex 负责实现与验证 |
| 本机工具链 | Rust stable 1.99.1（默认）与 Rust 1.97.0 MSRV（`rustup`），`cargo-deny` 0.20 已安装 |

## 固定约束

- 应用中性命名按 [ADR-016](decisions/adr-016-neutral-application-namespaces.md)：默认环境变量 `APP_`（`Application::config_namespace()` 允许启动前定制，core/bootstrap 已实现），默认表前缀 `tb_`，支持自定义/空前缀和显式全名映射；数据库 resolver 已贯通连接、迁移、TestDatabase 与认证 SQL，并经 PostgreSQL/MySQL 验证。旧品牌前缀不能作为新设计 contract。
- 本地数据库、缓存等基础服务验证必须先检查并复用现有且版本/配置/连通性合适的服务（包括 Docker 容器和本机服务）；当前开发目标为 PostgreSQL 18、MySQL 8.4 LTS、Redis 8。只有没有合适服务，或现有服务确实无法用于本项测试时，才创建隔离的临时服务。优先用 `TestDatabase`、独立测试库/数据做隔离；不得为方便而重复创建服务，也不得擅自停止、删除、重建或改动已有容器、卷、数据和配置。新建临时服务时使用独立 Compose project 与必要的隔离端口，结束后只清理本任务明确创建且确认无其他使用者的资源。
- OpenRust 核心目标架构按 [ADR-021](decisions/adr-021-loco-style-module-layout.md)：根 `src/` 放框架能力 modules，`apps/` 保留为同一仓库/workspace 内应用目录约定，`examples/` 放可复用示例，根 `tests/` 验证框架公共 API。当前 `apps/server` 是内部集成宿主；等根测试与示例覆盖其职责后才决定移除该应用，不删除 `apps/` 约定。源码迁移前已对既有 M3 WIP 做双库和生成项目预检；迁移后仍须复验，路径调整不代表 M3 功能完成。
- 生成项目中的运行时配置文件在项目根 `config/`；Rust 配置代码放 `src/config/` 模块目录，建议以 `mod.rs` 为入口，按复杂度拆分，不将 TOML/YAML 文件置于 `src/`。
- 配置类型现按实际域位于 `openrust::config::{app,api,auth,database,logger,mailer,server}`，由唯一 `ConfigLoader` 加载；旧 auth/database/mail 类型路径通过 re-export 保持。生成项目提供 `.env.example`；`.env` 仅供非生产本地覆盖，外部进程变量优先，production 不读取 `.env`。

- AI 平台为基于 OpenRust 的独立产品，最终按 [OpenRouter 对标计划](plans/ai-platform-openrouter-parity.md) 复刻公开功能与流程（MVP 非终点），须待框架 v1/v2 完整体成熟后再由开发者决定立项，平台完全体 P0–P8 验收通过并获上线批准后才对外开放；当前仅规划、不启动实现，后续开发必须使用 `projects/*` 分支和独立 worktree，产品业务不得进入框架核心或改变通用契约，详见 [产品计划](plans/ai-platform-product.md)。

- 领取/切换功能簇、压缩恢复或收到新需求时，按 [执行校准](plan.md#执行校准) 核对最新计划、Git/WIP 与待办；旧消息不能覆盖已确认范围，原阶段完成不能替代新增能力验收。
- 用户给出的扩展示例不是穷举清单；Agent 在每个功能簇主动评估常见校验/转换/替换/通知需求，复用合适机制并记录取舍，灵活补齐合理扩展点，不机械增加每个方法的前后钩子，见 [自主判断规则](plans/framework-design.md#开发-agent-的自主扩展性判断)。
- v1 在事务提交前后、注册/登录、文件上传等关键流程提供公开类型化扩展点，开发者通过注册钩子/监听器扩展而不改核心；同步校验与成功后事件、回滚/失败与集群投递边界分清，见 [设计规则](plans/framework-design.md#关键流程的公开扩展点)。
- 允许按相关功能簇借鉴 OpenSpring 的优秀设计思想；先只读核对来源代码，评估已有能力并按 Rust 语义适配，记录取舍和验收，不机械移植或引入平行系统，见 [跨语言设计参考](plans/framework-design.md#跨语言设计参考)。
- 集群部署是 v1 必须项：多 API/worker/scheduler、共享状态、跨节点实时投递、滚动发布及故障恢复必须有双库多副本证据，见 [集群计划](plans/v1-cluster-deployment.md)；进程内或节点本地能力不能冒充共享状态。
- 版本分工：v1 交付完整通用框架，v2 交付类似若依使用体验的现成后台套件；后台复用 v1 公共能力，不把组织模型和前端依赖放进 core，见 [v2 计划](plans/v2-admin-platform.md) 与 [版本路线](roadmap.md)。当前仍先完成 v1。
- 通用加密与密钥轮换在框架进程内完成，默认不依赖第三方服务；普通路径只需密钥配置及加解密调用，采用当前密钥写入、历史密钥解密，不强制远程密钥中心或复杂算法配置，详见 [通用能力计划](plans/v1-common-capabilities.md)。
- v1 完整通用底座按 [能力覆盖与归属](plans/v1-capability-coverage.md) 交付；常见底层机制由框架整合，每类职责一个入口，不让业务开发者重复搭建，不因复用 Rust 库而省略框架接入与验收。
- 国际化、WebSocket、统一出站 HTTP、通用加密与密钥、签名链接、审计、请求幂等均纳入 v1。请求级国际化已按 [ADR-032](decisions/adr-032-request-internationalization.md) 实现；审计记录按 [ADR-035](decisions/adr-035-audit-recording.md) 接入数据库与可替换 sink，并支持同事务写入；签名链接按 [ADR-036](decisions/adr-036-signed-links.md) 实现用途隔离、过期校验和可选共享撤销；邮件/SMS driver 与 Twilio adapter 已按 [ADR-033](decisions/adr-033-sms-driver-and-template-contract.md) 实现，真实 Twilio 实发仍待授权账号与目标；其余范围及待决策建议见 [通用能力计划](plans/v1-common-capabilities.md)。SSE 等其他通用机制按能力覆盖计划补齐，完整实时消息平台另行扩展。
- 性能与开箱即用补强按 [专题计划](plans/v1-performance-and-readiness.md) 执行；其中验收要求与待决策建议分开记录，实施前先确定 contract，不以规划代替完成证据。
- 框架定位：Rust 应用开发的“瑞士军刀”，以高性能为基础，提供开箱即用的常用能力和稳定扩展机制，让开发者直接进入业务开发，减少重复搭建底层设施。性能须以代表性场景实测，不因使用 Rust 就宣称更快。
- 规划名称统一为“v1 版本”，原 `plans/gen1-plan.md` 路径暂保留以兼容引用；当前开发包版本仍以 `Cargo.toml` 为准，发布与 tag 由开发者执行。
- v1 版本包含邮件与短信通信能力；Mailer/SmsSender、SMTP/Twilio contract、LocalizedEmail 与模板 SMS 已按 [ADR-033](decisions/adr-033-sms-driver-and-template-contract.md) 实现。真实 Twilio 投递待授权凭据和目标；验证码机制纳入 v1，薄多渠道通知编排纳入 v1，业务通知中心/UI 归 v2 后台套件，见 [ADR-011](decisions/adr-011-sms-and-communication-baseline.md)。
- 默认实现满足常见需求，公开扩展契约承接差异需求；数据库、缓存等能力复用既有插件与配置机制，具体边界见 [ADR-009](decisions/adr-009-extensible-capability-drivers.md)。缓存 driver/store contract 见 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md)，代码仍待 M5。
- OpenRust 不是任何既有项目的移植，没有兼容包袱；设计以 [v1 版本计划](plans/gen1-plan.md) 与 [框架设计](plans/framework-design.md) 为准。
- 应用内核自研，基于 `tokio` + `axum` + `tower`；按统一维度综合评估以下参考框架：Loco、Laravel、Goravel、OpenSpring、RuoYi 若依、芋道；不将任何一个框架作为底层依赖或完整复制目标，见 [框架参考与对标](plans/framework-design.md#框架参考与对标)。OpenRouter 是独立产品的对标对象，不属于框架参考清单。
- 基础读写分离已按 [ADR-029](decisions/adr-029-database-read-routing.md) 纳入并实现：默认单库，可选同 backend 只读副本，显式 `read_connection()` 轮询，`connection()`/事务固定主库，`with_primary` task-local 写后读保护；PostgreSQL 18.6 / MySQL 8.4.13 真实复制暂停/追平测试通过。完整集群与代表性业务性能基线仍未完成。
- 数据服务暂定最低支持 PostgreSQL 14+、MySQL 8.0+、Redis 6.2+；日常本地开发与 Compose 使用 PostgreSQL 18、MySQL 8.4 LTS、Redis 8，常规数据库 CI 使用 PostgreSQL 18 与 MySQL 8.4，Redis 集成在 M5 实现后纳入验证。运行时 PostgreSQL/MySQL 版本检查已与暂定下限对齐；最低版本的完整框架兼容性仍须在 v1 发布验收中复核，Redis 6.2+ 须待 M5 Redis driver 落地后做真实验证。仅在主版本跨度大、涉及版本专属行为或存在明确兼容风险时增加额外专项测试。每个数据功能仍须有 PostgreSQL/MySQL 双库证据（[ADR-002](decisions/adr-002-seaorm-dual-database.md)）。
- 同时支持单机与集群；标准应用默认 Redis cache，memory/file 显式选择，库 feature 保持可组合，见 [ADR-015](decisions/adr-015-redis-default-calibration.md)。需要跨节点共享语义时必须明确配置共享后端，不能静默退化；队列/会话仍默认共享数据库，部署边界见 [ADR-013](decisions/adr-013-deployment-and-redis-cache-default.md)。缓存实现待 M5。
- 认证以 Bearer JWT access token 为前后端分离 API 的主要路径；高熵 refresh token 轮换并存储哈希。通用 Cookie Session 保留为显式可选项，PAT 面向自动化/服务集成，三者复用同一主体与授权入口；按 [ADR-018](decisions/adr-018-jwt-primary-authentication.md) 实施。AuthFlows、验证码/限流、Policy/RBAC 与可选 Session helper 已实现；PostgreSQL 18 / MySQL 8.4 数据库与 runtime 测试、新生成项目完整 TCP HTTP 流程均已通过。M3 本地认证与安全总验收已关闭；真实 Twilio 送达需授权账号与目标。
- 许可证 MIT OR Apache-2.0；Edition 2024；MSRV 为 stable 减两个小版本（[ADR-005](decisions/adr-005-license-naming-msrv.md)）。
- 生成项目布局、CLI、配置键与公共 API 命名以 [ADR-006](decisions/adr-006-rust-native-naming.md) 为唯一 contract；M6 才建立 mdBook `docs/`，此前用户文档维护在 crate README。
- 库 crate 禁止 `unsafe`、`unwrap` / `expect` / `panic`；公共项必须有文档。细则见 [conventions.md](conventions.md)。
- 功能范围以计划的「v1 版本能力范围」为准，不在里程碑内追加能力；新需求先进 `roadmap.md`。

## 对外动作（仅开发者执行）

- 在 crates.io 预留 `openrust`、`openrust-cli` 及 `openrust-*` 名称（发布 0.0.0 占位版本）。
- 发布任何版本到 crates.io。
