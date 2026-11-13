# 03-06 补验 · 缓存记忆化

对照 Laravel v13.40.0 的 `CacheManager::memo` 与 `MemoizedStore`，复用旧版 `6d1a8192` 的命中及未命中快照、写入版本和清空代际，以及写入、清空与延迟读取交错的用例。 [Laravel Cache Memoization](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#cache-memoization) · [Laravel MemoizedStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/MemoizedStore.php)

- `Cache::memo(None)` 或 `Cache::memo(Some("redis"))` 使用实际仓储标识，在应用容器的生命周期内复用同一装饰器。批量读取只查询没有记忆化结果的键。经该仓储写入会使对应键失效，清空会使所有内存结果失效，锁由底层仓储提供。
- Rust 在记忆化状态锁之外执行后端 I/O，以版本和代际防止异步读取在写入或清空后恢复旧结果。框架生命周期外每次调用返回独立装饰器。已结束作用域的仓储读取直接访问后端。标签仓储另按缓存标签契约推进。
- macOS arm64、Rust 1.94.0、Redis 8.10.3。4 项锁定离线测试通过，覆盖版本失效、清空、作用域结束、锁转发、实际 TCP HTTP 与 Redis、实际 Artisan `queue:work` 连续消费两项工作。HTTP 和任务每次均读到前一生命周期写回的值。专项 Clippy、工作区格式与差异检查通过。
- 三语 Cache 文档补充 `Cache Memoization` 小节。VitePress 构建及 12 项通用浏览器检查通过。
