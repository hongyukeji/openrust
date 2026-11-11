# 03-05 密码哈希和重哈希

核对固定 Laravel 13.x 的[配置、基本用法和算法校验](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/hashing.md)、[HashManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Hashing/HashManager.php)、[BcryptHasher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Hashing/BcryptHasher.php)、[ArgonHasher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Hashing/ArgonHasher.php)、[Argon2IdHasher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Hashing/Argon2IdHasher.php)和[回归测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Hashing/HasherTest.php)。参考固定 [Goravel Hash Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/hash/service_provider.go) 的单例装配。核对旧版 `6d1a8192` 的 `hashing/{drivers,parameters,legacy_bcrypt,manager}.rs` 与 PHP 样例。提取字节契约、bcrypt 历史验证、元数据解析与阻塞容量模型。

- 接通应用级 `Hash`、`HashManager` 和 `Hasher` 契约。支持 bcrypt、Argon2i 与 Argon2id，保留现有 Laravel 配置和默认值。公开接口涵盖 `make`、`check`、`needsRehash`、`info`、`isHashed`、`verifyConfiguration` 与具名驱动。应用默认开启算法校验，关闭时可验证其他受支持算法的已存哈希。bcrypt 新哈希使用 `$2y$`，历史 `$2x$` 仅验证。Argon 可读取 v16 与缺少版本字段的历史载荷，新哈希使用 v19。
- Rust 用类型化 `Result` 与异步入口表达 PHP 异常和同步 KDF。复用旧版应用级容量和计算预算，阻塞任务在真正完成前保留许可。默认两个并发操作。默认最大 bcrypt 成本 16，Argon 上限为 256 MiB、时间成本 10 和 4 个通道。应用可调整。超过预算会在启动 KDF 前失败。
- bcrypt 保留 72 字节截断和验证时在首个 NUL 截断的 PHP 行为，生成遇到 NUL 返回错误。可通过 `BCRYPT_LIMIT` 拒绝过长的新输入。Rust 调用传入明确的选项与字节，空哈希返回 false。登录时自动重哈希与更新数据库依赖后续认证单元。自定义驱动工厂尚未接入。

## 验收

macOS arm64、Rust 1.94.0。在 `/private/tmp/openrust-03-05.QAk6af/app-under-test` 以 03-04 候选为底合入本批文件和模块出口。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。测试报告 384 项通过，含 4 项新增回归。真实 MySQL 与 S3 未在本批复跑。
- PHP 8.5.12 运行固定 Laravel 驱动。三种算法各对非 UTF-8 字节和超过 72 字节的密码执行双向互操作，共 12 项。验证正确及错误密码、元数据、参数变化和重哈希。复跑旧版 24 条 `$2x$` 样例，并通过 PHP 实测 Argon v16、有无版本字段的验证行为。
- 实际 HTTP 执行三语示例，覆盖 bcrypt 成本升级、关闭算法校验时从 bcrypt 迁移到 Argon2id、严格模式拒绝、错误密码和应用隔离。跨具名驱动并发用例验证共享容量、超预算拒绝和释放后的重试。`./artisan inspire` 正常退出。
- 三语页面 `npm run build` 通过，12 项常规浏览器组合和 6 项哈希页面正文、锚点及侧栏检查通过。工作区锁定依赖解析与 `git diff --check` 通过。

下一批为 03-06 缓存、过期与原子锁，前置为 02-01 数据库连接和 03-01 本地文件系统。验收包括真实 MySQL 8.4、缓存 TTL 和多实例锁竞争。
