# 06-04 Artisan 批次清理

- 对照固定 Laravel [Pruning Batches](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#pruning-batches)、[PruneBatchesCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/PruneBatchesCommand.php) 与 [DatabaseBatchRepository](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/DatabaseBatchRepository.php)。固定 Goravel 的 [队列命令目录](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/queue/console) 与旧版 `v0.x`（`6d1a8192`）无对应清理命令。沿用当前 Artisan 注册与 SQL 仓库。
- `queue:prune-batches` 默认按完成时间保留 24 小时。`--unfinished` 与 `--cancelled` 按创建时间计算，可单独指定。按 Laravel 顺序执行并分别报告删除数，每次最多删除 1,000 条，投递记录通过已发布的外键级联删除。Rust CLI 接受非负整数小时，在执行删除前验证所有期限。
- 基于 `8f668d82` 隔离验收。SQLite、MySQL 8.4.13 的独立 Artisan 进程覆盖跨批删除 1,001 条、三种期限、保留近期记录、取消以创建时间计算、级联删除和重复执行。三语正文六条命令实际在两库运行。站点构建、12 项通用浏览器检查及 6 项批次清理小节检查通过。
- Rust 1.94.0 格式、严格 Clippy 和工作区锁定离线串行 572 项通过，6 项辅助进程夹具忽略。
