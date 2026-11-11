# 遷移

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
