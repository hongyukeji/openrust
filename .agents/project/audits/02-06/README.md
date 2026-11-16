# 02-06 迁移命令与批次账本

对照固定 Laravel 13.x 的[生成迁移](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#generating-migrations)、[迁移结构](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#migration-structure)、[执行](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#running-migrations)和[回滚](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#rolling-back-migrations)。核对 [Migrator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/Migrator.php)、[账本](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/DatabaseMigrationRepository.php)、[迁移命令](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations)和对应的数据库迁移测试。参考固定 [Goravel Migrator](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/migration/migrator.go) 的编译期注册与运行边界。复用旧版 `6d1a8192` 的 `database/migration.rs` 及 `database/migration/execution.rs` 中显式迁移目录、具名连接、写库账本和同一执行器串行回归。

- 迁移 Provider 注册应用自己的迁移清单和账本。初始应用迁移编译到二进制，包含用户、重置令牌和会话表。`main.rs` 增加根应用数据库模块声明。生成器创建带时间戳的 Rust 文件，手动加入 `MigrationEntry` 后重新构建。
- 接通 `make:migration`、`migrate`、`migrate:install`、`migrate:status`、`migrate:rollback`、`migrate:reset`、`migrate:refresh`、`migrate:fresh` 和 `db:wipe`。按文件名和批次维护账本，支持指定连接、已编译路径、step、batch 和生产确认。账本只记录成功的迁移。MySQL 清库独占物理会话并恢复外键设置，SQLite 清库复用事务化 Schema 会话。
- Rust 用 `MigrationContext` 显式传递连接与预演输出。空库预演会准备账本，迁移 SQL 和迁移记录保持未执行。绑定值以占位符显示。SQLite 依赖目录的表重建在预演中返回错误。与固定版的 SQLite/MySQL 语法一致，迁移器不在整个迁移外自动包事务，先成功的语句和批次记录在失败后可见。
- 未闭合的迁移功能包括非 MySQL 的 Schema dump/load、缺库创建及 Seeder 选项。当前串行锁作用于同一应用的 Migrator。输出使用当前 Artisan 的文本格式。Seeder 选项随 02-07 衔接。

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

## 跨进程迁移隔离

对照固定 Laravel [MigrateCommand 的隔离声明](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/MigrateCommand.php#L21)、[Console Command 的竞争退出与释放](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Command.php#L278) 和 [CacheCommandMutex 的一小时租期与键名](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/CacheCommandMutex.php#L45)。旧版 `migration_lock.rs` 使用数据库会话上的 advisory lock 和等待重试，保留其真实子进程验收思路。主线沿 Laravel 使用共享缓存及竞争时立即退出。

- `CommandDefinition::isolatable()` 声明命令支持隔离，Artisan 在处理器之前获取默认缓存的原子锁。迁移命令用同一个 `framework/command-migrate` 键，具名数据库和迁移路径不改变锁名。统一使用斜杠，使不同操作系统上的部署进程共享同一个锁。退出码可省略或提供整数。
- Rust 使用缓存的所有者检查释放，避免旧执行者删除新持有者的锁。缓存不提供原子锁或锁操作失败时返回命令错误。数据库缓存需要已创建的锁表，首次迁移可使用 Redis。租期为一小时，正常返回、迁移错误和非零状态都走释放。
- `docker/accept.py --mysql --redis` 使用临时 MySQL 8.4.13 与 Redis 8.10.3。实际首进程持 Redis 锁并阻塞在 MySQL 迁移账本写入。两个竞争调用分别以 0 和 19 退出，锁所有者保持不变。验证约一小时的 Redis TTL、成功及失败后锁消失、随后重试成功、缓存不可用时账本无变更，以及数据库缓存的锁表无残留。
- 原有真实 MySQL 迁移账本回归通过。隔离副本从 `3d2641c0f` 取 Docker 验收入口，实际执行三语示例的 `./artisan migrate --isolated`、`./artisan migrate --isolated=12`、`./artisan inspire`。严格 Clippy、变更文件格式、三语站点构建和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## MySQL Schema 转储与恢复

对照固定 Laravel [压缩迁移文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#squashing-migrations)、[DumpCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/DumpCommand.php#L25)、[MySqlSchemaState](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/MySqlSchemaState.php#L19) 和 [MigrateCommand 的加载流程](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/MigrateCommand.php#L278)。旧版未找到 `schema:dump` 或 mysqldump 接线，沿用主线写连接、迁移账本和编译期迁移清单。

- `schema:dump` 使用选定写端点的 MySQL 原生客户端。支持 `--database`、`--path`、`--without-migration-data` 和 `--prune`。保存结构及账本记录，排除业务行和自增计数。转储成功后原子替换目标。清理限定编译过的根应用迁移源文件，保留 Rust 模块声明和其他文件。
- `migrate` 在账本无记录时自动识别默认 `.dump` 或 `.sql` 文件，也可用 `--schema-path`。预演跳过加载。加载成功后按快照账本跳过已执行迁移，继续当前二进制中的剩余迁移。原生客户端返回错误时停止，MySQL 已执行的 DDL 仍遵循其原生提交语义。
- 使用独立 MySQL 8.4.13，调用同一夹具内的真实 MySQL 8.4.13 客户端。验收覆盖表、视图、触发器和例程，账本数据保留、业务行排除、仅结构导出、清理范围、预演、无效快照失败后重试和已有记录时跳过加载。旧的真实 MySQL 迁移账本和 MySQL／Redis 双进程隔离回归通过。
- 三语共用的 `./artisan schema:dump --database=mysql`、`./artisan migrate --database=mysql` 及 `./artisan inspire` 已运行。严格 Clippy、变更文件格式、三语站点构建及 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。SQLite 原生快照另随 02-06 验收。

## 迁移与 Schema 生命周期事件

对照固定 Laravel [迁移事件文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#events)、[Migrator 的批次和单项事件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/Migrator.php#L181)、[FreshCommand 的填充前事件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/FreshCommand.php#L105) 和 [DumpCommand 的转储、清理事件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/DumpCommand.php#L55)。旧版未找到对应迁移事件，复用主线应用级 Dispatcher 和已有写库账本。

- 批次事件报告 `MigrationMethod` 及有类型的 `MigrationOptions`，单项事件携带编译登记的迁移实例、方向和名称。按 Laravel 的顺序在迁移成功后、变更记录前触发 `MigrationEnded`。预演触发批次事件，跳过单项事件。主动跳过和没有待处理迁移分别触发 `MigrationSkipped` 和 `NoPendingMigrations`。
- `DatabaseRefreshed` 在 `migrate:fresh`、`migrate:refresh` 完成迁移后且可选 Seeder 开始前触发。上述事件均可通过 `MigrationEvent::NAME` 订阅。转储、删除源文件和加载快照各自使用 `SchemaDumped`、`MigrationsPruned`、`SchemaLoaded`，载荷含连接、名称与路径。
- 临时 MySQL 8.4 上的真实 Artisan 集成测试通过。验证预演、失败中止、重试、跳过、空批次、回滚顺序、单项事件时的真实账本状态、Schema 事件及填充前事件。复跑根应用既有 MySQL 迁移账本。快照子进程调用临时容器内的原生客户端。
- 三语共用监听器示例经实际 Artisan 执行，`main artisan inspire`、严格 Clippy、变更文件格式、三语站点构建和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。日常验收范围为 MySQL，SQLite 快照仍待验收。

## 创建缺失的 MySQL 数据库

对照固定 Laravel [MigrateCommand::handleMissingDatabase 与确认流程](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/MigrateCommand.php#L179) 及 [MySQL 1049](https://dev.mysql.com/doc/mysql-errors/8.4/en/server-error-reference.html#error_er_bad_db_error)。旧版没有等价的命令流程。本批沿用主线的 SQLx 连接错误、写端连接参数和迁移账本。

- `migrate` 只在账本初次检查遇到 MySQL 1049 时尝试建库。交互终端提示确认，按回车使用默认的同意。拒绝时退出。`--no-interaction` 或无终端时保留连接失败，带 `--force` 可执行无人值守迁移。生产确认仍先执行。
- 在原写端点用同一账号与 TLS 设置建立无默认库的短连接，执行引用过名称的 `CREATE DATABASE IF NOT EXISTS`，保留服务器默认字符集和排序规则。成功后复查原账本。不改写共享连接配置。显式读连接和与基准连接配置不同的写库不会被自动创建。
- 用 `python3 -B tests/mysql_acceptance.py` 启动独立 MySQL 8.4.13，实际子进程通过。覆盖无人值守拒绝、强制建库并写账本、显式读连接、权限不足、带连字符的库名以及对同一库重复执行。伪终端验证选择拒绝、生产确认及缺库确认默认同意。原有 MySQL 迁移账本回归通过。
- 三语共用的 `./artisan migrate --database=mysql --force` 与 `./artisan inspire` 在隔离环境实际执行。严格 Clippy、变更文件格式、三语站点构建和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。SQLite 缺失文件的自动创建与其他驱动行为待发布矩阵。

## MySQL migrate:fresh 建库与快照

对照固定 Laravel [FreshCommand 的账本检查、清库和内部 migrate](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/FreshCommand.php#L67)。旧版未找到对应 Artisan 接线。本批复用 MySQL 缺库创建、限定 Schema 清库、快照加载和生产确认。

- `migrate:fresh` 生产确认通过后，遇到 MySQL 1049 可由内部迁移流程按写连接建库。已有库只有在迁移账本存在时才清库。无迁移账本的现有库保留其表，其他连接错误直接返回。
- `--schema-path` 指定输入快照。加载后按账本跳过已记录迁移，续跑编译期清单。公开 Rust Migrator 的 `fresh` 继续沿用原有调用参数。
- 用 `python3 -B tests/mysql_acceptance.py` 在 MySQL 8.4.13 上通过真实 Artisan 缺库与快照回归。覆盖生产拒绝、无账本保留独立表、显式快照恢复、历史保留，`sh tests/table_catalog.sh` 复验限定库清理。
- 三语共用 `./artisan migrate:fresh --database=mysql --schema-path=database/schema/custom.sql`、`./artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版，非 MySQL 的建库和快照待发布矩阵。

## MySQL migrate:refresh 回滚与默认快照

对照固定 Laravel [刷新命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/RefreshCommand.php#L39)和[回滚后重跑文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#roll-back-migrate-using-a-single-command)。旧版未找到对应 Artisan 入口。本批复用当前回滚和默认快照加载路径，公开 Rust `Migrator::refresh` 保留原签名与返回类型。

- `migrate:refresh` 先回滚，后运行迁移。正数 `--step` 限定回滚范围，零和省略都回滚全部。回滚阶段要求能够连接目标库，MySQL 缺库错误直接传播。
- 迁移账本为空时加载该连接的默认快照，Artisan 显示加载提示，并按恢复的账本跳过已有迁移。
- 临时 MySQL 8.4.13 上通过真实二进制账本和快照回归。覆盖缺库失败、默认快照恢复、`--step=1` 保留早期表数据及 `--step=0` 完整重跑。三语 `./artisan migrate:refresh --database=mysql --step=1`、`./artisan inspire`、严格 Clippy、变更文件格式、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。
