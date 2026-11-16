# 03-06 缓存、过期与原子锁

## 契约与实现

- 对照 Laravel v13.40.0 的 Repository、ArrayStore、FileStore、DatabaseStore、Lock 和缓存测试。参考固定 Goravel 的服务提供者，在每个应用容器中注册 CacheManager，门面在应用作用域内解析。 [Laravel Cache](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md) · [Goravel CacheServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/cache/service_provider.go)
- 实现 array、file、database 的过期、原子增删和计数、锁获取、等待、恢复、续租与释放。内存存储归属应用。文件写入用进程锁和同目录原子重命名。数据库读写走写连接，原子更新用事务与唯一键。锁和普通条目分开存放。数据库锁无显式租期时使用 24 小时。锁释放与续租检查持有者，过期持有者无法操作后继锁。
- 从 openrust-v0.x 的 `6d1a8192` 提取毫秒时间戳、文件重命名、键命名空间及 `vendor/openrust/framework/tests/unit/cache/flush.rs` 的清空边界回归。缓存支持 JSON 和字节，数据库和磁盘使用 Rust 编码。数据库期限列用 BIGINT 保存毫秒值，键使用前缀与键的摘要。数据库计数器缺失返回类型化错误，对应 Laravel 的失败结果。
- 文件锁首次并发打开曾在 macOS 返回 ENOENT。改为先打开已有文件，缺失时独占创建，竞争者重开同一文件。

## 验收

隔离基线 `21a65e98`，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。388 项测试通过。本批 4 项缓存测试包括真实 SQLite 与独立 MySQL 8.4.13，覆盖过期、竞争、递增、过期所有者、跨实例读取及 Artisan 清理。其他工作区数据库测试按各自的环境开关执行。
- 在两个独立的 MySQL 8.4.13 schema 启用真实迁移与清库测试。4 项迁移测试通过，含 SQLite、MySQL 迁移与 MySQL `migrate:fresh --drop-views --step`。
- 两个真实 OS 进程通过文件锁竞争、凭令牌恢复与释放。清空文件缓存保留目录内的普通文件、目录和符号链接。
- 三语 Rust 示例分别编译并经回环 TCP 发出 3 次请求。跨应用依次取得 404、204、200，命中正文为 `ready`。实际运行 `./artisan migrate`、`./artisan cache:forget reports/latest file`、`./artisan cache:clear file` 和 `./artisan inspire`，退出码均为 0。
- 文档 `npm run build` 成功。站点在 `/` 和 `/docs/` 通过 12 项浏览器组合及 6 项缓存页面专项检查。

## 后续边界

Redis 连接及缓存锁按 03-09 推进。标签、通用缓存事件、迁移生成器、回调持锁与其他存储驱动的完成情况按核心账本跟踪。Windows 文件锁实机未测。下一单元 03-07 使用已通过的 01-01 配置基础。

## 缓存故障切换

- 对照固定 Laravel 的 [FailoverStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/FailoverStore.php) 和 [Cache Failover](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#cache-failover)。openrust-v0.x 的缓存实现没有 failover 组合驱动，复用当前存储契约、原子操作和应用事件分发。
- 依据配置顺序按次解析具名存储并尝试操作。命中、未命中和成功返回 `false` 都直接结束，所有存储报错时返回最后一个错误。再次操作时从主存储开始。拒绝空存储列表，循环依赖返回类型化错误。
- `CacheFailedOver` 记录新报错的存储。Rust 在异步缓存操作取得结果后分发，`exception` 为错误消息，原操作保留自己的返回值或错误。锁构造沿相同顺序选取可创建句柄的存储，获取、续期和释放使用该句柄。当前同步锁构造和恢复接口不会派发异步事件，`get_prefix` 返回配置前缀。标签、标签清理与批量清锁接口仍待闭合。
- 隔离基线 `1b98fc2c7`，macOS arm64，Rust 1.94.0。固定 PHP 对照通过故障切换、首次成功结果、主存储恢复、事件重发和最后一个异常。MySQL 8.4.13 与 Redis 8.10.3 下，`cargo test --test cache --locked --offline -- --test-threads=1` 的 6 项通过，含实际 Artisan `cache:clear failover` 和 `inspire`。另用真实 Redis 运行 2 项 `cache_memo` 回归，验证 HTTP 请求和队列任务生命周期。
- 三语共用 Rust 示例运行通过，VitePress 构建和 18 项浏览器检查通过。变更文件格式及工作区全目标严格 Clippy 通过。全仓格式检查只报告既有 `sanctum_authentication.rs:1412` 排版，已核对该文件与基线逐字节一致。
- 临时 MySQL、Redis 和 PHP 容器已回收。清理本批 3.5 GiB 隔离 Cargo 产物和文档缓存，项目卷可用 60.79 GB（56.61 GiB），周额度剩余 70%，第二窗口无读数。证据目录为 `/private/tmp/openrust-03-06-failover.3xRqxr8D`。
