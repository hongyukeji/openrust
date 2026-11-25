# Webhook 验签判定依据

本批补 WD-01 的有限验证证据。固定 [Spatie Processor](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookProcessor.php) 的布尔验签和失败事件保持原语义。审计由应用按 profile 显式启用，使用可信 scope 和当前 profile 的数据库连接。应用需维护凭据集合的 revision 标签。该标签不辨别轮换期间实际命中的具体签名密钥。

三类判定分别为已验签、无签名提示和拒绝。固定原因区分签名成功、缺签名、坏签名及验证器错误。账本只保存 scope、profile、revision、验证器、策略、固定分类、原因、UTC 分钟及索引摘要。用原生 upsert 在每个分钟维度保存一次观测。重复请求合并，不用于计数。无签名候选在策略接收前记录，已验签在 Profile 与业务处理前记录。验签拒绝先保存判定，再照常派发失败事件。启用后的数据库错误使本次处理失败。定时清理由应用显式安排，预演和实际删除按范围、现有保留期和批次限制执行。

隔离源码基线 `4f19d7c29` 加本任务提交至 `b1ca9b186` 和本批文件，Rust 1.94.0，构建及验收经项目 build_guard。真实 HTTP 与 Artisan 覆盖三类判定、不同验证器及配置标签、重复合并、缺签名和验证器错误、失败监听器、可信处理任务照常执行、配置缓存、审计预演及范围清理。跨主机需要各应用节点的 UTC 时钟一致。跨主机共享限流、正式供应商查单及凭据历史修订库留在后续门禁。日志保留于 `target/tmp/webhooks-verification`。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 每库新增验收 6 个真实 HTTP、1 次独立 Worker 和 3 次审计清理调用。最终每库 192 个真实 HTTP、126 次 Worker，另执行 15 次唤醒及清理调用。三库迁移回滚和重建、旧接收与签名回归、许可检查、全仓格式、两 Webhook feature 的 workspace/all-targets 严格 Clippy 均通过。三语示例、站点构建及根／子目录浏览器检查通过。已清理隔离数据库、应用和站点输出。
