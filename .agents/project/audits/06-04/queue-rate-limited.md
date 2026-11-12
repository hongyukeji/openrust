# 06-04 队列限流中间件

## 基线

核对固定 Laravel 的 [Rate Limiting](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#rate-limiting)、[RateLimited](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Middleware/RateLimited.php)、[RateLimitedWithRedis](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Middleware/RateLimitedWithRedis.php) 和 [缓存测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/RateLimitedTest.php)、[Redis 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/RateLimitedWithRedisTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的运行边界。v0.x（`6d1a8192`）未发现对应中间件。

## 实现与 Rust 适配

- `RateLimiter::forJob` 注册接收 `Payload` 的限流策略。HTTP 请求的类型化策略继续通过既有 `r#for` 注册。两个注册表都由应用容器拥有，可用相同策略名称。
- `RateLimited` 使用现有缓存计数器和锁一次检查并扣减限额。Redis 版本复用 `DurationLimiter` 的原子命令，并以策略为单位给多键配置相同的 Redis Cluster 哈希标签。队列计数键与 HTTP 及手动计数分开。Cluster 实机未纳入本批验收。
- 找不到策略、无限制或空限额时继续执行。达到限额时默认按剩余窗口加三秒释放，支持相对或绝对的 `releaseAfter` 和 `dontRelease`。后者确认任务且跳过处理器。任务载荷仍按现有编译型注册方式处理，PHP 对象序列化钩子不适用。

## 验收

- SQLite、MySQL 8.4.13、Redis 8.10.3 使用实际 HTTP 派发与独立 Artisan 验证按载荷键限流、同名 HTTP 与队列策略独立、命中后延迟释放并二次执行、`dontRelease`、缺失和无限制策略。
- Redis 的两个独立 Worker 同时消费同一策略的两条任务，多键限额下仅一条进入处理器，另一条按策略确认。
- Rust 1.94.0 格式、锁定离线严格 Clippy 和串行工作区测试通过，日志合计 575 项通过、6 项忽略，含独立进程夹具。提取三语 Rust 示例编译并通过实际 sync 队列执行。文档构建与 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#rate-limiting) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#rate-limiting) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#rate-limiting)
