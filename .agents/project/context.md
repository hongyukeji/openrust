# 项目上下文

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 是全新的开源 Rust 应用开发框架，当前处于 v1 版本 M3 阶段。

## 当前状态

| 项 | 值 |
| --- | --- |
| 版本 | 0.0.0（未发布，crates.io 名称未预留） |
| 阶段 | v1 版本 M0、M1、M2 已完成，当前进入 M3「邮件、认证与授权」；当前功能簇见 `todo.md` |
| 仓库 | `https://github.com/hongyukeji/openrust`（公开，默认分支 `main`，已推送）；本地 `/Users/Home/Codes/Rust/openrust` |
| 协作模式 | 单开发者 + AI Agent；Claude 负责方案与设计，Codex 负责实现与验证 |
| 本机工具链 | Rust stable 1.99.1（默认）与 Rust 1.97.0 MSRV（`rustup`），`cargo-deny` 0.20 已安装 |

## 固定约束

- 领取/切换功能簇、压缩恢复或收到新需求时，按 [执行校准](plan.md#执行校准) 核对最新计划、Git/WIP 与待办；旧消息不能覆盖已确认范围，原阶段完成不能替代新增能力验收。
- 用户给出的扩展示例不是穷举清单；Agent 在每个功能簇主动评估常见校验/转换/替换/通知需求，复用合适机制并记录取舍，灵活补齐合理扩展点，不机械增加每个方法的前后钩子，见 [自主判断规则](plans/framework-design.md#开发-agent-的自主扩展性判断)。
- v1 在事务提交前后、注册/登录、文件上传等关键流程提供公开类型化扩展点，开发者通过注册钩子/监听器扩展而不改核心；同步校验与成功后事件、回滚/失败与集群投递边界分清，见 [设计规则](plans/framework-design.md#关键流程的公开扩展点)。
- 允许按相关功能簇借鉴 OpenSpring 的优秀设计思想；先只读核对来源代码，评估已有能力并按 Rust 语义适配，记录取舍和验收，不机械移植或引入平行系统，见 [跨语言设计参考](plans/framework-design.md#跨语言设计参考)。
- 集群部署是 v1 必须项：多 API/worker/scheduler、共享状态、跨节点实时投递、滚动发布及故障恢复必须有双库多副本证据，见 [集群计划](plans/v1-cluster-deployment.md)；进程内或节点本地能力不能冒充共享状态。
- 版本分工：v1 交付完整通用框架，v2 交付类似若依使用体验的现成后台套件；后台复用 v1 公共能力，不把组织模型和前端依赖放进 core，见 [v2 计划](plans/v2-admin-platform.md) 与 [版本路线](roadmap.md)。当前仍先完成 v1。
- 通用加密与密钥轮换在框架进程内完成，默认不依赖第三方服务；普通路径只需密钥配置及加解密调用，采用当前密钥写入、历史密钥解密，不强制远程密钥中心或复杂算法配置，详见 [通用能力计划](plans/v1-common-capabilities.md)。
- v1 完整通用底座按 [能力覆盖与归属](plans/v1-capability-coverage.md) 交付；常见底层机制由框架整合，每类职责一个入口，不让业务开发者重复搭建，不因复用 Rust 库而省略框架接入与验收。
- 国际化、WebSocket、统一出站 HTTP、通用加密与密钥、审计、请求幂等均纳入 v1，最小范围及待决策建议见 [通用能力计划](plans/v1-common-capabilities.md)；SSE 等其他通用机制按能力覆盖计划补齐，完整实时消息平台另行扩展。
- 性能与开箱即用补强按 [专题计划](plans/v1-performance-and-readiness.md) 执行；其中验收要求与待决策建议分开记录，实施前先确定 contract，不以规划代替完成证据。
- 框架定位：Rust 应用开发的“瑞士军刀”，以高性能为基础，提供开箱即用的常用能力和稳定扩展机制，让开发者直接进入业务开发，减少重复搭建底层设施。性能须以代表性场景实测，不因使用 Rust 就宣称更快。
- 规划名称统一为“v1 版本”，原 `plans/gen1-plan.md` 路径暂保留以兼容引用；当前开发包版本仍以 `Cargo.toml` 为准，发布与 tag 由开发者执行。
- v1 版本包含邮件与短信通信能力；短信真实供应商及公共 API 待选型设计，验证码机制纳入 v1 设计，具体流程与公共契约待决策，薄多渠道通知编排纳入 v1，业务通知中心/UI 归 v2 后台套件，见 [ADR-011](decisions/adr-011-sms-and-communication-baseline.md)。
- 默认实现满足常见需求，公开扩展契约承接差异需求；数据库、缓存等能力复用既有插件与配置机制，具体边界见 [ADR-009](decisions/adr-009-extensible-capability-drivers.md)。缓存 driver/store contract 见 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md)，代码仍待 M5。
- OpenRust 不是任何既有项目的移植，没有兼容包袱；设计以 [v1 版本计划](plans/gen1-plan.md) 与 [框架设计](plans/framework-design.md) 为准。
- 应用内核自研，基于 `tokio` + `axum` + `tower`；Loco 只作参考与对标（[ADR-001](decisions/adr-001-own-kernel-on-axum.md)）。
- v1 版本正式支持 PostgreSQL 14+ 与 MySQL 8.0+；每个数据功能必须有双库证据（[ADR-002](decisions/adr-002-seaorm-dual-database.md)）。
- 同时支持单机与集群；标准应用默认进程内 memory cache，无需外部服务；file 与 Redis 显式选择。需要跨节点共享语义时必须明确配置共享后端，不能静默退化；队列/会话仍默认共享数据库，部署边界见 [ADR-013](decisions/adr-013-deployment-and-redis-cache-default.md)。缓存实现待 M5。
- 认证默认为 Cookie 会话 + 个人访问令牌，JWT 不在 v1 版本（[ADR-004](decisions/adr-004-session-and-token-auth.md)）。
- 许可证 MIT OR Apache-2.0；Edition 2024；MSRV 为 stable 减两个小版本（[ADR-005](decisions/adr-005-license-naming-msrv.md)）。
- 生成项目布局、CLI、配置键与公共 API 命名以 [ADR-006](decisions/adr-006-rust-native-naming.md) 为唯一 contract；M6 才建立 mdBook `docs/`，此前用户文档维护在 crate README。
- 库 crate 禁止 `unsafe`、`unwrap` / `expect` / `panic`；公共项必须有文档。细则见 [conventions.md](conventions.md)。
- 功能范围以计划的「v1 版本能力范围」为准，不在里程碑内追加能力；新需求先进 `roadmap.md`。

## 对外动作（仅开发者执行）

- 在 crates.io 预留 `openrust`、`openrust-cli` 及 `openrust-*` 名称（发布 0.0.0 占位版本）。
- 发布任何版本到 crates.io。
