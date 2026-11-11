# Eloquent Relationships

## Defining Relationships

Declare `HasOne`, `HasMany`, or `BelongsTo` with explicit keys. For example, `HasMany::<Author, Post>::new("id", "author_id")` joins an author's key to the post's foreign key. `BelongsTo::<Post, Author>::new("author_id", "id")` describes the inverse. Use `getResults` to fetch the relation for a single model.

`BelongsToMany` takes the intermediate table, its parent and related foreign keys, and the two model keys. For an `author_post` table, use `BelongsToMany::<Author, Post>::new("author_post", "author_id", "post_id", "id", "id")`.

### Default Models

Use `withDefault` on `BelongsTo`, `HasOne`, `HasOneThrough`, or `MorphOne` to supply an unsaved model when the related row is absent. The closure receives the new model and the parent. Use `|_, _| {}` for a default with no extra attributes. `HasOne` initializes the foreign key, and `MorphOne` also initializes the type alias. A fallback is available through the loaded relationship. Relationship existence and count queries still reflect stored rows.

```rust
use openrust::database::eloquent::{Model, relations::BelongsTo};

pub fn author_with_default() -> BelongsTo<Post, Author> {
    BelongsTo::new("author_id", "id").withDefault(|author: &mut Author, _| {
        author.setAttribute("name", "Guest author");
    })
}
```

### Has One of Many

Convert a `HasMany` or `MorphMany` descriptor with `one`, then choose `latestOfMany`, `oldestOfMany`, or `ofMany`. With no column argument, the latest and oldest methods use the related primary key. When ranking by another column, a tied value is resolved using the maximum primary key unless you specify another tie rule. Use `whereCandidates` to filter before ranking. SQL min and max ignore nulls, so a parent with only null ranking values has no result.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_top_post(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    Author::queryOn(connection)
        .with("top_post", posts.one().latestOfMany(Some("score")))
        .orderBy("id", "asc")
        .get()
        .await
}
```

### Has One Through and Has Many Through

Use `HasOneThrough` or `HasManyThrough` to query through an intermediate model. The constructor takes the intermediate model's foreign key, the final model's foreign key, the parent model's local key, and the intermediate model's local key. For example, an author has comments through posts. The intermediate and related models must use the same joined database connection.

Soft-deleted intermediate rows are excluded. Call `withTrashedParents` to include them. The related model's own soft-delete scope still applies.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasManyThrough},
};

pub async fn authors_with_comments(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    let comments = HasManyThrough::<Author, Post, Comment>::new("author_id", "post_id", "id", "id");
    Author::queryOn(connection).with("comments", comments).orderBy("id", "asc").get().await
}
```

### Has One Through of Many

Convert a `HasManyThrough` relationship with `one`, then choose `latestOfMany`, `oldestOfMany`, or `ofMany`. Ranking spans all eligible intermediate rows for each parent. `whereCandidates` filters the final model before ranking. The intermediate and final model soft-delete rules still apply.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasManyThrough},
};

pub async fn authors_with_latest_comment(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    let comments = HasManyThrough::<Author, Post, Comment>::new("author_id", "post_id", "id", "id");
    Author::queryOn(connection)
        .with("latest_comment", comments.one().latestOfMany(None))
        .orderBy("id", "asc")
        .get()
        .await
}
```

### Polymorphic Relationships

Use `MorphOne` or `MorphMany` to query children using both an ID and a type alias. Use `MorphTo` to resolve the inverse relation, and register each alias with its model type. Eager loading queries each distinct type in a batch. Read the loaded value using `getRelation::<Option<MorphTarget>>("subject")` and use `downcast_ref::<Post>()` on the target.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::MorphTo},
};

pub async fn comments_with_subjects(connection: &Connection) -> DatabaseResult<Collection<Comment>> {
    let subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id");
    Comment::queryOn(connection).with("subject", subjects).orderBy("id", "asc").get().await
}
```

`MorphToMany` adds a type discriminator to intermediate-table reads and writes. For the inverse direction, reverse the two pivot key arguments and use the same alias. The pivot value is available as `MorphPivot`.

## Querying Relationship Existence

`has` and `whereHas` keep parents with a matching related row. Use `whereDoesntHave` to keep parents without a match. These methods use a correlated subquery on the same database connection. `withWhereHas` applies the same relation to filtering and eager loading.

`hasCount` compares the related row count with a number. For a `MorphTo` relation, pass explicit aliases to `whereHasMorph`, `whereDoesntHaveMorph`, or `hasMorphCount`. Use `morphWithCount` to register a target type with a count on one of its own relationships.

