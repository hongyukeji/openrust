# 搜尋

## 全文搜尋 {#full-text-search}

使用 MySQL 相容連線，建立欄位清單與查詢一致的全文索引。查詢建構器將搜尋字串繫結為參數。`whereFullText` 使用自然語言模式。用 `whereFullTextWith` 可選擇 `Boolean` 或 `QueryExpansion`。

範例使用既有資料庫連線 `db`，建立資料表及全文索引後執行分頁搜尋。

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

Eloquent 查詢與巢狀條件群組也支援此述詞。記錄變更提交後，資料庫更新原生索引。SQLite 使用此述詞時回傳查詢錯誤。

## 資料庫模型搜尋 {#database-engine}

為 Eloquent 模型實作 `Searchable` 即可搜尋該模型的資料表。用 `FullText` 宣告全文索引欄位，用 `Prefix` 比對前綴，用 `Contains` 比對包含內容。搜尋條件放在同一個條件群組內，模型作用域和附加篩選適用於每筆比對。下例在前一個範例的模型加入 `slug` 欄位。

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

在應用程式內用 `Article::search(term)?` 取得模型連線。用 `withTrashed` 或 `onlyTrashed` 調整軟刪除篩選。空搜尋詞保留模型的一般篩選。資料庫在寫入交易中維護索引。

## 搜尋功能 {#search-capabilities}

資料庫引擎支援 MySQL 相容的全文述詞與 Eloquent 模型搜尋。PostgreSQL 全文查詢、向量索引、嵌入生成、重新排序和獨立 Scout 引擎各自需要對應的驅動程式。
