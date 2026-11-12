# 06-04 任务异常次数上限

## 基线

核对固定 Laravel [Max Exceptions](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#max-exceptions)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php) 的异常上限、超时及预约行为。沿用固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型运行方式。v0.x（`6d1a8192`）未找到任务级异常上限。本批复用当前缓存原子计数和 Worker 终态结算。

## 实现与 Rust 适配

- `JobOptions::max_exceptions` 是可选载荷字段。Worker 对未处理错误和超时按 UUID 计数，在一日窗口内达到上限时记录失败，独立于尝试次数和 `fail_on_timeout`。旧载荷缺省沿用原规则。
- Worker 使用应用默认缓存的原子计数机制。跨进程累计使用共享缓存。正常主动释放和已被中间件处理的异常无需增加计数。外部进程崩溃的预约标记属于后续交付。

## 验收

- 实际 HTTP 派发及独立 Artisan Worker，在 SQLite、MySQL 8.4.13 和 Redis 8.10.3 上验证同名任务 UUID 计数隔离、主动释放不计入、无限尝试下达到异常上限及终态入库。
- 三后端验证普通超时和独立进程强制超时触发任务异常上限。验证旧载荷反序列化。
- Rust 1.94.0 格式、锁定离线严格 Clippy、工作区串行测试、三语 Rust 示例提取运行、文档构建与 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#max-exceptions) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#max-exceptions) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#max-exceptions)
