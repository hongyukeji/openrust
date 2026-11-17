# T-03 数据库测试

- 按固定 [Laravel Database Testing](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md) 核对迁移、Seeder 和数据库断言。参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go) 的应用装配，复用旧版 `6d1a8192` 中独立 SQLite 文件和正常迁移路径。
- `TestApp::sqlite` 为每个测试应用创建独立 SQLite 文件，经应用 Provider 配置连接。迁移和 Seeder 继续走实际 Artisan。三个断言复用查询构建器，支持 SQL NULL。每次创建独立夹具实现测试间隔离。Laravel 的 `RefreshDatabase` 事务策略及其他驱动的测试库管理仍待接入。
- Rust 1.94.0 锁定离线工作区 537 项通过，5 项已有测试夹具忽略，格式和严格 Clippy 通过。定向用例并行执行两个应用的 `migrate --seed`，验证空列和行数、缺失 Seeder 退出码 1、`migrate:fresh --seed` 重试、另一应用数据和临时文件释放。
- 英文、简体中文、繁体中文示例提取执行通过。三语站点构建、12 项通用浏览器检查、6 项数据库测试页面定向检查通过。

## MySQL 测试事务隔离

- 核对固定 Laravel `RefreshDatabase` 的迁移完成后开始事务、应用内部事务、测试结束回滚与断开连接。02-06 的 MySQL 迁移和 02-07 Seeder 已验收。固定 Goravel 提供容器测试库和测试应用装配。旧版 `testing/database/{owned_pool,ownership,lease}.rs` 使用 SeaORM 的所有权和回收协议，本批保留专用测试库、连接与清理绑定的边界，通过当前 SQLx 会话实现回滚。 [固定 Laravel RefreshDatabase](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Testing/RefreshDatabase.php) · [固定 Laravel 数据库测试指南](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md)
- `TestApp::databaseTransaction` 在默认 MySQL 连接上固定一个写会话。测试回调内的 HTTP 内核、进程内 Artisan 和数据库断言复用该会话。应用事务使用保存点。成功或错误返回时回滚，取消或放弃保存点时关闭物理会话，后续查询不会静默换池重试。数据库结构变更与可能隐式提交的命令在事务开始前执行。其他连接和独立 Tokio 任务保持各自边界。
- 隔离基线 `cb10f0c4b`，MySQL 8.4.13。回归覆盖迁移与基础 Seeder、回调中的 HTTP 与 Artisan 写入、嵌套提交后统一回滚、回调错误、取消、丢弃嵌套句柄、拒绝 DDL 和独立观察连接。另复验普通 MySQL 查询构建器及流式游标。
- 独立 `./artisan inspire`、Rust 1.94.0 全仓格式与严格 Clippy、三语站点、12 项通用和 6 项小节浏览器检查通过。本批临时 MySQL、文档及隔离 Cargo 已回收，Cargo 清理 3.0 GiB。项目卷可用 80.71 GB（75.17 GiB），周额度剩余 51%。测试库自动分配、`RefreshDatabase` 的迁移准备及多连接事务仍待接入。证据目录 `/private/tmp/openrust-T-03-mysql-transaction.gjfdEQx5`。

## MySQL 测试库归属与租约

- 对照固定 Laravel 并行测试的独立库、迁移与清理流程，以及固定 Goravel 的容器测试数据库。复核旧版 `testing/database/{lease,marker,owned_pool,ownership,resources}.rs`，提取随机命名、同物理会话租约、例程归属标记、业务连接验证及忙库保护，移植到当前 SQLx 连接工厂。 [Laravel 并行测试数据库](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Testing/Concerns/TestDatabases.php) · [Goravel 测试数据库](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/testing/docker/database.go)
- `TestApp::mysql` 在指定测试服务器严格创建随机库，应用端新物理连接核验服务器 UUID、选中的库、租约持有者及归属标记。夹具关闭时先停用并回收应用池，再以原管理会话验证租约、标记及服务器活动状态。外部会话仍选中库或归属验证失败时保留该库，返回含库名的错误。显式 `close().await` 等待结果，普通析构在仍运行的 Tokio runtime 上尝试清理。进程中断后的恢复记录与重领仍待接入。
- 隔离基线 `7fad0453a`，真实 MySQL 8.4.13 验证并发创建独立库、实际 Artisan 迁移与 Seeder、`migrate:fresh` 后标记保留并清库、提前取得的连接句柄关闭后失效、外部连接占用时保留、标记变化时重连与清理拒绝。复验上批默认连接事务隔离。
- Rust 1.94.0 全仓格式、严格 Clippy、独立 `./artisan inspire`、三语站点、12 项通用和 6 项小节浏览器检查通过。临时 MySQL 和隔离产物已回收，Cargo 清理 2.9 GiB。项目卷可用 80.42 GB（74.89 GiB），周额度剩余 50%。证据目录 `/private/tmp/openrust-T-03-mysql-ownership.mzG7QzzJ`。

