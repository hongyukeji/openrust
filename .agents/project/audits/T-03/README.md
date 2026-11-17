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
