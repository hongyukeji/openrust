# 接收 Worker 的显式终态失败

固定 Spatie `ProcessWebhookJob` 复用 Laravel `InteractsWithQueue`，业务处理可主动释放或标记终态失败。OpenRust 保留原生 `JobContext::fail_without_retry()` 只在处理器返回错误时生效的契约，并将该控制与接收租约的完成记录接通。 [固定 Spatie 处理 Job](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/Jobs/ProcessWebhookJob.php) · [固定 Laravel 队列控制](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/InteractsWithQueue.php)

启用数据库接收租约时，业务返回错误且要求立即失败，使用既有 `stop_processing` 在同一事务标记接收记录终止和当前尝试 `stopped`。令牌与期限继续限制写入，队列收到原有脱敏错误。显式 `PermanentFailure` 走同一存储流程。原生队列只在收到错误时应用该控制，成功返回仍完成，成功并主动释放仍允许后继尝试。失败和释放同时提出时，原生终态失败优先。已有接收和任务格式保持原样。[进程强制终止后的尝试耗尽](processing-failed-hook.md#worker-killed)在原生队列独立结算，接收记录可在租约到期后重新领取。

## 验收

以 `b1947e39c` 的生产逻辑和本批测试夹具运行真实 SQLite HTTP 与独立 Worker，成功复现领域尝试仍为 `failed` 的错误。修复后 SQLite、PostgreSQL 18.7、MySQL 8.4.13 实际运行 HTTP 提交、Artisan 和独立 Worker。显式错误只有一个终态失败记录，数据库接收记录与尝试均已终止，同一接收记录的重复任务直接确认。返回成功时保持完成语义。主动释放、真实竞争、进程退出、租约接管、旧令牌隔离，以及配置／路由缓存和跨后端恢复均随批回归。独立 Artisan 读取确认脱敏诊断，失败队列未保存业务异常正文。
