# 遷移

## 產生遷移

用 `make:migration` 在 `database/migrations` 產生帶時間戳記的 Rust 原始檔。可以用 `--create=flights` 或 `--table=flights` 明確選擇範本。

```sh
./artisan make:migration create_flights_table
```

OpenRust 在建置時自動探索 `database/migrations/` 中的 Rust 遷移。手寫檔案和 `make:migration` 產生的檔案都使用 `YYYY_MM_DD_HHMMSS_name.rs` 命名，並提供實作 `Migration` 的 `Definition`。重新建置後，`bootstrap/app.rs` 的 `withMigrations` 會接入自動產生的清單。檔名決定遷移帳本名稱，`--path` 從已編譯的遷移中選擇。產生到自訂目錄的遷移需另行接入應用程式。

### 壓縮遷移 {#squashing-migrations}

用 `schema:dump` 儲存 MySQL 資料表定義、檢視表、常式、觸發器和遷移帳本。傾印不含業務資料，並重設資料表的自動遞增計數。PATH 中須有 MySQL 8.4 的 `mysqldump` 和 `mysql` 用戶端。

對 MariaDB 執行 `./artisan schema:dump --database=mariadb`。匯出與還原使用 MariaDB 11.4 或更新版本的 `mariadb-dump` 和 `mariadb`，工具會啟用 TLS 驗證。預設的 `required` 模式下，伺服器和驗證外掛支援時，使用 MariaDB 的自動驗證。需要驗證伺服器 CA 和主機名稱時，設定 `verify_identity` 並提供伺服器 CA。若連線設定為 `verify_ca`，請在執行快照指令前改用 `verify_identity`，並填寫正確的伺服器名稱。

對 PostgreSQL 執行 `./artisan schema:dump --database=pgsql`，由 `pg_dump` 匯出 SQL 定義及遷移帳本。還原 SQL 檔案使用 `psql`，還原 PostgreSQL 封存檔使用 `pg_restore`。自訂傾印路徑使用 `.sql` 副檔名，並將對應的用戶端加入 PATH。設定直連端點時，工具使用該端點，預設檔名取設定中的連線名稱。

對 SQLite 執行 `./artisan schema:dump --database=sqlite`。快照儲存資料表、索引、檢視表、觸發器、虛擬表及遷移帳本，排除業務資料列、SQLite 內部表和虛擬表影子表。影子表由 SQLite 在還原時重建。OpenRust 使用內建 SQLite 連線匯出與還原，支援還原至記憶體資料庫。還原由框架開啟交易，自訂快照指令碼應由框架統一控制交易。

```sh
./artisan schema:dump --database=mysql
./artisan migrate --database=mysql
```

預設檔案是 `database/schema/<connection>-schema.sql`。用 `--database` 選取連線，用 `--path` 指定輸出檔案。帳本沒有遷移紀錄時，`migrate` 先載入快照，再執行待處理的編譯期遷移。`--schema-path` 可指定輸入檔案，預演模式略過載入。

`--without-migration-data` 只匯出定義，包括帳本資料表定義。壓縮遷移時保留帳本資料，遷移器才能略過快照涵蓋的遷移。`--prune` 在傾印寫入後移除 `database/migrations/` 中已編譯的應用遷移原始碼檔案。清理後重新建置應用程式。

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

### 遷移交易 {#migration-transactions}

PostgreSQL 和 SQL Server 預設將一項遷移的 `up` 或 `down` 放在一個交易中。使用傳入的 `MigrationContext` 執行 Schema 變更和 SQL，遷移監聽器可使用事件上下文提供的交易。遷移或同步生命週期監聽器在交易提交前失敗時，會回滾本次變更，帳本保留原先狀態。

