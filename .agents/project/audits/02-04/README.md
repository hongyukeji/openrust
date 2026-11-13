# 02-04 Query Builder 高级查询与写入

对照固定 Laravel 13.x 的[联结](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#joins)、[UNION](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#unions)、[子查询](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#advanced-where-clauses)、[写入](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#insert-statements)和[悲观锁](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#pessimistic-locking)。核对 [Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php)、通用及 MySQL/SQLite Grammar 和[上游回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php)。参考固定 [Goravel Tx::Table](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/db/db.go) 复用事务连接。提取旧版 `6d1a8192` 的 `query/subqueries.rs` 来源校验和 `query/insert_id.rs`、`query/insert_id_tests.rs` 的同连接生成 ID 与回滚回归。

- 新增联结和 ON 条件、MySQL LATERAL、派生表、相关 EXISTS、IN 和标量子查询、UNION 和 UNION ALL。Rust 用独立方法承载 PHP 重载，嵌入查询校验来源并将 SQL 和绑定一起交给外层构造器，事务内无需第二次借用连接。
- 新增批量插入、忽略冲突、INSERT SELECT、同次执行返回主键、upsert、条件更新与删除、原子增减、`updateOrInsert`、JSON 更新和清空表。批量 `Record` 使用有序键，SQLite 冲突目标来自调用参数，MySQL 使用表唯一索引，并支持 `use_upsert_alias`。SQLite 的带联结或限制的写入通过 rowid 子查询确定目标。
- MySQL 排他锁和共享锁由同一事务会话持有。SQLite 按固定语法省略行锁子句。SQLite `truncate` 在写连接上原子清空表并重置序列，可回滚并处理同名临时表。MySQL `truncate` 使用原生语句，Rust 在显式事务内拒绝会隐式提交的调用。
- 剩余契约包括跨连接的子查询自动限定、upsert 的固定值更新项、额外的 JSON 查询谓词、全文及向量查询和其他数据库方言。MySQL 的影响行数保留 SQLx 驱动语义。SQLx 在当前版本启用 FOUND_ROWS，匹配但未改变的行数与 PDO 默认值有差异。

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
