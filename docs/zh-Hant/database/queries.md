# 查詢建構器

## 執行資料庫查詢

在應用程式生命週期內使用 `DB::table("users")?`，或對具名連線呼叫 `connection.table("users")`。交易內使用 `transaction.table("users")`，讓查詢繼續使用同一條寫庫實體連線。查詢執行方法為非同步，傳回 `Result`。

`get` 傳回資料列清單。`first` 和 `find` 傳回選用資料列。`value` 傳回單一值，找不到資料列時傳回 `Value::Null`。`pluck` 傳回單一欄位的值清單。用 `exists` 或 `doesntExist` 判斷是否存在符合的資料列。`useWritePdo` 將該查詢送往寫庫。

### 彙總

使用 `count("*")`、`min("column")`、`max("column")`、`sum("column")` 或 `avg("column")`。計數傳回無號整數，其他彙總保留驅動程式傳回的值型別。MySQL 的精確小數以文字傳回。空集合加總傳回零。

## SELECT 陳述式

用 `select` 指定傳回欄位，用 `addSelect` 附加欄位。欄名支援 `as` 別名。`distinct()` 去除重複資料列。語法編譯器引用識別字並套用連線的資料表前綴。用 `selectRaw` 或 `DB::raw` 明確傳入受信任的運算式。值個別繫結，識別字從應用程式預先定義的名稱中選擇。

## 基本 WHERE 子句

Rust 中 `where` 是關鍵字，使用 `r#where(column, operator, value)` 對應 Laravel 的 `where`。`orWhere` 用 OR 連接條件，用 `whereNested` 加上括號分組，用 `whereNot` 否定分組。可用 `whereNull`、`whereNotNull`、`whereIn`、`whereNotIn`、`whereBetween` 或 `whereColumn` 表達對應比較。空 IN 不符合任何資料列，空 NOT IN 符合所有資料列。

`whereLike` 接受區分大小寫參數。`whereDate` 和 `whereTime` 分別接受 `chrono::NaiveDate` 和 `chrono::NaiveTime`。可用 `toSql()` 和 `getBindings()` 檢視 SQL 及依執行順序排列的參數。

```rust
use openrust::database::{Connection, DatabaseResult, Row, Value};

pub async fn active_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["id", "name"])
        .r#where("active", "=", true)
        .whereNested(|query| {
            query
                .whereNull("archived_at")
                .orWhere("name", "=", "Admin")
        })
        .orderBy("id", "asc")
        .forPage(1, 20)
        .get()
        .await
}

pub async fn active_count(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("users")
        .r#where("active", "=", Value::Bool(true))
        .count("*")
        .await
}
```

## 排序、分組、限制和位移

用 `orderBy("id", "asc")` 或 `orderByDesc("id")` 排序。`reorder()` 清除現有排序。用 `groupBy` 和 `having` 篩選分組。`limit` 和 `offset` 選取指定範圍的資料列。`forPage(page, per_page)` 的頁碼從 1 開始。SQLite 和 MySQL 執行位移查詢時要求同時指定限制資料列數。分頁時請指定能穩定決定資料列順序的排序條件。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn departments(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["department"])
        .selectRaw("count(*) as total", Vec::new())
        .groupBy(&["department"])
        .having("total", ">=", 2)
        .orderByDesc("total")
        .get()
        .await
}
```
