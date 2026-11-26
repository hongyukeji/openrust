# 接收 Job 的原生处理限流与并发锁

固定 Laravel `CallQueuedHandler` 在业务 Job 执行前组合其声明的队列中间件。`ProcessWebhookJob::middleware` 把应用声明的原生中间件按原顺序交给现有 Worker。已有处理器默认使用空列表。使用具名 `RateLimited` 控制执行率，使用 `WithoutOverlapping` 和稳定业务键协调多个 Worker。双方共用框架 Queue 和 Cache，接收包保留原有租约与记录格式。 [固定 Laravel 队列中间件装配](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php)

中间件释放当前原生任务时，接收记录保持原状态，处理代次和领域尝试记录不消耗。原生队列的预留、释放和尝试上限继续按自身契约处理。锁竞争与已验签接收限流分别发生在 Worker 和 HTTP 入口。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 用共享数据库缓存，PostgreSQL 与 MySQL 另外用同机 Redis 8.10.3，均用两个独立应用目录、真实 HTTP、Artisan、配置与路由缓存及每组合六次独立 Worker 验证。具名限流使第二个任务延期。应用锁跨 Worker 排除不同接收 ID 的竞争，并在首个处理完成并解除锁后允许后续任务。被限制的任务仍留队，处理代次为零，解除限制后完成。任务测试先按接收 ID 选定具体队列行，再读取释放时间，避免把正持有锁的任务误计入。SQLite 接收全量回归、包契约、严格 Clippy、格式及三语文档检查通过。
