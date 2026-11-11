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
