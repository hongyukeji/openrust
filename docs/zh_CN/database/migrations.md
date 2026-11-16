# 迁移

## 生成迁移

用 `make:migration` 在 `database/migrations` 生成带时间戳的 Rust 源文件。可以用 `--create=flights` 或 `--table=flights` 显式选择模板。

```sh
./artisan make:migration create_flights_table
```

OpenRust 在构建时自动发现 `database/migrations/` 中的 Rust 迁移。手写文件和 `make:migration` 生成的文件都使用 `YYYY_MM_DD_HHMMSS_name.rs` 命名，并提供实现 `Migration` 的 `Definition`。重新构建后，`bootstrap/app.rs` 的 `withMigrations` 会接入自动生成的清单。文件名决定迁移账本名称，`--path` 从已编译的迁移中选择。生成到自定义目录的迁移需另行接入应用。

## 迁移结构

在 `up` 中执行迁移，在 `down` 中撤销。通过传入的 `MigrationContext` 执行 Schema 和 SQL 操作，预演时迁移器才能收集语句。实现 `getConnection` 可指定迁移连接，实现 `shouldRun` 可按条件跳过迁移。

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

## 运行迁移

第一次连接前先创建 SQLite 数据库文件，或配置 MySQL 数据库和凭据。`migrate` 按文件名顺序执行待运行的迁移，并将每项成功记录到配置的账本。

```sh
./artisan migrate --pretend
./artisan migrate
./artisan migrate:status
```

用 `--database` 选择账本连接，用 `--step` 为每个迁移单独分配批次。在空库上，`--pretend` 会准备账本并输出 SQL，迁移本身不执行，账本也不增加记录。需要重建表的 SQLite 变更依赖实际目录元数据，在预演中会返回错误。

生产环境中改变结构的命令会要求确认。部署流程已确认变更时可传 `--force`。MySQL 和 SQLite 的单项迁移可能包含多条 DDL，其中一条失败时，先前成功的语句可能已生效。重试前先检查失败的迁移。

## 回滚迁移

`migrate:rollback` 按逆序撤销最近一批迁移。用 `--step` 指定条数，或用 `--batch` 指定批次。

```sh
./artisan migrate:rollback --pretend
./artisan migrate:rollback --step=1
```

用 `migrate:reset` 回滚全部迁移，用 `migrate:refresh` 回滚后重新执行。账本存在时，`migrate:fresh` 删除所选数据库中的所有表，包括使用其他前缀的表，然后重新迁移。传入 `--drop-views` 可同时删除视图。

## 表

用 `Schema::create("books", |table| { … }).await?` 在默认连接上建表，用 `Schema::connection("reporting")?` 选择具名连接。已持有连接时，调用 `getSchemaBuilder()` 获取同一套接口。

用 `table` 改表，用 `rename` 重命名，用 `dropIfExists` 按存在条件删除。用 `hasTable` 和 `getTables` 查看数据库。Schema 操作使用写连接。

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

### MySQL 表选项 {#mysql-table-options}

在 `Schema::create` 中使用 `engine`、`charset` 和 `collation` 配置表。MySQL 优先采用 Blueprint 的显式设置，其次采用连接对应的默认值。`comment` 可用于建表和改表。用 `getTables()` 读取实际的存储引擎、排序规则和注释。

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

用列级 `charset` 和 `collation` 覆盖文本编码或比较规则。SQLite 应用列级排序规则，并忽略 MySQL 专用的表设置。

### 临时表 {#temporary-tables}

使用 `Blueprint::temporary()` 前先借用 Schema 会话。用会话的 Schema Builder 建表，再通过同一会话执行 SQL。`close().await?` 会先清理受管理的临时表，再归还连接。会话被丢弃时，清理操作持有原物理连接，直到可以安全归还。

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

SQLite 的同一会话可读取临时表元数据。MySQL 的 `hasTable()` 可检查会话管理的临时表，详细列信息可通过会话执行 `SHOW COLUMNS` 查看。

## 列

定义闭包接收可变的 `Blueprint`。列修饰符接在列定义后，例如 `table.string("isbn", 20).nullable()`。`id()` 创建自增主键，`timestamps()` 创建允许为空的创建时间和更新时间。长度、精度和小数位数由 Rust 参数显式指定。

用 `change` 指定替换后的列定义，包括要保留的修饰符。`renameColumn` 和 `dropColumn` 使用驱动原生操作。`getColumns`、`getColumnListing` 和 `getColumnType` 从数据库目录读取列结构。

SQLite 对需要重建的列或外键变更执行表重建。请在活动事务之外，通过连接的 Schema Builder 发起重建。重建拥有自己的事务，并恢复连接的外键设置。会隐式提交的 MySQL DDL 也通过连接的 Schema Builder 执行。

### 更多列类型 {#additional-column-types}

用 `tinyText`、`year`、`ipAddress`、`macAddress` 和 `r#enum` 声明对应的列。`year` 配合 `useCurrent()` 时默认采用数据库当前年份。MySQL 使用原生 ENUM 和 SET。SQLite 用 CHECK 约束枚举值，对 SET 返回不支持。

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

`binary()` 声明 BLOB。`binaryWith(name, length, fixed)` 在 MySQL 使用 BINARY 或 VARBINARY，在 SQLite 使用 BLOB。MySQL 的 `tinyIncrements`、`smallIncrements`、`mediumIncrements` 及对应无符号整数方法采用各自的整数宽度，SQLite 使用 INTEGER。两个驱动的 `Tz` 方法沿用各自的时间类型。

### 生成列 {#generated-columns}

