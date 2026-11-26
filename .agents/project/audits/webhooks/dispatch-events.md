# Webhook Builder 派发与发送事件

## 固定基线和 Rust 适配

核对 Spatie Server `6da6be3a721190bcdedcdef32d31d5980b77eb0e` 的六个 Builder 派发入口。上游异步在准备后、派发前触发 Dispatching 事件；同步不派发该事件。失败尝试先报告失败，再按是否为终态报告最终失败。OpenRust 条件方法使用 Rust 布尔值，条件不满足时连准备都不执行。调用方显式提供投递范围对应的服务、稳定幂等键和期限；异步返回投递 ID 和原生 Queue 派发结果，同步返回投递 ID 和进度。同步终态失败按设置返回带 ID 的错误；后台任务按相同设置由原生 Worker 记为失败。[固定 WebhookCall](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php) · [固定 CallWebhookJob](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/CallWebhookJob.php)

Builder 复用准备快照、同范围数据库登记以及统一领取；异步先校验具名 Worker 与队列连接。异步事件在投递记录提交后、原生 Queue 推送前同步派发。入队前监听器失败返回脱敏错误及投递 ID，保留记录供扫描恢复。尝试结果在租约校验及事务提交后派发；监听器失败只记通用警告，无法撤销已提交的状态或增加尝试次数。过期旧令牌不能派发该次结果事件。事件送达与数据库提交之间仍可能因进程退出而中断，进程崩溃后的 Unknown 补记和消息重放事件另随恢复工作验收。

Laravel 事件的 PHP 序列化和完整 Guzzle 响应不沿用。Rust 同步事件显式提供范围、稳定 ID、记录 ID、请求方法、裁剪查询串后的 URL、原始签名字节及应用 Meta／Tags、请求头名称和单次尝试状态／耗时。头值、签名密钥或引用、响应内容、传输原始错误不进入事件。该载荷没有 Debug 或通用序列化，现有事件诊断不会自动复制正文，业务监听器可显式访问正文并承担其使用边界。[后续传输统计批次](transport-events.md)为结果事件提供了实际 HTTP 耗时及脱敏失败类别。[固定 WebhookCallEvent](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/Events/WebhookCallEvent.php)

## 验收范围

使用前一批隔离基线及 `276487478` Webhook 实现叠加本批文件，Rust 1.94.0。验收入口为 `tests/webhooks_send_acceptance.py`。另复测缓存、包发现、三语可编译示例和严格格式／Clippy。原始执行日志保留于 `target/tmp/webhooks-dispatch/`。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 实际运行迁移、配置／路由缓存、真实 HTTP 登记／发送、Artisan 与独立 Worker。验证异步入队前观察到已提交投递且暂无当前任务，同步只发结果事件，四个跳过分支均不落库／入队／发送，试行失败后重试成功依次发出两个事件，终态失败顺序、同步带 ID 错误、Worker failed_jobs、旧令牌隔离和监听器异常后的恢复通过。事件和日志未输出密钥或测试异常原文。包发现、配置发布与两种签名协议回归通过。SQLite 接收回归计 136 个真实 HTTP 和 83 次独立 Worker。Webhook Server 八个测试、三语六组示例、全仓格式检查及 workspace/all-targets 双特性严格 Clippy 通过。首次测试的事件时序断言误计同队列保留的崩溃任务，改为只查当前投递；头名称断言补齐默认 Content-Type，未改生产事件边界。

WH-04／WH-05 仍开放。[出站定时恢复](outbound-recovery.md)在后续批次已接入。后续继续补齐可自定义 Job 的受限执行钩子、出站重放、完整接收端点管理、出站代理／mTLS／自定义 CA 与单机／集群门禁。运行期事件被进程中断、公网真实证书链、应用跨主机与真实部署结果继续单列。

## 恢复尝试的 Unknown 事件

过期租约被 Worker 重新领取或被 Artisan 收尾时，原有事务将尚未结束的旧尝试写为 `unknown`。只有写入成功且事务提交后才派发 `WebhookCallUnknownEvent`。事件提供投递范围、投递 ID 和尝试序号。Worker 先处理新尝试，再通知旧尝试，以保留现有传输时间预算。事件用于进程内观测，进程中断后的持久状态可通过投递尝试查询。

本批在隔离源码 `7ed3f823a` 叠加变更，用 Rust 1.94.0 构建实际夹具。SQLite 的真实 HTTP、Artisan、独立 Worker 和 Scheduler 通过。测试杀死正在发送的 Worker，核对新 Worker 领取后发出一次事件，且监听器从公开查询读取到已提交的 `unknown`。两次并行恢复扫描对已到期投递只收尾并通知一次，预演、重复扫描和过期所有者均未产生额外事件。三语站点构建通过。PostgreSQL、MySQL 和 Redis 队列的本批复验仍待 Docker 引擎恢复。证据位于本机 `target/tmp/goal-webhook-unknown-observation/`。
扩展包的 11 个测试、扩展包与实际夹具的严格 Clippy、Rust 格式及 Python 语法检查均通过。
