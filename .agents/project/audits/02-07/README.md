# 02-07 数据填充

核对固定 Laravel 13.x 的[编写 Seeder](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/seeding.md#writing-seeders)和[运行 Seeder](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/seeding.md#running-seeders)，对照 [Seeder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Seeder.php)、[SeedCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Seeds/SeedCommand.php)、[调用回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSeederTest.php)、[连接恢复回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/SeedCommandTest.php)和[生成器回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Generators/SeederMakeCommandTest.php)。参考固定 [Goravel Seeder](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/seeder/seeder.go) 的编译期注册和顺序调用。核对旧版 `6d1a8192` 的 `database/seed.rs` 及 `tests/server/database/migration.rs`，提取应用 Provider 接线和实际迁移后数据回归。

- 实现 `Seeder`、`SeederEntry` 和应用级注册清单。默认 `DatabaseSeeder` 作为应用入口。子 Seeder 支持 `call`、`callOnce`、`callSilent` 和 `callWith`。同一次调用记录已成功的子项，循环调用返回错误，失败停止后续调用。
- `db:seed` 支持位置参数、`--class`、`--database`、`--force` 和无交互取消。迁移命令复用同一 Seeder 执行路径，接通 `migrate --seed`、`migrate:refresh` 和 `migrate:fresh` 的 `--seed` 与 `--seeder`。按固定 Laravel 跳过预演中的填充。生成器写入 Rust 模块，重复生成保留原文件，显式 `--force` 可覆盖。
- Rust 通过 `SeederContext` 提供连接、参数与应用服务。默认连接通过任务作用域设置，让 Seeder 中的 DB 门面选择指定连接，同一应用的并发操作各用自己的连接。调用返回、失败或取消后恢复外层作用域。
- 应用默认 Seeder 现为用户编写填充逻辑的入口。Eloquent 工厂、批量赋值控制和模型事件抑制随 05-04 及模型能力衔接。Rust 源文件生成后需显式注册并重建，Artisan 进度行沿用当前文本输出。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-02-07.enZkH4` 以 02-06 为前置，逐文件核对本批源码、测试和三语文档。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。357 项通过，0 项忽略。真实 MySQL 8.4.13 使用专用容器，清库及文档测试各用独立 schema。
- 实际 Artisan 验证默认和指定类、位置参数优先级、子项顺序、单次去重、静默、传参、未知及失败类的退出码、循环拒绝、指定连接隔离、迁移预演跳过填充和迁移后填充。测试检查失败后的默认连接，额外回归覆盖同一管理器的并发作用域及取消恢复。
- 生产环境通过伪终端实际确认后写入数据，真实子进程验证无交互拒绝和 `--force`。`./artisan inspire` 正常退出。生成器的三语命令实际执行，生成的 Rust 文件编译通过。
- 三语 Seeder 示例编译进测试应用，其文档命令分别在 SQLite 和 MySQL 执行并核对写入结果。`npm run build` 成功，12 项浏览器组合与 6 次 Seeder 正文及侧栏检查通过。工作区锁定依赖解析和 `git diff --check` 通过。

下一批 02-08 通用集合，以 01-01 应用基础为前置，核对键保留、映射、过滤、聚合、排序、分组和序列化。
