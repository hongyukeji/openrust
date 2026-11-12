# 06-04 任务互斥锁

## 基线

核对固定 Laravel [Preventing Job Overlaps](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#preventing-job-overlaps)、[WithoutOverlapping](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Middleware/WithoutOverlapping.php) 和 [WithoutOverlappingJobsTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/WithoutOverlappingJobsTest.php)。沿用固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型运行边界。v0.x（`6d1a8192`）未找到对应中间件。本批复用现有缓存原子锁与独立 Worker。

## 实现与 Rust 适配

- 补齐 `shared`、`withPrefix`、`getLockKey`。以注册任务名对应 Laravel 的任务类或显示名，保留现有 OpenRust 默认锁键。开启共享后不同任务名可竞争同一个键。
- `releaseAfter` 与 `expireAfter` 接受 `Duration` 或 `SystemTime`。相对释放从本次竞争起算，绝对释放在竞争时计算剩余时间，绝对租约在配置时换算时长。保留 `dontRelease` 和异常时释放锁。
- 默认锁租约由缓存驱动决定，替代原先的固定 90 秒。文件与 Redis 缓存默认无到期时间，数据库缓存按已有驱动规则使用 24 小时。任务需要在进程退出后自动恢复时，可用 `expireAfter` 设定有限租约。

## 验收

- 实际 HTTP 派发和独立 Artisan Worker 验证跨任务共享、前缀和任务名隔离、绝对延迟后二次执行、`dontRelease`、绝对租约到期以及异常时释放。覆盖 SQLite、MySQL 8.4.13 与 Redis 8.10.3，使用文件或 Redis 缓存锁。
- Rust 1.94.0 格式、锁定离线严格 Clippy 和工作区串行测试通过。三语示例提取后通过实际 sync 队列执行，文档构建及 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#preventing-job-overlaps) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#preventing-job-overlaps) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#preventing-job-overlaps)