需要自動提交的操作，例如 [PostgreSQL 線上索引](#postgresql-online-indexes)，可在該遷移的 `impl Migration` 中覆寫 `withinTransaction()`。退出交易後，後續陳述式失敗時，已完成的變更可能保留。`--pretend` 顯示 SQL，不會啟動遷移交易。

```rust
fn withinTransaction(&self) -> bool {
    false
}
```

### 檢查待執行遷移 {#checking-pending-migrations}

使用 `--pending` 只列出待執行的遷移。使用 `--pending=17` 時，有待執行遷移則回傳 17，清單為空則回傳 0。明確指定的結束碼範圍為 0 到 255，遷移帳本不存在時回傳 1。可透過 `--path` 和 `--database` 限定檢查範圍。

```sh
./artisan migrate:status --pending=17
```

遇到第一個待執行遷移前，`shouldRun` 回傳 false 的遷移顯示為 `Skipped`。此後尚未登記的遷移顯示為 `Pending`，因為前序遷移可能改變後續條件所依賴的資料庫狀態。

### 以警告回報遷移失敗 {#graceful-migrations}

使用 `--graceful` 時，遷移或 Seeder 的執行錯誤會顯示為警告，指令結束碼為 0。失敗的遷移仍是待執行狀態，此前成功的遷移仍保留在帳本中。正式環境確認仍然生效。

```sh
./artisan migrate --graceful
```

### 排除指定的待執行遷移 {#excluding-pending-migrations}

在服務提供者中呼叫目前應用程式遷移器的 `withoutMigrations`，可按名稱從實際執行和預演中排除待執行遷移。可以傳入遷移名稱或 `.rs` 路徑。再次呼叫會取代清單，傳入空切片可清空。狀態仍根據帳本與遷移條件顯示，回滾仍按帳本批次執行。

```rust
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::migrations::Migrator,
    foundation::Application,
    support::ProviderResult,
};

fn configure_migrations(app: &Application) -> ProviderResult {
    let migrator = app.container().get::<Migrator>("migrator")?;
    migrator.withoutMigrations(&["database/migrations/0001_create_first.rs"]);
    Ok(())
}
```

### 建立缺少的資料庫 {#creating-a-missing-database}

設定的 MySQL 或 PostgreSQL 資料庫不存在時，`migrate` 會在建立前提示確認。無人值守部署可使用 `--force`。選定的寫入帳號需要有權建立資料庫。PostgreSQL 會使用選定的遷移端點，包括已設定的直連端點，並暫時連線至 `postgres` 管理資料庫。

對於 SQLite，`migrate` 會詢問是否在設定的路徑建立缺少的資料庫檔案。無人值守時使用 `--force`。未指定此選項時，`--no-interaction` 會保留檔案缺少的狀態。資料庫檔案的上層目錄須已存在。既有檔案及唯讀連線不會自動建立。

```sh
./artisan migrate --database=sqlite --force
./artisan migrate --database=mysql --force
./artisan migrate --database=pgsql --force
```

### 隔離遷移執行 {#isolating-migration-execution}

用 `--isolated` 在遷移前透過預設快取取得原子鎖。參與部署的程序須共用快取後端與鍵前置詞。首次遷移可用 Redis 提供鎖。使用資料庫快取時，鎖表須已存在。

```sh
CACHE_STORE=redis ./artisan migrate --isolated
CACHE_STORE=redis ./artisan migrate --isolated=12
```

其他程序持鎖時，指令會提示遷移已在執行，並以狀態碼 0 結束。可用 `--isolated=12` 將競爭時的狀態碼設為 12，參數接受整數。鎖或快取出錯時指令會停止。執行結束後釋放鎖，程序異常結束時鎖在一小時後到期。

## 回復遷移

`migrate:rollback` 預設在寫入連線中只選取帳本裡最近記錄的一批，再按遷移逆序回滾。用 `--step` 指定筆數，或用 `--batch` 指定批次。

```sh
./artisan migrate:rollback --pretend
./artisan migrate:rollback --step=1
```

用 `migrate:reset` 回復全部遷移，用 `migrate:refresh` 回復後重新執行。帳本存在時，`migrate:fresh` 刪除所選資料庫中的所有資料表，包括使用其他前綴的資料表，然後重新遷移。傳入 `--drop-views` 可同時刪除檢視表。

### 重新執行 MySQL 遷移 {#refreshing-mysql-migrations}

執行 `migrate:refresh` 可回滾已執行的遷移並重新執行。正整數 `--step` 限定回滾的遷移數。遷移帳本為空時，遷移階段會載入所選連線既有的預設 Schema 快照。開始回滾前，資料庫必須可連線。

```sh
./artisan migrate:refresh --database=mysql --step=1
```

### 重建資料庫 {#rebuilding-mysql-with-fresh}

通過正式環境確認後，`migrate:fresh` 可以用選定的寫入帳號建立缺少的 MySQL 或 PostgreSQL 資料庫。資料庫已存在且沒有遷移帳本時，指令直接執行遷移，保留既有資料表。用 `--schema-path` 選擇要還原的快照。

在 PostgreSQL 中，`db:wipe` 和 `migrate:fresh` 的清理階段以選定寫入連線實際生效的 `search_path` 為起點。連線的 `dont_drop` 清單接受實體資料表名稱或附 Schema 的名稱，預設為 `["spatial_ref_sys"]`。用 `--drop-views` 一併刪除檢視表，用 `--drop-types` 刪除使用者定義的型別與定義域。PostgreSQL 的串聯刪除可能影響其他 Schema 的相依物件。

```sh
./artisan migrate:fresh --database=mysql --schema-path=database/schema/custom.sql
```

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

在 `Schema::create` 中用 `engine`、`charset` 和 `collation` 設定資料表。MySQL 優先採用 Blueprint 的明確設定，其次採用連線對應的預設值。`comment` 可用於建立和修改資料表。用 `getTables()` 讀取實際的儲存引擎、定序和註解。 `innoDb()` 等同於 `engine("InnoDB")`。

```rust
use openrust::database::{DatabaseResult, schema::Schema};

async fn create_customers() -> DatabaseResult<()> {
    Schema::create("customers", |table| {
        table.innoDb();
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

### ULID 欄位長度 {#ulid-column-length}

用 `ulidWith(name, length)` 和 `foreignUlidWith(name, length)` 為匯入的識別碼指定字元欄寬。`ulid` 和 `foreignUlid` 預設仍為 26。這些方法設定欄寬。標準 ULID 仍為 26 個字元。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_import_tables() -> DatabaseResult<()> {
    Schema::create("imports", |table| {
        table.ulidWith("id", 32).primary();
    }).await?;
    Schema::create("import_entries", |table| {
        table.id();
        table.foreignUlidWith("import_id", 32)
            .constrainedOn("imports", "id", None)
            .cascadeOnDelete();
    }).await
}
```

### 檢查 SQLite 欄位 {#inspecting-sqlite-columns}

SQLite 欄位宣告為 `VARCHAR(32)` 時，`getColumnType(..., false)` 回傳 `varchar`，`getColumnType(..., true)` 回傳 `varchar(32)`。欄位未宣告型別時，兩者都回傳空字串。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_title() -> DatabaseResult<(String, String)> {
    Ok((
        Schema::getColumnType("books", "title", false).await?,
        Schema::getColumnType("books", "title", true).await?,
    ))
}
```

當整數主鍵是 rowid 的別名且 SQLite 能自動產生其值時，欄位目錄的 `auto_increment` 為真。`nullable` 表示欄位能否儲存空值。例如，一般 SQLite 複合主鍵可允許空值，`NOT NULL`、`STRICT` 和 `WITHOUT ROWID` 會對此加以限制。

### 編輯 Blueprint {#editing-a-blueprint}

用 `idWith("import_id")` 指定自動遞增主鍵欄位名稱，對應 Laravel 的 `id("import_id")`。預設欄位名稱使用 `id()`。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_imports() -> DatabaseResult<()> {
    Schema::create("imports", |table| {
        table.idWith("import_id");
        table.string("source", 32);
        table.text("temporary_notes");
        table.removeColumn("temporary_notes");
    })
    .await
}
```

在 Blueprint 回呼裡用 `removeColumn()` 撤銷待執行的新增欄位或修改欄位，欄位層級修飾符也會一併移除。明確宣告的索引和約束仍留在 Blueprint。刪除資料庫現有欄位時使用 `dropColumn()`。

### 檢查 Blueprint {#inspecting-a-blueprint}

在 Schema 回呼中，`getTable()` 回傳傳給建構器的邏輯表名，連線資料表前綴會在後續產生 SQL 時套用。`creating()` 區分 `Schema::create` 與 `Schema::table` 或條件式改表。

```rust
use openrust::database::schema::Blueprint;

fn prepare_status(table: &mut Blueprint) {
    if table.creating() {
        table.id();
    }

    if !table
        .getColumns()
        .any(|column| column.getName() == "status")
    {
        table.string("status", 24).default("pending");
    }
}
```

用 `getColumns()`、`getAddedColumns()` 和 `getChangedColumns()` 按宣告順序讀取待執行的欄位定義。這些迭代器借用 Blueprint。檢查資料庫現有欄位時，請使用 Schema 門面的目錄方法。

### 解析 Blueprint {#resolving-a-blueprint}

在 Schema 建構器上註冊解析器，可以在操作回呼前加入共用定義。解析器收到可修改的 Blueprint，其中已設定邏輯資料表名稱、連線前綴和建表狀態。

```rust
use openrust::database::schema::Builder;

fn use_import_defaults(schema: &mut Builder<'_>) {
    schema.blueprintResolver(|table| {
        if table.creating() {
            table.id();
            table.string("source", 32);
        }
    });
}
```

每個 Schema 建構器各自持有解析器。請在遷移、交易或工作階段實際使用的建構器上註冊。遷移預演會以相同定義產生 SQL。

### 讀取連線的資料表前綴 {#reading-the-connection-table-prefix}

用 `DB::getTablePrefix()` 讀取預設連線的前綴，或用 `DB::connection(Some("archive"))?.getTablePrefix()` 讀取具名連線的前綴。Blueprint 回呼中的 `getPrefix()` 回傳該 Blueprint 所屬連線的前綴。

```rust
use openrust::{database::DatabaseResult, support::facades::DB};

fn migration_prefixes() -> DatabaseResult<(String, String)> {
    let default = DB::getTablePrefix()?;
    let archive = DB::connection(Some("archive"))?;
    Ok((default, archive.getTablePrefix().to_owned()))
}
```

呼叫 Schema 或查詢建構器時傳入邏輯資料表名稱，框架會自動套用前綴。遷移需要在原生 SQL 中引用實體資料表時，可以讀取這個前綴。

### JSON 欄位 {#json-columns}

MySQL 的 `json()` 和 `jsonb()` 都宣告為 `JSON`。SQLite 預設都宣告為 `TEXT`。在 SQLite 連線設定中分別將 `use_native_json` 或 `use_native_jsonb` 設為 `true`，即可宣告 `JSON` 或 `JSONB`。兩個選項獨立生效。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_documents() -> DatabaseResult<()> {
    Schema::create("documents", |table| {
        table.id();
        table.json("document");
        table.jsonb("snapshot");
    })
    .await
}
```

在 SQLite 中宣告 `JSONB` 欄位不會自動把寫入值編碼成二進位 JSON。用 SQLite 的 `jsonb(...)` SQL 函式寫入二進位值，再用 `json(...)` 讀出 JSON 文字。

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

#### MySQL 產生欄位與目前時間預設值 {#mysql-generated-columns-and-current-defaults}

MySQL 產生欄位預設允許 NULL。使用 `nullableWith(false)` 要求產生值不可為空，或使用 `nullable()` 明確允許 NULL。一般欄位預設不可為空。

在 MySQL 上，DATE、YEAR、DATETIME 和 TIMESTAMP 欄位的 `useCurrent()` 優先於明確指定的預設值，呼叫順序不影響結果。資料庫提供目前的日期、年份或時間戳記，時間戳記沿用欄位精度。其他欄位型別保留明確指定的預設值。

```rust
use openrust::database::schema::Blueprint;

fn define_import_metrics(table: &mut Blueprint) {
    table.id();
    table.integer("quantity").nullable();
    table.integer("doubled").virtualAs("quantity * 2");
    table.integer("total")
        .storedAs("COALESCE(quantity, 0)")
        .nullableWith(false);
    table.timestamp("recorded_at", 3)
        .useCurrent()
        .default("2000-01-01 00:00:00");
}
```

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

### 向量欄位 {#vector-columns}

PostgreSQL 宣告向量欄位前，先啟用已安裝的 `pgvector` 擴充功能。傳入 `None` 使用預設 Schema，也可用 `Some(schema)` 指定擴充功能所在的 Schema。連線的搜尋路徑需要能解析該擴充功能。MariaDB 11.7 及以上版本原生支援向量欄位。

```rust
use openrust::support::facades::Schema;

Schema::ensureVectorExtensionExists(None).await?;
```

```rust
use openrust::support::facades::Schema;

Schema::create("documents", |table| {
    table.id();
    table.vector("embedding", Some(1536)).index();
}).await?;
```

兩種驅動程式都支援明確指定維度。PostgreSQL 也接受 `None` 建立不限維度的向量欄位。預設餘弦索引使用固定維度。MariaDB 需要明確指定維度，建立索引的欄位須為非空。

對向量欄位呼叫 `index()` 會建立餘弦索引。PostgreSQL 使用 HNSW 和 `vector_cosine_ops`，MariaDB 使用 `VECTOR INDEX`、`M=6` 和餘弦距離。可用 `vectorIndex("embedding", Some("semantic_idx"))` 指定名稱。PostgreSQL 可用 `algorithm("ivfflat")` 和 `operatorClass("vector_l2_ops")` 選擇其他索引方法與運算子類別。查詢使用的距離運算式應與運算子類別相符。

用 `dropVectorIndex("semantic_idx")` 刪除具名索引，或用 `dropVectorIndexColumns(&["embedding"])` 刪除慣例名稱的索引。查詢方式見[向量相似度查詢](./queries.md#vector-similarity-clauses)。

MariaDB 可透過 `operatorClass("M=8 DISTANCE=euclidean")` 設定向量索引參數。`M` 接受 3 到 200，`DISTANCE` 接受 `cosine` 或 `euclidean`。查詢建構器的向量搜尋方法使用餘弦距離。

需要在寫入期間建置向量索引時，見 [PostgreSQL 線上索引](#postgresql-online-indexes)。

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

#### 預設欄位長度與時間精度 {#default-column-length-and-time-precision}

`Schema::defaultStringLength` 和 `Schema::defaultTimePrecision` 設定目前應用程式後續欄位定義的預設值。初始預設值為 255 個字元和 0 位小數秒。單一欄位傳入 `None` 可使用應用程式預設值，也可以明確指定長度或精度。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_entries() -> DatabaseResult<()> {
    Schema::defaultStringLength(120)?;
    Schema::defaultTimePrecision(Some(3))?;

    Schema::create("entries", |table| {
        table.id();
        table.string("label", None);
        table.timestamps();
        table.softDeletesDatetime(Some("removed_at"), 6);
    })
    .await
}
```

`timestamps()` 和 `timestampsTz()` 使用設定的精度。使用 `timestampsWith` 或 `timestampsTzWith` 指定精度。`datetimes` 建立可為空值的 `created_at` 和 `updated_at` DATETIME 欄位。`softDeletesDatetime` 建立可為空值的 DATETIME 軟刪除欄位。SQLite 使用原生欄位宣告。

#### MySQL 欄位長度與更新運算式 {#mysql-column-lengths-and-update-expressions}

在 MySQL 上使用 `char(name, 0)` 建立長度為零的固定長度字元欄位。CHAR 最多支援 255 個字元。`binaryWith` 的長度為零時與 `binary` 一樣建立 BLOB，正數長度則依 `fixed` 參數建立 BINARY 或 VARBINARY。

透過 `onUpdate` 為 TIMESTAMP 或 DATETIME 欄位指定可信 SQL 運算式。運算式的小數秒精度應與欄位一致。兩個更新修飾符同時指定時，`useCurrentOnUpdate` 依欄位精度產生運算式並優先使用。

```rust
use openrust::database::{query::Expression, schema::Blueprint};

fn define_import_status(table: &mut Blueprint) {
    table.id();
    table.char("flag", 0).nullable();
    table.binaryWith("payload", 0, false).nullable();
    table.dateTime("changed_at", 3)
        .useCurrent()
        .onUpdate(Expression::new("NOW(3)"));
}
```

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

`morphs()` 和 `nullableMorphs()` 使用應用程式設定的多型鍵類型。可用 `numericMorphs`、`uuidMorphs`、`ulidMorphs` 及允許空值的變體明確選擇 ID 類型。如需指定索引名稱或 MySQL 欄位位置，可向型別專用方法、`morphsWith` 或 `nullableMorphs` 傳入對應參數。 可直接用 `Schema::dropColumns(table, columns)` 移除指定欄位。

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

async fn remove_legacy_columns() -> DatabaseResult<()> {
    Schema::dropColumns("attachments", &["legacy_name", "legacy_rank"]).await
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

#### 自動遞增欄位與明確主鍵 {#auto-incrementing-columns-and-explicit-primary-keys}

`integerIncrements("id")` 是 `increments("id")` 的別名，兩者皆建立無號整數。`autoIncrement()` 修飾符保留欄位型別的有號或無號設定。在 MySQL 中，資料表層級的 `primary` 宣告決定完整主鍵欄位。InnoDB 要求自動遞增欄位是某個索引的第一欄。

```rust
use openrust::database::schema::Blueprint;

fn define_import_entries(table: &mut Blueprint) {
    table.integer("entry_id").autoIncrement().startingValue(500);
    table.integer("import_id");
    table.primary(&["entry_id", "import_id"], None);
}
```

#### 條件式 Schema 變更 {#conditional-schema-changes}

使用 `whenTableHasColumn`、`whenTableDoesntHaveColumn`、`whenTableHasIndex`、`whenTableDoesntHaveIndex`，只在目錄檢查符合條件時執行 Blueprint。目錄檢查和後續的 Schema 陳述式是兩次操作。

```rust
use openrust::{
    database::{DatabaseResult, schema::KeySelector},
    support::facades::Schema,
};

async fn prepare_imports() -> DatabaseResult<()> {
    Schema::whenTableDoesntHaveColumn("imports", "status", |table| {
        table.string("status", 24).default("pending");
    })
    .await?;

    Schema::whenTableDoesntHaveIndex(
        "imports",
        KeySelector::Columns(&["status", "source"]),
        |table| {
            table.unique(&["status", "source"], Some("imports_status_source"));
        },
        Some("unique"),
    )
    .await
}
```

`hasIndexWith` 與兩個索引回呼接受依名稱或精確欄位順序比對的 `KeySelector`，可選類型包括 `primary`、`unique` 和 `btree`。`hasForeignKey` 使用相同的選擇類型比對外鍵名稱或本機欄位。

#### 建立和刪除資料庫 {#creating-and-dropping-databases}

`createDatabase` 在所選 MySQL 伺服器建立資料庫，並採用連線設定的字元集與定序。資料庫同名時會回傳錯誤。`dropDatabaseIfExists` 刪除指定資料庫，資料庫已不存在時仍會成功。

在 PostgreSQL 中，這兩個方法在交易外執行資料庫層級的 DDL。建立時，連線設定若提供字元集，就用它指定資料庫編碼。建立前先連線至一個已存在的資料庫。刪除時，請連線至同一伺服器上的另一個資料庫。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn create_reporting_database() -> DatabaseResult<()> {
    Schema::createDatabase("reporting_staging").await?;
    Ok(())
}

async fn drop_reporting_database() -> DatabaseResult<()> {
    Schema::dropDatabaseIfExists("reporting_staging").await?;
    Ok(())
}
```

SQLite 使用檔案路徑。目標路徑已存在時會建立失敗，原資料庫會保留。刪除檔案前請關閉使用它的連線。

#### 刪除全部資料表或檢視表 {#dropping-all-tables-or-views}

`dropAllTables()` 刪除所選連線目前 Schema 中的永久資料表，`dropAllViews()` 刪除同一 Schema 中的檢視表。即使設定了資料表前置詞，方法仍會處理該類型的全部物件。操作獨立資料庫時，請使用具名連線。

PostgreSQL 採用與 [`db:wipe` 相同的 Schema 範圍和排除規則](#rebuilding-mysql-with-fresh)。用 `dropAllTypes()` 刪除使用者定義的型別與定義域。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn clear_reporting_schema() -> DatabaseResult<()> {
    let mut schema = Schema::connection("reporting_staging")?;
    schema.dropAllViews().await?;
    schema.dropAllTables().await
}
```

刪除資料表時會保留檢視表。引用已刪除資料表的檢視表可能需要刪除或重建。MySQL 刪除資料表時會暫時關閉寫入工作階段的外鍵檢查，並在操作後恢復原本的設定。

#### 檢查資料表目錄 {#inspecting-tables}

`getTables(None)` 列出所有可見使用者 Schema 的資料表。使用 `Some(name)` 限定一個 Schema，或使用 `getTablesIn(&[...])` 限定多個。檢查清除結果時應明確指定已清除的 Schema。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_tables(schema: &str) -> DatabaseResult<()> {
    let names = Schema::getTableListing(Some(schema)).await?;
    for name in names {
        println!("{name}");
    }
    Ok(())
}
```

`getTableListing` 回傳限定資料表名稱。需要不含 Schema 的名稱時，使用 `getTableListingWith(schema, false)`。資料表記錄回傳資料庫目錄中的名稱，並在資料庫提供時附上大小及儲存中繼資料。

#### 解析資料表名稱參照 {#parsing-table-references}

`parseSchemaAndTable(reference, fallback)` 在連線加入資料表前置詞之前分割邏輯資料表名稱。限定名稱優先採用明確指定的 Schema。僅傳資料表名稱時，可用 `SchemaFallback::None`、`SchemaFallback::Current` 或 `SchemaFallback::Named(name)`。用 `Schema::connection(name)` 選擇連線。資料表名稱參照最多包含兩個非空部分。

```rust
use openrust::{
    database::{DatabaseResult, schema::SchemaFallback},
    support::facades::Schema,
};

async fn list_tables_in_same_schema(reference: &str) -> DatabaseResult<Vec<String>> {
    let (schema, _) = Schema::parseSchemaAndTable(reference, SchemaFallback::Current)?;
    Schema::getTableListing(schema.as_deref()).await
}
```

#### 檢查單欄 {#inspecting-a-column}

`Schema::getColumn` 回傳指定欄位的中繼資料。比對欄位名稱時忽略大小寫。欄位不存在時 Rust 回傳 `None`，資料庫錯誤透過 `DatabaseResult` 傳遞。

在 PostgreSQL 中，欄位目錄回傳 identity 與 serial 標記、主鍵成員關係及生成運算式。未限定的資料表名稱使用伺服器目前的 Schema。可傳入限定名稱檢查其他 Schema 的資料表。

```rust
use openrust::{
    database::{DatabaseResult, schema::Column},
    support::facades::Schema,
};

async fn inspect_title() -> DatabaseResult<Option<Column>> {
    Schema::getColumn("posts", "title").await
}
```

#### 檢查 Schema 和檢視表 {#inspecting-schemas-and-views}

`getSchemas()` 列出可見的 Schema，並標記目前使用的 Schema。SQLite 也會回傳各 Schema 的檔案路徑。在 MySQL 上，`getViews(None)` 列出所有可見使用者 Schema 的檢視表。在 SQLite 上，它查詢目前寫入工作階段中的所有 Schema。可用 `getViews(Some(name))` 指定一個，或用 `getViewsIn(&[...])` 指定多個。 `getCurrentSchemaName()` 和 `getCurrentSchemaListing()` 依寫入連線設定回傳目前的 Schema。MySQL 回傳設定的資料庫名稱，SQLite 回傳 `main`。這兩個方法讀取連線設定，無須執行 SQL。

在 PostgreSQL 中，`getCurrentSchemaListing()` 依設定順序回傳 `search_path`，並將 `$user` 解析為所選端點的使用者名稱。可使用逗號分隔的字串或 Schema 名稱陣列。`getCurrentSchemaName()` 回傳第一項。資料庫目錄方法查詢所選寫入連線，`getSchemas()` 標記伺服器實際使用的目前 Schema。

`Schema::getTypes(None)` 列出 PostgreSQL 使用者 Schema 中的型別。可傳入 `Some(schema)`，或用 `Schema::getTypesIn(&[...])` 限定範圍。每項包含限定名稱、型別、類別，以及標示自動產生陣列和多範圍型別的 `implicit` 欄位。其他驅動會回傳不支援的 Schema 操作錯誤。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn inspect_status_view() -> DatabaseResult<()> {
    if Schema::hasView("status_report").await? {
        let current = Schema::getCurrentSchemaName()?;
        for view in Schema::getViews(Some(&current)).await? {
            println!("{}", view.schema_qualified_name);
        }
    }
    Ok(())
}
```

`hasView("name")` 檢查目前的 Schema，使用限定名稱可檢查其他 Schema。連線設定的資料表前置詞會套用於 `hasView`。檢視表記錄會回傳資料庫目錄中的名稱，並在可讀取時提供定義。

#### 同時重新命名與修改欄位 {#renaming-and-changing-a-column}

在 MySQL 上，將 `renameTo()` 和 `change()` 搭配使用，可在同一條 ALTER 中重新命名欄位並更新定義。欄位定義以目前欄位名稱開頭，並明確寫出要保留的修飾符。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn revise_import_labels() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.string("old_label", 100).nullable().renameTo("label").change();
    })
    .await
}
```

只改名時使用 `renameColumn`。在 SQLite 上，使用 `renameColumn` 改名，再單獨用 `change()` 更新欄位定義。

#### 修改欄位與索引 {#changing-columns-and-their-indexes}

僅呼叫 `change()` 時會保留索引及外部索引鍵。用 `uniqueWith(false)` 或 `indexWith(false)` 搭配 `change()`，可依慣例名稱刪除索引。同一次操作重新命名欄位時，刪除會依舊欄名查找索引，新索引則使用新欄名。

使用 `indexWith("name")`、`uniqueWith("name")`、`fullTextWith("name")` 或 `spatialIndexWith("name")` 新增具名欄位索引。刪除自訂名稱的索引時，使用對應的資料表層級刪除方法。MySQL 要求保留外部索引鍵所依賴的索引，直到明確刪除條件約束或提供另一個可用索引。

```rust
use openrust::database::schema::Blueprint;

