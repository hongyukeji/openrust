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

### 主键字典 {#primary-key-dictionary}

用 `getDictionary` 按规范化主键索引已加载模型。字典保留每个键首次出现的位置，并使用该键最后出现的模型。没有主键的模型会跳过。数值键使用十进制字节，文本键使用 UTF-8 字节，二进制键保留原始字节。例如整数 `1` 和字符串 `"1"` 共用一个键。此操作不发起查询。

```rust
use openrust::database::{DatabaseResult, eloquent::{Collection, Model}};

pub fn find_by_bytes<'a, M: Model>(
    models: &'a Collection<M>,
    key: &[u8],
) -> DatabaseResult<Option<&'a M>> {
    Ok(models.getDictionary()?.get(key).copied())
}
```

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

### 映射与集合转换

`map`、`mapWithKeys`、`pluck`、`pluckWithKeys` 和 `keys` 返回 Support 集合。映射结果需要继续使用 Eloquent 方法时，使用 `mapModels`。用 `toBase` 将原有模型移入 Support 集合。Eloquent 集合使用零基位置，带键投影保留指定的键，同一个键保留最后写入的值。

```rust
use openrust::{
    database::{Value, eloquent::{Collection, Model}},
    support::Collection as BaseCollection,
};

pub fn primary_keys<M: Model>(models: &Collection<M>) -> BaseCollection<Option<Value>> {
    models.pluck(|model| model.getKey().cloned())
}
```

### 保留模型集合的键

用 `mapModelsWithKeys` 生成带显式键的 Eloquent 集合，也可用 `Collection::fromEntries` 从键与模型配对构造。按集合键取模型用 `getByKey`，按模型主键查找用 `find`，下标用于访问位置。`keys`、`map`、`mapModels`、`fresh`、`toBase` 和 `toJson` 保留集合键。`all` 和 `toArray` 返回按位置排列的值。`only`、`except`、`merge`、`unique`、`diff` 和 `intersect` 对结果重新编号。

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn label_models<M: Model>(models: Collection<M>) -> Collection<M> {
    models.mapModelsWithKeys(|model, key| [(format!("model-{key}"), model)])
}
```

### 分组与查找重复项

用 `partition` 将集合分成匹配项和其余项，两组都保留原有集合键和 Eloquent 接口。`countBy` 返回 Support 集合中的各组计数，闭包接收模型及其集合键。

`duplicates` 和 `duplicatesStrict` 按模型身份查找重复项，保留各重复模型的原有键。`duplicatesBy` 按投影类型的相等规则比较，并在 Support 集合中返回重复的投影值。`uniqueBy` 为每个投影键保留首个模型。

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn persisted_models<M: Model>(models: Collection<M>) -> Collection<M> {
    let (persisted, _) = models.partition(|model, _| model.state().exists());
    persisted
}
```

### 按位置转换集合

`zip` 按位置将模型和值配对，返回由元组组成的 Support 集合。较短的一侧用 `None` 补齐。`flatten` 将模型转为使用连续整数键的 Support 集合，模型本身作为叶节点。要合并嵌套集合，可以先将每个模型投影为 Support 集合，再对结果调用 `collapse`。

额外列数在运行时确定时，用 `zipMany` 配对同一元素类型的多列。每行单独保留模型，并为每个额外列保留一个可选值，空列也占一列。

用 `flipBy` 将投影得到的标量键与原集合键交换。返回 `None` 可跳过当前项，投影键重复时保留最后一个原集合键。值类型为 `ArrayKey` 的 Support 集合可直接调用 `flip`。

`pad` 接收带符号的目标长度和用于填充的模型。负数向前填充，正数向后填充。需要填充时，整数键重新编号，字符串键保留。新增模型由传入模型的状态构造。

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

### 队列元数据

可以读取编译时确定的模型类型、主键、数据库连接名和已加载的关系路径。主键保留集合顺序和重复项。关系列表只包含每个模型都已加载的路径，独立于 JSON 可见性设置。读取元数据不会执行查询。空集合没有模型类型和连接名，混用连接名会返回错误。

派发与恢复示例见[队列模型关系](../digging-deeper/queues.md#queued-model-relationships)。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Collection, Model}};

pub fn queued_ids<M: Model>(models: &Collection<M>) -> DatabaseResult<Vec<Value>> {
    models.getQueueableConnection()?;
    Ok(models.getQueueableIds())
}
```
