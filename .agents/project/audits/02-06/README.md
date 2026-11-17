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

## 待迁移检查与退出码

- 对照固定 Laravel `StatusCommand` 的 `--pending[=code]`、缺失迁移账本、条件跳过与空列表处理。Rust 用能直接传入进程退出码的 `u8` 表达用户指定值。沿用当前迁移账本及编译清单，公开 `Migrator::status` 继续查询账本。旧版未找到可复用的同名 Artisan 命令。 [固定 Laravel StatusCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/StatusCommand.php) · [Laravel Running Migrations](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#running-migrations)
- 基于 `4953d4f77` 隔离验收。真实 MySQL 8.4.13 上，根应用命令验收无账本退出 1、只筛选待迁移项以及指定退出码。用三语共用示例的独立 Artisan 运行确认，迁移前退出 17，迁移后显示 `No pending migrations` 并退出 0。
- 框架控制台验收已执行路径筛选、`Skipped`／`Pending`／`Ran` 的转换、已执行项显示批次、仅跳过项时退出 0。首个待迁移项之后保持 `Pending`，待前序迁移完成后再求值条件。
- `sh tests/table_catalog.sh` 的表目录及限定库清理、实际 `main artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点与 18 项浏览器检查通过。数据库创建、快照和 Redis 隔离的历史验收仍各自追踪，本批按待迁移检查的范围实测。临时 MySQL、本批 3.5 GiB Cargo 产物和文档缓存已回收。磁盘可用 63.25 GB（58.91 GiB），周额度剩余 48%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-06-status.hRPakDvy`。

## 迁移失败警告与继续退出

- 对照固定 Laravel `MigrateCommand::handle`。生产确认仍先执行，`--graceful` 只改变迁移或 Seeder 执行错误的报告和退出码。Rust 在执行分支内处理 `Result`，复用原迁移与账本流程。旧版有迁移失败回归，没有对应的 Artisan 选项。 [固定 Laravel MigrateCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/MigrateCommand.php)
- 以 `fb7bba359` 为隔离基线。在真实 MySQL 8.4.13 上，先执行用户表迁移，再制造缓存表冲突。普通模式退出 1，`--graceful` 显示失败迁移名并退出 0，账本只保留已成功的一项，后续任务表尚未创建。删除冲突表后重跑，待执行列表为空。
- 根应用的独立 Artisan 回归还核对未知 Seeder 的普通失败与警告模式，以及拒绝生产确认时仍退出 1。三语共用的 `./artisan migrate --graceful` 行为通过独立二进制验证。
- Rust 1.94.0 全仓格式、严格 Clippy、实际 `main artisan inspire`、三语站点与 18 项浏览器检查通过。临时 MySQL、本批 2.8 GiB Cargo 产物及文档缓存已回收。磁盘可用 62.68 GB（58.37 GiB），周额度剩余 47%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-06-graceful.I7KI0i3E`。

## 按名称排除待执行迁移

- 对照固定 Laravel `Migrator::withoutMigrations`、`pendingMigrations` 与 `StatusCommand`。Rust 在应用容器的 Migrator 实例保存名单，在选出本次待执行迁移时一次读取。接受迁移名和 `.rs` 路径，重复调用替换名单。实际执行和预演采用同一筛选，状态和回滚沿用账本。旧版未找到同名公开入口。 [固定 Laravel Migrator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/Migrator.php) · [Laravel Running Migrations](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#running-migrations)
- 隔离基线 `cc9667837` 的 MySQL 8.4.13 框架控制台回归已验收预演、路径筛选、实际执行、两个应用的名单隔离、清空后补跑及回滚。状态仍显示尚未登记的排除项为 `Pending`。
- 同一 MySQL 环境通过迁移状态和根应用账本回归。将三语共用示例编入独立应用，从服务提供者的 `boot` 调用，实测 Artisan 只预演和执行第二条迁移。首轮示例编译发现 `ProviderResult` 不接收类型参数，三语修正后重新编译并执行通过。
- Rust 1.94.0 全仓格式、严格 Clippy、实际 `main artisan inspire`、三语站点及 18 项浏览器检查通过。临时 MySQL、本批 3.6 GiB Cargo 产物及文档缓存已回收。磁盘可用 63.30 GB（58.95 GiB），周额度剩余 47%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-06-exclusions.UonJZpW3`。

## 默认回滚与迁移账本读取

- 对照固定 Laravel `DatabaseMigrationRepository::getLast`、`getRan` 与 `getMigrationBatches`。旧版先取迁移名及批次，再在内存中筛选。当前默认回滚在写库同一条查询中用 `MAX(batch)` 子查询限定记录，仍按迁移名逆序。迁移检查仅查询迁移名，状态批次查询仅选择迁移名和批次。 [固定 Laravel 迁移账本](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/DatabaseMigrationRepository.php) · [Laravel 回滚章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#rolling-back-migrations)
- 在 `da656c9b7` 隔离基线的 MySQL 8.4.13 上，普通读连接因指定库不存在而返回错误。账本最新批次、已执行名及批次列表都成功，查询日志确认三次查询均走写连接。空账本返回空列表，多个批次排序正确，最新批次仅传输相应记录。
- 框架的迁移状态回归通过。实际根应用二进制执行 `migrate:rollback --pretend` 和默认 `migrate:rollback`，预演只列最新批次，实际撤销只移除最新批次的表和账本记录。早期用户表及其迁移记录保留，重新迁移成功。
- Rust 1.94.0 全仓格式、严格 Clippy、`main artisan inspire`、三语站点及 18 项浏览器检查通过。临时 MySQL、本批 3.3 GiB Cargo 产物及文档缓存已回收。磁盘可用 63.27 GB（58.93 GiB），周额度剩余 47%，第二窗口无读数。证据目录 `/private/tmp/openrust-02-06-ledger-query.jdYdibZs`。

## 创建缺失的 PostgreSQL 数据库

- 对照 Laravel v13.40.0 的 `MigrateCommand` 和 `FreshCommand`，在迁移账本首次检查确认目标数据库缺失后，沿用 Artisan 对建库的确认规则。核对旧版 `6d1a81924` 及其数据库测试，未找到等价的 PostgreSQL 缺库创建流程，复用当前 MySQL 的确认、建库与账本重查接线。 [固定 Laravel MigrateCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/MigrateCommand.php#L179) · [固定 Laravel FreshCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/FreshCommand.php#L67)
- SQLx 提供 PostgreSQL 原生 SQLSTATE，因此使用 `3D000` 识别缺库。复制选定写连接的端点、认证和 TLS 配置，通过短连接进入 `postgres` 管理库，执行已转义的 `CREATE DATABASE`。成功后在原连接重查迁移账本，共享配置保持原值。只读别名或覆盖了基准库名的端点不会触发自动建库。 [PostgreSQL 错误码](https://www.postgresql.org/docs/current/errcodes-appendix.html)
- 在隔离基线 `b249e3e7b` 上用 PostgreSQL 协议夹具验证 SQLx 对 `3D000` 的分类及认证、权限和连接错误的排除。同时验证直连端点、TLS 配置、库名转义及建库资格。实际 PostgreSQL 服务器的管理库连接、权限和建库后的迁移账本留待发布验收。
- 真实 MySQL 8.4.13 用独立 Artisan 复验无交互拒绝、强制建库、读连接拒绝、权限不足、重复迁移、`migrate:fresh` 建库和已有表保留。根应用 `./artisan inspire`、全仓格式、严格 Clippy、三语站点及 18 项浏览器检查通过。已回收本批临时 MySQL、3.4 GiB Cargo 产物及文档缓存。证据目录 `/private/tmp/openrust-02-06-postgres-create.lOKJraPT`。

## PostgreSQL Schema 转储与恢复

- 对照固定 Laravel v13.40.0 的 `PostgresSchemaState` 和压缩迁移章节。检查旧版 `6d1a81924`，未找到 `pg_dump` 或 `schema:dump` 接线。复用主线的迁移账本、客户端退出码检查及临时文件发布。 [固定 Laravel SchemaState](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/PostgresSchemaState.php) · [固定 Laravel 压缩迁移](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#squashing-migrations)
- PostgreSQL 用 `pg_dump` 保存 SQL 定义，并通过写连接解析迁移账本所在 Schema，仅追加该表数据。SQL 用 `psql` 加载，归档文件用 `pg_restore`。恢复客户端启用单事务及错误退出。配置直连时，命令使用其端点、用户及 TLS 设置，默认文件名沿用基准连接名，便于跨平台使用。 [psql 选项](https://www.postgresql.org/docs/current/app-psql.html) · [pg_restore 选项](https://www.postgresql.org/docs/current/app-pgrestore.html)
- 隔离基线 `1b061e6c7` 的无网络回归验证客户端选取、连接串转义、TLS 参数和恢复错误退出参数。真实 PostgreSQL 转储、恢复和账本验证留待发布矩阵。已存在账本的 PostgreSQL 库仍缺 `migrate:fresh` 所需的清库能力，单独跟踪。
- 真实 MySQL 8.4.13 通过独立 Artisan 转储、恢复、无账本数据选项、无业务数据、恢复错误、裁剪及迁移账本验证。根应用 `./artisan inspire`、全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。已回收临时 MySQL、3.4 GiB 本批 Cargo 产物及文档缓存。证据目录 `/private/tmp/openrust-02-06-postgres-schema.rkT6ygzi`。

## PostgreSQL 限定 Schema 清库

- 对照固定 Laravel v13.40.0 的 `PostgresBuilder`、`WipeCommand` 和 Grammar 断言。核对旧版 `6d1a81924`，未找到同等的 Schema 清库入口。本批复用现有事务、写连接、迁移账本检查和 Artisan 生产确认。 [固定 Laravel PostgresBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/PostgresBuilder.php) · [固定 Laravel WipeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/WipeCommand.php)
- PostgreSQL 在同一个写连接事务中枚举和删除对象。查询基于服务器实际生效的 `search_path`，排除系统 Schema。表名支持 `dont_drop` 物理名与限定名，默认保留 `spatial_ref_sys`。清理依 Laravel 顺序执行可选视图、表、可选用户类型和域。目录名称逐段转义，批量 DDL 使用 `CASCADE`，依赖影响可跨 Schema。成功后 Artisan 迁移清理退役原连接池。
- `migrate:fresh` 保留已有账本才清库的判定。Schema 提供 `dropAllTables()`、`dropAllViews()` 和 `dropAllTypes()`。隔离基线 `a3d175ca6` 的无网络测试验证排除、限定名、带引号名称和空目标。真实 PostgreSQL 的对象枚举、级联与快照重建留待发布验收。
- 真实 MySQL 8.4.13 通过表目录、限定 Schema 清库和 Artisan 快照转储恢复三项回归。根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点与 18 项浏览器检查通过。已回收临时 MySQL、3.6 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-02-06-postgres-wipe.yEPquOZh`。

## 真实 PostgreSQL 缺库、快照与限定 Schema 清库

- 沿用固定 Laravel 的迁移连接选择、缺库提示、快照加载与清库机制。真实 PostgreSQL 18.6 上，`migrate --no-interaction` 因目标库不存在返回 1，保留原状。加入 `--force` 创建 PostgreSQL 数据库并运行根应用迁移。普通迁移的账本仅写直连端点。
- `schema:dump --database=pgsql` 使用真实 `pg_dump`，快照包含表结构和迁移账本。业务行未写入快照。独立 Artisan 通过真实 `psql` 恢复 SQL，真实 `pg_restore` 恢复归档，无效 SQL 让恢复事务回滚。测试包装器先核对客户端收到直连端口，再映射到同一个隔离容器的本地端口。
- `migrate:fresh` 重建目标 Schema，保留相邻 Schema 的哨兵行。`db:wipe --drop-views --drop-types` 清空指定搜索路径中的表、视图、类型和域，保留相邻 Schema 的数据与其他 Schema 的迁移账本。其他驱动快照和 PostgreSQL 版本矩阵继续跟踪。 [Laravel 快照](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/PostgresSchemaState.php) · [Laravel 清库](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/WipeCommand.php)
- 验收脚本、日志和快照保留在 `/private/tmp/openrust-02-05-postgres-types.ZYsFwGbc`。

## 整条迁移的事务与显式退出

- 对照固定 Laravel v13.40.0，PostgreSQL 与 SQL Server 默认将一条迁移的 `MigrationStarted`、`up`／`down` 和 `MigrationEnded` 放在一个事务中，成功返回后再更新账本。复核旧版 `6d1a81924` 的 `migration_uses_transaction`、应用和回滚实现，沿用 PostgreSQL 默认事务和逐条回滚的语义，账本的写入时序按固定 Laravel。 [固定 Laravel Migrator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/Migrator.php#L390)
- Rust 的 `MigrationContext` 显式持有选定连接的事务，Schema 和 SQL 都可通过该上下文执行，同步监听器通过 `EventContext::transaction()` 加入。迁移可用 `withinTransaction()` 返回 `false`，允许 PostgreSQL 在线索引等自动提交语句。预演不启动迁移事务。直接新取连接的操作不参与该事务。
- PostgreSQL 18.4 集成测试通过。实际二进制的独立 Artisan 覆盖预演、DDL 和 DML 同时回滚、结束监听器失败回滚、失败后重跑、`down` 失败保留数据与账本、默认事务拒绝在线索引和显式退出后创建有效索引。HTTP 实测读取已提交的迁移写入和监听器写入，之后 Artisan 回滚成功。SQL Server 的真实驱动验收、其他版本矩阵和驱动快照继续跟踪，核心账本维持 62/66。
- MySQL 8.4.13 迁移仓储和 Artisan 状态三项、`sh tests/table_catalog.sh` 两项、根应用 `./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy、三语站点及 18 项浏览器检查通过。本批容器和隔离产物已回收，磁盘空间净增 3.88 GB（3.62 GiB），可用 105.11 GB（97.89 GiB）。提交前周额度剩余 43%，第二窗口无读数。配置脚本及日志保留于 `/private/tmp/openrust-02-06-migration-transactions.EMrJgz6P`。

## MariaDB 原生 Schema 快照

- 对照固定 Laravel v13.40.0 的 `MariaDbSchemaState`。按选定写连接调用 `mariadb-dump` 和 `mariadb`，复用现有快照原子替换、结构和账本分批导出及自增计数清理。MariaDB 客户端不接收 Oracle MySQL 特有的 GTID 和统计参数。复核旧版 `6d1a81924`，未找到对应的原生快照命令。 [固定 Laravel MariaDbSchemaState](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/MariaDbSchemaState.php)
- Rust 继续要求加密。MariaDB 11.8.9 双客户端实测仅传 `--ssl` 可退回明文，显式启用服务器验证时拒绝无 TLS 的服务端。因此快照使用 11.4 或更新的客户端，支持默认配置下的 MariaDB 自动验证，以及指定服务器 CA 和主机名的 `verify_identity`。客户端没有等价的仅 CA 模式，快照拒绝 `verify_ca` 并给出配置错误。 [MariaDB 自动 TLS 验证](https://mariadb.com/docs/server/security/encryption/data-in-transit-encryption/zero-configuration-ssl)
- 真实 MariaDB 11.8.9 分别用默认 TLS 和配置 CA 验收实际 Artisan。覆盖快照结构、账本、自增计数、业务行排除、视图、例程、触发器、`--without-migration-data`、`--prune`、无效 SQL 失败和自定义路径下的 `migrate:fresh`。`migrate:refresh` 复验默认快照恢复。实际 HTTP `GET /up` 和独立 `artisan inspire` 均通过。MySQL 8.4.13 原生快照回归通过。
- Rust 1.94.0 全仓格式和严格 Clippy、三语站点构建及 18 项浏览器检查通过。临时实例、证书和本批隔离产物已回收，磁盘空间净增 2.89 GB（2.69 GiB），可用 105.07 GB（97.86 GiB）。提交前周额度剩余 43%，第二窗口无读数。日志保留于 `/private/tmp/openrust-02-06-mariadb-snapshot.as80uhK5`。

## SQLite Schema 快照

- 对照固定 Laravel v13.40.0 的 `SqliteSchemaState`。快照保存表、视图、索引、触发器、虚拟表和迁移账本，排除业务行、SQLite 内部表和虚拟表影子表。复核旧版 `6d1a81924`，未找到对应的 SQLite 快照接口。 [固定 Laravel SqliteSchemaState](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/SqliteSchemaState.php)
- Rust 直接在应用的 SQLite 写连接上从 `sqlite_schema` 与 `pragma_table_list` 导出，避免依赖额外的 SQLite 命令行工具，也能在原连接上恢复内存库。SQLite 恢复使用框架事务，迁移前的空账本和快照 DDL 共用该事务。导出沿用临时文件原子替换，并拒绝将输出指向数据库文件及其运行时附属文件。
- 实际 Artisan 验证文件库、内存库、自定义路径、`migrate:fresh`、`--without-migration-data` 和损坏 SQL 回滚。验证 RTree 结构恢复且影子表由 SQLite 重新建立，索引、视图、触发器和历史账本保留，业务行排除，自增计数重新开始。通过实际 HTTP `GET /up` 和独立 `./artisan inspire`。MySQL 8.4.13 快照端到端回归通过。
- Rust 1.94.0 全仓格式、严格 Clippy、三语站点构建及 18 项浏览器检查通过。本批容器与编译、文档产物已回收，磁盘空间净增 3.40 GB（3.16 GiB），可用 105.04 GB（97.83 GiB）。提交前周额度剩余 43%，第二窗口无读数。验收日志与隔离 SQLite 样本保留于 `/private/tmp/openrust-02-06-sqlite-snapshot.cVpDUNZ1`。
- 下一批按固定 Laravel 补 SQLite 缺库的创建确认。SQL Server 事务与发布版本矩阵继续跟踪。

## SQLite 缺库创建确认

- 对照固定 Laravel v13.40.0 的 `MigrateCommand::createMissingSqliteDatabase`。当 SQLite 文件缺失时，`migrate` 在交互式终端询问并默认同意。非交互运行须提供 `--force`，否则保留缺失状态。之前的 `migrate:fresh` 调用继续复用已有确认与创建授权。复核旧版 `6d1a81924` 的 SQLite 文件解析及普通连接的禁止隐式创建设置，Rust 延续普通连接独立于迁移创建授权的行为。 [固定 Laravel MigrateCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Migrations/MigrateCommand.php#L187)
- Rust 只处理缺失的 SQLite 写库路径。保留只读与 immutable URL、缺失父目录、目录和现有损坏文件的错误。使用系统原子独占创建，竞争者先创建的常规文件被保留，已有内容不会被截断。
- 实际 Artisan 通过非交互拒绝和强制建库、带空格的相对路径及 URL 路径、`migrate:fresh`、只读和 immutable 拒绝、已有损坏文件保留以及快照回归。真实伪终端拒绝后文件不存在，直接回车同意后创建文件并写入迁移账本。使用新库的实际 HTTP `GET /up` 返回 200，独立 `artisan inspire` 正常退出。MySQL 8.4.13 缺库创建及无建库权限的拒绝通过。
- SQLite 原子建库竞争回归和 PostgreSQL 启动协议回归通过。Rust 1.94.0 全仓格式、严格 Clippy、三语站点构建及 18 项浏览器检查通过。本批临时 MySQL、编译和文档产物已回收，磁盘空间净增 3.47 GB（3.23 GiB），可用 104.98 GB（97.77 GiB）。提交前周额度剩余 43%，第二窗口无读数。日志与隔离 SQLite 样本保留于 `/private/tmp/openrust-02-06-sqlite-create.4MhjNckb`。
- SQL Server 迁移事务和发布版本矩阵继续跟踪。下一批回到 02-02，补 PgBouncer 在关闭预备语句跟踪时的事务池模式。

## 真实 SQL Server 迁移失败回滚

- 复用固定 Laravel 迁移事务与同一连接写入语义。共享 PostgreSQL 事务测试中的失败夹具，SQL Server 用普通整数列写入，避免 IDENTITY 专有规则干扰事务断言。真实 SQL Server 2022 Developer 16.0.4325.4 验证事务内 DDL、DML 和迁移账本，并复跑 PostgreSQL 18.6 同组测试。 [固定 Laravel SQL Server Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SqlServerGrammar.php#L16) · [固定 Laravel Migrator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Migrations/Migrator.php#L390)
- 真实二进制的独立 Artisan 预演和执行均通过。实际 `up` 在在线索引与外键 DDL 完成后触发 SQL 错误，命令退出 1，表和账本均未留下记录。成功重跑后，`down` 在删表后触发错误，命令退出 1，表、索引和账本均恢复。再次回滚成功，HTTP 确认目录变化，`artisan inspire` 通过。测试也验证显式退出事务时已写入的表和数据继续存在，失败迁移不写入账本。
- 核心账本维持 62/66。临时容器、证书及隔离编译和文档产物已回收，项目卷可用空间净增 3.95 GB（3.68 GiB），剩余 102.33 GB（95.30 GiB）。提交前周额度剩余 42%。原生 Linux x86_64 和其他支持版本仍待发布复验。日志保留于 `/private/tmp/openrust-02-05-sqlsrv.1X6pMKI8`。

## 禁止破坏性命令与整单元验收

- 固定 Laravel 的 `DB::prohibitDestructiveCommands` 控制 `db:wipe`、`migrate:fresh`、`migrate:refresh`、`migrate:reset` 和 `migrate:rollback`。状态默认关闭。启用后先于确认和数据库连接给出上游诊断并退出 1，`--force` 只跳过生产确认。保留普通迁移和状态命令。旧版 `openrust-v0.x` 的迁移入口未找到对应阻止机制。复用当前数据库 Facade 与 `queue:clear` 已有的应用级状态思路，由应用的数据库管理器保存策略。 [固定 Laravel DB Facade](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades/DB.php#L134) · [固定 Laravel Prohibitable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Prohibitable.php)
- 隔离基线 `8d8e13e18` 用实际 MySQL 8.4.13 编译运行三语共享 Provider 示例。生产环境先验证无交互拒绝普通迁移、空库预演仅建立空账本、实际迁移和重复执行。五个破坏性命令带 `--force` 均退出 1，原账本与标记数据保留。禁用时传入未配置连接也得到禁用诊断。切到本地环境后实际 `migrate:rollback --force` 清空批次记录，标记数据保留。`main artisan inspire` 通过。同进程两个应用通过 SQLite 控制台验证策略隔离。
- 真实 MySQL 环境下，根应用 Artisan 账本回归一项和框架迁移回归 6 项通过。Rust 1.94.0 全仓格式与严格 Clippy、三语站点构建、两种部署路径下 12 项通用和 6 项章节浏览器检查通过。日志保留于 `/private/tmp/openrust-02-06-prohibit.MBG1V0Iv`。
- 对照当前计划的核心验收条件。空库迁移、状态、回滚、预演与生产确认见[首批实测](#验收)及本批。编译期清单、隔离执行和失败重试见[自动发现](#编译时自动发现复验)、[跨进程隔离](#跨进程迁移隔离)和[失败警告与继续退出](#迁移失败警告与继续退出)。默认回滚选取最新批次、失败账本与连接事务见[默认回滚](#默认回滚与迁移账本读取)、[整条迁移事务](#整条迁移的事务与显式退出)和[真实 SQL Server 回滚](#真实-sql-server-迁移失败回滚)。Seeder 选项及事件顺序已经接通，独立填充见 [02-07](../02-07/README.md) 和[数据库刷新事件](#迁移与-schema-生命周期事件)。
- 02-06 核心范围闭合。SQLite、MySQL、MariaDB、PostgreSQL 已有原生快照验收，完整数据库版本及 SQL Server 原生 Linux x86_64 继续由[发布验收](../database-dialects.md#发布验收)跟踪。项目卷净增 7.87 GB（7.33 GiB），剩余 100.88 GB（93.96 GiB）。周额度剩余 39%，第二窗口无读数。
