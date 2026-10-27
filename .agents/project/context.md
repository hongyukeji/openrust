# 项目上下文

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：OpenRust 当前处于 v1 M5；源码 workspace、C1–C3 中文化及已列 M3/M4/F6/F7/F8 能力均按各自契约验收。gRPC G1–G3c 与 ADR-059 WebSocket/SSE 真实跨节点在线路由均已完成对应验证；当前按 todo.md 继续 M5 可观测性与生产资源。全栈多服务集群、v1 性能与生产门禁仍未完成。

## 当前状态

| 项 | 值 |
| --- | --- |
| 版本 | 0.0.0（源码开发版本；整套 workspace 交付，不独立发布 Cargo 包） |
| 阶段 | v1 处于 M5 收尾：M3/M4 与多数 M5 功能簇已有对应验收记录；当前工作是可观测性、生产资源验证和 M6 最终门禁。具体交付及未完成项以 [progress.md](progress.md)、[todo.md](todo.md) 和 [plan.md](plan.md) 为准；完整集群、性能及生产门禁尚未完成 |
| 仓库 | `https://github.com/hongyukeji/openrust`（公开，默认分支 `main`，已推送）；本地 `/Users/Home/Codes/Rust/openrust` |
| 协作模式 | 单开发者 + AI Agent；Claude 负责方案与设计，Codex 负责实现与验证 |
| 本机工具链 | Rust stable 1.99.1（默认）与 Rust 1.97.0 MSRV（`rustup`），`cargo-deny` 0.20 已安装 |

## 固定约束

