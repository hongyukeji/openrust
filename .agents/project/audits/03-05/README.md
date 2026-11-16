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

## 自定义哈希驱动

对照固定 Laravel 的 [Manager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Manager.php#L65) 与 [HashManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Hashing/HashManager.php)。提取 v0.x 的锁外工厂、按名缓存和构造占位释放机制。

- `extend` 在首次解析前覆盖内置或自定义创建逻辑。已解析实例在 `forgetDrivers` 前保持稳定，`getDrivers` 返回缓存快照。工厂可通过 `getContainer` 取得当前应用容器。Rust 通过显式管理器引用表达 Laravel 工厂的容器参数及闭包绑定，应用容器以弱引用保存。
- 构造失败后可重试。对同名进行递归或并发创建返回 `DriverBusy`。工厂直接返回所属管理器时返回 `DriverSelfReference`，返回其他管理器时解析其底层驱动再缓存，构造互环在进入缓存前被拦截。清缓存会使在途构造结果失去缓存发布资格，正在执行的调用仍可使用其结果。
- 工厂使用管理器的内置创建方法时，共享现有容量与执行预算。Rust 的工厂和驱动保留原字节输入和类型化错误契约。

## 03-05 整单元验收

隔离基线 `9c942a5d8`。PHP 8.5.12 运行固定 Laravel，验证容器参数、延迟创建、实例缓存、更换工厂、忘记驱动和默认驱动委托。八项 Rust 哈希测试通过，覆盖 bcrypt、Argon2i／Argon2id、错误密码、算法验证、参数重哈希、历史 PHC 和 bcrypt 哈希、跨驱动容量、失败重试及并发缓存边界。实际回环 HTTP 验证两个应用对同一驱动名解析各自容器内的服务，分别生成与校验 bcrypt 和 Argon2id。登录持久化重哈希见 [05-08](../05-08/README.md)。

三语共用示例、实际 `./artisan inspire`、变更文件格式、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式检查仍报告既有 Sanctum 排版差异。03-05 核心闭合。

PHP 容器及本批 3.3 GiB Cargo 产物和文档缓存已清理，项目卷可用 60.84 GB（56.66 GiB），周额度剩余 71%。证据目录为 `/private/tmp/openrust-03-05-custom.Tb3PSbM0`。
