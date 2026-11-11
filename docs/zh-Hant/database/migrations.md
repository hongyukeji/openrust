# 遷移

## 產生遷移

用 `make:migration` 在 `database/migrations` 產生帶時間戳記的 Rust 原始檔。可以用 `--create=flights` 或 `--table=flights` 明確選擇範本。

```sh
./artisan make:migration create_flights_table
```

在 `database/migrations/mod.rs` 登錄產生的模組和 `MigrationEntry`，然後重新建置。登錄時填寫的路徑用於產生帳本名稱，也可以用 `--path` 選擇該遷移。

## 遷移結構

在 `up` 執行遷移，在 `down` 撤銷。透過傳入的 `MigrationContext` 執行 Schema 和 SQL 操作，預演時遷移器才能收集陳述式。實作 `getConnection` 可指定遷移連線，實作 `shouldRun` 可依條件跳過遷移。

```rust
use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct Definition;

impl Migration for Definition {
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection.getSchemaBuilder().create("flights", |table| {
                table.id();
                table.string("name", 120);
                table.timestamps();
            }).await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection.getSchemaBuilder().dropIfExists("flights").await
        })
    }
}
```

## 執行遷移

第一次連線前先建立 SQLite 資料庫檔案，或設定 MySQL 資料庫和認證資訊。`migrate` 按檔名順序執行待執行的遷移，並將每項成功結果記錄到設定的帳本。

```sh
./artisan migrate --pretend
./artisan migrate
./artisan migrate:status
```

用 `--database` 選擇帳本連線，用 `--step` 為每項遷移單獨分配批次。在空資料庫上，`--pretend` 會準備帳本並輸出 SQL，遷移本身不執行，帳本也不增加記錄。需要重建資料表的 SQLite 變更仰賴實際目錄中繼資料，在預演中會傳回錯誤。

正式環境中改變結構的命令會要求確認。部署流程已確認變更時可傳入 `--force`。MySQL 和 SQLite 的單項遷移可能包含多條 DDL，其中一條失敗時，先前成功的陳述式可能已生效。重試前先檢查失敗的遷移。

## 回復遷移

`migrate:rollback` 按反向順序撤銷最近一批遷移。用 `--step` 指定筆數，或用 `--batch` 指定批次。

```sh
./artisan migrate:rollback --pretend
./artisan migrate:rollback --step=1
```

用 `migrate:reset` 回復全部遷移，用 `migrate:refresh` 回復後重新執行。帳本存在時，`migrate:fresh` 刪除所選資料庫中的所有資料表，包括使用其他前綴的資料表，然後重新遷移。傳入 `--drop-views` 可同時刪除檢視表。

## 資料表

用 `Schema::create("books", |table| { … }).await?` 在預設連線建立資料表，用 `Schema::connection("reporting")?` 選擇具名連線。已持有連線時，呼叫 `getSchemaBuilder()` 取得同一套介面。

用 `table` 修改資料表，用 `rename` 重新命名，用 `dropIfExists` 依存在條件刪除。用 `hasTable` 和 `getTables` 檢視資料庫。Schema 操作使用寫入連線。

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_catalog(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();

    schema.create("authors", |table| {
        table.id();
        table.string("name", 120);
    }).await?;

    schema.create("books", |table| {
        table.id();
        table.foreignId("author_id").constrained().cascadeOnDelete();
        table.string("title", 200);
        table.unique(&["author_id", "title"], None);
        table.timestamps();
    }).await?;

    schema.table("books", |table| {
        table.string("isbn", 20).nullable();
        table.index(&["isbn"], Some("books_isbn_index"));
    }).await?;

    assert!(schema.hasColumns("books", &["title", "isbn"]).await?);
    assert!(schema.hasIndex("books", "books_isbn_index").await?);
    Ok(())
}
```

## 欄位

定義閉包接收可變的 `Blueprint`。欄位修飾詞接在欄位定義後，例如 `table.string("isbn", 20).nullable()`。`id()` 建立自動遞增主鍵，`timestamps()` 建立允許為空值的建立與更新時間。長度、精確度和小數位數由 Rust 參數明確指定。

用 `change` 指定替換後的欄位定義，包括要保留的修飾詞。`renameColumn` 和 `dropColumn` 使用驅動程式的原生操作。`getColumns`、`getColumnListing` 和 `getColumnType` 從資料庫目錄讀取欄位結構。

SQLite 對需要重建的欄位或外鍵變更執行資料表重建。請在活動交易之外，透過連線的 Schema Builder 發起重建。重建擁有自己的交易，並恢復連線的外鍵設定。會隱含提交的 MySQL DDL 也透過連線的 Schema Builder 執行。

## 索引

用 `index`、`unique` 或 `primary` 傳入欄位切片。可以指定名稱，或傳 `None` 使用慣例名稱。用 `getIndexes`、`hasIndex` 和 `renameIndex` 檢視或重新命名索引。MySQL 支援在相容欄位建立全文索引。

### 外鍵約束

`foreignId("author_id").constrained()` 參照 `authors.id`。需要明確指定參照資料表和欄位時，使用 `constrainedOn`。先設定欄位修飾詞，再定義外鍵，最後設定 `cascadeOnDelete()` 等參照動作。用 `getForeignKeys` 檢視約束。SQLite 目錄提供約束欄位，`dropForeignColumns` 可依這些欄位刪除資料表層級的外鍵。
