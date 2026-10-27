# 数据库

`DatabaseConfig` 定义于 `openrust::config::database`，并从 `openrust::database` 重新导出。运行时配置保存在生成应用根目录的 `config/`；数据库凭据可放入受保护的 `.env` 或由部署环境注入，进程环境变量优先。生产使用 `.env` 时须限制文件权限，并确保应用根目录不作为静态公开目录。

OpenRust 框架的数据库模块。

## 连接数据库

启用一个 backend feature，并在应用中注册 `DatabasePlugin`：

```toml
[dependencies]
openrust = { version = "0.0.0", features = ["db-postgres"] }
```

数据库连接默认使用 `DB_CONNECTION`、`DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME` 和 `DB_PASSWORD`，表前缀使用 `DB_TABLE_PREFIX`。驱动名采用 `postgres` 或 `mysql`，连接信息会在加载时组装为驱动 URL。需要完整 URL 时可在 `[database].url` 或 `APP_DATABASE_URL` 中设置。连接池默认最多 10 个连接、最少 1 个连接，连接超时为 5 秒，获取连接超时为 30 秒。可在 `[database]` 中覆盖这些值，也可使用 `APP_DATABASE_MAX_CONNECTIONS`、`APP_DATABASE_MIN_CONNECTIONS`、`APP_DATABASE_CONNECT_TIMEOUT_SECS` 和 `APP_DATABASE_ACQUIRE_TIMEOUT_SECS`。

框架管理的表默认使用 `tb_` 前缀。设置 `table_prefix` 可更改前缀（空字符串表示不使用前缀）；也可以将指定的逻辑表名映射为完整物理表名：

```toml
[database]
table_prefix = "app_"

[database.table_names]
users = "accounts"
```

显式映射优先级更高，且不会再添加前缀。表名必须是 ASCII 标识符，框架解析器限制物理表名最多 64 字节；PostgreSQL 另检查 63 字节、MySQL 检查 64 字节。SQLite 自定义连接使用双引号引用。`Db::table_name()` 返回经过校验的物理表名；`Db::quoted_table_name()` 返回按 backend 转义后的形式。`MigrationPlugin` 和 `TestDatabase::create_with_table_names()` 会在每次迁移调用期间设置相同的 resolver。创建框架相关表的手写迁移可读取 `DatabaseTableNames::current()`；在迁移作用域外调用会返回 `None`。框架不会全局重写原始 SQL：框架认证 SQL 会自行解析表名，应用 SQL 则必须使用 `Db` resolver。修改表名配置不会迁移现有 schema 或数据。

```rust,ignore
use openrust::{Application, AppBuilder, DatabasePlugin, NoConfig};

struct App;

impl Application for App {
    type Config = NoConfig;

    fn build(builder: &mut AppBuilder) {
        builder.plugin(DatabasePlugin);
    }
}
```

该 plugin 会注册可克隆的 `Db` 服务，并在应用完成组装时创建连接池；启用 `HttpPlugin` 时还为每个数据库实例注册只读就绪探针，检查主库及所有只读副本。探针失败会使 `/ready` 返回 503，但不会暴露 URL 或驱动错误。日常开发使用 PostgreSQL 18、MySQL 8.4 和 Redis 8；数据库集成 CI 使用 PostgreSQL 18 与 MySQL 8.4。Redis 集成会在 M5 缓存能力落地后加入 CI。当前运行时版本检查接受暂定最低版本 PostgreSQL 14+ 和 MySQL 8.0+；v1 发布前仍须验证框架在这些最低版本上的完整兼容性。Redis 暂定最低版本为 6.2+，日常开发基线为 8；其 driver 和兼容性验证待 M5 完成。只有主版本跨度较大或存在具体兼容风险时才增加额外版本专项检查。`Db::connection()` 返回 SeaORM `DatabaseConnection`；`Db::transaction()` 会提交成功的回调，并回滚返回错误的回调。标准 `DatabasePlugin` 与 `EventsPlugin` 同时注册时，提交前会派发可拒绝并回滚的 `TransactionBeforeCommit`；成功提交后只尽力通知 direct `TransactionCommitted` listeners，监听器失败不会伪装成事务回滚。可靠副作用应在提交前通过事务内 queued listener 持久化。直接使用 SeaORM 连接或原生 SQL 不触发生命周期事件；自定义数据库 plugin 也不会自动绑定事件。其 `Debug` 输出不包含已配置的 URL。若 URL 缺失、backend 未启用、连接池配置无效或连接失败，启动会以配置错误终止。

只读副本可选配置，但必须与主库使用相同 backend：

