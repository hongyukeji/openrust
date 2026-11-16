# 02-02 多连接、读写路由和事务

核对固定 Laravel 13.x 的[读写连接](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#read-and-write-connections)、[多连接](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#using-multiple-database-connections)和[事务](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#database-transactions)。对照 [ConnectionFactory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/ConnectionFactory.php)、[DatabaseManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseManager.php)、[ManagesTransactions](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/ManagesTransactions.php)和[连接回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseConnectionTest.php)。装配沿用 02-01 参考[固定 Goravel Database Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)的应用级连接管理器。

- `ConnectionFactory` 合并 `read`、`write` 与公共配置，MySQL 接受主机列表。读操作优先使用读池，具名连接允许 `::read` 和 `::write`。`selectFromWriteConnection` 强制单次查询走写库。`sticky` 和强制走写库状态以逻辑连接为单位，由当前 HTTP/Artisan 作用域保管。独立管理器以逻辑连接的克隆共享状态。
- 事务借用一个写库物理连接，支持回调、手动提交和回滚、任意层级保存点回退及并发错误重试。参考旧版 `6d1a8192` 的 `src/database/mod.rs` 中事务回调显式传入句柄，以及 `src/database/sqlite/provider_tests.rs` 的保存点隔离回归。SQLx 的事务管理器跟踪物理层级。MySQL 中断或死锁的租约关闭后重新取连接，SQLite 通过 worker 排队回滚，并校验归还后的事务深度。
- Rust 的回调通过借用 `Transaction` 独占会话，异步返回 `DatabaseFuture`。调用方显式设置总尝试次数，零次返回配置错误。上层仍以 `Result` 传播驱动错误。
- 读库空闲连接丢失时池可换用新物理连接。运行中连接断开后的自动语句重放、显式连接刷新、`::direct`、查询事件及跨 Query Builder/Schema 的 `ConnectionInterface` 仍待接通。事务中的 SQL 必须通过事务句柄发送。MySQL 隐式提交语句沿用数据库本身的行为。

## 验收

macOS arm64、Rust 1.94.0。候选目录 `/private/tmp/openrust-02-01.mS5Z18` 复用 02-01 独立源码和缓存，仅叠加本批变更。门面导出按本批补丁应用，排除其他模块在同一文件中的工作区修改。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。333 项通过，0 项忽略。启用 `OPENRUST_REQUIRE_MYSQL=1` 并连接专用 MySQL 8.4.13。
- 实际 SQLite 验证提交、回调失败回滚、3 层保存点回退和丢弃活动事务后的内存库复用。MySQL 验证读写会话分离、写后 sticky、零行更新、强制写读、`KILL CONNECTION` 后读库换连接、锁等待超时重试、真实 1213 嵌套死锁后外层重试，以及事务查询取消后换连接且写入回滚。
- 7 次回环 HTTP 请求验证两应用独立的写库事务、读库与 sticky 作用域。Artisan 子进程 `db:probe` 通过事务读取实际 SQLite，`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语数据库正文闭合读写连接、多连接和事务。每种语言 2 个 Rust 示例实际编译执行。`npm run build` 生成三语数据库页面，`node tests/check_browser.mjs` 在 `/` 与 `/docs/` 共 12 项通过，使用本机 Playwright 驱动和 Chromium。工作区 `cargo +1.94.0 tree --workspace --locked --offline` 和 `git diff --check` 通过。

下一批 02-03 接 Query Builder 基础查询，依赖 02-01 参数化 SQL，验收 SQL 生成、绑定及真实 SQLite 和 MySQL。

## 事务生命周期事件

对照固定 Laravel 13.40.0 的 [ManagesTransactions](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/ManagesTransactions.php) 与[事务章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#database-transactions)。旧版已经区分提交前监听失败和提交后监听失败。当前沿用这一区别，通过既有应用事件分发器交付 Laravel 对应的四个类型。

- `TransactionBeginning` 在开始事务或保存点后触发。`TransactionCommitting` 仅在最外层物理提交之前触发。`TransactionCommitted` 在层级下降并执行适用的提交回调后触发。`TransactionRolledBack` 在回滚操作成功后按最终层级发一次。回滚目标等于当前层级时无事件。
- 事件包含应用连接和快照层级。活动事务通过事件上下文借用，最外层结束后使用普通事件上下文。提交前监听失败时，受管事务按原失败路径回滚。提交后监听失败走 `AfterCommit`，避免已提交事务被重试。未确认的驱动取消与租约回收沿用 SQLx 清理路径。
- macOS arm64、Rust 1.94.0、实际 SQLite 与独立 MySQL 8.4.13。数据库 SQL、查询、写入、Schema 和事件回归共 39 项通过，1 个现有 Artisan 夹具按设计由父测试启动。实测两应用事件隔离、嵌套提交、跨层回滚、提交前后监听失败、MySQL 锁等待失败后的回滚与再提交、实际回环 HTTP 和 Artisan 子进程。首次测试后复跑最终源码。工作区严格 Clippy 与本批文件格式检查通过。
- 全工作区测试在 `tests/startup.rs` 的 `stub:publish` 数量断言停止。用本批基线 `3fa4fa148` 单独复验同一测试，也得到实际 32、预期 28。
- 本批三语事务事件示例通过实际 Artisan 在 MySQL 和 SQLite 各运行一次，主程序 `main artisan inspire` 成功退出。VitePress 构建及根路径、子路径下 12 项站点浏览器检查和 6 项事务事件小节检查通过。
- `ConnectionEstablished` 见下方续批。剩余查询事件、连接重连，以及 `beforeStartingTransaction` 和 `afterRollBack` 回调继续按 02-02 验收。02-02 仍在研。

## 连接建立事件

固定 Laravel [DatabaseManager::connection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseManager.php#L94) 在缓存逻辑连接后派发 `ConnectionEstablished`。旧版无同名事件可直接复用，当前复用应用事件分发器与惰性连接池。

- Rust 同步获取惰性句柄。该句柄首次异步使用前向所属应用派发事件，也可通过 `Connection::establish().await` 显式等待。事件先于读写池选择和借用，因此监听器可直接等待同一连接的 SQL。并行首查等待该连接的初始化，连接缓存仍按应用与名称划分。
- 已完成的派发只执行一次。监听器返回的错误交给发起操作，后续操作继续使用已缓存的连接。派发被取消时，后续异步操作重新初始化。连接池替换物理会话沿用现有机制。
- 隔离 SQLite 与 MySQL 8.4.13 实测首次并发查询、监听器同连接初始化、应用隔离、派发错误及取消后的恢复。两套实际 HTTP 应用和 Artisan 子进程实测各自只收到一次。42 项定向回归通过，1 个现有子进程夹具按设计由父测试启动。工作区严格 Clippy 与变更文件格式检查通过。
- 三语文档示例均通过实际 Artisan 分别在 SQLite 和 MySQL 运行。实际 `main artisan inspire` 退出成功。VitePress 构建、12 项站点浏览器组合和 6 项连接事件小节检查通过。全工作区测试的既有 `stub:publish` 断言问题见上批。
- 查询事件与累计耗时见下方续批。02-02 继续处理显式重连及事务前置与回滚回调。

## 查询事件与累计耗时

对照固定 Laravel 的[查询监听](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#listening-for-query-events)和[累计查询耗时](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#monitoring-cumulative-query-time)。旧版无同名查询事件或累计回调可直接复用。沿用已有应用事件分发器、绑定参数、读写路由和事务上下文。

- `DB::listen` 与 `Connection::listen` 订阅应用的 `QueryExecuted`。事件记录成功 SQL 的原始文本和绑定、执行耗时、实际读写角色及逻辑连接。事务内回调通过上下文访问当前事务。池化查询先归还借用连接，回调可在单连接池继续查询。SQL 驱动失败时无查询事件和计时增量。
- 使用 Rust `Duration` 和单调时钟；时间覆盖池等待及 SQL 执行，排除连接建立监听与查询监听耗时。流式游标在成功耗尽后派发，计时排除调用方处理行的时间。这是 SQLx 继续向调用方流式交付并释放行流借用所需的时序调整。未耗尽或失败游标没有成功事件。
- `whenQueryingForLongerThan` 在逻辑连接的累计用时超过阈值时调用一次处理器，支持 `totalQueryDuration`、`resetTotalQueryDuration` 和 `allowQueryDurationHandlersToRunAgain`。并发或监听器重入共享该处理器的执行状态。
- Rust 1.94.0、实际 SQLite 与 MySQL 8.4.13。44 项定向回归验证命名绑定、读写池和具名读连接、失败 SQL、流式游标、监听器重入、事务上下文、累计与重置、并发重新启用、真实 HTTP 以及 Artisan 子进程。1 个现有夹具由父测试启动。工作区严格 Clippy、变更文件格式检查通过。
- 三语 Artisan 示例在两种数据库上用实际连接池等待触发 500 毫秒累计阈值。原生 `main artisan inspire` 通过。VitePress 构建、12 项站点浏览器组合和 6 项小节检查通过。
- SQL 渲染与查询日志见下方续批。02-02 仍需显式重连与剩余事务回调验收。

## 查询诊断与日志

对照固定 Laravel 的 [QueryExecuted::toRawSql](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Events/QueryExecuted.php) 与 [Connection 查询日志](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L1730)。旧版无对应入口可直接复用。本批复用已验收的命名绑定解析以及当前查询事件。

- `QueryExecuted::toRawSql` 与 `getRawQueryLog` 使用同一个诊断渲染器。普通注释、字符串和引号标识符的占位符保留原样。按 MySQL 与 SQLite 规则处理引号、反斜杠、字节、空值、布尔值及 SQLite 编号绑定。带空字符的文本返回诊断错误，原始绑定和日志条目仍可读取。
- 查询日志默认关闭，按逻辑连接保存。支持经 Connection 或 DB 开启、读取、关闭和清空，关闭后仍可读取已记录条目。成功事件监听完毕后记录当前查询。失败查询不增加条目。诊断渲染保持在应用主动调用的路径，SQL 执行仍使用预处理绑定。
- Rust 1.94.0，实际 SQLite 与 MySQL 8.4.13。46 项定向回归通过，1 个现有子进程夹具由父测试启动。真实数据库对照了原始绑定和诊断结果、SQLite 编号及混合参数、逻辑连接隔离、启停清空与监听顺序。工作区严格 Clippy、变更文件格式检查通过。
- 六次三语示例执行覆盖两种驱动，经实际 Artisan 校验读取和清空。原生 `main artisan inspire` 成功。三语 VitePress 构建、12 项站点浏览器组合与 6 项查询日志小节检查通过。
- 通用 `pretend`、查询前回调、显式重连与剩余事务回调待按固定 Laravel 验收。当前迁移的 Schema 预演继续使用独立路径。
