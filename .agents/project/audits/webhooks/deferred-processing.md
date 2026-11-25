# Webhook 待关联通知的持久延期

本批处理 WH-03 / WD-02 的同库持久延期路径。[固定 Laravel 队列基线](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md) 的 release 会使用原生队列尝试次数，独立业务延期预算因此由接收记录持有。原有 `ProcessWebhookJob::handle` 和注册入口继续可用。有租约的注册调用 `handle_outcome`，默认适配旧处理器的成功和错误。公开结果支持 `Completed`、`Deferred { delay_seconds }`、`RetryableFailure`、`PermanentFailure`。

`register_with_database_deferred_processing` 接受延期次数和首次延期起算的期限。延期按现有 token 和有效租约写入次数及下次运行时间，在同一个短数据库事务中派发使用新 UUID 的原生数据库队列任务。新任务的原生预算与业务延期次数分开，复用当前队列名称、连接及已核验接收标识。原任务由 Worker 现有确认流程删除。早到的副本依据已提交的下次运行时间跳过 handler，保留正常的后继任务。次数用尽或期限已过写入独立停止状态及通用诊断。明确终止沿用 Worker 的即时失败流程，普通可重试失败沿用原生队列策略。

隔离源码以 `4f19d7c29` 加已验的 `a6d725045`、`fd193a6c3` 及本批文件为基线，使用 Rust 1.94.0 和项目 build_guard。实测 HTTP 接收先于关联就绪，首次 Worker 在仅一次原生尝试的条件下持久延期，独立 Worker 读取后继任务并在关联合适时完成。检查早到副本、后继任务 UUID、已用预算、永久失败、原生失败、次数耗尽和过期不进入 handler。验收脚本对三库执行安装、迁移回滚、恢复迁移及旧记录读取。日志保留至 `target/tmp/webhooks-deferred`。

本批范围为同库后继任务。外部 broker 恢复扫描、关联就绪主动唤醒、普通队列失败后的领域重建、跨主机、尝试明细、未验签提示和真实审核消费者仍待相应门禁。Webhook WH-03 与 WD-02 总体保持开放。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各通过 5 个新区分场景的真实 HTTP 和 9 次独立 Artisan Worker。每库复跑 152 个已有 HTTP 请求和 95 次已有 Worker。旧接收、签名、许可、全仓格式和启用两 Webhook feature 的 workspace/all-targets 严格 Clippy 均通过。三语 Rust 示例、站点构建和三语根／子目录浏览检查通过。已有块体积提示仍保留。临时数据库及站点输出已清理，保留批次日志和可重用增量缓存。
