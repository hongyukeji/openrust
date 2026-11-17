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

### 索引提示 {#index-hints}

同一查詢以最後設定的索引提示為準。MySQL 和 MariaDB 支援 `useIndex`、`forceIndex` 及 `ignoreIndex`，可指定單一名稱或以逗號分隔的多個名稱。SQL Server 的 `forceIndex` 接收單一索引，並與同表的資料列鎖定提示合併。SQLite 的 `forceIndex` 對單一非 `PRIMARY` 索引生效。SQL Server 與 SQLite 忽略其餘兩種方法。PostgreSQL 忽略這三種提示。請使用應用程式定義的索引名稱。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn indexed_items(connection: &Connection, score: i64) -> DatabaseResult<Vec<Row>> {
    connection
        .table("items")
        .forceIndex("idx_score")
        .r#where("score", "=", score)
        .orderBy("id", "asc")
        .get()
        .await
}
```

## 聯結

使用 `join`、`leftJoin`、`rightJoin` 或 `crossJoin`。需要多個 ON 條件時，將閉包傳給 `joinWith` 或 `leftJoinWith`。`on` 比較欄位，`r#where` 繫結值。`joinSub` 可聯結衍生查詢。

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

### 交叉聯結與固定順序聯結 {#cross-and-straight-joins}

使用 `crossJoinSub` 將每一筆外層資料與衍生查詢的每一筆資料配對。MySQL 和 MariaDB 的 `straightJoin` 維持左側來源先於右側來源。使用 `straightJoinWhere` 在聯結條件中繫結值，`straightJoinWith` 接收閉包，`straightJoinSub` 接收衍生查詢。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn label_paid_orders(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    let orders = connection.table("orders as o");
    let labels = orders.subquery("labels").r#where("active", "=", 1);

    orders
        .straightJoin("payments as p", "o.id", "=", "p.order_id")
        .crossJoinSub(&labels, "label")?
        .r#where("p.state", "=", "paid")
        .select(["o.id", "label.code"])
        .orderBy("label.code", "asc")
        .get()
        .await
}
```

### 關聯子查詢聯結 {#lateral-joins}

PostgreSQL、MySQL 8.0.14+ 與 SQL Server 支援 `joinLateral` 和 `leftJoinLateral`。在子查詢中以 `whereColumn` 參照外層欄位。左聯結保留未配對的外層資料列，右側欄位傳回 null。PostgreSQL 與 MySQL 使用 `JOIN LATERAL`，SQL Server 使用 `CROSS APPLY` 或 `OUTER APPLY`。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn latest_posts(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    let users = connection.table("users as u");
    let posts = users
        .subquery("posts as p")
        .select(["p.id as post_id"])
        .whereColumn("p.user_id", "=", "u.id")
        .r#where("p.published", "=", 1)
        .orderByDesc("p.id")
        .limit(1);

    users
        .leftJoinLateral(&posts, "latest")?
        .select(["u.id", "latest.post_id"])
        .orderBy("u.id", "asc")
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

用 `whereExists`、`whereNotExists`、`whereInSub` 或 `whereSub` 引入子查詢。`selectSub` 加入純量投影，`fromSub` 從衍生資料表讀取。交易內用外層建構器的 `subquery` 建立子查詢，整條陳述式透過外層交易連線執行。

### MySQL 跨資料庫子查詢 {#mysql-cross-schema-subqueries}

兩個 MySQL 連線設定的服務端點一致時，可以組合它們的查詢建構器。框架會為子查詢尚未限定的主資料表加上資料庫名稱，並保留已指定資料庫名稱的資料表。額外的 JOIN 資料表或預先建構的衍生查詢需明確指定資料庫名稱。

```rust
use openrust::database::{Connection, DatabaseResult, Value};