fn change_region_code(table: &mut Blueprint) {
    table.string("postal_code", 30)
        .uniqueWith(false)
        .indexWith("region_lookup")
        .renameTo("region_code")
        .change();
}
```

#### 替換 MySQL 主鍵 {#replacing-a-mysql-primary-key}

在同一個 MySQL 資料表變更回呼中依序緊接呼叫 `dropPrimary()` 和 `primary(...)`，OpenRust 會產生一條 ALTER TABLE。自動遞增欄位須為某個可用索引的第一欄，例如下例的複合主鍵。儲存引擎支援時，可在新主鍵宣告中指定 `inplace()` 或 `lock("none")`。

```rust
use openrust::database::schema::Blueprint;

fn replace_import_key(table: &mut Blueprint) {
    table.dropPrimary();
    table.primary(&["id", "tenant_id"], None);
}
```

#### 同時新增自動遞增欄位及主鍵 {#adding-an-auto-increment-column-and-a-primary-key}

在既有 MySQL 資料表的同一個變更回呼中，宣告新的自動遞增欄位後，緊接著宣告包含該欄位的主鍵。OpenRust 會產生一條 ALTER TABLE。自動遞增欄位缺少其他支援索引時，應放在主鍵的第一欄。若已有主鍵，請在宣告新欄位前緊接呼叫 `dropPrimary()`。

```rust
use openrust::database::schema::Blueprint;

