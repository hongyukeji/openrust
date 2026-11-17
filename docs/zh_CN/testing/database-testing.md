# 数据库测试

## 隔离数据库

用 `TestApp::sqlite` 为每个测试应用创建自己的 SQLite 文件。夹具在 Provider 注册阶段替换数据库连接并选择 SQLite。文件位于测试根目录内，并行测试使用各自的数据。

## 迁移与填充

在 Builder 回调中按应用的装配方式注册迁移和 Seeder，再运行 `test.artisan(["migrate", "--seed"]).await.assertSuccessful()`。在同一夹具内重新建表和填充时使用 `migrate:fresh --seed`。新夹具从空数据库文件开始，通过各自的数据库文件实现隔离。

### 运行 Seeder {#running-seeders}

调用 `test.seed().await?` 执行已注册的 `DatabaseSeeder`。用 `seedWith` 传入一个或多个 Seeder 名称，按列表顺序执行。空列表直接返回成功。用 `seedOn("reports", None)` 在具名连接上运行默认 Seeder，或传入 `Some(&[...])` 选择指定的 Seeder。

每个 Seeder 都通过关闭交互的 Artisan `db:seed` 执行。命令失败时立即返回错误并停止列表。处于测试事务时，选中连接上的写入随该事务回滚。其他连接按各自的事务边界提交。

```rust
use openrust::testing::TestApp;

async fn populate(
    test: &mut TestApp,
    seeders: &[&str],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.seed().await?;
    test.seedWith(seeders).await
}
```

## 托管 MySQL 测试库 {#managed-mysql-test-schemas}

用 `TestApp::mysql` 在专用 MySQL 测试服务器上分配独立库。在 Builder 中注册迁移和 Seeder，再通过 Artisan 执行。账号需要创建及删除测试库、创建归属标记例程，以及读取 `performance_schema` 中服务活动表的权限。

```rust
use openrust::testing::TestApp;

async fn run_migrations(
    admin_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::mysql(admin_url, |builder| builder).await?;
    test.artisan(["migrate"]).await.assertSuccessful();
    test.close().await
}
```

调用 `close().await` 等待清理。它先关闭应用连接池，再核验管理租约、归属标记及其他活动连接。核验失败时，错误会提供保留的库名以便检查。夹具被丢弃时，会在仍运行的 Tokio runtime 上尝试清理。

### 恢复遗留的 MySQL 测试库 {#recovering-mysql-test-schemas}

要在测试进程被终止后恢复，使用 `TestApp::mysqlWithRecovery` 或 `TestApp::mysqlConnectionsWithRecovery`。传入夹具临时根目录之外、已存在的本地私有目录。Unix 上使用权限 `0700`。每个库在创建前写入一份 `openrust_test_<id>.json` 归属记录。记录包含服务器标识和归属令牌，需私下保存。连接 URL 和凭据另行保管。

在新进程中传入归属记录路径和原专用测试服务器的管理连接 URL。删除库之前，恢复流程会核验服务器 UUID、取得库租约，再检查归属例程及会话活动。租约仍被持有、库仍被占用或无法验证归属时，恢复返回错误并保留记录。清理成功后删除记录，重复恢复直接成功。

```rust
use std::path::Path;
use openrust::testing::TestApp;

async fn recover_test_schema(admin_url: &str, receipt: &Path) -> std::io::Result<()> {
    TestApp::recoverMysql(admin_url, receipt).await
}
```

## 刷新数据库 {#refreshing-the-database}

在托管 MySQL 夹具上调用 `refreshDatabase`。它在首次回调前运行 `migrate:fresh`，后续回调沿用同一组选项时复用迁移后的基线。每次回调都有独立的回滚事务。具名 Seeder 优先于 `seed`。

```rust
use openrust::testing::{RefreshDatabaseOptions, TestApp};

async fn check_baseline(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options = RefreshDatabaseOptions { seed: true, ..Default::default() };
    test.refreshDatabase(options, |test| Box::pin(async move {
        test.assertDatabaseCount("items", 1).await
    })).await
}
```

迁移或 Seeder 失败会直接返回错误，可以再次调用来重试准备。回调返回错误且回滚成功时保留基线。事务中断或选项变化后，下次调用会重新准备。调用在迁移期间被取消时，下一次刷新先等待正在运行的迁移结束。

将 `without_migration_events` 设为 `true`，可在准备期间抑制所选连接的查询等连接事件。迁移生命周期事件、查询日志和其他连接的事件仍可使用。测试回调和准备阶段之外的命令会正常分发连接事件。

