# 06-03 唯一任务

核对固定 Laravel 的缓存锁和 Worker 释放点。旧版 `6d1a8192` 在 SQL 和 Redis 队列驱动内保存唯一键。本次保留旧版按任务类型与标识去重的契约，按固定 Laravel 使用应用缓存原子锁，并在载荷保存所有者供独立 Worker 安全释放。 [Laravel UniqueLock](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/UniqueLock.php) · [Laravel CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php)

Rust 的注册处理器通过 `uniqueJob` 返回类型化策略。活动事务在提交时取得锁，显式要求立即入队时返回配置错误。普通重试保留锁，最终完成或失败后释放。处理前唯一在中间件交给处理器时释放。旧任务载荷继续可读。直接调用底层驱动和批次任务使用原有派发规则。

macOS ARM64、Rust 1.94.0，MySQL 8.4.13 和 Redis 8.10.3 的真实 Artisan Worker 验证并发重复派发、重试期间保持锁、成功与最终失败释放、失败记录及处理前释放。两种队列使用共享 Redis 缓存锁。三语队列文档与 VitePress 构建通过。
