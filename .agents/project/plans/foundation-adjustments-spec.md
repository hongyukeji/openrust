# 框架底层调整 Spec

> 状态：实施中；以当前任务授权底层调整优先于暂停中的 M3 功能开发。既有 M3 WIP 必须保留，不以迁移为由覆盖或混入无关提交。

1. 第一簇完成根 `openrust` package 的模块布局迁移；已按契约调整的配置模块路径为 `src/config/mod.rs`，运行配置仍在项目根 `config/`。
2. 保持 `openrust::...` 导入、feature 可选性、错误语义、模板 manifest 和 `apps/server` 验收；只删除已由等价测试/示例接替的 package。
3. 第二簇为 `openrust::db` 读写分离：默认单库，显式主/副本连接、事务主库固定、强制主库、请求/工作单元内写后读一致性和可配置故障策略。
4. 读写分离需先明确 `Db`/连接逃生口覆盖边界、连接池预算、路由 API 与错误语义；不得解析任意 SQL 宣称透明路由。
5. 第三簇按 M4/M5/M6 依赖实现集群共享能力：队列租约/调度去重、缓存/锁/存储、跨节点实时路由、就绪摘流与观测；memory/local file 不充当共享权威状态。
6. 依赖顺序：模块边界与 feature 收敛 → 数据库路由 contract/实现 → 共享状态和后台生命周期 → 多副本验收；独立模块可并行验证但不并行开功能簇。
7. 涉及 `openrust::{core,config,http,db,auth,mail}` 模块、`openrust-test`、`openrust-cli`、`openrust-macros`、`apps/server` 及生成模板；workspace 保留 package 已按迁移盘点确定。
8. AI 平台及其产品业务不进入框架源码，也不作为改变通用 API 的理由。
9. 兼容性以当前 facade 公共路径为准；工作区现有未提交内容逐项保留，只有纯迁移且经过目标验收的内容才进入架构提交。
10. 验收包括 metadata/package 理由、有效 feature 矩阵、workspace fmt/clippy/test/doc、生成项目、根集成测试；日常数据库行为以 PostgreSQL 16 与 MySQL 8.4 LTS 实测。
11. 读写分离另需真实主/只读副本拓扑、复制延迟/强制主库/故障恢复与资源测量；集群最终验收按 `v1-cluster-deployment.md` 完成，不以单机测试代替。