async fn active_customers(
    primary: &Connection,
    reporting: &Connection,
) -> DatabaseResult<Vec<Value>> {
    let active = reporting
        .table("customer_activity")
        .select(["customer_id"])
        .r#where("active", "=", 1);
    primary
        .table("customers")
        .whereInSub("id", &active)?
        .orderBy("id", "asc")
        .pluck("id")
        .await
}
```

MySQL 使用外層連線的權限執行整個陳述式。外層連線的帳號需要有權存取引用的資料表。

### 向量相似度子句 {#vector-similarity-clauses}

向量查詢使用 PostgreSQL 的 `pgvector` 擴充功能或 MariaDB 11.7 及以上版本的餘弦距離。啟用 PostgreSQL 擴充功能、宣告欄位和索引的方法見[向量欄位](./migrations.md#vector-columns)。

`whereVectorSimilarTo` 接收數值向量、0 到 1 的最低相似度，以及是否將最相似的資料列排在前面的旗標。傳入 `false` 可自訂排序。相似度 1 符合方向相同的向量。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn related_documents(
    connection: &Connection,
    embedding: &[f64],
) -> DatabaseResult<Vec<Row>> {
    connection
        .table("documents")
        .select(["id"])
        .selectVectorDistance("embedding", embedding, Some("distance"))?
        .whereVectorSimilarTo("embedding", embedding, 0.7, true)?
        .limit(10)
        .get()
        .await
}
```

可單獨使用 `selectVectorDistance`、`whereVectorDistanceLessThan`、`orWhereVectorDistanceLessThan` 和 `orderByVectorDistance`。距離篩選包含等於上限的結果。查詢 PostgreSQL 向量欄位時，結果以文字表示向量。MariaDB 回傳向量的位元組值。

Eloquent 建構器提供相同的向量方法。可在 `whereNested` 中將距離述詞與其他條件分組。Eloquent 全域作用域約束整個群組，包括每個 `orWhereVectorDistanceLessThan` 分支。

```rust
use openrust::database::eloquent::Model;

let mut query = Document::queryOn(&connection).select(["id"]);
query.whereNested(|group| {
    group
        .whereVectorDistanceLessThan("embedding", &primary, 0.3)
        .orWhereVectorDistanceLessThan("embedding", &alternate, 0.2)
});
let documents = query.orderByVectorDistance("embedding", &primary)?.get().await?;
```

#### 文字嵌入 {#text-embeddings}

在應用程式容器中註冊 `Embeddings`。內建的 `HttpEmbeddingProvider` 接受相容 OpenAI 嵌入協定的完整端點 URL、模型、權杖和選用維度。請從受信任的應用程式設定讀取這些值。其他服務可實作 `EmbeddingProvider`，再透過 `Embeddings::new` 註冊。

```rust
use std::sync::Arc;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::query::embeddings::{Embeddings, HttpEmbeddingProvider},
};

let provider = HttpEmbeddingProvider::new(&endpoint, model, token, Some(1536))?;
app.container().instance(Arc::new(Embeddings::new(provider)))?;
```

Query Builder 和 Eloquent 中帶 `Text` 後綴的方法會先非同步產生向量，再組裝 SQL 子句。數值向量方法維持同步。快取未命中時，查詢文字會傳送給設定的 Provider。成功結果依 Provider 識別與文字在應用程式預設快取中保存 30 天。內建識別包含端點、模型、憑證及維度。

```rust
let documents = connection
    .table("documents")
    .select(["id"])
    .selectVectorDistanceText("embedding", "Rust web frameworks", Some("distance"))
    .await?
    .whereVectorSimilarToText("embedding", "Rust web frameworks", 0.7, true)
    .await?
    .limit(10)
    .get()
    .await?;
```

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

MySQL 和 PostgreSQL 支援純量、陣列及物件候選值。SQLite 的包含條件接受純量。值為 JSON null 的鍵仍存在。PostgreSQL 對 `json` 和 `jsonb` 欄位使用原生 JSONB 包含運算，並用 `jsonb_array_length` 檢查所選陣列的長度。鍵存在條件支援物件鍵和陣列索引，包括 `options->languages[-1]` 這樣的負索引。鍵不存在或文件為 SQL NULL 時，存在條件會傳回 false。PostgreSQL 將路徑區段跳脫為 SQL 常值，候選值和物件鍵分開繫結。

SQL Server 使用 `OPENJSON` 檢查純量包含、鍵存在和長度。長度條件會計算陣列元素或物件成員，路徑不存在或文件為 SQL NULL 時，計數為零。成員名稱區分大小寫，可用 `options->languages[0]` 這類路徑檢查陣列位置。資料庫相容性層級須為 130 或更高。

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

