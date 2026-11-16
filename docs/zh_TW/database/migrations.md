# 遷移

## 產生遷移

用 `make:migration` 在 `database/migrations` 產生帶時間戳記的 Rust 原始檔。可以用 `--create=flights` 或 `--table=flights` 明確選擇範本。

```sh
./artisan make:migration create_flights_table
```

OpenRust 在建置時自動探索 `database/migrations/` 中的 Rust 遷移。手寫檔案和 `make:migration` 產生的檔案都使用 `YYYY_MM_DD_HHMMSS_name.rs` 命名，並提供實作 `Migration` 的 `Definition`。重新建置後，`bootstrap/app.rs` 的 `withMigrations` 會接入自動產生的清單。檔名決定遷移帳本名稱，`--path` 從已編譯的遷移中選擇。產生到自訂目錄的遷移需另行接入應用程式。

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

### MySQL 資料表選項 {#mysql-table-options}

在 `Schema::create` 中用 `engine`、`charset` 和 `collation` 設定資料表。MySQL 優先採用 Blueprint 的明確設定，其次採用連線對應的預設值。`comment` 可用於建立和修改資料表。用 `getTables()` 讀取實際的儲存引擎、定序和註解。

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_customers() -> DatabaseResult<()> {
    Schema::create("customers", |table| {
        table.engine("InnoDB");
        table.charset("utf8mb4");
        table.collation("utf8mb4_unicode_ci");
        table.comment("Customer records");
        table.id();
        table.string("code", 24).collation("utf8mb4_bin");
    })
    .await
}
```

用欄位層級的 `charset` 和 `collation` 覆寫文字編碼或比較規則。SQLite 套用欄位定序，並忽略 MySQL 專用的資料表設定。

### 暫存資料表 {#temporary-tables}

使用 `Blueprint::temporary()` 前先借用 Schema 工作階段。用該工作階段的 Schema Builder 建立資料表，再透過同一工作階段執行 SQL。`close().await?` 會先清理受管理的暫存資料表，再歸還連線。工作階段被捨棄時，清理操作會持有原實體連線，直到可以安全歸還。

```rust
use openrust::database::{DatabaseResult, Value, schema::Schema};

async fn staged_code() -> DatabaseResult<Value> {
    let mut session = Schema::session().await?;
    session.getSchemaBuilder().create("import_codes", |table| {
        table.temporary();
        table.string("code", 20);
    }).await?;
    session.statement("INSERT INTO import_codes (code) VALUES (?)", &["pending".into()]).await?;
    let rows = session.select("SELECT code FROM import_codes", &[]).await?;
    session.close().await?;
    Ok(rows[0].get_at(0).cloned().unwrap_or(Value::Null))
}
```

SQLite 的同一工作階段可讀取暫存資料表的中繼資料。MySQL 的 `hasTable()` 可檢查工作階段管理的暫存資料表，詳細欄位資訊可透過工作階段執行 `SHOW COLUMNS` 查看。

## 欄位

定義閉包接收可變的 `Blueprint`。欄位修飾詞接在欄位定義後，例如 `table.string("isbn", 20).nullable()`。`id()` 建立自動遞增主鍵，`timestamps()` 建立允許為空值的建立與更新時間。長度、精確度和小數位數由 Rust 參數明確指定。

用 `change` 指定替換後的欄位定義，包括要保留的修飾詞。`renameColumn` 和 `dropColumn` 使用驅動程式的原生操作。`getColumns`、`getColumnListing` 和 `getColumnType` 從資料庫目錄讀取欄位結構。

SQLite 對需要重建的欄位或外鍵變更執行資料表重建。請在活動交易之外，透過連線的 Schema Builder 發起重建。重建擁有自己的交易，並恢復連線的外鍵設定。會隱含提交的 MySQL DDL 也透過連線的 Schema Builder 執行。

### 更多欄位型別 {#additional-column-types}

用 `tinyText`、`year`、`ipAddress`、`macAddress` 和 `r#enum` 宣告對應的欄位。`year` 搭配 `useCurrent()` 時預設採用資料庫目前的年份。MySQL 使用原生 ENUM 和 SET。SQLite 用 CHECK 約束列舉值，對 SET 傳回不支援。

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_events() -> DatabaseResult<()> {
    Schema::create("incoming_events", |table| {
        table.smallIncrements("id");
        table.r#enum("status", &["pending", "processed"]).default("pending");
        table.year("batch_year").useCurrent();
        table.ipAddress("source_ip").nullable();
        table.binaryWith("digest", 32, true).nullable();
        table.timestampsTz();
    })
    .await
}
```

`binary()` 宣告 BLOB。`binaryWith(name, length, fixed)` 在 MySQL 使用 BINARY 或 VARBINARY，在 SQLite 使用 BLOB。MySQL 的 `tinyIncrements`、`smallIncrements`、`mediumIncrements` 及對應無號整數方法採用各自的整數寬度，SQLite 使用 INTEGER。兩個驅動程式的 `Tz` 方法沿用各自的時間型別。

### 生成欄位 {#generated-columns}

用 `virtualAs` 宣告讀取時運算的欄位，用 `storedAs` 儲存運算結果。兩者都接受可信 SQL 運算式。`virtualAsJson` 和 `storedAsJson` 可使用箭頭分隔的 JSON 路徑。生成欄位需要指定型別，由資料庫依其他欄位運算。

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_invoice_lines() -> DatabaseResult<()> {
    Schema::create("invoice_lines", |table| {
        table.id();
        table.integer("quantity");
        table.integer("unit_cents");
        table.integer("total_cents").storedAs("quantity * unit_cents");
        table.json("metadata").nullable();
        table.string("currency", 3).virtualAsJson("metadata->currency").nullable();
    })
    .await
}
```

