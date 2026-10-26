# 活跃待办

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## 框架底层调整总览（领取任务前必读）

以下是 v1 已承诺、尚未完整实现或验收的底层工作。它们分属不同功能簇和里程碑，不能当成一次性目录重构，也不能因为 M0-M2 基线完成就视为已交付。每次切换任务先核对本表及对应专题计划；专题计划是范围和验收标准的详细来源。

1. **M2 数据库读写分离已实现并验证**：`Db` 提供显式只读副本路由与 task-local primary scope；PostgreSQL 18 / MySQL 8.4 真实复制暂停/追平及双 App 隔离测试通过。指标接入和完整业务负载性能测试归入通用可观测性与性能计划，不表示完整集群验收完成。
2. **按 M3/M4/M5/M6 依赖顺序分批补齐其他底层机制**：M3 已完成请求级国际化、统一出站 HTTP、邮件/SMS 驱动、进程内加密与轮换、审计及签名链接；继续完成认证 HTTP 总验收、真实 SMS 外部条件和通用流程扩展点；M4 前确定 worker/scheduler 安全停机、取消和租约恢复契约；M5 实现 Redis 默认缓存与 memory/file、命名 store、WebSocket/SSE、存储和可观测性；M6 补齐 CLI、doctor、TestApp 与完整用户文档。
3. **集群是跨里程碑的 v1 强制验收项**：共享会话/限流/缓存、队列与调度竞争、跨节点实时路由、共享文件、故障恢复和滚动升级须在对应能力实现时逐步覆盖，最后按 [集群计划](plans/v1-cluster-deployment.md) 做 PostgreSQL/MySQL 多副本验收；单机测试或多个互不通信的进程不算集群验收。

