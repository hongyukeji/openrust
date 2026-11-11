# 迁移

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

## 索引

用 `index`、`unique` 或 `primary` 传入列切片。可以指定名称，或传 `None` 使用约定名称。用 `getIndexes`、`hasIndex` 和 `renameIndex` 查看或重命名索引。MySQL 支持在兼容列上建全文索引。

### 外键约束

`foreignId("author_id").constrained()` 引用 `authors.id`。需要明确指定引用表和列时，使用 `constrainedOn`。先设置列修饰符，再定义外键，最后设置 `cascadeOnDelete()` 等引用动作。用 `getForeignKeys` 查看约束。SQLite 目录提供约束列，`dropForeignColumns` 可按这些列删除表级外键。
