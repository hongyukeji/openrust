# 查詢建構器

## 執行資料庫查詢

在應用程式生命週期內使用 `DB::table("users")?`，或對具名連線呼叫 `connection.table("users")`。交易內使用 `transaction.table("users")`，讓查詢繼續使用同一條寫庫實體連線。查詢執行方法為非同步，傳回 `Result`。

`get` 傳回資料列清單。`first` 和 `find` 傳回選用資料列。`value` 傳回單一值，找不到資料列時傳回 `Value::Null`。`pluck` 傳回單一欄位的值清單。用 `exists` 或 `doesntExist` 判斷是否存在符合的資料列。`useWritePdo` 將該查詢送往寫庫。

### 彙總

使用 `count("*")`、`min("column")`、`max("column")`、`sum("column")` 或 `avg("column")`。計數傳回無號整數，其他彙總保留驅動程式傳回的值型別。MySQL 的精確小數以文字傳回。空集合加總傳回零。

## 串流讀取結果 {#streaming-results}

在查詢上呼叫 `cursor()` 逐列讀取結果。首次輪詢時開始查詢，`try_next` 讀取並解碼一列，錯誤由串流傳回。連線和查詢遊標沿用已設定的讀寫路由。捨棄非交易遊標會釋放連線租約。

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{Connection, DatabaseResult, Row};

async fn first_streamed_row(db: &Connection) -> DatabaseResult<Option<Row>> {
    let mut rows = db.table("reports").orderBy("id", "asc").cursor();
    rows.try_next().await
}
```

交易中應先讀完遊標，再執行下一條陳述式或提交。陳述式尚未執行完畢時捨棄遊標會中斷交易，捨棄該交易會回滾。

### 分批讀取結果 {#chunking-results}

在已排序的查詢上呼叫 `chunk(count, callback)`。非同步回呼收到非空批次和從 1 開始的頁碼。回傳 `Ok(false)` 提前停止，回傳 `Err` 傳遞錯誤。既有偏移量用於跳過開頭的列，既有限制用於控制總列數。分批透過偏移量翻頁，走訪期間修改比對條件涉及的列可能導致漏讀或重複讀取。

用 `chunkById(count, callback, column, alias)` 或 `chunkByIdDesc` 依唯一且非空的鍵逐批讀取。將 `column` 設為 `None` 時使用 `id`。SQL 投影名稱與欄位名稱不同時，用 `alias` 指定投影名稱。偏移量只用於首批，限制控制所有批次的總列數。處理期間保持分批鍵穩定。回呼可在檢查分批鍵前停止。用 `fromSub` 包裝聯集查詢，使鍵值條件套用到所有分支。

```rust
use openrust::database::{Connection, DatabaseResult};

async fn process_by_id(db: &Connection) -> DatabaseResult<bool> {
    db.table("reports")
        .select(["id as batch_id"])
        .chunkByIdDesc(200, |rows, page| async move {
            println!("{}: {}", page, rows.len());
            Ok(true)
        }, None, Some("batch_id"))
        .await
}
```

用 `each(callback, count)` 逐筆處理偏移量分批結果，每批索引從零開始。`eachById(callback, count, column, alias)` 依鍵升序分批讀取，索引跨批遞進。回呼回傳 `Ok(false)` 可立即停止，回傳 `Err` 可傳遞錯誤。

用 `lazy(count)`、`lazyById(count, column, alias)` 或 `lazyByIdDesc` 將逐批讀取的列作為非同步串流使用。讀取串流時會檢查參數並回傳資料庫錯誤。丟棄串流後即停止擷取後續批次。依鍵讀取的滿批需要投影邊界鍵，最後一批不足批量時可直接結束。`chunkMap(callback, count)` 非同步映射每筆紀錄，回傳重新編號的結果集合。

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

## JSON WHERE 子句

用 `whereJsonContains` 和 `whereJsonDoesntContain` 判斷 JSON 是否包含指定值，用 `whereJsonContainsKey` 和 `whereJsonDoesntContainKey` 檢查路徑是否存在，用 `whereJsonLength` 比較長度。用 `->` 分隔路徑段，也可附加 `groups[0]` 這樣的索引。每個方法都有 `orWhere` 變體。

```rust
use openrust::database::{Connection, DatabaseResult, Row};
use serde_json::json;

pub async fn matching_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .whereJsonContains("options->languages", json!("en"))
        .whereJsonContainsKey("options->timezone")
        .whereJsonLength("options->languages", ">", 1)
        .get()
        .await
}
```

MySQL 支援純量和陣列候選值。SQLite 的包含條件接受純量。值為 JSON null 的鍵依然存在。路徑和值均透過參數繫結傳入 SQL。

### JSON 重疊 {#json-overlap}

`whereJsonOverlaps` 檢查所選 JSON 值是否與候選值共享元素或鍵值對。MySQL 使用 `JSON_OVERLAPS`。`whereJsonDoesntOverlap` 將述詞取反，兩者均有 `orWhere` 變體。路徑不存在或文件為 SQL NULL 時，MySQL 傳回 SQL NULL，WHERE 會篩除該列，取反時亦然。

```rust
use openrust::database::{Connection, DatabaseResult, Value};
use serde_json::json;

async fn eligible_user_ids(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection
        .table("users")
        .whereJsonOverlaps("options->languages", json!(["en", "fr"]))
        .orderBy("id", "asc")
        .pluck("id")
        .await
}
```

候選值可以是 JSON 值，也可以是含可信 SQL 和獨立繫結的 `Expression`。SQLite 對這些述詞傳回不支援操作的錯誤。

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

#### 衝突時賦值 {#upsert-conflict-assignments}

需要在衝突時同時使用插入值和指定值時，呼叫 `upsertWith`。`UpsertUpdate::FromInsert` 複製待插入資料列中的指定欄位，`UpsertUpdate::Value` 繫結固定值，`UpsertUpdate::Expression` 接收可信 SQL 和獨立繫結值。沒有衝突的資料列依插入值和資料表預設值寫入。

```rust
use openrust::database::{
    Connection, DatabaseResult,
    query::{Record, UpsertUpdate},
};

pub async fn import_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let row = Record::from([("code".into(), code.into()), ("stock".into(), stock.into())]);
    connection
        .table("items")
        .upsertWith(
            &[row],
            &["code"],
            &[
                UpsertUpdate::FromInsert("stock"),
                UpsertUpdate::Value("source", "inventory import".into()),
            ],
        )
        .await
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
