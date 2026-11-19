# V04 Pail 请求认证与日志时序

2026-11-20，V04 按 Rust 显式 await 适配闭合。同步日志继续读取已解析身份快照；需要在日志调用时解析异步 guard 的 HTTP 代码使用 `Log::logWithRequest(request, level, message, context).await`，内置 HTTP 异常报告已接入。此结论不代表任意同步回调可以等待异步认证，也不关闭 V05 Windows 原生终端验收。

## 固定原文与设计

固定 [Pail v1.2.5 Handler](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/Handler.php) 在渠道写入后的 MessageLogged 监听中取得 Auth::id／user；没有活跃文件和被抑制的弃用记录不会查询身份。固定 Framework 13.40.0 的 Logger::writeLog 在写渠道后发送事件。旧版 `v0.x` 无 Pail 对应实现，沿用既有日志、Guard、应用作用域和会话机制，未引入旧版 API 或依赖。[原始边界复核](pail-auth-boundary.md)保留了不能提前认证、响应后回填、复制请求或绕过可变借用的原因。

Rust Guard 可执行任意异步 I/O，并持有 `&mut Request`。本批没有在同步回调里阻塞运行时；在显式请求日志入口中，渠道先完成写入，然后依注册顺序调用普通同步监听器或等待请求监听器。`LogManager::listenForRequest` 将同一个观察者的同步快照版本与请求版本一起注册，一条事件只调用其中一个，不会重复捕获。监听器列表先取快照，调用期间不持锁，新注册项在下一条事件生效。

公开入口包括 Log、LogManager、已解析 Logger 的 `logWithRequest`；归一化异常通过日志契约的 `logWithExceptionForRequest` 传递。自定义同步 Logger 的默认实现回退到原 `logWithException`，保持已有适配器可编译及既有职责。内置 Handler 的 HTTP 路径传入当前请求；命令、队列、流式输出及其他不持有可变请求的报告继续使用原同步日志边界。

Pail 仅在所属应用、当前 HTTP 来源、活跃会话且未抑制的记录中等待选中 guard。已解析用户由原 Guard 缓存复用，登录、登出和 shouldUse 后的快照随原认证机制发布。认证期间的同步或异步嵌套日志只读取当前快照；Tokio 任务作用域阻止递归认证，取消自动释放作用域，后续可重试。没有请求拷贝、原始指针、block_on 或响应后回填。

渠道记录已经写入后，认证或观察者失败沿 LogResult 返回，后续监听器不运行；取消也可能留下普通日志但没有 Pail 行。这是显式异步调用的可观察边界，三语正文均已说明。应用的 HTTP 异常报告沿既有策略忽略报告失败并继续构造响应。HTTP 内程序化 Artisan 保留 HTTP 来源，独立进程 Artisan 才使用 console 来源，保持原有契约。

## 实际验收

证据 `/private/tmp/openrust-v04-20261120/`。从 baseline.txt 的提交导出独立源码，仅叠加本批 15 份源码／测试／正文；与共享工作区逐份 [SHA](pail-auth-timing-sha256.json) 一致。期间 Passport 自行提交 ecc607f2b，其后端／依赖和其他协作者改动没有进入此快照；本批结果不外推到那些后续变更。

| 验收 | 实际结果与日志 |
| --- | --- |
| Framework | Rust 1.94.0，log_listeners、exception_reporting、exception_rendering、logging、log_context 共 36 项通过，framework-final.log，补强弱引用后 listeners-final.log 的 7 项再次通过。新增请求监听测试检查写入后回调、顺序、过滤、注册快照、错误截断、取消及恢复；既有异常级别／上下文／限流／去重和真实 HTTP／Artisan 回归通过。 |
| 认证时序 | auth_timing 的真实双 TCP 应用通过，auth-timing-final.log：异步让出、认证内部同步及异步日志不递归，已缓存身份、真实 SessionGuard login／logout、guard 切换、无日志／过滤／弃用不查询、无活跃会话不查询、取消后同请求重试、认证错误、HTTP 异常自动认证、后续请求和多应用隔离、关停及端口复用。 |
| Pail 现有行为 | options 6 项、files 4 项及子进程并发文件写入，command 原生跟随／取消／HTTP／Artisan／信号／管道，printer 的 432 固定 PHP 向量通过，pail-final.log。Provider 22 组真实 HTTP／Artisan 和路径隔离通过，provider-final.log。 |
| MySQL Worker | 官方 tests/mysql_acceptance.py 实查 MySQL 8.4.13；cargo test -p openrust-pail --test capture 通过真实双应用 HTTP→独立 Artisan 筛选与打印、同步任务、实际 Worker、隔离任务、失败回调及后续消费、应用储存覆盖，mysql-http-capture.log。 |
| 根应用实际入口 | 以 features pail 构建真实 main；同一 MySQL 官方夹具运行 tests/pail_auth_http.py，HTTP 缺失视图500→请求入口解析 guest→Pail 会话和终端输出，随后页面恢复200、超时退出及删除会话、SIGINT退出0、端口释放、独立 Artisan inspire 均通过。root-build.log／mysql-http-capture.log。 |
| 严格检查 | cargo +1.94.0 clippy --offline --locked --workspace --all-targets --features pail,passport -- -D warnings 通过，clippy.log／clippy-final.log；cargo +1.94.0 fmt --all --check 通过，fmt-final.log；Python AST 和 git diff --check 通过。 |
| 三语文档 | 最终 npm run build 通过，docs-build-final.log；12 个根路径／docs 子目录／三语／桌面移动浏览器组合，6 个 View／Blade／Logging 锚点、示例和链接组合通过，docs-browser-final.log／docs-contracts-final.log。正文示例接口已在原生测试执行。 |

初轮测试修正了 Artisan::command 的闭包返回类型、Request::path 的已有去前导斜杠约定，以及 HTTP 中 Artisan 保留 HTTP 来源的预期，未更改这些生产契约。Provider 首轮因 macOS `/var` 与 `/private/var` 临时目录字符串比较失败，使用 `TMPDIR=/private/tmp` 原测试重跑全部通过，不改生产路径。初轮及最终日志分别保留。47 个不同测试函数及上述独立 harness 通过，不将重复子进程测试计为新增函数。

不运行无筛选工作区数据库测试，未测试 SQLite／其他数据库与 Windows、Linux 全目标矩阵。本批同步日志主动解析异步 guard 的限制通过明确 API 适配保留，不能将普通 Log::info 描述为自动查询身份。

## 收尾

仅本批文件本地提交，不推送。保留日志、夹具、源码和隔离快照，直接删除本批闲置 target、浏览器依赖和文档 dist。MySQL 官方夹具已回收专属容器，保留所有数据库卷与共享缓存。资源实测补记于下。

清理前可用 54,065,288 KiB，清理后 66,408,708 KiB（68.0 GB／63.33 GiB），实测增加 12,343,420 KiB。读数包含同期其他工作影响。官方批次 80dfa1b99a17af1a388cd931b0c69add 的容器复查为空。