`getColumns()` 傳回生成模式和運算式。MySQL 和 SQLite 建立資料表時都支援這兩種模式。SQLite 原生 ALTER 可新增虛擬欄位。儲存型生成欄位可在建立資料表時定義，也可透過明確重建資料表的遷移新增。

### 空間欄位 {#spatial-columns}

用 `geometry` 或 `geography` 宣告空間欄位。選用的子型別對應 MySQL 的 `point`、`multipolygon` 等型別。`geometry` 的預設 SRID 為 0，`geography` 為 4326。傳入 `Some(0)` 時省略 SRID 限制。

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_places() -> DatabaseResult<()> {
    Schema::create("places", |table| {
        table.id();
        table.geography("location", Some("point"), None).spatialIndex();
        table.geometry("outline", Some("polygon"), Some(4326)).nullable();
    })
    .await
}
```

MySQL 驗證宣告的子型別和非零 SRID。對於不可為 NULL 的空間欄位，可用 `spatialIndex()` 宣告索引。SQLite 宣告 `geometry` 型別並支援存入二進位值，空間函式和空間索引需要具備空間能力的資料庫。

### MySQL 欄位修飾詞

用 `invisible()` 讓 `SELECT *` 省略該欄位，仍可按名稱讀取。資料列中的其他值變更時，`useCurrentOnUpdate()` 會更新 timestamp 或 datetime 欄位。如果插入時也要預設使用目前時間，請同時呼叫 `useCurrent()`。在現有 MySQL 資料表中新增或修改欄位時，用 `first()` 或 `after("column")` 指定位置。建立資料表時依所需順序宣告欄位。

```rust
Schema::table("books", |table| {
    table.string("internal_note", 120).nullable().invisible();
    table.timestamp("reviewed_at", 6).useCurrent().useCurrentOnUpdate();
    table.string("edition", 20).nullable().after("title");
}).await?;
```

## 索引

用 `index`、`unique` 或 `primary` 傳入欄位切片。可以指定名稱，或傳 `None` 使用慣例名稱。用 `getIndexes`、`hasIndex` 和 `renameIndex` 檢視或重新命名索引。MySQL 支援在相容欄位建立全文索引。

### 外鍵約束

`foreignId("author_id").constrained()` 參照 `authors.id`。需要明確指定參照資料表和欄位時，使用 `constrainedOn`。先設定欄位修飾詞，再定義外鍵，最後設定 `cascadeOnDelete()` 等參照動作。用 `getForeignKeys` 檢視約束。SQLite 目錄提供約束欄位，`dropForeignColumns` 可依這些欄位刪除資料表層級的外鍵。

#### 浮點精度與原生欄位定義 {#float-precision-and-raw-columns}

`float()` 使用 Laravel 預設的 53 位元精度。用 `floatWith(name, precision)` 明確設定精度。MySQL 的 1–24 位元對應 FLOAT，25–53 位元對應 DOUBLE。用 0 宣告不帶精度參數的原生 FLOAT。SQLite 依原生浮點規則儲存，精度參數不會改變儲存行為。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_measurements() -> DatabaseResult<()> {
    Schema::create("measurements", |table| {
        table.id();
        table.float("value");
        table.floatWith("device_value", 24);
        table.rawColumn("sample_count", "smallint").default(0);
    })
    .await
}
```

