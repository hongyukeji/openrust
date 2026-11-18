# 06-04 Worker 崩溃计入异常次数

## 基线

对照固定 Laravel [Max Exceptions](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#max-exceptions)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php) 的崩溃标记、恢复前失败及完成后清理。沿用固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型运行边界。v0.x（`6d1a8192`）未找到对应实现。本批复用持久预约、缓存锁、异常次数上限及终态结算。

## 实现与 Rust 适配

- `JobOptions::count_crashes_as_exceptions` 默认关闭。配置异常上限后，Worker 在应用默认缓存保存一日有效的任务 UUID 标记。重新预约发现旧标记时累计一次异常，达到上限在调用处理器前失败。
- OpenRust 以带令牌的缓存标记区分尝试。获取和删除标记时使用短期缓存锁，旧执行器结束时只清理自己的标记。已返回的尝试在重新发布预约前清理，正常释放和已处理的超时不会计为崩溃。跨进程依赖同一支持锁的缓存及命名空间。
- 初始强制终止验收使用 Unix。Windows ARM64 已在后续补验，见[强制终止与异常次数](#windows-arm64-worker-强制终止与异常次数)。

## 验收

- 真实 HTTP 派发、独立 Artisan 和实际进程终止，覆盖 SQLite、MySQL 8.4.13 和 Redis 8.10.3。验证两次崩溃后的第三次预约终态失败、未启用时正常重试、普通释放及超时后正常重试、旧 Worker 迟到无法清除新标记。
- Rust 1.94.0 格式、锁定离线严格 Clippy、工作区串行测试、三语 Rust 示例提取运行、文档构建与 18 项 Chrome 检查通过。全量运行确认生产实现与三个持久后端场景通过。夹具随后明确断言旧 Worker 退出码 1 和 Unix SIGKILL 状态，定向复跑三后端。

[English](../../../../docs/en/digging-deeper/queues.md#counting-worker-crashes) · [简体中文](../../../../docs/zh_CN/digging-deeper/queues.md#counting-worker-crashes) · [繁體中文](../../../../docs/zh_TW/digging-deeper/queues.md#counting-worker-crashes)

## Windows ARM64 Worker 强制终止与异常次数

- 固定 Laravel 在任务开始时写入缓存标记，重领旧标记时计入异常预算，并在正常完成、释放或已处理超时后清理。旧版无对应崩溃预算实现，本批复用正式版现有租约和令牌标记，将原 Unix 集成用例扩展到 Windows。Unix 校验 SIGKILL，Windows 校验原生子进程被强制终止后的退出码 1。生产实现保留原有逻辑。 [固定 Laravel Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php#L708)
- 从 `0872d77c9` 干净归档及本批测试改动出发，在 Windows 11 ARM64 Build 26200 上用 Rust 1.94.0、MSVC 14.44.35207 锁定离线运行 `cargo test -p openrust-framework --test queue_worker artisan_worker_database_retries_failures_and_hydrates_context -- --exact`。原生 PE Machine 为 `0xAA64`。SQLite 持久队列与来宾本地文件缓存通过真实 HTTP 派发和独立 Artisan。两次强制终止触发预算耗尽，随后在调用处理器前记失败。关闭计数时可重试，主动释放和已处理超时不增加崩溃数，旧 Worker 也无法清除新标记。
- macOS 上同一测试目标用 SQLite、MySQL 8.4.13 和 Redis 8.10.3 复验；Windows 的 MySQL／Redis 崩溃计数不在本批实际矩阵内。Windows Worker 硬超时的数据库矩阵另见[硬超时记录](worker-hard-timeout.md#mysql-与-redis-驱动补验)。
- 本批同时验证 Artisan、格式、macOS 严格 Clippy、三语构建和 18 项浏览器检查。隔离容器、来宾夹具和构建产物已回收，虚拟机恢复停止。磁盘可用 72.97 GB（67.96 GiB），提交前周额度剩余 20%，按用户授权继续。原始日志位于 `/Users/Home/openrust-worker-crash.8cQfEDFt`。

## Windows ARM64 MySQL 与 Redis Worker 崩溃恢复

- 从干净提交 `3c139ed24` 构建，Windows 11 ARM64 使用 Rust 1.94.0、MSVC 14.44.35207，PE Machine 为 `0xAA64`。用同一测试二进制分别运行 `artisan_worker_mysql_retries_failures_and_hydrates_context` 和 `artisan_worker_redis_retries_failures_and_hydrates_context`，强制要求两个后端并使用真实 HTTP 和独立 Artisan 进程。MySQL 8.4.13 和 Redis 8.10.3 经宿主机的独立验收容器提供，连接仅经虚拟机私有网络。MySQL 场景使用来宾本地文件缓存，Redis 场景使用同一 Redis 服务的缓存。
- 两种组合均验证两次强制终止后由第三次预约在调用处理器前落失败记录、关闭崩溃计数时继续重试、主动释放及已处理超时不计入崩溃，以及旧 Worker 无法清掉新 Worker 的标记。生产代码沿用 `3c139ed24`。
- Rust 1.94.0 格式、macOS 锁定离线全目标严格 Clippy、三语构建和 18 项 Chrome 检查通过。已回收本批容器、来宾夹具及专属编译和文档产物，虚拟机恢复停止。磁盘可用 70.58 GB（65.73 GiB），提交前周额度剩余 20%，按用户授权继续。原始验收日志位于 `/Users/Home/openrust-crash-backends.ez4Pum3a`。