### Querying All Related Models

Use the asynchronous `whereHasMorphAll`, `whereDoesntHaveMorphAll`, and `hasMorphCountAll` methods for Laravel's wildcard behavior. They first read distinct morph aliases from the model table, independently of the current query's filters and scopes. Register the stored aliases on the relation. When looking for absent targets, `whereDoesntHaveMorphAll` also matches a null morph type.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::MorphTo},
};

pub async fn comments_with_existing_subject(connection: &Connection) -> DatabaseResult<Collection<Comment>> {
    let subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id");
    Comment::queryOn(connection)
        .whereHasMorphAll(&subjects)
        .await?
        .orderBy("id", "asc")
        .get()
        .await
}
```

### Filtering by Relationship Identity

Use `whereBelongsTo` with a typed relation and a slice of owners. It compares the foreign key against the configured owner key. Use `whereAttachedTo` to match rows linked through a many-to-many relationship. For a polymorphic relation, use `identity` to obtain a type and owner key pair, then pass one or more pairs to `whereMorphedTo` or `whereNotMorphedTo`. Use `whereMorphType` to filter by alias alone or pass `None` for an unset type.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::MorphTo},
};

pub async fn comments_for_subjects(
    connection: &Connection, author: &Author, post: &Post,
) -> DatabaseResult<Collection<Comment>> {
    let subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id");
    Comment::queryOn(connection)
        .whereMorphedTo(&subjects, &[subjects.identity("author", author)?, subjects.identity("post", post)?])?
        .get()
        .await
}
```

## Aggregating Related Models

Use `withCount`, `withExists`, `withSum`, `withAvg`, `withMin`, or `withMax` to select relation attributes in the parent query. The example exposes `posts_count`, `posts_exists`, and `posts_sum_score`. Use a name such as `"posts as published_count"` for an explicit alias. Apply any parent `select` before the aggregate methods. An empty sum is `Null`, and existence is a Boolean.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_post_totals(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    Author::queryOn(connection)
        .withCount("posts", &posts)?
        .withExists("posts", &posts)?
        .withSum("posts", &posts, "score")?
        .orderBy("id", "asc")
        .get()
        .await
}
```

### Default Relationship Counts

Implement `Model::withDefaultCounts` to add count projections whenever the model is read. This corresponds to Laravel's `$withCount` property. The counts share the model's SELECT. An explicit `select` replaces the default projection.

```rust
fn withDefaultCounts(query: Builder<'static, Self>) -> DatabaseResult<Builder<'static, Self>> {
    query.withCount("posts", &HasMany::<Self, Post>::new("id", "author_id"))
}
```

### Loading Polymorphic Target Groups

Use `loadMorph` or `loadMorphCount` after retrieving parent models. Pass the target alias and a typed related relationship. The resulting batch exposes the loaded relationship or count. Open a group with `morphModels` to continue traversing a relationship chain. Each call selects one alias, so different target types can use different relationships even when their primary keys overlap. Newly fetched relations and count attributes are scoped to the returned batch.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Collection, relations::{HasMany, MorphTo}},
};

pub async fn author_post_counts_for_subjects(comments: &mut Collection<Comment>) -> DatabaseResult<Vec<Value>> {
    let subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id");
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let authors = comments.loadMorphCount("subject", &subjects, "author", "posts", &posts).await?;
    Ok((0..authors.all().len())
        .filter_map(|index| authors.getAttribute(index, "posts_count").cloned())
        .collect())
}
```

## Eager Loading

Pass a name and descriptor to `with`. For up to 500 distinct parent keys, this loads the parents and posts in two SELECTs. It retains parent order and assigns an empty result to authors without posts. The example assumes `Author` and `Post` implement [Model](./getting-started.md).

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_posts(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    Author::queryOn(connection)
        .with("posts", HasMany::<Author, Post>::new("id", "author_id"))
        .orderBy("id", "asc")
        .get()
        .await
}
```

Use `getRelation::<Vec<Arc<Post>>>("posts")` to read loaded posts. Singular relations use `Option<Arc<T>>`. A missing relation name means it has not been loaded. Related models are shared through `Arc` when several parents reference them. Use `load` on a model or collection to load after retrieval, and `loadMissing` on a collection to preserve previously loaded values.

### Eager Loading by Default

Implement `Model::withDefaultRelations` to declare the typed relationships loaded by default. This is the model's equivalent of Laravel's `$with` property.

```rust
fn withDefaultRelations(relations: EagerLoads<Self>) -> EagerLoads<Self> {
    relations.with("posts", HasMany::<Self, Post>::new("id", "author_id"))
}
```

For one query, use `without` to remove named relationships or `withOnly` to replace the eager loads. Calling `with` with the same name replaces that relationship's definition for the current query.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_only_mentees(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    Author::queryOn(connection)
        .withOnly(|relations| relations.with("mentees", HasMany::<Author, Author>::new("id", "mentor_id")))
        .orderBy("id", "asc")
        .get()
        .await
}
```

### Constraining Eager Loads

Use `constrain` to configure the related model's query. Call `with` in that closure for nested eager loading. The example filters authors and then loads their matching posts and each post's author. In a related query, `limit` and `offset` apply separately to each parent. Add `orderBy` for a predictable choice of related rows.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::{BelongsTo, HasMany}},
};

