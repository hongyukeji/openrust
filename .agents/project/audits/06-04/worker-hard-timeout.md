# 06-04 阻塞任务强制超时

## 基线

核对固定 Laravel [Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php) 和 [Job Expirations and Timeouts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-expirations-and-timeouts)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。核对 v0.x（`6d1a8192`）的 Worker 和队列驱动测试，未找到从外部强制终止阻塞任务的实现。本批复用当前框架的进程组、Provider 启动和预约结算。

## 实现与 Rust 适配

- 按任务选择 `JobOptions::hard_timeout` 后，Unix Artisan Worker 在独立进程组启动执行器并保留预约。时限沿用任务覆盖值及 Worker `--timeout`，包含子进程启动。任务处理器、中间件和日志上下文在执行器中装配。
- 到期时由父进程终止进程组并等待执行器退出，再按现有规则结算和派发 `JobTimedOut`。重试预约按退出设置保留到租约到期或释放，`fail_on_timeout` 仍进入失败记录，守护进程沿用配置的超时退出码。
- Laravel 通过 PHP 进程信号处理中断。Rust 的 Tokio 无法取消同步阻塞或死循环，强制路径由父进程执行。被阻塞的子进程无法在时限到达时调用协作式 `Interruptible` 回调。普通异步任务继续使用 Tokio 超时。
- 首批强制路径使用 Unix 与内置持久队列预约，已在 macOS 与 arm64 Linux 容器运行真实 Worker。自定义预约和合成链式批次仍需单独支持。通过 `Process` 启动的命令继承执行器进程组，主动脱离进程组或会话的程序可脱离此边界。跨进程异常保留错误链文本，具体 Rust 错误类型不跨进程传输。

## 验收

- 独立 Artisan `queue:work` 在 SQLite、MySQL 8.4.13 和 Redis 8.10.3 上执行真实预约。CPU 死循环、同步睡眠和 `Process` 外部命令均按任务超时停止。事件监听器实测执行器 PID 已回收，晚到副作用未发生。验证配置退出码 124、可重试预约保留及租约后重试、两条终态失败记录和关闭自动退出后继续消费。
- Rust 1.94.0 的 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 和 `cargo test --workspace --locked --offline -- --test-threads=1` 通过。三语队列文档的 Artisan 命令实跑、Rust 示例提取编译和执行，文档构建及 12 项导航与 6 项 Worker Chrome 检查通过。

## Linux 补验

- 在 macOS arm64 宿主的 Docker Desktop Linux 虚拟机内运行 Debian 12.15 arm64 容器。实测内核 `7.0.14-linuxkit`，Rust 1.94.0，目标 `aarch64-unknown-linux-gnu`。容器使用 `--init` 回收被终止进程组中的孤儿进程。此记录覆盖实际 Linux 内核上的容器运行。
- 从 `411e2d35` 的 Git 归档及本批回归断言运行 `cargo +1.94.0 test --locked --offline --test queue_hard_timeout`。启用容器内置 SQLite、独立 MySQL 8.4.13 验收库及 Redis 8.10.3。测试通过实际 Artisan 子进程确认 CPU 死循环、同步阻塞、外部 shell 和 sleep 的进程级中断。
- 三种驱动均通过。执行器在 `JobTimedOut` 分发时已回收，外部 shell 和 sleep 的 PID 在自然完成前消失，无晚到副作用。核对实际退出码 124、可重试任务在租约内保留、租约到期后成功取件、`fail_on_timeout` 与最大异常的失败记录，以及关闭自动退出后的继续消费。
- Rust 1.94.0 格式及受影响测试目标的严格 Clippy 通过。三语文档构建与 18 项浏览器检查通过。本批未修改 Worker 生产源码。

## Windows 补验

- Windows 父进程使用 [Job Object](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) 收纳执行器与后代。执行器从继承的标准输入收到归组确认后才创建应用。Worker 超时终止整个 Job Object，等待执行器和成员进程退出，再按已有策略结算预约并发出 `JobTimedOut`。父进程异常退出时，Job Object 的最后一个句柄关闭会终止剩余成员。
- 在 macOS arm64 宿主的 Parallels Windows 11 Pro ARM64 虚拟机（Build 26200）中，使用 x64 兼容层运行 `x86_64-pc-windows-msvc` 目标。工具链为 Rust 1.94.0、Cargo 1.94.0 和 MSVC 14.44.35207。执行 `cargo test --locked --offline --target x86_64-pc-windows-msvc --test queue_hard_timeout`。9 个相关源码和 Cargo 文件的 SHA-256 与提交候选一致。
- 实际 `queue:work` 使用内置 SQLite 数据库队列。验证 CPU 死循环、同步阻塞、外部进程及其子进程都在超时后退出。`JobTimedOut` 监听器以 Windows 进程列表确认执行器和外部进程树已退出，超过原定运行时间后仍无晚到副作用。未归组的内部执行器在创建应用前退出。
- 实测超时退出码 124。CPU 任务在租约到期前保持预约，随后由新 Worker 重试并成功。按任务 UUID 验证 `fail_on_timeout` 和最大异常规则写入超时失败记录，关闭超时退出后，同一 Worker 能继续消费。
- Windows Debug 的终态结算曾触发默认栈溢出。终态结算的 Future 改由堆持有，失败记录写入在独立 Tokio 任务中轮询，沿用原连接的路由状态并等待写入完成。取消等待时会中止写入任务。Windows 强制超时首批验证 x64 与 SQLite。原生 ARM64 和 x64 MSVC 的 MySQL 与 Redis 验收见后续小节。非隔离失败回调在该批扩展夹具中复现的 Debug 栈溢出见下节。
- macOS 全工作区主测试进程 592 项通过、7 项忽略。最终候选补跑 SQLite、MySQL 8.4 和 Redis 8 的硬超时进程验收、进程入口测试和严格 Clippy。格式、三语站点构建及 18 项浏览器检查通过。Linux 实测见前节。

