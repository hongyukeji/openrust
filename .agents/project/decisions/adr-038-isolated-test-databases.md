# ADR-038 数据库集成测试的用例级隔离

- 状态：Accepted
- 编号校准：原文件误用 ADR-011，与短信 ADR 重复；仅调整编号和路径，隔离决定不变。
- 日期：2026-10-25

## 决策

`testing::TestDatabase::create::<M>(admin_url)` 接受一个可创建和删除数据库的管理连接 URL，并使用应用的 `MigratorTrait` 初始化独立数据库。PostgreSQL 在每次创建时建立迁移后的短暂模板库，再通过 `CREATE DATABASE ... TEMPLATE ...` 克隆；MySQL 为用例创建独立库并重放迁移。返回的 guard 提供连接 URL 和连接句柄；`close().await` 显式关闭连接并删除数据库，Drop 在 Tokio runtime 可用时尽力异步清理。

库名由框架生成，不从用户输入拼接 SQL。连接 URL 的数据库名路径会替换为隔离库名，凭据与查询参数保留。数据库创建或迁移失败时清理已创建的库。

## 理由

每个测试用例拥有独立 schema 生命周期，避免共享测试库的并发读写污染。PostgreSQL 原生模板克隆避免重复应用迁移；MySQL 不支持该克隆方式，因此每用例运行迁移。
