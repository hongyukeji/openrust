# 07-04 Concurrency

依赖 07-03。对照固定 [Laravel Concurrency](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/concurrency.md)、[ProcessDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/ProcessDriver.php)、[SyncDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/SyncDriver.php)、[ConcurrencyTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Concurrency/ConcurrencyTest.php) 和 [ProcessDriverSecurityTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Concurrency/ProcessDriverSecurityTest.php)。Provider 沿用应用编译型装配，参考固定 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)。旧版 `6d1a8192` 无 Concurrency 模块，核对 `foundation/lifecycle.rs` 及 SQLite 迁移锁的真实子进程夹具，使用 07-03 已验收的 Process 池、进程组和取消生命周期。

默认 `process` 驱动把任务交给同一应用的隐藏 Artisan 命令，每个任务一个子进程。Rust 用编译期注册的处理器名称和类型化输入表达 Laravel 的闭包传输。请求 JSON 经 stdin 传输，上限 8 MiB。每个任务使用独立随机 HMAC 密钥，父进程只解析签名匹配的结果帧。传递公开和隐藏的 Context，沿用应用的 dehydration 和 hydration 钩子。保留位置和命名结果顺序、收集子进程后返回单任务错误、单任务超时和取消清理。`sync` 在本进程顺序执行。

HTTP `defer` 在响应体交给传输层后进入现有终止队列。状态码低于 400 或显式 `always()` 时运行，工作失败通过应用异常处理器报告。登记时捕获 Context，终止队列继续持有任务直到完成。Laravel 的 PHP 闭包序列化、异常类型重建、CLI `fork` 驱动和非 HTTP defer 尚无对等实现。Rust 工作线程不直接复制多线程运行时。进程子任务继承实际环境和应用根目录，父进程 CLI 的 `--env` 选择未单独传输。

## 验收

- Rust 1.94.0、macOS Apple Silicon。锁定离线工作区 512 项通过，4 个忽略项由集成测试作为子进程启动。格式及严格 Clippy 通过。首次全量在密码重置 HTTP 用例暴露调试构建栈溢出，调整请求 Future 的装配边界后，失败用例及全量均重跑。
- 四项定向测试覆盖独立子进程 PID、结果顺序、带伪帧的输出、失败时同批任务完成、超时和取消清理、sync 和真实 HTTP 终止阶段的 defer、always 与 Context。
- 独立编译的入口通过真实 HTTP 启动默认 process 驱动，验证两个独立工作 PID、顺序结果和公开及隐藏 Context。隐藏命令未出现在 Artisan list。临时 SQLite 的 migrate、inspire、首页及 /up 通过。
- 三语三个 Rust 示例实际运行，VitePress 构建、12 项通用浏览器检查及 6 项 Concurrency 页面导航通过。

下一单元 07-05 Images，依赖 03-01 和 04-02。
