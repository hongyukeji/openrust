# 資料填充

## 編寫 Seeder

用 `make:seeder` 在 `database/seeders` 產生 Rust Seeder。在 `database/seeders/mod.rs` 登錄它的 `SeederEntry`，再重新建置應用程式。`run` 接受 `SeederContext`，可取得所選連線和應用程式服務。

```sh
./artisan make:seeder CatalogSeeder
```

### 呼叫其他 Seeder

`DatabaseSeeder` 是預設入口。用 `call` 依序執行子 Seeder。`callSilent` 隱藏子項的進度輸出。`callOnce` 跳過本次執行中已成功的 Seeder。`callWith` 可傳入參數，子項用 `parameters()` 讀取。

下列範例需要 `plans` 資料表具有唯一的 `code` 欄位和一個 `label` 欄位。重複執行時，upsert 更新同一筆資料列。

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult, query::Record,
    seeder::{Seeder, SeederContext, SeederEntry},
};

pub struct CatalogSeeder;

impl Seeder for CatalogSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let row = Record::from([
                ("code".into(), "standard".into()),
                ("label".into(), "Standard plan".into()),
            ]);
            context.connection().table("plans")
                .upsert(&[row], &["code"], Some(&["label"]))
                .await?;
            Ok(())
        })
    }
}

pub struct DatabaseSeeder;

impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.call(&["CatalogSeeder"]))
    }
}

pub const ENTRIES: &[SeederEntry] = &[
    SeederEntry::new("DatabaseSeeder", &DatabaseSeeder),
    SeederEntry::new("CatalogSeeder", &CatalogSeeder),
];
```

## 執行 Seeder

用 `db:seed` 執行預設 Seeder。用 `--class` 或位置參數指定類別，用 `--database` 選擇連線。Seeder 內透過 `DB` 門面存取時預設使用所選連線，其他工作各自保留自己的連線選擇。

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
