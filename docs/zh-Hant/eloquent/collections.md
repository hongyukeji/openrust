# Eloquent 集合

## 簡介

模型查詢依查詢順序回傳 `Collection<M>`。使用 `all` 取得切片，也能迭代並消耗集合。

## 可用方法

用 `modelKeys` 讀取主鍵。`find` 和 `contains` 依主鍵查找。`only` 和 `except` 依模型鍵篩選並維持原本的模型順序。

```rust
use openrust::database::{
    Connection, DatabaseResult, Value,
    eloquent::Model,
};

pub async fn selected_author_keys(connection: &Connection, keys: &[Value]) -> DatabaseResult<Vec<Value>> {
    let authors = Author::queryOn(connection).orderBy("id", "asc").get().await?;
    Ok(authors.only(keys)?.modelKeys())
}
```

集合的 `load` 和 `loadMissing` 可批次載入關聯。請見[預先載入](./relationships.md#預先載入)。

### 依主鍵查找模型

`find` 回傳可選模型。主鍵不存在時，`findOrFail` 回傳 `DatabaseError::ModelNotFound`。用 `contains` 和 `doesntContain` 檢查主鍵是否在集合中。傳入一組主鍵時，使用 `findMany` 或 `findManyOrFail`，兩者都依集合順序回傳模型參照。`findManyOrFail` 要求每個不同的主鍵都存在，即使集合內有重複主鍵。這些方法直接讀取已載入的模型，不執行 SQL。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Collection, Model}};

pub fn required_models<'a, M: Model>(
    models: &'a Collection<M>,
    keys: &[Value],
) -> DatabaseResult<Vec<&'a M>> {
    models.findManyOrFail(keys)
}
```

### 依模型或述詞查找

`findModel` 和 `findModelOrFail` 取傳入模型的主鍵來查找。用 `containsModel` 或 `doesntContainModel` 比較模型身分，主鍵值和型別、資料表名稱、連線名稱均須相同。`Model::is` 和 `Model::isNot` 使用同一規則。依條件檢查時，用 `containsWhere` 或 `doesntContainWhere`，閉包接收模型和從零開始的索引。

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn has_model_in_prefix<M: Model>(models: &Collection<M>, candidate: &M) -> bool {
    models.containsWhere(|model, index| index < 3 && model.is(candidate))
}
```

### 依模型主鍵操作

`merge` 和 `unique` 為每個鍵保留最後一個模型，沿用該鍵首次出現的位置。`diff` 和 `intersect` 依模型鍵比較並維持原集合順序。`toQuery` 使用原連線為選中的鍵建立查詢，集合須有模型且都使用同一個連線。

```rust
use openrust::database::{
    Connection, DatabaseResult, Value,
    eloquent::Model,
};

pub async fn requery_author_keys(connection: &Connection, keys: &[Value]) -> DatabaseResult<Vec<Value>> {
    let selected = Author::queryOn(connection).get().await?.only(keys)?;
    Ok(selected.toQuery()?.orderBy("id", "asc").get().await?.modelKeys())
}
```

### 重新讀取模型與載入聚合

`fresh` 在原連線上略過父模型作用域，依原順序回傳重新讀取的模型集合，並略過未儲存的模型和已從資料庫移除的記錄。軟刪除的記錄仍可讀取。用 `freshWith` 傳入型別化關聯，可在新集合中預先載入該關聯。

`loadCount`、`loadExists`、`loadSum`、`loadAvg`、`loadMin` 和 `loadMax` 為已載入的模型補充聚合屬性。也可以透過 `loadAggregate` 明確傳入 `RelationAggregate`。這些方法依主鍵分批查詢，保留其他屬性的未儲存修改。如果有模型記錄已經消失，載入會在修改集合中的任何模型之前回傳錯誤。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, relations::HasMany},
};

pub async fn reload_authors(authors: &Collection<Author>) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let mut authors = authors.freshWith("posts", &posts).await?;
    authors.loadCount("posts", &posts).await?;
    Ok(authors)
}
```

### 一次載入多個關聯聚合

向 `loadAggregates` 傳入多個 `RelationAggregateQuery`，每批主鍵用一次 SELECT 讀取所有聚合。各屬性需要不同的名稱，可以用 `as` 指定別名。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, RelationAggregate, RelationAggregateQuery, relations::HasMany},
};

pub async fn load_author_totals(authors: &mut Collection<Author>) -> DatabaseResult<()> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    authors.loadAggregates(&[
        RelationAggregateQuery::new("posts", &posts, RelationAggregate::Count),
        RelationAggregateQuery::new("posts", &posts, RelationAggregate::Sum("score")),
    ]).await
}
```
