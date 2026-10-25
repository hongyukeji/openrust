# OpenRust 第一代开发计划

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 是全新的开源 Rust 应用开发框架，第一代交付开箱即用的框架底座，开发者创建项目后直接进入业务开发。本文件定义目标、技术基线、能力范围、黄金路径、里程碑与验收；架构设计见 [framework-design.md](framework-design.md)。

## 目标与边界

- OpenRust 是独立的开源 Rust 应用开发框架，仓库 `https://github.com/hongyukeji/openrust`。
- 第一代交付「框架底座」：项目生成、配置、HTTP、数据库、认证授权、任务、缓存、存储、邮件、CLI、测试工具和部署路径全部可用；业务开发者拿到项目后只写 `controllers` / `models` / `services` / `policies` / `jobs`。
- 设计风格借鉴 Laravel 的约定优于配置与统一开发入口，用 Rust 类型系统和 trait 实现；不复制 PHP 运行期魔法，不复制 Spring 容器与注解装配。
- 核心开发者画像：构建 API 服务与后台系统的中小团队和独立开发者，熟悉 Rust 基础，不想自己拼装 axum / SeaORM / tower 生态。
- OpenRust 不是任何既有项目的移植，没有兼容包袱；所有约定按第一代目标从零设计。
- 质量优先于功能数量：黄金路径稳定、双库证据齐全、公共 API 文档完整是发布条件；业务功能数量不是指标。
- 本文件是待实施方案。进入执行时按功能簇拆入 [`todo.md`](../todo.md)，本文件不充当 WIP 状态板；里程碑完成后把稳定事实迁入 `architecture.md` / `core/*.md` / `docs/` 并删除已完成部分。

## 关键决策

| 领域 | 决策 | 理由 |
| --- | --- | --- |
| 应用内核 | **自有内核，基于 axum + tower + tokio；Loco 只作参考与对标**（[ADR-001](../decisions/adr-001-own-kernel-on-axum.md)） | Loco 1.2 只有 `db-sqlite` / `db-postgres` feature，队列后端为 Redis / Postgres / SQLite，无 MySQL 路径，支持 MySQL 等于长期维护 fork。建立在 Loco 之上，OpenRust 只剩模板层，目录、配置、CLI、认证、错误模型全部由 Loco 决定，无法保证设计一致性与「标准规范、Laravel 式优雅」。Loco 本身基于 axum，OpenRust 直接使用同一底层并复用成熟 crate，自研面集中在应用层约定 |
| ORM | SeaORM 2.x + `sea-orm-migration`；SQLx 直连作为显式逃生口（[ADR-002](../decisions/adr-002-seaorm-dual-database.md)） | 唯一同时覆盖 PostgreSQL / MySQL 的成熟 Rust ORM，带迁移与实体生成 |
| 数据库 | PostgreSQL 14+（推荐默认）与 MySQL 8.0+（同等正式支持）；SQLite 不在第一代 | 两者都支持 `SKIP LOCKED`、CTE、JSON；SQLite 增加第三条验证矩阵但不是目标部署环境 |
| Redis | 可选 driver，不是必需依赖 | 默认队列、缓存、会话分别用数据库表与进程内实现；多实例共享时再启用 Redis |
| 认证 | Cookie 会话（Web）与个人访问令牌（API）两种 Guard 为默认；密码 Argon2id；JWT 不在第一代（[ADR-004](../decisions/adr-004-session-and-token-auth.md)） | 有状态令牌可撤销、可审计；JWT 作为 0.2 候选 `auth-jwt` feature |
| 授权 | `Policy` + 最小 `roles` / `permissions` 模型；DataScope、多租户不在第一代 | 覆盖绝大多数应用的授权需求，不引入特定业务系统的数据范围概念 |
| 队列 | 自有数据库队列（`SKIP LOCKED`），M0 对比 `apalis` 后定稿（[ADR-003](../decisions/adr-003-database-queue-default.md)） | 范围有界（拉取、重试、退避、失败表），自有实现可完全控制 Schema 与双库行为 |
| 配置 | TOML 分层 + 环境变量 + `.env`；类型化结构启动校验 | 与 Rust 生态习惯一致，错误在启动时暴露 |
| 验证 | `validator` derive；M0 评估 `garde` | 生态成熟，与 serde 配合良好 |
| 存储 | OpenDAL；本地默认，S3 兼容为 `storage-s3` feature | 一个适配层覆盖所有对象存储，不自写 SDK 封装 |
| 邮件 | lettre SMTP + log + fake driver | 密码重置与邮箱验证依赖邮件，必须进入第一代 |
| 模板 | minijinja 为可选 `templates` feature，第一代只服务邮件模板 | API 优先；页面渲染脚手架与表单 CSRF 示例列入 0.2 候选，不做 Blade 等价物 |
| 管理后台 / 插件市场 | 不在第一代 | 由 `ServiceProvider` 扩展模型承载第三方包，不做动态加载 |
| 许可证与名称 | MIT OR Apache-2.0；crate 名 `openrust`、`openrust-cli`、`openrust-*`（[ADR-005](../decisions/adr-005-license-naming-msrv.md)） | 2026-10-25 核实 crates.io 上 `openrust` 与 `openrust-core` 未被占用；开发者在 M0 发布 0.0.0 占位版本预留 |
| Rust 版本 | Edition 2024；MSRV 为当前 stable 减两个小版本 | 既用新语法，又给下游留升级窗口 |
| 仓库 | `https://github.com/hongyukeji/openrust`，本地 `/Users/Home/Codes/Rust/openrust`，默认分支 `main` | 独立发布、独立 CI、独立贡献者入口；Agent 记忆在本仓库 `.agents/project/` |

