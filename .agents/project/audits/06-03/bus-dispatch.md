# 06-03 Bus 派发

对照固定 Laravel Dispatcher 的派发、同步队列、直接执行、终止阶段执行、批量派发和处理器映射。复核旧版 `6d1a8192` 的事务派发及 Worker 路径，沿用当前 QueueManager 的 JSON 载荷、事务后回调和应用级处理器注册。 [固定 Laravel Dispatcher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Dispatcher.php)

- Rust 显式传递 `Payload`、派发选项和事务。Worker 在独立进程按稳定名称查找同一应用编译注册的处理器，并解析载荷数据。跨进程契约为 JSON 数据和编译注册，PHP 对象及闭包字节流没有互读契约。直接执行用 Bus 管道，同步执行沿用 sync 队列中间件和事件。
- `bulk` 按输入顺序逐项派发并返回各项结果。各项沿用唯一任务和事务契约。启用时，响应后派发使用 HTTP 终止阶段。关闭时同步执行。
- Rust 1.94.0、macOS ARM64。用真实 TCP HTTP 和独立 Artisan `queue:work` 验证直接执行、同步队列、响应后执行、普通派发及批量派发，MySQL 8.4、Redis 8 和 SQLite 均通过。三语文档构建和专项严格 Clippy 通过。