```toml
[database]
url = "postgres://app:secret@primary/app"
read_replicas = ["postgres://app:secret@replica-a/app", "postgres://app:secret@replica-b/app"]
```

`Db::connection()` 和 `Db::primary_connection()` 始终返回主库连接。`Db::read_connection()` 会轮询已配置副本；没有副本时返回主库。只应将它用于可接受复制延迟的只读操作；框架不会检查 SQL、重试查询，也不会在出错后回退。`Db::with_primary(future)` 会临时让当前 Tokio task 中此 handle 的 `read_connection()` 返回主库，适用于同一工作单元内先写后读。独立 spawn 的 task 不会继承该作用域。事务、迁移、seed 和框架关键路径始终使用主库。副本连接或 backend 校验失败会导致启动失败。每个副本都有使用相同连接池上限的独立连接池，因此最大总连接数为 `(replicas + 1) * max_connections`。该配置只管理连接；数据库复制、一致性和副本只读权限仍由部署环境负责。

一个应用可以同时配置多个彼此独立的数据库。现有 `[database]` 是默认实例，原有 `Db` 服务继续指向它；具名实例在 `[database.instances.<name>]` 下完整配置，不继承默认 URL、池参数、表名解析或副本。名称须匹配小写 ASCII 字母开头、后续为小写字母/数字/下划线/短横线，且不能使用保留名称 `default`：

```toml
[database]
url = "postgres://app:password@primary/app"
table_prefix = "tb_"

[database.instances.analytics]
url = "postgres://app:password@analytics/app"
max_connections = 4
table_prefix = "analytics_"
read_replicas = ["postgres://app:password@analytics-replica/app"]
```

在应用中通过 `Databases::get("analytics")` 选择具名 `Db`；`Databases::default_database()` 与 `AppState::try_get::<Db>()` 返回相同默认句柄。每个句柄各自固定事务主库、只读副本池和表名 resolver。未知名称、无效名称、缺 URL、连接/backend 错误均失败关闭；错误和 `Debug` 不显示凭据。`migrate --database analytics up`、`status` 或 `down` 会对选中实例运行当前应用注册的 migration；不传 `--database` 时沿用 default。应用须逐实例显式执行迁移。

Auth、Session、队列、审计和共享限流仍使用默认 `Db`。选择其他连接不会隐式切换这些框架服务；框架也不提供跨数据库事务或自动查询路由。

`DatabasePlugin` 还会使用 PostgreSQL 或 MySQL 存储注册共享的 `openrust::core::RateLimiter`。使用前请将 `DatabaseRateLimitMigration` 加入应用 migrator；schema 变更必须显式执行，启动时不会自动运行：

```rust,ignore
use openrust::database::DatabaseRateLimitMigration;

fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(DatabaseRateLimitMigration)]
}
```

调用 `RateLimiter::check` 时，key 应稳定并包含策略标识，同时提供正数上限和整秒窗口。共享同一数据库的应用节点之间，固定窗口计数器具有原子性。超限请求仍会计数并返回带 `Retry-After` 的 HTTP 429；存储故障或缺失表会失败关闭。数据库行保存 SHA-256 摘要而非原始 key，这属于假名化，不足以保护电子邮件地址等可猜测值。每个 store 实例每累计 256 次消费，会尝试清理最多 512 条过期记录；自定义 store 必须说明自己的保留策略。框架不提供可用于集群共享的内存或本地文件实现。

## 跨实例租约与请求幂等

需要数据库共享锁或显式请求幂等时，将 `CoordinationMigration` 加入应用 migrator；迁移不会由服务隐式执行。`SharedLocks` 以数据库时钟保存 owner token、过期时间和 fencing generation。竞争者在租约到期前得到 `Busy`；到期后可接管，旧 lease 不能续约或释放新 owner。保护同一数据库的业务写入时，须在业务事务内调用 `assert_current_in_transaction`；TTL 不能阻止暂停的旧进程继续执行任意外部副作用。

`RequestIdempotency` 必须由变更接口显式调用。应用提供稳定的调用者 scope、幂等 key 和规范化请求指纹；同一有效 key 会返回处理中、指纹冲突或已完成 JSON 响应。首次领取后，将业务写入和 `complete_in_transaction` 放在同一个 `Db::transaction`；事务失败时结果快照与业务写入一起回滚，lease 到期后可再次领取。响应仅支持有上限的 JSON body，不保存 header、二进制或流式内容；授权重验、敏感字段过滤与指纹规范化由应用负责。外部副作用不具备数据库原子性，应结合供应商幂等键或事务队列。

