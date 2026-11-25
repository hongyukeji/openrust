# Webhook 发送持久领取与首次投递

## 固定契约与实现

Spatie Server 固定提交 `6da6be3a721190bcdedcdef32d31d5980b77eb0e` 的 `CallWebhookJob` 以 2xx 为成功，未收到响应或命中可重试状态才继续，允许全量抖动 `0..=backoff`，可用 Retry-After 延长等待。OpenRust 复用指数退避和具名策略；本批把尝试编号、绝对期限和上限放在数据库。Retry-After 支持秒数与 HTTP 日期，日期直接以绝对时刻参与调度。队列只用于唤醒一次尝试，原生 Worker 设置单次预算。启用最终失败抛出时，当前 Job 将失败交给原生 Worker。同步 API 返回投递进度。事件载荷、上游全部异常及访问器仍按后续批次核对。[固定 Job 源码](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/CallWebhookJob.php)

采用同库事务写业务操作与投递意图。原生 Queue 收到提交后的 ID，提交与通知之间中断时，限定范围和数量的 Artisan 扫描从数据库恢复。扫描、同步与 Worker 共同使用基于数据库时钟的条件更新领取租约。领取与尝试开始记录同事务，写回校验租约及令牌，实际连接在事务外。接管时旧尝试标记结果未知，端点应按稳定消息 ID 去重。HTTP 请求到达后的进程崩溃使本地结果无法确认，重试可能再次到达端点。[AWS Transactional Outbox](https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html)

持久化版本化请求、稳定消息 ID、原始正文及签名引用。相同范围与幂等键、相同快照返回旧 ID，快照改变时返回冲突。业务事务回滚随之移除发送意图。快照拒绝内联密钥和敏感请求头，队列数据仅含投递 ID。应用策略在每次尝试重新校验当前端点许可与凭据引用；默认策略要求 HTTPS，按次解析并验证全部地址。HTTP Client 固定连接、保留 Host 和 SNI、保持证书校验并禁止代理和重定向。保存有限状态与 HTTP 状态码，响应读取限制 64 KiB。超时与传输中断记为未知，重试受同一个截止时间与尝试上限约束。DB 和 Queue 驱动错误转换为脱敏包错误。

具名 Job 按数据库驱动、连接名、数据库名和范围隔离，避免选择不同连接时误用已注册的 Worker。生产布局中的 Worker 必须使用一致的逻辑连接、范围和授权策略。迁移与包发现一起启用，已有投递记录时拒绝回滚投递表。

## 已执行验收

Rust 1.94.0，使用 `4f19d7c29` 独立源码和已验收 Webhook 实现至 `289e12714`，再叠加本批文件。隔离锁图与根工作树分别更新，只接入已有锁定依赖。三语六组代码示例、Webhook Server 八个测试和 workspace/all-targets + 两特性严格 Clippy 通过。

`tests/webhooks_send_acceptance.py` 使用独立接收进程和二进制应用，在 SQLite、PostgreSQL 18.7、MySQL 8.4.13 分别执行迁移、配置及路由缓存、真实 HTTP 登记、Artisan 发送和独立 Worker。验证原始二进制与独立 HMAC、固定连接及 Host、禁用重定向、事务回滚、并发幂等与快照冲突、跨范围和连接隔离、2xx／302／503、允许列表、Retry-After 秒数与日期、超时、预算耗尽、许可撤销、进程退出后重领与旧令牌拒绝写回。等待时间以数据库中保存的到期时刻验证，再在隔离数据库提前触发扫描或租约到期，以确定性覆盖竞争。日志位于 `target/tmp/webhooks-first-delivery/`。

现有接收回归的数据库队列计数曾在改动前基线与本批夹具上同样失败。诊断发现前置用例留下两个 `webhook-receipts` 任务，被恢复用例的全表计数混入。回归改为核对该用例实际使用的 `webhook-recovery` 队列，并在结束时检查其他队列的计数没有变化。修复后 SQLite 公开路由与控制器套件通过，记录 136 个 HTTP 请求及 83 次独立 Worker。原始正文、两种签名协议、包发布与配置缓存回归也通过。

WH-04／WH-05 保持开放。[Builder 派发和发送事件](dispatch-events.md)在后续批次已接入。任务定制、自动调度和入列丢失恢复、撤销前后时序、重放、端点／订阅、限流、日志保留、代理／mTLS／自定义 CA 和单机／集群部署门禁仍须分别验收。首次投递用应用授权的回环 HTTP 对端，TLS 边界来自上一批固定连接测试。MariaDB／SQL Server 尚未运行本批发送夹具。本批没有远程供应商或生产 TLS 结果。
