# Eloquent 集合

## 简介

模型查询按查询顺序返回 `Collection<M>`。用 `all` 获取切片，也可以迭代并消费集合。

## 可用方法

用 `modelKeys` 读取主键。`find` 和 `contains` 按主键查找。`only` 和 `except` 按模型键过滤并保持原有模型顺序。

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

集合的 `load` 和 `loadMissing` 可批量加载关系。见[预加载](./relationships.md#预加载)。

### 按主键查找模型

`find` 返回可选模型。主键不存在时，`findOrFail` 返回 `DatabaseError::ModelNotFound`。用 `contains` 和 `doesntContain` 检查主键是否在集合中。传入一组主键时，使用 `findMany` 或 `findManyOrFail`，两者都按集合顺序返回模型引用。`findManyOrFail` 要求每个不同的主键都存在，即使集合内有重复主键。这些方法直接读取已加载的模型，不执行 SQL。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Collection, Model}};

pub fn required_models<'a, M: Model>(
    models: &'a Collection<M>,
    keys: &[Value],
) -> DatabaseResult<Vec<&'a M>> {
    models.findManyOrFail(keys)
}
```

### 按模型或谓词查找

`findModel` 和 `findModelOrFail` 取传入模型的主键来查找。用 `containsModel` 或 `doesntContainModel` 比较模型身份，主键值和类型、表名、连接名均须一致。`Model::is` 和 `Model::isNot` 使用同一规则。按条件检查时，用 `containsWhere` 或 `doesntContainWhere`，闭包接收模型和从零开始的索引。

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn has_model_in_prefix<M: Model>(models: &Collection<M>, candidate: &M) -> bool {
    models.containsWhere(|model, index| index < 3 && model.is(candidate))
}
```

### 按模型主键操作

`merge` 和 `unique` 为每个键保留最后一个模型，沿用该键首次出现的位置。`diff` 和 `intersect` 按模型键比较并保持原集合顺序。`toQuery` 使用原连接为选中的键构建查询，集合须有模型且都使用同一个连接。

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

### 重新读取模型与加载聚合

`fresh` 在原连接上跳过父模型作用域，按原顺序返回重新读取的模型集合，并跳过未保存的模型和已从数据库移除的记录。软删除的记录仍可读取。用 `freshWith` 传入类型化关系，可在新集合中预加载该关系。

`loadCount`、`loadExists`、`loadSum`、`loadAvg`、`loadMin` 和 `loadMax` 为已加载的模型补充聚合属性。也可以通过 `loadAggregate` 显式传入 `RelationAggregate`。它们按主键分批查询，保留其他属性的未保存修改。如果有模型记录已经消失，加载会在修改集合中任何模型之前返回错误。

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

### 一次加载多个关系聚合

向 `loadAggregates` 传入多个 `RelationAggregateQuery`，每批主键用一次 SELECT 读取所有聚合。各属性需要不同的名称，可以用 `as` 指定别名。

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
