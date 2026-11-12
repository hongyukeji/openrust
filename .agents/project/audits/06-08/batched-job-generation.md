# 06-08 Artisan 批次任务生成

- 对照固定 Laravel [JobMakeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/JobMakeCommand.php)、[批次任务模板](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/stubs/job.batched.queued.stub)、[Batchable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batchable.php) 和 [Job Batching](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-batching)。固定 Goravel [JobMakeCommand](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/console/job_make_command.go) 提供编译型注册参考。旧版 `v0.x`（`6d1a8192`）未发现批次生成器，沿用主线已验收的 SQL 批次和 Worker。
- `--batched` 优先于 `--sync` 并选择独立可发布模板。生成任务使用 Rust 显式模块和 Provider 注册，以 `Payload` 加入 `Bus::batch`。处理器从载荷读取可选批次 ID，使用 `SkipIfBatchCancelled` 在进入业务方法前略过已取消批次。继续复用 `--test` 和 `--force` 的现有流程。
- 跨存储批次派发、批次回调和清理命令仍为 06-04 后续范围。
- 以 `852c4fe6` 为隔离基线。实际 Artisan 进程覆盖 `--batched --sync --test`、独立模板发布、重复保护和强制覆盖。三语生成命令产生一致源码，文档示例编译后由实际 HTTP 派发。SQLite 和 MySQL 8.4.13 的独立 Worker 验证生成处理器收到对应批次 ID、取消时跳过业务代码、批次完成及生成的功能测试。
- Rust 1.94.0 格式、严格 Clippy 和工作区锁定离线串行 572 项通过，6 项辅助夹具忽略。三语站点构建、12 项通用浏览器检查和 6 项生成小节检查通过。
