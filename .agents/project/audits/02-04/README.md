# 02-04 Query Builder 高级查询与写入

对照固定 Laravel 13.x 的[联结](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#joins)、[UNION](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#unions)、[子查询](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#advanced-where-clauses)、[写入](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#insert-statements)和[悲观锁](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#pessimistic-locking)。核对 [Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php)、通用及 MySQL/SQLite Grammar 和[上游回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php)。参考固定 [Goravel Tx::Table](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/db/db.go) 复用事务连接。提取旧版 `6d1a8192` 的 `query/subqueries.rs` 来源校验和 `query/insert_id.rs`、`query/insert_id_tests.rs` 的同连接生成 ID 与回滚回归。

- 新增联结和 ON 条件、MySQL LATERAL、派生表、相关 EXISTS、IN 和标量子查询、UNION 和 UNION ALL。Rust 用独立方法承载 PHP 重载，嵌入查询校验来源并将 SQL 和绑定一起交给外层构造器，事务内无需第二次借用连接。
- 新增批量插入、忽略冲突、INSERT SELECT、同次执行返回主键、upsert、条件更新与删除、原子增减、`updateOrInsert`、JSON 更新和清空表。批量 `Record` 使用有序键，SQLite 冲突目标来自调用参数，MySQL 使用表唯一索引，并支持 `use_upsert_alias`。SQLite 的带联结或限制的写入通过 rowid 子查询确定目标。
- MySQL 排他锁和共享锁由同一事务会话持有。SQLite 按固定语法省略行锁子句。SQLite `truncate` 在写连接上原子清空表并重置序列，可回滚并处理同名临时表。MySQL `truncate` 使用原生语句，Rust 在显式事务内拒绝会隐式提交的调用。
- 剩余契约包括向量查询和其他数据库方言。MySQL 全文谓词见 [07-06 验收](../07-06/README.md)，PostgreSQL 全文随对应驱动跟踪。MySQL 的影响行数保留 SQLx 驱动语义。SQLx 在当前版本启用 FOUND_ROWS，匹配但未改变的行数与 PDO 默认值有差异。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-02-04.OlQOOE` 以 02-03 验收副本为前置，本批源码、测试和文档与工作区逐文件核对。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。340 项通过，0 项忽略。测试启用 `OPENRUST_REQUIRE_MYSQL=1`，专用 MySQL 8.4.13 实际执行。
- SQLite 和 MySQL 验证 SQL、绑定顺序、联结和子查询、UNION 排序和分页、写入、upsert、JSON、生成主键、事务回滚及序列重置。额外覆盖 SQLite 同名临时表与事务内清空回滚，MySQL 竞争事务在共享锁及排他锁下得到错误 1205，持锁事务提交后可写。
- 8 次真实回环 HTTP 请求覆盖两应用读写隔离、事务查询、请求级 sticky 以及通过构造器原子写入。Artisan 子进程 `db:probe` 通过构造器执行事务查询。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 6 个新三语示例分别在 SQLite 和 MySQL 上执行，共 12 次通过。`npm run build` 成功。12 项既有浏览器检查和 6 项查询正文、代码块及侧栏访问通过，覆盖 `/` 和 `/docs/`。主工作区锁定依赖解析和 `git diff --check` 通过。

下一批 02-05 Schema 表、列、索引及外键，以 02-01 的 SQLite 和 MySQL 连接为前置。

## JSON WHERE 子句补验

- 对照固定 Laravel 的 `whereJsonContains`、`whereJsonContainsKey`、`whereJsonLength` 及其否定和 OR 变体。复用旧版 `6d1a8192` 的 JSON 条件、箭头路径拆分和数组索引处理方式，保留同一条件树用于嵌套、查询和写入。Goravel 的固定版查询层继续作为应用装配参考。 [Laravel JSON WHERE 子句](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#json-where-clauses) · [Laravel MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/MySqlGrammar.php#L286-L336)
- 路径与候选值按各驱动的占位符顺序绑定。MySQL 使用原生 JSON 包含、路径存在和长度函数。SQLite 使用 `json_each`、`json_type`、`json_array_length`，包含条件接受标量，浮点参数保留 Rust 数值类型。`whereJsonLength` 显式接收比较符，使用 `"="` 进行等值比较。
- macOS arm64，Rust 1.94.0。`cargo test -p openrust-framework --test database_query --locked --offline` 六项通过。使用独立 MySQL 8.4.13 数据库和 SQLite，验证标量与 MySQL 数组包含、否定、嵌套和 OR、JSON null 与缺失路径、带点号及引号的键、多级数组索引、绑定顺序、拒绝非法比较符，以及更新、事务读取和回滚。其余数据库按发布前矩阵验收。
- 定向严格 Clippy 和 `git diff --check` 通过。三语示例均独立编译，运行 SQLite 查询并检查返回值。VitePress 构建通过，保留现有包体积提示。

## upsert 冲突赋值

对照固定 Laravel [Builder::upsert](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L4448)、[MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/MySqlGrammar.php#L430) 与 [SQLite Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/SQLiteGrammar.php#L362)。旧版已有按列名更新及插入列校验，本次复用当前构建器的插入、参数绑定和连接执行路径。

- `upsertWith` 用 `UpsertUpdate` 表达 Laravel 的混合更新数组。支持复制插入列、绑定固定值和执行可信表达式，表达式的绑定按照更新项顺序追加到批量插入参数之后。固定值和表达式可以更新插入记录未包含的列。沿用现有 `upsert` 列名接口、空批处理和显式空更新退回普通插入的行为。
- Rust 1.94.0，SQLite 和真实 MySQL 8.4.13。MySQL 分别开启和关闭 `use_upsert_alias`。验证批量写入、冲突与新记录、表默认值、带引号的固定值、SQL 表达式绑定、显式 NULL、事务回滚与非法输入。63 项定向回归、严格 Clippy 和变更文件格式检查通过。1 个子进程夹具由父测试启动。
- 三语共用示例在实际 SQLite 和 MySQL Artisan 命令中运行，原生 `main artisan inspire` 通过。三语 VitePress 构建、12 项站点浏览器组合及 6 项 upsert 小节检查通过。

## JSON 重叠谓词

对照固定 Laravel [Query Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L2384)、[MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/MySqlGrammar.php#L300) 及 [DatabaseQueryBuilderTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php#L7472)。旧版未找到重叠谓词，用现有 JSON 路径和参数绑定接入同一条件树。

- `whereJsonOverlaps`、`whereJsonDoesntOverlap` 及两种 OR 变体在 MySQL 编译为 `JSON_OVERLAPS`。根文档直接传入，嵌套路径用 `JSON_EXTRACT`，先绑定路径，再绑定 JSON 值或可信 SQL 表达式的参数。SQLite 对这组谓词返回不支持，符合固定 Laravel Grammar 的方言边界。
- Rust 1.94.0、MySQL 8.4.13 与 SQLite。验证标量、数组及对象、JSON null 与缺失路径、SQL NULL、取反和嵌套 OR、表达式的参数顺序、非法 JSON 路径、带前缀的别名、事务更新与回滚，以及 SQLite 的显式错误。63 项定向回归、严格 Clippy 与变更文件格式检查通过。1 个子进程夹具由父测试启动。
- 实际 Artisan 运行三语共用示例，校验 MySQL 命中记录与 SQLite 的方言错误。原生 `main artisan inspire`、三语 VitePress 构建、12 项站点浏览器组合和 6 项 JSON 重叠小节检查通过。

## MySQL 跨库子查询

对照固定 Laravel [Builder::parseSub 与 prependDatabaseNameIfCrossDatabaseQuery](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L426)。旧版保留了连接来源校验，未找到自动加库名的实现。本批沿用子查询 SQL 与绑定合并以及同会话执行。

- 同端点 MySQL 的跨库构建器自动限定内层主表。已限定的主表保持原样，保留表前缀及别名。编译不修改原构建器，数据查询与写入仍由外层连接执行。额外的 JOIN 表和预先构建的派生查询需显式限定。
- Rust 通过当前连接池的 TCP 或 Unix socket 端点配置核对两侧来源，端点配置不一致时返回错误。两侧需要明确数据库。SQLite 的其他连接仍走既有来源限制。
- Rust 1.94.0，MySQL 8.4.13 验证查阅另一库、绑定顺序、已限定表、别名和前缀、同库独立连接、其他端口拒绝、内层账号不参与执行、事务内 INSERT SELECT 及回滚。MySQL 与 SQLite 共 64 项定向回归、严格 Clippy 和变更文件格式检查通过。1 个子进程夹具由父测试启动。
- 三语共用示例通过实际 Artisan 在同一 MySQL 实例的两个独立库中运行。原生 `main artisan inspire`、三语站点构建、12 项站点浏览器组合和 6 项小节检查通过。