- 设计取向按 [ADR-001](decisions/adr-001-own-kernel-on-axum.md#成熟组件优先与-laravel-式开发体验)：优先复用优秀第三方组件，统一配置、生命周期、错误、扩展契约与开发入口，以 Laravel 的约定优于配置和一致体验为重要参照；采用 Rust 惯用接口，按真实缺口补充实现。已验证能力不因本取向重写；参考范围按下文 V1/V2 分工执行。业务多样性由应用和可选领域插件承接，当前优先完成 V1 通用底座，以减少开发者基础设施工作为评价原则。

- 项目语言采用 [简体中文为主、英文为辅](style.md#语言规范)：第一方文档、Rustdoc、源码/配置注释与生成模板说明立即按此编写；既有英文安排分批整改。完整英文文档待框架成熟后再评估，技术名称和原始许可证等例外保持有效，运行时 i18n 契约另行管理。

- 源码交付与中性包名按 [ADR-039](decisions/adr-039-source-workspace-distribution.md) 已实现：开发者获取整套框架，在 `apps/` 可为根 `server` 编写显式 Rust 业务模块，也可通过 `openrust new` 生成 `apps/<name>/` 独立应用；两种形态的编译边界不同，见 [ADR-063](decisions/adr-063-explicit-app-module-registry.md)。根 workspace packages 为 `openrust`、`cli`、`macros`、`testing`、`performance`，均禁用发布，四个独立工具 package 集中位于 `tools/`；包名、`openrust::` API 与 CLI 命令名保持不变。`performance` 独立承载维护者基准，不进入默认构建目标。宿主入口与测试目录按 [ADR-062](decisions/adr-062-root-server-entry.md) 迁入根 package：根 `src/main.rs` 是唯一 `server` 入口，根 `apps/mod.rs` 显式组合模块；不会自动扫描或动态加载 Rust 代码。生成器合并 workspace 成员并按应用定位启动和配置；干净源码副本、双应用隔离及 PostgreSQL 18/MySQL 8.4 验收通过。C1 默认文档入口、C2 Rustdoc/源码注释、C3 生成说明与 CLI 帮助均已按简体中文完成。

- 应用中性命名按 [ADR-016](decisions/adr-016-neutral-application-namespaces.md)：默认环境变量 `APP_`（`Application::config_namespace()` 允许启动前定制，core/bootstrap 已实现），默认表前缀 `tb_`，支持自定义/空前缀和显式全名映射；数据库 resolver 已贯通连接、迁移、TestDatabase 与认证 SQL，并经 PostgreSQL/MySQL 验证。旧品牌前缀不能作为新设计 contract。
- 本地数据库、缓存等基础服务验证必须先检查并复用现有且版本/配置/连通性合适的服务（包括 Docker 容器和本机服务）；当前开发目标为 PostgreSQL 18、MySQL 8.4 LTS、Redis 8。只有没有合适服务，或现有服务确实无法用于本项测试时，才创建隔离的临时服务。优先用 `TestDatabase`、独立测试库/数据做隔离；不得为方便而重复创建服务，也不得擅自停止、删除、重建或改动已有容器、卷、数据和配置。新建临时服务时使用独立 Compose project 与必要的隔离端口，结束后只清理本任务明确创建且确认无其他使用者的资源。
- OpenRust 核心目录按 [ADR-021](decisions/adr-021-loco-style-module-layout.md)、[ADR-062](decisions/adr-062-root-server-entry.md) 与 [ADR-063](decisions/adr-063-explicit-app-module-registry.md) 已完成收敛：根 `src/` 放框架 modules 与唯一 `server` 启动入口，根 `apps/mod.rs` 显式注册源码 workspace 宿主的 Rust 业务模块；`apps/<name>/` 生成项目仍是独立 Cargo package。根 `tests/` 验证公共 API 和集成宿主，`apps/example/` 放综合示例，`tests/consumers/` 放独立验收项目。裸 `cargo run` 与真实 `/health` smoke 已通过；工具包、测试、配置与基准迁移验收见 [迁移 Spec](plans/module-layout-migration-spec.md)。
- 生成项目中的运行时配置文件按领域命名并放项目根 `config/`（如 `app.toml`、`cache.toml`）；环境值和密钥使用 `.env` 或进程环境变量。Rust 配置代码放 `src/config/`，不将 TOML/YAML 放入 `src/`。
- 配置类型按实际能力位于 `openrust::config` 的各子模块（清单见 core/core.md），由唯一 `ConfigLoader` 加载；旧 auth/database/mail 类型路径通过 re-export 保持。根宿主与生成项目提供 `.env.example`；所有环境均可加载应用工作目录的 `.env`，进程环境变量优先，文件错误安全失败；生产部署限制文件权限，且不得将含 `.env` 的应用根目录公开为静态站点。

- [AI模型平台](plans/ai-model-platform.md) 是唯一 AI 项目计划，集中维护范围、插件交付与工程验收。只有用户明确要求开发且框架成熟门禁满足后才启动；使用 `projects/ai-model-platform` 与一个独立 worktree，直接包含网关源码并以本地依赖接入；当前不创建实现、分支或环境。完成框架 V1/V2 或通用插件机制不构成开工许可。

- 领取/切换功能簇、压缩恢复或收到新需求时，按 [执行校准](plan.md#执行校准) 核对最新计划、Git/WIP 与待办；旧消息不能覆盖已确认范围，原阶段完成不能替代新增能力验收。
- 用户给出的扩展示例不是穷举清单；Agent 在每个功能簇主动评估常见校验/转换/替换/通知需求，复用合适机制并记录取舍，灵活补齐合理扩展点，不机械增加每个方法的前后钩子，见 [自主判断规则](plans/framework-design.md#开发-agent-的自主扩展性判断)。
- v1 在事务提交前后、注册/登录、文件上传等关键流程提供公开类型化扩展点，开发者通过注册钩子/监听器扩展而不改核心；同步校验与成功后事件、回滚/失败与集群投递边界分清，见 [设计规则](plans/framework-design.md#关键流程的公开扩展点)。
- 允许按相关功能簇借鉴 OpenSpring 的优秀设计思想；先只读核对来源代码，评估已有能力并按 Rust 语义适配，记录取舍和验收，不机械移植或引入平行系统，见 [跨语言设计参考](plans/framework-design.md#跨语言设计参考)。
- 集群部署是 v1 必须项：多 API/worker/scheduler、共享状态、跨节点实时投递、滚动发布及故障恢复必须有双库多副本证据，见 [集群计划](plans/v1-cluster-deployment.md)；进程内或节点本地能力不能冒充共享状态。
- gRPC 已按 [ADR-040](decisions/adr-040-grpc-integration.md) 正式纳入 v1 底层范围，M5 交付服务端/客户端、四类调用、安全/流资源、生成与真实集群验收；旧“仅 v2 候选”已失效。G1 基础/生成已完成并提交；G2a TLS/mTLS、G2b JWT/PAT/Policy 身份与错误映射、G2c deadline/资源限制/health/reflection/有界停机、G3a HTTP/RPC 联合运行及 G3b 新旧 proto/traceparent 兼容均已通过对应 TCP 与 workspace 门禁；G3c 双节点路由已按 `todo.md` 验收；全栈集群仍留在 M6 最终门禁。跨协议验证/上下文、出站策略和生产保障见 [能力覆盖](plans/v1-capability-coverage.md)。
- V1 缺口已确认补齐，见 [底层补齐计划](plans/v1-foundation-completion.md)：日历调度、邮件模板、国际化表达、缓存 tag/SWR/同键加载、查询授权、可选租户隔离、外部 OAuth2/OIDC/SSO 与 TOTP、CSV/XLSX、模板覆盖和轻量 fixtures 均须交付。“可选”指应用启用方式，不是可省略开发。更多适配按该计划后续清单另行评估；ADR-047 查询授权、ADR-048 稳定键分批、ADR-049 迁移互斥、ADR-050 存储与上传下载及 ADR-051 出站目标安全和 ADR-052 命名依赖韧性均已完成，OIDC 与 TOTP/恢复码子簇均已完成；F7 双应用 SSO/统一退出已完成；F6-A/F6-B 可信租户上下文、查询隔离及 Cache、Queue、Storage、Audit 传播已完成；F8-A/B CSV/XLSX 数据交换已通过双库验收；gRPC G1 基础/生成、G2a TLS/mTLS、G2b 身份/错误映射、G2c 调用预算/资源/health/reflection/有界停机及 G3a HTTP/RPC 联合生命周期与 G3b proto/trace 兼容已通过对应验收，G3c 双节点 HTTP/2 路由和 ADR-059 WebSocket/SSE 实时在线路由均已验收；后续按 `todo.md` 继续 M5 可观测性与生产资源簇。
- 版本分工：v1 主攻并交付完整通用框架底层能力，v2 完善前端视图与现成后台套件：Rust 服务端、Vue Admin 管理界面、uni-app App；PC 端仅规划设计、暂不开发。Admin 视图骨架采用 pure-admin 精简国际化版 + Element Plus，具体版本按 [v2 计划](plans/v2-admin-platform.md) 的 B1 锁定。后台复用 v1 公共能力，不把组织模型和前端依赖放进 core，见 [版本路线](roadmap.md)。当前先完成 v1 全部门禁再实施 v2；AI模型平台仍须用户另行明确要求开发。
- 通用加密与密钥轮换在框架进程内完成，默认不依赖第三方服务；普通路径只需密钥配置及加解密调用，采用当前密钥写入、历史密钥解密，不强制远程密钥中心或复杂算法配置，详见 [通用能力计划](plans/v1-common-capabilities.md)。
- v1 完整通用底座按 [能力覆盖与归属](plans/v1-capability-coverage.md) 交付；常见底层机制由框架整合，每类职责一个入口，不让业务开发者重复搭建，不因复用 Rust 库而省略框架接入与验收。
- 国际化、WebSocket、统一出站 HTTP、通用加密与密钥、签名链接、审计、请求幂等均纳入 v1。请求级国际化已按 [ADR-032](decisions/adr-032-request-internationalization.md) 实现；审计记录按 [ADR-035](decisions/adr-035-audit-recording.md) 接入数据库与可替换 sink，并支持同事务写入；签名链接按 [ADR-036](decisions/adr-036-signed-links.md) 实现用途隔离、过期校验和可选共享撤销；邮件/SMS driver 与 Twilio adapter 已按 [ADR-033](decisions/adr-033-sms-driver-and-template-contract.md) 实现，真实 Twilio 实发仍待授权账号与目标；ADR-059 已提供在线 WebSocket/SSE 与 Redis 跨节点路由，完整多服务集群仍待 M6 门禁。
- 性能与开箱即用补强按 [专题计划](plans/v1-performance-and-readiness.md) 执行；其中验收要求与待决策建议分开记录，实施前先确定 contract，不以规划代替完成证据。
- 框架定位：Rust 应用开发的“瑞士军刀”，以高性能为基础，提供开箱即用的常用能力和稳定扩展机制，让开发者直接进入业务开发，减少重复搭建底层设施。性能须以代表性场景实测，不因使用 Rust 就宣称更快。
- 规划名称统一为“v1 版本”，原 `plans/gen1-plan.md` 路径暂保留以兼容引用；当前开发包版本仍以 `Cargo.toml` 为准，发布与 tag 由开发者执行。
- v1 版本包含邮件与短信通信能力；Mailer/SmsSender、SMTP/Twilio contract、LocalizedEmail 与模板 SMS 已按 [ADR-033](decisions/adr-033-sms-driver-and-template-contract.md) 实现。真实 Twilio 投递待授权凭据和目标；验证码机制纳入 v1，薄多渠道通知编排纳入 v1，业务通知中心/UI 归 v2 后台套件，见 [ADR-011](decisions/adr-011-sms-and-communication-baseline.md)。
- 默认实现满足常见需求，公开扩展契约承接差异需求；数据库、缓存等能力复用既有插件与配置机制，具体边界见 [ADR-009](decisions/adr-009-extensible-capability-drivers.md)。缓存 driver/store contract 见 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md)，代码仍待 M5。
- OpenRust 不是任何既有项目的移植，没有兼容包袱；设计以 [v1 版本计划](plans/gen1-plan.md) 与 [框架设计](plans/framework-design.md) 为准。
- 应用内核自研，基于 `tokio` + `axum` + `tower`。V1 综合参考顺序为 Loco、Laravel、Goravel、OpenSpring；若依、芋道仅在 V2 开发时参考。均不作为底层依赖或完整复制目标，见 [框架参考与对标](plans/framework-design.md#框架参考与对标)；OpenRouter 仅是 AI模型平台的产品能力对标。
- 基础读写分离已按 [ADR-029](decisions/adr-029-database-read-routing.md) 纳入并实现：默认单库，可选同 backend 只读副本，显式 `read_connection()` 轮询，`connection()`/事务固定主库，`with_primary` task-local 写后读保护；PostgreSQL 18.6 / MySQL 8.4.13 真实复制暂停/追平测试通过。完整集群与代表性业务性能基线仍未完成。
- 数据服务暂定最低支持 PostgreSQL 14+、MySQL 8.0+、Redis 6.2+；日常本地开发与 Compose 使用 PostgreSQL 18、MySQL 8.4 LTS、Redis 8，常规数据库 CI 使用 PostgreSQL 18 与 MySQL 8.4，Redis 集成在 M5 实现后纳入验证。运行时 PostgreSQL/MySQL 版本检查已与暂定下限对齐；最低版本的完整框架兼容性仍须在 v1 发布验收中复核，Redis 6.2+ 须待 M5 Redis driver 落地后做真实验证。仅在主版本跨度大、涉及版本专属行为或存在明确兼容风险时增加额外专项测试。每个数据功能仍须有 PostgreSQL/MySQL 双库证据（[ADR-002](decisions/adr-002-seaorm-dual-database.md)）。
- 同时支持单机与集群；标准应用默认 Redis cache，memory/file 显式选择，库 feature 保持可组合，见 [ADR-015](decisions/adr-015-redis-default-calibration.md)。需要跨节点共享语义时必须明确配置共享后端，不能静默退化；队列/会话仍默认共享数据库，部署边界见 [ADR-013](decisions/adr-013-deployment-and-redis-cache-default.md)。缓存实现待 M5。
- 认证以 Bearer JWT access token 为前后端分离 API 的主要路径；高熵 refresh token 轮换并存储哈希。通用 Cookie Session 保留为显式可选项，PAT 面向自动化/服务集成，三者复用同一主体与授权入口；按 [ADR-018](decisions/adr-018-jwt-primary-authentication.md) 实施。AuthFlows、验证码/限流、Policy/RBAC 与可选 Session helper 已实现；PostgreSQL 18 / MySQL 8.4 数据库与 runtime 测试、新生成项目完整 TCP HTTP 流程及 Mailer/Policy 生成器双库 check/test 均已通过。M3 运行时、生成器及通用 HTTP 限流/查询边界，以及 ADR-041 认证与事务公开流程扩展已有双库验收；真实 Twilio 送达需授权账号与目标。
- 许可证 MIT OR Apache-2.0；Edition 2024；当前 MSRV 为 `Cargo.toml` 中的 1.97；升级政策为 stable 减两个小版本（[ADR-005](decisions/adr-005-license-naming-msrv.md)）。
- 生成项目布局、CLI、配置键与公共 API 命名以 [ADR-006](decisions/adr-006-rust-native-naming.md) 为唯一 contract；M6 才建立 mdBook `docs/`，此前用户文档维护在 crate README。
- 库 crate 禁止 `unsafe`、`unwrap` / `expect` / `panic`；公共项必须有文档。细则见 [conventions.md](conventions.md)。
- 功能范围以 v1 主计划与已确认专题要求为准；用户新增明确需求先校准对应计划与依赖，不因旧阶段已关闭而丢弃。未经确认的远期候选进入 `roadmap.md`。

## 对外动作（仅开发者执行）

- 当前不安排 crates.io 名称预留、占位包或独立 Cargo 包发布。
- 源码 tag/release、远端推送等对外动作仍需独立授权，不从完成本地开发推断已获授权。
