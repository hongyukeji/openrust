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

## 值范围与指定顺序

- 对照固定 Laravel 的 `whereValueBetween`、否定及 OR 形式、`inOrderOf` 和 `DatabaseQueryBuilderTest`。复核 `v0.x` 的 `query/between_columns.rs` 和绑定回归，沿用固定两端边界、可信表达式和原有编译路径。固定 Goravel 的事务装配仍复用同一查询执行连接。 [Laravel 查询构造器](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#additional-where-clauses) · [固定 Laravel 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php)
- Rust 用现有 `Column` 表达列名和可信表达式，用 `ValueOperand` 表达绑定值或可信表达式。范围包含端点。指定顺序使用上游的 `CASE` 表达式，空列表保留原排序，未匹配值归入末位。重复使用表达式时按 SQL 出现顺序拷贝其绑定，复用现有 UNION 排序及标识符引用。
- 隔离基线 `11b7692dd`。使用 `python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test -p openrust-framework --test database_query --locked --offline mysql -- --test-threads=1` 验证真实 MySQL 8.4.13 五项。覆盖前缀、包含端点的范围、否定和 OR、表达式绑定、空列表、NULL 与列表外值、次级排序、UNION 绑定及无效标识符。
- 三语示例提取后在独立 MySQL 执行。独立 `./artisan inspire`、全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。临时 MySQL、文档和隔离 Cargo 产物已回收。日志保留于 `/private/tmp/openrust-02-04-order-ranges.OHYXbP3W`。
- 02-02 的剩余 PostgreSQL 独立直连及池化路由仍随该驱动验收。02-04 继续核对公开分组限额接口及向量等方言能力。

## 公开分组限额

- 对照固定 Laravel 的 `Builder::groupLimit` 与 `Grammar::compileGroupLimit`。按分区生成 `row_number()`，先绑定窗口排序的参数，再绑定后续条件与排名边界。公开方法复用已有 Eloquent 分组限额编译路径。检查旧版 `v0.x` 查询代码及测试和固定 Goravel 查询层，没有可直接复用的同名实现。 [Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L3258) · [Laravel 查询文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#limit-and-offset)
- MySQL 8.4.13 的 6 项查询回归和 1 项 Eloquent 关系回归通过。验证 SQL 预览与执行的绑定顺序、每组排序及偏移、零限额、计数、游标隐藏排名列和关联预加载。为避免直接组合时丢失 UNION 分支，同层组合返回错误，已实测用 `fromSub` 包住 UNION 后再排名。分组查询拒绝更新和删除，实测拒绝后原表依然保留全部记录。
- 三语示例在同一真实 MySQL 夹具上分别执行通过。Rust 1.94.0 全仓格式与严格 Clippy、真实 Artisan、三语站点和 18 项浏览器检查通过。其他数据库方言及向量能力随对应驱动验收。临时 MySQL、本批 3.1 GiB 隔离 Cargo 产物和文档缓存已回收。项目磁盘可用 64.26 GB（59.85 GiB），周额度剩余 48%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-group-limit.I7Wj6OIb`。

## 向量相似度查询

- 对照固定 Laravel v13.40.0 的向量相似度查询。旧版未找到可复用的向量谓词或驱动语法。Query Builder 提供 `whereVectorSimilarTo`、`selectVectorDistance`、`whereVectorDistanceLessThan`、`orWhereVectorDistanceLessThan`、`orderByVectorDistance`。组合调用按 `distance <= 1 - minSimilarity` 过滤，可关闭默认升序。 [固定 Laravel 查询](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#vector-similarity-clauses) · [固定 Laravel 构建器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L1214)
- Rust 接收数值切片和显式相似度／排序参数。继续使用投影、谓词和排序的参数绑定。PostgreSQL 用 `<=>`，SQLx 的文本参数先绑定为文本，再由服务器转换为 vector。MariaDB 用 `vec_distance_cosine` 与 `vec_fromtext`。默认投影中 PostgreSQL 向量解码为文本，MariaDB 保留驱动字节值。查询构建器的 Eloquent 转发、嵌套向量谓词、Schema 扩展与向量列方法，以及文本自动嵌入另行跟踪。 [pgvector 0.8.2](https://github.com/pgvector/pgvector/tree/v0.8.2) · [MariaDB 余弦距离](https://mariadb.com/docs/server/reference/sql-functions/vector-functions/vec_distance_cosine)
- 真实 PostgreSQL 18.4／pgvector 0.8.2 和 MariaDB 11.8.9 共四项定向测试。验证距离投影、阈值等号、OR 谓词、自定义顺序、接近度排序、NULL 排除及默认投影。SQL 编译测试另验证跨子句和 UNION 排序绑定顺序、无效输入及不支持驱动。根应用实际 HTTP 在两种驱动均返回相同文档顺序和距离，独立 `./artisan inspire` 通过。
- MySQL 8.4.13 原有 Query Builder 定向回归、Rust 1.94.0 全仓格式及严格 Clippy、三语站点、12 项通用和 6 项向量章节浏览器检查通过。核心账本维持 62/66。临时 PostgreSQL、MariaDB 及 MySQL 验收容器和本批 Cargo／文档产物已回收。项目卷可用空间增加 3.04 GB（2.83 GiB），剩余 104.84 GB（97.64 GiB）。周额度剩余 45%，第二窗口无读数。验收脚本与日志保留在 `/private/tmp/openrust-02-04-vector.PEGt57i5`。

## Eloquent 向量查询与嵌套条件

- 固定 Laravel 的 Eloquent Builder 将向量调用转发给底层 Query Builder，并在应用作用域时对含 OR 的条件分组。旧版未找到可复用的向量谓词。OpenRust 显式暴露同名的投影、距离过滤、组合相似度及排序方法。距离过滤沿用 Eloquent 谓词组，投影和排序委托现有 Query Builder。嵌套条件中的列名、向量和最大距离保持为结构化谓词，在外层查询的方言中编译。 [固定 Laravel Eloquent Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php#L2162) · [固定 Laravel 向量查询](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#vector-similarity-clauses)
- `database_vectors` 五项，含 PostgreSQL 18.4／pgvector 0.8.2 与 MariaDB 11.8.9 的真实运行。两种驱动均验证括号内的 AND／OR 绑定顺序、距离投影和模型结果、作用域外匹配被排除。SQL 编译另验证不支持的方言。两个隔离 HTTP 实例用实际 Eloquent 路由返回相同的文档 ID 与距离，两个实例均通过独立 `./artisan inspire`。
- MySQL 8.4.13 的模型持久化及全局作用域回归、Rust 1.94.0 格式与严格 Clippy、三语站点构建和 18 项浏览器检查通过。Schema 向量列和索引、文本自动嵌入继续跟踪。核心账本维持 62/66。本批临时数据库及隔离 Cargo 和文档产物已回收，项目卷可用空间增加 3.04 GB（2.84 GiB），剩余 105.06 GB（97.85 GiB）。周额度剩余 45%，第二窗口无读数。验收脚本和日志位于 `/private/tmp/openrust-02-04-eloquent-vector.ycbGZhVB`。

## 文本嵌入与向量查询

- 对照固定 Laravel 的字符串向量参数、AI SDK 的 Provider 与 30 天缓存。旧版未找到可复用的文本嵌入。Query Builder 和 Eloquent 各提供五个带 `Text` 后缀的异步入口，嵌入在组装 SQL 前解析，数值方法继续同步。复用原有向量投影、谓词、排序及 Eloquent 作用域。 [固定 Laravel 向量查询](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#vector-similarity-clauses) · [固定 Laravel 嵌入缓存](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/ai-sdk.md#caching-embeddings)
- 应用通过容器注册 `Embeddings`，内置 OpenAI 兼容 HTTP 适配器复用现有客户端。缓存使用当前应用的默认存储，缓存键区分 Provider、端点、模型、凭据、维度和原文。无效响应和 Provider 失败不写入缓存。文本入口按需启用，完整 Laravel AI SDK 仍独立跟踪。
- 两项回归使用真实本地 TCP 嵌入端点，验证重复调用、跨应用隔离、模型切换、错误维度、503 后重试、未配置 Provider 以及拒绝不支持的驱动。真实 PostgreSQL 18.4／pgvector 0.8.2 和 MariaDB 11.8.9 的独立二进制通过 Eloquent 文本距离投影、OR 与作用域、结果排序和缓存重用，实际 Artisan 通过。两种数据库五项原有向量回归通过。Provider 验收使用隔离的兼容协议服务，商业端点另行验证。
- MySQL 8.4.13 定向回归、Rust 1.94.0 格式与严格 Clippy、三语站点构建及 18 项浏览器检查通过。核心账本维持 62/66。已回收临时数据库和本批隔离 Cargo／文档产物，项目卷可用空间增加 3.24 GB（3.01 GiB），剩余 105.27 GB（98.04 GiB）。周额度剩余 44%，第二窗口无读数。验收脚本和日志位于 `/private/tmp/openrust-02-04-text-vector.2RSOzEDe`。

## PostgreSQL JSON WHERE 谓词

- 对照固定 Laravel v13.40.0 的 PostgreSQL Grammar。复用旧版 `6d1a81924` 的 `json_contains.rs`、`json_contains_key.rs` 中 JSONB 包含、对象成员存在及数组长度判定的思路，沿用本版已支持生成列的 PostgreSQL 选择器。把 `whereJsonContains`、`whereJsonContainsKey`、`whereJsonLength` 接入既有条件树，保留否定、OR、嵌套及写入条件。 [固定 Laravel PostgreSQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/PostgresGrammar.php#L267-L323)
- PostgreSQL 的包含与长度使用原生 JSONB 运算。对象成员使用 PostgreSQL 原生操作符 `?`，键值独立绑定并用既有 `??` 重写约定区分操作符和占位符。数组索引用长度判断，缺失路径和 SQL NULL 文档的键存在检查返回 false；JSON null 成员仍存在。路径选择器保留转义过的 JSON 键和类型化下标。
- 隔离基线 `6cf1472a0` 使用真实 PostgreSQL 18.6。对 `json` 和 `jsonb` 各测试原生预备及模拟预备，覆盖对象与数组包含、长度、嵌套 OR、否定、带点号／引号／反斜杠／问号的键、负数组索引、超大下标、SQL NULL 和 JSON null、事务更新及回滚。真实 PostgreSQL 生成列回归通过。同一个三语示例用实际 HTTP 返回 200、独立 Artisan `db:postgres-json` 返回 0，均只读到预期的用户。
- MySQL 8.4.13 环境下 `database_query` 套件 15 项通过，含 SQLite 本地回归，PostgreSQL 真实连接另由前述夹具验收。独立 `./artisan inspire`、Rust 1.94.0 全仓格式及严格 Clippy、三语站点和 18 项浏览器检查通过。核心账本维持 63/66。临时数据库和本批隔离编译与文档产物已回收，项目磁盘可用空间净增 8.76 GB（8.16 GiB），剩余 101.72 GB（94.73 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-postgres-json.rIaM7Zeq`。

## PostgreSQL JSON 路径更新与限额写入

- 对照固定 Laravel v13.40.0 PostgreSQL Grammar 的 `jsonb_set`、联结及限额写入。旧版 `6d1a81924` 没有可直接复用的 `jsonb_set` 写入代码，本批复用当前 PostgreSQL 选择器的路径拆分与既有写入编译。路径的每个文本段与 JSON 值分别绑定。同一列的多路径更新在一个赋值中嵌套 `jsonb_set`，避免 PostgreSQL 拒绝对同一列重复赋值，按 `Record` 键顺序应用。联结或限额更新及删除使用原生 `ctid` 子查询。 [固定 Laravel JSON 更新](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/PostgresGrammar.php#L492) · [固定 Laravel 限额写入](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/PostgresGrammar.php#L608)
- 隔离基线 `ebf1a0d13`，真实 PostgreSQL 18.6。对 `json` 和 `jsonb` 各使用原生与模拟预备，验证多路径、别名、普通列混合写入、负下标、含点号和引号的键、JSON null、SQL NULL、缺失中间对象和非法路径。验证联结与限额组合更新、事务回滚、已有行的 `updateOrInsert` 和限额删除。两项 PostgreSQL 查询回归及三语共用示例的实际 HTTP 200、独立 Artisan 退出 0 通过。
- MySQL 8.4.13 环境下查询与写入套件共 22 项通过，含 SQLite 本地回归及驱动条件用例。独立 `./artisan inspire`、Rust 1.94.0 全仓格式及严格 Clippy、三语站点和 18 项浏览器检查通过。PostgreSQL `truncate` 与 SQL Server JSON 查询等仍按方言跟踪，账本维持 63/66。临时数据库和本批隔离编译与文档产物已回收，磁盘空间净增 8.55 GB（7.96 GiB），剩余 101.68 GB（94.69 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-postgres-json-update.P2CJgEDY`。

## PostgreSQL 原生清空表与序列重置

- 对照固定 Laravel v13.40.0 的 PostgreSQL Grammar，默认走原生 `TRUNCATE … RESTART IDENTITY CASCADE`。旧版 `6d1a81924` 未发现可复用的 PostgreSQL 原生截断实现，沿用当前表名限定、前缀和写入执行路径。Laravel 的级联开关保存在静态 Grammar 上，OpenRust 通过 PostgreSQL 连接选项 `cascade_on_truncate` 保留可关闭的行为，并按应用和连接隔离。有事务句柄时继续借用其物理会话执行。 [固定 Laravel 编译](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/PostgresGrammar.php#L678) · [PostgreSQL 官方语义](https://www.postgresql.org/docs/18/sql-truncate.html)
- 隔离基线 `675c717e1` 用真实 PostgreSQL 18.6 核对限定 schema 和前缀、同名表隔离、默认外键级联、带起始值的主子表序列、关闭级联后的 SQLSTATE 0A000 及数据保留、事务回滚恢复数据和序列、别名拒绝。查询日志核对实际发送的原生语句。三语文档示例通过真实 HTTP POST 和独立 Artisan `db:postgres-truncate`，两者都复核关联表为空及 identity 起始值。
- MySQL 8.4.13 环境的查询与写入套件 23 项通过，含 SQLite 本地回归及驱动条件用例。独立 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。02-04 继续跟踪 SQL Server 等方言差异，核心账本维持 63/66。临时实例和本批隔离编译与文档产物已回收，项目磁盘可用空间净增 8.59 GB（8.00 GiB），剩余 101.63 GB（94.65 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-postgres-truncate.V7257JOh`。

## SQL Server JSON WHERE 谓词

- 对照固定 Laravel v13.40.0，SQL Server 使用 `OPENJSON` 实现标量包含、键存在及长度，保留布尔值的 JSON 文本语义。旧版 `6d1a81924` 无可复用的 `OPENJSON` 实现。沿用现有路径解析，单独绑定标量、键和路径。末级数组下标经十进制规范化后与 `OPENJSON` 文本键比较，兼顾大小写敏感对象键和超出整数范围的下标。 [固定 Laravel SQL Server Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/SqlServerGrammar.php#L174-L232) · [Microsoft OPENJSON](https://learn.microsoft.com/en-us/sql/t-sql/functions/openjson-transact-sql)
- 隔离基线 `284c07975` 在 Apple Silicon 的 Linux amd64 兼容容器中用真实 SQL Server 2022 Developer 16.0.4325.4 和验证 CA 的 TLS 连接验收。覆盖嵌套条件、参数绑定顺序、布尔与数值、键大小写、数组下标、含点号与单引号的键、JSON null 与 SQL NULL、事务写入和回滚。三语共享示例通过真实 HTTP 及独立 Artisan `db:sqlserver-json` 查询同一批数据。
- MySQL 8.4.13 环境的查询与写入套件 24 项通过，含 SQLite 回归及其他驱动的条件用例。独立 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。02-04 继续跟踪 SQL Server JSON 路径更新与原生截断，核心账本维持 63/66。临时实例和本批隔离编译与文档产物已回收，项目磁盘可用空间净增 8.56 GB（7.97 GiB），剩余 101.58 GB（94.60 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-sqlsrv-json.xRO50O8g`。

## SQL Server 原生清空表与 JSON 路径写入边界

- 固定 Laravel v13.40.0 的 SQL Server Grammar 继承通用 `TRUNCATE TABLE`。保留表名前缀和 Schema 限定，并参考固定 Goravel 的事务 Table 装配，使用现有事务句柄连接。固定 Laravel 文档的 JSON 路径更新范围为 MariaDB、MySQL 和 PostgreSQL，SQL Server Grammar 未实现 JSON 路径赋值。Rust 在执行前返回 `UnsupportedOperation`，可继续按列名绑定 `Value::Json` 替换整份文档。旧版 `6d1a81924` 应用及框架未找到相应的 SQL Server 实现。 [通用 Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/Grammar.php#L1447-L1450) · [固定 Laravel JSON 更新文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#updating-json-columns) · [固定 Goravel 事务 Table](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/db/db.go) · [SQL Server 官方语义](https://learn.microsoft.com/en-us/sql/t-sql/statements/truncate-table-transact-sql)
- 隔离基线 `37b4fd0e4` 在 Apple Silicon 的 Linux amd64 兼容容器中使用真实 SQL Server 2022 Developer 16.0.4325.4，带 CA 验证的 TLS。定向 `database_query_writes` 用例覆盖 Schema、表前缀、同名表隔离、别名拒绝、标识列起始值、事务回滚、外键错误 4712、JSON 路径提前报错与整列 JSON 写入。查询日志核对原生语句。三语共享 `reset_events` 示例通过真实 HTTP POST 和独立 Artisan `db:sqlserver-truncate`，两者都核对空表和标识列起始值。
- MySQL 8.4.13 查询和写入套件共 25 项通过，含 SQLite 回归和其他驱动的条件用例。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点及 18 项浏览器检查通过。02-04 仍跟踪 SQL Server 联结与限额写入，核心账本维持 63/66。临时实例与本批隔离编译和文档产物已回收，项目磁盘可用空间净增 7.73 GB（7.20 GiB），剩余 101.54 GB（94.57 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-sqlsrv-writes.rNPdrejz`。

## SQL Server 联结与限额写入

- 对照固定 Laravel v13.40.0 的 SQL Server Grammar，联结写入通过 `FROM` 使用目标表或别名，普通限额写入使用 `TOP`，带排序时由派生表选定目标行。保持联结、正数偏移和零限额时省略 `TOP` 的行为。SQL 按占位符顺序绑定赋值、联结、谓词和排序参数。Rust 按目标列名生成 `SET` 左侧，右侧保留递增表达式的限定名。固定 Goravel 的事务 Table 对应复用事务执行器。旧版 `6d1a81924` 的 `sqlite_write_target.rs` 与测试提供联结与绑定隔离参考，未发现可复用的 SQL Server 编译器。 [固定 Laravel SQL Server Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/SqlServerGrammar.php#L271-L290) · [固定 Laravel 限额更新](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/SqlServerGrammar.php#L398-L447) · [固定 Laravel 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php#L4213-L4228)
- 在隔离基线 `319446741` 用真实 SQL Server 2022 Developer 16.0.4325.4、Apple Silicon 上的 Linux amd64 兼容容器和带 CA 验证的 TLS 执行。定向测试验证别名、Schema 与前缀、联结增量更新和删除、顺序限额、绑定顺序、纯限额、正数偏移和零限额、事务内删除及回滚。三语共享示例通过真实 HTTP POST 和独立 Artisan `db:sqlserver-bounded`，两者都核对实际选中的目标行。
- MySQL 8.4.13 查询与写入套件共 26 项通过，含 SQLite 回归与其他驱动的条件用例。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。02-04 继续跟踪 SQL Server 的 `CROSS APPLY`／`OUTER APPLY` 及 PostgreSQL 的 `LATERAL`。核心账本维持 63/66。临时实例和本批隔离产物已回收。项目盘净增 11.84 GB（11.03 GiB），可用 101.48 GB（94.51 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-sqlsrv-bounded.esdlKc38`。

## PostgreSQL 与 SQL Server 关联子查询联结

- 对照固定 Laravel v13.40.0 的 `joinLateral`、`leftJoinLateral` 公开契约和两种驱动语法。PostgreSQL 使用 `JOIN LATERAL … ON TRUE`，SQL Server 使用 `CROSS APPLY` 或 `OUTER APPLY`。复用现有子查询来源、别名与绑定规则，SQLite 和 MariaDB 维持方言限制。事务子查询继续沿用同一事务句柄。旧版 `6d1a81924` 未找到可直接复用的关联联结实现。 [固定 Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#lateral-joins) · [PostgreSQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/PostgresGrammar.php#L481-L484) · [SQL Server Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/SqlServerGrammar.php#L512-L517) · [固定 Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php#L3442)
- 隔离基线 `aed43a9ac` 在真实 PostgreSQL 18.6 和 SQL Server 2022 Developer 16.0.4325.4 上定向测试。SQL Server 运行在 Apple Silicon 的 Linux amd64 兼容容器，连接使用验证 CA 的 TLS。覆盖内联结和左联结、前缀与别名、外层引用、投影及子查询参数绑定顺序、无匹配行的 NULL、不同连接拒绝以及读取事务内写入与回滚。三语共享 `latest_posts` 示例在两个驱动上均通过实际 HTTP 和独立 Artisan `db:lateral`。
- MySQL 8.4.13 环境的查询与写入套件 27 项通过，含真实 MySQL LATERAL 和 SQLite 回归及其他驱动的条件用例。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。02-04 继续跟踪 PostgreSQL `updateFrom`。核心账本维持 63/66。临时实例和本批隔离产物已回收。项目盘净增 7.71 GB（7.18 GiB），可用 101.43 GB（94.46 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-lateral.zdx5f1J3`。

## PostgreSQL 联结来源更新

- 对照固定 Laravel v13.40.0 的 `Builder::updateFrom`、PostgreSQL Grammar 和上游回归。使用原生 `UPDATE … FROM`，将联结来源放在 `FROM`，将联结条件按原有布尔顺序接在查询条件之后。保留上游忽略排序、限额和偏移的行为，其他驱动返回不支持。Rust 复用 `Record` 和 `ValueOperand` 绑定普通值、JSON 路径和显式 SQL 表达式。派生来源可能在 `FROM` 中含占位符，按实际 SQL 顺序绑定赋值、来源、查询条件和联结条件。JSON 路径赋值用目标表或别名限定读取列，避免与来源同名列冲突。固定 Goravel 的事务 Table 对应复用现有事务执行器。旧版 `6d1a81924` 未发现可直接复用的 `updateFrom`。 [固定 Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L4062-L4081) · [PostgreSQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/PostgresGrammar.php#L509-L599) · [固定 Laravel 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php#L4401-L4423)
- 隔离基线 `73041f3a2` 在真实 PostgreSQL 18.6 上验收。覆盖 Schema、前缀、别名、派生来源与多来源、绑定顺序、普通值和表达式、两条同列 JSON 路径、联结起始 OR、事务回滚、无来源更新以及空赋值拒绝。三语共享示例通过实际 HTTP POST 与独立 Artisan `db:update-from`，只更新有已付款记录的订单。
- MySQL 8.4.13 环境的查询与写入套件共 28 项通过，含 SQLite 不支持边界及驱动条件用例。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。02-04 继续核对固定 Laravel 的 `crossJoinSub` 与 `straightJoin` 系列。核心账本维持 63/66。临时实例和本批隔离产物已回收。项目盘净增 7.73 GB（7.20 GiB），可用 101.38 GB（94.42 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-pg-update-from.aMUvV0ol`。

## 交叉联结与固定顺序联结

- 对照固定 Laravel v13.40.0 的 `crossJoinSub`、`straightJoin`、`straightJoinWhere` 和 `straightJoinSub`。复用派生来源、连接校验、前缀及 SQL 绑定，交叉联结只编译派生来源。按 MySQL Grammar 和 MariaDB 继承关系生成原生 `STRAIGHT_JOIN`，其余驱动在查询编译时返回不支持。Rust 用同一体系的 `straightJoinWith` 表达 Laravel 的闭包重载。旧版 `6d1a81924` 未找到可直接复用的实现。 [固定 Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L764-L821) · [固定 Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php#L3406-L3441) · [MySQL 8.4 联结语法](https://dev.mysql.com/doc/refman/8.4/en/join.html)
- 隔离基线 `e76840bb8` 在真实 MySQL 8.4.13 与 MariaDB 11.8.9 验证前缀、别名、笛卡尔配对、列联结、值与闭包条件、派生查询和绑定顺序。SQLite 真实执行交叉联结，并在 SQL 预览和发出查询前验证 `STRAIGHT_JOIN` 不支持。三语共享 `label_paid_orders` 示例在 MySQL 和 MariaDB 都通过实际 HTTP 和独立 Artisan `db:join-variants`。
- MySQL 8.4.13 环境的查询与写入套件共 29 项通过，含 SQLite 回归和其他驱动的条件用例。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。02-04 继续核对索引提示。核心账本维持 63/66。临时实例和本批隔离产物已回收。项目盘净增 7.71 GB（7.18 GiB），可用 101.35 GB（94.39 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-join-variants.TcnQglPp`。

## 查询索引提示

- 对照固定 Laravel v13.40.0 的 `useIndex`、`forceIndex` 和 `ignoreIndex`，保留最后一次提示及方言边界。MySQL 与 MariaDB 接受逗号列表。SQLite 对单个非 `PRIMARY` 的强制提示生成 `INDEXED BY`，包括带联结或限额的写入子查询。SQL Server 对单个索引生成 `INDEX`，与行锁选项合并在同一个 `WITH` 中以符合 T-SQL。PostgreSQL 忽略提示。实际生成 SQL 的索引名按固定 Laravel 的字符范围验证。旧版 `6d1a81924` 未找到可直接复用的实现。 [MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/MySqlGrammar.php#L148-L164) · [SQLite Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/SQLiteGrammar.php#L164-L182) · [SQL Server Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/SqlServerGrammar.php#L108-L120) · [T-SQL 表提示](https://learn.microsoft.com/en-us/sql/t-sql/queries/hints-transact-sql-table)
- 隔离基线 `7f21dc4e1` 用真实 MySQL 8.4.13、MariaDB 11.8.9、SQL Server 2022 Developer 16.0.4325.4、SQLite 内存库核对提示、替换、索引列表和非法名称。SQL Server 在 Apple Silicon 上通过 Linux amd64 兼容容器与带 CA 验证的 TLS 运行并验证事务行锁与索引选项组合。SQLite 验证带限额的实际写入包含提示，PostgreSQL 的省略行为通过 SQL 预览核对。三语共享 `indexed_items` 示例在 MySQL、MariaDB、SQL Server 都通过实际 HTTP 与独立 Artisan `db:index-hints`。
- MySQL 8.4.13 环境的查询与写入套件 31 项通过，含 SQLite 回归和其他驱动的条件用例。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。下一批对照固定 Laravel 与交付计划核对 02-04 整单元验收条件。核心账本维持 63/66。临时实例和本批隔离产物已回收。项目盘净增 7.73 GB（7.20 GiB），可用 101.31 GB（94.35 GiB）。提交前周额度剩余 40%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-index-hints.2ZJjpX0H`。

## 查询原生锁定子句

- 按固定 Laravel v13.40.0 的 `lock`、`lockForUpdate`、`sharedLock` 对账。Rust 的 `lock` 接收布尔值或应用定义的 SQL 字符串，所有锁定读取走写连接。MySQL 与 PostgreSQL 在查询尾部编译子句，SQL Server 编译到表来源，SQLite 省略行锁子句。SQL Server 同时使用 `forceIndex` 时，将受信任的 `WITH (...)` 锁定选项与索引放在同一个列表。复用旧版 `6d1a81924` 在 `tests/server/database/connection.rs` 的双事务验收思路。 [固定 Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php#L3139-L3173) · [固定 Laravel 锁定测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseQueryBuilderTest.php#L5257-L5300) · [固定 Laravel 悲观锁文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#pessimistic-locking)
- 隔离基线 `8cf2bac9b` 在真实 MySQL 8.4.13、PostgreSQL 18.6 与 SQL Server 2022 Developer 16.0.4325.4 验证。两条事务连接持锁和跳过，先取到第 2 行，回滚释放后再次取到第 1 行。核对布尔模式、替换行为、绑定顺序、SQLite 子句省略、SQL Server 组合索引提示及非法组合。在三种驱动通过实际 HTTP 和独立 Artisan `db:query-locks` 运行三语示例。
- MySQL 8.4.13 环境查询与写入套件共 32 项通过，含真实共享与排他锁竞争回归、SQLite 与其他驱动的条件用例。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。02-04 整单元核对发现 Laravel `update` 的表达式与标量子查询赋值仍待补齐。核心账本维持 63/66。临时实例和本批隔离产物已回收。项目盘净增 7.71 GB（7.18 GiB），可用 101.26 GB（94.30 GiB）。提交前周额度剩余 39%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-04-query-locks.kUXXSf7B`。
