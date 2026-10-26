# 验收口径

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：下表归纳仓库已有实现和测试记录；最新目录收尾已执行 workspace 与双库服务验证。其他命令和历史证据边界见 [testing.md](testing.md)。

## 当前已有证据

[ADR-039](../decisions/adr-039-source-workspace-distribution.md) 的源码交付与包改名目前只有设计/文档，尚无实现验收。旧包名、旧模板 patch 的通过记录不能替代新源码 workspace 黄金路径。

| 范围 | 已有通过记录 | 证据边界 |
| --- | --- | --- |
| V2 前端选型文档 | pure-admin 精简国际化版 + Element Plus 选型同步；102 个本地链接目标、11 个锚点和 2 个上游链接检查通过；diff 空白检查与旧选型引用扫描通过 | 仅文档验证；未创建前端应用、安装依赖或执行前后端运行验收，v2 仍须待 v1 完整通过后启动 |
| 根模块/配置/目录 | workspace 默认/关键/all-features、clippy/test/doc/deny；配置层级、`.env` 生产隔离、脱敏与生成项目；工具目录/示例/测试分组迁移前后 5 packages、265 test-list entries 一致 | 不等于 v1 全部能力、最低版本或集群验收 |
| HTTP/出站/i18n | 路由、响应、CORS、request ID；本机 TCP/fake client、受界 body/retry；locale 并发与生成项目中文错误 | 不等于第三方供应商发送、OpenAPI或实时通信验收 |
| HTTP 路由限流 | 公开 `RateLimitLayer` 支持 trusted IP、auth user、匿名 fallback 与自定义 scope/key；HTTP 行为测试加 PG18/MySQL8.4 真实隔离库 store/路由层各通过 | 未包裹路由不自动限流；完整跨节点压测仍属集群门禁 |
| M3 数据输出/查询边界 | 排序 parser 单测、严格 DTO/显式过滤与关联示例；PG18/MySQL8.4 CLI 生成 UUIDv7/软删除项目的格式、check 和真实隔离库 CRUD 通过 | allowlist/授权由应用定义；软删除 scope 是显式调用，不自动覆盖 raw SeaORM/SQL |
| M4 类型化事件 | 顺序/短路/类型隔离、fake queue payload/error 单测；临时下游 consumer API check；PostgreSQL 18.6 与 MySQL 8.4 的真实隔离库事件 rollback/commit/worker 消费各通过；workspace default/all-features tests、all-target/all-features clippy、warnings-denied docs、deny、fmt 通过 | 不覆盖事件跨节点广播、exactly-once 或完整集群 |
| 双库与读写路由 | PG18/MySQL8.4 真实数据库迁移/事务/隔离；真实复制暂停、primary scope 隔离与追平 | 串行 SELECT 1 微基准不是业务性能证明；不等于完整集群 |
| M3 认证/安全 | 两库 auth_database/auth_runtime 每库各 6 项；新生成 auth 项目格式/check/真实 TCP auth_flows；Mailer/Policy 生成器双库项目 check/test | 不包含业务流程扩展点或跨节点完整场景 |
| Mail/SMS | Mailpit v1.32.0 本机 SMTP 实收；Twilio transport 协议/错误/超时/no retry；模板 check/test | 没有实际 Twilio 账号投递或送达证据 |
| 加密 | AES-256-GCM-SIV 已知向量、轮换/AAD/篡改/配置脱敏、生成项目 | 算法标准化不等于 crate 实现经过完整独立审计；发布前复核审计证据和平台性能 |
| 审计/签名 | 双库同事务审计提交/回滚/失败回滚/清理，签名用途/TTL/轮换/共享撤销，生成 auth 流程 | 不承诺审计不可篡改、远端 sink 可靠投递或 JWT access token 即时撤销 |
| 队列基础/唯一键 | 双库并发领取、事务派发、失败/手动重试、超时后继续处理、强杀进程后 lease 恢复、活动唯一键并发/同事务去重与终结释放 | 至少一次；未覆盖全部旧 token 接管竞态、完整请求幂等 |
| 有限任务链/批次 | 双库任务顺序、停止/继续、并发上限、事务回滚与进度测试已有记录 | 单个持久 parent job；重试可重跑全部子任务，不提供逐子任务独立 checkpoint |
| Scheduler | 两库独立进程同 slot 竞争、提交后异常退出去重、once 和 SIGTERM；UTC interval/daily 边界 | 触发去重不是副作用 exactly-once；不是生产负载/完整集群验收 |

## 尚未关闭的门禁

- 目录收尾通过 5-package/265-test-list 对比、workspace 门禁、双库 server 集成、生成项目及实际 hello TCP HTTP smoke；`apps/server` 有明确保留证据。以上不等同于 v1 集群或性能门禁。
- HTTP 数据输出/查询边界；M4 事件/编排/公开流程扩展/幂等与共享锁；M5 缓存/存储/实时/可观测性；M6 开发编排/生成全集/测试工具/正式文档。
- PostgreSQL/MySQL 负载均衡下至少两个 API 与多 worker/scheduler 的共享认证状态、配额/幂等、任务竞争、跨节点在线投递、共享文件、滚动升级及故障恢复。
- Redis 与对象存储真实驱动语义；代表性业务负载 p50/p95/p99、吞吐、CPU、内存和资源上限；发布前设定基于实测的回归门槛。
- 暂定最低 PostgreSQL 14+ / MySQL 8.0+ / Redis 6.2+ 的完整承诺路径；日常只用 PG18/MySQL8.4/Redis8，额外矩阵按大跨度/明确兼容风险与发布门禁触发。运行时版本检查通过不等于最低版本功能已全部验证。
- Twilio 实发需要开发者授权账号、凭据与目标；未满足则外部验收保持未通过，不能虚报投递成功。发布/tag/上线仍由独立授权决定。

## 报告规则

每簇报告源码/WIP/本地提交状态、真实执行命令及退出结果、使用的服务与隔离范围、跳过/缺失项。历史记录只作为已有证据，变更影响对应行为时重新运行；无需为文档改动重复全量构建。

无数据库 URL 的 test skip、all-features 编译、fake transport、单机多 App、各自独立数据库的两个进程，都不能代替所声称的真实共享/多节点路径。目录收尾前 CI 的服务作业未包含 queue feature；本簇已在工作区补入，仍须验证实际执行后才可报告 M4 双库 CI 覆盖完整。
