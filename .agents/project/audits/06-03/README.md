# 06-03 任务派发与队列存储

- 对照固定 Laravel 的任务创建、派发、事务边界、SyncQueue、DatabaseQueue、RedisQueue 和预约脚本。参考固定 Goravel 的队列 Provider 与编译任务登记。复用 `v0.x` 的预约过期保护、Redis 服务端时钟和 MySQL `SKIP LOCKED`，移植延迟与预约回归路径。 [Laravel 队列文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md) · [Laravel DatabaseQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/DatabaseQueue.php) · [Laravel Redis Lua](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/LuaScripts.php) · [Goravel Queue](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/queue)
- 应用级 QueueManager 缓存命名连接并持有处理器注册表。版本化 JSON 保存任务名、UUID、数据和运行选项。sync 即时执行。SQLite 通过单条原子更新预约，MySQL 通过锁定选取预约。Redis 在 Lua 中迁移到期任务、预约、确认及释放，JSON 数据原文独立保存。阻塞领取使用独立 Redis 连接和有界等待轮询，其他命令可继续执行。
- 预约以尝试次数和到期时间校验，旧执行者不能确认后继预约。数据库释放按 Laravel 重新插入待领取记录并保留尝试次数。相对延迟使用后端时钟，绝对延迟使用给定时间点。
- 明确传入活动事务后，`after_commit` 等到最外层提交，回滚丢弃待派发任务。立即入队时，同一数据库连接的存储写入复用该事务，sync 处理器可通过 JobContext 访问同一事务。Rust 显式登记处理器并传递事务句柄。PHP 序列化、Bus 派发 DSL、自定义连接工厂和其他队列驱动仍待实现。Worker、失败记录、任务中间件及队列监听器接线归后续单元。
- 隔离基线 `12e3f47e`。Rust 1.94.0 格式、469 项锁定离线测试、严格 Clippy 和 `./artisan inspire` 通过。SQLite、真实 MySQL 8.4 和 Redis 8 的 HTTP 验收覆盖入队、并发领取、过期重领、旧预约拒绝、释放、延迟、确认、事务提交及回滚。Redis 额外验证阻塞唤醒与 64 位整数载荷完整性。实际 Artisan 在 SQLite 与独立 MySQL 验收库验证了默认 `jobs` 迁移的创建和回滚。
- 三语正文同一示例从文档抽取，经实际 HTTP 在三种驱动上运行。VitePress 构建、12 项通用浏览器检查与 6 项队列页检查通过。
