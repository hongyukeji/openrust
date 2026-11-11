# 02-03 Query Builder 基础查询

对照固定 Laravel 13.x 的[选择语句](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#select-statements)、[基本 WHERE](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#basic-where-clauses)、[排序、分组和分页边界](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#ordering-grouping-limit-and-offset)及[聚合](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#aggregates)。逐项核对 [Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php)、[通用 Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/Grammar.php)、MySQL/SQLite Grammar 和[上游回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php)。参考固定 [Goravel DB](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/db/db.go) 在连接及事务上装配查询。

- 接通 `DB::table`、`Connection::table`、`Transaction::table`。保留查询与语法编译的职责划分。列名、别名和连接表前缀交给语法编译器，绑定按 SQL 子句顺序送给 SQLx，事务查询用已有物理会话。
- 实现投影、去重、比较、空值、IN、范围、逻辑分组和否定、多列比较、日期、LIKE、排序、分组、HAVING、聚合及分页边界。可显式传入可信 SQL 表达式并单独绑定参数。沿用旧版 `6d1a8192` 的 `src/database/query/comparison.rs` 的操作符白名单、`comparison_tests.rs` 的空值回归及 `multi_column_tests.rs` 的空条件组回归，用当前 Grammar 与 SQLx 实现。两个驱动按固定 Laravel 生成 `offset 0` 和 offset-only，后者保留数据库的语法错误。
- Rust 用 `r#where` 表达保留关键字，显式传入操作符。嵌套回调接收 `Conditions`，普通查询拥有共享连接句柄，事务查询借用独占句柄。返回行向量、可选行和类型化标量。空值及精确小数由驱动类型表示。运算符、排序方向和标识符形状错误用 `Result` 在执行前返回。
- 三语正文闭合运行基础查询、聚合、选择、基础条件和排序分组的已验收小节。联结、子查询、UNION、写入与锁属于 02-04。JSON 谓词、基于当前时间的日期条件、流式查询、完整分页和其他数据库语法仍需按对应单元验收。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-02-03.1pbjN3` 以 02-02 验收副本为前置，逐文件核对本批源码与文档一致。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。337 项通过，0 项忽略。MySQL 用例启用 `OPENRUST_REQUIRE_MYSQL=1`，连接专用 MySQL 8.4.13。
- SQLite 和 MySQL 分别核对引用、绑定顺序、嵌套条件、NULL、空 IN、大小写 LIKE、日期、分组聚合、分页和原始表达式。真实回滚测试确认事务构造器可看到尚未提交的写入。覆盖非法操作符、方向、缺少绑定和 offset-only 驱动错误。
- 8 次回环 HTTP 请求核对两应用的读写连接、事务内读取、请求级 sticky 和显式写库读取。Artisan 子进程 `db:probe` 使用事务查询构造器。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 6 个三语 Rust 示例分别在 SQLite 和 MySQL 上运行，共 12 次通过。`npm run build` 成功。现有浏览器组合 12 项通过，另有 6 次访问核对三语查询页面、代码块和侧栏链接，覆盖 `/` 和 `/docs/`。主工作区的锁定依赖解析和 `git diff --check` 通过。

下一批 02-04 高级查询与写入，依赖 02-02 的事务执行及本批查询状态、语法编译和绑定。