## MySQL RefreshDatabase 迁移准备

- 对照固定 Laravel `RefreshDatabase::refreshTestDatabase` 和 `CanConfigureMigrationCommands`，首次执行 `migrate:fresh` 后再开始测试事务。复核旧版 `testing/database.rs` 的建库后迁移和清理状态，沿用已验收的 SQLx 归属夹具与实际 Artisan 命令。 [固定 Laravel RefreshDatabase](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Testing/RefreshDatabase.php) · [固定 Laravel 迁移选项](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Testing/Traits/CanConfigureMigrationCommands.php)
- `TestApp::refreshDatabase` 在受管 MySQL 夹具上使用同一组选项准备一次，支持默认或具名 Seeder 与清除视图。普通回调结束后回滚，回滚成功后复用基线。迁移失败或事务失效后可重试。迁移等待被取消时，夹具持有任务并等待其完成后再决定复用或重试，避免并行重建同一库。
- 隔离基线 `b932c1e0d`。真实 MySQL 8.4.13 验证具名 Seeder 首次失败后同选项重试、成功与回调错误后回滚、有效基线复用、异常事务后的重建、选项改变时清除视图、迁移中取消后复用同一准备任务及默认 Seeder。管理租约和默认连接事务两项回归另行通过。
- Rust 1.94.0 全仓格式、严格 Clippy、独立 `./artisan inspire`、三语站点、12 项通用和 6 项小节浏览器检查通过。临时 MySQL 和隔离产物已回收，Cargo 清理 2.9 GiB。项目卷可用 79.44 GB（73.98 GiB），周额度剩余 50%。多连接事务、迁移事件抑制及进程级异常后的归属恢复待补。证据目录 `/private/tmp/openrust-T-03-mysql-refresh.MF86Nmvt`。

## MySQL 多连接测试事务

- 对照固定 Laravel `RefreshDatabase::connectionsToTransact`、`beginDatabaseTransaction` 与默认连接迁移。复核旧版 `testing/database/{lease,owned_pool,resources}.rs`，沿用按数据库核验租约和归属的模式。旧版没有同一测试内对多个具名连接开启回滚事务的 API。 [固定 Laravel RefreshDatabase](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Testing/RefreshDatabase.php) · [固定 Laravel 数据库测试指南](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md)
- `TestApp::mysqlConnections` 逐名分配独立托管库，第一条为默认。各池匹配自己的归属记录，关闭会尝试清理所有已分配库。用 `RefreshDatabaseOptions::connections_to_transact` 或 `databaseTransactionOn` 选择回滚连接，空列表使用默认。启动失败回滚已开启的事务，结束时逐条尝试回滚。默认库由 `migrate:fresh` 准备，其他库继续走显式 Artisan 迁移。
- 隔离基线 `6b9fc960b`，真实 MySQL 8.4.13 验证双库独立租约、HTTP 和进程内 Artisan 的同用例写入、读别名复用、重复选择拒绝、启动与回滚失败的清理、仅选择一条连接时其他连接的正常提交，以及归属失败时仍清理其余库。复验 MySQL 单连接事务、迁移准备与归属三项。
- Rust 1.94.0 全仓格式、严格 Clippy、独立 `./artisan inspire`、三语站点、12 项通用和 6 项小节浏览器检查通过。本批临时 MySQL 和隔离产物已回收，Cargo 清理 2.9 GiB。项目卷可用 78.83 GB（73.41 GiB），周额度剩余 50%。迁移事件抑制及进程中断后的归属恢复待补。证据目录 `/private/tmp/openrust-T-03-mysql-multi.tZsd4tuc`。