遷移中用 `rawColumn(name, definition)` 傳入可信的 SQL 欄位定義。Schema 負責引用欄位名稱並附加常用修飾符，資料庫解讀所傳入的定義。

#### 欄位群組與刪除別名 {#column-groups-and-drop-aliases}

`morphs()` 和 `nullableMorphs()` 使用應用程式設定的多型鍵類型。可用 `numericMorphs`、`uuidMorphs`、`ulidMorphs` 及允許空值的變體明確選擇 ID 類型。如需指定索引名稱或 MySQL 欄位位置，可向型別專用方法、`morphsWith` 或 `nullableMorphs` 傳入對應參數。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_attachments() -> DatabaseResult<()> {
    Schema::create("attachments", |table| {
        table.id();
        table.nullableMorphs("attachable", Some("attachments_attachable"), None);
        table.rememberToken();
        table.nullableTimestamps();
    })
    .await
}

async fn remove_attachment_metadata() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.dropMorphs("attachable", Some("attachments_attachable"));
        table.dropRememberToken();
        table.dropTimestamps();
    })
    .await
}
```

`dropMorphs` 先刪除複合索引，再刪除兩個欄位。建立時指定索引名稱的，刪除時傳入相同名稱。`dropConstrainedForeignId` 先刪除外鍵，再刪除欄位。`dropSoftDeletes(None)` 和 `dropSoftDeletesTz(None)` 刪除預設 `deleted_at`，傳入 `Some(name)` 可指定欄位名稱。

在 `Schema::table` 中使用 `table.after("title", |table| { … })`，可依宣告順序把多個欄位放在 MySQL 現有欄位之後。SQLite 會回傳方言不支援錯誤。

#### 預設值運算式 {#default-expressions}

透過 `DB::raw()` 或 `Expression::new()` 把可信的 SQL 運算式傳給 `default()`。插入資料時省略該欄位，資料庫就會計算運算式。一般字串會作為字面值預設值。

```rust
use openrust::{database::DatabaseResult, support::facades::{DB, Schema}};

async fn create_flights() -> DatabaseResult<()> {
    Schema::create("flights", |table| {
        table.id();
        table.json("movies").default(DB::raw("(JSON_ARRAY())"));
    })
    .await
}
```

預設值運算式不支援參數繫結。請依照資料庫要求編寫運算式，並在需要時加上括號。資料庫會檢查該欄位類型和 Schema 操作是否支援此運算式。

#### MySQL 即時欄位變更與鎖定模式 {#instant-column-operations}

新增欄位或透過 `change()` 修改欄位時，可用 `instant()` 要求即時演算法。使用 `lock()` 指定欄位操作的鎖定模式，支援 `none`、`shared`、`exclusive` 與 `default`。演算法與鎖定能否搭配由 MySQL 判斷。操作不相容時回傳資料庫錯誤，框架不會改用其他演算法重試。

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn prepare_imports() -> DatabaseResult<()> {
    Schema::create("imports", |table| {
        table.id().from(500);
        table.string("name", 100);
    })
    .await?;

    Schema::table("imports", |table| {
        table.string("note", 100).nullable().instant().lock("default");
    })
    .await
}
```

在 MySQL 上用 `from()` 或 `startingValue()` 設定自動遞增欄位的起始值。實際可用的下一個值由 MySQL 根據現有資料決定。SQLite 使用原生序列，使用 MySQL 線上 DDL 修飾符時會回傳方言錯誤。

