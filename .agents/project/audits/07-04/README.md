# 07-04 Concurrency

依赖 07-03。对照固定 [Laravel Concurrency](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/concurrency.md)、[ProcessDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/ProcessDriver.php)、[SyncDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/SyncDriver.php)、[ConcurrencyTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Concurrency/ConcurrencyTest.php) 和 [ProcessDriverSecurityTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Concurrency/ProcessDriverSecurityTest.php)。Provider 沿用应用编译型装配，参考固定 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)。旧版 `6d1a8192` 无 Concurrency 模块，核对 `foundation/lifecycle.rs` 及 SQLite 迁移锁的真实子进程夹具，使用 07-03 已验收的 Process 池、进程组和取消生命周期。

默认 `process` 驱动把任务交给同一应用的隐藏 Artisan 命令，每个任务一个子进程。Rust 用编译期注册的处理器名称和类型化输入表达 Laravel 的闭包传输。请求 JSON 经 stdin 传输，上限 8 MiB。每个任务使用独立随机 HMAC 密钥，父进程只解析签名匹配的结果帧。传递公开和隐藏的 Context，沿用应用的 dehydration 和 hydration 钩子。保留位置和命名结果顺序、收集子进程后返回单任务错误、单任务超时和取消清理。`sync` 在本进程顺序执行。

HTTP `defer` 在响应体交给传输层后进入现有终止队列。状态码低于 400 或显式 `always()` 时运行。延后操作在登记时捕获 Context，终止阶段持有受管子进程直至完成。Rust 使用类型化任务、JSON 结果和错误消息表达 Laravel 的 PHP 闭包及异常传输。并行运行使用受管进程，`sync` 提供顺序运行。Laravel 的 `fork` 驱动依赖 PHP CLI 和 `spatie/fork`。

## 验收

- Rust 1.94.0、macOS Apple Silicon。锁定离线工作区 512 项通过，4 个忽略项由集成测试作为子进程启动。格式及严格 Clippy 通过。首次全量在密码重置 HTTP 用例暴露调试构建栈溢出，调整请求 Future 的装配边界后，失败用例及全量均重跑。
- 四项定向测试覆盖独立子进程 PID、结果顺序、带伪帧的输出、失败时同批任务完成、超时和取消清理、sync 和真实 HTTP 终止阶段的 defer、always 与 Context。
- 独立编译的入口通过真实 HTTP 启动默认 process 驱动，验证两个独立工作 PID、顺序结果和公开及隐藏 Context。隐藏命令未出现在 Artisan list。临时 SQLite 的 migrate、inspire、首页及 /up 通过。
- 三语三个 Rust 示例实际运行，VitePress 构建、12 项通用浏览器检查及 6 项 Concurrency 页面导航通过。

## Artisan 与队列延后执行

- 固定 Laravel 在 `CommandFinished` 按退出码筛选回调，在 `JobAttempted` 按本次尝试是否失败或抛出异常筛选回调。无异常的主动释放算作成功。`sync` 与 `deferred` 队列连接沿用外层生命周期。执行回调期间新登记的回调继续排空，并沿用同一结束状态。 [固定终止监听器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Providers/FoundationServiceProvider.php#L195-L207) [固定 JobAttempted](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/JobAttempted.php#L25-L28) [固定回调集合](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Defer/DeferredCallbackCollection.php#L43-L58)
- 在 Rust 中，硬超时隔离进程交回已登记的类型化任务及其 Context 和 `always()` 标记。父 worker 确定结果并派发 `JobAttempted` 后再执行。子进程在交回前被终止时，登记随进程退出丢弃。任意 Rust Future 仍由其登记进程持有。Artisan 程序化调用按命令状态执行，在同应用的 HTTP 请求中沿用请求边界。受管进程沿用应用根目录及 `--env` 选择。
- Apple Silicon macOS 上编译隔离应用，用实际 HTTP 验证 200 响应先交付、400 仅执行 `always()`。用实际 Artisan 验证退出码 0／7、任务失败报告和后续回调继续执行、隐藏命令不进入列表、公开与隐藏 Context、`--env` 路径、`sync` 及 `deferred` 连接中的嵌套延后任务。
- 独立 Redis 8 和真实 `queue:work redis --once` 验证普通和硬隔离路径的成功、主动释放、失败与 `always()`。通过父 worker 写出的 `JobAttempted` 标记验证先后，用 1.25 秒任务预算和 1.7 秒延后操作验证两者独立。硬超时杀死任务子进程时无延后操作启动，验收记录的任务及工作 PID 均退出。
- Rust 1.94.0 锁定离线运行 `cargo test --locked -p openrust-framework --test concurrency --test processes --test http_client`，32 项通过，其中另有一个忽略的 Artisan 夹具由测试作为子进程执行。工作区全目标严格 Clippy 和变更文件格式通过。工作区格式检查仍报告基线 Sanctum 测试的排版差异。三语 VitePress 构建、12 项通用浏览器检查、两种部署路径共 6 项 Concurrency 页面检查通过。
- Redis 容器使用 tmpfs，验收后已回收。隔离应用、文档构建、探针输出及验收可执行文件已删除。项目前后可用空间为 75,348,000 KiB 和 75,574,676 KiB，净增 226,676 KiB。共享 Cargo 缓存保留。

## 整单元验收

前置 07-03 已闭合。默认驱动、顺序与命名结果、任务失败、单任务超时、取消和子进程回收，及 HTTP、Artisan、队列三类延后执行边界均通过验收。07-04 核心单元闭合。当前实机验收在 macOS 完成，其他平台的原生运行按发布矩阵跟踪。

下一单元 07-05 Images，依赖 03-01 和 04-02。