## RefreshDatabase 迁移准备的连接事件抑制

- 固定 Laravel 的 `WithoutMigrationEvents` 在准备阶段停用所选连接的事件分发器。上游回归检查迁移查询与测试查询的分界，Migrator 继续使用自己的生命周期分发器。旧版没有同名夹具选项，本批复用当前连接身份、查询观察器及应用与任务作用域。 [固定 Laravel 夹具](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Testing/RefreshDatabase.php) · [固定 Laravel 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Foundation/Testing/LazilyRefreshDatabaseWithoutMigrationEventsTest.php)
- `RefreshDatabaseOptions::without_migration_events` 只在准备任务内抑制所选连接的查询、事务与连接建立事件。查询日志、迁移生命周期、未选连接以及其他任务仍按原路径。真实 MySQL 8.4.13 验证选中与未选连接、同一连接的并发任务、回调和普通 Artisan、失败后恢复，以及事务事件和日志行为。三项 MySQL 回归覆盖迁移重试及取消、具名多连接和单连接事务。
- 隔离基线 `97778566c`。Rust 1.94.0 全仓格式、严格 Clippy、独立 `./artisan inspire`、三语站点、12 项通用和 6 项正文浏览器检查通过。本批临时 MySQL 和隔离产物已回收，Cargo 清理 2.9 GiB。项目卷可用 78.80 GB（73.39 GiB），周额度剩余 50%。进程中断后的归属恢复待补。证据目录 `/private/tmp/openrust-T-03-mysql-migration-events.48Cmf1p8`。

## MySQL 测试进程中断后的恢复

