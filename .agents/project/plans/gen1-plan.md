# OpenRust v1 版本开发计划

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 是全新的开源 Rust 应用开发框架，v1 版本交付开箱即用的框架底座，开发者创建项目后直接进入业务开发。本文件定义目标、技术基线、能力范围、黄金路径、里程碑与验收；架构设计见 [framework-design.md](framework-design.md)。

## 目标与边界

OpenRust 定位为 Rust 应用开发的“瑞士军刀”：以高性能为基础，提供开箱即用的常用能力，并通过清晰、稳定的扩展机制承接不同场景的需求，让开发者快速进入业务开发，无需反复搭建底层基础设施。简单需求容易实现，复杂需求有路可走。

规划统一称为“v1 版本”；本文件沿用 `gen1-plan.md` 路径，当前包版本以 `Cargo.toml` 为准。本计划不代表已发布，不执行版本、tag 或发布操作。

- OpenRust 是独立的开源 Rust 应用开发框架，仓库 `https://github.com/hongyukeji/openrust`。
- v1 版本交付「框架底座」：项目生成、配置、HTTP、数据库、认证授权、任务、缓存、存储、邮件、短信、国际化、WebSocket、出站 HTTP、加密、审计、请求幂等、CLI、测试工具和部署路径全部可用；业务开发者拿到项目后只写 `controllers` / `models` / `services` / `policies` / `jobs`。
- 设计风格采用 Rust 生态熟悉的类型、trait 与显式依赖，以可见的项目约定提供统一开发入口。
- 核心开发者画像：构建 API 服务与后台系统的中小团队和独立开发者，熟悉 Rust 基础，不想自己拼装 axum / SeaORM / tower 生态。
- OpenRust 不是任何既有项目的移植，没有兼容包袱；所有约定按 v1 版本目标从零设计。
- 质量优先于功能数量：黄金路径稳定、双库证据齐全、公共 API 文档完整是发布条件；业务功能数量不是指标。
- 本文件是待实施方案。进入执行时按功能簇拆入 [`todo.md`](../todo.md)，本文件不充当 WIP 状态板；里程碑完成后把稳定事实迁入 `architecture.md` / `core/*.md` / `docs/` 并删除已完成部分。

## 关键决策

