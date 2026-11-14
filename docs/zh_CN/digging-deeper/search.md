# 搜索

## 全文搜索 {#full-text-search}

使用 MySQL 兼容连接，创建列清单与查询一致的全文索引。查询构建器将搜索词绑定为参数。`whereFullText` 使用自然语言模式。用 `whereFullTextWith` 可选择 `Boolean` 或 `QueryExpansion`。

示例使用已有数据库连接 `db`，创建表及全文索引后执行分页搜索。

```rust
use openrust::database::query::FullTextOptions;

db.getSchemaBuilder().create("articles", |table| {
    table.id();
    table.string("title", 150);
    table.text("body");
    table.fullText(&["title", "body"], None);
}).await?;

let results = db.table("articles")
    .whereFullTextWith(&["title", "body"], "+openrust -draft", FullTextOptions::Boolean)
    .orderBy("id", "asc")
    .paginate(10)
    .await?;
```

Eloquent 查询和嵌套条件组也支持该谓词。记录变更提交后，数据库更新原生索引。SQLite 使用该谓词时返回查询错误。

## 数据库模型搜索 {#database-engine}

为 Eloquent 模型实现 `Searchable`，即可搜索该模型的表。用 `FullText` 声明全文索引列，用 `Prefix` 匹配前缀，用 `Contains` 匹配包含内容。搜索条件放在同一个条件组内，模型作用域和附加过滤适用于每条匹配。下例在前一示例的模型上增加了 `slug` 列。

```rust
use openrust::database::eloquent::{SearchColumn, Searchable};

impl Searchable for Article {
    fn searchableColumns() -> &'static [SearchColumn] {
        &[
            SearchColumn::FullText("title"),
            SearchColumn::FullText("body"),
            SearchColumn::Prefix("slug"),
        ]
    }
}

let page = Article::searchOn(&db, "openrust")
    .orderBy("id", "asc")
    .paginate(10)
    .await?;
```

在应用内用 `Article::search(term)?` 解析模型连接。用 `withTrashed` 或 `onlyTrashed` 调整软删除过滤。空搜索词保留模型的常规过滤。数据库在写事务中维护索引。

## 搜索能力 {#search-capabilities}

数据库引擎支持 MySQL 兼容的全文谓词和 Eloquent 模型搜索。PostgreSQL 全文查询、向量索引、嵌入生成、重排和独立 Scout 引擎各自需要对应驱动。