### 值範圍與指定順序 {#value-ranges-and-ordering}

用 `whereValueBetween(value, [lower, upper])` 將繫結值與每列的兩個欄位邊界比較，範圍包含端點。取反用 `whereValueNotBetween`，兩種形式都有 OR 變體。

`inOrderOf` 讓列出的值依指定順序排在前面。清單外的值和空值同屬最後一級。要固定同級順序時再加一項排序。空清單會保留既有排序。可信 SQL 運算元可用 `Expression` 並個別繫結參數。

```rust
use openrust::database::{Connection, DatabaseResult, Value};

pub async fn ordered_matches(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection.table("order_ranges")
        .whereValueBetween(5, ["lower_bound", "upper_bound"])
        .inOrderOf("status", vec!["active".into(), "pending".into()])
        .orderBy("id", "asc")
        .pluck("id")
        .await
}
```

### 限制每組回傳的資料列數 {#group-limit}

呼叫 `groupLimit(count, column)` 限制分區欄位每個值對應的回傳列數。用 `orderBy` 決定每組保留哪些資料列。需要固定順序時，加入唯一欄位作為同值排序條件。`offset` 在每組內略過指定列數，分組上限為零時回傳空結果。

查詢建構器使用 `row_number()` 排名，回傳的資料列會隱藏內部排名欄位。需要對 UNION 結果套用分組上限時，先將 UNION 組成衍生資料表。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn second_per_category(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection.table("group_items")
        .r#where("active", "=", true)
        .orderByDesc("score")
        .orderBy("id", "asc")
        .groupLimit(1, "category")
        .offset(1)
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

PostgreSQL 的聯結或限額更新和刪除透過 `ctid` 子查詢選出目標列。

### 更新運算式與純量子查詢 {#update-expressions}

使用 `updateWith` 混合繫結值、可信的 SQL `Expression` 與 `UpdateValue::Subquery` 指派。透過目標建構器的 `subquery` 建立關聯子查詢，資料庫為每筆相符資料求值，並依自身規則處理空結果或多筆結果。MySQL、MariaDB 與 PostgreSQL 的 JSON 路徑支援運算式。SQLite 合併修補接收字面值。

```rust
use std::collections::BTreeMap;

use openrust::database::{
    Connection, DatabaseResult,
    query::{Expression, UpdateValue},
};

pub async fn refresh_credits(connection: &Connection, id: i64) -> DatabaseResult<u64> {
    let accounts = connection.table("accounts as a");
    let credits = accounts
        .subquery("entries as e")
        .selectRaw("sum(amount)", vec![])
        .whereColumn("e.account_id", "=", "a.id")
        .r#where("e.state", "=", "posted");

    accounts
        .r#where("a.id", "=", id)
        .updateWith(BTreeMap::from([
            ("credits".into(), UpdateValue::Subquery(&credits)),
            (
                "note".into(),
                Expression::new("coalesce(?, 'pending')")
                    .with_bindings(vec!["refreshed".into()])
                    .into(),
            ),
        ]))
        .await
}
```

### PostgreSQL 聯結來源更新 {#postgresql-update-from}

在 PostgreSQL 上呼叫 `updateFrom`，依聯結來源更新目標資料表。PostgreSQL 在 `FROM` 中列出來源，並將聯結條件接在 `WHERE` 的查詢條件之後。與 Laravel 相同，此寫法會忽略排序、限額與位移。繫結一般值可使用 `Record`。需要混合繫結值與可信的 SQL 運算式時使用 `ValueOperand`。

```rust
use openrust::database::{Connection, DatabaseResult, query::Record};

pub async fn verify_paid_orders(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("orders as o")
        .join("payments as p", "o.id", "=", "p.order_id")
        .r#where("p.state", "=", "paid")
        .updateFrom(Record::from([("status".into(), "verified".into())]))
        .await
}
```

### SQL Server 聯結與限額寫入 {#sql-server-bounded-writes}

聯結更新和刪除使用 `FROM` 指定目標資料表或其別名。無聯結時，正數限額使用 `TOP`。有排序時，SQL Server 透過帶排序的 `TOP` 子查詢決定目標資料列。依 Laravel 的行為，帶聯結或正數位移的寫入會忽略限額，限額為零也會省略 `TOP`。

```rust
use std::collections::BTreeMap;
use openrust::database::{Connection, DatabaseResult};

pub async fn activate_latest(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("event_archive")
        .r#where("score", "=", 0)
        .orderBy("id", "desc")
        .limit(1)
        .update(BTreeMap::from([("score".into(), 1.into())]))
        .await
}
```

### 更新 JSON 欄位 {#updating-json-columns}

將 `options->preferences->enabled` 這類鍵傳給 `update`。MySQL 使用 `JSON_SET`，支援陣列索引。SQLite 使用合併修補，值為 null 的成員會移除對應的鍵。

PostgreSQL 對 `json` 和 `jsonb` 欄位使用 `jsonb_set`，路徑區段和新的 JSON 值分開繫結。物件鍵以箭頭分隔，陣列索引使用方括號，支援負索引。路徑區段內的點屬於鍵名。同一欄位的多個路徑依記錄的鍵順序在同一指派中套用。JSON null 會寫入成員。缺少的中間物件維持原樣，SQL NULL 文件仍為 SQL NULL。

在 SQL Server 上，將 JSON 路徑傳給 `update` 會在執行 SQL 前傳回不支援操作的錯誤。仍可傳入欄位名稱和 `Value::Json` 值來取代整份文件。

```rust
use std::collections::BTreeMap;
use openrust::database::{Connection, DatabaseResult};

pub async fn enable_notifications(connection: &Connection, id: i64) -> DatabaseResult<u64> {
    connection
        .table("users")
        .r#where("id", "=", id)
        .update(BTreeMap::from([
            ("options->preferences->enabled".into(), true.into()),
            ("options->preferences->channel".into(), "email".into()),
        ]))
        .await
}
```

## 刪除陳述式

設定條件後呼叫 `delete()`，會傳回受影響的資料列數。未設定條件時，刪除會套用到整張資料表。`truncate()` 清空資料表並重設自動遞增主鍵。MySQL 的此陳述式會隱含提交，請在交易外執行。

### PostgreSQL 清空資料表 {#postgresql-truncate}

PostgreSQL 預設使用 `TRUNCATE … RESTART IDENTITY CASCADE`，清空目標資料表及透過外鍵參照它的資料表，並重設這些資料表擁有的序列。在 PostgreSQL 連線中將 `cascade_on_truncate` 設為 `false` 時，若其他資料表參照目標資料表，PostgreSQL 會拒絕清空。透過交易控制代碼執行時，回滾會還原資料和序列。

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn reset_events(connection: &Connection) -> DatabaseResult<()> {
    connection.table("event_archive").truncate().await
}
```

### SQL Server 清空資料表 {#sql-server-truncate}

SQL Server 執行原生 `TRUNCATE TABLE`，重設識別欄位的起始值。若其他資料表的外部索引鍵參照目標資料表，資料庫會拒絕清空。透過交易控制代碼執行時，回滾會還原資料與識別欄位狀態。

## 悲觀鎖定

在交易內使用 `lockForUpdate` 或 `sharedLock`。`lock(true)` 和 `lock(false)` 對應相同模式。鎖定查詢使用寫入連線。SQLite 使用自身的交易鎖定方式，省略資料列鎖定子句。

### 原生鎖定子句 {#native-locking-clauses}

可將應用程式定義的 SQL 子句傳給 `lock`。MySQL 與 PostgreSQL 可用 `FOR UPDATE SKIP LOCKED`，SQL Server 可用 `WITH (UPDLOCK, ROWLOCK, READPAST)`。SQL Server 同時使用 `forceIndex` 時，自訂鎖必須寫成 `WITH (...)` 清單。請勿將請求輸入拼入子句。

```rust
use openrust::database::{DatabaseResult, Row, Transaction};

pub async fn next_unlocked(
    transaction: &mut Transaction,
    lock_clause: &'static str,
) -> DatabaseResult<Option<Row>> {
    transaction
        .table("job_slots")
        .orderBy("id", "asc")
        .lock(lock_clause)
        .first()
        .await
}
```
