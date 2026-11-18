# 06-03 生命周期与故障转移驱动

固定 Laravel v13.40.0 的 Null 丢弃任务，Deferred 在生命周期结束时运行 sync，Background 通过进程驱动执行，Failover 按连接顺序发布并从首选连接预约。核对旧版 `6d1a8192`，未找到可直接移植的四个驱动。复用当前 sync 队列、终止回调、进程控制及应用级连接缓存。 [固定 Laravel 队列驱动](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue)

- 实际 HTTP 验证 Null 丢弃、失败响应不执行 Deferred、Deferred 使用原进程、Background 使用另一个 Artisan 进程。实际 Artisan 验证成功命令结束后运行 Deferred。MySQL 8.4.13 与 Redis 8.10.3 各运行实际 Worker，覆盖主连接成功、MySQL 队列表故障后 Redis 接管及 Worker 成功尝试后执行 Deferred。
- Failover 失败时按顺序尝试，读取与预约委托首选连接。持久后备连接需要各自的 Worker。写入已提交但确认失败时，故障转移存在重复投递窗口。Background 在发起进程退出后仍可继续执行。Deferred 和 Background 支持共享缓存唯一锁。其他平台的真实进程验收另记。
- 任务载荷定稿为版本化 JSON 数据和编译注册的处理器名称。跨进程恢复依赖同一名称及兼容字段，旧版 PHP 对象或闭包字节流采用各自运行时处理。定稿范围不要求 PHP 字节流互读。
- Beanstalkd 和 SQS 独立跟踪。分别使用真实 Beanstalkd 与 SQS 验证派发、延迟、预约、释放、失败和重投递。SQS 另核对可见性超时和 FIFO 行为。

Rust 1.94.0 锁定离线驱动专项和完整 `queue_hard_timeout` 进程回归通过。后者覆盖 SQLite、MySQL、Redis、独立 Artisan 与实际 `main` 的 Runner。格式、专项严格 Clippy 和三语站点构建通过。


## Background 脱离式运行

对照固定 Laravel ProcessDriver 的生命周期回调与脱离式启动。Rust 复用同一二进制的 Artisan 任务入口，使用临时文件句柄传递编译任务名、JSON 参数和 Context，保留应用目录和选定的环境。Unix 使用独立会话，Windows 使用 detached process 与新进程组。子进程错误交给本应用报告器。 [固定 Laravel ProcessDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/ProcessDriver.php)

macOS ARM64 和 Rust 1.94.0，MySQL 8.4.13 与 Redis 8.10.3 驱动专项通过。真实 Artisan 发起进程退出后才解除子任务的等待，子任务随后写入完成标记。专项也复验 HTTP 生命周期、Deferred 与实际 Worker 故障转移。Windows、Linux 实机留在发布验收范围。

专项严格 Clippy、Rust 格式、三语站点构建及 12 项浏览器检查通过。

## Deferred／Background 唯一任务

固定 Laravel 的 DeferredQueue、BackgroundQueue 复用同步执行路径。本批保留应用缓存中的所有者锁，借用旧版队列的重复派发和终态释放契约。Deferred 回调成功或报错后结算，Background 将锁随 JSON 交给独立进程。失败响应跳过任务和子进程启动失败会释放锁。若进程异常退出且无法执行清理，由租期回收。 [Laravel DeferredQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/DeferredQueue.php) · [Laravel BackgroundQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/BackgroundQueue.php)

macOS ARM64、Rust 1.94.0，实际 HTTP、独立 Artisan、MySQL 8.4.13、Redis 8.10.3 专项通过。覆盖重复请求返回 409、失败响应跳过后释放、正常完成后释放、在生命周期外登记失败后可重新取得锁。真实 Worker 复验 Deferred 与 MySQL 到 Redis 的故障转移。三语文档构建、12 项浏览器检查、专项严格 Clippy 与格式检查通过。

## Windows ARM64 Background 输出管道诊断

在 `9ab6b416c` 的 Windows 11 ARM64 原生 `main.exe` 上，从真实 Artisan 向 `background` 连接派发。子进程正确收到 stdin 载荷和公开、隐藏 Context，父进程以 0 退出。子进程等待闸门时，调用方仍无法从父 stdout 和 stderr 捕获到 EOF，放行并等子进程退出后管道才关闭。保留此项待修复，下一批验证只继承任务输入和自身所需句柄。 [原生验收环境与记录](../07-04/README.md#windows-arm64-concurrency-延后执行与管道)
