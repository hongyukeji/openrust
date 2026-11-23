# 資料填充

## 編寫 Seeder

用 `make:seeder` 在 `database/seeders/` 產生 Rust Seeder。產生器會寫入具名的 `ENTRY: SeederEntry`，手寫 Seeder 也提供相同的常數。OpenRust 建置時會自動探索該目錄和子目錄中的檔案，並透過 `bootstrap/app.rs` 的 `with_seeders` 接入清單。此目錄中的每個 Rust 檔案提供一個項目，`mod.rs` 用於模組宣告。`run` 接受 `SeederContext`，可取得所選連線和應用程式服務。

Artisan 執行 Seeder 及其子 Seeder 時會暫時停用 Eloquent 的批次賦值保護。`Model::create` 和 `fill` 等方法可以設定受保護的屬性，仍會套用修改器與型別轉換。指令成功或失敗時都會恢復保護。獨立的 Tokio 工作（包括透過 `tokio::spawn` 建立的工作）保留各自的保護。

```sh
./artisan make:seeder CatalogSeeder
```

### 呼叫其他 Seeder

`DatabaseSeeder` 是預設入口。用 `call` 依序執行子 Seeder。`callSilent` 隱藏子項的進度輸出。`callOnce` 跳過本次執行中已成功的 Seeder。`callWith` 可傳入參數，子項用 `parameters()` 讀取。

用 `callWithMany(names, parameters)` 依序傳入相同的值給多個 Seeder。用 `callSilentWith(names, parameters)` 隱藏這些 Seeder 的進度行，或用 `callOnceWith(names, silent, parameters)` 跳過已成功的 Seeder 並選擇是否顯示進度。各 Seeder 透過 `parameters()` 讀取參數。發生錯誤時停止執行清單，失敗的 Seeder 可以重試。

下列範例需要 `plans` 資料表具有唯一的 `code` 欄位和一個 `label` 欄位。重複執行時，upsert 更新同一筆資料列。

將子 Seeder 放在 `database/seeders/catalog_seeder.rs`。

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult, query::Record,
    seeder::{Seeder, SeederContext, SeederEntry},
};

pub(super) const ENTRY: SeederEntry = SeederEntry::new("CatalogSeeder", &CatalogSeeder);

pub struct CatalogSeeder;

impl Seeder for CatalogSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let label = context.parameters().get("label").cloned()
                .unwrap_or_else(|| "Standard".into());
            let row = Record::from([
                ("code".into(), "standard".into()),
                ("label".into(), label),
            ]);
            context.connection().table("plans")
                .upsert(&[row], &["code"], Some(&["label"]))
                .await?;
            Ok(())
        })
    }
}
```

在 `database/seeders/database_seeder.rs` 中依登錄名稱呼叫它。

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult, query::Record,
    seeder::{Seeder, SeederContext, SeederEntry},
};

pub(super) const ENTRY: SeederEntry = SeederEntry::new("DatabaseSeeder", &DatabaseSeeder);

pub struct DatabaseSeeder;

impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.callOnceWith(
            &["CatalogSeeder"],
            true,
            Record::from([("label".into(), "Standard plan".into())]),
        ))
    }
}
```

## 執行 Seeder

用 `db:seed` 執行預設的 `DatabaseSeeder`，由它明確呼叫其他 Seeder。用 `--class` 或位置參數指定類別。以 `Admin/CatalogSeeder` 產生的 Seeder 可用 `--class='Admin\CatalogSeeder'` 執行。用 `--database` 選擇連線。Seeder 內透過 `DB` 門面存取時預設使用所選連線，其他工作各自保留自己的連線選擇。

```sh
./artisan db:seed
./artisan db:seed --class=CatalogSeeder
./artisan migrate:fresh --seed --seeder=CatalogSeeder
```

也可以在 `migrate --seed` 或 `migrate:refresh --seed` 後填充資料。用 `--seeder` 選擇指定類別。Seeder 失敗會停止後續呼叫，並傳回失敗結束碼。

### 在正式環境中強制執行 Seeder

正式環境會提示確認。部署流程已確認資料變更時，使用 `--force`。

```sh
./artisan db:seed --force
```

## 使用模型工廠 {#using-model-factories}

模型繫結、狀態、回呼及關聯工廠請見 [Eloquent 工廠](../eloquent/factories.md#factory-model-binding)。

應用程式在 `database/factories` 提供預設 `UserFactory`。遷移空資料庫後，`./artisan db:seed` 使用它建立已驗證的範例帳號 `test@example.com`，密碼為 `password`。模型依應用程式設定雜湊密碼。預設 Seeder 使用選定的資料庫，執行期間靜默模型事件。對同一帳號重複執行預設 Seeder 會收到電子郵件重複錯誤。

將 Seeder 選定的連線傳給 `createOn`，即可沿用 `db:seed --database=…` 的連線選擇。可在 Seeder 的 `run` 中呼叫下方函式。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
    seeder::SeederContext,
};

async fn seed_flights<M: Model>(context: &SeederContext<'_>) -> DatabaseResult<()> {
    Factory::<M>::new(|| Attributes::from([("name".into(), "Flight".into())]))
        .sequence(|index| Attributes::from([("name".into(), format!("Flight {index}").into())]))
        .count(3)
        .createOn(context.connection())
        .await?;
    Ok(())
}
```

## 靜默模型事件 {#muting-model-events}

註冊 Seeder 時呼叫 `withoutModelEvents()`，可在執行期間靜默 Eloquent 模型事件。它呼叫的子 Seeder 會繼承此範圍。成功傳回或發生錯誤後皆會恢復模型事件。Seeder 進度輸出與工廠回呼照常執行。

```rust
use openrust::database::seeder::SeederEntry;

pub(super) const ENTRY: SeederEntry =
    SeederEntry::new("DatabaseSeeder", &DatabaseSeeder).withoutModelEvents();
```
