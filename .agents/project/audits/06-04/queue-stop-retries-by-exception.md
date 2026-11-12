# 06-04 按异常停止队列重试

## 基线

对照固定 Laravel [Stopping Retries by Exception](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#stopping-retries-by-exception)、[Exceptions 配置](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/Exceptions.php)、[异常处理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Exceptions/Handler.php)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。v0.x（`6d1a8192`）已有 `dont_retry`、`dont_retry_when` 和类型化来源查询。本批沿用该匹配思路与现有 Worker 结算。

## 实现与 Rust 适配

- 在应用的 `withExceptions` 接入 `dontRetry::<E>()`、`dontRetryWhen` 和异常处理器的 `shouldStopRetries`。先匹配类型，再判断谓词。Rust 类型规则沿用现有异常来源链查询。
- Worker 对命中规则的未处理错误执行常规失败结算和报告。未命中的错误保留原有尝试、退避与超时路径。自定义异常处理器未覆盖接口时使用默认规则。
- 独立执行器在类型仍可用时判断策略，只向父 Worker 回传布尔决策。父 Worker 继续持有预约并写入失败记录。

## 验收

- SQLite、MySQL 8.4.13、Redis 8.10.3 通过真实 HTTP 派发和独立 Artisan，验证类型优先、谓词匹配、失败只执行一次、报告继续运行、未命中时释放后执行，以及独立应用实例关闭规则后同一类型可重试。
- 三后端独立执行器验证原始错误类型匹配后由父 Worker 结算。Rust 1.94.0 格式、锁定离线严格 Clippy、工作区串行测试、三语示例编译执行、文档构建与 18 项浏览器检查，另验证错误文档链接跳转及同进程两个应用的处理器隔离。

[English](../../../../docs/en/digging-deeper/queues.md#stopping-retries-by-exception) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#stopping-retries-by-exception) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#stopping-retries-by-exception)
