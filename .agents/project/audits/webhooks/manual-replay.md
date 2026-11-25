# Webhook 接收耗尽诊断与受控重放

本批落实 WH-06、WH-V13 和 WH-V23 的接收耗尽路径。固定 Laravel `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 `queue:retry` 为原生失败任务提供独立重试，重置尝试并刷新期限。OpenRust 沿用原生队列任务承载分离后的重放代次，额外使用接收持久状态核对范围、租约和当前许可。参考 Svix 将人工操作和自动退避分开。[Laravel RetryCommand 固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RetryCommand.php) · [Svix 重试说明](https://docs.svix.com/retries)

`webhook-client:exhausted` 返回指定可信范围的有限元数据。接收记录须已确认入列、超过等待窗口、自动补派预算耗尽，且未结案、无活跃租约和未来延期。`webhook-client:replay` 由应用提供的 `ReplayPolicy` 核对操作身份、范围以及配置或凭据修订当前许可。无策略即拒绝。验证原始正文仍可读取后，在同一事务记录操作人、原因、期限与新代次，并重新打开持久派发意图。沿用原接收 ID、验签历史和旧处理尝试。旧队列代次无法领取新一轮业务处理；扫描和 Worker 在各自执行前重新核对应用策略及期限。原生任务使用重放期限并保留单个任务的重试上限。未另开自动补派预算。审计随原接收记录留存。

## 验收

隔离源码以 `4f19d7c29` 和本任务已验批次至 `0621a67a3` 叠加本批变更，守卫固定 Rust 1.94.0。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 连接隔离 Redis 8.10.3。每库使用真实 HTTP 建立原始接收记录，由独立 Artisan 进程查询、申请与恢复，再以独立 Worker 核对旧代次、撤销后拒绝与获准执行。每库验证范围隔离、缺少策略拒绝、伪造操作人拒绝、两个并发申请只登记一个代次、授权有效期、代次上限、审计及留存清除后拒绝。过期反例在隔离数据库推进审计期限。SQLite 另通过具名数据库队列路径。

三库各通过 136 次公开路由 HTTP、83 次独立 Worker 及接收租约、去重、延期、附件保留和派发恢复回归。Webhook Client 包测试、全仓格式、双特性 workspace/all-targets 严格 Clippy、三语公开示例及浏览器检查、包发现、签名及许可回归均通过。日志保留于 `target/tmp/webhooks-manual-replay`。

本批覆盖已确认入列且自动补派耗尽的记录。其他终态失败的受控重放、诊断管理面的细粒度授权、真实多主机、托管队列及业务幂等消费者仍待落实或验收。WH-06 保持开放。
