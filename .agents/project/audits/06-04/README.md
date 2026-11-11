# 06-04 Worker、失败任务与队列中间件

- 核对固定 Laravel Worker 的尝试次数、退避、超时、停止条件、失败记录、手工重试、队列监听器和 Context。参考固定 Goravel 队列启动装配。复用 `v0.x` 的尝试隔离、超时取消、失败重试、队列监听器和实际 Worker 验收路径。 [Laravel Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) · [Laravel RetryCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RetryCommand.php) · [Laravel Context](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/context.md) · [Goravel Queue](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/queue)
- 实现 `queue:work` 的单次、空队列退出、数量、时间、重试和超时选项。SIGTERM 结束领取等待，并让正在执行的尝试完成。每次尝试使用独立容器与 Facade 作用域，Rust 超时取消当前 Future。可通过 JobMiddleware 继续、短路或释放任务。WithoutOverlapping 复用原子缓存锁。
- 默认迁移创建 `failed_jobs`。支持数据库 UUID 失败存储、禁用失败存储、列表、重试、删除和清理。失败先保存记录再确认预约。手工重试保留原任务 UUID，生成新的派发标记，入队后仅移除原失败记录。跨存储的手工重试保持至少一次语义。
- 类型化队列监听器用编译期登记的稳定 ID 与 JSON 编解码。入队前捕获公开和隐藏 Context，每次尝试恢复并运行 hydrated 回调。dehydrating 回调可修改待序列化副本。PHP 任务对象序列化、Worker 进程级硬终止、queue:restart、失败钩子和其他内置中间件仍有差异。自行派生的任务需由应用管理其取消。
- 隔离基线 `063b0071`。Rust 1.94.0 格式、472 项锁定离线工作区测试、严格 Clippy、`./artisan queue:work --help` 和 `./artisan inspire` 通过。SQLite、真实 MySQL 8.4、Redis 8 均通过 HTTP 派发与独立 Artisan Worker 的重试、超时、失败记录、手工重试、SIGTERM 收尾、监听器事务边界和 Context 隔离。MySQL 独立验收库复验迁移、回滚与 fresh。
- 三语 Context 示例从正文抽取并在上述三种 Worker 路径运行。VitePress 构建、12 项通用浏览器检查及 6 项队列和 Context 专题检查通过。
