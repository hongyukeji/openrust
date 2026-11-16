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
