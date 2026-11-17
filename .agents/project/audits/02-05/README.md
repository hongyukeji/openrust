# 02-05 Schema 表、列、索引和外键

核对固定 Laravel 13.x 的[表](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#tables)、[列](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#columns)、[索引](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#indexes)和[外键](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#foreign-key-constraints)。对照 [Blueprint](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php)、[MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php)、[SQLite Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php)、[MySQL 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseMySqlSchemaGrammarTest.php)和[SQLite 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSQLiteSchemaGrammarTest.php)。参考固定 [Goravel Schema](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/schema/schema.go) 通过回调装配 Blueprint，使用驱动语法执行。提取旧版 `6d1a8192` 的 `database/schema.rs`、`schema/grammar.rs` 和 `schema/sqlite_tests.rs` 中写库绑定、参数化目录查询、同一事务读取未提交 DDL 和标识符处理的契约。外键目标表名推断复用旧版锁定的 MIT/Apache-2.0 `pluralizer 0.5.0`。

- 接通应用内 Schema Facade、具名连接及事务 Schema Builder。回调定义表、列、索引和外键，语法编译器负责表前缀、标识符和驱动差异。目录读取走写库，返回结构化表、列、索引与外键。Rust 显式传入字符串长度、精度等参数，使用 `constrainedOn` 表达 PHP 的可选目标参数。
- 支持创建、重命名和删除表，新增、修改、重命名和删除列，创建、改名和删除索引，以及创建和删除表级外键。MySQL 通过原生 DDL 执行。SQLite 原生 DDL 可在已有事务中运行。需要重建时在独占写会话中暂停外键检查，使用事务复制数据并恢复索引、触发器和序列，提交前检查外键，最后恢复会话设置。调用方取消等待后，由拥有连接的任务完成收尾。SQLite 失败时回滚整次变更。
- 剩余范围包括其他类型与修饰符以及额外数据库方言。SQLite 表重建需要在活动事务外运行，MySQL 显式事务中的 DDL 返回错误，避免隐式提交。SQLite PRAGMA 外键目录不提供约束名称，具名删除从原始建表语句识别约束。

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

## 生成列

对照固定 Laravel [MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php)、[SQLite Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php) 及 [列修饰符](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#column-modifiers)。旧版未找到对应 API，本批复用查询层的 JSON 路径拆分和 SQLite 重建事务。

- `virtualAs`、`storedAs` 及两个 JSON 变体支持 MySQL 和 SQLite。原始表达式由应用定义，JSON 路径通过已有路径解析与 SQL 转义编译。目录读取提供生成模式和表达式。生成列与默认值、自增、主键和时间更新修饰符冲突时返回错误。
- SQLite 重建以目标表的可写列计算复制列表，保留其他生成表达式和索引，生成值由引擎重新计算。修改列类型时，生成列转换为普通列会复制旧计算值，普通列转为生成列会按新定义计算。
- SQLx 0.9 的 SQLite 执行缓存可能在 DDL 后沿用旧列元数据并使工作线程崩溃。本批 SQLite 读取和游标执行采用非持久化语句，保留已有绑定校验及写入缓存。此修复未进行独立吞吐基准。
- Rust 1.94.0，真实 MySQL 8.4.13 与 SQLite 共 71 项定向回归。覆盖计算值更新、JSON 对象键和数组、目录元数据、SQLite 原生添加虚拟列、相同查询与游标在 DDL 前后读取、索引保留及重建中的生成模式转换。严格 Clippy 和变更文件格式检查通过。1 个子进程夹具由父测试启动。
- 两种驱动通过实际 Artisan 运行三语共用示例。原生 `main artisan inspire`、VitePress 构建、12 项站点浏览器组合与 6 项生成列小节检查通过。

## 空间列

对照固定 Laravel [Blueprint](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php)、[MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L1159) 及 [SQLite Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php#L1058)。旧版未找到框架空间列实现，沿用当前标识符、索引和二进制值处理。

- `geometry` 默认 SRID 0，`geography` 默认 4326。MySQL 使用受支持的原生子类型，未知类型退回 GEOMETRY，非零 SRID 编入 DDL。两个可选参数以 `Option` 表达。
- 列级 `spatialIndex()` 沿用现有的空间索引命名。SQLite 声明 GEOMETRY 并接受 BLOB，声明空间索引返回不支持，创建或变更失败时保留表原状态。
- Rust 1.94.0、MySQL 8.4.13 和 SQLite。验证原生子类型、默认及自定义 SRID、错误 SRID 和子类型拒绝、空间索引创建与删除、几何二进制返回、SQLite 的 BLOB 和索引边界。73 项定向回归、严格 Clippy 和变更文件格式检查通过。1 个子进程夹具由父测试启动。
- 实际 Artisan 运行三语共用示例，MySQL 建表与写入通过，SQLite 返回预期方言错误。原生 `main artisan inspire`、三语 VitePress 构建、12 项站点浏览器组合及 6 项空间列小节检查通过。

## 会话临时表

对照固定 Laravel [MySQL CREATE TABLE](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L242)、[SQLite CREATE TABLE](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php#L231)。旧版未找到框架临时表生命周期实现，本批复用 Schema 写会话与驱动 DDL。

- `Blueprint::temporary()` 由写会话的 Schema Builder 生成原生 TEMPORARY TABLE。会话持有同一物理租约供建表、索引、运行 SQL、改名和删除。迁移预演复用同一语法。SQLite 将临时表放在 temp schema。
- 显式关闭或取消后，清理完受管理的临时表才归还连接。清理使用 SQLite temp schema 或 MySQL DROP TEMPORARY TABLE，保留同名永久表。失败的创建或索引装配收回已创建对象，无法确认清理的会话退役。MySQL `hasTable` 能识别受管理对象，临时表详细信息需在会话中用 SHOW 查询。
- Rust 1.94.0、真实 MySQL 8.4.13 和 SQLite。验证临时表遮蔽同名永久表、原连接复用、改名及删除、重复索引失败后的清理、取消后重新借用、SQLite 临时表列及索引元数据。75 项定向回归、严格 Clippy 与变更文件格式检查通过。1 个子进程夹具由父测试启动。
- 三语共用示例在真实 MySQL 和 SQLite 的 Artisan 命令中运行。原生 `main artisan inspire`、VitePress 构建、12 项站点浏览器组合及 6 项临时表小节检查通过。

## 表选项与列排序规则

对照固定 Laravel [MySQL 建表选项](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L270)、[Blueprint](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L349) 和 [SQLiteProcessor](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Processors/SQLiteProcessor.php#L8)。旧版未找到这些表选项的框架实现，沿用现有 Schema 编译与元数据读取。

- MySQL 建表按 Blueprint、连接配置顺序选择存储引擎、字符集、排序规则。表注释在创建或修改时生成原生 ALTER，目录返回应用后的引擎、排序规则与注释。两种驱动均支持列级排序规则，MySQL 另外支持列字符集。SQLite 忽略 MySQL 专用表选项，列排序规则可从建表语句读取，并可用列变更替换。
- Rust 1.94.0、真实 MySQL 8.4.13 与 SQLite。验证默认及覆盖、表注释的单引号和反斜杠、目录值、大小写比较、SQLite 重建前后排序规则、无效选项预先拒绝。77 项定向回归、严格 Clippy 与变更文件格式检查通过。1 个子进程夹具由父测试启动。
- 三语共用示例在真实 MySQL Artisan 命令中运行。原生 `main artisan inspire`、三语 VitePress 构建、12 项站点浏览器组合与 6 项表选项小节检查通过。

## SQLite 内联约束

对照固定 Laravel [BlueprintState](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/BlueprintState.php) 的列变更和外键移除机制，并核对 [SQLite 列约束语法](https://www.sqlite.org/syntax/column-constraint.html)。旧版未找到框架内联约束重建实现，本批复用已有 SQLite 目录 token、重建事务及索引和触发器恢复。

- 改列时保留 SQLite 内联主键、UNIQUE、CHECK 和 REFERENCES，沿用原有约束名称、冲突动作与外键延迟模式。可按本地列或显式名称移除内联和表级外键。删除内联主键时只移除主键及其自增修饰符。
- 重新建成的表仍有自增主键时恢复原序列。移除自增后，保留原列值并清除旧序列记录。删除或变更路径仍由同一 SQLite 重建事务执行。
- Rust 1.94.0，真实 SQLite 及 MySQL 8.4.13。79 项定向回归、严格 Clippy 和变更文件格式检查通过。覆盖真实约束行为、重建保留旧序列、按名称／列移除、其余约束继续生效、移除主键后的重复值及同表数据。1 个子进程夹具由父测试启动。
- 实际 SQLite Artisan 运行三语共用示例，原生 `main artisan inspire`、三语 VitePress 构建、12 项站点浏览器组合及 6 项小节检查通过。

## MySQL 即时列变更、锁和自增起始值

对照固定 Laravel [即时列操作](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#instant-column-operations)、[DDL 锁](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#ddl-locking) 和 [MySQL 列编译](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L293)。旧版未找到对应列修饰符。本批复用 MySQL 索引和外键已有的锁模式校验，沿用 Schema 的原生 DDL 执行路径。

- 新增或修改 MySQL 列可以请求 `instant()` 和 `lock()`。由服务器判定操作、算法及锁是否兼容并返回原生错误。SQLite 的原生 ALTER 和重建路径均拒绝 MySQL 在线修饰符。
- `from()` 和 `startingValue()` 用 `u64` 表达自增起始值。在 MySQL 的列和索引 DDL 执行后设置表序列。修改已有自增列时保留现有主键，显式 `primary()` 时编译主键请求。SQLite 沿用原生序列。
- 真实 MySQL 8.4.13 验证即时新增、即时修改默认值、锁模式、引擎不支持的操作返回错误、锁模式非法时先校验整批、设置和变更自增起始值及原有数据保留。SQLite 验证原生起始值和两条 ALTER 路径的方言边界。Rust 1.94.0，81 项定向回归通过，1 个子进程夹具由父测试启动。
- 严格工作区 Clippy、变更文件格式检查、真实 MySQL Artisan 三语共用示例、`main artisan inspire`、VitePress 构建和 18 项浏览器检查通过。全工作区格式检查另报既有 Sanctum 测试断言排版，已与本批基线逐字节核对。

## 默认值表达式

对照固定 Laravel [Default Expressions](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#default-expressions) 和 [Schema Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/Grammar.php#L484)。继续沿用已有 Schema 回调和驱动语法装配路径。旧版已有 SQLite 重建时保留默认值表达式的回归，未找到同名 Blueprint 入口。本批用真实插入验证变更后的 SQLite 定义。

- 列和外键 ID 的 `default()` 都接受已有 `Value` 输入，也接受 `Expression::new()` 和 `DB::raw()`。Rust 用类型区分字面量和 SQL。DDL 拒绝带参数绑定、空或含 NUL 的表达式。表达式语法及驱动支持由数据库判断。
- MySQL 8.4.13 和 SQLite 验证 JSON 数组、计算值与普通字符串的区别、外键 ID 的表达式入口、修改默认值后新旧行差异、目录信息和无效定义拒绝。83 项定向回归通过，1 个子进程夹具由父测试启动。
- 三语共用示例分别通过实际 MySQL 和 SQLite Artisan 验证。原生 `main artisan inspire`、严格工作区 Clippy、变更文件格式、VitePress 构建及 18 项浏览器检查通过。全工作区格式仍有既有 Sanctum 测试断言排版差异，已核对文件与基线相同。

## 列组与迁移别名

对照固定 Laravel [Column Order](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#column-order)、[Available Command Aliases](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#available-command-aliases) 及 [Blueprint 多态列](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L1469)。旧版 Schema 未找到对应列组或迁移别名。本批复用应用内的多态类型配置、现有列类型、索引命名及外键删除路径。

- `Blueprint::after` 在 MySQL ALTER 中按声明顺序安排列。SQLite 沿用现有的列位置方言错误。多态列可跟随应用配置或显式选数字、UUID、ULID，支持可空列、命名索引以及 MySQL 位置。原 `morphs(name)` 简写保留，Rust 的额外参数由 `morphsWith` 和显式 `Option` 表达。
- 新增 remember token、可空时间戳别名与对应删除入口。`dropMorphs` 先删除复合索引，再删两列。约定名称复用索引创建的前缀规则，也可提供显式名称。`dropConstrainedForeignId` 复用现有按列删外键机制，随后删列。
- MySQL 8.4.13 和 SQLite 共 85 项定向回归。验证组内新增顺序、组外正常追加、可空多态和时间戳、两种索引命名、外键与列移除、数据保留及 SQLite 位置边界。1 个子进程夹具由父测试启动。三语共用例在两种驱动通过实际 Artisan 验证，原生 `main artisan inspire` 正常退出。
- 严格工作区 Clippy、变更文件格式、三语站点构建和 18 项浏览器检查通过。全工作区格式检查仍报既有 Sanctum 测试排版差异，文件已与本批基线逐字节核对。

## 从模型定义外键列

对照固定 Laravel [foreignIdFor](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#column-method-foreignIdFor)、[Blueprint](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L987) 和 [ForeignIdColumnDefinition](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/ForeignIdColumnDefinition.php#L34)。旧版框架 Schema 未找到对应接口，本批复用 Eloquent 元数据、UUID／ULID 策略、字符串蛇形命名和驱动外键操作。

- Rust 用泛型模型类型和 `Option<&str>` 表达 Laravel 的模型类与可选列名。默认列名由模型默认外键名提供。`foreignIdFor` 根据主键类型选择无符号大整数、UUID 或 ULID。`Model::getKeyType` 可为手动分配的键声明类型；在非主键上生成 UUID 或 ULID 时，不改变主键类型。
- `foreignUuidFor`、`foreignUlidFor` 允许显式选择类型。`constrained()` 使用模型表名与主键名，显式列名仅覆盖引用方列名。`dropForeignIdFor` 和 `dropConstrainedForeignIdFor` 复用列名推导及原有删除路径。
- MySQL 8.4.13 与 SQLite 共 90 项定向回归，覆盖整型、UUID、ULID、自定义表和主键、列名覆盖、前缀、约束拒绝、删除置空、回滚移除及已有数据保留。1 个子进程夹具由父测试启动。三语共用例在两种驱动通过实际 Artisan 验证，原生 `main artisan inspire` 正常退出。
- 严格工作区 Clippy、变更文件格式、三语站点构建和 18 项浏览器检查通过。全工作区格式检查仍报既有 Sanctum 测试排版差异，已核对文件与基线相同。

## 浮点精度与原生列定义

对照固定 Laravel [float](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#column-method-float)、[MySQL FLOAT](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L827)、[SQLite FLOAT 与 DOUBLE](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php#L771) 及 [rawColumn](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L1626)。旧版未找到对应 Blueprint 实现，沿用当前原生 DDL 与目录查询。

- `float(name)` 按 Laravel 默认使用 53 位精度。MySQL 新建或变更此类列时会声明 `FLOAT(53)`，由服务器落为 DOUBLE。Rust 用 `floatWith(name, precision)` 表达显式精度，0 对应无参数原生 FLOAT。SQLite 的 `float` 与 `double` 分别产生对应类型声明。
- `rawColumn` 把应用提供的可信 SQL 作为列类型定义，保留列名引用和常规修饰符。空定义或含 NUL 的定义在执行 DDL 前拒绝。
- MySQL 8.4.13 与 SQLite 共 89 项定向回归，覆盖 24／25 位边界、实际舍入、默认精度、SQLite 浮点行为、原生列新建和变更、数据保留、非法定义拒绝。1 个子进程夹具由父测试启动。三语共用例在两种驱动通过实际 Artisan 验证，原生 `main artisan inspire` 正常退出。
- 严格工作区 Clippy、变更文件格式、三语站点构建和 18 项浏览器检查通过。全工作区格式仍报既有 Sanctum 测试排版，已核对文件与基线相同。

## 索引方法与在线删列

对照固定 Laravel [MySQL compileKey 与 compileDropColumn](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L463) 和 [DDL 锁](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#ddl-locking)。旧版未找到索引方法和在线删列定义，本批复用原生 ALTER、标识符引用、锁模式校验及驱动目录。

- 索引使用 `algorithm("btree")` 或 `algorithm("hash")` 指定 `USING` 方法。`inplace()` 单独指定 DDL 算法。实际支持由 MySQL 存储引擎决定。
- `dropColumn` 返回 `DropColumnDefinition`，可链式传入 `instant()` 和 `lock()`。同次传入的多列按 Laravel 合并为一条 MySQL ALTER。SQLite 保留逐列 SQL 及现有方言错误。
- 与测试连接相同实例实际查询 `SELECT VERSION()` 得到 MySQL 8.4.13。Rust 1.94.0 执行 `cargo +1.94.0 test -p openrust-framework --test database_schema --locked --offline -j 5 mysql -- --skip sqlite_`，13 项通过。验证 InnoDB 主键和普通索引 BTREE、MEMORY HASH、在线删列、同语句失败时保留被删列、合法行保留与无效选项预检。首轮宽筛选额外命中一条既有 SQLite 用例，最终日常验收仅计 MySQL。
- 三语共用示例经实际 MySQL Artisan 运行，原生 `main artisan inspire` 通过。严格工作区 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。全工作区格式仍报既有 Sanctum 排版差异，已核对文件与基线相同。

## 同时重命名和修改列

对照固定 Laravel [MySQL compileChange](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L372) 与 [Modifying Columns](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#modifying-columns)。旧版未找到链式 `renameTo()`，本批复用列定义、标识符和原生 ALTER。

- MySQL 将 `renameTo().change()` 编译为一条 `CHANGE COLUMN`。原名和新名分别引用，新增的列级索引使用新名。SQLite 固定上游没有该修饰符，框架显式返回方言错误。独立 `renameColumn` 仍支持 SQLite。
- 使用测试实例实际查询 `SELECT VERSION()` 得到 MySQL 8.4.13。Rust 1.94.0 执行 `cargo +1.94.0 test -p openrust-framework --test database_schema --locked --offline -j 5 mysql -- --skip sqlite_`，14 项通过。覆盖旧数据、原索引及新索引、默认值、目标重名拒绝、单独修饰符拒绝和反向变更。
- 三语共用示例在实际 MySQL Artisan 中完成正向及反向变更。原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。全工作区格式仍有既有 Sanctum 测试排版差异。按日常验收范围仅运行 MySQL，SQLite 路径通过工作区编译检查。

## Schema 与视图目录

对照固定 Laravel [Builder 目录方法](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L146)、[MySQL 目录查询](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L60) 和 [SQLite getViews](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/SQLiteBuilder.php#L59)。旧版框架未找到对应公开方法，复用现有写会话、目录读取和前缀解析。

- `getSchemas()` 返回名称、默认标志和 SQLite 路径。`getViews(None)` 查询 MySQL 可见用户 Schema 或同一 SQLite 写会话中的所有 Schema。一个 Schema 用 `getViews(Some(name))`，多个用 `getViewsIn(&[...])`。返回目录名称、限定名称以及数据库提供的定义。
- `hasView()` 使用连接的表前缀，允许限定 Schema 并按 Laravel 的大小写规则比较视图名。默认使用写连接的当前 Schema。
- 实际 `SELECT VERSION()` 为 MySQL 8.4.13。Rust 1.94.0 执行 `cargo +1.94.0 test -p openrust-framework --test database_schema --locked --offline -j 5 mysql -- --skip sqlite_`，15 项通过。覆盖真实读写数据库分离、默认标志、同名视图、指定和全部 Schema、系统 Schema 排除、缺失视图及删除后目录刷新。
- 三语共用示例经实际 MySQL Artisan 验证，原生 `main artisan inspire` 通过。严格 Clippy、变更文件格式、三语站点构建和 18 项浏览器检查通过。SQLite 本批通过工作区编译，运行验收留在发布前。全工作区格式仍有既有 Sanctum 排版差异。

## 表目录范围与限定名

对照固定 Laravel [getTables 与 getTableListing](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L201)、[MySQL compileTables](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L88) 及 [SQLite compileTables](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php#L97)。旧版框架未找到对应表清单方法，本批复用写连接、绑定参数和现有结构化目录。

- `getTables(None)` 从仅当前库调整为 Laravel 的全部可见用户 Schema，仍排除视图和 MySQL 系统 Schema。单个 Schema 使用 `Some(name)`，多个使用 `getTablesIn`。`hasTable` 仍按传入限定名或当前 Schema 查找。
- `TableInfo` 增加限定名与可选存储大小。`getTableListing` 默认返回限定名，Rust 用 `getTableListingWith(schema, false)` 表达可选的不限定名称。清库验收显式读取被清理的 Schema。
- 实际测试连接 `SELECT VERSION()` 为 MySQL 8.4.13。Rust 1.94.0 的 `cargo test -p openrust-framework --test database_schema --locked --offline -j 5 mysql -- --skip sqlite_` 通过 15 项，覆盖同名跨库表、读写分离、系统库与视图排除、限定名、可读大小及元数据。另运行根应用 `cargo test --test migrations --locked --offline -j 5 mysql_fresh_wipes_the_selected_schema -- --exact`，实际 `migrate:fresh` 与 `db:wipe` 通过，另一库的标记表仍可查询。
- 三语共用例经实际 MySQL Artisan 验证，原生 `main artisan inspire` 通过。严格 Clippy、变更文件格式、三语站点构建和 18 项浏览器检查通过。SQLite 本批通过工作区编译，运行验收留在发布前。全工作区格式仍有既有 Sanctum 排版差异。

## 默认列长度与时间精度

对照固定 Laravel [Builder 默认值](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L35)、[Blueprint 字符串](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L786) 与 [时间列组](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L1222)。旧版未找到对应公开入口，复用现有多态键默认值的应用隔离机制与方言编译。

- 默认长度 255、时间精度 0，并由当前应用设置。Rust 用 `None` 表达省略参数，保留既有显式长度与精度调用。固定长度辅助列继续使用自身长度。新增 `datetimes`、`softDeletesDatetime` 和显式精度的时间戳／软删除辅助入口。
- 实际 `SELECT VERSION()` 为 MySQL 8.4.13。Rust 1.94.0 执行 `cargo test -p openrust-framework --test database_schema --locked --offline -j 5 mysql -- --skip sqlite_`，16 项通过。覆盖并行应用、默认及显式值、多态类型列、可空时间组、无效设置拒绝和自定义软删除列名。
- 三语共用例在实际 MySQL Artisan 中验证默认长度与显式精度，原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点构建及 18 项浏览器检查通过。SQLite 本批通过工作区编译。全工作区格式仍报既有 Sanctum 测试排版差异。

## 条件式 Schema 变更

对照固定 Laravel [条件式 Blueprint 回调](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L279) 及 [索引和外键匹配](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L430)。旧版框架未找到对应公开入口，本批复用现有写连接目录、列及索引查询和 Blueprint 执行路径。

- 新增 `whenTableHasColumn`、`whenTableDoesntHaveColumn`、`whenTableHasIndex`、`whenTableDoesntHaveIndex`。只有匹配时调用回调，目录或 DDL 错误沿用原路径传播。
- Rust 用 `KeySelector` 表达按名称或按顺序匹配列。`hasIndexWith` 支持可选的主键、唯一性或驱动索引类型。`hasForeignKey` 支持按名称或按本地列匹配，保留原 `hasIndex(table, name)`。
- 使用独立 MySQL 8.4.13 运行 `sh tests/table_catalog.sh`，17 项 Schema 回归及 1 项实际 `migrate:fresh`／`db:wipe` 验收通过。验证条件未命中不执行回调、重复执行、索引列顺序和类型、外键匹配、DDL 错误传播以及另一 Schema 的标记行保留。
- 使用 `python3 docker/accept.py --mysql -- ./target/debug/examples/schema_conditional_acceptance` 运行三语共用 Artisan 示例，重复运行条件操作及反向移除索引通过。原生 `main artisan inspire`、Rust 1.94 严格 Clippy、变更文件格式、三语构建和 18 项浏览器检查通过。全工作区格式检查仍有既有 Sanctum 排版差异。
- 当前主分支的 `5aa5636a` 已回退两个 Docker 验收入口。本批仅在隔离副本提取 `3d2641c0f` 中的原脚本运行，并验证两轮均回收各自的 MySQL 容器与卷。

### 条件式 Schema 当前主线复验

在 `e5661b8eb` 的隔离快照复验原实现提交 `7d67c94d3`。MySQL 8.4.13 下，`sh tests/table_catalog.sh` 的写连接目录和实际 `migrate:fresh` 限定库清理通过，另一 Schema 的标记行保留。按指定入口在隔离快照提取历史 `docker/accept.py`，以 `--mysql` 验证条件式回调回归、三语共用 Artisan 示例和实际二进制 `main artisan inspire`。严格 Clippy 通过。全仓格式检查仍只有既有 Sanctum 排版差异。三轮夹具均已回收。

#### 2026-11-17 02:21 恢复复验

- 在 `042dc8c9a` 的隔离快照复验条件式 Schema 实现 `7d67c94d3`。MySQL 8.4.13 下，`sh tests/table_catalog.sh` 通过写连接表目录和实际 `migrate:fresh`／`db:wipe` 限定库清理，另一 Schema 的标记行仍存在。
- `docker/accept.py --mysql` 通过条件式回调回归、三语共用的实际 Artisan 示例及 `main artisan inspire`。扩展回归初跑有三项跨 Schema 用例因测试账号权限失败，改用夹具自带的 `--mysql-schema-admin` 后 22 项全部通过。该权限只用于临时 MySQL。历史 `docker/accept.py` 从 `3d2641c0f` 提取到隔离快照。
- Rust 1.94.0 工作区严格 Clippy 通过。全仓格式检查只报既有 Sanctum 测试排版差异。四轮临时夹具已回收，清理本批 3.6 GiB Cargo 产物后，项目卷可用 60.73 GB（56.56 GiB）。日志目录为 `/private/tmp/openrust-02-05-recheck.8ZWYyLV9`。

## 创建和删除数据库

对照固定 Laravel [Builder 的数据库操作](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L123)、[MySQL 建库默认值](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L42)、[SQLite 文件操作](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/SQLiteBuilder.php#L16)。旧版框架未找到相同接口，本批在 Schema 的写连接及原生 DDL 路径实现。

- `createDatabase` 按选定驱动建库。MySQL 继承连接字符集和排序规则，名称不加表前缀。`dropDatabaseIfExists` 使用明确名称，重复删除成功。
- SQLite 建库使用独占创建，避免截断同名文件。这与固定 Laravel 文件写入的覆盖语义有意区分。SQLite 文件变更只从连接 Builder 执行，事务和 SQL 预演不得修改文件。
- `sh tests/table_catalog.sh` 使用独立 MySQL 8.4.13，18 项 Schema 回归及 1 项实际限定 Schema 清库通过。测试覆盖连接默认值、带连字符名称、重复建库保留原表、重复删库和当前数据库保留。
- 通过 `docker/accept.py --mysql --mysql-schema-admin` 运行三语共用 Artisan 示例。原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。SQLite 回归已编译，运行留待发布前。全工作区格式仍有既有 Sanctum 排版差异。验收入口取自隔离副本的 `3d2641c0f`，未恢复主分支回退，两轮夹具均已回收容器和卷。

## 删除全部表或视图

对照固定 Laravel [MySqlBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/MySqlBuilder.php#L11) 和 [SQLiteBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/SQLiteBuilder.php#L89)。旧版框架未找到同名公开接口。本批复用新主线 `db:wipe` 的写连接、当前 Schema 限定、SQLite 事务和 MySQL 外键检查恢复。

- `dropAllTables` 只清理所选写库的永久表，`dropAllViews` 只清理视图。表前缀不限制清理对象。MySQL 表清理持有专属写租约并恢复该会话原有外键检查状态。
- `db:wipe` 和 `migrate:fresh` 继续使用同一条内部清库路径。SQLite 保持现有原生 DDL 与事务路径，限定主 Schema。
- `sh tests/table_catalog.sh` 在独立 MySQL 8.4.13 上通过 19 项 Schema 回归及 1 项限定 Schema 清库。验证读写库分离、表和视图各自清理、无前缀对象、重复执行、同库外键检查恢复和另一库标记行保留。
- 使用 `docker/accept.py --mysql` 通过三语共用的具名连接实际 Artisan 示例。原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。SQLite 回归已编译，实际运行留待发布前。全工作区格式仍有既有 Sanctum 差异。验收入口取自隔离副本的 `3d2641c0f`，两轮夹具容器与卷已回收。

## 单列目录

对照固定 Laravel [Builder::getColumn](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L394)。旧版框架未找到同名目录接口。本批复用现有写连接列目录和 `Column` 元数据。

- `Schema::getColumn(table, name)` 和具名连接 Builder 都比较列名时忽略大小写。Laravel 缺失时返回空数组，Rust 使用 `Option<Column>` 表达缺失。数据库错误按原路径传播。`getColumnType` 复用查找，保留原有缺失列错误。
- 使用 `sh tests/table_catalog.sh` 在独立 MySQL 8.4.13 上通过 19 项 Schema 回归及 1 项限定库清理回归。测试覆盖在既有带前缀连接中读取修改后列的名称、类型、注释及可空性，以及缺失列行为。验收入口取自隔离副本的 `3d2641c0f`。
- 使用 `docker/accept.py --mysql` 运行三语共用例的实际 Artisan 命令，验证普通和大写列名、缺失列及删表后的结果。原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。全工作区格式仍有既有 Sanctum 排版差异，SQLite 日常仅编译。
- 类型目录另行跟踪。固定版 [通用 Schema Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/Grammar.php) 对不支持的驱动报错，[PostgreSQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/PostgresGrammar.php) 查询原生类型。当前连接工厂只有 MySQL 和 SQLite 的运行时实现，暂未提供 `getTypes`，也未用空目录替代。

## 原生索引表达式

对照固定 Laravel [Blueprint::rawIndex](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L740)、[MySQL 语法测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseMySqlSchemaGrammarTest.php#L420)及 [SQLite 语法测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSQLiteSchemaGrammarTest.php#L267)。旧版未找到相同方法，本批沿用索引定义、语法编译、目录查询及删除命令。

- `rawIndex(expression, name)` 接收迁移代码提供的原生键列表，返回 `IndexDefinition` 并保留普通索引支持的修饰符。MySQL 函数键由调用端显式传入一对括号，框架只为外层键列表补括号。空表达式和 NUL 在执行 DDL 前拒绝。
- `sh tests/table_catalog.sh` 在独立 MySQL 8.4.13 通过 20 项 Schema 回归及 1 项限定库清理回归。验证建表和修改表、列与表达式的混合键顺序、索引启用及删除、`EXPLAIN` 查询计划、实际命中行和无效输入预检。目录中表达式位置保留既有 `None` 表示。验收入口取自隔离副本的 `3d2641c0f`。
- `docker/accept.py --mysql` 跑通三语共用的实际 Artisan 示例并验证索引创建和删除。原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。SQLite 的表达式索引及重建回归已编译，执行留待发布前。

## 当前 Schema 与 DDL 快捷入口

对照固定 Laravel [MySqlBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/MySqlBuilder.php#L54)、[SQLiteBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/SQLiteBuilder.php#L169)、[Builder::dropColumns](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L580) 和 [Blueprint::innoDb](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L359)。旧版未找到同名入口。

- `getCurrentSchemaName` 和 `getCurrentSchemaListing` 同步读取连接配置。MySQL 取写库数据库名，SQLite 取 `main`。Builder 借用名称，Facade 返回拥有所有权的字符串。用于目录解析的默认 Schema 与公开查询共用同一逻辑。
- `dropColumns` 复用 Blueprint 的多列删除。MySQL 将多个删除项合并到同一次 ALTER。空列表在执行 DDL 前报错。`innoDb` 复用 `engine("InnoDB")`，沿用 MySQL 的表选项覆盖规则。
- `sh tests/table_catalog.sh` 在独立 MySQL 8.4.13 通过 21 项 Schema 回归及 1 项限定库清理回归。覆盖读库与写库不同时的默认 Schema、具名连接的限定表名和表前缀、组合删列失败后的列保留以及 InnoDB 覆盖连接默认引擎。验收脚本仅从 `3d2641c0f` 提取到隔离副本。
- `docker/accept.py --mysql` 运行三语共用的实际 Artisan 示例，核对 InnoDB、Schema 与视图目录及删列。原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点和 30 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版，SQLite 已编译，实际运行留待发布前。

## 按列列表删除索引

对照固定 Laravel [Blueprint::dropIndexCommand 与 createIndexName](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L1805) 和 [MySQL 空间索引删除测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseMySqlSchemaGrammarTest.php#L269)。旧版未找到对应的按列列表入口。本批复用 `dropMorphs` 使用的默认索引名生成与原有删除路径。

- Laravel 的删除方法接受名称或数组。Rust 用 `dropIndexColumns`、`dropUniqueColumns`、`dropFullTextColumns`、`dropSpatialIndexColumns` 表达列列表，沿用 `dropForeignColumns` 的命名。默认索引名保留限定 Schema、表前缀配置、列顺序和索引类型后缀。空列表在执行 DDL 前报错。
- `sh tests/table_catalog.sh` 在独立 MySQL 8.4.13 通过 22 项 Schema 回归及 1 项限定库清理回归。验证普通、唯一、全文、空间索引删除，列顺序颠倒导致删除失败，具有相同列的自定义名称索引保留。验收脚本仅从 `3d2641c0f` 提取到隔离副本。
- `docker/accept.py --mysql` 运行三语共用的实际 Artisan 示例。原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。SQLite 已编译，实际运行留待发布前。

## 解析限定表名

对照固定 Laravel [Builder::parseSchemaAndTable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L772) 和 [SQLite 限定 Schema 索引语法测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSQLiteSchemaGrammarTest.php#L233)。旧版未找到对应的独立解析入口。本批把现有目录与 DDL 的拆分逻辑共用于 Builder 和 Facade。

- 固定 Laravel 用字符串、布尔值或 null 表达 Schema 回退。Rust 用 `SchemaFallback::{None, Current, Named}` 表达同样的三种选择。显式 Schema 优先。Builder 借用输入与配置，Facade 返回拥有所有权的值。表前缀仍由 DDL 和目录查询应用。
- 三段式引用与 Laravel 一样报错。Rust 保留现有 Schema 路径对空表名或空限定段的预检，解析入口会提前报告这些输入。
- `sh tests/table_catalog.sh` 在独立 MySQL 8.4.13 通过 22 项 Schema 回归及 1 项限定库清理回归。实测当前与显式回退、读写库分离、带前缀的跨 Schema 限定表名和无效引用预检。验收脚本仅从 `3d2641c0f` 提取到隔离副本。
- `docker/accept.py --mysql` 运行三语共用 Artisan 示例，实际创建限定表并按解析结果读取其 Schema 下的表目录。原生 `main artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。SQLite 附加库解析回归已编译，实跑留待发布前。

## JSON 列声明

对照固定 Laravel [MySQL JSON 语法](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L961)和 [SQLite JSON 语法](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php#L888)。旧版未找到这两个设置，沿用当前连接配置、写端 Grammar 与 Blueprint 类型声明。

- MySQL 的 `json()` 和 `jsonb()` 都声明为 `JSON`。SQLite 两种类型默认声明为 `TEXT`，分别通过连接选项 `use_native_json` 和 `use_native_jsonb` 启用 `JSON` 和 `JSONB` 类型名。SQLite 的 JSONB 二进制写入由数据库函数完成。
- 隔离基线 `6d8f18109`。使用 `docker/accept.py --mysql --mysql-schema-admin` 验收 MySQL 8.4.13 的 23 项 Schema 用例，新增用例覆盖两种声明都为 JSON、接受有效 JSON 并拒绝无效输入。用 `sh tests/table_catalog.sh` 验收写库目录与限定 Schema 清库。实际 SQLite 测试通过四种配置组合、建表和改表、二进制写入与 JSON 路径读取。
- 两种驱动均通过实际 Artisan 执行三语共用示例，`main artisan inspire`、严格 Clippy、变更文件格式、三语站点及 18 项浏览器检查通过。全仓格式检查仅报既有 Sanctum 排版差异。验收所用历史 `docker/accept.py` 来自 `3d2641c0f`。两轮临时 MySQL 已回收。
- 移除本批 3.6 GiB 隔离 Cargo 产物和本批文档缓存后，项目卷可用 55.57 GB（51.75 GiB）。证据目录为 `/private/tmp/openrust-02-05-json.REFu2dP8`。

## 编辑 Blueprint

对照固定 Laravel [命名自增主键](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L769)和[撤销待执行的列定义](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L1901)。旧版未找到对应入口，复用当前 `bigIncrements`、列级修饰符和 Blueprint 命令列表。

- Rust 通过 `idWith(name)` 表达 Laravel `id(name)` 的显式参数，既有 `id()` 使用默认列名。`removeColumn(name)` 撤销该名称的待执行列定义及其列级修饰符。显式索引和约束保持独立。
- 隔离基线 `399587b88`，MySQL 8.4.13 的 24 项 Schema 回归通过。验证自定义无符号自增主键和起始值、撤销重复的待执行列定义、撤销待执行修改后保留原列类型与数据、独立索引保留。
- 实际 Artisan 执行三语共用示例及 `main artisan inspire`，三语站点构建和 18 项浏览器检查通过。临时 MySQL 使用从 `3d2641c0f` 提取的 `docker/accept.py --mysql`。
- 用户确认恢复后，复用原隔离 Cargo 产物运行 `sh tests/table_catalog.sh`，写库表目录、限定 Schema 清库及保留旁库标记行通过。变更文件格式检查、工作区严格 Clippy 通过，叠加当时主线 `dd304ad1e` 的图像模块已提交改动后再跑严格 Clippy 通过。全仓格式检查只有既有 Sanctum 排版差异。
- 三轮临时 MySQL 已回收。清理本批 3.6 GiB 隔离 Cargo 产物和本批文档缓存后，项目卷可用 73.89 GB（68.81 GiB）。证据目录为 `/private/tmp/openrust-02-05-blueprint.0B3TbYWv`。

## SQLite 主键索引目录

对照固定 Laravel [SQLite 目录语法](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php#L188)和[索引结果处理](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Processors/SQLiteProcessor.php#L55)。旧版未找到对应索引目录入口，复用现有写连接、SQLite PRAGMA 和有序列表示。

- 一次查询返回 SQLite 原生索引及没有独立索引的行主键。原生主键按实际索引名和键列顺序报告。行主键显示为 `primary`。按名称及主键标志归组，因此普通同名索引和主键各自保留。遇到真实主键索引时仍接受 `hasIndex(table, "primary")` 作为兼容查询。表达式索引继续保留 `None` 键位。
- 隔离基线 `e018ad1f7`。SQLite 21 项和 MySQL 8.4.13 的 24 项 Schema 用例通过。新增回归覆盖复合主键的键列顺序、普通同名索引与原生及行主键并存、带前缀限定名及缺失表。MySQL 使用隔离副本内 `docker/accept.py --mysql --mysql-schema-admin`，`sh tests/table_catalog.sh` 的写库目录、限定 Schema 清库和旁库标记行保留通过。
- SQLite 实际 Artisan 通过三语共用示例，`main artisan inspire`、变更文件格式、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式检查仅有既有 Sanctum 排版差异。
- 两轮临时 MySQL 已回收。清理本批 3.6 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 71.37 GB（66.47 GiB）。证据目录为 `/private/tmp/openrust-02-05-sqlite-index.V614h8M8`。

## SQLite 列类型与主键标志

对照固定 Laravel [SQLite 列目录](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php#L171)、[SQLite 列结果处理](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Processors/SQLiteProcessor.php#L8)和 [Builder::getColumnType](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L363)。旧版列清单已用 `pragma_table_xinfo`。沿用当前参数化写连接目录，并结合已有索引来源信息。

- `type_name` 去掉声明中的参数，`full_type` 保留完整声明，无类型声明时两者为空串。SQLite 普通行主键即使未声明 `AUTOINCREMENT` 也报告可自动分配值。与 Laravel 的目录推断有两处差异：用原生索引目录排除 `WITHOUT ROWID` 和 `PRIMARY KEY DESC`，并让 `nullable` 反映是否可实际储存空值。SQLite 重建仍独立解析 `AUTOINCREMENT` 以判断是否恢复序列。
- 隔离基线 `59e2c3967`。SQLite 22 项和 MySQL 8.4.13 的 24 项 Schema 测试通过。新增 SQLite 回归覆盖长度、精度、未声明类型和五种主键形态，通过实际插入验证自动分配、空值存储及空值拒绝。MySQL 使用隔离副本内 `docker/accept.py --mysql --mysql-schema-admin`，`sh tests/table_catalog.sh` 的写库表目录和限定 Schema 清库通过。
- SQLite 实际 Artisan 通过三语共用示例。`main artisan inspire`、变更文件格式、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式检查只报告既有 Sanctum 排版差异。
- 两轮临时 MySQL 已回收。清理本批 3.6 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 71.34 GB（66.44 GiB）。证据目录为 `/private/tmp/openrust-02-05-sqlite-columns.DaP8WwVi`。

## SQLite 外键目录

对照固定 Laravel [SQLite 外键查询](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SQLiteGrammar.php#L211)及[外键结果处理](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Processors/SQLiteProcessor.php#L83)。旧版未找到对应外键元数据接口，复用现有写连接、SQLite PRAGMA 及 MySQL 信息目录。

- SQLite 外键返回所在 Schema，包括附加库名。按目录的数字编号分组及排序，保留同一约束中的列顺序。SQLite 目录的约束名为 `None`，省略的目标列也按原始位置表示为 `None`。MySQL 继续读取服务器提供的约束名和目标 Schema。
- 隔离基线 `815689dc7`。SQLite 25 项和 MySQL 8.4.13 的 24 项 Schema 测试通过。新增回归用同一写会话实际附加 SQLite 库，验证 13 个外键在超过一位数时的顺序、带前缀的表名、复合引用、正确写入及无效值拒绝。MySQL 的表目录、限定 Schema 清库及旁库标记行保留通过。
- 三语共用示例在 SQLite 和 MySQL 的实际 Artisan 中运行，`main artisan inspire`、变更文件格式、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式检查仅报告既有 Sanctum 排版差异。临时 MySQL 验收入口从 `3d2641c0f` 提取到隔离副本。
- 两轮临时 MySQL 已回收。清理本批 3.6 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 71.31 GB（66.42 GiB）。证据目录为 `/private/tmp/openrust-02-05-sqlite-foreign.pWfq2qkS`。

## Blueprint 回调上下文

对照固定 Laravel [Blueprint::creating](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L327)、[表名及待执行列入口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L1945)和 [Builder::create](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L538)。旧版未找到对应读取入口，复用当前 Blueprint 命令列表和条件式改表回调。

- `getTable()` 返回原始逻辑表引用，包括传入的 Schema 限定名。`creating()` 反映构建器进入回调时的创建或改表状态。`getColumns()`、`getAddedColumns()` 和 `getChangedColumns()` 按声明顺序借用现有待执行列，`ColumnDefinition::getName()` 提供类型明确的列名读取入口。
- 隔离基线 `ec4ed2aa8`。SQLite 26 项和 MySQL 8.4.13 的 25 项 Schema 回归通过。新增回归覆盖带前缀的限定表名、普通建表、条件式改表、已撤销的待执行列和独立索引。跨驱动测试以修改列的可空性作为共同断言，避免依赖 SQLite 是否保留 VARCHAR 长度或 MySQL 是否允许对 TEXT 建无前缀索引。
- 两种驱动的实际 Artisan 均跑通三语共用示例。`main artisan inspire`、`sh tests/table_catalog.sh` 中的表目录与限定清库、变更文件格式、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式检查仅有既有 Sanctum 排版差异。
- 三轮临时 MySQL 已回收。清理本批 3.6 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 69.75 GB（64.96 GiB）。证据目录为 `/private/tmp/openrust-02-05-blueprint-context.xCmiOb12`。

## 连接表前缀读取

对照固定 Laravel [Connection::getTablePrefix](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L1852)和 [Blueprint::getPrefix](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Blueprint.php#L1957)。旧版已有按连接配置表前缀的机制，未找到同名公开读取入口。本批复用当前查询和 Schema 共用的前缀。

- `DB::getTablePrefix()` 返回当前应用默认连接的前缀。具名连接可通过自身的 `getTablePrefix()` 读取。Blueprint 的 `getPrefix()` 对应固定 Laravel 的兼容接口。Rust 的连接和 Blueprint 借用共享字符串，Facade 返回拥有所有权的字符串。
- 隔离基线 `9435c81bc`。SQLite 26 项和 MySQL 8.4.13 的 25 项 Schema 回归通过，覆盖建表及条件式修改回调。两种驱动的实际 Artisan 分别验证默认连接和具名连接创建同名逻辑表、按各自前缀执行原生 SQL，并通过查询构建器读回。
- `main artisan inspire`、`sh tests/table_catalog.sh` 中的表目录和限定 Schema 清库、变更文件格式、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式检查仅有既有 Sanctum 排版差异。
- 两轮临时 MySQL 已回收。清理本批 3.6 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 70.10 GB（65.28 GiB）。证据目录为 `/private/tmp/openrust-02-05-prefix.TDhAAec8`。

## Blueprint 解析器

对照固定 Laravel [创建 Blueprint](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L732)和[注册解析器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Builder.php#L810)。旧版未找到相同接口，复用当前 Blueprint 构造、逻辑表名、连接前缀和各执行路径。

- `blueprintResolver` 在当前 Schema 构建器上注册可替换的初始化回调。Rust 先创建类型明确的 Blueprint，解析器可在操作回调之前加入定义。回调持有自己的捕获值，现有事务及会话的可变借用可在最后一次使用构建器时结束。具名连接、事务、会话和预演各自使用其构建器。
- 隔离基线 `5f3578bc8`。SQLite 27 项和 MySQL 8.4.13 的 26 项 Schema 回归通过。SQLite 验证注册作用域、替换解析器、条件改表和事务回滚。MySQL 的连接级预演验收生成 SQL、数据库无副作用和实际建表。
- 三语共用示例在两种驱动的 Artisan 实际跑通 `migrate --pretend`、`migrate` 和 `migrate:rollback`，验证预演无表、迁移后列正确和回滚移除表。`main artisan inspire`、表目录、限定 Schema 清库、变更文件格式、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式检查仅有既有 Sanctum 排版差异。
- 两轮临时 MySQL 已回收。清理本批 3.6 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 65.11 GB（60.63 GiB）。证据目录为 `/private/tmp/openrust-02-05-resolver.GfNQd7Qd`。

## Blueprint 主线复验

在 `master` 的隔离快照 `0230eeb29` 复验。编辑 Blueprint 的补丁已进入 `47690d179`，回调上下文、连接前缀和解析器随后已进入 `69eaac8f9`、`5f3578bc8` 和 `da9443208`。

- `python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test -p openrust-framework --test database_schema --locked --offline mysql_` 在 MySQL 8.4.13 通过 26 项 Schema 回归，覆盖命名自增主键、撤销待执行列和修改、条件式 Schema 回调、解析器预演及实际建表。
- `sh tests/table_catalog.sh` 在另一个 MySQL 8.4.13 实例通过写连接表与视图目录、`db:wipe`、`migrate:fresh` 和旁库标记行保留检查。`target/debug/main artisan inspire` 正常退出。
- 02-05 涉及的 8 个 Rust 文件通过 Rust 1.94.0 的 `rustfmt --check`，`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。`cargo +1.94.0 fmt --all --check` 只报告 `sanctum_authentication.rs:1412` 的既有排版，已核对该文件与 Blueprint 补丁前逐字节一致。
- 原隔离编译产物已清理。本轮在同一新隔离目录构建，Blueprint、表目录和 Artisan 复用本轮产物。两轮 MySQL 均已回收，清理本轮 3.5 GiB Cargo 产物后，项目卷可用 61.20 GB（57.00 GiB），周额度剩余 71%，第二窗口无读数。证据保存在 `/private/tmp/openrust-02-05-blueprint-recheck.pm7lV6rw`。

#### 2026-11-17 08:06 恢复复验

- 在当前 `master` 的隔离快照 `87e690c10` 复验。保存的 `blueprint-delivery.patch` 与 `47690d179` 中对应五个文件的稳定补丁标识一致。02-05 的八个 Rust 文件和表目录脚本与上次复验快照一致。原隔离产物已清理，本轮在新隔离目录构建一次并复用同一 target 目录。
- 在隔离快照从 `3d2641c0f` 提取历史 `docker/accept.py`，使用 `--mysql --mysql-schema-admin` 提供临时 MySQL，26 项 Schema 回归通过。`sh tests/table_catalog.sh` 在独立 MySQL 8.4.13 上通过写连接表与视图目录检查，以及实际 Artisan `db:wipe`、`migrate:fresh` 和旁库标记行保留检查。`target/debug/main artisan inspire` 正常退出。
- 涉及的八个 Rust 文件通过 Rust 1.94.0 格式检查。`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。全仓格式检查只报告 `sanctum_authentication.rs:1412` 的既有排版，该文件与补丁前一致。
- 已核对两轮临时 MySQL 的容器和卷均已回收，清理本轮 3.7 GiB 隔离 Cargo 产物。项目卷可用 57.49 GB（53.55 GiB），周额度剩余 69%，无第二窗口读数。日志存于 `/private/tmp/openrust-02-05-blueprint-current.vFUL6Ao0`。02-05 的其余类型和方言修饰符继续由账本跟踪。

#### 2026-11-17 09:25 主线复验

- 在当前 `master` 的隔离快照 `85b31347c` 复验。保存的 Blueprint 补丁与 `47690d179` 中五个交付文件的稳定补丁标识一致，02-05 的数据库源码和回归脚本与上次验收快照一致。本轮复制现有隔离 Cargo 缓存后复用同一 target，原目录和在途改动均保留。
- `sh tests/table_catalog.sh` 在临时 MySQL 8.4.13 上通过写连接的表与视图目录，以及实际 Artisan `db:wipe`、`migrate:fresh` 和旁库标记行保留检查。第二个临时 MySQL 8.4.13 上的 26 项 Schema 回归及 `main artisan inspire` 通过。
- 02-05 涉及的八个 Rust 文件通过 Rust 1.94.0 格式检查，`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。全仓格式检查只报告 `sanctum_authentication.rs:1412`，已核对与 Blueprint 补丁前的文件一致。
- 两轮 MySQL 夹具已回收，并清理本轮独立 target。项目卷可用 53.96 GB（50.25 GiB），周额度剩余 68%，第二窗口无读数。日志存于 `/private/tmp/openrust-02-05-blueprint-resume.6MjrCHZ6`。本轮复验范围通过，02-05 的其余类型和方言修饰符继续跟踪。


## 自增列与显式主键

- 对照固定 Laravel Blueprint 的 `integerIncrements` 和 MySQL Grammar 的 `modifyIncrement`。旧版 `v0.x` 未找到复合主键与自增组合的回归，沿用现有 Blueprint 命令、显式主键及自增起始值的编译路径。 [Laravel 自增列语法](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L1270)
- MySQL Blueprint 有表级主键命令时，自增列只输出 `AUTO_INCREMENT`，整个主键按显式列顺序生成。`autoIncrement()` 保留原列的有符号或无符号属性。`id` 及各 `*Increments` 快捷方法继续生成无符号键，新增 `integerIncrements` 别名。InnoDB 仍要求自增列为某个索引的首列。 [MySQL InnoDB 自增规则](https://dev.mysql.com/doc/refman/8.4/en/innodb-auto-increment-handling.html)
- 隔离基线 `01777330`、Rust 1.94.0。27 项 Schema 回归通过，其中 MySQL 8.4.13 有 26 项，名称筛选同时带入 1 项 SQLite 用例。新用例验证复合主键目录、有符号列写入负数、起始值分配、变更后的主键与序列、无符号别名。另用 `sh tests/table_catalog.sh` 验证写连接目录和限定 Schema 清库。
- 三语共有示例经实际 Artisan 执行 `migrate --pretend`、`migrate` 和 `migrate:rollback`，验证预演无表、建表后的列和索引、回滚移除表。`main artisan inspire` 通过。
- 严格工作区 Clippy、改动文件格式、三语站点构建、12 项通用和 6 项专题浏览器检查通过。全仓格式仅报告既有的 `sanctum_authentication.rs:1412`。本批日常外部驱动验收使用 MySQL，其他驱动实机覆盖按发布范围跟踪。证据目录 `/private/tmp/openrust-02-05-mysql-types.muP968Hh`。02-05 仍跟踪其余类型和方言修饰符。
- 三轮临时 MySQL 已回收，另清理本批 4.1 GiB Cargo 产物及文档缓存。项目磁盘可用 115.05 GB（107.15 GiB），周额度剩余 66%，第二窗口无读数。


## MySQL 列长度与更新表达式

- 对照固定 Laravel MySQL Grammar 的 `typeBinary`、`modifyOnUpdate` 和时间列类型处理。旧版 `v0.x` 未找到零长度及显式更新表达式的实现，沿用当前 Blueprint、列类型编译及 `Expression` 对默认值的校验。 [Laravel 二进制类型](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L1030) · [Laravel 更新修饰符](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php#L1257)
- MySQL 的 `CHAR` 接受 0–255。`binaryWith` 长度为零时使用原生 BLOB，正数长度继续区分 BINARY 和 VARBINARY。`onUpdate(Expression)` 接受应用提供的不带绑定的 MySQL 时间列更新表达式，`useCurrentOnUpdate()` 优先并使用列精度。生成列及其他方言的现有规则继续执行。 [MySQL CHAR](https://dev.mysql.com/doc/refman/8.4/en/char.html) · [MySQL 时间列自动更新](https://dev.mysql.com/doc/refman/8.4/en/timestamp-initialization.html)
- 隔离基线 `5813fcb9`、Rust 1.94.0。按驱动筛选后的 29 项 MySQL 8.4.13 Schema 回归通过，覆盖列宽边界、二进制往返、原生时间列更新、修改精度及无效声明拒绝。首轮名称筛选带入一项 SQLite 测试，复验已将其排除并单独执行原有 MySQL 建表入口。表目录和限定 Schema 清库通过。
- 三语共用示例经实际 Artisan 执行迁移预演、迁移和回滚。核对预演无表、实际列类型和回滚后表已移除。`main artisan inspire`、严格工作区 Clippy、变更文件格式、三语站点及 18 项浏览器检查通过。全仓格式只报告既有 `sanctum_authentication.rs:1412`。
- 02-05 仍跟踪默认值修饰符和其他方言。证据目录 `/private/tmp/openrust-02-05-width-time.v6lgHeoV`。
- 临时 MySQL 与本轮隔离产物已回收，清理 Cargo 产物 4.1 GiB。磁盘可用 115.02 GB（107.12 GiB），周额度剩余 66%，第二窗口无读数。
