# `openrust::database`

> 受众：AI Agent
> 更新：2026-11-03
> 摘要：M2 数据层当前实现边界。

## 插入标识返回限定交付

`QueryBuilder::insert_get_id` 的同次执行结果、默认空行插入、sequence、回调次数与失败、主库/事务及原始标识转换已由 `960aca43` 提交并推送。PostgreSQL RETURNING 保持精确文本，SQLite 负 rowid 经保留完整来源的最小 SeaORM 安全访问器读取；Query Processor 的数值转换与 Model 属性 cast 是不同契约。完整 Eloquent 与 SQLite 其他能力不因此完成。精确组合及干净提交树证据见[验收基线](../memory/testing.md#插入标识返回与事务执行限定验收)。

## SQLite 连接基础限定范围

`db-sqlite` 提供标准Provider的文件/URI/内存连接、同仓储application_root及逐物理PRAGMA，默认MySQL/Redis不变。PDO初始60000ms、外键默认true，未显式配置时不强制WAL/NORMAL；普通文件必须存在。内存池限制/禁回收、URI单次解码、named隔离及owned TestDatabase显式close已实际验证；唯一模块reporting路由不启用默认库业务。SQLite-only导出与类型推断已修，严格单driver Clippy、fresh11和文档通过；主10门禁、真实MySQL3、根进程HTTP及影响复用见[限定验收](../memory/testing.md#sqlite-连接基础限定验收)。本地提交以Git为准。

SQLite配置与迁移的当前限定证据见[验收](../memory/testing.md#sqlite-配置与迁移限定验收)：nullable/raw PRAGMA、原生默认、内存所有者、文件跨进程锁、既有FrameworkMigrator/example清单、main普通表删列重建、生成安装和真实宿主已验。3.26.0与3.51.3分别计证；完整Migrator/Schema、Auth/Session/Queue业务SQL、构造取消/Drop全故障与原子禁重连仍未完成。仅SQLite代码启用物理handle collation析构见证，避免把并发借走误判为归还失败；名称碰撞拒绝、同handle有界复用，故障失效不证明池内其他连接失效。

## 列边界与布尔条件链

固定Laravel d52c3b3的四种between-columns入口已接线，QueryBuilder与WhereGroup共用ColumnOperand和SQL AND/OR条件链；原首OR空条件和后续AND优先级同步纠正。标量子查询、嵌套组与事务重绑保留来源约束。10项新SQLite回归及13项查询家族、完整12项组合门禁、MySQL/PG各2项唯一示例真实HTTP、fresh SQLite JWT应用27项及外部query-authorization锁定检查已通过；范围与失败修复见[限定验收](../memory/testing.md#query-列边界与布尔条件链限定验收)。公开contract见[查询指南](../../../docs/src/database/queries.md)，状态只在todo维护。

## 存在性终结操作

固定Laravel exists/doesntExist与compileExists的借用查询API已接线，保留完整查询、绑定、NULL投影、分页、UNION、读副本和显式事务。SQLite6案、main12门禁、真实MySQL/PG各1核心与2HTTP、生成应用27项及公开consumer已通过，见[限定验收](../memory/testing.md#query-存在性终结操作限定验收)。唯一模块时间线按同一逻辑表的open记录返回账户内在途标记。回调变体、before-query和完整Query/ORM仍开放。

## 存在性条件回调

固定existsOr/doesntExistOr的两个懒执行分支已接线，结果enum保留true和任意T，FnOnce异步闭包只在选中分支创建一次；唯一模块与指南同步。8项SQLite回归、完整12门禁、MySQL/PG各2项HTTP、fresh27项及外部consumer通过，详见[限定验收](../memory/testing.md#query-存在性条件回调限定验收)。&self允许回调只读借用同query。before-query管理/共享子节点已进入下述未提交WIP，完整Query/ORM继续开放。

## 查询前回调与共享子查询

单一QueryState、双生命周期、父Clone独立/child共享、冻结绑定、图环/来源拒绝和跨I/O租约已限定验收；回调、投影、分页/chunk按固定官方时机执行，唯一时间线与Auth事务调用方同步。main12门禁、真实双库各42公开契约/2HTTP、fresh27及consumer通过，详见[限定证据](../memory/testing.md#query-查询前回调与共享子查询限定验收)。服务器确认行锁后的取消恢复已验；MySQL冷prepare取消的单连接淘汰/替换与锁释放已另行[限定验收](../memory/testing.md#mysql-冷预处理取消与连接淘汰限定验收)，不承诺任意阶段同连接恢复；完整Query/ORM仍开放，下一步只在todo维护。

## 插入标识与事务执行

DB-I01限定实现同次执行返回ID、默认行/sequence、PHP64 Processor数值规则、before-query/来源租约/主库与原事务；PG同RETURNING原值+文本副本保精度，SQLite负rowid通过固定SeaORM安全访问器保留。唯一示例事务内写后读取、CLI生成公开回归及独立patch图同步。适用验收与分发边界见[testing](../memory/testing.md#插入标识与事务执行闭环)；不关闭完整Query、Model或Passport，普通模型getKey转换与Query Processor不得混用。

## 模型属性与认证前置（限定验收）

MB-A01 已由 `06b4d50c6c89001a2db0adee2ff54f13d78bffb8` 限定提交并核远端：显式元数据/连接/身份、原始行水合、getter/cast/异步setter、拥有式多字段回执及认证Cookie committed读取。根/模板User与唯一示例复用公开属性管线；普通setter保护同字段后继，回执处理祖先取消、ABA与资源回收。

完整适用主线门禁、真实双库原始行与认证、实际SQLite3.26、三后端生成认证应用及适用consumer已通过。JSON使用局部RawValue保持数字词法，不启用全局arbitrary_precision；PHP8.5.12对照及真实HTTP getter见证通过。范围和输入绑定见[验收](../memory/testing.md#模型属性与认证前置限定验收)。

完整Eloquent save/dirty/关系、日期及完整值载体、PG会话显示仍开放；内存回滚不保证未知DB提交或跨连接持久顺序。公开边界见[模型基础](../../../docs/src/eloquent/getting-started.md)。


## SQLite 同机 SharedLocks

SQLite方言已由 `5c26fd04` 限定提交推送：数据库时钟、owner/fence/期限、续约、同一事务写保护及真实导入生产边界保留；最低3.26与原本MySQL/PG协调回归均有绑定输入证据。当前只支持同机文件租约，不代表完整CacheLock/Queue/幂等。详细范围、native构建诊断与纯提交树闭合见[验收](../memory/testing.md#sqlite-同机租约方言限定验收)。

## 当前实现

- C07-W01 基本比较已编码、待独立静态审查及运行验收：`query::{WhereOperator, WhereValue}`、`r#where`／`where_compare` 与 OR／条件组入口，原便捷比较复用内核；NULL／JSON 路径、双编译绑定及子查询来源保留方言约束。唯一示例时间线改用该公开入口，生成测试与文档同步；Closure、多列条件与完整 Query 未关闭。

- 通用能力门控已通过[限定验收](../memory/testing.md#db-通用能力与驱动分层限定验收)：db及facade/prelude常开、迁移总注册，has_default_connection区分default与named；无DB/queue-only、真MySQL/PG、SQLite扩展与fresh12/3/2、资源收尾通过。当前双库事实不限制最终固定官方五后端范围；完整Laravel懒连接/默认名切换与扩展resolver仍开放。

- `Db::table` 的限定 Query Builder 默认选择 `*`，显式 `select` 替换默认列；真实 MySQL 文章分页已验证计数、行数据、空第二页与所有者隔离。完整 C07、ORM、CursorPaginator 和全后端矩阵仍开放；本批证据见 [testing](../memory/testing.md#当前-main-组合限定验收)。

- `DatabaseConfig` 定义在 `openrust::config::database`，旧的 `openrust::database::DatabaseConfig` 导出保持可用。默认可编辑配置位于应用根 `config/*.rs`，TOML仅显式可选来源；本地 URL 可用 `.env`，生产环境使用进程变量。

- `DatabaseConfig` 从 `[database]` 读取 URL、池大小和连接/获取超时；自定义 `Debug` 会遮蔽 URL。
- `DatabaseBackend` 集中解析官方 PostgreSQL/MySQL URL scheme 与 feature 状态；它当前描述 PostgreSQL/MySQL/SQLite 连接集合，固定官方五后端仍保留交付缺口，扩展插件可用 `Db::backend()` 读取 SeaORM 实际 backend。SQLite已按上述限定范围内置；未编译db-sqlite的外部consumer仍可使用通用连接/事务/查询，框架迁移明确返回database_feature_disabled后由其自有Migrator执行。
- `DatabaseServiceProvider` 在 register 登记当前配置的异步 singleton 工厂；首次解析按最终可见配置连接 SeaORM pool，每个成功连接立即登记受管关闭，之后的副本、具名库或版本检查失败会回收前池。boot 解析并验证；最终校验涵盖 pool 参数、完整 URL 语法和 backend feature。无主库 URL 时提供明确未初始化的可选句柄，配置副本却无主库则拒绝。当前运行时版本门禁仍为PostgreSQL14/MySQL8，属于待整改事实，不能当作最终支持基线。Laravel兼容目标为MariaDB 10.3+、MySQL 5.7+、PostgreSQL 10.0+、SQLite 3.26.0+、SQL Server 2017+；日常实际数据库仅MySQL8.4，发布前统一完整验证多库及最低/当前版本。
- 启用 `HttpServiceProvider` 时，`DatabaseServiceProvider` 为每个数据库实例注册 runtime readiness 探针，使用只读 `ping` 检查主库与所有配置只读副本。依赖故障会使 `/ready` 返回 503；连接池恢复后下一轮探测可重新转为 ready，不暴露连接错误或 URL。
- 命名数据库按 历史决策 045 实现：既有 `[database]` 和 `Db` 是兼容默认实例；`[database.instances.<name>]` 为独立完整连接配置，由应用级 `Databases` 选择。实例各自持有连接池、只读副本与表名 resolver，未知名称启动/选择时显式失败。认证、Session、Queue、Audit 和 RateLimiter 仍绑定 default。
- `DatabaseServiceProvider` 注册 `openrust::core::RateLimiter`，其官方 `DatabaseRateLimitStore` 通过当前 `Db` 实例在 PostgreSQL/MySQL 上原子更新固定窗口记录；`DatabaseRateLimitMigration` 由应用 migrator 显式注册，启动不会自动建表。
- 限流 key 仅存 SHA-256 digest（不抵抗低熵输入猜测）；每个 store 实例每 256 次消费最多清理 512 行已过期记录，窗口长度一并持久化。缺少迁移、数据库故障与不支持 backend 均以错误传播，不作为未命中或放行。
- 自定义 `ServiceProvider` 可在 register 登记异步 `Db` 工厂，在工厂成功连接后立即登记受管关闭，再调用 `Db::initialize()`；`initialize_with_replicas()` 接受同 backend 的主库和副本连接；`Db::backend()` 读取主库 backend。标准 `DatabaseServiceProvider` 可按 `[database.instances.<name>]` 建立多个具名连接；自定义 Provider 负责连接配置与能力校验。当前通用初始化与 `DatabaseTableNames` 支持 PostgreSQL、MySQL、SQLite；其他 SeaORM backend 在定义表名 quoting 前会明确拒绝。
- MySQL pool 的每条新连接将会话时区设为 `+00:00`，与 历史决策 002 一致。
- `Db` 是可克隆服务句柄；`connection()` / `primary_connection()` 始终返回主库，`read_connection()` 显式轮询只读副本（未配置副本时返回主库）。`with_primary(future)` 让当前 Tokio task scope 内的读入口固定主库；独立 `tokio::spawn` 不继承 scope。插件完成初始化前调用会返回内部错误。
- 读写分离使用显式 API，不解析 SQL；调用方须保证副本入口只执行可接受复制延迟的只读操作。官方插件连接并校验所有配置副本，任一失败即启动失败；不自动重试或回退。每个 pool 使用相同 pool 上限，因此副本数为 N 时总连接上限为 `(N + 1) * max_connections`。
- `Db::transaction()` 将闭包放在 SeaORM 事务中执行；回调成功则派发可选 `TransactionBeforeCommit` 并提交。回调或提交前监听器返回业务错误时，回滚成功后原样保留其稳定错误码与 HTTP 状态；数据库、回滚和提交失败映射为框架内部错误，不把回滚失败误报为已回滚。标准 `DatabaseServiceProvider` 同时发现 `EventsServiceProvider` 时才绑定生命周期事件；自定义数据库 plugin 不自动接入。真实双库业务错误与回滚边界见 [R1-E testing](../memory/testing.md#r1-e-业务示例定向验收证据整簇进行中)。
- 按 历史决策 048，`BatchReader<E>` 复用 SeaORM keyset cursor，以实体所有主键列组成升序/降序游标；每次最多物化 1–1000 行，默认 100。通过 `Db::primary_connection()` 执行，不使用只读副本；`AuthorizedSelect::into_batches()` 保留每页授权范围。取消在批次间检查，报告游标只推进完整成功 callback；callback 失败立即返回。没有隐式跨批事务或快照，副作用重试与一致快照由调用方负责。此限制按行数而非字节数计算，输入 Select 不应已有分页 offset/limit。
- 成功提交后只通知 `TransactionCommitted` 的 direct listeners。其错误仅记录稳定错误码，返回值仍表示数据库已提交；queued listeners 不在此边界执行。需可靠持久派发的副作用须在提交前事件中通过事务队列落库。直接 SeaORM/raw SQL 不经过此 API，不触发事务事件。
- `CoordinationMigration` 需由应用 migrator 显式注册；`SharedLocks` 基于数据库 UTC 时钟提供跨实例 owner token、TTL、单调 fencing token、条件续约/释放及事务内 `assert_current_in_transaction`。过期可接管，但 TTL 不会终止暂停的旧进程或 fence 任意外部副作用。`RequestIdempotency` 由应用显式传入 scope/key/规范化 fingerprint；同键不同 fingerprint 返回冲突，活跃 owner 返回处理中，完成后可重放不超过 64 KiB 的 JSON body。业务写入与 `complete_in_transaction` 必须同事务；lease 到期可恢复，完成记录用有界 `prune_expired(1..=1000)` 清理。表名使用 Db resolver，SHA-256 摘要和 256-bit CSPRNG token 不写日志。真实 PG18/MySQL8.4 双池竞争、旧 owner 防护、scope/冲突/事务回滚/回放/有界清理已通过。
- `DatabaseConfig` 支持 `table_prefix`（默认空值，显式值启用）和 `table_names` 完整物理名映射；`Db` 持有不可变 `DatabaseTableNames`，提供物理名与 backend quoting。标识符只允许 ASCII 字母/数字/下划线；连接初始化时按 PostgreSQL 63-byte、MySQL 64-byte 限制校验映射和框架核心表名。
- `MigrationServiceProvider` 在每次 up/down/status 调用中用 Tokio task-local scope 提供 `DatabaseTableNames`；OpenRust AuthMigration 无 scope 时失败。手写 SeaORM migration 可用 `DatabaseTableNames::current()` 获取当前 resolver。`TestDatabase::create_with_table_names()` 用相同 scope 执行隔离迁移。
- 认证迁移、Session/PAT/refresh-token SQL 使用同一 resolver；应用自行书写的 raw SQL 不自动改写，必须使用 `Db::table_name()` / `quoted_table_name()`。表名前缀或映射不会迁移既有 schema 或数据。
- 按 历史决策 049，`MigrationServiceProvider<M>` 在 PostgreSQL/MySQL 上的 `up`、`down`、`status` 于所选 Db 主库取得独立 `migration_locks` 固定行的事务锁；resolver-aware 锁表惰性初始化，默认等待 30 秒，可通过 `with_lock_wait_timeout` 配置。竞争按 PostgreSQL/MySQL 驱动锁冲突短暂重试，超时 fail-closed；进程/连接退出释放行锁。仅保护同一物理数据库、相同 resolver 且通过 `MigrationServiceProvider` 执行的命令；直接 SeaORM/raw SQL/外部工具不受保护。DDL 仍由 SeaORM runner 管理，MySQL 隐式提交和部分 schema 清理由应用负责。PostgreSQL 18/MySQL 8.4 多进程等待超时与 owner 强杀恢复测试均通过。
- `MigrationServiceProvider<M>` 注册 `migrate` 命令，调用应用 migrator 的 `up`、`down` 和 `status`；`sea_orm_migration` 从 `openrust::database` re-export。
- `SeedServiceProvider::new(seed_fn)` 注册 `db seed`；异步回调接收 `Db`，种子数据与幂等性由应用负责。
- `openrust generate model` 与 `generate migration` 可一致传入 `--uuid-id`（`db-uuid` feature，UUIDv7）及 `--soft-delete`（nullable UTC `deleted_at`）；实体生成显式 `Entity::not_deleted()` scope。删除范围不会自动作用于 `find()`、raw SQL 或其他入口，恢复由应用将 `deleted_at` 清空；模型和迁移选项须匹配。
- `dialect::timestamp_utc(name, backend)` 生成 PostgreSQL `TIMESTAMP WITH TIME ZONE` / MySQL `DATETIME(6)` / SQLite `DATETIME`；`dialect::json(name, backend)` 生成 PostgreSQL `JSONB` / MySQL `JSON` / SQLite `TEXT`，完整SQLite JSON选项仍开放；其他backend明确拒绝。
- `db-postgres` 与 `db-mysql` 是可选 feature；未启用 backend feature 时默认不启用具体数据库 driver。
- 数据库连接失败只给出 backend 名，不把连接 URL 或驱动错误写入启动错误。

## 后续边界

嵌套事务与 savepoint、重试、直接 SeaORM/原生 SQL 逃生口均不受事务事件覆盖；提交前与提交后失败边界见 历史决策 041。

当前内置连接器包括PostgreSQL、MySQL与限定SQLite；版本门禁仍为PG14/MySQL8/SQLite3.26。目标为MariaDB 10.3+、MySQL 5.7+、PostgreSQL 10.0+、SQLite 3.26.0+、SQL Server 2017+，MariaDB差异、SQL Server driver及旧版SQL/驱动/能力门控仍须实现，不能仅改版本检查或假定MySQL等同MariaDB。日常仅真实MySQL8.4；其他类型及最低/当前版本统一发布前验收，历史PG14/MySQL8结果不证明新基线完整支持。

SQLite自定义插件仍可经通用Db公开API提供连接；只有实际启用内置feature、标准配置与迁移锁来源时才走SQLite迁移所有者。固定迁移crate依赖只作可追溯manifest修正：移除其传递SeaORM默认RETURNING，其他此前有效features在workspace显式保持，生成/consumer不另引官方迁移包。源码、许可和微补丁见vendor/sea-orm-migration/SOURCES.md；path/Git消费已验不等于未来crates.io发布。五后端、完整懒连接与SQLite业务链继续在唯一todo内，不将连接或DDL通过扩张成全框架支持。

## 验证

`cargo test -p openrust` 验证默认配置、URL 遮蔽、未初始化句柄、pool 参数、backend feature 与 table-name resolver。历史 PostgreSQL 14 / MySQL 8.0 隔离库测试覆盖 resolver-aware 迁移、独立 limiter 实例的并发共享配额、SHA-256 存储、固定窗口重置、过期记录清理、迁移 up/status/down、UTC/JSON 列与 seed 命令。MySQL 强制 Rustls TLS，并断言协商了 TLS 且连接会话时区为 `+00:00`。2026-10-26 根模块迁移后的认证数据库与生成项目测试已在 PostgreSQL 16 / MySQL 8.4 通过。2026-10-26 在 PostgreSQL 18.6 / MySQL 8.4.13 真实复制拓扑分别运行 `OPENRUST_TEST_VERIFY_REPLICA_LAG=1 cargo test -p openrust --features db-postgres --test db_connection` 与 `OPENRUST_TEST_VERIFY_REPLICA_LAG=1 cargo test -p openrust --features db-mysql --test db_connection` 并通过：确认独立副本身份、暂停复制时旧读、主库 scope 新读、两个 App 的 scope 隔离、spawn 不继承 scope、恢复后追平；`initialize_with_replicas` 的自定义接入也通过。双库 release `db_routing` 500 次 `SELECT 1` 微基准已记录于 `memory/testing.md`，不代表业务负载性能提升。数据库默认连接支持 `DB_CONNECTION`、`DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME`、`DB_PASSWORD` 与 `DB_TABLE_PREFIX`；需要完整 URL 时可通过配置文件或应用变量 `APP_DATABASE__URL` 设置。历史决策002的暂定版本已由[兼容基线](../plans/database-compatibility-baseline.md)替代；上述历史验收不外推新最低版本。