| 领域 | 决策 | 理由 |
| --- | --- | --- |
| 应用内核 | **自有内核，基于 axum + tower + tokio；参考框架综合对标**（[ADR-001](../decisions/adr-001-own-kernel-on-axum.md)） | Loco、Laravel、Goravel、OpenSpring、RuoYi 若依、芋道按[框架参考与对标](framework-design.md#框架参考与对标)的统一维度综合评估；各项目适用领域影响证据权重，但不设单一优先对象。OpenRust 不依赖或完整复制任何一个；MySQL/PostgreSQL 双库、Rust API 惯例与自有公共契约由 OpenRust 自行验证 |
| ORM | SeaORM 2.x + `sea-orm-migration`；SQLx 直连作为显式逃生口（[ADR-002](../decisions/adr-002-seaorm-dual-database.md)） | 唯一同时覆盖 PostgreSQL / MySQL 的成熟 Rust ORM，带迁移与实体生成 |
| 数据库 | PostgreSQL 与 MySQL 同等正式支持；SQLite 不在 v1 版本 | 暂定最低支持 PostgreSQL 14+ / MySQL 8.0+ / Redis 6.2+；日常开发和 Compose 使用 PostgreSQL 18 / MySQL 8.4 LTS / Redis 8；常规 CI 使用 PostgreSQL 18 / MySQL 8.4，Redis 能力在 M5 落地后加入集成服务。运行时数据库版本检查已与暂定下限对齐。最低版本整体兼容性及 Redis 6.2+ 须在 v1 发布前按真实能力路径验证；只在跨度较大、涉及版本专属行为或存在明确兼容风险时增加其他专项验证 |
| 缓存 | 标准应用默认 Redis，memory/file 显式选择，库层 Redis feature 可选 | 单机与集群共用默认缓存方案；共享/原子语义仍按契约校验，默认值见 ADR-015；队列/会话仍默认数据库，见 ADR-013/ADR-014 |
| 认证 | 前后端分离 API 默认使用 Bearer JWT access token + 可轮换 refresh token；通用 Cookie Session 显式可选；PAT 面向自动化集成；密码 Argon2id（[ADR-018](../decisions/adr-018-jwt-primary-authentication.md)） | 短期 access token 限制撤销窗口；需立即撤销时使用共享状态，集群不能依赖进程内 denylist |
| 授权 | `Policy` + 最小 `roles` / `permissions` 模型；DataScope、多租户不在 v1 版本 | 覆盖绝大多数应用的授权需求，不引入特定业务系统的数据范围概念 |
| 队列 | 自有数据库队列（`SKIP LOCKED`），M0 对比 `apalis` 后定稿（[ADR-003](../decisions/adr-003-database-queue-default.md)） | 范围有界（拉取、重试、退避、失败表），自有实现可完全控制 Schema 与双库行为 |
| 配置 | TOML 分层 + 环境变量 + `.env`；类型化结构启动校验 | 与 Rust 生态习惯一致，错误在启动时暴露 |
| 验证 | `validator` derive；M0 评估 `garde` | 生态成熟，与 serde 配合良好 |
| 存储 | OpenDAL；本地默认，S3 兼容为 `storage-s3` feature | 一个适配层覆盖所有对象存储，不自写 SDK 封装 |
| 邮件 | lettre SMTP + log + fake driver | 密码重置与邮箱验证依赖邮件，必须进入 v1 版本 |
| 短信 | 统一 `SmsSender`/`SmsDriver`、Twilio Messages REST、log / fake driver；真实账户发送需授权验收 | contract 与发送边界见 [ADR-033](../decisions/adr-033-sms-driver-and-template-contract.md) |
| 模板 | 邮件通过 `LocalizedEmail` 与 i18n catalog 渲染；SMS 支持纯文本或供应商 template SID | 不新增第二套模板语言；页面渲染脚手架与表单 CSRF 示例列入 v2 评估候选 |
| 管理后台 / 插件市场 | 不在 v1 版本 | 由 `Plugin` 扩展模型承载第三方包，不做动态加载 |
| 许可证与名称 | MIT OR Apache-2.0；crate 名 `openrust`、`openrust-cli`、`openrust-*`（[ADR-005](../decisions/adr-005-license-naming-msrv.md)） | 2026-10-25 核实 crates.io 上 `openrust` 与 `openrust-core` 未被占用；开发者在 M0 发布 0.0.0 占位版本预留 |
| Rust 版本 | Edition 2024；MSRV 为当前 stable 减两个小版本 | 既用新语法，又给下游留升级窗口 |
| 仓库 | `https://github.com/hongyukeji/openrust`，本地 `/Users/Home/Codes/Rust/openrust`，默认分支 `main` | 独立发布、独立 CI、独立贡献者入口；Agent 记忆在本仓库 `.agents/project/` |

## v1 版本能力范围

| 纳入 | 不纳入 |
| --- | --- |
| 项目生成器与模板（双库） | 管理后台 UI 与前端脚手架 |
| 应用内核、Plugin、配置、生命周期、优雅停机 | 动态插件加载、插件市场 |
| HTTP 路由约定、中间件栈、验证、统一响应包装、OpenAPI、WebSocket、SSE | GraphQL、gRPC |
| 数据库连接池、迁移、`src/seed.rs`、`fake::Dummy` 测试数据、事务、双库方言边界、可选基础读写分离 | SQLite、多租户、分库分表与跨库事务 |
| JWT access/refresh token、可选 Cookie 会话、可选 PAT、密码重置、邮箱验证、Policy、角色权限 | OAuth / 社交登录、SSO、DataScope |
| 队列、常用任务链/批处理、调度、事件（进程内） | 外部消息总线、任意 DAG 工作流引擎 |
| 缓存与锁、存储与上传下载、邮件、短信、薄通知编排及 fake driver | 移动推送供应商、业务通知中心/UI |
| 国际化、出站 HTTP、加密与密钥管理、审计、请求幂等 | 后台业务 UI、完整聊天/消息平台 |
| 可观测性：tracing、请求 ID、健康与就绪检查、可选 OTLP / Prometheus | APM 后端、告警 |
| CLI 全集、测试工具、文档站、示例应用、Dockerfile / compose | 云平台一键部署、Kubernetes Operator |

不纳入项若确有需求，先进入 `roadmap.md`，不在 v1 版本里程碑内追加。

## 已确认的设计要求与评估项

| 要求 | 执行位置与验收边界 |
| --- | --- |
| 聚合启动入口 | `apps/server` 作为仓库开发与集成验证宿主，使用门面组装已实现能力；具体状态见 [server 事实](../core/server.md) 与 ADR-007 |
| 统一入口与第三方扩展 | 按 [ADR-009](../decisions/adr-009-extensible-capability-drivers.md) 提供驱动契约、注册、配置、错误语义、示例与契约测试；用独立适配示例检查第三方是否需要修改框架 |
| 基础读写分离 | v1 数据层增补已实现：默认单库、显式只读副本轮询、事务固定主库及 task-local 主库 scope；故障不自动回退。PostgreSQL 18.6 / MySQL 8.4.13 真实复制验收见 [专题计划](v1-database-read-write-splitting.md)；完整集群另行验收 |
| 数据库可扩展 | M2 集中后端及方言差异；v1 正式交付仍为 PostgreSQL/MySQL。明确 SeaORM 边界，不承诺任意数据库或数据模型直接替换 |
| 缓存可替换 | M5 交付统一 `Cache` 与 default/named store、可注册 driver；标准应用默认 Redis（ADR-015），memory/file 显式选择，库层 Redis feature 可选；契约测试验证 TTL、序列化、错误及原子能力边界。标准应用开发编排按默认配置启用 Redis |
| 邮件与短信 | M3 提供发送、模板、供应商扩展与本地/测试路径；M4 接入队列化发送；真实适配和验收按 [短信决策](../decisions/adr-011-sms-and-communication-baseline.md) |
| 高性能且按需启用 | 能力通过 feature 按需组合；在明确硬件、并发、数据规模及依赖环境下记录吞吐、延迟分位数与资源占用，评估中间件和驱动开销，不以微基准推断整体性能 |
| 完整通用底座 | 多实例使用、上传下载、通用 Session/限流、签名、API 输出、查询体验与常用任务编排均按覆盖计划交付；具体 API/命名与默认值待决策 |

Laravel 等成熟框架用于能力与体验对照，API 与命名遵循 Rust 惯例；v1 不承诺复制其完整生态。数据库/Rust 支持政策、现有实现盘点等说明性讨论不另算新增功能。

性能、开发编排、异步一致性、生产边界及验收补强见 [v1 补强计划](v1-performance-and-readiness.md)。该计划纳入验收补强，并提供待决策方案；具体公共 API 与默认值须先定 contract。验证码升级为 v1 设计项，是否启用短信登录仍需独立评估。

国际化、统一出站 HTTP、应用数据加密与本地密钥轮换已分别按 ADR-032、ADR-031、ADR-034 实现并独立验证；审计按 ADR-035、签名链接按 ADR-036 提供显式入口并经双库验证；WebSocket、请求幂等仍按 [通用能力补齐计划](v1-common-capabilities.md) 推进。具体 API/依赖选型先定 contract；不将底层框架原生支持视为 OpenRust 已完成接入。

通用底座完整性以 [能力覆盖与归属](v1-capability-coverage.md) 为准：补齐常见开发机制，既有能力在原入口完善，新增机制明确唯一归属；不将“已有底层库”或“待设计”作为省略交付的理由。

版本分工见 [v2 后台计划](v2-admin-platform.md)：v1 提供完整通用机制，v2 提供用户/组织/菜单等现成模块与 UI；本文件“不纳入”表示不属 v1，v2 核心与候选状态以对应计划为准。v2 不复制 v1 实现。

集群部署为 v1 硬性要求，详见 [集群与多副本计划](v1-cluster-deployment.md)：多节点共享身份/配额/幂等状态、任务竞争、跨节点实时投递、共享文件及滚动/故障恢复必须实际验证。

## 开箱即用的定义

黄金路径是 v1 版本的验收主线，每个里程碑都以它的通过程度衡量：

```bash
cargo install openrust-cli
openrust new blog --db postgres
cd blog
openrust dev
```

`openrust new` 默认生成注册与登录、一个 scaffold 资源、一封邮件、一个 job 和请求测试。`openrust dev` 一条命令按项目配置启动所需依赖容器（标准应用为数据库、Redis、Mailpit），执行迁移并运行服务；显式选择 memory/file 时可不启动 Redis。`/health` 返回 200，`/docs` 展示 OpenAPI，注册 / 登录可用，`cargo test` 全绿。

```bash
openrust generate scaffold post title:string body:text published_at:date_time? author:references
openrust migrate
openrust g job PublishPost
openrust queue work
```

生成的代码无需修改即可编译、通过测试；字段 DSL 采用小写类型名，`?` 表示可空，`references` 表示外键。开发者只补业务规则。部署：`docker build` 产出单二进制镜像，`APP_ENV=production`（目标命名见 ADR-016，代码迁移待实现） 下执行 `blog migrate && blog serve`。MySQL 路径以 `--db mysql` 走完同一流程。

## 里程碑

| 里程碑 | 交付 | 验收 |
| --- | --- | --- |
| M0 决策与基线 | ADR：自有内核、ORM 与双库、队列引擎、认证形态；预留 crates.io 名；workspace 骨架、`[workspace.lints]`、CI（fmt / clippy / test / deny / MSRV / 双库服务）；技术 spike：SeaORM 2 双库迁移、`SKIP LOCKED` 队列、axum 提取器组合 | CI 全绿；spike 结论写入 ADR；未通过的技术假设在进入 M1 前替换 |
| M1 内核与 HTTP | `openrust::core`（Application / AppBuilder / Plugin / Config / Error / 生命周期）、`openrust::http`（路由约定、中间件栈、`Validated`、统一响应包装、健康检查）、CLI `new` / `dev` / `serve`、无数据库模板 | `openrust new` 生成项目 `cargo test` 通过；`/health` 200；验证失败返回 422 统一 JSON 包装；缺失配置启动失败并指出键名 |
| M2 数据层 | `openrust::db`（连接池、迁移运行器、seed、事务、`dialect` 边界）、`generate model` / `generate migration`、`migrate` / `db seed`、`openrust-test` 数据库隔离、模板 `--db`、CI 双库矩阵 | 空 PostgreSQL / MySQL 均初始化成功；重复 `migrate` 幂等；`rollback` 可用；生成实体直接编译；每个数据功能有双库测试证据 |
| M3 邮件、短信、认证与授权 | `openrust::sms`（log / fake / Twilio REST，contract 见 ADR-033；真实供应商实发需授权）、`openrust::mail`（SMTP / log / fake / 本地 Mailpit）、`openrust::auth`（用户约定、Argon2id、JWT access/refresh、可选 Session 与 PAT、`CurrentUser`、注册 / 登录 / 登出 / 密码重置 / 邮箱验证、Policy、角色权限、登录限流）、`openrust::audit`（ADR-035）、`openrust::signing`（ADR-036）、`generate mailer` / `generate policy`，国际化、出站 HTTP、加密与密钥 | Twilio 协议、log/fake 与 Mailpit SMTP 验证；认证/签名/审计双库测试及新生成项目 HTTP 流程通过。真实供应商短信实发需授权账号与目标；跨 M3/M4/M5 业务流程扩展点按第六项依赖顺序实施 |
| M4 队列、调度与事件 | `openrust::queue`（database driver、重试退避、`failed_jobs`、`queue work`、调度器与锁、`schedule run` / `schedule list`）、`openrust::events`、队列化邮件与短信、`generate job`，请求幂等、事务一致性、锁、常用任务编排与薄通知层（按专题计划） | 任务失败 → 重试 → 失败表 → 手动重试成功；多副本调度不重复；worker 停机不丢任务；两库皆验证 |
| M5 缓存、存储与可观测性 | `openrust::cache`（标准应用默认 Redis、显式 memory/file、库层 Redis feature 可选；driver/store contract 见 ADR-014）、`openrust::storage`（local / s3）、JSON 日志、请求 ID、`/ready`、`otel` / `metrics` feature、panic 捕获、WebSocket/SSE 与上传下载（按专题计划） | 三驱动基础契约测试；file 临时目录验证；Redis 真实集成验证；扩展示例只用公开契约；S3 兼容路径用 MinIO 集成验证；`/ready` 依赖超时正确报告 |
| M6 开发者体验与 v1 发布 | mdBook 文档站（Diátaxis、迁移对照页、`coming-from-axum`）、`examples/blog` 黄金路径示例、Dockerfile / compose 模板、`routes` / `config show` / 只读 `doctor` / `db status`、`generate` / `g`（`scaffold`、`controller`、`model`、`migration`、`job`、`mailer`、`policy`、`command`）与快照测试、`openrust-test` `TestApp` / HTTP response assertions、`cargo semver-checks`、新手走查、v1 正式发布 | 新开发者按文档从空目录到部署在 30 分钟内完成且不读框架源码；本文件验收原则全部达标；发布动作由开发者执行 |

当前暂定最低支持 PostgreSQL 14+、MySQL 8.0+、Redis 6.2+；日常开发基线采用 PostgreSQL 18、MySQL 8.4 LTS、Redis 8。v1 发布验收重新核实届时官方生命周期，并在最低版本真实服务上验证框架实际承诺的能力路径；Redis 6.2+ 的真实缓存/共享语义验证须待 M5 driver 实现后完成。只有主版本跨度较大或存在明确兼容风险时，才增加其他版本专项测试。版本升级时重新核实并同步运行时门槛、CI/Compose 镜像、模板、文档与兼容性说明。PostgreSQL 官方版本政策不使用“LTS”称谓，Redis 官方使用“Extended Support”类别。

里程碑按依赖顺序推进，每个里程碑结束时黄金路径必须比上一里程碑更完整，不允许跳过验收进入下一阶段。

## 验收原则

- 集群按专题计划在 PostgreSQL/MySQL 上完成至少两个 API 和多 worker/scheduler 的共享状态、跨节点实时通信及滚动/故障恢复验收；不能以单机测试或黏性会话代替。
- 黄金路径在 PostgreSQL 与 MySQL 上均可按文档走完，不要求开发者手工安装或配置数据库、邮件等依赖，也不要求阅读框架源码。
- 每个首版数据访问、迁移、队列功能都有双库 CI 证据；数据库专属实现显式标注并集中在方言边界。
- 关键边界有集成测试：认证与授权、事务回滚、任务重试与幂等、调度去重、配置缺失时的启动错误、优雅停机。
- 每个公共类型与入口函数有文档与可运行示例；生成器产物在两库模板上有快照测试。
- 错误信息可操作：配置错误给出键名与来源，迁移错误给出文件名，启动失败给出缺失的 plugin 或服务。
- 性能基线使用 `examples/blog` 的代表性路径与明确数据规模建立，记录吞吐、p50/p95/p99 延迟、CPU 与内存，以及环境、构建模式和测量方法；覆盖基础 HTTP、数据库访问、认证及队列场景，发布前据基线确定性能回归阈值，不预先承诺未经测量的数值。
- 依赖升级、迁移回滚、备份恢复和基础设施异常各有一页可操作说明。

## 框架质量门禁

CI 必过项：`cargo fmt --check`、`cargo clippy -D warnings`（含 pedantic）、`cargo test --workspace`、doctest、`cargo deny check`、MSRV 构建、`cargo semver-checks`、双库模板快照测试。库 crate `#![forbid(unsafe_code)]` 与 `#![deny(missing_docs)]`，禁止 `unwrap` / `expect` / `panic`。细则见 [设计文档「框架代码规范」](framework-design.md#框架代码规范)。

## 风险与对策

| 风险 | 对策 |
| --- | --- |
| Rust 编译时间拖累开发体验 | workspace 拆分、feature 最小化、`openrust dev` 增量重编译、模板 `profile.dev` 调优、文档推荐 sccache |
| SeaORM 2.x API 变动 | workspace 锁定版本；常用操作封装在 `openrust::db`，业务代码不直接依赖易变 API |
| 双库语义漂移 | 可移植列类型白名单、`dialect` 模块强制双分支、CI 矩阵、框架表禁用单库特性 |
| 自研范围膨胀 | 以「v1 版本能力范围」为准；新增能力先进 `roadmap.md`，不在里程碑内追加 |
| 小团队维护多 crate | v1 版本允许事件、调度并入 `core` / `queue`；crate 拆分只跟随发布边界 |
| 名称与生态冲突 | 开发者在 M0 预留 crates.io 名；GitHub 仓库已确定 |
| 决策反复 | 关键决策以 ADR 固化；变更需新 ADR 标记替代 |

## 已定稿事项与对外动作

基础方向以既有 ADR 为准；新增需求与补强计划中的待决策事项由负责设计的 Agent 基于现有契约和验证证据形成决策，不把建议直接视为已定公共 API。

需要开发者执行的对外动作：创建 GitHub 仓库并推送 `main`；安装本机 Rust stable 工具链；在 crates.io 发布 0.0.0 占位版本预留 `openrust`、`openrust-cli` 与 `openrust-*` 名称；之后每次正式发布。

## AI 协作与模型分工

- **方案与计划：Claude Code + `anthropic/claude-opus-5.5`，通过 OpenRouter API 按量付费。** 用于梳理目标与约束、形成阶段计划、比较技术方案、定义验收条件，并根据实现证据修订后续阶段。复杂架构方案优先使用 Opus 5.5；需要长程编码或知识工作能力时再评估 `anthropic/claude-fable-5.1`，按使用时的 OpenRouter 价格与任务效果决定，不假设 Fable 更省费用。
- **代码实现与验证：Codex + GPT-6.1 Sol，使用开发者现有的 20X 会员订阅。** Codex 按已确认的里程碑逐个功能簇实施，执行匹配的构建、测试和 smoke，并回报实际改动、命令结果及未解决项。模型以 Codex 当前可用选项为准；若 GPT-6.1 Sol 不可选，使用当前可用的最新 GPT-6 Sol 系列模型，不为此改用 OpenRouter API 计费。
- **交接格式：** 每个待实施功能簇都应有目标、背景、范围、依赖、关键决策、验收命令／观察点、完成条件和明确不做事项。Claude 负责维护本计划与设计文档中的长期方向与阶段边界；进入执行时，当前可闭环功能簇再按项目约定进入 `.agents/project/todo.md`。
- **执行反馈：** Codex 完成一个功能簇后，将变更摘要、验证证据和偏差交回 Claude Code；Claude 据此更新未执行里程碑与设计文档。不得把尚未验证的推测写成已完成事实，也不得以计划文本代替代码验证。
- **密钥与费用：** OpenRouter 密钥只通过 Claude Code 所需的环境变量或本机凭据管理提供，不写入仓库、计划文件、命令输出或提交记录。Claude API 的模型、路由和费用按实际 OpenRouter 账单核对；Codex 实施调用使用既有 20X 订阅额度。

## 相关

- [OpenRust 框架设计](framework-design.md)
- [架构决策](../decisions/)
- [开发计划索引](README.md)
