# 02-05 Schema 表、列、索引和外键

核对固定 Laravel 13.x 的[表](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#tables)、[列](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#columns)、[索引](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#indexes)和[外键](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#foreign-key-constraints)。对照 [Blueprint](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php)、[MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php)、[SQLite Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php)、[MySQL 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseMySqlSchemaGrammarTest.php)和[SQLite 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSQLiteSchemaGrammarTest.php)。参考固定 [Goravel Schema](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/schema/schema.go) 通过回调装配 Blueprint，使用驱动语法执行。提取旧版 `6d1a8192` 的 `database/schema.rs`、`schema/grammar.rs` 和 `schema/sqlite_tests.rs` 中写库绑定、参数化目录查询、同一事务读取未提交 DDL 和标识符处理的契约。外键目标表名推断复用旧版锁定的 MIT/Apache-2.0 `pluralizer 0.5.0`。

- 接通应用内 Schema Facade、具名连接及事务 Schema Builder。回调定义表、列、索引和外键，语法编译器负责表前缀、标识符和驱动差异。目录读取走写库，返回结构化表、列、索引与外键。Rust 显式传入字符串长度、精度等参数，使用 `constrainedOn` 表达 PHP 的可选目标参数。
- 支持创建、重命名和删除表，新增、修改、重命名和删除列，创建、改名和删除索引，以及创建和删除表级外键。MySQL 通过原生 DDL 执行。SQLite 原生 DDL 可在已有事务中运行。需要重建时在独占写会话中暂停外键检查，使用事务复制数据并恢复索引、触发器和序列，提交前检查外键，最后恢复会话设置。调用方取消等待后，由拥有连接的任务完成收尾。SQLite 失败时回滚整次变更。
- 剩余范围包括完整列类型和修饰符、公共外键检查开关、池化临时表、额外数据库方言，以及 SQLite 已有内联约束列的变更。SQLite 表重建需要在活动事务外运行，MySQL 显式事务中的 DDL 返回错误，避免隐式提交。SQLite 目录无法读取外键名称，按列可移除表级外键。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-02-05.vsXzlJ` 以 02-04 为前置，叠加本批文件及门面导出。主工作区其他门面改动留在原处。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。346 项通过，0 项忽略。测试进程启用 `OPENRUST_REQUIRE_MYSQL=1` 并使用专用 MySQL 8.4.13。
- 实际 SQLite 和 MySQL 覆盖建表、改表、删除、重命名、目录读取、约定名称、默认值、外键拒绝和级联动作。SQLite 还覆盖不支持索引、执行中途失败回滚、写锁等待时取消、保留子表和目录对象、自增序列、约束不满足回滚，以及两条池连接上的外键检查和 ALTER 设置恢复。
- 经 Artisan 实际调度 `schema:probe`，通过 Schema 门面写入和读取 SQLite 文件。常规 `./artisan inspire` 正常退出，工作区的 8 次回环 HTTP 数据库回归通过。
- 三语 3 个 Rust 示例在 SQLite 和 MySQL 上各运行一次。`npm run build` 成功，现有浏览器检查 12 项及迁移正文和侧栏检查 6 项通过，覆盖 `/` 与 `/docs/`。主工作区锁定依赖解析和 `git diff --check` 通过。

下一批 02-06 迁移命令与批次账本，以 02-02 的事务和本批 Schema 为前置。
