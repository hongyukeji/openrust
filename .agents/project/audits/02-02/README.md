# 02-02 多连接、读写路由和事务

核对固定 Laravel 13.x 的[读写连接](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#read-and-write-connections)、[多连接](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#using-multiple-database-connections)和[事务](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#database-transactions)。对照 [ConnectionFactory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/ConnectionFactory.php)、[DatabaseManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseManager.php)、[ManagesTransactions](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/ManagesTransactions.php)和[连接回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseConnectionTest.php)。装配沿用 02-01 参考[固定 Goravel Database Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)的应用级连接管理器。

- `ConnectionFactory` 合并 `read`、`write` 与公共配置，MySQL 接受主机列表。读操作优先使用读池，具名连接允许 `::read` 和 `::write`。`selectFromWriteConnection` 强制单次查询走写库。`sticky` 和强制走写库状态以逻辑连接为单位，由当前 HTTP/Artisan 作用域保管。独立管理器以逻辑连接的克隆共享状态。
- 事务借用一个写库物理连接，支持回调、手动提交和回滚、任意层级保存点回退及并发错误重试。参考旧版 `6d1a8192` 的 `src/database/mod.rs` 中事务回调显式传入句柄，以及 `src/database/sqlite/provider_tests.rs` 的保存点隔离回归。SQLx 的事务管理器跟踪物理层级。MySQL 中断或死锁的租约关闭后重新取连接，SQLite 通过 worker 排队回滚，并校验归还后的事务深度。
- Rust 的回调通过借用 `Transaction` 独占会话，异步返回 `DatabaseFuture`。调用方显式设置总尝试次数，零次返回配置错误。上层仍以 `Result` 传播驱动错误。
- 读库空闲连接丢失时池可换用新物理连接。运行中连接断开后的自动语句重放、显式连接刷新、`::direct`、连接和事务事件及跨 Query Builder/Schema 的 `ConnectionInterface` 仍待接通。事务中的 SQL 必须通过事务句柄发送。MySQL 隐式提交语句沿用数据库本身的行为。

## 验收

macOS arm64、Rust 1.94.0。候选目录 `/private/tmp/openrust-02-01.mS5Z18` 复用 02-01 独立源码和缓存，仅叠加本批变更。门面导出按本批补丁应用，排除其他模块在同一文件中的工作区修改。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。333 项通过，0 项忽略。启用 `OPENRUST_REQUIRE_MYSQL=1` 并连接专用 MySQL 8.4.13。
- 实际 SQLite 验证提交、回调失败回滚、3 层保存点回退和丢弃活动事务后的内存库复用。MySQL 验证读写会话分离、写后 sticky、零行更新、强制写读、`KILL CONNECTION` 后读库换连接、锁等待超时重试、真实 1213 嵌套死锁后外层重试，以及事务查询取消后换连接且写入回滚。
- 7 次回环 HTTP 请求验证两应用独立的写库事务、读库与 sticky 作用域。Artisan 子进程 `db:probe` 通过事务读取实际 SQLite，`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语数据库正文闭合读写连接、多连接和事务。每种语言 2 个 Rust 示例实际编译执行。`npm run build` 生成三语数据库页面，`node tests/check_browser.mjs` 在 `/` 与 `/docs/` 共 12 项通过，使用本机 Playwright 驱动和 Chromium。工作区 `cargo +1.94.0 tree --workspace --locked --offline` 和 `git diff --check` 通过。

下一批 02-03 接 Query Builder 基础查询，依赖 02-01 参数化 SQL，验收 SQL 生成、绑定及真实 SQLite 和 MySQL。
