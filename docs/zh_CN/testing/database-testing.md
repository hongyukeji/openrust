# 数据库测试

## 隔离数据库

用 `TestApp::sqlite` 为每个测试应用创建自己的 SQLite 文件。夹具在 Provider 注册阶段替换数据库连接并选择 SQLite。文件位于测试根目录内，并行测试使用各自的数据。

## 迁移与填充

在 Builder 回调中按应用的装配方式注册迁移和 Seeder，再运行 `test.artisan(["migrate", "--seed"]).await.assertSuccessful()`。在同一夹具内重新建表和填充时使用 `migrate:fresh --seed`。新夹具从空数据库文件开始，通过各自的数据库文件实现隔离。

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

## 数据库断言

用 `assertDatabaseHas` 和 `assertDatabaseMissing` 检查指定字段值的记录，包括空值。用 `assertDatabaseCount` 检查记录数。

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