#### 索引方法與線上刪除欄位 {#index-methods-and-online-column-drops}

在 MySQL 上，用 `algorithm("btree")` 或 `algorithm("hash")` 要求索引方法。`inplace()` 選擇目前操作的 DDL 演算法。MySQL 會根據儲存引擎判斷選項是否受支援。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn revise_imports() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.index(&["status"], Some("imports_status"))
            .algorithm("btree")
            .inplace()
            .lock("none");
        table.dropColumn(&["legacy_code", "legacy_note"])
            .instant()
            .lock("default");
    })
    .await
}
```

將多個欄位名稱傳給 `dropColumn`，可在同一條 MySQL ALTER 陳述式中刪除。可鏈式呼叫 `instant()` 或 `lock()` 要求線上 DDL 選項。引擎拒絕該陳述式時，這條陳述式涉及的欄位會保留。同一個 Blueprint 中較早的陳述式可能已經執行。

#### 從模型定義外鍵欄位 {#foreign-ids-from-models}

使用 `foreignIdFor::<Model>(None)`，由 Eloquent 模型提供外鍵欄位名稱。`constrained()` 讀取模型的資料表和主鍵名稱，也支援自訂名稱。傳入 `Some(name)` 可指定引用端的欄位名稱。

範例中的 `PhotoAsset` 使用 `photo_assets` 資料表，主鍵 `photo_code` 為 UUID。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn add_photo_reference() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.foreignIdFor::<PhotoAsset>(Some("photo_ref"))
            .nullable()
            .constrained()
            .nullOnDelete();
    })
    .await
}

async fn drop_photo_reference() -> DatabaseResult<()> {
    Schema::table("attachments", |table| {
        table.dropConstrainedForeignIdFor::<PhotoAsset>(Some("photo_ref"));
    })
    .await
}
```

模型 ID 預設使用無號大整數。模型為主鍵產生 UUID 或 ULID 時會採用對應欄位類型。可用 `foreignUuidFor` 和 `foreignUlidFor` 明確選擇格式。手動指派 ID 的模型可覆寫 `Model::getKeyType`。刪除沒有外鍵約束的欄位時可用 `dropForeignIdFor`。

#### SQLite 內嵌約束 {#sqlite-inline-constraints}

修改 SQLite 欄位時，既有的內嵌主鍵、UNIQUE、CHECK 和 REFERENCES 子句會保留。重建後的資料表若仍有自動遞增主鍵，也會還原其序列。傳入 `change()` 的欄位定義決定型別、是否可為 NULL 及預設值。

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn relax_owner_reference() -> DatabaseResult<()> {
    Schema::table("orders", |table| {
        table.dropForeignColumns(&["owner_id"]);
        table.integer("owner_id").nullable().change();
    })
    .await
}
```

用 `dropForeignColumns` 依本地欄位刪除外鍵，或用 `dropForeign` 刪除有明確名稱的外鍵。兩種方法都支援內嵌 REFERENCES 和資料表層級外鍵。用 `dropPrimary` 刪除內嵌或資料表層級主鍵。其餘內嵌約束仍然生效。

#### 切換外鍵檢查 {#toggling-foreign-key-constraints}

用 `Schema::withoutForeignKeyConstraints` 在同一寫入工作階段中暫停外鍵檢查。透過傳入的工作階段執行 SQL，並呼叫其 `getSchemaBuilder()` 操作 Schema。離開範圍時會恢復進入前的設定。

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn import_order() -> DatabaseResult<()> {
    Schema::withoutForeignKeyConstraints(|session| {
        Box::pin(async move {
            session
                .statement(
                    "INSERT INTO orders (id, customer_id) VALUES (?, ?)",
                    &[1.into(), 42.into()],
                )
                .await?;
            Ok(())
        })
    })
    .await
}
```

需要手動切換時，呼叫 `Schema::session().await?` 或具名連線建構器的 `session().await?` 取得寫入工作階段，再呼叫 `disableForeignKeyConstraints` 或 `enableForeignKeyConstraints`，最後用 `close().await?` 歸還。SQLite 資料表重建繼續使用連線的 Schema Builder，由它管理重建所需的工作階段和交易。
