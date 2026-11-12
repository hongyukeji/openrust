# 06-04 阻塞任务强制超时

## 基线

核对固定 Laravel [Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php) 和 [Job Expirations and Timeouts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-expirations-and-timeouts)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。核对 v0.x（`6d1a8192`）的 Worker 和队列驱动测试，未找到从外部强制终止阻塞任务的实现。本批复用当前框架的进程组、Provider 启动和预约结算。

## 实现与 Rust 适配

- 按任务选择 `JobOptions::hard_timeout` 后，Unix Artisan Worker 在独立进程组启动执行器并保留预约。时限沿用任务覆盖值及 Worker `--timeout`，包含子进程启动。任务处理器、中间件和日志上下文在执行器中装配。
- 到期时由父进程终止进程组并等待执行器退出，再按现有规则结算和派发 `JobTimedOut`。重试预约按退出设置保留到租约到期或释放，`fail_on_timeout` 仍进入失败记录，守护进程沿用配置的超时退出码。
- Laravel 通过 PHP 进程信号处理中断。Rust 的 Tokio 无法取消同步阻塞或死循环，强制路径由父进程执行。被阻塞的子进程无法在时限到达时调用协作式 `Interruptible` 回调。普通异步任务继续使用 Tokio 超时。
- 强制路径要求 Unix 与内置持久队列预约。已在 macOS 与 arm64 Linux 容器运行真实 Worker。Windows、自定义预约和合成链式批次不支持这条执行路径。通过 `Process` 启动的命令继承执行器进程组，主动脱离进程组或会话的程序可脱离此边界。跨进程异常保留错误链文本，具体 Rust 错误类型不跨进程传输。

## 验收

- 独立 Artisan `queue:work` 在 SQLite、MySQL 8.4.13 和 Redis 8.10.3 上执行真实预约。CPU 死循环、同步睡眠和 `Process` 外部命令均按任务超时停止。事件监听器实测执行器 PID 已回收，晚到副作用未发生。验证配置退出码 124、可重试预约保留及租约后重试、两条终态失败记录和关闭自动退出后继续消费。
- Rust 1.94.0 的 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 和 `cargo test --workspace --locked --offline -- --test-threads=1` 通过。三语队列文档的 Artisan 命令实跑、Rust 示例提取编译和执行，文档构建及 12 项导航与 6 项 Worker Chrome 检查通过。

## Linux 补验

- 在 macOS arm64 宿主的 Docker Desktop Linux 虚拟机内运行 Debian 12.15 arm64 容器。实测内核 `7.0.14-linuxkit`，Rust 1.94.0，目标 `aarch64-unknown-linux-gnu`。容器使用 `--init` 回收被终止进程组中的孤儿进程。此记录覆盖实际 Linux 内核上的容器运行。
- 从 `411e2d35` 的 Git 归档及本批回归断言运行 `cargo +1.94.0 test --locked --offline --test queue_hard_timeout`。启用容器内置 SQLite、独立 MySQL 8.4.13 验收库及 Redis 8.10.3。测试通过实际 Artisan 子进程确认 CPU 死循环、同步阻塞、外部 shell 和 sleep 的进程级中断。
- 三种驱动均通过。执行器在 `JobTimedOut` 分发时已回收，外部 shell 和 sleep 的 PID 在自然完成前消失，无晚到副作用。核对实际退出码 124、可重试任务在租约内保留、租约到期后成功取件、`fail_on_timeout` 与最大异常的失败记录，以及关闭自动退出后的继续消费。
- Rust 1.94.0 格式及受影响测试目标的严格 Clippy 通过。三语文档构建与 18 项浏览器检查通过。本批未修改 Worker 生产源码。

[English](../../../../docs/en/digging-deeper/queues.md#hard-job-timeouts) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#hard-job-timeouts) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#hard-job-timeouts)
