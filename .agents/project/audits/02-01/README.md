# 02-01 数据库配置、连接与基本 SQL

对照固定 Laravel 13.x [数据库配置](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#configuration)及[执行 SQL](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#running-queries)。核对 [DatabaseManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseManager.php)、[Connection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php)、[SQL 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseConnectionTest.php)和[连接 URL 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/ConfigurationUrlParserTest.php)。装配参考固定 [Goravel Database ServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)。

- 使用现有 `config/database.rs`，框架登记应用级 `db`、`DatabaseManager` 和连接解析契约。具名连接惰性创建共享池，数据库启动不阻塞应用 boot。SQLite 文件以应用根目录解析，缺失时返回驱动错误。内存库使用单连接，默认私有缓存。
- 从旧版 `6d1a8192` 的 `src/database/mod.rs`、`src/database/sqlite.rs` 和 `vendor/launchbadge/sqlx-mysql/OPENRUST.md` 提取路径解析、内存缓存、强制 TLS 和取消后淘汰回归。当前使用固定 SQLx 0.9.0，未启用无 TLS 的 MySQL RSA 认证。Rust 在操作层持有连接租约，取消或协议错误时关闭 MySQL 物理连接，无需移植旧版驱动补丁。
- DB 门面和 Connection 支持异步预处理查询、绑定计数验证、标量、行查询、插入、更新、删除、影响行数和无参数原始 SQL。Rust 使用 `Value` 和有序列记录。MySQL 无符号整数保留完整范围，DECIMAL 用精确文本表示。错误的 Display 和 Debug 省略 SQL、绑定值和凭据，显式错误来源仍可由调用方检查。
- URL 解析优先使用 URL 中的已指定配置。显式 URL 用户缺少密码时使用空密码，以免带入不同用户的配置密码。MySQL TLS 为 Required，配置 CA 时默认 VerifyIdentity。首次真实验收确认加密会话，证书与主机名校验见下方整单元验收。
- 本批运行连接为 SQLite 和 MySQL。读写路由、事务、重新连接及完整 ConnectionInterface 留到 02-02，Query Builder 与 Schema 随各自单元接线。存储过程多结果集见下方续批验收。数据库事件归 02-02。

## 验收

macOS arm64，Rust 1.94.0。隔离副本基于 `21a65e98`、已验收的 01-02y 至 01-07 以及本批文件。MySQL 使用独立的临时 MySQL 8.4.13 容器和独立数据库，应用现有服务未参与测试。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 329 项测试。测试进程启用 `OPENRUST_REQUIRE_MYSQL=1`，MySQL 路径实际执行。
- SQLite 验证默认文件读写、绑定数量、外键、缺失文件、未知连接、类型及内存库隔离。真实 MySQL 验证加密会话、URL 优先、参数化读写、精确类型、驱动错误，以及执行取消和元数据锁阻塞冷 PREPARE 后物理连接更换。
- 3 次真实回环 HTTP 请求通过 DB 门面验证两应用 SQLite 文件隔离。Artisan 子进程 `db:probe` 实际查询 SQLite 并退出成功。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语示例由 Rust 1.94.0 编译执行，另用 SQLite 读取写入结果。文档构建及根路径、子路径下共 12 个浏览器组合通过。当前工作区 Cargo 锁定依赖解析通过，保留已有包版本。

下一批 02-02 接读写连接、切换与事务，复用两种已验收的物理连接池，并以真实 SQLite 与专用 MySQL 8.4 验证事务、锁与故障回退。

补交复验。02-02 复用了原始目录，单连接实现从该快照提取并独立重建。读写路由、事务和作用域连接状态归入 02-02。待提交树通过 368 项工作区测试、严格 Clippy 和格式检查，真实 MySQL 路径已执行，SQLite、HTTP 和 Artisan 数据库探针通过。

## 多结果集查询

对照 [Laravel 数据库文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#selecting-multiple-result-sets)及[固定 Connection 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L462)。旧版未提供多结果集入口。当前复用预处理、参数绑定、行解码、读写连接路由和物理连接租约。

- `DB::selectResultSets` 和 `Connection::selectResultSets` 显式选择读路由或写连接。事务接口直接使用当前事务的物理连接。返回值为 `Vec<Vec<Row>>`，沿用数据库层的行表示。
- 独立 MySQL 8.4.13 上同时运行原生预处理 PHP PDO 和 SQLx。含空查询的绑定过程返回 `[[5],[],[6],[]]`，无查询的过程和普通空查询均返回单个空结果集。最后的空集来自 MySQL 的 CALL 状态。PDO SQLite 的 `nextRowset` 返回 `IM001`，OpenRust 对应 `UnsupportedOperation`。
- Rust 1.94.0 隔离基线 `991ab3ada` 上，`database_query` 11 项和 `database_sql` 7 项通过。真实 MySQL 覆盖绑定计数、无效过程后的连接恢复、读写连接 ID、事务连接固定及提交。SQLite 回归覆盖不支持的接口及常规查询。针对框架库和这两个测试目标的严格 Clippy 通过。
- 独立验收二进制通过实际 `Application::handleCommand` 执行 `db:rowsets`，验证 DB 门面和三语示例。应用 `main artisan inspire` 成功退出。VitePress 构建及根路径、子路径浏览器验收通过，共 12 项站点组合和 6 项多结果集小节检查。
- PDO 的自定义抓取选项在 Rust 中沿用统一的类型化 `Row`。命名绑定见下方整单元验收。

## 命名绑定与整单元验收

对照固定 Laravel [命名绑定](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md#using-named-bindings)和 [Connection::bindValues](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L757)。旧版未提供原始 SQL 命名绑定，当前复用参数预处理与连接租约。

- `DB`、`Connection` 和 `Transaction` 的原始 SQL 方法接受 `Bindings::Named`，允许键带冒号，按 SQL 中的顺序传递原值的引用。仅改写通用占位符。字符串、标识符和注释照原样传给数据库。位置参数调用仍借用 SQL 和传入的值。
- PHP PDO 8.5 使用原生预处理对照两种驱动。MySQL 拒绝重复参数名，SQLite 允许重复引用。SQLite PDO 还接受混合和缺失参数。OpenRust 延续现有的严格绑定数量约定，命名接口要求同一语句使用一种占位符并显式提供所有值。缺失、未使用和重复键返回结构化错误。
- 真实 MySQL 8.4.13 证书夹具验证默认 `verify_identity` 接受与 IP SAN 匹配的证书、拒绝不匹配主机名，显式 `verify_ca` 可校验证书链。SQLx 的 CA-only 校验器依赖 Rustls 进程级加密提供器。双后端构建时，框架在该模式下补装与 SQLx 一致的 ring 后备，尊重应用已设置的提供器。
- Rust 1.94.0 上 `database_sql`、`database_query` 和 `database_query_writes` 共 24 项通过，含实际 SQLite 和 MySQL 8.4.13、预处理读写、游标、存储过程、读写路由、事务、证书验证以及 DB 门面的回环 HTTP 和 Artisan 子进程。工作区严格 Clippy 通过。本批 Rust 文件格式检查通过。全量 `cargo fmt --all --check` 仍报告基线已有的 `sanctum_authentication.rs` 格式差异。
- 原生二进制 `main artisan inspire` 通过。独立 Artisan 命令在真实 MySQL 和 SQLite 各执行三语命名绑定示例。三语 VitePress 构建、根路径及子路径下 12 项站点组合与 6 项小节检查通过。
- 02-01 计划内的 SQLite／MySQL 基本 SQL、连接 URL、未知连接和驱动错误已有首次及续批验收证据，整单元闭合。连接与事务事件归 02-02，其他数据库驱动及生产部署矩阵继续跟踪。

## 从应用名称派生默认数据库名称（2026-11-19）

按用户要求，在应用 `config/database.rs` 提取独立的 `database_name` 方法。复用现有 `Str::ascii` 转写，保留小写 ASCII 字母与数字，将连续分隔符合并为下划线，数字开头加 `app_`，空结果回退为 `app`。最多保留 63 个 ASCII 字节，并移除截断后尾部的下划线。该上限参考 [PostgreSQL 默认标识符限制](https://www.postgresql.org/docs/current/sql-syntax-lexical.html#SQL-SYNTAX-IDENTIFIERS)，字符选择兼顾 [MySQL 8.4 标识符规则](https://dev.mysql.com/doc/refman/8.4/en/identifiers.html)。

MySQL、MariaDB、PostgreSQL 和 SQL Server 共用一次解析的默认库名，显式 `DB_DATABASE` 按原值使用。SQLite 的默认数据库路径沿用原实现。旧版 `v0.x` 的 `src/config/database.rs` 以连接 URL 和连接池设置为主，本批复用当前配置入口，名称转换属于用户指定的应用默认策略。

Rust 1.94.0 配置回归验证默认包名、应用名覆盖、空格及特殊字符、重音字母、中文、空值、数字开头、长度及尾部边界，还有包含大小写和符号的显式 `DB_DATABASE`。配置测试、应用二进制和配置测试的严格 Clippy、全仓格式及差异空白检查通过。本批仅修改配置派生规则，未执行真实数据库建库操作。三语数据库文档同步，站点构建及两种部署路径的 12 项浏览器检查通过。