fn add_import_key(table: &mut Blueprint) {
    table.bigIncrements("id");
    table.primary(&["id", "tenant_id"], None);
}
```

欄位和索引同時指定鎖定模式時，兩者必須一致。MySQL 會檢查合併操作選用的演算法及鎖定模式。

#### 依欄位清單移除索引 {#dropping-indexes-by-columns}

將有序欄位清單傳給 `dropIndexColumns`、`dropUniqueColumns`、`dropFullTextColumns` 或 `dropSpatialIndexColumns`，可移除該類型下使用預設名稱的索引。自訂名稱的索引使用對應的依名稱移除方法。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn remove_import_lookup() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.dropIndexColumns(&["status", "created_at"]);
    })
    .await
}
```

#### 檢查 SQLite 主鍵索引 {#sqlite-primary-indexes}

SQLite 的 `getIndexes()` 回傳主鍵底層索引的實際名稱。以 rowid 為基礎且沒有獨立索引的主鍵顯示為 `primary`。名為 `primary` 的一般索引可以與它同時出現。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn primary_columns() -> DatabaseResult<Vec<Option<String>>> {
    let indexes = Schema::getIndexes("deliveries").await?;
    Ok(indexes
        .into_iter()
        .find(|index| index.primary)
        .map(|index| index.columns)
        .unwrap_or_default())
}
```

用 `primary` 旗標識別主鍵，欄位按主鍵定義順序回傳。`hasIndex("deliveries", "primary")` 也能識別具有實際索引名稱的主鍵。

#### 原生索引運算式 {#raw-indexes}

`rawIndex(expression, name)` 使用資料庫方言的 SQL 鍵清單建立具名索引。MySQL 要求每個函式索引鍵額外包一層括號。SQLite 可直接寫 `lower(label)`。SQL 由遷移程式碼提供。用 `dropIndex(name)` 移除索引。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn index_labels() -> DatabaseResult<()> {
    Schema::table("imports", |table| {
        table.rawIndex("(lower(label))", "imports_lower_label");
    })
    .await
}
```

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

