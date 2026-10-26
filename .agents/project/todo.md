# 活跃待办

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：只列未完成工作；完成移出本表，事实与证据进入 `core/`、`progress.md`、`memory/` 和 Git。一次只领取一个实现功能簇。

## 领取规则

先核对 Git/WIP 和 [执行校准](plan.md#执行校准)。数据库队列、调度、活动唯一键及有限任务链/批次已提交，不重复实现。若存在并行 Agent 的 WIP，先保留并闭合其当前最小簇，再按以下顺序继续。当前文档校准不修改源码；实现 Agent 领取时把实际唯一 in_progress 写在对应条目中。

## 优先：目录与测试入口收尾

状态：in_progress（实现 Agent 已迁移路径，正在执行 workspace、真实双库与生成项目验收；尚未按完整门槛关闭）。

- 范围：剩余 CLI/macros/test package 迁出 `crates/`；examples/tests 按能力分目录；审计 `apps/server` 职责；更新 workspace/path deps、宏展开、模板、脚本、CI 与文档。
- 推荐目标：`cli/`、`macros/`、`testing/`；保留 package 名和发布边界。完成全部迁移后通过 Git 删除空的旧目录。server 在职责完全替代前保留，不能为清空目录删测试。
- 验收与依赖：[目录收尾 Spec](plans/module-layout-migration-spec.md)；根迁移已完成不等于本项完成。本项关闭前不启动新的大范围能力开发。

## 补齐既有阶段的遗漏

| 功能簇 | 前置与交付 | 验收 |
| --- | --- | --- |
| M3 生成器 | 目录稳定后实现 `generate mailer` / `generate policy`；复用当前 Mailer/Policy contract，资源策略接口若需扩展先定稿 | 双库生成项目 check/test、错误输入与不覆盖现有文件；不能把 M3 运行时验收算作生成器交付 |
| 通用 HTTP 限流与数据输出/查询边界 | 复用 `DatabaseRateLimitStore`，补公开路由中间件与主体/IP/key 策略；按能力覆盖补 DTO 字段白名单、过滤/排序白名单、软删除、查询 scope 与关系/N+1 示例 | 双库限流与并发、代理/匿名边界、越权字段与非法排序拒绝；不重造 ORM；UUID/软删除生成选项按实际 contract 交付 |

## M4：异步与流程扩展

按下表顺序拆成可独立提交的功能簇；已经存在的基础 API 先核查，不重复开发。

| 功能簇 | 前置与交付 | 验收 |
| --- | --- | --- |
| 事件与公开流程扩展 | 进程内类型化有序 listener；明确 fail-fast 与队列 listener；事务 before/after、注册/登录校验与成功/失败事件，复用现有 Plugin/事务/queue | 外部应用只用公开 API 注册；顺序/拒绝/回滚/提交失败/后置错误/敏感载荷/重试；双库提交后语义；可靠通知走事务内任务 |
| 共享锁与请求幂等 | 复用同一数据库/Redis 能力契约，先落数据库实现；明确 owner token、过期接管、冲突请求指纹、进行中响应、结果保留窗口与失败恢复 | 两节点竞争、旧 owner 不能解锁新 owner；同键不同请求拒绝；业务提交和幂等记录一致；不把 `dispatch_unique` 当完整请求幂等 |
| Worker 租约与队列边界补强 | 补旧 worker token 接管专项和生产资源边界；Redis/sync driver 按 M5 可选适配补交，不把未实现 feature 写成可用。有限任务链/批次已交付，见 `core/queue.md` | 双库实际竞争/崩溃/租约接管、长任务超时、停机 drain/join；handlers 至少一次、业务幂等 |
| 队列化通信与薄通知层 | 序列化邮件/SMS 作业与多渠道编排；复用 Mailer/SmsSender、locale、队列 retry；不新增独立重试系统 | 同事务派发、失败/重复投递边界、fake 与真实本机 SMTP；Twilio 实发仍需授权目标与凭据 |
| M4 生成与集成收口 | `generate job`、可运行 API/worker/scheduler 组合示例；双库 CI 显式启用 queue 并运行进程测试 | `.github/workflows/ci.yml` 原数据库作业只启用 auth/db；当前结构簇已补 queue，验证时证明 queue/schedule/崩溃测试确实执行，缺 URL 不得作为通过；完成 M4 全部门禁 |

## M5：共享基础设施与实时能力

| 功能簇 | 前置与交付 | 验收 |
| --- | --- | --- |
| 缓存驱动与命名实例 | ADR-014/015：标准应用 Redis 默认，memory/file 显式选择；default/named stores、custom driver、TTL/序列化/原子边界；库 Redis feature 可选 | 三驱动契约、真实 Redis、file 安全路径/过期/原子替换/并发/错误、独立扩展示例；Redis 故障不隐式回退 |
| 多实例与可选驱动 | 明确默认/named database、cache、storage 等实例与生命周期；数据库已完成的主从路由不等于多个独立连接；可选 Redis Session/queue 及 sync 测试执行复用既有契约，先定与数据库 driver 不同的能力边界 | 实例命名/重复注册/未知名称/配置错误；会话过期/撤销、Redis 队列恢复与真实服务测试；sync 显式使用，不伪装持久化或跨后端事务派发；不引入全局 service locator |
| 存储与上传下载 | local/S3-compatible、流式 I/O、限额/MIME/路径与签名下载；上传前、内容可读发布前、完成/失败扩展 | MinIO 实集成、受控临时对象、检测拒绝/超时/取消清理、版本绑定与直传绕过防护；集群文件可达 |
| WebSocket/SSE | 复用 auth/config/HTTP；握手与频道授权、Origin、凭据过期/撤销、心跳/背压/连接与消息大小边界；共享在线跨节点投递 | 两 API 节点真实通信、断线/慢消费/停机、权限变更、资源回收；不承诺离线可靠消息 |
| 可观测性与生产资源 | readiness、指标与可选 OTLP、HTTP/worker/DB/cache/实时有界并发与超时；安全脱敏和统一 request/trace 关联 | 依赖失效 readiness、低基数指标、阻塞任务不拖垮 runtime；代表性负载与峰值资源证据，不能只写指标接口 |

## M6：开发体验与 v1 最终验收

- CLI：补 controller/scaffold/command 等剩余生成器、routes、只读 doctor、`db status` 和显式 `db reset`；复用已有 config show/migrate/seed。status/doctor 不写数据；reset 另定目标环境、确认和数据清理边界，在隔离库测试，不由 doctor/dev 隐式触发。
- 开发编排：dev 先探测并复用合适服务，缺失时才建本任务资源；按显式配置处理迁移、watch、信号、端口冲突和进程回收。当前 dev 只是代理 serve，不能宣传自动编排已完成。
- 测试体验：TestApp/HTTP assertions 复用 finalize/build_router/Tower oneshot 与 TestDatabase；fake 显式替换。OpenAPI、错误/分页 schema、文档端点与授权范围一致。
- 交付：`examples/blog`、双库生成黄金路径、Docker/Compose、备份恢复/滚动升级/迁移说明、mdBook 与迁移对照；验证源码分发和包内容完整，未发布依赖用受控本地 patch 验证。
- 最终门禁：默认/最小/关键/all-features、MSRV、依赖审计、双库真实所有数据路径、最低兼容版本；至少两个 API 加多 worker/scheduler 经负载均衡验证共享状态、实时与文件、故障/滚动恢复；代表性性能基线与回归阈值。
- 加密：按 ADR-034 复核选用实现的审计证据、依赖与目标平台性能；算法标准不能替代实现审计。外部供应商真实短信需要授权账号/目标，未验证如实标注，不能把 v1 最终门禁报告为全部通过。
- 生成器各原定里程碑交付，M6 只做全集复验，不借“最终统一做”遗漏 M3/M4 交付。发布/tag/推送由独立授权处理。

## v1 通过后的已授权下一阶段

按 [v2 后台计划](plans/v2-admin-platform.md) 执行 B1–B5；候选功能仍先评估，不因列出即全部开发。AI 平台未启动，不创建产品分支/worktree，不混入框架。

## 外部条件与证据缺口

- Twilio 实发缺授权账号、凭据与目标；可继续不依赖实发的工作，不能代称已投递。
- 完整 Redis/对象存储/跨节点实时、最低版本整体兼容、集群与性能门禁尚未完成。可运行基础服务或单项进程测试不能替代这些证据。
