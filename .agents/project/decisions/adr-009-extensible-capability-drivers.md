# ADR-009 能力驱动扩展与默认实现

- 状态：Accepted（设计原则与需求边界；不表示相关驱动已经实现）
- 日期：2026-10-25

## 背景

框架无法内置所有数据库、缓存及基础设施实现。官方提供常用实现，开发者应能按公共契约在独立 crate 中实现、注册和分发扩展，无需修改或 fork 框架。默认体验与扩展成本共同决定框架的适用范围。

## 决策

### 通用原则

- 默认实现满足常见需求，扩展契约承接差异需求；官方能力有边界，开发者的扩展空间保持开放。
- v1 版本以创建项目后直接进入业务开发为目标，优先打通数据库、认证、队列、存储与测试的完整路径；不以穷举驱动或其他成熟框架的全部功能为完成标准。
- 数据库、缓存、存储、邮件、队列、认证等能力在适合的边界提供统一入口与可替换实现；不同领域不强行共用一个万能 driver trait。
- 复用既有能力 crate、`Plugin`、feature 与配置机制。普通应用使用合理默认值；需要差异实现的开发者通过公开契约接入。
- 扩展契约是需要正式维护的公共能力：包含注册入口、生命周期、错误语义、文档与契约测试；不只写“以后可扩展”。检查内部枚举、具体类型及私有注册逻辑是否不必要地阻断外部实现。
- 基础行为保持一致，后端特殊能力显式说明或校验；不承诺所有驱动完全等价，不以非原子操作模拟并宣称原子语义。
- 按真实实现提炼最小契约，不自研通用 ORM，不提前创建庞大抽象或空壳 crate。公共 API 与命名仍遵循 ADR-006，具体 contract 形成后再实施。

### 数据库边界

- v1 版本仍只交付 PostgreSQL 与 MySQL；暂定最低支持 PostgreSQL 14+ / MySQL 8.0+，开发期分别跟随 PostgreSQL 当前稳定主版本与 MySQL 当前 LTS 版本线。v1 发布前按届时官方生命周期复核并完成最低版本能力验证。
- 统一 `Db` 使用入口，将连接初始化、后端识别与必要能力检查，以及迁移、UTC/JSON、锁等方言差异集中管理，避免认证、会话、队列中散布数据库判断。
- 切换主要通过编译期 feature 与连接配置完成，不要求运行中热切换；切换驱动不负责跨库迁移已有数据。
- 现有连接与事务 API 使用 SeaORM 类型；其支持范围构成当前扩展边界。ORM 未支持的数据库还需要底层适配，不能仅实现连接插件就宣称支持。
- 数据库扩展复用 `Db`、`DatabasePlugin` 约定与 `Plugin` 生命周期，不增加通用 ORM/driver trait。自定义插件在 `build` 注册唯一 `Db::default()`，在 `finish` 建立 SeaORM `DatabaseConnection` 并调用一次性的 `Db::initialize()`；框架 `MigrationPlugin`、`SeedPlugin`、事务辅助继续使用该服务。注册重复或重复初始化必须报错。
- 内置 backend URL scheme 和 feature 检查由 `openrust::db::DatabaseBackend` 统一提供，并由 `openrust-test` 共用；自定义插件可直接接入 SeaORM 后端并自行管理连接配置。SeaORM `DbBackend` 是实际连接类型，不扩展为 OpenRust 专属 SQL/ORM trait。
- 当前内置 `DatabasePlugin` 建连后校验暂定最低 PostgreSQL 14+ / MySQL 8.0+；日常开发与 CI 基线为 PostgreSQL 18 / MySQL 8.4 LTS。最低版本完整能力矩阵仍须在发布前验证。自定义插件绕过内置连接初始化时，必须自行执行适用的版本与能力检查。
- `Db::backend()` 提供已初始化连接的 SeaORM backend。官方 v1 暂定支持 PostgreSQL 14+ 与 MySQL 8.0+；扩展可连接 SeaORM 支持的其他关系型 backend，但须自行校验自身及所用框架能力，适配迁移、UTC/JSON、锁、生成器与测试隔离，不能因 CRUD 或成功连接宣称框架全功能兼容。
- 新增关系型 backend 要进入官方支持矩阵，仍需增补 feature、连接默认值、安全策略、能力检查、方言、迁移/seed、测试隔离、生成器、文档与真实集成验证。MongoDB 等不同数据模型不适用此 `Db` 替换契约。
- `openrust::db::dialect` 对未支持 backend 明确返回迁移错误；`openrust-test::TestDatabase` 的 PostgreSQL 模板复制与 MySQL 迁移重放是 backend 专属实现，新增 backend 必须单独实现，不能由连接插件自动继承。
- MongoDB 等不同数据模型不承诺直接替换关系型数据库。能够 CRUD 不代表满足迁移、会话、数据库队列等全部要求。
- 用 PostgreSQL 与 MySQL 的真实集成测试验证公共契约和差异行为。保留必要的专属能力入口，并明确其可移植性限制。

### 缓存边界

- 公共能力入口为 `Cache`；支持通过配置选择缓存驱动，业务尽量使用同一基础接口。
- 需求范围包括进程内 memory、file 与 feature 可选 Redis；标准应用默认 Redis，memory/file 显式选择（默认值见 ADR-015），具体 contract 见 [ADR-014](adr-014-cache-driver-and-store-contract.md)。其他实现允许后续扩展，不要求 v1 全部内置。
- driver 是实现方式，如 memory、file、redis；命名实例是某种驱动加具体配置。`store` 仅为命名实例概念，不能替代公共能力名 `Cache`。
- 多实例使用能力已按 [v1 覆盖计划](../plans/v1-capability-coverage.md) 纳入范围；default store 与具名 store contract 见 [ADR-014](adr-014-cache-driver-and-store-contract.md)，不建立平行缓存入口。
- 统一基础读写、删除、TTL 与序列化约定，区分未命中和驱动故障；明确增减、条件写入和锁的原子性及适用范围。缓存 tag 等后续能力不因此提前加入。
- memory 仅进程内共享；file 需处理安全路径映射、过期、原子文件替换、并发与 I/O 错误，不承诺跨主机共享或分布式锁；Redis 等外部驱动按 feature 可选启用。需要跨节点共享的功能不得静默降级到本地实现。
- 各驱动执行公共契约测试并补后端专项验证。扩展能力的命名与行为按 Rust 惯例设计，不机械复制其他语言的 API。

## 影响与执行

- 本决策补充 [架构](../architecture.md) 的扩展模型，以及 ADR-002 的数据库范围、ADR-003 的默认实现、ADR-006 的命名规则；不替代这些契约。
- 数据库扩展边界随 M2 推进，缓存驱动随 M5 推进。实施前按本决策细化 Spec 与公共 contract，再将可闭环任务纳入活跃待办；不打断正在验证或提交的功能簇。
- 在后续能力设计与审查中，检查第三方能否仅依赖公开契约完成扩展、是否需要修改框架，以及可通过哪些测试验证行为。
- 具体驱动与外部扩展入口的实现状态以代码和 `core/` 事实为准，不因本决策被接受而视为已完成。
