# 接收 Job 的原生终态回调

固定 Spatie 接收 Job 通过 Laravel 原生队列执行。固定 Laravel 的 `CallQueuedHandler::failed` 在队列进入终态时重建原命令，再调用应用的失败方法。OpenRust 在 `ProcessWebhookJob` 提供默认空实现的 `failed`，由已有 `JobHandler::failed` 转发。普通重试沿用原处理入口。 [固定 Spatie ProcessWebhookJob](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Jobs/ProcessWebhookJob.php) · [固定 Laravel CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php)

回调在原生失败日志结算后重新从登记的存储取得记录。接收 ID、范围、配置、修订和重放代次必须仍匹配。重放另按当前策略验证授权和有效期。回调获得只读队列上下文、读取后的模型和类型化错误，可以记录告警或执行按接收 ID 幂等的动作。实际业务异常仍由包固定脱敏，回调错误同样映射为固定分类。该观察是原生 Job 的进程内终态通知，进程退出后的可靠恢复和接收租约根据各自数据库状态运行。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 使用实际 HTTP、Artisan 配置／路由缓存和独立 Worker。普通业务首次失败重试，只有耗尽后的实际终态调用回调。显式立即停止时，回调读到同一接收记录、已终止诊断及已存在的失败队列记录。成功、终态重复任务、队列修订失配、过时的重放代次和撤销重放权限均未调用业务回调。允许当前重放后，原生队列终态回调正常读取记录，独立有效任务仍可处理。夹具区分队列尝试上限和单次 Worker 的 `--tries`，并检查回调异常与原始业务错误未被输出。严格 Clippy 与三语文档同步验收。

## 进程强制终止后的尝试耗尽 {#worker-killed}

固定 Spatie 的 Job 沿用原生队列机制。固定 Laravel Worker 在领取时已超过最大尝试数的场景进入原生失败流程。OpenRust 区分该原生队列结算和接收租约。进程被强制终止后，下一 Worker 在旧任务的尝试数超限时保存失败记录并调用 `failed`，接收租约依数据库时间独立结算。这使队列预算耗尽后的有效新投递可以重新领取未完成的接收记录。业务显式 `PermanentFailure` 继续在同一事务终止接收记录与当前尝试。 [固定 Laravel Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) · [固定 Spatie ProcessWebhookJob](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Jobs/ProcessWebhookJob.php)

在隔离源码 `cf93ec968` 上，用 Rust 1.94.0 构建真实消费者。通过 HTTP 接收原文，待可观测标记确认已领取接收租约且进入业务处理后，实际强制终止独立 Artisan Worker。仅在隔离测试库把旧任务推进到重领和预算边界，并把接收租约推进到过期；已有租约测试另覆盖自然到期。下一 Worker 将旧投递结算为失败，回调读到已保存的失败记录，接收尝试仍可接管。相同投递身份的副本只被确认。真实 `queue:retry` 使用同一任务 UUID 和接收标识生成新投递身份，清除旧失败记录。独立 Worker 重新领取并完成，旧领域尝试标记过期，新尝试标记完成。

第一次复跑使用相同投递身份的副本，按原生规则被确认。改用真实 `queue:retry` 后，夹具正确处理首次投递的可选身份字段。各次日志保存在本机 `target/tmp/webhook-native-exhaustion/`。本轮范围为数据库队列、SQLite、PostgreSQL 18.7 和 MySQL 8.4.13。全仓格式检查、启用两个 Webhook 特性的消费者严格 Clippy、三语站点构建及两种部署前缀下的十二个浏览器页面检查通过。强制超时隔离执行器、其他队列服务和远端部署仍按各自门禁验收。
