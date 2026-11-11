# 資料庫分頁

## 基本用法

`paginate(15)` 先查詢總數，再讀取目前頁面。只需要上一頁和下一頁時，使用 `simplePaginate(15)`，它多讀一列來判斷是否還有下一頁。Query Builder 和 Eloquent 均支援這兩種方法。Eloquent 預設按主鍵排序。Query Builder 可明確指定排序以維持跨頁順序。

使用 `paginateWith` 或 `simplePaginateWith` 並傳入 `PageOptions`，可以自訂頁碼參數名稱、查詢欄位和頁碼。HTTP 從目前請求的查詢字串讀取頁碼。HTTP 之外預設使用第 1 頁。

## 游標分頁

`cursorPaginate(15)` 比較排序欄位的值，並多讀一列判斷後續資料。下次請求傳入回傳的 `next_cursor` 或 `prev_cursor`。Query Builder 需按已查詢、非空的純量欄位排序，最後使用唯一欄位區分同值資料。Eloquent 在未宣告排序時使用主鍵。分組查詢、聯合查詢和原始排序運算式使用偏移分頁。

## JSON 和分頁連結

當分頁項目實作 Serde 的 `Serialize` 時，可直接序列化分頁器。用 `map` 將資料庫列或 Eloquent 模型轉換為回應項目，保留分頁中繼資料。`withQueryString` 保留篩選條件，`appends` 加入選定參數。連結使用請求相對路徑。

```rust
use openrust::database::{DB, DatabaseResult};

async fn article_ids(table: &str) -> DatabaseResult<impl serde::Serialize> {
    let mut page = DB::table(table)?
        .orderBy("id", "asc")
        .paginate(15)
        .await?;
    page.withQueryString();
    Ok(page.map(|row| match row.get("id") {
        Some(openrust::database::Value::Integer(id)) => Some(id.to_string()),
        Some(openrust::database::Value::Unsigned(id)) => Some(id.to_string()),
        _ => None,
    }))
}
```
