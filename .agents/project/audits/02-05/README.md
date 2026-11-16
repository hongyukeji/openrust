# 02-05 Schema 表、列、索引和外键

核对固定 Laravel 13.x 的[表](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#tables)、[列](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#columns)、[索引](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#indexes)和[外键](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#foreign-key-constraints)。对照 [Blueprint](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php)、[MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php)、[SQLite Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php)、[MySQL 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseMySqlSchemaGrammarTest.php)和[SQLite 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSQLiteSchemaGrammarTest.php)。参考固定 [Goravel Schema](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/schema/schema.go) 通过回调装配 Blueprint，使用驱动语法执行。提取旧版 `6d1a8192` 的 `database/schema.rs`、`schema/grammar.rs` 和 `schema/sqlite_tests.rs` 中写库绑定、参数化目录查询、同一事务读取未提交 DDL 和标识符处理的契约。外键目标表名推断复用旧版锁定的 MIT/Apache-2.0 `pluralizer 0.5.0`。

- 接通应用内 Schema Facade、具名连接及事务 Schema Builder。回调定义表、列、索引和外键，语法编译器负责表前缀、标识符和驱动差异。目录读取走写库，返回结构化表、列、索引与外键。Rust 显式传入字符串长度、精度等参数，使用 `constrainedOn` 表达 PHP 的可选目标参数。
- 支持创建、重命名和删除表，新增、修改、重命名和删除列，创建、改名和删除索引，以及创建和删除表级外键。MySQL 通过原生 DDL 执行。SQLite 原生 DDL 可在已有事务中运行。需要重建时在独占写会话中暂停外键检查，使用事务复制数据并恢复索引、触发器和序列，提交前检查外键，最后恢复会话设置。调用方取消等待后，由拥有连接的任务完成收尾。SQLite 失败时回滚整次变更。
- 剩余范围包括生成列、空间列等剩余类型与修饰符、池化临时表、额外数据库方言，以及 SQLite 已有内联约束列的变更。SQLite 表重建需要在活动事务外运行，MySQL 显式事务中的 DDL 返回错误，避免隐式提交。SQLite 目录无法读取外键名称，按列可移除表级外键。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-02-05.vsXzlJ` 以 02-04 为前置，叠加本批文件及门面导出。主工作区其他门面改动留在原处。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。346 项通过，0 项忽略。测试进程启用 `OPENRUST_REQUIRE_MYSQL=1` 并使用专用 MySQL 8.4.13。
- 实际 SQLite 和 MySQL 覆盖建表、改表、删除、重命名、目录读取、约定名称、默认值、外键拒绝和级联动作。SQLite 还覆盖不支持索引、执行中途失败回滚、写锁等待时取消、保留子表和目录对象、自增序列、约束不满足回滚，以及两条池连接上的外键检查和 ALTER 设置恢复。
- 经 Artisan 实际调度 `schema:probe`，通过 Schema 门面写入和读取 SQLite 文件。常规 `./artisan inspire` 正常退出，工作区的 8 次回环 HTTP 数据库回归通过。
- 三语 3 个 Rust 示例在 SQLite 和 MySQL 上各运行一次。`npm run build` 成功，现有浏览器检查 12 项及迁移正文和侧栏检查 6 项通过，覆盖 `/` 与 `/docs/`。主工作区锁定依赖解析和 `git diff --check` 通过。

下一批 02-06 迁移命令与批次账本，以 02-02 的事务和本批 Schema 为前置。

## 外键检查会话

对照固定 Laravel [Schema Builder 外键开关及回调](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L642) 和 [Toggling Foreign Key Constraints](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#toggling-foreign-key-constraints)。旧版未找到通用开关。复用当前连接的物理租约、查询记录及 SQLite DDL 事务路径。

- `Schema::withoutForeignKeyConstraints` 在一个独占写会话执行回调。显式 `session()` 支持 Laravel 命名的 `enableForeignKeyConstraints` 和 `disableForeignKeyConstraints`。回调通过会话执行 SQL 或取得 Schema Builder，SQLite 表重建继续通过拥有专用事务的连接 Builder。
- Rust 通过传入会话限定 SQL 的物理连接，恢复进入前的设置。回调错误仍执行恢复。取消时，恢复任务持有原租约直至验证设置。无法验证的会话退役，防止将关闭检查的会话放回池。
- 实际 SQLite 和 MySQL 8.4.13 验证同会话写入及 DDL、作用域退出后拒绝无效外键、原设置关闭时保留原状态、回调失败与取消恢复、MySQL 并行租约的检查隔离、手动开关及关闭会话。
- Rust 1.94.0，67 项定向回归、严格 Clippy、变更文件格式检查通过。1 个子进程夹具由父测试启动。三语共用示例分别在真实 MySQL 和 SQLite 的 Artisan 命令中运行，原生 `main artisan inspire`、VitePress 构建、12 项站点浏览器组合及 6 项外键小节检查通过。

## 扩展列类型

对照固定 Laravel [Blueprint](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php)、[MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php) 和 [SQLite Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php)。旧版未找到本批列类型的框架实现，沿用现有 Blueprint、列修饰符和驱动 DDL 路径。

- 增加短文本、原生 ENUM/SET、年份、IP/MAC、定长及变长二进制、较小整数宽度的自增和无符号方法，以及与两种驱动对应的时间类型别名。SQLite 枚举使用 CHECK，SET 显式返回方言不支持，二进制使用 BLOB。
- Rust 用 `r#enum` 表达关键字方法。保留 `binary(name)`，带长度与定长选项使用 `binaryWith(name, length, fixed)`。枚举和集合候选使用现有字面量转义。年份 `useCurrent()` 使用驱动当前日期表达式。
- Rust 1.94.0，MySQL 8.4.13 与 SQLite 的 69 项定向回归、严格 Clippy 与文件格式检查通过。覆盖真实目录类型、自增主键、当前年份、二进制值、安全转义、枚举拒绝、SQLite 重建保留 CHECK、SET 方言边界。1 个子进程夹具由父测试启动。
- 两种驱动均通过实际 Artisan 执行三语共用示例，验证建表和枚举写入。原生 `main artisan inspire`、VitePress 构建、12 项站点浏览器组合及 6 项列类型小节检查通过。