当前任务顺序及阻塞以本文件各功能簇条目和 [`plan.md` 执行校准](plan.md#执行校准) 为准。遇到测试服务不可用时，明确记录缺少的验收证据；可继续推进不依赖该服务的独立工作，但不得越过明确的前置门槛或把未验收能力宣称为完成。

## 后续里程碑

- OpenAPI：后续里程碑用 `utoipa` 生成 schema，并在非生产环境挂载 `/docs`（ADR-008 §6）。

## 已完成：模块化配置与 `.env`

- 参考 Loco `src/config/{auth,database,logger,mailer,server}.rs` 的按能力拆分思路，以及 Laravel 的 `.env` 示例与进程环境覆盖习惯；OpenRust 采用 Rust typed config，不照搬 PHP facade、运行时任意字符串读取或 config cache。
- 已将当前 `src/config/mod.rs` 的框架配置定义拆到有真实配置的子模块；认证、数据库、邮件配置类型集中到 `src/config/`，通过旧模块 re-export 保持现有公共路径。未为尚未实现的 cache/queue/storage 创建空配置模块。
- 保留应用根 `config/default.toml`、环境 TOML 的 Figment 分层；`.env` / `.env.example` 承载本地敏感值与常见覆盖项，进程环境变量优先，production 不加载 `.env`。不改变 ADR-016 的 `APP_` 与命名空间 contract。
- Spec 与取舍记入 [ADR-030](decisions/adr-030-modular-configuration.md)。验证覆盖配置来源优先级、环境选择、敏感值脱敏、`.env` 生产隔离、生成项目变量和旧 API re-export；通过证据见本次提交及 `memory/verification.md`。

## 已完成的底层调整

- 根 `openrust` 模块迁移已独立提交；M2 读写分离已按 [ADR-029](decisions/adr-029-database-read-routing.md) 实现并通过 PostgreSQL 18 / MySQL 8.4 真实复制拓扑验证。
- 根模块迁移依据 [迁移 Spec](plans/module-layout-migration-spec.md)；既有 M3 WIP 的双库数据库测试与 PostgreSQL/MySQL 生成项目 HTTP e2e 已在迁移后复验通过。M3 认证实现继续保留为独立待办，不计入迁移成果。
- PostgreSQL 18 / MySQL 8.4 测试服务使用现有 `compose.ci.yaml`。当前根 tests/examples 与 server 验收职责映射见迁移 Spec；由于聚合启动、生成项目与双库验收仍由 `apps/server` 承担，继续保留该 package。

## M3 邮件、认证与授权（当前优先功能簇）

- M3 通信框架能力已实现：Mailer/SmsSender 独立公开驱动、log/fake、SMTP/Twilio adapter、显式 locale 与 CLI `--sms-twilio`；本机 Mailpit STARTTLS 限制范围内的明文 SMTP 已实测。Twilio REST 由 transport 契约测试覆盖，尚缺已授权账号凭据与真实目标的实发证据；不得把协议测试写成短信送达验收。复用 ADR-031 出站 HTTP、ADR-032 国际化，M3 外部短信验收仍需已授权目标与凭据。
- M3 应用数据加密/本地密钥轮换已按 [ADR-034](decisions/adr-034-application-encryption-and-key-rotation.md) 独立提交 `e5e86f2`；实现 crate 审计状态作为 v1 发布前复核项。
- M3 签名与限时链接按 [ADR-036](decisions/adr-036-signed-links.md) 完成实现：用途隔离 key ring、purpose/subject 绑定、TTL 验证、共享数据库逐链接撤销与有界过期清理；`--auth` 模板注册 plugin/迁移。PostgreSQL 18/MySQL 8.4 聚合双库和新生成项目 HTTP 流程通过；workspace 与 deny 门禁均通过。
- 下一功能簇：完成 M3 剩余认证与通用安全总验收，逐项核对 AuthFlows、JWT/refresh、Session、PAT、验证码/限流、Policy/RBAC、可信代理和生成项目的完整 HTTP 矩阵；真实 Twilio 发送仅在获得授权凭据与目标后验收。

- ADR-021 根 package/source 迁移已完成实现、验证和独立本地提交；M3 代码仅做必要模块路径适配并保留功能 WIP。ADR-019 的逐 crate 目录改名与 `apps/server` 改名不执行。迁移后的历史 PostgreSQL 16 / MySQL 8.4 双库测试与生成项目 HTTP e2e 均复验通过；当前日常基线以 PostgreSQL 18 / MySQL 8.4 为准。
- 当前功能簇：完成 [ADR-012](decisions/adr-012-auth-current-user-contract.md) 的业务用户 ID 映射、路由认证配置和身份注入闭环。运行时 WIP 已加入 `AuthLayer`、身份提取器、默认数据库认证器与 `AuthMigration`；CLI 已支持 `openrust new --db postgres|mysql --auth`，生成数据库认证插件、迁移和受保护的身份示例路由。
- 当前 WIP 已按 [ADR-017](decisions/adr-017-generic-session-runtime.md) 增加通用 `SessionPlugin`、SeaORM 双库 store 与认证/CSRF 共用路径；其 store、错误包装和跨实例撤销已有 PostgreSQL/MySQL 测试。不要重做这部分。
- [ADR-022](decisions/adr-022-jwt-runtime-contract.md) 已完成独立 contract review；当前实现含 `JwtConfig`、密钥环、HS256 签发/验证及 JWT 路由认证，并接入 facade、`apps/server` 测试和 `--auth` 默认模板。认证双库增量测试、PostgreSQL/MySQL 生成项目 check/test 已验证；本簇剩余项见下文。
- `AuthMigration` 与认证 Session/PAT/refresh 查询已接入 ADR-016 表名解析 contract；PostgreSQL/MySQL 真实隔离库覆盖默认、空前缀、自定义前缀与完整 users 映射。生成项目最终验收仍按认证流程完成条件执行。
- Refresh token 持久化已实现为 `RefreshTokens` 与追加迁移 `AuthRefreshTokenMigration`，并接入 `AuthPlugin` 应用状态；已覆盖哈希存储、单次轮换、固定 family expiry、重放整组撤销、账户级撤销、账户行锁并发序列化。历史 PostgreSQL 14/MySQL 8.0 真实测试通过；AuthFlows 已接入登录签发与 HTTP refresh 轮换。
- AuthFlows service/plugin 与 `--auth` HTTP 路由模板已实现：注册、邮箱验证码申请/验证、登录、refresh、登出、找回/重置；敏感操作经共享数据库 RateLimiter，key 由 endpoint、身份和 `ClientIp` 组成，转发地址仅在 socket peer 命中可信代理配置时采用。服务层 PG16/MySQL8.4 隔离库全流程通过。新增真实 TCP HTTP + `ClientIp` + `Inject` + fake mail 测试，覆盖验证码、登录/JWT、refresh 重放、密码重置、登出撤销、统一 JSON 与限流错误；MySQL 8.4.11 全流程通过。生成项目测试未配置 `OPENRUST_TEST_DATABASE_URL` 时显式跳过。验证码消费与账户更新分开；密码更新与 refresh family revoke 已合并在同一 DB 事务。
- 可信代理 client-IP contract 见 [ADR-028](decisions/adr-028-trusted-proxy-client-ip.md)：`ServerConfig.trusted_proxy_ranges` 默认空，`ClientIp` 仅信任 TCP peer 匹配配置的代理、XFF 右向左剥可信 hop，非可信 peer 忽略转发头，malformed 链拒绝；HTTP module 5 项单测及 MySQL 生成 HTTP 测试通过。
- Policy/basic RBAC 与迁移已按 [ADR-027](decisions/adr-027-policy-and-basic-rbac.md) 实现；PostgreSQL 16 / MySQL 8.4 真实集成验证了授权拒绝、授予/撤销传播和 PAT ability 交集。
- Session 可选认证登录 helper 已按 [ADR-017](decisions/adr-017-generic-session-runtime.md#认证登录-helper-contract) 实现：`login_session` 复用登录校验/限流，轮换 ID，保存用户 ID 与 CSRF digest，返回一次性 CSRF token；`logout_session` 清理认证键后轮换 ID并保留应用数据。AuthFlows 单测通过；Session helper 与注册错误分类已在 PostgreSQL 16 / MySQL 8.4 的真实隔离库测试通过。
- 注册已修复为只将 SeaORM 分类出的唯一约束冲突映射到重复邮箱 `None`，不再使用 MySQL `INSERT IGNORE` 吞掉其他写入错误；双库集成用 CHECK 约束验证错误传播，该行为已在 PostgreSQL 16 / MySQL 8.4 真实隔离库测试通过。
- 模块迁移后已重跑 AuthFlows 双库隔离库测试，并基于根 package 的 `--auth` 生成项目执行 PostgreSQL/MySQL `cargo test` 真实 HTTP 流程；完整 M3 汇总验收仍需与后续正常开发任务一起闭环。自定义认证器只验证扩展路径，不替代默认实现验收。
- PAT 生命周期 API 已实现 `PersonalAccessTokens` 服务并注册到 `AuthPlugin` state：一次性 256-bit 随机 bearer 值、仅 SHA-256 落库、列举不暴露 digest、单个/账户级撤销按账户隔离。历史 PostgreSQL 14 / MySQL 8.0 真实测试覆盖签发/哈希/能力/到期输入/归属隔离与跨应用实例撤销；当前 AuthFlows 和 Policy 已接入 JWT、refresh、PAT 及基本角色权限路径。
- `--auth` 生成物注册 JWT/Auth/AuthFlows plugins，提供 JWT 保护的身份示例和认证流程 HTTP 路由；不能把模板生成、服务层测试或项目编译通过视为 HTTP 端到端流程完成。
- M3 生成认证项目验收修复已完成：CLI 生成的包名断言不再把项目名内嵌进易变排版的代码行。PG18 与 MySQL 8.4 新生成项目均通过 `cargo fmt --all --check`、`cargo check` 和真实数据库 HTTP 流程；本项 workspace fmt/clippy/test/doc/deny 通过。整体 M3 仍有邮件/短信、通用安全能力和真实供应商验收待完成。
- 本簇验收前按 [执行校准](plan.md#执行校准) 核对契约：共享会话与跨节点撤销、认证所需全局限流/验证码边界、注册/登录公开扩展点及 `Inject` 共存；后续 M3 任务按完整 v1 覆盖清单逐簇领取，不以身份注入完成代替整个 M3。
- ADR-012 定义身份/路由 contract；授权 API 与角色/权限跨节点变更传播已由 ADR-027 定义为共享数据库实时读取。该实现不替代本簇端到端验收。

## M4 队列生命周期契约（M4 实施前）

- 按 [ADR-023](decisions/adr-023-loco-application-experience-evaluation.md)，在实现 worker/scheduler 前，基于既有 `Plugin`/graceful shutdown 流程确定是否需要异步插件 shutdown 扩展，并定义顺序、错误聚合、超时、取消、join、租约释放与节点故障恢复。不得新增并列 `Hooks` 系统，也不能把 HTTP 连接 drain 等同于后台任务已安全停止。

## M5 缓存驱动与 store（M5 领取）

- 按 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md) 实现 `openrust::cache` 与 `CachePlugin`：标准应用默认 Redis、显式 memory/file、库层 Redis feature 可选（默认值按 ADR-015）；default/named stores、custom driver 注册、统一序列化/TTL/错误与原子能力边界。
- 标准生成项目显式启用 `cache-redis` 并配置 Redis default store；开发编排按配置提供 Redis，缺失配置/feature 或服务故障不得隐式回退 memory；Redis 配置读取 `REDIS_URL`，同键所选应用命名空间（默认 `APP_`）变量优先，按 ADR-016 迁移。共享缓存用途在集群配置中必须选择可用共享 driver，禁止静默退化。
- 验收：统一基础契约测试；file 临时目录安全路径、过期、原子替换、并发及 I/O 错误；Redis 真实集成；默认/file/redis feature 组合；`apps/server` 接入；独立公开扩展示例与 README/core 事实。

## M6 CLI 与测试体验（后续里程碑）

- 按 [ADR-023](decisions/adr-023-loco-application-experience-evaluation.md) 完成只读 `doctor`，复用配置、数据库版本/连接与迁移检查；不得隐式迁移或改写数据。`config show` 继续是脱敏配置查看入口。
- 在 M3/M4 对应能力 contract 和模块化迁移完成后，补齐稳定的 `openrust-test` `TestApp` / HTTP response assertions：复用 `finalize`、`build_router`、Tower oneshot、现有 `TestDatabase` 与能力 fake；不启动真实端口、不复制 ORM fixture/自动 truncate 系统。
- task/worker 等生成器先按对应 Job/Command/Scheduler contract 评估；既有 `gen1-plan.md` 中 mailer/policy/job/controller/scaffold 生成器保持原里程碑，不在此另建平行任务。
- M6 本项验收：`doctor` 只读检查配置、已配置数据库后端/版本/连通性与迁移状态，不执行迁移/seed、不改写配置或数据、不泄漏密钥；`TestApp` 以独立 `Application` 公共 API 复用 `finalize`、`build_router` 和 oneshot，验证统一响应、状态、headers、JSON path、错误与显式 fake/plugin 替换，数据库路径复用 `TestDatabase` 并在 PostgreSQL/MySQL 真实执行。生成器的 check/test 验收只按 [gen1-plan.md](plans/gen1-plan.md) 对应里程碑完成，不在本项重复排期。

## 阻塞

- 完整集群负载均衡、跨节点认证/幂等/队列等最终验收尚未完成；M2 读写路由的真实复制拓扑证据不替代这些集群门禁。
