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

Redis 连接及缓存锁按 03-09 推进。标签、memo、failover、缓存事件、迁移生成器、回调持锁与其他存储驱动尚未验收。Windows 文件锁实机未测。下一单元 03-07 使用已通过的 01-01 配置基础。