- 固定 Laravel 并行测试使用进程令牌分配并清理数据库，Goravel 参考仍为容器测试数据库及实际 Artisan 迁移。复核旧版 `testing/database/ownership.rs` 的先记归属、再建库，以及 `lease.rs`、`marker.rs` 的回收校验。Rust 夹具用私有目录内的不可变记录和 MySQL 会话租约清理被强制终止的独立库。 [固定 Laravel 数据库测试](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md) · [固定 Laravel 并行数据库](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/testing.md#parallel-testing-and-databases)
- `TestApp::mysqlWithRecovery` 及 `mysqlConnectionsWithRecovery` 在创建每个库之前原子写入并同步记录。记录包含服务器 UUID 和归属令牌，连接凭据由调用者另行提供。 `recoverMysql` 逐份恢复，要求服务器一致、取得租约、库内归属标记匹配、没有活动外部连接。MySQL 明确返回库不存在时移除记录。权限不足、库忙或归属无法验证时保留记录。
- 隔离基线 `095b16728`。用 `python3 -B tests/mysql_acceptance.py` 在 MySQL 8.4.13 跑七项数据库夹具回归。子进程执行真实 Artisan `migrate --seed` 后被强制终止。父进程验证活租约及忙库拒绝、权限不足及归属不匹配时保留记录、恢复成功并保留另一夹具、普通关闭删除记录，以及重复恢复。HTTP 事务、迁移重试、事件抑制与具名连接回归通过。
- 三语文档示例提取后在独立 MySQL 8.4.13 夹具中执行。独立 `./artisan inspire`、全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。临时 MySQL、文档与隔离 Cargo 产物已回收。证据目录 `/private/tmp/openrust-T-03-mysql-recovery.y1DfxNzB`。
- 剩余 Laravel 差异包含独立重置策略 `DatabaseMigrations`、`DatabaseTruncation`，Seeder 测试入口，以及空表、软删除、模型存在性和查询次数等数据库断言。继续按小批对照并验证。

## 数据库行、软删除及模型断言

- 对照固定 Laravel `InteractsWithDatabase`、`NotSoftDeletedInDatabase` 和 `FoundationInteractsWithDatabaseTest`。复核旧版 `v0.x` 的数据库夹具及测试，沿用连接隔离，行值匹配复用当前查询构建器的空值安全等值条件。 [Laravel Available Assertions](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md#available-assertions) · [Laravel 断言回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Foundation/FoundationInteractsWithDatabaseTest.php)
- 接通空表、软删除、存活行、模型存在和模型缺失断言。模型按实际表、连接名和主键校验，直接查行，因此保留软删除行的存在语义。表断言以 `On` 方法声明可选连接，软删除的 `On` 方法还可指定列。Rust 用独立模型方法和显式循环表达 PHP 的模型重载及批量参数。查询次数断言仍待接入。
- 隔离基线 `d5f02e59c`，运行 `python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test -p openrust-framework --test database_testing --locked --offline mysql_ -- --test-threads=1`，MySQL 8.4.13 八项通过。两套独立库用 Artisan 建同名表，验证命名连接、空表、空值、自定义主键和软删除列。软删除后按模型仍可查到，物理删除后断言缺失，存活行断言因缺行而失败。
- 三语代码示例提取后在独立 MySQL 8.4.13 夹具中通过。独立 `./artisan inspire`、全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。临时 MySQL、文档和隔离 Cargo 产物已回收。日志保留于 `/private/tmp/openrust-T-03-assertions.nBwMSdyq`。

## 数据库查询次数断言

- 对照固定 Laravel 的 `InteractsWithDatabase::expectsDatabaseQueryCount` 与 `FoundationInteractsWithDatabaseTest::testExpectsDatabaseQueryCount`，保留注册后计数、默认的应用总数、具名连接身份筛选、`pretend` 和关闭时精确核对。复核旧版 `v0.x` 的测试数据库夹具及固定 Goravel 的容器数据库，沿用隔离和实际 Artisan 入口。 [Laravel 查询次数断言](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md#expects-database-query-count) · [固定 Laravel 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Foundation/FoundationInteractsWithDatabaseTest.php#L465)
- Rust 在应用夹具的查询事件生成点记录，不受事件替身的交付选择影响。夹具关闭前核对，显式 `close().await` 先核对再清库并返回错误，正常析构时按断言失败处理。已在异常展开时保留原始 panic。数据库准备阶段被抑制的事件和失败 SQL 不计入。
- 隔离基线 `90fca6a2e`。使用 `python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test -p openrust-framework --test database_testing --locked --offline mysql_ -- --test-threads=1`，MySQL 8.4.13 十项通过。新增验证 HTTP 内核、进程内 Artisan、不同命名连接与读别名、两个应用隔离、事件替身、查询预演和失败 SQL。计数不符时，显式关闭仍回收测试库，普通析构报告断言失败。
- 三语示例在独立 MySQL 执行。独立 `./artisan inspire`、全仓格式、严格 Clippy、三语站点与 18 项浏览器检查通过。临时 MySQL、文档和隔离 Cargo 产物已回收。日志保留于 `/private/tmp/openrust-T-03-query-count.5Ai3V5mb`。

## Seeder 测试入口

- 固定 Laravel 的 `InteractsWithDatabase::seed` 把默认 Seeder 或名称列表交给 Artisan `db:seed`，按列表顺序关闭交互执行。核对上游 `DatabaseSeederTest`、固定 Goravel 的测试 `Database.Seed` 和旧版 `v0.x` 的 `database/seed.rs`。沿用旧版经应用容器运行 Seeder 的入口，复用当前编译注册表和 Artisan。 [Laravel Running Seeders](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md#running-seeders) · [固定 Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Testing/Concerns/InteractsWithDatabase.php#L407)
- `TestApp::seed()` 运行 `DatabaseSeeder`，`seedWith` 接收名称切片，`seedOn` 显式选择连接。每项调用同一应用的 Artisan，非零退出状态返回错误并停止列表。测试回调里的 Seeder 写入沿用已选连接的事务范围。
- 隔离基线 `05f147478`。用 `python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test -p openrust-framework --test database_testing --locked --offline mysql_ -- --test-threads=1` 验证真实 MySQL 8.4.13 十一项。覆盖默认填充、空列表、限定名称、实际插入顺序、未知 Seeder 及列表停止、双库回滚和未选库独立提交。
- 三语示例提取后在独立 MySQL 运行。独立 `./artisan inspire`、全仓格式、严格 Clippy、三语站点及 18 项浏览器检查通过。临时 MySQL、文档和隔离 Cargo 产物已回收。日志保留于 `/private/tmp/openrust-T-03-test-seeders.caL3ZHPb`。
