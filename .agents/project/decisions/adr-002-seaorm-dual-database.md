# ADR-002 SeaORM 2 与 PostgreSQL / MySQL 双库支持

- 状态：Accepted
- 日期：2026-10-25

## 背景

框架需要同时覆盖 PostgreSQL 与 MySQL 以扩大适用环境。SeaORM 是唯一同时成熟支持两库并带迁移与实体生成的 Rust ORM；SQLx 的编译期检查宏绑定单一数据库，不适合作为跨库默认层。

## 决策

- ORM 与迁移使用 `sea-orm` 2.x + `sea-orm-migration`；SQLx 直连只通过 `db.sqlx()` 显式获取。
- 正式支持 PostgreSQL 14+ 与 MySQL 8.0+，PostgreSQL 为推荐默认；SQLite 不在第一代。
- 迁移只使用可移植列类型；时间 PostgreSQL `timestamptz` / MySQL `DATETIME(6)`，JSON `jsonb` / `json`；框架表不用数组、枚举等单库特性。
- 单库 SQL 集中在 `openrust-db::dialect`，必须同时提供两库分支并有双库测试；CI 以 service 容器运行双库矩阵。
- 生成项目在创建时选择一个后端并只启用对应 feature，不要求运行时切换。

## 影响

- 每个数据功能的完成定义包含双库证据。
- 测试隔离采用每用例独立数据库（PostgreSQL 模板库复制、MySQL 迁移结果重放）。

## 验证

待 M0 Spike A 补充：幂等重复迁移、回滚、索引与外键在两库的实际行为。
