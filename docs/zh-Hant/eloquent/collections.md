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

### 主鍵字典 {#primary-key-dictionary}

用 `getDictionary` 按正規化主鍵索引已載入模型。字典保留每個鍵第一次出現的位置，並使用該鍵最後出現的模型。沒有主鍵的模型會略過。數值鍵使用十進位位元組，文字鍵使用 UTF-8 位元組，二進位鍵保留原始位元組。例如整數 `1` 和字串 `"1"` 共用一個鍵。此操作不發出查詢。

```rust
use openrust::database::{DatabaseResult, eloquent::{Collection, Model}};

pub fn find_by_bytes<'a, M: Model>(
    models: &'a Collection<M>,
    key: &[u8],
) -> DatabaseResult<Option<&'a M>> {
    Ok(models.getDictionary()?.get(key).copied())
}
```

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

`fresh` 在原連線上略過父模型作用域，依原順序回傳重新讀取的模型集合，並略過未儲存的模型和已從資料庫移除的記錄。軟刪除的記錄仍可讀取。用 `freshWith` 傳入型別化關聯，可在新集合中預先載入該關聯。用 `freshWithPaths` 可同時重新載入多個已註冊關聯，支援點分路徑。指定路徑的首段與預設預先載入同名時，以指定路徑為準，其餘預設預先載入照常執行。

`loadCount`、`loadExists`、`loadSum`、`loadAvg`、`loadMin` 和 `loadMax` 為已載入的模型補充聚合屬性。也可以透過 `loadAggregate` 明確傳入 `RelationAggregate`。這些方法依主鍵分批查詢，保留其他屬性的未儲存修改。如果有模型記錄已經消失，載入會在修改集合中的任何模型之前回傳錯誤。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, relations::HasMany},
};

pub async fn reload_authors(authors: &Collection<Author>) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let mut authors = authors.freshWithPaths(&["posts.comments"]).await?;
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

### 映射與集合轉換

`map`、`mapWithKeys`、`pluck`、`pluckWithKeys` 和 `keys` 回傳 Support 集合。映射結果需要繼續使用 Eloquent 方法時，使用 `mapModels`。用 `toBase` 將原有模型移入 Support 集合。Eloquent 集合使用零基位置，帶鍵投影保留指定的鍵，同一個鍵保留最後寫入的值。

```rust
use openrust::{
    database::{Value, eloquent::{Collection, Model}},
    support::Collection as BaseCollection,
};

pub fn primary_keys<M: Model>(models: &Collection<M>) -> BaseCollection<Option<Value>> {
    models.pluck(|model| model.getKey().cloned())
}
```

### 保留模型集合的鍵

用 `mapModelsWithKeys` 產生帶有明確鍵的 Eloquent 集合，也可用 `Collection::fromEntries` 從鍵與模型配對建構。依集合鍵取得模型用 `getByKey`，依模型主鍵查找用 `find`，索引用來存取位置。`keys`、`map`、`mapModels`、`fresh`、`toBase` 和 `toJson` 保留集合鍵。`all` 和 `toArray` 回傳依位置排列的值。`only`、`except`、`merge`、`unique`、`diff` 和 `intersect` 對結果重新編號。

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn label_models<M: Model>(models: Collection<M>) -> Collection<M> {
    models.mapModelsWithKeys(|model, key| [(format!("model-{key}"), model)])
}
```

### 分組與查找重複項

用 `partition` 將集合分成符合項與其餘項，兩組都保留原有集合鍵和 Eloquent 介面。`countBy` 回傳 Support 集合中的各組計數，閉包接收模型及其集合鍵。

`duplicates` 和 `duplicatesStrict` 依模型身分查找重複項，保留各重複模型的原有鍵。`duplicatesBy` 依投影型別的相等規則比較，並在 Support 集合中回傳重複的投影值。`uniqueBy` 為每個投影鍵保留首個模型。

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn persisted_models<M: Model>(models: Collection<M>) -> Collection<M> {
    let (persisted, _) = models.partition(|model, _| model.state().exists());
    persisted
}
```

### 依位置轉換集合

`zip` 依位置將模型與值配對，回傳由元組組成的 Support 集合。較短的一側用 `None` 補齊。`flatten` 將模型轉為使用連續整數鍵的 Support 集合，模型本身作為葉節點。要合併巢狀集合，可以先將每個模型投影為 Support 集合，再對結果呼叫 `collapse`。

額外欄數在執行時決定時，用 `zipMany` 配對同一元素型別的多欄。每列單獨保留模型，並為每個額外欄保留一個可選值，空欄也占一欄。

用 `flipBy` 將投影取得的純量鍵與原集合鍵交換。回傳 `None` 可略過目前項目，投影鍵重複時保留最後一個原集合鍵。值型別為 `ArrayKey` 的 Support 集合可直接呼叫 `flip`。

`pad` 接收帶正負號的目標長度與填充用的模型。負數向前填充，正數向後填充。需要填充時，整數鍵重新編號，字串鍵保留。新增模型由傳入模型的狀態建構。

```rust
use openrust::{
    database::eloquent::{Collection, Model},
    support::Collection as BaseCollection,
};

pub fn label_with_columns<M: Model>(
    models: Collection<M>,
    columns: Vec<Vec<String>>,
) -> BaseCollection<(Option<M>, Vec<Option<String>>)> {
    models.zipMany(columns)
}
```

### 佇列中繼資料

可以讀取編譯時確定的模型型別、主鍵、資料庫連線名稱和已載入的關聯路徑。主鍵保留集合順序和重複項目。關聯清單只包含每個模型都已載入的路徑，獨立於 JSON 可見性設定。讀取中繼資料不會執行查詢。空集合沒有模型型別和連線名稱，混用連線名稱會傳回錯誤。

派送與還原範例見[佇列模型關聯](../digging-deeper/queues.md#queued-model-relationships)。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Collection, Model}};

pub fn queued_ids<M: Model>(models: &Collection<M>) -> DatabaseResult<Vec<Value>> {
    models.getQueueableConnection()?;
    Ok(models.getQueueableIds())
}
```

## 自訂集合 {#custom-collections}

覆寫 `Model::newCollection` 可自訂查詢結果的集合建構。掛鉤在關聯預先載入後接收已水合模型，`makeMany` 建立尚未儲存的關聯模型時也會呼叫它。設定集合鍵時應保留輸入模型的順序。Rust 回傳 `Collection<Self>`，可用擴充 trait 新增業務方法。

指定數量的模型工廠也會對 `make()` 和 `create()` 的結果呼叫此掛鉤。佇列還原在組裝好模型清單後呼叫掛鉤。一般模型按承載資料順序還原，保留重複 ID，略過已缺少的記錄。承載資料保存模型識別碼，接收端透過模型掛鉤設定集合鍵。

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn keyed_models<M: Model>(models: Vec<M>) -> Collection<M> {
    Collection::fromEntries(
        models.into_iter().enumerate().map(|(index, model)| {
            (format!("model-{index}"), model)
        }),
    )
}
```
