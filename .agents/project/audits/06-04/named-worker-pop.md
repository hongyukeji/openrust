# 06-04 命名 Worker 取件回调

## 基线

对照固定 Laravel [Running the Queue Worker](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#running-the-queue-worker)、[Worker::popUsing](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 与 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/worker.go) 的取件路径。v0.x 未找到命名回调。复用现有预约契约、轮询期限及取件事件。

## 实现与 Rust 适配

- `Queue::popUsing`、`QueueManager::popUsing` 与 `Worker::popUsing` 将命名回调保存在当前应用。传入 `None` 可撤销，正在运行的调用持有回调快照。
- `queue:work` 和 `queue:listen` 通过 Worker 名称选择回调。回调接收逗号分隔的队列列表和异步取件接口，返回空值时本轮空闲。取件事件仍在回调前后按正常顺序发出。
- 自定义策略决定队列选择和暂停检查。取件接口保留单次、无阻塞和空闲期限。Rust 使用应用级注册表表达 Laravel 静态注册的职责，隔离同进程中的应用。

## 验收

- 独立 Artisan 在 SQLite、MySQL、Redis 验证命名回调、空值、移除后恢复默认策略以及 `queue:listen` 子进程继承名称。最终工作区验收包含同进程两应用隔离。
- 三语代码编译并执行对应的实际 Artisan Worker 命令，文档构建与 18 项浏览器检查通过。Rust 1.94.0 格式、严格 Clippy 及锁定离线串行工作区验收通过，含子进程 577 项通过、7 项忽略。

[English](../../../../docs/en/digging-deeper/queues.md#selecting-jobs-for-a-named-worker) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#selecting-jobs-for-a-named-worker) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#selecting-jobs-for-a-named-worker)