## MySQL 事务隔离 {#mysql-transaction-isolation}

为测试应用配置专用的 MySQL 测试库。先运行迁移和基础填充，再调用 `test.databaseTransaction`。回调中，使用默认连接的 HTTP 请求、进程内 Artisan 命令和数据库断言共用一个会话。成功和失败都会回滚，应用事务使用保存点。

```rust
use openrust::testing::TestApp;

async fn check_seeded_items(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.databaseTransaction(|test| Box::pin(async move {
        test.artisan(["db:seed"]).await.assertSuccessful();
        test.assertDatabaseHas("items", &[("name", "seeded".into())]).await
    })).await
}
```

使用 InnoDB 表。结构变更在回调外执行。同一会话上的操作应逐个等待完成。另行启动的任务、外部进程和其他具名连接有各自的事务边界。

## 多个测试连接 {#multiple-test-connections}

调用 `TestApp::mysqlConnections`，为每个具名服务器 URL 创建独立的托管库。第一个名称成为默认连接。`refreshDatabase` 通过 `migrate:fresh` 准备默认连接。在回调前用 `migrate --database=<名称>` 准备次级库。

```rust
use openrust::testing::{RefreshDatabaseOptions, TestApp};

async fn run_test(
    url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::mysqlConnections(&[("mysql", url), ("reports", url)], |builder| {
        builder
    }).await?;
    test.artisan(["migrate", "--database=reports"]).await.assertSuccessful();
    let options = RefreshDatabaseOptions {
        connections_to_transact: vec!["mysql".into(), "reports".into()],
        ..Default::default()
    };
    test.refreshDatabase(options, |_| Box::pin(async { Ok(()) })).await?;
    test.close().await
}
```

用 `connections_to_transact` 选择每次回调结束后回滚的连接，空列表选择默认连接。夹具已经准备好时，`databaseTransactionOn` 可直接运行同样的回滚流程。

## 数据库断言

用 `assertDatabaseHas` 和 `assertDatabaseMissing` 检查行值，支持空值。用 `assertDatabaseCount` 核对行数，用 `assertDatabaseEmpty` 检查空表。对应的 `On` 方法接受可选的连接名称。

```rust
use openrust::{database::query::Record, testing::TestApp};

#[tokio::test]
async fn keeps_records_in_the_test_database() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::sqlite(|builder| builder)?;
    let db = test.database().await?;
    db.getSchemaBuilder()
        .create("items", |table| {
            table.id();
            table.string("name", 40);
        })
        .await?;
    db.table("items")
        .insert(&[Record::from([("name".into(), "fixture".into())])])
        .await?;
    test.assertDatabaseHas("items", &[("name", "fixture".into())]).await?;
    test.assertDatabaseCount("items", 1).await?;
    Ok(())
}
```

### 软删除与模型断言 {#soft-deletion-and-model-assertions}

`assertSoftDeleted` 要求匹配行的 `deleted_at` 非空。`assertNotSoftDeleted` 要求存在匹配行，且其 `deleted_at` 为空。需要指定列名或连接时，使用 `assertSoftDeletedOn` 或 `assertNotSoftDeletedOn`。

将 Eloquent 模型传给 `assertModelExists` 或 `assertModelMissing`，按模型连接和主键查询。软删除的行仍计为存在。`assertSoftDeletedModel` 和 `assertNotSoftDeletedModel` 还会使用模型配置的软删除列。

```rust
use openrust::{database::eloquent::Model, testing::TestApp};

async fn assert_archived(
    test: &mut TestApp,
    model: &impl Model,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.assertModelExists(model).await?;
    test.assertSoftDeletedModel(model).await
}
```

### 数据库查询次数 {#expects-database-query-count}

在需要统计的代码前调用 `expectsDatabaseQueryCount(expected, None)`。它统计当前测试应用所有连接产生的查询。传入 `Some("reports")` 可限定为具名连接。计入 `pretend` 模式生成的查询。执行失败的 SQL 和夹具准备期关闭数据库事件时的查询不计入。

调用 `close().await` 核对次数并等待测试库清理。次数不符会返回错误，夹具仍会清理测试库。夹具正常离开作用域时，次数不符会使测试失败。

```rust
use openrust::testing::TestApp;

async fn count_queries(
    mut test: TestApp,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.expectsDatabaseQueryCount(1, None).await?;
    test.database().await?.select("SELECT 1", &[]).await?;
    test.close().await
}
```
