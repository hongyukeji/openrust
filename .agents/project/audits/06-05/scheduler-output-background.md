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

## Windows ARM64 后台调度与 Redis 互斥

- 固定 Laravel 在 Windows 通过后台命令转交退出码，再由 `schedule:finish` 完成任务和派发事件。OpenRust 直接创建同一应用二进制的独立进程，保留前后置钩子、退出码、锁释放和完成事件的顺序。旧版 v0.x（`6d1a81924`）可参考其进程间协调验收，未提供可复用的后台完成钩子。 [CommandBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/CommandBuilder.php#L42) · [ScheduleFinishCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleFinishCommand.php#L37)
- 从 `012c21d86` 干净归档在 Windows 11 ARM64 用 Rust 1.94.0、MSVC 14.44.35207 锁定离线构建，调度用例和独立应用均核对 PE Machine `0xAA64`。以 `OPENRUST_REQUIRE_REDIS=1` 在真实 Redis 8.10.3 上运行 `cargo test -p openrust-framework --test scheduling` 的原生测试二进制，覆盖 Redis 重叠锁、单节点选举、打断、暂停、维护及清锁。Redis 运行在宿主机隔离容器，经虚拟机私有网络连接。
- 在归档内的应用路由添加仅用于验收的命令及事件订阅，用 `cargo build --bin main --features standalone` 构建并在带空格的独立目录执行 `main.exe artisan schedule:run`。文件和 Redis 缓存均验证父命令先退出、子进程 PID 与父进程不同、重叠执行被阻止、输出追加。实测子进程退出码 0、7、1 对应成功、命令失败、后置钩子失败。监听器查询同一互斥锁，确认事件派发时锁已释放。随后重复执行均可再次领取锁。独立 `artisan inspire` 也通过。
- Rust 1.94.0 格式、锁定离线全目标严格 Clippy、三语构建和 18 项 Chrome 检查通过。已回收本批容器、来宾夹具和专属产物，虚拟机恢复停止。磁盘可用 70.77 GB（65.91 GiB），提交前周额度剩余 20%，按用户授权继续。夹具与原始日志位于 `/Users/Home/openrust-schedule-arm.JrFz6PWK`。
- Windows 套件 16 项通过，两个子进程夹具按设计在常规枚举中忽略。外部命令夹具从 `/bin/sh` 改为原生测试二进制，验证带空格的单个参数与退出码 7，macOS 对应用例也通过。
- 该次验证发现 Windows 重定向管道的 EOF 差异。用 .NET 分别等待 PID 退出和 stdout／stderr EOF，在后台任务仍被测试闸门挂起时，父进程已退出，但捕获端读不到 EOF。重叠命令未生成子进程时，捕获端正常收到 EOF。验收脚本基于进程退出状态判断父进程完成，这个管道差异单独跟踪。

## Windows 后台调度关闭父输出管道

- 在基线 `aeede8cc7` 上复用上一批进程退出与管道 EOF 独立观察的夹具。固定 Laravel 在 Windows 通过后台命令完成退出码转交与 `schedule:finish`，OpenRust 保留同一应用二进制、钩子和原持有者令牌。旧版 v0.x 的进程间协调测试可参考，未找到脱离句柄继承的实现。 [固定 Laravel CommandBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/CommandBuilder.php#L42)
- Windows 使用既有 `windows-sys` 的 `CreateProcessW` 创建脱离控制台的后台进程，禁止继承父句柄。按 Windows 的参数规则编码命令行，传递 Unicode 环境、配置目录及 `--env`，并关闭启动返回的两个句柄。进程创建失败继续由父进程释放锁。 [CreateProcessW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw)
- Windows 11 ARM64，Rust 1.94.0、MSVC 14.44.35207，原生 PE Machine `0xAA64`。新增的 `scheduler_windows_background` 定向回归通过，覆盖父 stdout 与 stderr EOF、子进程继续运行、包含空格和引号的参数、末尾反斜杠、Unicode 路径和继承环境以及 `APP_BASE_PATH` 的不区分大小写覆盖。原有调度套件 16 项通过，2 个子进程夹具按设计忽略。
- 从带空格的可执行文件路径运行 `main.exe artisan schedule:run --env=pipe-test`，应用目录包含空格和非 ASCII 字符。文件缓存及私有网络 Redis 8.10.3 均验证父进程退出、捕获端读取两条管道 EOF 后，后台进程仍等待放行。验证子进程退出码 0、7、1，输出追加、同一锁阻止重入、完成事件派发前释放锁、后续可再次领取，以及选定环境文件和工作目录。独立 `artisan inspire` 通过。
- macOS 调度套件 17 项通过，2 个夹具按设计忽略。独立 `schedule:run` 实测进程退出、管道 EOF、任务继续、重叠锁与完成事件通过。Rust 1.94.0 格式、macOS 锁定离线全目标严格 Clippy、三语构建和 18 项 Chrome 检查通过。已回收本批专属构建、容器和来宾夹具，Windows 虚拟机恢复停止。磁盘可用 68.65 GB（63.94 GiB），提交前周额度剩余 20%，按用户授权继续。原始日志和独立应用夹具位于 `/Users/Home/openrust-schedule-eof.9CtfV7UI`。
