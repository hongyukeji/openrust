# Eloquent Collections

## Introduction

A model query returns `Collection<M>` in query order. Use `all` for a slice, or iterate the collection to consume it.

## Available Methods

Use `modelKeys` to read primary keys. `find` and `contains` look up a primary key. `only` and `except` filter by model key and retain the original model order.

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

Use `load` and `loadMissing` to batch relationships on an existing collection. See [eager loading](./relationships.md#eager-loading).

### Primary-key dictionary {#primary-key-dictionary}

Use `getDictionary` to index borrowed models by their normalized primary keys. The dictionary keeps the first position of each key and the last model with that key. Models without a key are skipped. Numeric keys use their decimal bytes, text keys use UTF-8 bytes, and binary keys retain their raw bytes. For example, the integer `1` and the string `"1"` share a key. This operation uses the loaded models and issues no queries.

```rust
use openrust::database::{DatabaseResult, eloquent::{Collection, Model}};

pub fn find_by_bytes<'a, M: Model>(
    models: &'a Collection<M>,
    key: &[u8],
) -> DatabaseResult<Option<&'a M>> {
    Ok(models.getDictionary()?.get(key).copied())
}
```

### Looking Up Models by Key

`find` returns an optional model. `findOrFail` returns `DatabaseError::ModelNotFound` when the key is absent. Use `contains` and `doesntContain` for membership checks. For a list of keys, use `findMany` or `findManyOrFail`. Both return borrowed models in collection order. `findManyOrFail` requires every distinct key to be present, even when the collection contains repeated model keys. These operations use the hydrated collection and issue no SQL.

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Collection, Model}};

pub fn required_models<'a, M: Model>(
    models: &'a Collection<M>,
    keys: &[Value],
) -> DatabaseResult<Vec<&'a M>> {
    models.findManyOrFail(keys)
}
```

### Matching Models and Predicates

`findModel` and `findModelOrFail` look up the supplied model’s primary key. Use `containsModel` or `doesntContainModel` to check identity by exact key value and type, table, and connection name. `Model::is` and `Model::isNot` use the same identity rule. For a predicate, call `containsWhere` or `doesntContainWhere` with a closure receiving the model and its zero-based index.

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn has_model_in_prefix<M: Model>(models: &Collection<M>, candidate: &M) -> bool {
    models.containsWhere(|model, index| index < 3 && model.is(candidate))
}
```

### Working with Model Keys

`merge` and `unique` retain the last model for each key at its first position. `diff` and `intersect` compare model keys and preserve the source collection order. Use `toQuery` to build a query for the selected keys on their original connection. The collection must be nonempty and all models must use the same connection.

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

### Refreshing Models and Loading Aggregates

`fresh` reads existing models again on their original connection, bypassing parent model scopes. It returns a new collection in the original order and omits unsaved models and rows that no longer exist. Soft-deleted rows remain eligible. Use `freshWith` with a typed relationship to eager load it on the new collection. Use `freshWithPaths` to reload several registered relationships, including dot-separated paths. Requested roots take precedence over default loads with the same name, and other default loads still apply.

`loadCount`, `loadExists`, `loadSum`, `loadAvg`, `loadMin` and `loadMax` fetch aggregate attributes for models already loaded. Use `loadAggregate` to pass the aggregate as a `RelationAggregate` value. The methods batch by primary key and leave unrelated pending changes intact. If a model row has disappeared, the load returns an error before updating any model in the collection.

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

### Loading Several Aggregates

Pass several `RelationAggregateQuery` values to `loadAggregates` to fetch them in one SELECT per batch of primary keys. Each projected attribute needs a distinct name. Use `as` to assign an alias.

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

### Mapping and Converting Collections

`map`, `mapWithKeys`, `pluck`, `pluckWithKeys`, and `keys` return a Support collection. Use `mapModels` when the result should retain Eloquent methods, or `toBase` to move the original models into a Support collection. Eloquent collections use zero-based positions. Keyed projections preserve the supplied keys and use the last value for a repeated key.

```rust
use openrust::{
    database::{Value, eloquent::{Collection, Model}},
    support::Collection as BaseCollection,
};

pub fn primary_keys<M: Model>(models: &Collection<M>) -> BaseCollection<Option<Value>> {
    models.pluck(|model| model.getKey().cloned())
}
```

### Keeping Keys on Model Collections

