# 02-06 迁移命令与批次账本

对照固定 Laravel 13.x 的[生成迁移](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#generating-migrations)、[迁移结构](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#migration-structure)、[执行](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#running-migrations)和[回滚](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#rolling-back-migrations)。核对 [Migrator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/Migrator.php)、[账本](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/DatabaseMigrationRepository.php)、[迁移命令](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations)和对应的数据库迁移测试。参考固定 [Goravel Migrator](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/migration/migrator.go) 的编译期注册与运行边界。复用旧版 `6d1a8192` 的 `database/migration.rs` 及 `database/migration/execution.rs` 中显式迁移目录、具名连接、写库账本和同一执行器串行回归。

- 迁移 Provider 注册应用自己的迁移清单和账本。初始应用迁移编译到二进制，包含用户、重置令牌和会话表。`main.rs` 增加根应用数据库模块声明。生成器创建带时间戳的 Rust 文件，手动加入 `MigrationEntry` 后重新构建。
- 接通 `make:migration`、`migrate`、`migrate:install`、`migrate:status`、`migrate:rollback`、`migrate:reset`、`migrate:refresh`、`migrate:fresh` 和 `db:wipe`。按文件名和批次维护账本，支持指定连接、已编译路径、step、batch 和生产确认。账本只记录成功的迁移。MySQL 清库独占物理会话并恢复外键设置，SQLite 清库复用事务化 Schema 会话。
- Rust 用 `MigrationContext` 显式传递连接与预演输出。空库预演会准备账本，迁移 SQL 和迁移记录保持未执行。绑定值以占位符显示。SQLite 依赖目录的表重建在预演中返回错误。与固定版的 SQLite/MySQL 语法一致，迁移器不在整个迁移外自动包事务，先成功的语句和批次记录在失败后可见。
- 未闭合的迁移功能包括依赖缓存原子锁的跨进程 `--isolated`、Schema dump/load、迁移事件、空文件自动创建及 Seeder 选项。当前串行锁作用于同一应用的 Migrator。输出使用当前 Artisan 的文本格式。Seeder 选项随 02-07 衔接。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-02-06.UCKIPa` 以 02-05 为前置，叠加本批源码、测试、三语文档和必要框架接线。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。351 项通过，0 项忽略。测试使用专用 MySQL 8.4.13，清库测试在该容器内另一专用 schema 执行。
- 从真实 Artisan 进程在空 SQLite/MySQL 上验证预演、应用、状态、指定批次预演、指定条数回滚、重跑、重置及清库。另运行 `./artisan migrate:install` 两次，确认同一空账本保留。实际生成的 Rust 文件可编译，重复生成返回失败并保留原文件。伪终端实际输入 yes 验证生产确认，子进程覆盖无交互拒绝和 `--force`。
- 失败及并发回归验证先成功项保留、后续重试、同一 Migrator 的重复执行串行、单项批次递增、跳过项不入账和指定批次回滚。`./artisan inspire` 正常退出，工作区既有 HTTP 数据库回归通过。
- 三语新增 Rust 示例各在 SQLite 和 MySQL 执行，三语命令块逐条执行，生成器输出的 Rust stub 实际编译。`npm run build` 成功，12 项浏览器组合及 6 次迁移页面与侧栏检查通过。主工作区依赖锁定和 `git diff --check` 通过。

下一批 02-07 数据填充，以当前迁移账本、具名连接和 Artisan 装配为前置。

## Bootstrap 注册复验

应用启动通过 `bootstrap/app.rs` 的 `withMigrations(database::migrations::ENTRIES)` 登记编译期迁移清单。实际 Artisan 子进程在独立 MySQL 8.4.13 上通过 `artisan_migrations_use_a_real_mysql_ledger`，覆盖迁移、状态、预演及回滚。三语文档构建和 12 项浏览器组合通过。

## 编译时自动发现复验

根应用的 `build.rs` 从 `database/migrations/` 的直接子文件生成模块和 `ENTRIES`，原有 `withMigrations` 接入生成结果。以 `YYYY_MM_DD_HHMMSS_name.rs` 为文件约定，每个文件提供实现 `Migration` 的 `Definition`。构建监听目录变化。账本名称、`--path` 和内置 Sanctum 迁移的功能开关沿用现有契约。

在隔离副本和 MySQL 8.4.13 上先构建初始二进制，再加入手写迁移并用实际 `make:migration` 生成第二份文件。原二进制未列出两份迁移。重建后 `migrate:status` 均显示 Pending，实际 `migrate --force` 按名称顺序建表入账，`migrate:rollback --step=2 --force` 逆序删除两张表及账本记录。全程未编辑迁移注册清单。移除两个文件并重建后，状态列表也移除了两个条目。

默认构建未执行 Sanctum 迁移，启用 `--features sanctum` 后 MySQL 账本回归通过。另核对 `make:notifications-table` 和自定义目录的 `make:migration --path` 提示。Rust 1.94.0 严格 Clippy、改动文件格式、三语站点构建和 12 项浏览器检查通过。全仓格式检查仍检出既有 Sanctum 测试格式差异。

## 清单访问接口复验

生成的迁移清单改由 `database::migrations::entries()` 提供，应用启动和 Job Batch 测试均使用该入口。MySQL 8.4.13 实际 Artisan 复验手写与生成迁移在重建后可发现、可执行及可逆序回滚，移除文件重建后清单同步更新。
