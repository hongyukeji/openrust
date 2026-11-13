# 06-02 延迟事件派发

对照固定 Laravel Dispatcher 的 `defer`。旧版 `6d1a8192` 未实现该接口。本次沿用当前派发器、监听器顺序及事务回调。Rust 用异步任务局部状态隔离请求，以派发器标识隔离应用。回调成功时按派发顺序重放，失败时丢弃，嵌套作用域先完成自身重放。显式传入的活动事务保持原有事务派发语义。 [Laravel Dispatcher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Events/Dispatcher.php)

- Rust 1.94.0 锁定离线测试使用实际 MySQL 8.4.13 和 SQLite，2 项通过。真实 HTTP 验证指定名称、立即派发、嵌套顺序、成功重放及回调失败丢弃，同时覆盖既有事务提交和回滚行为。
- 专项严格 Clippy、三语文档构建及 12 项通用浏览器检查通过。事件发现缓存和 Eloquent Observer 单独跟踪。