Use `mapModelsWithKeys` to produce an Eloquent collection with explicit keys, or `Collection::fromEntries` to build one from key-model pairs. Use `getByKey` for a collection key, `find` for a model primary key, and indexing for a position. `keys`, `map`, `mapModels`, `fresh`, `toBase`, and `toJson` preserve collection keys. `all` and `toArray` return positional values. `only`, `except`, `merge`, `unique`, `diff`, and `intersect` return reindexed results.

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn label_models<M: Model>(models: Collection<M>) -> Collection<M> {
    models.mapModelsWithKeys(|model, key| [(format!("model-{key}"), model)])
}
```

### Grouping and Finding Duplicates

Use `partition` to split the collection into matching and remaining Eloquent collections. Both retain the original collection keys. `countBy` returns counts in a Support collection. Its closure receives the model and its collection key.

`duplicates` and `duplicatesStrict` use model identity and retain each repeated model’s original key. `duplicatesBy` returns repeated projected values in a Support collection using the projected type’s equality. `uniqueBy` retains the first model for each projected key.

```rust
use openrust::database::eloquent::{Collection, Model};

pub fn persisted_models<M: Model>(models: Collection<M>) -> Collection<M> {
    let (persisted, _) = models.partition(|model, _| model.state().exists());
    persisted
}
```

### Positional Conversions

`zip` pairs models with values by position and returns a Support collection of tuples. The shorter side is filled with `None`. `flatten` returns the models in a Support collection with consecutive integer keys. Eloquent models are leaves.

`collapse` merges one level of groups in a Support collection. A group can be a vector, Eloquent collection, Support collection, or a loaded `RelatedCollection`. Numeric keys are reindexed. Later values replace earlier values with the same string key. PHP can mix models and nested arrays in a collection. In Rust, declare the grouped input type before calling `collapse`.

Collapsing loaded relationship groups retains their shared model snapshots and performs no database reads. Use `getRelationOrLoad` on the model’s collection to load and write back missing relations. Independently constructed model states have independent attributes.

```rust
use openrust::{
    database::eloquent::{Collection, Model},
    support::Collection as BaseCollection,
};

pub fn collapse_groups<M: Model>(
    groups: BaseCollection<Collection<M>>,
) -> BaseCollection<M> {
    groups.collapse()
}
```

Use `zipMany` for a runtime number of extra columns of the same item type. Each row keeps the model separately and contains one optional value per extra column, even when a column is empty.

Use `flipBy` to exchange a projected scalar key with its original collection key. Return `None` to skip an item. Repeated projected keys keep the last original key. Call `flip` directly on a Support collection of `ArrayKey` values.

`pad` accepts a signed target length and a model to use for padding. Negative lengths pad the beginning, positive lengths pad the end. When padding is needed, integer keys are renumbered and string keys are preserved. Each added model is constructed from the supplied model state.

`multiply(n)` repeats the current model order and assigns consecutive integer collection keys. A zero or negative multiplier returns an empty collection. Each copy retains the model key and loaded relationships, and owns its ordinary attributes. Repeating models does not query the database.

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

### Queue Metadata

Use these methods to inspect the compiled model type, primary keys, database connection name, and loaded relationship paths. Keys keep the collection order and duplicates. The relationship list contains only paths loaded on every model and is independent of JSON visibility. Reading this metadata issues no queries. An empty collection has no class or connection. Mixing connection names returns an error.

For dispatch and restoration, see [queued model relationships](../digging-deeper/queues.md#queued-model-relationships).

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Collection, Model}};

pub fn queued_ids<M: Model>(models: &Collection<M>) -> DatabaseResult<Vec<Value>> {
    models.getQueueableConnection()?;
    Ok(models.getQueueableIds())
}
```

## Custom collections {#custom-collections}

Override `Model::newCollection` to customize how query results are assembled. The hook receives hydrated models after eager loading and also runs when `makeMany` constructs unsaved related models. Keep the supplied models in order when assigning collection keys. Rust returns `Collection<Self>` and supports adding domain methods with extension traits.

Numbered and simple pages preserve your collection keys, including after `through` transforms the items. Cursor pagination reindexes page results.

Counted factories apply the same hook to the result of `make()` and `create()`. Queue restoration applies the hook after assembling the final model list. Ordinary models follow payload order, including repeated IDs, and missing rows are omitted. The queue payload carries model identifiers, and the receiving application supplies the collection keys through the model hook.

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
