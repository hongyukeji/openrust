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

## Windows ARM64 Concurrency 延后执行与管道

- 固定 Laravel 的 process `defer` 从生命周期回调启动后台命令，Laravel BackgroundQueue 也经此派发。OpenRust 的公开 `Concurrency::defer` 在终止阶段监管子进程、读取签名结果并报告异常，HTTP 响应可先交付，独立 Artisan 则延至子进程结束后退出。只有队列的 `background` 连接使用脱离路径。该现象已由后续脱离运行批次调整，见下文。核对旧版 v0.x（`6d1a81924`），未发现可复用的 Concurrency defer。 [固定 Laravel ProcessDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/ProcessDriver.php#L98) · [BackgroundQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/BackgroundQueue.php)
- 从 `9ab6b416c` 的干净归档，在 Windows 11 ARM64 用 Rust 1.94.0 和 MSVC 14.44.35207 锁定离线构建。原生 PE Machine 为 `0xAA64`，`cargo test -p openrust-framework --test concurrency` 的 5 项用例通过，子进程夹具按设计忽略。macOS 同套件 5 项通过。
- 独立 `main.exe` 和应用目录均含空格，应用目录含非 ASCII 字符。通过实际 Artisan 验证子进程收到 stdin 中的任务参数、登记时的公开和隐藏 Context、所选 `--env` 环境及应用工作目录。子进程等待期间父进程仍运行，放行后 stdout 和 stderr 均到 EOF，父进程保持原退出码 0／7。退出 7 仅执行 `always()`，普通回调被跳过。实际 HTTP 返回 204 后放行子进程，验证响应无需等待子任务完成。独立 `artisan inspire` 通过。
- 同一夹具复现脱离式队列的 Windows 输出管道差异，单独留在 [06-03 验收记录](../06-03/lifecycle-failover-drivers.md#windows-arm64-background-输出管道诊断)。
- Rust 1.94.0 格式、macOS 锁定离线全目标严格 Clippy、三语构建及 18 项 Chrome 检查通过。回收来宾夹具和本批专属产物，虚拟机恢复停止。磁盘可用 67.69 GB（63.05 GiB），提交前周额度剩余 20%，按用户授权继续。夹具与日志位于 `/Users/Home/openrust-concurrency-arm.WYpUrEqM`。

## process defer 在终止阶段脱离运行

- 固定 Laravel 的 process `defer` 在生命周期回调中逐项启动后台进程，sync 则在回调中按顺序执行。旧版 v0.x（`6d1a81924`）无可复用实现。OpenRust 复用 06-03 Background 的独立 Artisan 和 stdin 启动。进程启动失败由发起应用报告，任务失败在各子进程报告。同批一项无法启动时仍尝试其余任务。需要等待或收取结果时仍使用 `run`。 [固定 Laravel ProcessDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/ProcessDriver.php#L98) · [SyncDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/SyncDriver.php#L25) · [Laravel Concurrency 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Concurrency/ConcurrencyTest.php#L185)
- 继续在登记时捕获公开和隐藏 Context，供硬超时隔离进程在交回登记时一并传递。固定 Laravel 的 process 回调在启动时提取 Context。当时 Rust 适配沿用登记时快照，捕获时点已在下一节调整。启用 `always()` 不会改变隔离进程被强制终止前无法交回登记的边界。
- macOS ARM64 从 `5ff2222f3` 干净归档及核定改动锁定离线构建。真实 Artisan 成功／非零退出仍返回 0／7，父 stdout 和 stderr 均先到 EOF。子进程仍可接收输入和登记 Context，显式 `--env` 和 Unicode 工作目录复验通过。真实 HTTP 返回 204 后放行子进程。新增独立 Artisan 回归校验任务仍活跃时父进程退出、普通回调被跳过、`always()` 继续执行、超限载荷由父进程报告、任务错误由子进程报告，且其他任务继续启动。
- 真实 Redis 8.10.3 使用 `queue:work redis --once --timeout=1 --tries=1`。普通任务及隔离成功任务分别在原 Worker 与隔离执行器登记任务。隔离失败时 `always()` 的任务仍由父 Worker 在 `JobAttempted` 后启动。强制终止隔离执行器时观察到失败输出、执行器退出以及无延后任务启动。独立 Background 派发及 `artisan inspire` 通过。
- `concurrency` 和 `processes` 定向测试 18 项、独立 `concurrency_defer` 回归、Rust 1.94.0 格式和锁定离线全目标严格 Clippy、三语构建和 18 项 Chrome 检查通过。Windows 的脱离 stdin 和 EOF 复用 [06-03 已实测实现](../06-03/lifecycle-failover-drivers.md#windows-arm64-background-输入与父输出分离)，本次公开入口实测在 macOS。已回收独立 Redis、文档产物和本批专属 3.3 GiB 编译产物。磁盘可用 53.75 GB（50.06 GiB），提交前周额度剩余 19%，按用户授权继续。夹具和日志位于 `/Users/Home/openrust-defer-parity.hNekf6SD`。

## defer 在执行边界捕获 Context

- 固定 Laravel 的 process `defer` 在终止回调启动每项工作前调用 `Context::dehydrate()`。Rust 注册时只保留原作用域 Context 和应用钩子的引用，选中终止回调后再提取快照，避免从父 Worker 新开的作用域读到空 Context。Sync 延后执行同样可读到登记后的变更。同批任务共享一次快照。v0.x（`6d1a81924`）已有分离公开和隐藏字段的作用域仓储，当前实现沿用其契约，旧版无延后进程交接实现。 [固定 Laravel ProcessDriver](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Concurrency/ProcessDriver.php#L101)
- 硬超时 Worker 的隔离执行器在交回已登记的回调时提取最终 Context，在应用作用域内执行脱水钩子。回调及其快照经过现有 JSON 结果交给父 Worker，父 Worker 按 `JobAttempted` 的结果筛选并启动。强制终止时来不及取快照或交回登记。
- macOS ARM64 从 `3970ff0a1` 干净归档及核定改动锁定离线构建。真实 HTTP 204、独立 Artisan 退出码 0／7 和真实 Redis 8.10.3 Worker 均覆盖登记后覆盖公开及隐藏字段、删掉旧字段、脱水钩子投影、作用域配置读取和父进程 EOF。Sync 请求回归及独立 Artisan 夹具验证注册时不运行脱水钩子、回调采用最新字段。独立 Background 队列和 `artisan inspire` 回归通过。
- 实际 `queue:work redis --once --timeout=1 --tries=1` 验证普通任务、隔离成功、隔离失败的 `always()` 和强制超时。普通任务由原 Worker 提取快照，隔离任务由执行器提取，父 Worker 在 `JobAttempted` 后启动子进程。杀死隔离执行器后无快照、无子进程。独立 `concurrency_defer` 和 5 项并发回归、Rust 1.94.0 格式和锁定离线全目标严格 Clippy、三语构建和 18 项 Chrome 检查通过。本批实际入口在 macOS。已回收独立 Redis、文档和本批专属编译产物。磁盘可用 52.46 GB（48.86 GiB），提交前周额度剩余 19%，按用户授权继续。夹具和日志位于 `/Users/Home/openrust-defer-context.OX03dC5Z`。

## 通用进程内延迟回调（2026-11-21）

为扩展缓冲刷新开放 `openrust::support::defer(future, name, always)`，复用现有 `DeferredCallbacks`，不另建应用结束执行器。核对固定 Laravel `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Support/functions.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/functions.php)、[DeferredCallbackCollection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Defer/DeferredCallbackCollection.php)、[InvokeDeferredCallbacks](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Http/Middleware/InvokeDeferredCallbacks.php) 与 Application 的 terminating/terminate。HTTPS原始源码留在本轮临时证据目录。旧版 `openrust-v0.x` 的 `foundation/termination.rs` 及 application/http termination 测试提供结束顺序参考；其全局回调注册表与当前多请求／多Worker任务作用域不一致，不直接引入。

Rust 接受 `Future<Output = Result<(), E>> + Send + 'static`，保持 `name`／`always` 参数顺序；同生命周期内同名回调替换前者，未具名回调独立保留。返回已有 `DeferredCallback`，允许 `.always()`；框架外部不能通过该入口手动排空其他请求的回调。捕获原 scoped 服务的 `Arc`，不在已关闭请求作用域后重新解析缓冲区。不实现异步 Drop 或进程崩溃时的虚假可靠刷新。

HTTP在传输释放正文后执行；错误响应默认跳过，always包含失败。嵌套Artisan调用与sync任务保留外层生命周期。普通Worker在JobAttempted后排空本attempt；子进程硬超时任务的本地Future在子进程正常交付结果前执行，仍受任务超时预算，不能跨进程序列化任意Future。完成前取消、硬超时或SIGKILL不保证刷新；需要故障后持久化的业务使用既有持久队列。

隔离源码与日志：`/private/tmp/openrust-core-extension-contracts-20261121/`，复用核心ecosystem target，Rust1.94、命令局部dev/test debug=0，保留原生增量／并发。新增6项公开入口测试通过，覆盖并发HTTP原scope／最后值、一次执行、渲染422、同名替换、流式正文Drop、请求取消后隔离、外层HTTP中的嵌套Artisan/sync、命令0／非0／错误、回调错误报告及继续排空新回调。与既有核心接口／认证／模型／观察回归共26项通过，明确过滤原MySQL条件函数，不将缺URL提前返回计作数据库覆盖。日志 `core-lifecycle-tests.log`。

同一Worker函数在SQLite临时文件、PostgreSQL18和MySQL8.4独立容器分别通过：真正入队／出队、成功／失败／主动release／可取消超时四类attempt、JobAttempted先于回调、每attempt全新scoped对象、回调读到原对象最后状态与always筛选。PG／MySQL使用现行 `tests/postgres_acceptance.py`／`mysql_acceptance.py` 并显式映射 `OPENRUST_DEFER_DATABASE_DRIVER`／`OPENRUST_DEFER_DATABASE_URL`；未操作开发库或数据卷。日志 `defer-worker-{sqlite,postgres,mysql}.log`。三语helpers正文、站点构建和根／子目录桌面／手机12组浏览器通过。`cargo +1.94.0 clippy --workspace --all-targets --all-features --locked -- -D warnings` 与 `fmt --all --check` 通过；第三方依赖有既有弃用／cfg警告，未新增ignore。扩展集成由扩展负责人自行实现与验收，遵循最新分工规则。
