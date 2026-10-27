# v1 可选 SQLite 轻量后端计划

> 受众：AI Agent  
> 更新：2026-10-27  
> 摘要：为免部署数据库服务的轻量单机应用增加可选 SQLite 后端；不替代 PostgreSQL/MySQL 生产和集群基线。

## 目标与边界

- 依据 [ADR-066](../decisions/adr-066-optional-sqlite-backend.md)，在 v1 提供可选、可完整验收的 SQLite 集成，不把自定义 SeaORM 插件示例当成框架官方支持。
- 适用本地文件、低至中等写入并发的单机应用；不支持多台主机通过网络文件系统共同访问同一个 SQLite 文件，不宣称 SQLite 满足服务器集群共享状态。
- PostgreSQL 18 / MySQL 8.4 仍是日常数据库开发基线，PostgreSQL/MySQL 仍是生产与集群正式基线。SQLite 不成为生成项目默认项，不改变已有应用配置。
- 继续使用 SeaORM/SQLx 现有生态；不新增平行 ORM、不强制 SQLite native dependency 进入默认构建。
- 仓库已有 `tests/consumers/db-custom-sqlite/` 自定义插件 consumer。它只作为通用连接/事务/应用自有迁移的既有证据；实施前检查该目录及当前代码，不重复实现已验证部分。

## 实施顺序

进入 `todo.md` 唯一执行队列后，按以下顺序形成单一功能簇并提交：

1. **Feature 与配置**：新增独立 SQLite backend feature 和统一数据库 URL/配置路径；确认文件创建、只读/内存模式、父目录、连接池、busy timeout、journal/foreign-key 等连接选项的默认值及错误语义。默认值须由实施 Spec 和测试确认，不在此计划臆定。
2. **方言与迁移**：为 SQLite 实现明确 backend 识别、类型/helper、框架迁移和迁移 CLI 支持；缺失的 SQL 能力返回可理解错误，禁止落入 PostgreSQL/MySQL SQL 分支。
3. **测试隔离与生成器**：为 `TestDatabase` 增加安全隔离及清理策略；支持 `openrust new --db sqlite` 或与现有 CLI 选择机制一致的入口，并在生成 manifest/config 中只启用 SQLite 所需 feature。
4. **核心能力兼容矩阵**：逐项测试认证与 refresh、Session、限流/验证码、审计、队列/worker、调度、共享锁、请求幂等、迁移锁、租户和数据交换。每项标记 `支持`、`限制支持` 或 `不支持`，对不支持项提供显式错误/启动检查；不得依赖 feature 编译成功推断运行时语义兼容。
5. **开发体验与说明**：增加 SQLite 启动、迁移、备份/恢复、并发限制和转向 PostgreSQL/MySQL 的说明；确保文档明确本地文件位置及生产部署边界。

## 验收门槛

- 默认 workspace 不引入 SQLite driver feature；SQLite feature 单独构建，且与 PostgreSQL、MySQL feature 组合有明确的支持或编译期拒绝规则。
- 真实 SQLite 文件连接、事务提交/回滚、重复迁移、seed、失败恢复及并发启动行为通过测试；验证数据库文件及运行时 journal/WAL 辅助文件的创建、关闭和备份语义。
- `TestDatabase` 测试彼此隔离，不删除或覆盖用户数据库文件；临时测试资源只清理本用例创建的对象。
- CLI 新建项目可选 SQLite，干净生成项目能 check/test、运行迁移并启动；SQLite 配置缺失、路径不可写、文件权限错误均给出明确错误。
- 核心能力兼容矩阵中的每一项都有适用测试；SQLite 不支持的能力在配置/注册时明确失败，不能悄悄绕过锁、事务或跨节点共享要求。
- PostgreSQL/MySQL 既有验收无回归；至少执行 SQLite feature 定向门禁、默认 workspace 门禁、生成项目门禁及必要的 PostgreSQL/MySQL 回归。
- SQLite 只能在本机/单节点文件语义下验收；不能用单机测试替代原有 PostgreSQL/MySQL 多实例集群门禁。

## 限制与运维说明

- SQLite 允许多个读取者，但同一数据库文件同一时刻只允许一个写入者；忙等待、事务时长和写入竞争要通过测试及文档说明。
- WAL 模式会在主数据库文件之外生成 `-wal` / `-shm` 文件；关闭、备份和复制文件时必须遵守 SQLite 的一致性要求，不能建议运行中只复制主文件作为可靠备份。
- 不把 SQLite 文件存储视为共享文件系统、数据库高可用或集群复制方案。应用成长到多节点或高并发写入时，应迁移至 PostgreSQL/MySQL。
- 已验证 SQLite backend feature 与每项 framework capability 分开记录；`core/`、`stack.md` 和用户文档只有在实际验收后才能更新为“已支持”。
