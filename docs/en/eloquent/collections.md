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

`fresh` reads existing models again on their original connection, bypassing parent model scopes. It returns a new collection in the original order and omits unsaved models and rows that no longer exist. Soft-deleted rows remain eligible. Use `freshWith` with a typed relationship to eager load it on the new collection.

`loadCount`, `loadExists`, `loadSum`, `loadAvg`, `loadMin` and `loadMax` fetch aggregate attributes for models already loaded. Use `loadAggregate` to pass the aggregate as a `RelationAggregate` value. The methods batch by primary key and leave unrelated pending changes intact. If a model row has disappeared, the load returns an error before updating any model in the collection.

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