用 `virtualAs` 声明读取时计算的列，用 `storedAs` 保存计算结果。两者都接受可信 SQL 表达式。`virtualAsJson` 和 `storedAsJson` 可使用箭头分隔的 JSON 路径。生成列需要指定类型，由数据库根据其他列计算。

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

`getColumns()` 返回生成模式和表达式。MySQL 和 SQLite 建表时都支持这两种模式。SQLite 原生 ALTER 可追加虚拟列。存储型生成列可在建表时定义，也可通过显式重建表的迁移添加。

### 空间列 {#spatial-columns}

用 `geometry` 或 `geography` 声明空间列。可选子类型对应 MySQL 的 `point`、`multipolygon` 等类型。`geometry` 的默认 SRID 为 0，`geography` 为 4326。传入 `Some(0)` 时省略 SRID 限制。

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

MySQL 校验声明的子类型和非零 SRID。非空空间列可以通过 `spatialIndex()` 声明索引。SQLite 声明 `geometry` 类型并支持存入二进制值，空间函数和空间索引需要具备空间能力的数据库。

### MySQL 列修饰符

用 `invisible()` 让 `SELECT *` 省略该列，仍可按名称读取。行中的其他值发生变更时，`useCurrentOnUpdate()` 会更新 timestamp 或 datetime 列。如果插入时也要默认使用当前时间，请同时调用 `useCurrent()`。在已有 MySQL 表中添加或修改列时，用 `first()` 或 `after("column")` 指定位置。建表时按所需顺序声明列。

```rust
Schema::table("books", |table| {
    table.string("internal_note", 120).nullable().invisible();
    table.timestamp("reviewed_at", 6).useCurrent().useCurrentOnUpdate();
    table.string("edition", 20).nullable().after("title");
}).await?;
```

## 索引

用 `index`、`unique` 或 `primary` 传入列切片。可以指定名称，或传 `None` 使用约定名称。用 `getIndexes`、`hasIndex` 和 `renameIndex` 查看或重命名索引。MySQL 支持在兼容列上建全文索引。

### 外键约束

`foreignId("author_id").constrained()` 引用 `authors.id`。需要明确指定引用表和列时，使用 `constrainedOn`。先设置列修饰符，再定义外键，最后设置 `cascadeOnDelete()` 等引用动作。用 `getForeignKeys` 查看约束。SQLite 目录提供约束列，`dropForeignColumns` 可按这些列删除表级外键。

#### 列组与删除别名 {#column-groups-and-drop-aliases}

`morphs()` 和 `nullableMorphs()` 使用应用配置的多态键类型。可用 `numericMorphs`、`uuidMorphs`、`ulidMorphs` 及允许空值的变体显式选定 ID 类型。需要指定索引名或 MySQL 列位置时，可向类型专用方法、`morphsWith` 或 `nullableMorphs` 传入对应参数。

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

`dropMorphs` 先删复合索引，再删两列。创建时指定索引名的，删除时传入同一名称。`dropConstrainedForeignId` 先删外键，再删列。`dropSoftDeletes(None)` 和 `dropSoftDeletesTz(None)` 删除默认 `deleted_at`，传 `Some(name)` 可指定列名。

在 `Schema::table` 中用 `table.after("title", |table| { … })`，可把多列按声明顺序放到 MySQL 已有列之后。SQLite 会返回方言不支持错误。

#### 默认值表达式 {#default-expressions}

通过 `DB::raw()` 或 `Expression::new()` 把可信的 SQL 表达式传给 `default()`。插入数据时省略该列，数据库会求值。普通字符串会作为字面量默认值。

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

默认值表达式不支持参数绑定。按数据库要求编写表达式，在需要时加上括号。数据库会检查该列类型和 Schema 操作是否支持此表达式。

#### MySQL 即时列变更与锁模式 {#instant-column-operations}

新增列或通过 `change()` 修改列时，可用 `instant()` 请求即时算法。用 `lock()` 指定列操作的锁模式，支持 `none`、`shared`、`exclusive` 和 `default`。算法与锁能否组合由 MySQL 判断。操作不兼容时返回数据库错误，框架不会改用其他算法重试。

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

在 MySQL 上用 `from()` 或 `startingValue()` 设置自增列的起始值。实际可用的下一个值由 MySQL 结合已有数据决定。SQLite 使用原生序列，使用 MySQL 在线 DDL 修饰符时返回方言错误。

#### SQLite 内联约束 {#sqlite-inline-constraints}

变更 SQLite 列时，已有的内联主键、UNIQUE、CHECK 和 REFERENCES 子句会保留。重建后的表仍有自增主键时，也会恢复其自增序列。传入 `change()` 的列定义决定类型、是否可空及默认值。

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

用 `dropForeignColumns` 按本地列删除外键，或用 `dropForeign` 删除有显式名称的外键。两种方法都支持内联 REFERENCES 和表级外键。用 `dropPrimary` 删除内联或表级主键。其余内联约束继续生效。

#### 切换外键检查 {#toggling-foreign-key-constraints}

用 `Schema::withoutForeignKeyConstraints` 在同一写会话中暂停外键检查。通过传入的会话执行 SQL，通过它的 `getSchemaBuilder()` 操作 Schema。作用域结束时会恢复进入前的设置。

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

需要手动切换时，调用 `Schema::session().await?` 或具名连接构建器的 `session().await?` 获取写会话，再在此会话上调用 `disableForeignKeyConstraints` 或 `enableForeignKeyConstraints`，最后用 `close().await?` 归还。SQLite 表重建继续使用连接的 Schema Builder，由它管理重建所需的会话和事务。
