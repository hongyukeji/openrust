# 06-04 任务中间件与异常失败

固定 Laravel 的 Queue Middleware 目录包含 `FailOnException`、`RateLimited`、`RateLimitedWithRedis`、`Release`、`Skip`、`SkipIfBatchCancelled`、`ThrottlesExceptions`、`ThrottlesExceptionsWithRedis` 和 `WithoutOverlapping`。后八个已有框架实现，本批接通 `FailOnException`。旧版 `6d1a8192` 未提供同名实现，本批沿用现有 Worker 失败结算和隔离进程信号。 [Laravel FailOnException](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Middleware/FailOnException.php)

- Rust 支持类型匹配和接收异常与载荷的谓词。匹配时标记本次尝试为最终失败，原错误继续交给 Worker，统一完成失败记录和回调。隔离子进程通过现有执行结果返回失败标志。
- macOS ARM64、Rust 1.94.0。真实独立 Artisan `queue:work` 在 MySQL 8.4、Redis 8 和 SQLite 上验证匹配异常在首次尝试进入失败记录、执行回调，普通与隔离模式均通过。未匹配异常正常重试并在下一次成功。三语站点构建和专项严格 Clippy 通过。