## Windows 非隔离失败回调补验

在 `8c2b6860` 的 Windows Debug 扩展验收中，非隔离任务的失败回调场景以 `0xC00000FD` 结束。单独固定非隔离任务的作用域 Future 后仍可复现。最终将预约处理、单次尝试及作用域执行的 Future 分开固定在堆上，降低执行与失败结算路径的栈占用。任务上下文隔离、异常报告、失败记录和后续轮询沿用原有流程。

- 在同一 Parallels Windows 11 Pro ARM64 Build 26200 虚拟机的 x64 兼容层，使用 Rust 1.94.0 和 MSVC 14.44.35207 执行 `cargo test --locked --offline --target x86_64-pc-windows-msvc --test queue_hard_timeout`，使用未优化测试配置和内置 SQLite。Windows 验收源码的哈希与候选一致。实际 `queue:work --queue=callback-probe --stop-when-empty` 在同一进程内处理两个终态失败和一条成功任务，退出码为 0。测试按 UUID 核对两条失败记录，检查回调与 `JobFailed` 事件、回调自身异常报告和队列清空。强制超时的既有 Windows 断言同时通过。
- macOS 全工作区回归 592 项通过、7 项忽略。最终候选补跑 SQLite、MySQL 8.4、Redis 8 的队列进程验收，格式和全工作区严格 Clippy 通过。

## Windows ARM64 原生补验

- 从 `0600464137d4ae75ff484f67abfcf97e09f88bdc` 的干净归档构建。Parallels Windows 11 Pro ARM64 Build 26200 使用原生 Rust/Cargo 1.94.0、MSVC 14.44.35207 的 HostARM64 工具和 ARM64 Clang 19.1.5。安装 MSVC ARM64 与 Clang 组件后，保持 Cargo.lock 不变。五个相关源码和锁文件的 SHA-256 与归档一致。
- 执行 `cargo test --locked --offline --target aarch64-pc-windows-msvc --test queue_hard_timeout`，未优化测试配置通过，进程退出码为 0。实际执行的 Worker 夹具 PE Machine 为 `0xAA64`，父 Worker 与隔离执行器使用同一原生 ARM64 可执行文件。
- 内置 SQLite 持久队列的实际 `queue:work` 验证 CPU 死循环、同步阻塞、外部程序及其后代的超时回收，确认无晚到副作用、配置退出码 124、租约到期后重试、按 UUID 持久化超时失败记录，以及关闭超时退出后的继续消费。同一 Worker 处理两个非隔离终态失败和一条后续成功任务，验证失败回调、回调异常报告、`JobFailed` 事件、失败记录和退出码 0。

### MySQL 与 Redis 驱动补验

- 从 `b5edaed00a7a10b8907cd7cea625047490c4525f` 的干净归档在同一 Windows 11 ARM64 虚拟机运行 `cargo test --locked --offline --target aarch64-pc-windows-msvc --test queue_hard_timeout`。未优化构建退出 0，PE Machine 为 `0xAA64`。五个相关源码和锁文件的 SHA-256 与 Git 基线一致。
- 原生 Worker 经 Parallels 共享网络访问宿主机的 MySQL 8.4.13 与 Redis 8.10.3。MySQL 使用专用数据库与仅能访问该库的临时账号。Redis 使用专用容器，由虚拟机环回转发。MySQL 用例的队列和失败记录存于 MySQL。Redis 用例的队列存于 Redis，失败记录存于虚拟机内的 SQLite。
- SQLite、MySQL 和 Redis 三组真实 `queue:work` 均通过。每组验证 CPU 死循环、同步阻塞及外部进程树的超时回收和无晚到副作用，核对退出码 124、租约到期后重试、按 UUID 写入超时失败记录和关闭超时退出后的继续消费。每组还验证同一 Worker 执行两个非隔离失败任务的回调、报告回调异常、写入对应失败记录，并成功消费下一条任务后以 0 退出。

## Windows x64 MSVC 的 MySQL 与 Redis 补验

- 从 `3bed140d8943aad36a1ef1f155bde9027c142a3c` 的干净归档在 Parallels Windows 11 Pro ARM64 Build 26200 的 x64 兼容层运行 `cargo test --locked --offline --target x86_64-pc-windows-msvc --test queue_hard_timeout`。工具链为 Rust/Cargo 1.94.0、MSVC 14.44.35207。未优化构建退出 0，实际执行的 Worker 夹具 PE Machine 为 `0x8664`。五个相关源码和锁文件的 SHA-256 与 Git 基线一致。
- SQLite、MySQL 8.4.13 与 Redis 8.10.3 三组真实 `queue:work` 均通过。MySQL 队列和失败记录使用专用 MySQL 库，Redis 队列使用专用 Redis 容器，失败记录使用虚拟机内的 SQLite。
- 每组验证强制超时回收 CPU 死循环、同步阻塞及外部进程树，无晚到副作用。验证超时退出码 124、租约后重试、按任务 UUID 写入失败记录，以及关闭超时退出后继续消费。每组还验证非隔离失败回调及其异常报告，同一 Worker 成功消费后续任务并以 0 退出。

[English](../../../../docs/en/digging-deeper/queues.md#hard-job-timeouts) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#hard-job-timeouts) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#hard-job-timeouts)
