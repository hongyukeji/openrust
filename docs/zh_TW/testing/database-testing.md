# 資料庫測試

## 隔離資料庫

用 `TestApp::sqlite` 為每個測試應用程式建立自己的 SQLite 檔案。夾具在 Provider 註冊階段替換資料庫連線並選擇 SQLite。檔案位於測試根目錄內，平行測試使用各自的資料。

## 遷移與填充

在 Builder 回呼中依應用程式的組裝方式註冊遷移和 Seeder，再執行 `test.artisan(["migrate", "--seed"]).await.assertSuccessful()`。在同一夾具內重新建表和填充時使用 `migrate:fresh --seed`。新夾具從空資料庫檔案開始，透過各自的資料庫檔案實現隔離。

## 受管 MySQL 測試資料庫 {#managed-mysql-test-schemas}

用 `TestApp::mysql` 在專用 MySQL 測試伺服器上分配獨立資料庫。在 Builder 中註冊遷移和 Seeder，再透過 Artisan 執行。帳號需要建立與刪除測試資料庫、建立歸屬標記預存程序，以及讀取 `performance_schema` 伺服器活動資料表的權限。

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

呼叫 `close().await` 等待清理。它先關閉應用程式連線池，再核對管理租約、歸屬標記及其他使用中的連線。核對失敗時，錯誤會提供保留的資料庫名稱以便檢查。夾具被捨棄時，會在仍運作的 Tokio runtime 上嘗試清理。

### 復原遺留的 MySQL 測試資料庫 {#recovering-mysql-test-schemas}

若要在測試程序遭終止後復原，請使用 `TestApp::mysqlWithRecovery` 或 `TestApp::mysqlConnectionsWithRecovery`。傳入夾具暫存根目錄之外、已存在的本機私有目錄。Unix 上使用權限 `0700`。每個資料庫在建立前會寫入一份 `openrust_test_<id>.json` 歸屬紀錄。紀錄包含伺服器識別碼和歸屬權杖，須妥善保管。連線 URL 和憑證另行保存。

在新程序中傳入歸屬紀錄路徑和原專用測試伺服器的管理連線 URL。刪除資料庫之前，復原流程會驗證伺服器 UUID、取得資料庫租約，再檢查歸屬預存程序與連線活動。租約仍被持有、資料庫仍在使用或無法驗證歸屬時，復原會回傳錯誤並保留紀錄。清理成功後會刪除紀錄，重複復原會直接成功。

```rust
use std::path::Path;
use openrust::testing::TestApp;

async fn recover_test_schema(admin_url: &str, receipt: &Path) -> std::io::Result<()> {
    TestApp::recoverMysql(admin_url, receipt).await
}
```

## 更新資料庫 {#refreshing-the-database}

在受管 MySQL 夾具上呼叫 `refreshDatabase`。它在首次回呼前執行 `migrate:fresh`，後續回呼使用同一組選項時沿用遷移後的基線。每次回呼都有獨立的復原交易。具名 Seeder 優先於 `seed`。

```rust
use openrust::testing::{RefreshDatabaseOptions, TestApp};

async fn check_baseline(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options = RefreshDatabaseOptions { seed: true, ..Default::default() };
    test.refreshDatabase(options, |test| Box::pin(async move {
        test.assertDatabaseCount("items", 1).await
    })).await
}
```

遷移或 Seeder 失敗會直接傳回錯誤，可以再次呼叫以重試準備。回呼傳回錯誤且交易復原成功時會保留基線。交易中斷或選項變更後，下次呼叫會重新準備。呼叫在遷移期間被取消時，下一次更新會先等待執行中的遷移結束。

將 `without_migration_events` 設為 `true`，可在準備期間抑制所選連線的查詢等連線事件。遷移生命週期事件、查詢日誌和其他連線的事件仍可使用。測試回呼和準備階段以外的命令會正常分發連線事件。

## MySQL 交易隔離 {#mysql-transaction-isolation}

為測試應用程式設定專用的 MySQL 測試庫。先執行遷移和基礎填充，再呼叫 `test.databaseTransaction`。回呼中，使用預設連線的 HTTP 請求、程序內 Artisan 命令和資料庫斷言共用一個工作階段。成功和失敗都會回復，應用程式交易使用儲存點。

```rust
use openrust::testing::TestApp;

async fn check_seeded_items(test: &mut TestApp) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    test.databaseTransaction(|test| Box::pin(async move {
        test.artisan(["db:seed"]).await.assertSuccessful();
        test.assertDatabaseHas("items", &[("name", "seeded".into())]).await
    })).await
}
```

使用 InnoDB 資料表。結構變更在回呼外執行。同一工作階段上的操作應逐一等待完成。另行啟動的工作、外部程序和其他具名連線有各自的交易邊界。

## 多個測試連線 {#multiple-test-connections}

呼叫 `TestApp::mysqlConnections`，為每個具名伺服器 URL 建立獨立的受管資料庫。第一個名稱成為預設連線。`refreshDatabase` 透過 `migrate:fresh` 準備預設連線。在回呼前用 `migrate --database=<名稱>` 準備次要資料庫。

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

用 `connections_to_transact` 選擇每次回呼結束後復原的連線，空清單選用預設連線。夾具已準備好時，`databaseTransactionOn` 可直接執行相同的復原流程。

## 資料庫斷言

用 `assertDatabaseHas` 和 `assertDatabaseMissing` 檢查資料列的值，支援空值。用 `assertDatabaseCount` 核對資料列數，用 `assertDatabaseEmpty` 檢查空資料表。對應的 `On` 方法接受選填的連線名稱。

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

### 軟刪除與模型斷言 {#soft-deletion-and-model-assertions}

`assertSoftDeleted` 要求符合條件的資料列在 `deleted_at` 有值。`assertNotSoftDeleted` 要求存在符合條件的資料列，且其 `deleted_at` 為空。需要指定欄位名稱或連線時，使用 `assertSoftDeletedOn` 或 `assertNotSoftDeletedOn`。

將 Eloquent 模型傳給 `assertModelExists` 或 `assertModelMissing`，依模型連線和主鍵查詢。已軟刪除的資料列仍視為存在。`assertSoftDeletedModel` 和 `assertNotSoftDeletedModel` 也會使用模型設定的軟刪除欄位。

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

### 資料庫查詢次數 {#expects-database-query-count}

在要統計的程式碼前呼叫 `expectsDatabaseQueryCount(expected, None)`。它統計目前測試應用程式所有連線產生的查詢。傳入 `Some("reports")` 可限定為具名連線。`pretend` 模式產生的查詢也會計入。執行失敗的 SQL 和夾具準備期間停用資料庫事件時的查詢不計入。

呼叫 `close().await` 核對次數並等待測試資料庫清理。次數不符會回傳錯誤，夾具仍會清理測試資料庫。夾具正常離開作用域時，次數不符會使測試失敗。

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