pub async fn authors_with_matching_posts(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id").constrain(|mut query| {
        query.r#where("title", "like", "First%");
        query.with("author", BelongsTo::<Post, Author>::new("author_id", "id"))
    });

    Author::queryOn(connection).withWhereHas("posts", posts)?.get().await
}
```

### Automatic Eager Loading

Call `withRelationshipAutoloading` on a collection, then await `getRelationOrLoad` to access a model's typed relationship. The first missing access batches the load across that collection. Previously loaded values, including empty results, are reused. Enable it by default for a model by returning `true` from `Model::automaticallyEagerLoadRelationships`.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Model, relations::HasMany},
};

pub async fn author_post_counts(connection: &Connection) -> DatabaseResult<Vec<usize>> {
    let mut authors = Author::queryOn(connection)
        .orderBy("id", "asc")
        .get()
        .await?
        .withRelationshipAutoloading();
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let mut counts = Vec::with_capacity(authors.len());
    for index in 0..authors.len() {
        counts.push(authors.getRelationOrLoad(index, "posts", &posts).await?.len());
    }
    Ok(counts)
}
```

### Loading a Relationship Chain

Use `relatedModels` to open a batch of models reached through a singular or plural relationship. It reuses already-loaded values. Call `getRelationOrLoad` on that batch to fetch the next relationship across its models, or call `relatedModels` again to traverse another level. Results fetched on a batch are cached for that batch's lifetime. Shared instances appear once, and empty relationships contribute no models.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, relations::HasMany},
};

pub async fn count_post_comments(authors: &mut Collection<Author>) -> DatabaseResult<usize> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let comments = HasMany::<Post, Comment>::new("id", "post_id");
    let mut posts = authors.relatedModels("posts", &posts).await?;
    Ok(posts.relatedModels("comments", &comments).await?.all().len())
}
```

### Preventing Lazy Loading

Override `Model::preventsLazyLoading` to reject implicit single-model relation queries when accessing persisted models. Already-loaded relations, explicit `load`, and collection autoloading remain available. The default policy allows unsaved and recently created models.

```rust
fn preventsLazyLoading(&self) -> bool {
    true
}
```

Override `handleLazyLoadingViolationUsing` to handle the typed `DatabaseError::LazyLoadingViolation`. Return `Ok(())` to allow the query to proceed after recording the violation.

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::{Model, relations::HasMany}};

pub async fn author_post_count(connection: &Connection, id: i64) -> DatabaseResult<usize> {
    let mut author = Author::queryOn(connection).findOrFail(id).await?;
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    author.load("posts", &posts).await?;
    Ok(author.getRelationOrLoad("posts", &posts).await?.len())
}
```

## Inserting and Updating Related Models

`HasMany::create` and `save` populate the foreign key before saving a related model. `BelongsTo::associate` and `dissociate` modify the child in memory. Call `save` on the child to persist the change.

For many-to-many links, use `attach`, `detach`, `sync`, or `syncWithoutDetaching`. Synchronization adds and removes links in one database transaction. Use a composite unique key over the two pivot foreign keys to prevent duplicate links.

Use `withPivot(["label"])` to select extra columns from the intermediate table. Read them using `getRelation::<Pivot>("pivot")` on the related model. Use `r#as("membership")` to change the accessor. `attachWith` writes additional columns when creating links, and `updateExistingPivot` updates an existing link and returns its affected-row count.

`syncWithPivotValues` applies the same attributes to every key. Use `syncWithPivotAttributes` with `(key, attributes)` pairs for individual values. Both return attached, detached, and updated keys. A failed sync rolls back its changes.
