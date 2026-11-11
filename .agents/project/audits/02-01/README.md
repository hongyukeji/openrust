# 02-01 数据库配置、连接与基本 SQL

对照固定 Laravel 13.x [数据库配置](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#configuration)及[执行 SQL](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#running-queries)。核对 [DatabaseManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseManager.php)、[Connection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php)、[SQL 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseConnectionTest.php)和[连接 URL 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/ConfigurationUrlParserTest.php)。装配参考固定 [Goravel Database ServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)。

- 使用现有 `config/database.rs`，框架登记应用级 `db`、`DatabaseManager` 和连接解析契约。具名连接惰性创建共享池，数据库启动不阻塞应用 boot。SQLite 文件以应用根目录解析，缺失时返回驱动错误。内存库使用单连接，默认私有缓存。
- 从旧版 `6d1a8192` 的 `src/database/mod.rs`、`src/database/sqlite.rs` 和 `vendor/launchbadge/sqlx-mysql/OPENRUST.md` 提取路径解析、内存缓存、强制 TLS 和取消后淘汰回归。当前使用固定 SQLx 0.9.0，未启用无 TLS 的 MySQL RSA 认证。Rust 在操作层持有连接租约，取消或协议错误时关闭 MySQL 物理连接，无需移植旧版驱动补丁。
- DB 门面和 Connection 支持异步预处理查询、绑定计数验证、标量、行查询、插入、更新、删除、影响行数和无参数原始 SQL。Rust 使用 `Value` 和有序列记录。MySQL 无符号整数保留完整范围，DECIMAL 用精确文本表示。错误的 Display 和 Debug 省略 SQL、绑定值和凭据，显式错误来源仍可由调用方检查。
- URL 解析优先使用 URL 中的已指定配置。显式 URL 用户缺少密码时使用空密码，以免带入不同用户的配置密码。MySQL TLS 为 Required，配置 CA 时默认 VerifyIdentity。此次真实验收确认加密会话，证书主机名校验仍需有匹配证书的环境验收。
- 本批运行连接为 SQLite 和 MySQL。读写路由、事务、重新连接及完整 ConnectionInterface 留到 02-02，Query Builder 与 Schema 随各自单元接线。存储过程多结果集及数据库事件仍待后续实现。

## 验收

macOS arm64，Rust 1.94.0。隔离副本基于 `21a65e98`、已验收的 01-02y 至 01-07 以及本批文件。MySQL 使用独立的临时 MySQL 8.4.13 容器和独立数据库，应用现有服务未参与测试。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 329 项测试。测试进程启用 `OPENRUST_REQUIRE_MYSQL=1`，MySQL 路径实际执行。
- SQLite 验证默认文件读写、绑定数量、外键、缺失文件、未知连接、类型及内存库隔离。真实 MySQL 验证加密会话、URL 优先、参数化读写、精确类型、驱动错误，以及执行取消和元数据锁阻塞冷 PREPARE 后物理连接更换。
- 3 次真实回环 HTTP 请求通过 DB 门面验证两应用 SQLite 文件隔离。Artisan 子进程 `db:probe` 实际查询 SQLite 并退出成功。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语示例由 Rust 1.94.0 编译执行，另用 SQLite 读取写入结果。文档构建及根路径、子路径下共 12 个浏览器组合通过。当前工作区 Cargo 锁定依赖解析通过，保留已有包版本。

下一批 02-02 接读写连接、切换与事务，复用两种已验收的物理连接池，并以真实 SQLite 与专用 MySQL 8.4 验证事务、锁与故障回退。

补交复验。02-02 复用了原始目录，单连接实现从该快照提取并独立重建。读写路由、事务和作用域连接状态归入 02-02。待提交树通过 368 项工作区测试、严格 Clippy 和格式检查，真实 MySQL 路径已执行，SQLite、HTTP 和 Artisan 数据库探针通过。
