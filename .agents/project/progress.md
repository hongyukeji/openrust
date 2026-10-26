# 当前进度

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：框架处于 v1 M4，尚未达到完整 v1。目录迁移与 M3 mailer/policy 生成器已完成本地验证和提交；完整剩余范围见待办。

## 已有交付与边界

| 能力 | 当前实现与证据 | 尚未覆盖 |
| --- | --- | --- |
| 根模块与配置 | 框架合入根 `src/`；typed config、APP_ 可定制命名空间、`.env` 生产隔离与脱敏；默认/关键 features 和生成项目已有验证 | 无本簇剩余项 |
| 内核与 HTTP | Application/Plugin、显式服务、路由/resource、验证、统一响应、分页、request ID、CORS、压缩、超时、panic 处理；`RateLimitLayer` 显式限流支持可信 IP、已验证用户、匿名回退和应用 key，PG18/MySQL8.4 隔离库 HTTP 验收通过 | 数据输出/查询边界、OpenAPI、完整 readiness/metrics/OTLP |
| 数据层 | PostgreSQL/MySQL、迁移/seed/事务、tb_ resolver、隔离测试、显式读写分离；PG18/MySQL8.4 真实复制暂停/追平已验证 | named 独立连接、事务公开扩展、查询/输出补强、完整业务性能与集群 |
| 认证与授权 | JWT/refresh、可选 Session/CSRF、PAT、VerificationCodes、AuthFlows、Policy/RBAC、共享限流；双库 auth_database/auth_runtime 与生成项目 TCP HTTP 流程已有通过记录 | 公开注册/登录流程扩展、完整跨节点业务场景；access token 不逐请求查撤销 |
| HTTP client 与 i18n | 统一有界出站调用/fake、请求级 locale、中英资源与应用 catalog；生成项目/本地 HTTP 验证 | 不等于外部供应商实发或完整语言格式化引擎 |
| 邮件与短信 | Mailer/SmsSender、SMTP/log/fake、Twilio adapter、本地化模板；Mailpit 本机 SMTP 实收、Twilio 协议测试通过 | Twilio 真实账号投递待授权；队列发送和薄多渠道通知待补 |
| 加密、签名、审计 | 进程内加密/key ring、用途隔离签名链接/可选共享撤销、可替换审计 sink/同事务审计；已有单测、双库相关路径与生成验证 | 加密实现审计/平台性能复核；不承诺任意远程 sink 可靠送达或审计不可篡改 |
| 队列与 worker | 数据库任务、显式 handler、事务派发、有限并发/重试/失败保留/超时/lease/心跳；双库强杀恢复；活动唯一键、任务链/有限批次已提交 | 事件、共享锁/请求幂等、旧 token 接管专项与完整负载集群 |
| 目录与测试入口收尾 | CLI/macros/test packages 已迁至 `cli/`、`macros/`、`testing/`；hello 位于 `examples/hello/main.rs`；根 HTTP 和 server integration targets 按能力分组并显式保留 target 名。迁移前后均为 5 个 package、265 个可列测试条目。PG18/MySQL8.4 server all-features 集成各 40 项通过；生成 PostgreSQL/MySQL `--auth` 项目各通过 fmt/check/真实 auth_flows HTTP test；hello TCP HTTP 200；workspace fmt/clippy/test/docs、deny 与 no-default check 通过。`apps/server` 保留；旧 `crates/` 源码和目录已移除。 | 无本簇剩余项 |
| Scheduler | UTC interval/daily slot、数据库时钟、slot 与 job 原子去重、run/once/list；双库独立进程竞争/异常退出/SIGTERM 已验证 | 不含 cron/IANA/DST；生产资源、滚动升级和完整拓扑验收未完成 |
| CLI 与测试工具 | new、model/migration/mailer/policy 生成、dev/运行命令代理、TestDatabase；Mailer/Policy 按 [M3 Spec](plans/m3-cli-generators-spec.md) 完成；PG18/MySQL8.4 认证项目各通过格式、check 与真实 HTTP auth_flows 测试 | job/controller/scaffold/command 生成器、依赖编排/watch、doctor、TestApp/HTTP assertions、黄金路径/正式文档 |
| 缓存、存储、实时 | 已有设计 contract | Redis/file/memory cache、local/S3 存储、上传扩展、WebSocket/SSE 仍待实现 |

具体 API 只在 [core/](core/) 维护，验收入口见 [testing.md](memory/testing.md)，声明边界见 [verification.md](memory/verification.md)。Git 中的未提交改动只算 WIP，不因文档先写“实现”而升为交付。

## 阶段判定

- 原 M0–M2 基础范围和 M3 本地认证/安全运行时已验收；Mailer/Policy 生成器与公开 HTTP 路由限流已提交；数据输出/查询边界仍待补齐。
- M4 仅部分完成；单项 scheduler/worker 多进程测试不等于完整集群验收。
- M5/M6 尚有成组能力未交付；没有可宣称已完成的 v1 性能基线、全部最低版本兼容性或发布安装路径。
- v1 完整验收通过后按已授权顺序进入 v2；AI 平台仅规划并隔离，尚未立项。

## 下一步

按 [todo.md](todo.md) 完成数据输出/查询边界，再继续 M4、M5、M6。不得重做已提交能力，也不得因旧阶段标签跳过补交和最终验收。

## 本次文档核对范围

核对当前源码、Cargo/Git、Agent 文档、ADR 与本机可定位项目记录：Codex/Claude 原生 JSONL 快照 74 份（45 份关联 OpenRust，207 条去重人类候选文本）；Paseo 26 份相关日志（24 份项目/已确认 worktree，2 份明确关联的起源会话），与原生会话按来源交叉去重，不相加计算独立对话。OpenCode 找到 7 个项目会话元数据但无消息正文；其他客户端未发现额外可限定到本项目的本地正文。

覆盖可读取的本地项目资料，不声称覆盖全部云端聊天。原始聊天和凭据不入库；只将有效要求写入相应决策与计划，历史交付说法仍以当前代码及验收证据核实。
