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

在 `9ab6b416c` 的 Windows 11 ARM64 原生 `main.exe` 上，从真实 Artisan 向 `background` 连接派发。子进程正确收到 stdin 载荷和公开、隐藏 Context，父进程以 0 退出。子进程等待闸门时，调用方仍无法从父 stdout 和 stderr 捕获到 EOF，放行并等子进程退出后管道才关闭。修复及复验见下文。 [原生验收环境与记录](../07-04/README.md#windows-arm64-concurrency-延后执行与管道)

## Windows ARM64 Background 输入与父输出分离

- 对照固定 Laravel 的 BackgroundQueue 通过进程并发驱动在生命周期结束时调度同步队列。保留现有 JSON stdin、Context 与独立 Artisan 契约。v0.x（`6d1a81924`）未发现 Windows 句柄选择的旧实现。 [BackgroundQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/BackgroundQueue.php#L17)
- 复用调度的脱离进程创建，将其移入 Process 模块。带输入时，Windows 使用 `STARTUPINFOEXW` 的句柄清单，只传递任务输入文件和承接子进程 stdout／stderr 的 `NUL`。没有输入时仍禁止继承句柄。两个返回的进程句柄以及临时输入和属性列表均按所有权释放。 [Windows 句柄清单](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
- 从 `b28aa86bd` 干净归档加核定改动，在 Windows 11 ARM64 以 Rust 1.94.0、MSVC 14.44.35207 构建，PE Machine 为 `0xAA64`。用真实 `main.exe artisan probe:background --env=probe` 派发，发起进程退出 0，stdout 和 stderr 均到 EOF 后，独立子进程仍在等待闸门。放行后任务完成并退出 0。核对 stdin 载荷、公开和隐藏 Context、显式环境文件、工作目录及含空格和 Unicode 的路径。实际 `Concurrency::defer`、`always()`、HTTP 204 和 `artisan inspire` 通过。
- Windows 原生 `concurrency` 5 项、Windows 启动回归 2 项通过。带 stdin 的用例校验超过 128 KiB 的负载。macOS 调度和并发套件共 22 项通过，实际 Background 同样验证两管道 EOF 与载荷送达。Rust 1.94.0 格式、macOS 锁定离线全目标严格 Clippy、三语构建和 18 项 Chrome 检查通过。来宾夹具及专属产物已回收，虚拟机恢复停止。磁盘可用 55.87 GB（52.03 GiB），提交前周额度剩余 20%，按用户授权继续。日志和验收夹具位于 `/Users/Home/openrust-background-eof.qI6CrW6R`。

## 生命周期连接在终止回调捕获 Context

- 固定 Laravel 的 DeferredQueue 在终止时调用 SyncQueue，BackgroundQueue 在并发回调中派发同步任务。ContextServiceProvider 在生成队列载荷时脱水。v0.x 未提供同名生命周期驱动，保留既有公开／隐藏字段仓储契约。OpenRust 两种生命周期连接在终止回调读取来自派发作用域的最新仓储。持久连接及已发布任务沿用原有快照。 [DeferredQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/DeferredQueue.php#L16) · [BackgroundQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/BackgroundQueue.php#L17) · [ContextServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/Context/ContextServiceProvider.php#L37)
- Deferred 在现有应用作用域中临时覆盖 Context 并从载荷水合，保留原作用域的服务实例。Background 将同一快照交给 stdin 协议和任务载荷。派发跳过、事务回滚及启动失败沿用生命周期清理。
- 从 `93f27b6fd` 干净归档和核定改动，在 macOS ARM64 以 Rust 1.94.0 锁定离线构建独立应用。真实 HTTP 204、独立 Artisan 及 `queue:work redis --once` 使用 Redis 8.10.3，验证两种连接都读取派发后替换的公开与隐藏字段、已删字段、脱水和水合钩子。Deferred 与发起命令同 PID 且使用同一个 Context 仓储，Background 在父 stdout／stderr 到 EOF 后仍可继续运行。HTTP 400 不执行脱水钩子或任务。Redis 持久队列继续读取生产者派发时的快照。
- 通过 `tests/mysql_acceptance.py` 使用 MySQL 8.4.13 临时实例，真实 Artisan 验证 `after_commit` 在提交后才登记，两种连接读取直到生命周期结束的更新。回滚不脱水、不启动任务。实际 `artisan inspire`、格式、锁定离线全目标严格 Clippy、三语构建和 18 项 Chrome 检查通过。本批实测在 macOS。已回收服务和本批专属产物，保留数据卷。另回收 8 个已提交批次的可重建源码副本，保留原始夹具与日志。磁盘可用 52.59 GB（48.98 GiB），提交前周额度剩余 19%，按用户授权继续。本批夹具和日志位于 `/Users/Home/openrust-lifecycle-context.yKqe5Ifw`。
