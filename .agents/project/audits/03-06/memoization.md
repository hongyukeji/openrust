# 03-06 补验 · 缓存记忆化

对照 Laravel v13.40.0 的 `CacheManager::memo` 与 `MemoizedStore`，复用旧版 `6d1a8192` 的命中及未命中快照、写入版本和清空代际，以及写入、清空与延迟读取交错的用例。 [Laravel Cache Memoization](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#cache-memoization) · [Laravel MemoizedStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/MemoizedStore.php)

- `Cache::memo(None)` 或 `Cache::memo(Some("redis"))` 使用实际仓储标识，在应用容器的生命周期内复用同一装饰器。批量读取只查询没有记忆化结果的键。经该仓储写入会使对应键失效，清空会使所有内存结果失效，锁由底层仓储提供。
- Rust 在记忆化状态锁之外执行后端 I/O，以版本和代际防止异步读取在写入或清空后恢复旧结果。框架生命周期外每次调用返回独立装饰器。已结束作用域的仓储读取直接访问后端。标签装饰器与 `flushStale()` 转发见[补验](#标签仓储的记忆化与过期清理)。 [固定 Laravel memo 标签测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Cache/MemoizedTaggedCacheTest.php#L225)
- macOS arm64、Rust 1.94.0、Redis 8.10.3。4 项锁定离线测试通过，覆盖版本失效、清空、作用域结束、锁转发、实际 TCP HTTP 与 Redis、实际 Artisan `queue:work` 连续消费两项工作。HTTP 和任务每次均读到前一生命周期写回的值。专项 Clippy、工作区格式与差异检查通过。
- 三语 Cache 文档补充 `Cache Memoization` 小节。VitePress 构建及 12 项通用浏览器检查通过。

## 标签仓储的记忆化与过期清理

- 对照固定 Laravel v13.40.0 的 `MemoizedStore::tags` 和 `MemoizedTaggedCache`。旧版 `6d1a81924` 的 `memo.rs` 和回归测试已覆盖快照、写入版本与清空代际，本批复用当前实现的这些机制。Rust 保留 `TaggedCache` 的公开返回类型，使用共享状态按完整有序标签列表隔离命中和未命中。Redis 同一作用域的记忆化键保持稳定，其余标签存储沿用标签命名空间。 [固定 Laravel 实现](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/MemoizedTaggedCache.php) · [固定 Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Cache/MemoizedTaggedCacheTest.php)
- `flush()` 清空选定标签的后端数据，并使同一仓储的所有记忆化标签组失效。标签视图的 `clear()` 走整库清理，已有标签视图也会失效。 `flushMemoized()` 只清除该组的记忆化结果。Redis `flushStale()` 转发所选标签的过期索引清理，保留有效值与记忆化结果。记忆化命中无额外读取事件，整库清理只派发一对清理事件。
- 隔离基线 `7b329b8a6` 使用 Redis 8.10.3。两次实际 TCP HTTP、Artisan `queue:work` 两项真实消费、单机 Redis 与 Array 两个集成用例通过。验证同标签快照复用、标签顺序、命中与未命中、失败 `add` 后重新读取、普通 Redis 标签清空后的快照、重叠及无关标签视图失效、整库清理、Array 代际切换及事件。三项状态回归验证写入与清空和在途读的交错。
- `sh tests/redis_cluster.sh` 五项通过。由记忆化标签仓储转发所选标签清理，Redis `ZCARD` 确认相邻标签索引保留到存储级清理。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式与严格 Clippy、三语站点及 18 项浏览器检查通过。核心账本维持 62/66。临时 Redis 与本批编译和文档产物已回收，磁盘可用空间净增 13.44 GB（12.52 GiB），剩余 101.80 GB（94.81 GiB）。提交前周额度剩余 41%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-06-memo-tags.PmA3xF0j`。
