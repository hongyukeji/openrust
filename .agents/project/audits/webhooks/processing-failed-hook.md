# 接收 Job 的原生终态回调

固定 Spatie 接收 Job 通过 Laravel 原生队列执行。固定 Laravel 的 `CallQueuedHandler::failed` 在队列进入终态时重建原命令，再调用应用的失败方法。OpenRust 在 `ProcessWebhookJob` 提供默认空实现的 `failed`，由已有 `JobHandler::failed` 转发。普通重试沿用原处理入口。 [固定 Spatie ProcessWebhookJob](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Jobs/ProcessWebhookJob.php) · [固定 Laravel CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php)

回调在原生失败日志结算后重新从登记的存储取得记录。接收 ID、范围、配置、修订和重放代次必须仍匹配。重放另按当前策略验证授权和有效期。回调获得只读队列上下文、读取后的模型和类型化错误，可以记录告警或执行按接收 ID 幂等的动作。实际业务异常仍由包固定脱敏，回调错误同样映射为固定分类。该观察是原生 Job 的进程内终态通知，进程退出后的可靠恢复和接收租约根据各自数据库状态运行。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 使用实际 HTTP、Artisan 配置／路由缓存和独立 Worker。普通业务首次失败重试，只有耗尽后的实际终态调用回调。显式立即停止时，回调读到同一接收记录、已终止诊断及已存在的失败队列记录。成功、终态重复任务、队列修订失配、过时的重放代次和撤销重放权限均未调用业务回调。允许当前重放后，原生队列终态回调正常读取记录，独立有效任务仍可处理。夹具区分队列尝试上限和单次 Worker 的 `--tries`，并检查回调异常与原始业务错误未被输出。严格 Clippy 与三语文档同步验收。
