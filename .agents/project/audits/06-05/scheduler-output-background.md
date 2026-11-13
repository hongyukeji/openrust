# 06-05 调度输出、URL 探测与后台完成

对照固定 Laravel 的任务输出、任务钩子、后台任务及完成命令。旧版 `v0.x` 的 `schedule.rs` 主要负责按计划派发队列任务和选举，其日历、协调测试未覆盖这批接口。当前实现复用已有调度钩子、HTTP Client、Artisan 入口及缓存锁。 [Task Output](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#task-output) · [Task Hooks](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#task-hooks) · [ScheduleFinishCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleFinishCommand.php)

- `sendOutputTo` 与 `appendOutputTo` 接收应用相对或绝对路径，捕获命令 stdout、stderr 和非零退出时的输出。外部进程写入同一个文件句柄，Artisan 按当前缓冲输出的 stdout、stderr 顺序写入。输出钩子接收字节切片，指定文件时读取完整文件。三个钩子按登记顺序运行，可选择跳过空输出。
- URL 探测作为同一钩子链执行。前置、完成、成功及失败均有条件变体。请求和 HTTP 错误交给应用报告器，任务继续。
- 后台任务由同一个二进制的独立 Artisan 进程执行。父进程运行前置钩子，子进程运行命令及后置钩子。子进程用原持有者令牌释放锁，再派发完成事件。任务退出非零和后置钩子错误都会报告。父进程启动失败时负责释放锁。跨进程互斥要求共享缓存，强制终止或进程启动后引导失败时由锁的过期时间兜底。
- 本批闭合账本登记的文件输出、输出钩子、URL 探测及后台事件与锁释放。邮件发送输出、`schedule:clear-cache` 和 `schedule:test` 的后续实现及验收见下文。后台进程实测在 macOS ARM64。

## 实际验收

- 在独立应用二进制运行 `artisan schedule:run`。预置文件验证覆盖和追加，Artisan 验证重定向，系统命令验证混合 stdout／stderr、非零退出和失败钩子内容。无输出任务验证条件跳过。
- 本地 HTTP 接收端核对前置、完成、失败及成功的 GET 顺序，条件为假时无请求。完成探测返回 503，异常报告器收到错误，后续任务照常执行。
- 使用共享文件缓存运行独立后台命令，父 `schedule:run` 返回时任务仍在等待。第二个调度进程触发重叠跳过。放行后验证成功退出 0、命令非零退出 7、后置钩子失败退出 1 均触发后台完成事件。重复执行验证锁释放、输出追加与钩子顺序。
- 三语部署站点构建通过，根路径与 `/docs/` 共 12 项浏览器检查通过。
- macOS ARM64，Rust 1.99.1 的锁定离线工作区测试通过，汇总 596 项通过、7 项夹具忽略，含 SQLite 队列硬超时进程验收。Rust 1.94.0 工作区格式、全目标全特性严格 Clippy 和 13 项调度测试通过。Rust 1.99.1 的严格 Clippy 另报分页和 Redis 限流器中两处既有代码告警。

## 输出邮件、清锁与手工运行

- `emailOutputTo` 和 `emailWrittenOutputTo` 复用输出钩子并只发送非空输出。 `emailOutputOnFailure` 在命令失败时发送，即使输出为空。通过应用邮件器组装文本邮件。 [固定 Laravel Event](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/Event.php)
- `schedule:clear-cache` 遍历已注册任务并强制释放现存重叠锁。 `schedule:test --name` 选择唯一匹配的命令或回调，前台执行，跳过日历筛选且保留任务重叠检查和钩子。交互终端可按序号选择。 [ScheduleClearCacheCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleClearCacheCommand.php) · [ScheduleTestCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleTestCommand.php)
- Rust 1.94.0 实际 Artisan 进程验收 15 项通过。邮件使用应用日志邮件器验证成功内容、失败内容与收件人，以及成功时不发失败邮件。真实 Redis 8.10.3 验证跨进程清理、重复执行和后续可重新取得锁。手工运行验证非计划时间执行和未知命令提示。三语站点构建及 12 项浏览器检查通过。
