# ADR-066 可选 SQLite 轻量数据库后端

- 状态：Accepted（计划纳入 v1；实现与验收待办队列排期）
- 日期：2026-10-27
- 补充：[ADR-002](adr-002-seaorm-dual-database.md) 的数据库支持范围；SQLite 的专用实施与验收方案见 [v1 轻量数据库计划](../plans/v1-sqlite-backend.md)。

## 背景

轻量单机应用需要免部署数据库服务的启动路径。SeaORM/SQLx 提供 SQLite backend，仓库已有自定义插件 consumer 验证通用 `Db` 接入；但连接成功不等于 OpenRust 的配置、迁移、生成器、测试隔离及框架数据能力均已支持。

## 决策

- v1 纳入可选 SQLite 官方后端，使用独立 Cargo feature；不作为标准新项目的默认数据库，不强制启用 SQLite native dependency。
- SQLite 面向本地文件和单机轻应用。PostgreSQL/MySQL 继续作为标准生产及集群支持基线；SQLite 数据库文件不得放在网络共享盘供多台服务器共同访问，也不承诺具备 PostgreSQL/MySQL 的并发写入能力。
- 官方支持范围必须覆盖标准数据库配置、连接、事务、迁移、seed、CLI、生成项目和 `TestDatabase` 隔离。认证、Session、队列、限流、锁、审计、租户和其他依赖数据库的能力逐项纳入兼容矩阵：通过 SQLite 验收后方可标记支持；否则必须在启用时明确拒绝或在文档中标明边界，不得静默退化。
- 数据库方言和迁移 helper 必须显式识别 SQLite，不得将 PostgreSQL 方言作为 fallback。SQLite 不具备的复制读写分离、跨节点协调和集群共享语义保持不支持。
- 本 ADR 记录已确认的产品范围，不宣称实现完成；详细实现顺序、测试矩阵和验收门槛以专项计划为准。实现进入唯一 `todo.md` 队列后再开发。

## 后果

- 轻量应用可以选择本地 SQLite 文件，降低初次启动和部署服务依赖。
- SQLite 增加驱动、迁移、方言、测试隔离和功能兼容维护成本；只能以通过验收的能力矩阵宣传支持。
- 新建应用默认仍沿用 PostgreSQL/MySQL 选择策略；数据库切换可能需要迁移、数据搬迁或 SQL 调整，不承诺应用零修改切换。