租约与结果保留时长须在 1 毫秒至 24 小时之间。过期记录不会在请求路径中做无界清理；应用维护任务可反复调用 `RequestIdempotency::prune_expired(limit)`，其中 `limit` 为 `1..=1000`，每次最多删除指定数量。

```rust,ignore
use std::time::Duration;
use openrust::database::{CoordinationMigration, IdempotencyClaim, IdempotencyResponse, RequestIdempotency};

// 在应用 migrator 中显式注册：Box::new(CoordinationMigration)
let idempotency = RequestIdempotency::new(db.clone());
match idempotency.claim("account:42:create-order", key, fingerprint, Duration::from_secs(30)).await? {
    IdempotencyClaim::Acquired(lease) => {
        let response = IdempotencyResponse::new(201, serde_json::json!({"order_id": "..."}))?;
        let completing_idempotency = idempotency.clone();
        db.transaction(|transaction| {
            let idempotency = completing_idempotency.clone();
            let lease = lease.clone();
            let response = response.clone();
            Box::pin(async move {
                // 在同一事务执行订单写入，再保存 response 快照。
                idempotency.complete_in_transaction(transaction, &lease, &response, Duration::from_secs(86_400)).await
            })
        }).await?;
    }
    IdempotencyClaim::InProgress { .. } => { /* 返回应用定义的处理中响应 */ }
    IdempotencyClaim::Conflict => { /* 相同 key 携带了不同请求 */ }
    IdempotencyClaim::Replay(response) => { /* 重放前重新检查调用者授权 */ }
}
```

模型生成器默认保留整数自增 ID。可在实体和迁移命令中同时选择 `UUIDv7` 主键或软删除：

```bash
openrust generate model post --uuid-id --soft-delete title:string
openrust generate migration posts --uuid-id --soft-delete title:string
```

`--uuid-id` 会为两个生成 package 启用 `db-uuid` feature，在选定数据库中存储 UUID ID，并由应用进程使用 `UUIDv7` 补齐缺失的插入 ID。`--soft-delete` 会添加可空 UTC `deleted_at`，以及显式的 `Entity::not_deleted()` 查询 helper；未调用该 helper 的查询和原始 SQL 仍会包含已删除记录。模型和迁移选项必须一致。UUID 和软删除都需显式启用，不会修改现有项目或表。

`tools/performance/benches/db_routing.rs` 提供发布模式下主库与只读副本路由的 `SELECT 1` 微基准。运行 `cargo bench -p performance --bench db_routing --features db-routing-bench,db-postgres`，或改用 `db-routing-bench,db-mysql`；设置 `OPENRUST_TEST_DATABASE_URL` 与 `OPENRUST_TEST_READ_REPLICA_URL`。结果只反映该窄查询的串行吞吐量及 p50/p95/p99，不代表业务负载，也不能证明副本提升了吞吐量。该 package 位于 `tools/performance/`，不进入框架默认 targets。

应用如需不同的 SeaORM 连接策略，可用自定义 plugin 替换 `DatabasePlugin`。在 `build` 阶段注册一个 `Db::default()`，再于 `finish` 阶段连接后初始化。带副本的自定义 plugin 可调用 `initialize_with_replicas(primary, replicas)`；所有连接必须报告相同的 SeaORM backend：

```rust,ignore
impl Plugin for CustomDatabasePlugin {
    fn build(&self, app: &mut AppBuilder) -> openrust::Result<()> {
        if app.get::<Db>().is_some() {
            return Err(openrust::Error::custom("database_service_already_registered", "default database handle is already registered"));
        }
        app.insert(Db::default());
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            let connection = connect_with_custom_policy().await?;
            app.state().try_get::<Db>()?.initialize(connection)
        })
    }
}
```

`DatabaseBackend` 集中管理官方支持的 PostgreSQL/MySQL URL scheme 和 feature 检查。`Db::initialize` 接收 SeaORM `DatabaseConnection`；通用 `Db` 连接、事务、表名解析，以及自定义的 SeaORM `MigrationPlugin` 迁移均可用于 SQLite。SQLite 示例见 [`tests/consumers/db-custom-sqlite`](../../../tests/consumers/db-custom-sqlite/README.md)。`Db::backend()` 报告已连接的 SeaORM backend。自定义 plugin 负责自己的连接配置，并须校验应用所需能力。v1 官方数据库引擎为 PostgreSQL 和 MySQL，暂定最低版本分别为 PostgreSQL 14+ 和 MySQL 8.0+；日常开发及数据库 CI 使用 PostgreSQL 18 和 MySQL 8.4 LTS。只有主版本跨度较大或存在具体兼容风险时才增加专项版本检查；暂定最低版本的完整兼容性仍是 v1 发布门槛。SQLite 连接不自动启用 OpenRust 方言 helper、认证/队列/协调等框架迁移、UTC/JSON 列能力、数据库共享锁或 `TestDatabase`；应用必须为自定义迁移及需要的能力自行适配和验证。其他 SeaORM backend 亦须满足相同要求，MongoDB 等不同数据模型不在此契约范围内。切换连接不会迁移既有数据。

