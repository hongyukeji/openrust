# 資料庫分頁

## 基本用法

`paginate(15)` 先查詢總數，再讀取目前頁面。只需要上一頁和下一頁時，使用 `simplePaginate(15)`，它多讀一列來判斷是否還有下一頁。Query Builder 和 Eloquent 均支援這兩種方法。Eloquent 預設按主鍵排序。Query Builder 可明確指定排序以維持跨頁順序。

使用 `paginateWith` 或 `simplePaginateWith` 並傳入 `PageOptions`，可以自訂頁碼參數名稱、查詢欄位和頁碼。HTTP 從目前請求的查詢字串讀取頁碼。HTTP 之外預設使用第 1 頁。

## 游標分頁

`cursorPaginate(15)` 比較排序欄位的值，並多讀一列判斷後續資料。下次請求傳入回傳的 `next_cursor` 或 `prev_cursor`。Query Builder 需按已查詢、非空的純量欄位排序，最後使用唯一欄位區分同值資料。Eloquent 在未宣告排序時使用主鍵。分組查詢、聯合查詢和原始排序運算式使用偏移分頁。

## 關聯模型分頁 {#paginating-related-models}

在 `HasMany` 或 `MorphMany` 描述符上呼叫 `paginate`、`simplePaginate` 或 `cursorPaginate`，傳入父模型與每頁筆數。帶 `With` 的變體接受與 Eloquent 查詢相同的選項。查詢保留父鍵、適用的多型類別、已設定的篩選及模型作用域。父模型沒有鍵時回傳空頁。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasMany}},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn related_comments<P: Model, R: Model>(
    article: &P,
    comments: &HasMany<P, R>,
) -> DatabaseResult<LengthAwarePaginator<R>> {
    comments.paginateWith(article, PageOptions {
        perPage: Some(15),
        pageName: "comments",
        ..Default::default()
    }).await
}
```

`HasManyThrough` 和 `HasOneThrough` 也支援這三種分頁方法。查詢使用中間資料表連接，預設排除已軟刪除的中間模型。呼叫 `withTrashedParents()` 可包含這些模型。預設只選取關聯模型的欄位。連接的資料表有同名欄位時，自訂排序應加上資料表名稱。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasManyThrough}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn through_page<P: Model, I: Model, R: Model>(
    parent: &P,
    relation: &HasManyThrough<P, I, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation.cursorPaginateWith(parent, CursorOptions {
        perPage: Some(15),
        cursorName: "reactions",
        ..Default::default()
    }).await
}
```

`BelongsToMany` 和 `MorphToMany` 支援相同的分頁方法。目前頁面的每個模型都會載入 Pivot，支援自訂模型和存取器。游標分頁會查詢 `orderByPivot` 使用的欄位以產生游標。透過 `withPivot` 指定要在 Pivot 上公開的額外欄位。排序欄位的組合必須唯一識別每一列。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::BelongsToMany}},
    pagination::CursorPaginator,
};

async fn pivot_page<P: Model, R: Model>(
    parent: &P,
    relation: BelongsToMany<P, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation
        .withPivot(["label"])
        .orderByPivot("weight", "asc")
        .orderByPivot("label_id", "asc")
        .cursorPaginate(parent, 15)
        .await
}
```

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
