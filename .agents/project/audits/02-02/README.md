# 02-02 多连接、读写路由和事务

核对固定 Laravel 13.x 的[读写连接](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#read-and-write-connections)、[多连接](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#using-multiple-database-connections)和[事务](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#database-transactions)。对照 [ConnectionFactory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/ConnectionFactory.php)、[DatabaseManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseManager.php)、[ManagesTransactions](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/ManagesTransactions.php)和[连接回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseConnectionTest.php)。装配沿用 02-01 参考[固定 Goravel Database Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)的应用级连接管理器。

- `ConnectionFactory` 合并 `read`、`write` 与公共配置，MySQL 接受主机列表。读操作优先使用读池，具名连接允许 `::read` 和 `::write`。`selectFromWriteConnection` 强制单次查询走写库。`sticky` 和强制走写库状态以逻辑连接为单位，由当前 HTTP/Artisan 作用域保管。独立管理器以逻辑连接的克隆共享状态。
- 事务借用一个写库物理连接，支持回调、手动提交和回滚、任意层级保存点回退及并发错误重试。参考旧版 `6d1a8192` 的 `src/database/mod.rs` 中事务回调显式传入句柄，以及 `src/database/sqlite/provider_tests.rs` 的保存点隔离回归。SQLx 的事务管理器跟踪物理层级。MySQL 中断或死锁的租约关闭后重新取连接，SQLite 通过 worker 排队回滚，并校验归还后的事务深度。
- Rust 的回调通过借用 `Transaction` 独占会话，异步返回 `DatabaseFuture`。调用方显式设置总尝试次数，零次返回配置错误。上层仍以 `Result` 传播驱动错误。
- 读库空闲连接丢失时池可换用新物理连接。运行中断线重放、`::direct`、查询事件及跨 Query Builder／Schema 的 `ConnectionInterface` 见[整单元验收](#连接接口与-02-02-整单元验收)。事务中的 SQL 必须通过事务句柄发送。MySQL 隐式提交语句沿用数据库本身的行为。

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
- 显式重连见下方续批。通用 `pretend`、查询前回调与剩余事务回调待按固定 Laravel 验收。当前迁移的 Schema 预演继续使用独立路径。

## 断开、重连及清除缓存

核对固定 Laravel [DatabaseManager 的连接生命周期](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseManager.php#L307)。旧版未找到可直接复用的同名入口。本批复用连接配置解析、读写池构建及既有连接建立事件。

- `disconnect` 退役已缓存连接的当前读写池，保留逻辑连接。`reconnect` 用当前配置刷新池，旧句柄仍共享同一逻辑连接、查询日志及耗时。`purge` 再清除缓存，下一次解析创建新逻辑连接。驱动变化通过 `purge` 重新建立相应的逻辑方言。
- SQLx 按池管理物理会话。等待旧池的查询在实际执行 SQL 前切换到新池。已经借出的会话仍可完成原操作或事务，并在归还后关闭。每次建立替换池时派发一次 `ConnectionEstablished`。
- Rust 1.94.0，实际 SQLite 与 MySQL 8.4.13。SQLite 用两个数据库文件验收重读配置、旧句柄及日志保留、原事务提交和清除缓存。MySQL 通过会话 ID 验证读写池刷新、单连接池等待切换、事务留在原会话、租约归还后服务端会话关闭和事件计数。50 项定向回归通过，含真实 HTTP 的模型绑定回归。1 个现有子进程夹具由父测试启动。工作区严格 Clippy 和变更文件格式检查通过。
- 两种驱动分别通过实际 Artisan 运行三语示例，原生 `main artisan inspire` 成功。VitePress 构建、12 项站点浏览器组合与 6 项重连小节检查通过。
- 事务回调见下方续批。02-02 仍需核对运行中连接丢失的语句重放、`::direct`、通用查询预演和查询前回调。

## 事务开始前及回滚回调

对照固定 Laravel [ManagesTransactions](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/ManagesTransactions.php) 与 [DatabaseTransactionsManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseTransactionsManager.php)。旧版未找到可直接复用的同名入口。当前沿用现有事务和提交回调，保存每层的父子关系。

- `beforeStartingTransaction` 在每次根事务和保存点开始前执行。根事务钩子在借用连接前运行，嵌套钩子可借用当前事务。错误中止本次开始。连接克隆共享注册，门面注册到默认逻辑连接。
- `Transaction::afterRollBack` 的作用域是显式事务句柄。回滚时按保存点父子关系处理，包括已提交到父事务的保存点。根回滚依层级从深至浅执行，已提交的根事务丢弃回滚回调。确认回滚后的回调错误返回 `AfterRollback`。未确认的租约中断继续走驱动清理。
- 真正的 MySQL 死锁确认事务已撤销，退役旧会话后执行该次回滚回调，再按现有策略重试。SQLite 表重建的外层事务在回调前恢复会话设置及归还连接，回调可继续查询单连接池。
- Rust 1.94.0、SQLite 与真实 MySQL 8.4.13 上 53 项定向回归通过。覆盖开始钩子、中止、已提交子保存点、部分回滚、根回滚、回调错误、真实锁等待和死锁重试、SQLite 表重建恢复，以及事件的 HTTP 与 Artisan 回归。1 个现有子进程夹具由父测试启动。工作区严格 Clippy 和变更文件格式检查通过。
- 两种驱动分别通过实际 Artisan 运行三语示例，原生 `main artisan inspire` 成功。VitePress 构建、12 项站点浏览器组合与 6 项事务回调小节检查通过。执行中连接丢失见下方续批。

## 执行中断线与语句重放

对照固定 Laravel [Connection 的断线处理](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L1017)。旧版未提供对应的通用语句重放。本批复用现有的结构化断线检测及连接池换代。

- 事务外语句遇到驱动确认的连接丢失时，退役该语句使用的池并重放一次。并发失败共享替换池。成功后只记录一次查询事件和日志。执行写入后丢失响应时，重放可能重复写入。
- 已开始的事务维持原会话和断线错误。最外层 `BEGIN` 支持断线重试，实际驱动注入覆盖已执行语句及活动事务。游标在首行交付前可重放，交付首行后的断线通过流返回。连接建立监听器内的语句沿用单次执行，避免递归刷新。
- Rust 1.94.0。独立 MySQL 8.4.13 在服务器已执行 SQL 时终止会话，验证读写重放、一次上限、并发共享一次刷新、活动事务回滚及游标首行前后。SQLite 和 MySQL 等 55 项定向回归通过，含真实 HTTP 与事件回归。1 个既有子进程夹具由父测试启动。严格 Clippy 与变更文件格式检查通过。
- 真正 Artisan 命令在执行 MySQL 查询期间断线并成功重放，原生 `main artisan inspire` 通过。三语 VitePress 构建、12 项站点浏览器组合及 6 项断线小节检查通过。
- 02-02 继续处理 `::direct`、通用预演和查询前回调。

## 直连别名的写连接回退

对照固定 Laravel [DatabaseManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseManager.php) 与 [ConfiguresPooledConnections](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/Concerns/ConfiguresPooledConnections.php)。旧版数据库模块未找到对应实现。

- MySQL 与 SQLite 的 `name::direct` 读写都落在写端点。连接名保留后缀，查询日志和事件识别 `QueryRole::Direct`。别名沿用独立的逻辑缓存键及现有重连机制。
- Laravel 的独立直连端点限于 PostgreSQL。本批覆盖当前驱动的写端点回退。PostgreSQL 独立端点、连接池配置及自动选用直连的迁移行为随该驱动验收。
- Rust 1.94.0，真实 MySQL 8.4.13 和 SQLite 共 56 项定向回归通过，含真实 HTTP 和事件回归。1 个子进程夹具由父测试启动。工作区严格 Clippy 和变更文件格式检查通过。
- MySQL Artisan 验收让常规读连接进入 `information_schema`，校验别名读写走应用库、日志角色、重连和清除逻辑缓存。原生 `./artisan inspire` 成功。三语 VitePress 构建、12 项浏览器组合和 6 项直连小节检查通过。
- 02-02 仍待通用预演、查询前回调及 PostgreSQL 独立直连。

## 查询执行前回调

对照固定 Laravel [Connection 的 `run` 与 `beforeExecuting`](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L806) 和 [DatabaseConnectionTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseConnectionTest.php#L661)。旧版数据库代码未发现同名实现。

- `Connection::beforeExecuting` 与 `DB::beforeExecuting` 按注册顺序传入原始 SQL 和绑定值。回调返回错误时停止执行及后续回调。回调在获取普通查询会话与开始计时前执行，游标在首次轮询时执行，驱动内部重放沿用同一次逻辑执行。
- Rust 的异步回调额外接收当前显式事务句柄，事务里的回调可在同一会话读取尚未提交的数据。连接克隆共享已注册的回调。应用容器作用域覆盖回调执行。
- Rust 1.94.0，真实 MySQL 8.4.13 与 SQLite 共 58 项定向回归通过。覆盖原始命名绑定、注册顺序、回调拒绝写入、不写成功日志、游标懒执行、单连接池事务内重入与回滚、MySQL 实际断线后同一查询只调用一次，以及既有 HTTP 和事件回归。1 个子进程夹具由父测试启动。严格 Clippy 与变更文件格式检查通过。
- 三语文档共用的代码示例由实际 Artisan 命令分别在 SQLite 和 MySQL 运行，原生 `main artisan inspire` 通过。三语 VitePress 构建、12 项站点浏览器组合和 6 项回调小节检查通过。
- 02-02 继续处理通用查询预演，PostgreSQL 独立直连随驱动接入验收。

## 通用查询预演

对照固定 Laravel [Connection::pretend、withoutPretending 与 withFreshQueryLog](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L682) 及 [DatabaseConnectionTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseConnectionTest.php#L685)。旧版数据库代码未发现可复用的通用预演。既有迁移预演可记录 Schema 语句，本批覆盖通用连接、Facade、查询构建器和显式事务里的 SQL 执行入口。

- 预演收集渲染后的 SQL、原始绑定、读写角色与零执行耗时，保留查询前回调和查询事件。读取返回空结果，语句写入返回成功，影响行数为零，生成 ID 为无值。预演 SQL 不进入实时查询日志。
- Rust 将状态绑定到逻辑连接与当前异步作用域。嵌套预演有独立日志，错误或取消后恢复原状态。显式 `withoutPretending` 允许真实查询写入实时日志，并将同一查询纳入本次预演报告。事务的开始、提交、回滚与需要物理会话的 Schema 操作在实时作用域执行。
- Rust 1.94.0，真实 MySQL 8.4.13 与 SQLite 共 61 项定向回归通过。覆盖同一任务内并发隔离、写入不落库、预演事件、嵌套、错误和取消、原始绑定、游标、事务内预演、不可用 SQLite 数据库预演，以及既有 HTTP、迁移和事件相关回归。1 个子进程夹具由父测试启动。严格 Clippy 和变更文件格式检查通过。
- 两种驱动均通过实际 Artisan 执行三语共用示例并核对更新前后状态和查询事件。原生 `main artisan inspire`、VitePress 构建、12 项站点浏览器组合和 6 项预演小节检查通过。
- 02-02 继续跟踪 PostgreSQL 独立直连及池化路由，随该驱动接入验收。本批保留该单元待闭合状态。

## PostgreSQL 直连端点选择

- 对照固定 Laravel 的 `ConfiguresPooledConnections`、`Connection::hasDirectConnection` 和 `Migrator::resolveConnection`。OpenRust 按具名连接缓存独立的直连池，`::direct` 从连接基础配置合并 PostgreSQL 直连选项，无独立配置时回退到写端点。迁移上下文和账本访问默认选择独立端点，显式角色仍优先。旧版未找到可复用的直连配置。 [Laravel 连接配置](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/Concerns/ConfiguresPooledConnections.php) · [Laravel 迁移连接](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/Migrator.php)
- 隔离基线 `37ef76c7c`。无网络配置回归验证普通连接保留写端点、迁移默认路由至独立端点、显式角色优先、前缀继承、错误驱动拒绝和默认连接名不变。首轮发现空 `direct` URL 误走基础端点，调整后回退至写端点，最终快照通过。
- 真实 MySQL 8.4.13 的读写路由与断线恢复、`::direct` 回退、独立 Artisan 的迁移账本回归均通过。PostgreSQL 独立端点连通性、`pooled` 的事务池兼容及运行验收仍待发布矩阵。
- Rust 1.94.0 全仓格式、严格 Clippy、`main artisan inspire`、三语站点及 18 项浏览器检查通过。本批临时 MySQL、3.3 GiB Cargo 产物及文档缓存已回收。磁盘可用 63.22 GB（58.88 GiB），周额度剩余 47%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-02-direct-route.Arix2t9P`。

## 真实 PostgreSQL 直连与 PgBouncer 事务池

- 沿用固定 Laravel 的连接角色和迁移选择机制。隔离 PostgreSQL 18.6 配合 PgBouncer 1.26.1 使用 SCRAM、`pool_mode=transaction`、单条后端连接及 `max_prepared_statements=64`。将池端点的同名连接映射到另一物理数据库，24 个并发 HTTP 请求以实际数据库名与整数绑定区分普通查询和显式直连。迁移只在直连数据库写入账本。 [Laravel 直连配置](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/Concerns/ConfiguresPooledConnections.php) · [PgBouncer 语句跟踪](https://www.pgbouncer.org/config.html#max_prepared_statements)
- 验证应用与服务器的搜索路径一致时，池化连接使用预期 Schema，直连可使用独立路径。夹具的 HTTP Web 路由需要有效 `APP_KEY`。SQLx 使用原生预备语句，本次通过 PgBouncer 的具名语句跟踪获得兼容。Laravel 的 PDO 模拟预备策略和未启用语句跟踪的事务池仍待验收。
- 三语连接文档已列出 PgBouncer 配置和迁移直连要求。隔离站点 12 项通用和 6 项连接文档浏览器检查通过。
- 验收脚本、日志和快照保留在 `/private/tmp/openrust-02-05-postgres-types.ZYsFwGbc`。

## PgBouncer 关闭预备语句跟踪

- 对照固定 Laravel v13.40.0 的 `ConfiguresPooledConnections`。PostgreSQL 的 `pooled=true` 或独立 `direct` 配置使池化读写默认模拟预备。直连默认采用原生预备。Rust 允许在连接及各端点用 `emulate_prepares` 显式覆盖。复核旧版 `6d1a81924`，未找到可复用的事务池模拟预备实现。 [固定 Laravel 连接配置](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connectors/Concerns/ConfiguresPooledConnections.php)
- 模拟模式使用 PostgreSQL 简单查询协议。框架解析命名、位置和原生编号绑定，按类型编码值，避开 SQL 引号和嵌套注释中的占位符。扩展和用户自定义类型按服务器返回的文本读取。框架诊断继续使用原 SQL 和独立绑定，模拟模式关闭 SQLx 对已展开语句的日志。 [PgBouncer 具名语句跟踪](https://www.pgbouncer.org/config.html#max_prepared_statements)
- 真实 PostgreSQL 18.6 和 PgBouncer 1.26.1，事务池、两个后端并轮换，`max_prepared_statements=0`。通过文字、整型、浮点、布尔、NULL、二进制、JSON、自定义枚举、命名绑定、重复编号、流式读取、重复事务及失败回滚后的查询回归。24 个并发实际 HTTP 请求跨两个后端，验证池化和直连各自的数据库及绑定值。独立 Artisan 迁移只写直连账本，`artisan inspire` 通过。
- 框架查询回归、MySQL 8.4.13 表目录与限定 Schema 清库、Rust 1.94.0 全仓格式、严格 Clippy、三语站点及 18 项浏览器检查通过。本批临时容器、隔离产物和临时镜像标签已回收，磁盘空间净增 3.66 GB（3.41 GiB），可用 104.90 GB（97.70 GiB）。提交前周额度剩余 43%，第二窗口无读数。日志保留于 `/private/tmp/openrust-02-02-pgbouncer-untracked.HOsPMhtp`。

## SQL Server 自动回滚保留原始错误

- 固定 Laravel 在最外层回滚前通过 PDO 检查服务器事务状态。Rust TDS 在回滚批次中检查 `XACT_STATE()`，服务器已经撤销的事务直接清理本地句柄，仍可提交的事务按保存点回滚，无法提交的嵌套事务执行整条回滚。确认整条回滚时清空各层状态并运行相关回滚回调。旧版 `v0.x` 未找到对应 TDS 状态处理。 [固定 Laravel 事务](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/ManagesTransactions.php) · [SQL Server XACT_STATE](https://learn.microsoft.com/en-us/sql/t-sql/functions/xact-state-transact-sql)
- SQL Server 2022 Developer 16.0.4325.4 在 Apple Silicon 的 Linux amd64 兼容容器中通过七项 Schema／TDS 和一项迁移事务回归。嵌套事务实测服务器撤销整条事务后保留原始错误，内外层回滚回调各一次，原句柄关闭，连接池可继续承接新事务。普通保存点回滚与外层提交仍通过。另在真实 SQL Server 的 TRY/CATCH 批次由重复主键错误 2627 触发 `XACT_STATE() = -1`，执行框架生成的嵌套回滚 SQL 后回到状态 0，原事务未留下写入。
- 实际 Artisan 与 HTTP 使用无效的限定外键目标复验，保留原始 SQL Server 错误 1767，失败写入未提交、迁移账本无新增。迁移重试、回滚和后续请求通过。MySQL 8.4.13 的事务事件、保存点回调和嵌套死锁重试回归及全仓格式、严格 Clippy、三语站点构建和 18 项浏览器检查通过。
- 原生 Linux x86_64 及 SQL Server 2017 留待发布复验，核心账本维持 62/66。临时数据库和本批产物已回收，复验清理后项目盘可用空间净增 2.27 GB（2.12 GiB），剩余 101.88 GB（94.88 GiB）。提交前周额度剩余 41%，第二窗口无读数。日志和夹具位于 `/private/tmp/openrust-02-02-sqlsrv-rollback.lFm05PkI`。

## 连接接口与 02-02 整单元验收

- 对照固定 Laravel v13.40.0 的 `ConnectionInterface`。旧版 `6d1a81924` 没有同名 trait 可直接复用，本批沿用当前连接、事务、Query Builder、Schema Builder 和驱动绑定。普通 `Connection` 与 `Transaction` 实现同一接口，可通过 `&mut dyn ConnectionInterface` 调用。查询方法沿用 Laravel 命名，显式 `Bindings` 支持位置与命名参数。PHP 连接管理事务层级，Rust 由 `Transaction` 持有物理会话，事务开始、提交、回滚与预演沿用现有具体类型 API。 [固定 Laravel 接口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/ConnectionInterface.php)
- 隔离基线 `49adae04a`，真实 MySQL 8.4.13。通过实际会话 ID 验证 `scalar` 和流式 `cursor` 的读写选择，以及事务里的读取仍走同一写会话。事务内通过 Schema Builder 验证表存在，命名参数插入与带别名的 Query Builder 读取通过，回滚后行数为零。同一三语示例编入实际二进制，HTTP 返回 200，独立 Artisan `db:interface` 退出 0，共用接口分别读取普通连接和事务。数据库 SQL 套件 32 项通过。首轮发现未知驱动回归仍使用已支持的 `sqlsrv`，已改用未知驱动名。
- 按交付计划复核读写隔离、sticky、事务提交／回滚、保存点、重试和取消后的会话恢复。当前 MySQL 回归与既有[事务生命周期](#事务生命周期事件)、[在途断线重放](#执行中断线与语句重放)、[PostgreSQL 直连](#真实-postgresql-直连与-pgbouncer-事务池)、[PgBouncer 无语句跟踪](#pgbouncer-关闭预备语句跟踪)及[SQL Server 自动回滚](#sql-server-自动回滚保留原始错误)相互补足。02-02 核心验收闭合，完整数据库版本与跨平台发布矩阵沿用[数据库方言对账](../database-dialects.md)。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。核心账本更新为 63/66。临时 MySQL 与本批隔离编译和文档产物已回收，项目磁盘可用空间净增 8.65 GB（8.06 GiB），剩余 101.76 GB（94.77 GiB）。提交前周额度剩余 41%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-02-interface.YzO4B68u`。