#### 修改 PostgreSQL 欄位 {#postgresql-column-changes}

用 `change()` 宣告欄位的新型別、空值約束、預設值及註解。需要保留的修飾符應重新宣告。現有索引會保留，也可明確要求變更。PostgreSQL 需要明確轉換時，用 `using()` 傳入可信 SQL 運算式來轉換既有值。舊預設值無法轉換成新型別時，先移除舊預設值，再於新定義上宣告所需的預設值。

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext, query::Expression};

async fn update_amount(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.getSchemaBuilder().table("finance.fixture", |table| {
        table.decimal("amount", 7, 2)
            .using(Expression::new("amount::numeric / 100"))
            .default(1)
            .nullable()
            .comment("owner's amount")
            .change();
    }).await
}
```

#### PostgreSQL 生成欄位與 Identity {#postgresql-generated-and-identity}

PostgreSQL 12 以上用 `storedAs()` 定義儲存生成欄位，18 以上用 `virtualAs()` 定義虛擬生成欄位。PostgreSQL 13 以上可對儲存生成欄位用 `dropExpression().change()` 移除運算式，保留目前值作為一般資料。PostgreSQL 拒絕移除虛擬生成運算式，Schema Builder 拒絕透過 `change()` 取代生成運算式。

用 `generatedAs()` 定義 `BY DEFAULT` identity，加上 `always()` 即要求使用生成值。用 `generatedAsWith(Expression)` 傳入 `start with 10` 等可信序列選項。明確指定 identity 時，由 Blueprint 另外定義主鍵。在 `change()` 中明確宣告會依新定義重建序列，省略 identity 修飾符可以移除 identity。

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext, query::Expression};

async fn create_generated_table(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.getSchemaBuilder().create("finance.fixture", |table| {
        table.bigInteger("seq")
            .generatedAsWith(Expression::new("start with 10"))
            .always();
        table.integer("base");
        table.integer("saved").storedAs("base * 2");
        table.integer("computed").virtualAs("base + 1");
    }).await
}
```