为保证迁移列可移植，`openrust::database::dialect::timestamp_utc(name, backend)` 会创建 PostgreSQL `TIMESTAMP WITH TIME ZONE` 或 MySQL `DATETIME(6)`；`dialect::json(name, backend)` 会创建 PostgreSQL `JSONB` 或 MySQL `JSON`。两者均返回 `Result<ColumnDef, DbErr>`，遇到未处理的 backend 时会明确拒绝，而不会静默使用 PostgreSQL SQL。

注册 `MigrationPlugin::<Migrator>::new()`，即可为实现 `sea_orm_migration::MigratorTrait` 的应用 migrator 添加运行时 `migrate` 命令。迁移 API 通过 `openrust::database::sea_orm_migration` 重新导出。运行 `migrate` 或 `migrate up [steps]` 应用待执行迁移；运行 `migrate down [steps]` 回滚迁移（默认回滚一步）；运行 `migrate status` 查看已应用和待执行的迁移。具名数据库使用 `migrate --database <name> [up|down|status]` 选择目标；不指定时沿用默认数据库。三种命令均先在所选主库获取事务行锁，默认最多等待 30 秒；应用可用 `MigrationPlugin::<Migrator>::new().with_lock_wait_timeout(std::time::Duration::from_secs(60))` 调整上限。锁表按当前表名 resolver 惰性创建，应用 migration 不应管理或删除它。该锁仅协调使用同一数据库、相同 resolver 并经此插件运行的命令；直接 SeaORM runner、原生 SQL 和外部迁移工具不受保护。进程退出会释放事务锁，但 MySQL DDL 可能已隐式提交；迁移失败后仍须检查并清理部分 schema 再重试。

将 `SeedPlugin::new(seed_fn)` 与 `DatabasePlugin` 一并注册，即可提供 `db seed` 命令。异步函数接收可克隆的 `Db` handle，并返回 `openrust::Result<()>`；seed 数据及其幂等性由应用负责。

MySQL 连接必须使用 Rustls TLS。通过 MySQL URL 或服务器部署配置服务器证书与信任链；明文 MySQL 连接会被拒绝。PostgreSQL TLS 行为由 URL 和服务器配置决定。

## 能力范围

连接池、事务 helper、自定义 SeaORM 连接注册、迁移命令、UTC/JSON dialect helper、应用自定义 seed 命令、按测试隔离数据库、共享租约、请求幂等，以及 PostgreSQL/MySQL 项目模板均已实现。

## 稳定键有界分批读取

`BatchReader<E>` 复用 SeaORM keyset cursor，按实体主键列声明顺序进行升序或降序读取，不使用 offset。传入的 `Select<E>` 应是完整实体查询，不要预先添加 offset 或 limit。默认每批 100 行，可通过 `BatchOptions::new(1..=1000)` 调整。每次最多物化一批实体模型；此限制按行数计算，不限制单行大字段的内存大小。

受保护读取从 `AuthorizedSelect::into_batches()` 创建，授权范围与业务筛选在每页都保持 AND 关系。普通 SeaORM 实体查询也可显式使用 `BatchReader::new(Entity::find())`，但它不会自动应用资源授权。reader 通过传入 `Db` 的主库连接执行，不走只读副本，也不会切换具名数据库实例。

```rust,ignore
let report = rule
    .select(&user, Condition::all())?
    .into_batches()
    .run(
        &db,
        BatchOptions::default(),
        BatchDirection::Ascending,
        &cancellation,
        |batch| async move {
            process_batch(batch).await?;
            Ok(())
        },
    )
    .await?;
```

取消只在批次边界检查；报告包含已完整处理的记录数、批次数、是否因取消停止及最后完整批次游标。可将该游标交给 `.after(cursor)` 继续读取。callback 错误会停止遍历并返回错误，不推进失败批次的游标；callback 已产生的部分副作用不会自动撤销，调用方须按自身事务需求包裹每批工作并处理重试幂等性。

各批次不共享隐式事务或快照。遍历期间游标之后插入的行可能被读到，游标之前插入的行不会回头读取；删除或筛选字段变更会影响后续页。需要一致快照时，由应用自行管理隔离事务及其长事务成本。HTTP 的 `page/per_page` 和游标分页决策不受此 API 影响。
