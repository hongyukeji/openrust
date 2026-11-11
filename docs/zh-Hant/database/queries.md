# 查詢建構器

## 執行資料庫查詢

在應用程式生命週期內使用 `DB::table("users")?`，或對具名連線呼叫 `connection.table("users")`。交易內使用 `transaction.table("users")`，讓查詢繼續使用同一條寫庫實體連線。查詢執行方法為非同步，傳回 `Result`。

`get` 傳回資料列清單。`first` 和 `find` 傳回選用資料列。`value` 傳回單一值，找不到資料列時傳回 `Value::Null`。`pluck` 傳回單一欄位的值清單。用 `exists` 或 `doesntExist` 判斷是否存在符合的資料列。`useWritePdo` 將該查詢送往寫庫。

### 彙總

使用 `count("*")`、`min("column")`、`max("column")`、`sum("column")` 或 `avg("column")`。計數傳回無號整數，其他彙總保留驅動程式傳回的值型別。MySQL 的精確小數以文字傳回。空集合加總傳回零。

## SELECT 陳述式

用 `select` 指定傳回欄位，用 `addSelect` 附加欄位。欄名支援 `as` 別名。`distinct()` 去除重複資料列。語法編譯器引用識別字並套用連線的資料表前綴。用 `selectRaw` 或 `DB::raw` 明確傳入受信任的運算式。值個別繫結，識別字從應用程式預先定義的名稱中選擇。

## 聯結

使用 `join`、`leftJoin`、`rightJoin` 或 `crossJoin`。需要多個 ON 條件時，將閉包傳給 `joinWith` 或 `leftJoinWith`。`on` 比較欄位，`r#where` 繫結值。`joinSub` 可聯結衍生查詢。MySQL 支援相關 `joinLateral` 和 `leftJoinLateral`。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn items_in_region(connection: &Connection, region: &str) -> DatabaseResult<Vec<Row>> {
    let items = connection.table("items as i");
    let groups = items
        .subquery("groups as g")
        .select(["g.id"])
        .r#where("g.region", "=", region);

    items
        .joinSub(&groups, "regional", |join| join.on("i.group_id", "=", "regional.id"))?
        .select(["i.code", "i.stock"])
        .orderBy("i.code", "asc")
        .get()
        .await
}
```

## UNION

呼叫 `union(&other)?` 去除重複資料列，或用 `unionAll(&other)?` 保留重複資料列。合併前設定的排序或資料列數限制套用到第一個查詢，合併後設定的套用到合併結果。

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

## 進階 WHERE 子句

在同一具名連線上，用 `whereExists`、`whereNotExists`、`whereInSub` 或 `whereSub` 引入子查詢。`selectSub` 加入純量投影，`fromSub` 從衍生資料表讀取。交易內用外層建構器的 `subquery` 建立子查詢，整條陳述式透過外層交易連線執行。

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

## 插入陳述式

`insert` 接受一個或多個 `Record`，批次資料使用相同的欄位。`insertOrIgnore` 使用驅動程式原生的忽略行為，並傳回受影響的資料列數。`insertUsing` 接受目標欄位與選擇查詢。`insertGetId` 從同一次執行結果取得主鍵。

### Upsert

`upsert` 接受資料、衝突欄位和要更新的欄位。SQLite 使用傳入的衝突欄位。MySQL 透過資料表上的唯一鍵判斷衝突。

```rust
use openrust::database::{Connection, DatabaseResult, query::Record};

pub async fn save_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let mut transaction = connection.beginTransaction().await?;
    let row = Record::from([("code".into(), code.into()), ("stock".into(), stock.into())]);
    let affected = transaction
        .table("items")
        .upsert(&[row], &["code"], Some(&["stock"]))
        .await?;
    transaction.commit().await?;
    Ok(affected)
}
```

## 更新陳述式

`update` 傳回受影響的資料列數。`updateOrInsert` 先檢查資料是否存在，再執行寫入。需要在一條 SQL 內處理插入與衝突時使用 `upsert`。`increment` 和 `decrement` 在 SQL 中運算，並可一併更新其他欄位。

### 更新 JSON 欄位

將 `options->preferences->enabled` 這類鍵傳給 `update`。MySQL 使用 `JSON_SET`，支援陣列索引。SQLite 使用合併修補，值為 null 的成員會移除對應的鍵。

## 刪除陳述式

設定條件後呼叫 `delete()`，會傳回受影響的資料列數。未設定條件時，刪除會套用到整張資料表。`truncate()` 清空資料表並重設自動遞增主鍵。MySQL 的此陳述式會隱含提交，請在交易外執行。

## 悲觀鎖定

在交易中使用 `lockForUpdate` 或 `sharedLock`。MySQL 持有資料列鎖直到提交或復原。SQLite 採用自身的交易鎖定機制，沒有對應的資料列鎖定子句。