## 第一代能力范围

| 纳入 | 不纳入 |
| --- | --- |
| 项目生成器与模板（双库） | 管理后台 UI 与前端脚手架 |
| 应用内核、Provider、配置、生命周期、优雅停机 | 动态插件加载、插件市场 |
| HTTP 路由约定、中间件栈、验证、Problem Details、OpenAPI | GraphQL、gRPC、WebSocket 封装 |
| 数据库连接池、迁移、Seeder、Factory、事务、双库方言边界 | SQLite、多租户、读写分离 |
| 认证两种 Guard、密码重置、邮箱验证、Policy、角色权限 | OAuth / 社交登录、SSO、DataScope |
| 队列、调度、事件（进程内） | 外部消息总线、工作流引擎 |
| 缓存、存储、邮件及 fake driver | 短信、推送、通知中心 |
| 可观测性：tracing、请求 ID、健康与就绪检查、可选 OTLP / Prometheus | APM 后端、告警 |
| CLI 全集、测试工具、文档站、示例应用、Dockerfile / compose | 云平台一键部署、Kubernetes Operator |

不纳入项若确有需求，先进入 `roadmap.md`，不在第一代里程碑内追加。

## 开箱即用的定义

黄金路径是第一代的验收主线，每个里程碑都以它的通过程度衡量：

```bash
cargo install openrust-cli
openrust new blog --db postgres
cd blog
openrust dev
```

`openrust dev` 完成后：依赖容器（数据库、Mailpit）已启动并就绪，迁移已执行，服务监听 `http://localhost:3000`，`/health` 返回 200，`/docs` 展示 OpenAPI，注册 / 登录 / 密码重置端点可用，`cargo test` 全绿。

```bash
openrust make:model Post title:string body:text published_at:datetime? --controller --policy --test
openrust migrate
openrust make:job PublishPost
openrust queue:work
```

生成的代码无需修改即可编译、通过测试；开发者只补业务规则。部署：`docker build` 产出单二进制镜像，`APP_ENV=production` 下执行 `blog migrate && blog serve`。MySQL 路径以 `--db mysql` 走完同一流程。

## 里程碑

