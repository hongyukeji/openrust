# 数据库测试

## 隔离数据库

用 `TestApp::sqlite` 为每个测试应用创建自己的 SQLite 文件。夹具在 Provider 注册阶段替换数据库连接并选择 SQLite。文件位于测试根目录内，并行测试使用各自的数据。

## 迁移与填充

在 Builder 回调中按应用的装配方式注册迁移和 Seeder，再运行 `test.artisan(["migrate", "--seed"]).await.assertSuccessful()`。在同一夹具内重新建表和填充时使用 `migrate:fresh --seed`。新夹具从空数据库文件开始，通过各自的数据库文件实现隔离。

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
