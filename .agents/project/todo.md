# 活跃待办

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## 框架底层调整总览（领取任务前必读）

以下是 v1 已承诺、尚未完整实现或验收的底层工作。它们分属不同功能簇和里程碑，不能当成一次性目录重构，也不能因为 M0-M2 基线完成就视为已交付。每次切换任务先核对本表及对应专题计划；专题计划是范围和验收标准的详细来源。

1. **M2 数据库读写分离已实现并验证**：`Db` 提供显式只读副本路由与 task-local primary scope；PostgreSQL 18 / MySQL 8.4 真实复制暂停/追平及双 App 隔离测试通过。指标接入和完整业务负载性能测试归入通用可观测性与性能计划，不表示完整集群验收完成。
2. **按 M4/M5/M6 依赖顺序分批补齐其他底层机制**：M3 本地框架能力与认证/安全总验收已完成；真实 Twilio 发送需授权账号和目标。跨 M3/M4/M5 的流程扩展点按现有事务、plugin、队列和事件契约后续落地；M4 完成队列/调度/事件，M5 完成缓存、实时通信、存储与可观测性，M6 完成 CLI、doctor、TestApp 和用户文档。
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
- 根模块迁移依据 [迁移 Spec](plans/module-layout-migration-spec.md)；既有 M3 WIP 的双库数据库测试与 PostgreSQL/MySQL 生成项目 HTTP e2e 已在迁移后复验通过。M3 认证作为独立功能簇验收，当前本地总验收已关闭。
- PostgreSQL 18 / MySQL 8.4 测试服务使用现有 `compose.ci.yaml`。当前根 tests/examples 与 server 验收职责映射见迁移 Spec；由于聚合启动、生成项目与双库验收仍由 `apps/server` 承担，继续保留该 package。

## M3 邮件、认证与授权（本地验收完成）

- M3 认证和通用安全本地验收已完成。PostgreSQL 18 / MySQL 8.4 的 `auth_database` 与 `auth_runtime` 各 6 项通过；按当前模板新生成的 `--auth` 项目均通过 `cargo fmt --all --check`、`cargo check` 与真实 TCP `auth_flows` 流程。
- 验收覆盖 JWT/refresh 轮换与重放撤销、可选 Session 登录/退出与 CSRF、PAT 生命周期与 ability/RBAC 交集、验证码、共享限流、可信代理、审计和签名链接共享撤销。详细 contract 与测试入口见 [core/auth.md](core/auth.md) 和 `memory/testing.md`。
- Twilio adapter 已通过本地协议测试；真实短信投递需开发者提供授权账号、凭据与目标号码。供应商投递未作为已完成证据。
- 后续按 M4/M5 完成数据库事务、队列/事件与文件等基础机制后，再实现跨 M3/M4/M5 的请求幂等、事务边界、业务流程扩展点及多渠道通知；遵守现有 Plugin、事务和事件契约，不另建 Hooks 系统。

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
