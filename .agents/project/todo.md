# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## 后续里程碑

- OpenAPI：后续里程碑用 `utoipa` 生成 schema，并在非生产环境挂载 `/docs`（ADR-008 §6）。

## M3 邮件、认证与授权（进行中）

- 当前功能簇：按 [ADR-012](decisions/adr-012-auth-current-user-contract.md) 实现业务用户 ID 映射、路由认证配置和身份注入，并覆盖共享 Session/PAT 撤销、CSRF 与 `Inject` 共存。
- 最新认证默认方向按 [ADR-018](decisions/adr-018-jwt-primary-authentication.md)：前后端分离 API 以 JWT access/refresh 为主，Session 可选，PAT 用于自动化集成。当前 Session/PAT WIP 不等于默认方案已实现；认证路由、登录流程和生成项目需按新 contract 校准。
- 后续工作：优先实现 JWT Bearer 验证与签发、refresh 哈希存储/轮换/撤销及跨节点语义；接入通用 Session 全局中间件及 PostgreSQL/MySQL store 作为可选模式；公开 PAT 创建、一次性凭据返回、列举/撤销生命周期。认证流程与 `apps/server`/生成项目示例闭环后再关闭当前簇。自定义认证器仅验证扩展路径，不替代默认数据库实现验收。
- 本簇验收前按 [执行校准](plan.md#执行校准) 核对契约：共享会话与跨节点撤销、认证所需全局限流/验证码边界、注册/登录公开扩展点及 `Inject` 共存；后续 M3 任务按完整 v1 覆盖清单逐簇领取，不以身份注入完成代替整个 M3。
- ADR-012 已定义身份/路由 contract；本簇后续仍需形成独立授权 API 与角色/权限变更传播 contract，不把身份提取实现当作 M3 完成。

## 配置与数据命名校准（待领取）

- 按 [ADR-016](decisions/adr-016-neutral-application-namespaces.md) 实现 `APP_` 默认/启动前自定义配置命名空间，以及 `tb_` 默认/空或自定义表前缀和完整表名映射；同步配置引导、认证 schema/查询、CLI、模板与文档。当前仅定契约，避免与认证 WIP 重叠编辑。
- 当前认证簇在最终验收 schema 和生成物前核对本项依赖；跨 crate 修改另定 Spec，验证 SeaORM 静态实体与动态映射边界、索引/外键及已有数据迁移。完成全量工程门禁、双库和生成项目验证后更新事实。

## M2 数据层增补：基础读写分离（当前 M3 功能簇闭环后领取）

- 按 [读写分离计划](plans/v1-database-read-write-splitting.md) 先确定公共 contract，再扩展 `openrust-db`；默认单库、可选副本、事务/锁定读取固定主库、显式主库与请求内写后保持主库、可配置故障回退。当前状态：待开发，不打断认证 WIP。
- 核对认证撤销/权限、幂等、队列/调度、迁移等主库路径；完成 PostgreSQL/MySQL 真实复制拓扑与故障/延迟验证，更新模板、README 和事实文档，不能以多个连接池代替验收。

## M5 缓存驱动与 store（M5 领取）

- 按 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md) 实现 `openrust-cache` 与 `CachePlugin`：标准应用默认 Redis、显式 memory/file、库层 Redis feature 可选（默认值按 ADR-015）；default/named stores、custom driver 注册、统一序列化/TTL/错误与原子能力边界。
- 标准生成项目显式启用 `cache-redis` 并配置 Redis default store；开发编排按配置提供 Redis，缺失配置/feature 或服务故障不得隐式回退 memory；Redis 配置读取 `REDIS_URL`，同键所选应用命名空间（默认 `APP_`）变量优先，按 ADR-016 迁移。共享缓存用途在集群配置中必须选择可用共享 driver，禁止静默退化。
- 验收：统一基础契约测试；file 临时目录安全路径、过期、原子替换、并发及 I/O 错误；Redis 真实集成；默认/file/redis feature 组合；`apps/server` 接入；独立公开扩展示例与 README/core 事实。

## 阻塞

无。
