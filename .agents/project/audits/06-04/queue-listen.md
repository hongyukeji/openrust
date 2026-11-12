# 06-04 Artisan 队列监听

## 基线

对照固定 Laravel [Running the Queue Worker](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#running-the-queue-worker)、[ListenCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/ListenCommand.php)、[Listener](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Listener.php) 与 [QueueListenerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueListenerTest.php)。参考固定 Goravel [Worker](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/worker.go) 的编译运行。v0.x 未找到同名 Artisan 入口。复用现有单次 Worker、进程组与信号监控。

## 实现与 Rust 适配

- `queue:listen` 每轮启动当前编译产物的 `artisan queue:work --once`，使用应用目录与指定环境，转发输出。空队列在子进程内按 `--sleep` 等待，`--rest` 管理轮次间休息。父监听器执行子进程超时及自身常驻内存检查。
- Rust 使用字面 argv 和托管子进程。Unix 子进程后代沿用监听器管理的进程组。退出请求优先通知子进程，并在限时等待后回收。监听器的超时边界为整轮子进程。
- `--force` 的维护模式行为依赖正式的应用维护状态和 Worker 门禁；命令接受并转发参数。命名取件回调为后续 Queue Worker 扩展。

## 验收

- 独立 Artisan 定向验证同一队列连续两项使用不同 Worker PID、子进程输出、终止后回收，以及整轮超时和预约保留。SQLite、MySQL 与 Redis 的最终定向复验确认超时后回收 Worker 和外部后代。
- 三语命令通过实际 Artisan 启停，文档构建和 18 项浏览器检查通过。Rust 1.94.0 格式、严格 Clippy 和锁定离线串行工作区验收通过，含子进程 577 项通过、7 项忽略。

[English](../../../../docs/en/digging-deeper/queues.md#listening-with-a-fresh-worker-process) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#listening-with-a-fresh-worker-process) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#listening-with-a-fresh-worker-process)
