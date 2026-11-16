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
