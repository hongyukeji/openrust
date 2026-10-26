# 验收口径

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：下表归纳仓库已有实现和测试记录。本次文档校准未运行 Rust/服务集成测试；命令和复验入口以 [testing.md](testing.md) 为准。

## 当前已有证据

| 范围 | 已有通过记录 | 证据边界 |
| --- | --- | --- |
| 根模块/配置 | workspace 默认/关键/all-features、clippy/test/doc/deny；配置层级、`.env` 生产隔离、脱敏与生成项目 | 原根模块迁移；不含最新剩余目录/测试重组 |
| HTTP/出站/i18n | 路由、响应、CORS、request ID；本机 TCP/fake client、受界 body/retry；locale 并发与生成项目中文错误 | 不等于第三方供应商发送、OpenAPI或实时通信验收 |
| 双库与读写路由 | PG18/MySQL8.4 真实数据库迁移/事务/隔离；真实复制暂停、primary scope 隔离与追平 | 串行 SELECT 1 微基准不是业务性能证明；不等于完整集群 |
| M3 认证/安全 | 两库 auth_database/auth_runtime 每库各 6 项；新生成 auth 项目格式/check/真实 TCP auth_flows | 当前运行时 contract；不包含遗漏生成器/公共流程 hooks |
| Mail/SMS | Mailpit v1.32.0 本机 SMTP 实收；Twilio transport 协议/错误/超时/no retry；模板 check/test | 没有实际 Twilio 账号投递或送达证据 |
| 加密 | AES-256-GCM-SIV 已知向量、轮换/AAD/篡改/配置脱敏、生成项目 | 算法标准化不等于 crate 实现经过完整独立审计；发布前复核审计证据和平台性能 |
| 审计/签名 | 双库同事务审计提交/回滚/失败回滚/清理，签名用途/TTL/轮换/共享撤销，生成 auth 流程 | 不承诺审计不可篡改、远端 sink 可靠投递或 JWT access token 即时撤销 |
| 队列基础/唯一键 | 双库并发领取、事务派发、失败/手动重试、超时后继续处理、强杀进程后 lease 恢复、活动唯一键并发/同事务去重与终结释放 | 至少一次；未覆盖全部旧 token 接管竞态、完整请求幂等 |
| 有限任务链/批次 | 双库任务顺序、停止/继续、并发上限、事务回滚与进度测试已有记录 | 单个持久 parent job；重试可重跑全部子任务，不提供逐子任务独立 checkpoint |
| Scheduler | 两库独立进程同 slot 竞争、提交后异常退出去重、once 和 SIGTERM；UTC interval/daily 边界 | 触发去重不是副作用 exactly-once；不是生产负载/完整集群验收 |

## 尚未关闭的门禁

- 最新目录收尾、examples/tests target 保留、apps/server 替代职责审计与双库 CI 启用 queue。
- M3 生成器/通用 HTTP 边界、M4 事件/编排/公开流程扩展/幂等与共享锁、M5 缓存/存储/实时/可观测性、M6 开发编排/生成全集/测试工具/正式文档。
- PostgreSQL/MySQL 负载均衡下至少两个 API 与多 worker/scheduler 的共享认证状态、配额/幂等、任务竞争、跨节点在线投递、共享文件、滚动升级及故障恢复。
- Redis 与对象存储真实驱动语义；代表性业务负载 p50/p95/p99、吞吐、CPU、内存和资源上限；发布前设定基于实测的回归门槛。
- 暂定最低 PostgreSQL 14+ / MySQL 8.0+ / Redis 6.2+ 的完整承诺路径；日常只用 PG18/MySQL8.4/Redis8，额外矩阵按大跨度/明确兼容风险与发布门禁触发。运行时版本检查通过不等于最低版本功能已全部验证。
- Twilio 实发需要开发者授权账号、凭据与目标；未满足则外部验收保持未通过，不能虚报投递成功。发布/tag/上线仍由独立授权决定。

## 报告规则

每簇报告源码/WIP/本地提交状态、真实执行命令及退出结果、使用的服务与隔离范围、跳过/缺失项。历史记录只作为已有证据，变更影响对应行为时重新运行；无需为文档改动重复全量构建。

无数据库 URL 的 test skip、all-features 编译、fake transport、单机多 App、各自独立数据库的两个进程，都不能代替所声称的真实共享/多节点路径。目录收尾前 CI 的服务作业未包含 queue feature；本簇已在工作区补入，仍须验证实际执行后才可报告 M4 双库 CI 覆盖完整。