#### PostgreSQL JSON 選擇器生成欄位 {#postgresql-generated-json-selectors}

用 `storedAsJson()` 或 `virtualAsJson()` 定義生成欄位，使用 `->` 存取巢狀鍵、方括號存取陣列索引。PostgreSQL 將路徑末項擷取成文字。路徑缺少或 JSON null 會傳回 SQL NULL，路徑可能沒有值時，請宣告 `nullable()`。`01` 這類路徑項目是物件鍵，`[-1]` 選擇陣列最後一項。儲存欄位與虛擬欄位沿用[上文的 PostgreSQL 版本要求](#postgresql-generated-and-identity)。

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn create_json_columns(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.getSchemaBuilder().create("finance.fixture", |table| {
        table.integer("id");
        table.jsonb("payload");
        table.string("label", 80).storedAsJson("payload->items[0]->label").nullable().index();
        table.string("locale", 20).virtualAsJson("payload->settings->locale").nullable();
    }).await
}
```

#### PostgreSQL 索引方法與重新命名 {#postgresql-index-methods}

使用 `algorithm("brin")` 指定 PostgreSQL 索引存取方法。由伺服器檢查方法及其是否支援目標欄位或運算式。可與 `online()` 一起使用。`renameIndex()` 從帶有 Schema 的資料表名稱繼承 Schema，也接受明確限定 Schema 的舊索引名稱。資料表與舊索引名稱都未限定 Schema 時，PostgreSQL 依 `search_path` 尋找舊索引。重新命名後的索引仍在原 Schema。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn index_references() -> DatabaseResult<()> {
    Schema::table("archive.documents", |table| {
        table.index(&["external_id"], Some("documents_reference_idx")).algorithm("brin");
        table.renameIndex("documents_reference_idx", "documents_reference_brin");
    })
    .await
}
```

#### PostgreSQL 唯一約束 {#postgresql-unique-constraints}

使用 `unique()` 建立具名 PostgreSQL 約束。PostgreSQL 15 及以上可用 `nullsNotDistinct(true)` 將 NULL 視為相等，以 `nullsNotDistinct(false)` 明確保留 NULL 相異行為。`deferrable(true).initiallyImmediate(false)` 將檢查延後到交易提交。`online()` 或明確指定 `algorithm("btree")` 時會先建立支援索引，再把索引附加到約束。`dropUnique()` 移除約束及其索引，也能移除早期 OpenRust 遷移在同一資料表上建立的獨立唯一索引。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn require_unique_reference() -> DatabaseResult<()> {
    Schema::table("documents", |table| {
        table
            .unique(&["external_id"], Some("documents_external_unique"))
            .nullsNotDistinct(true)
            .deferrable(true)
            .initiallyImmediate(false);
    })
    .await
}
```

#### PostgreSQL 線上索引 {#postgresql-online-indexes}

一般索引、唯一索引、全文索引和向量索引可呼叫 `online()`，使用 PostgreSQL 的 `CREATE INDEX CONCURRENTLY`。建置可等待現有交易，其他工作階段仍可繼續寫入。

```rust
use openrust::support::facades::Schema;

Schema::table("documents", |table| {
    table.vectorIndex("embedding", Some("documents_vector_search")).online();
}).await?;
```

遷移中使用線上索引時，覆寫 [`withinTransaction()`](#migration-transactions) 並回傳 `false`。使用連線的 Schema 建構器。包含線上索引的 Blueprint 會逐一以自動提交模式執行。需要原子套用的變更應另用一個 Blueprint。若框架管理的交易仍在進行，這個 Blueprint 會在執行任何陳述式之前遭拒絕。並行建置失敗可能留下無效索引，可刪除後再試。

#### 修改及刪除 SQL Server 欄位 {#sql-server-column-changes}

SQL Server 的 `change()` 會先移除現有預設約束，再修改欄位型別與空值約束。Laravel 在 SQL Server 的改欄位陳述式中也會省略 `default()`。如需設定新預設值，請在同一遷移中另用 `ALTER TABLE … ADD DEFAULT … FOR …`。

用 `dropColumn()` 刪除欄位時，建構器會先移除這些欄位的預設約束與欄位層級檢查約束。引用這些欄位的資料表層級約束需要先明確移除。

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn update_columns(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.getSchemaBuilder().table("finance.fixture", |table| {
        table.string("label", Some(20)).nullable().change();
        table.dropColumn(&["score"]);
    }).await
}
```

#### 重新命名 SQL Server 資料表、欄位與索引 {#sql-server-renaming}

在 SQL Server 上，`rename()`、`renameColumn()` 和 `renameIndex()` 使用 `sys.sp_rename`。資料表重新命名在原 Schema 內完成，連線的資料表前綴套用到新舊名稱。索引會保留原索引欄位和資料。應用程式碼和儲存的 SQL 若仍引用舊名稱，需要一併更新。

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn rename_fixture(cx: &mut MigrationContext) -> DatabaseResult<()> {
    let mut schema = cx.getSchemaBuilder();
    schema.table("finance.fixture", |table| {
        table.renameColumn("score", "score_new");
        table.renameIndex("ix_fixture", "ix_new");
    }).await?;
    schema.rename("finance.fixture", "finance.archive").await
}
```

#### SQL Server 計算欄位 {#sql-server-computed-columns}

用 `computed(name, expression)` 傳入可信 SQL 運算式，由 SQL Server 推斷欄位型別。加上 `persisted()` 會儲存計算結果。對既有計算欄位，`persisted().change()` 啟用持久化，單獨呼叫 `change()` 則恢復為讀取時計算。兩種切換都沿用既有運算式，由 SQL Server 判斷該運算式能否持久化。

```rust
use openrust::database::{DatabaseResult, migrations::MigrationContext};

async fn persist_total(cx: &mut MigrationContext) -> DatabaseResult<()> {
    cx.getSchemaBuilder().table("finance.fixture", |table| {
        table.computed("twice", "quantity * 2").persisted().change();
    }).await
}
```

#### SQL Server 線上索引 {#sql-server-online-indexes}

在支援線上索引操作的 SQL Server 版本上，對 `index()` 或 `unique()` 呼叫 `online()` 以使用 `WITH (ONLINE = ON)`。保留預設的[遷移交易](#migration-transactions)。使用 `dropForeign(name)` 依約束名稱移除外鍵。

```rust
schema.table("dbo.fixture_entries", |table| {
    table.index(&["score"], Some("fixture_search")).online();
    table.unique(&["parent_id"], Some("fixture_unique")).online();
    table.dropForeign("fixture_parent");
}).await?;
```

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

#### 外鍵約束命名 {#naming-foreign-keys}

用 `constrainedWith(table, column, index_name)` 個別覆寫外鍵的參照表、參照欄位或約束名稱。省略參照表或參照欄位時沿用模型提供的值。未提供模型時，參照欄位預設是 `id`，表名根據本地欄位名稱最後一個 `_<參照欄位>` 之前的部分取複數。刪除具名約束時，向 `dropForeign` 傳入約束名稱。

```rust
Schema::table("attachments", |table| {
    table.foreignIdFor::<PhotoAsset>(Some("photo_ref"))
        .nullable()
        .constrainedWith(None, None, Some("attachments_photo"))
        .nullOnDelete();
}).await?;
```

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

#### PostgreSQL 外鍵 {#postgresql-foreign-keys}

使用 Schema 限定名稱新增跨 Schema 外鍵。PostgreSQL 預設驗證既有資料列。`notValid()` 會略過這次掃描，之後新增或修改外鍵值時仍會檢查。`deferrable(true).initiallyImmediate(false)` 將檢查延後到提交交易。`initiallyImmediate(true)` 預設在各陳述式結束時檢查。使用 `dropForeign(name)` 移除具名外鍵，或以 `dropForeignColumns(&[...])` 移除按慣例命名的外鍵。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn constrain_books() -> DatabaseResult<()> {
    Schema::table("library.books", |table| {
        table
            .foreign(&["author_id"], Some("books_author_foreign"))
            .references("id")
            .on("authors.records")
            .deferrable(true)
            .initiallyImmediate(false)
            .notValid();
    })
    .await
}
```

#### 檢查外鍵 {#inspecting-foreign-keys}

用 `getForeignKeys` 查看本地欄位、目標 Schema 和資料表、目標欄位以及更新和刪除動作。複合外鍵的欄位依照約束定義順序回傳。

在 PostgreSQL 中，外鍵目錄回傳約束名稱及參照的 Schema。複合外鍵的本地欄位與目標欄位依約束順序配對，也適用於跨 Schema 參照。

```rust
use openrust::{database::DatabaseResult, support::facades::Schema};

async fn author_schema() -> DatabaseResult<Option<String>> {
    Ok(Schema::getForeignKeys("books")
        .await?
        .into_iter()
        .find(|key| key.columns == ["author_id"])
        .and_then(|key| key.foreign_schema))
}
```

SQLite 回傳約束所在的 Schema，例如 `main` 或附加資料庫名稱。它的目錄不提供約束名稱。宣告省略目標欄位名稱時，相應項目為 `None`，SQLite 使用目標資料表的主鍵。

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

## 事件 {#events}

在服務提供者中註冊監聽器。批次事件為 `MigrationsStarted` 和 `MigrationsEnded`。單項執行觸發 `MigrationStarted` 和 `MigrationEnded`。遷移主動略過時觸發 `MigrationSkipped`，執行或回滾沒有待處理項目時觸發 `NoPendingMigrations`。預演觸發批次事件，略過單項執行事件。

`MigrationEnded` 在遷移方法成功後、更新帳本前觸發。訂閱 `contracts::database::events::MigrationEvent::NAME` 可接收遷移生命週期事件。`SchemaDumped`、`MigrationsPruned` 和 `SchemaLoaded` 各自訂閱。 `DatabaseRefreshed` 在 `migrate:fresh` 或 `migrate:refresh` 完成遷移後、選擇性的資料填充前觸發。

```rust
use std::sync::Arc;

use openrust::{
    contracts::events::Event as EventPayload,
    database::events::MigrationEnded,
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    support::facades::Event,
};

fn record_migration<'a>(
    event: &'a dyn EventPayload,
    _: &'a mut EventContext<'_>,
) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
    Box::pin(async move {
        if let Some(ended) = event.downcast_ref::<MigrationEnded>() {
            println!("{:?} {}", ended.method, ended.name);
        }
        Ok(None)
    })
}

fn register_migration_listener() -> EventResult<()> {
    Event::listen(MigrationEnded::NAME, Arc::new(record_migration), Delivery::Sync)
}
```
