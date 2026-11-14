# 資料庫測試

## 隔離資料庫

用 `TestApp::sqlite` 為每個測試應用程式建立自己的 SQLite 檔案。夾具在 Provider 註冊階段替換資料庫連線並選擇 SQLite。檔案位於測試根目錄內，平行測試使用各自的資料。

## 遷移與填充

在 Builder 回呼中依應用程式的組裝方式註冊遷移和 Seeder，再執行 `test.artisan(["migrate", "--seed"]).await.assertSuccessful()`。在同一夾具內重新建表和填充時使用 `migrate:fresh --seed`。新夾具從空資料庫檔案開始，透過各自的資料庫檔案實現隔離。

## 資料庫斷言

用 `assertDatabaseHas` 和 `assertDatabaseMissing` 檢查指定欄位值的紀錄，包括空值。用 `assertDatabaseCount` 檢查紀錄數。

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