| 里程碑 | 交付 | 验收 |
| --- | --- | --- |
| M0 决策与基线 | ADR：自有内核、ORM 与双库、队列引擎、认证形态；预留 crates.io 名；workspace 骨架、`[workspace.lints]`、CI（fmt / clippy / test / deny / MSRV / 双库服务）；技术 spike：SeaORM 2 双库迁移、`SKIP LOCKED` 队列、axum 提取器组合 | CI 全绿；spike 结论写入 ADR；未通过的技术假设在进入 M1 前替换 |
| M1 内核与 HTTP | `openrust-core`（Application / AppBuilder / ServiceProvider / Config / Error / 生命周期）、`openrust-http`（路由约定、中间件栈、`Validated`、Problem Details、健康检查）、CLI `new` / `dev` / `serve`、无数据库模板 | `openrust new` 生成项目 `cargo test` 通过；`/health` 200；验证失败返回 422 problem+json；缺失配置启动失败并指出键名 |
| M2 数据层 | `openrust-db`（连接池、迁移运行器、Seeder、Factory、事务、`dialect` 边界）、`make:model` / `make:migration` / `make:seeder`、`migrate` / `db:seed`、`openrust-test` 数据库隔离、模板 `--db`、CI 双库矩阵 | 空 PostgreSQL / MySQL 均初始化成功；重复 `migrate` 幂等；`rollback` 可用；生成实体直接编译；每个数据功能有双库测试证据 |
| M3 邮件、认证与授权 | `openrust-mail`（smtp / log / fake）、`openrust-auth`（用户约定、Argon2id、SessionGuard、TokenGuard、`CurrentUser`、注册 / 登录 / 登出 / 密码重置 / 邮箱验证、Policy、角色权限、登录限流）、`user:create`、`make:policy` | 全部认证流程有集成测试；令牌撤销即时生效；错误密码限流可验证；Policy 拒绝返回 403 Problem |
| M4 队列、调度与事件 | `openrust-queue`（database driver、重试退避、`failed_jobs`、`queue:work` / `queue:failed` / `queue:retry`、调度器与锁、`schedule:run`）、`openrust-events`、队列化邮件、`make:job` / `make:event` / `make:listener` / `make:mail` | 任务失败 → 重试 → 失败表 → 手动重试成功；多副本调度不重复；worker 停机不丢任务；两库皆验证 |
| M5 缓存、存储与可观测性 | `openrust-cache`（memory / redis）、`openrust-storage`（local / s3）、JSON 日志、请求 ID、`/ready`、`otel` / `metrics` feature、panic 捕获 | fake driver 测试覆盖；S3 兼容路径用 MinIO 集成验证；`/ready` 依赖超时正确报告 |
| M6 开发者体验与 0.1 发布 | 文档站、`examples/blog` 黄金路径示例、Dockerfile / compose 模板、`route:list` / `config:show`、生成器全集与快照测试、`cargo semver-checks`、新手走查、发布 0.1.0 | 新开发者按文档从空目录到部署在 30 分钟内完成且不读框架源码；本文件验收原则全部达标；发布动作由开发者执行 |

里程碑按依赖顺序推进，每个里程碑结束时黄金路径必须比上一里程碑更完整，不允许跳过验收进入下一阶段。

## 验收原则

- 黄金路径在 PostgreSQL 与 MySQL 上均可按文档走完，不要求开发者手工安装或配置数据库、邮件等依赖，也不要求阅读框架源码。
- 每个首版数据访问、迁移、队列功能都有双库 CI 证据；数据库专属实现显式标注并集中在方言边界。
- 关键边界有集成测试：认证与授权、事务回滚、任务重试与幂等、调度去重、配置缺失时的启动错误、优雅停机。
- 每个公共类型与入口函数有文档与可运行示例；生成器产物在两库模板上有快照测试。
- 错误信息可操作：配置错误给出键名与来源，迁移错误给出文件名，启动失败给出缺失的 provider 或服务。
- 性能基线使用 `examples/blog` 的代表性路径与明确数据规模建立，不以微基准推断整体快慢。
- 依赖升级、迁移回滚、备份恢复和基础设施异常各有一页可操作说明。

## 框架质量门禁

CI 必过项：`cargo fmt --check`、`cargo clippy -D warnings`（含 pedantic）、`cargo test --workspace`、doctest、`cargo deny check`、MSRV 构建、`cargo semver-checks`、双库模板快照测试。库 crate `#![forbid(unsafe_code)]` 与 `#![deny(missing_docs)]`，禁止 `unwrap` / `expect` / `panic`。细则见 [设计文档「框架代码规范」](framework-design.md#框架代码规范)。

## 风险与对策

| 风险 | 对策 |
| --- | --- |
| Rust 编译时间拖累开发体验 | workspace 拆分、feature 最小化、`openrust dev` 增量重编译、模板 `profile.dev` 调优、文档推荐 sccache |
| SeaORM 2.x API 变动 | workspace 锁定版本；常用操作封装在 `openrust-db`，业务代码不直接依赖易变 API |
| 双库语义漂移 | 可移植列类型白名单、`dialect` 模块强制双分支、CI 矩阵、框架表禁用单库特性 |
| 自研范围膨胀 | 以「第一代能力范围」为准；新增能力先进 `roadmap.md`，不在里程碑内追加 |
| 小团队维护多 crate | 第一代允许事件、调度并入 `core` / `queue`；crate 拆分只跟随发布边界 |
| 名称与生态冲突 | 开发者在 M0 预留 crates.io 名；GitHub 仓库已确定 |
| 决策反复 | 关键决策以 ADR 固化；变更需新 ADR 标记替代 |

## 已定稿事项与对外动作

方向性决策已全部定稿并落为 ADR-001 至 ADR-005；实施中只由 M0 spike 证据修订细节，不重开方向讨论。

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
