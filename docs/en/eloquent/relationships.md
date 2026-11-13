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

### Dynamic relationships {#dynamic-relationships}

Register a named resolver with `Model::resolveRelationUsing` in a service provider’s `boot` method. The closure receives the current model and returns a relationship descriptor with explicit keys. Each application has its own registry. Registering the same name and model type again replaces the resolver for future loads.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{DynamicRelation, Model, relations::BelongsTo}},
    foundation::Application,
};

type CustomerRelation = BelongsTo<Order, Customer>;

fn register(app: &Application) -> DatabaseResult<()> {
    Order::resolveRelationUsing(app, "customer", |_| {
        CustomerRelation::new("customer_id", "id")
    })
}

fn eager_customer() -> DynamicRelation<CustomerRelation> {
    DynamicRelation::new("customer")
}
```

Use `getDynamicRelationOrLoad` for cached access, or use `DynamicRelation` with `with`, `load`, and `loadMissing`. The descriptor type is explicit in Rust. The resolver can select different query constraints for each model instance, so dynamic eager loading can issue one query per parent. A hydrated model resolves against its connection’s application.

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

Use `orWhereBelongsTo`, `orWhereAttachedTo`, `orWhereMorphedTo`, or `orWhereNotMorphedTo` to add a relationship identity as an alternative to the preceding conditions. `whereAttachedTo` and `orWhereAttachedTo` also accept a `MorphToMany` descriptor and apply its intermediate-table discriminator. Global scopes and soft-delete filters still apply.

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

## Filtering by Intermediate Table Columns {#filtering-queries-via-intermediate-table-columns}

Use `wherePivot` for a comparison, `wherePivotIn` or `wherePivotNotIn` for a list, `wherePivotBetween` or `wherePivotNotBetween` for an inclusive range, and `wherePivotNull` or `wherePivotNotNull` for null checks. The corresponding `orWherePivot…` methods combine alternatives inside the same owner and morph scope. Filters apply to loading, existence queries, and selecting existing links for updates, detach, and sync.

Use `withPivotValue` to apply an equality filter and supply the same value when inserting a link through `attach` or `sync`. Explicit insert attributes take precedence. The default must be non-null and cannot replace relationship keys.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::BelongsToMany}};

pub fn approved_roles<U: Model, R: Model>() -> DatabaseResult<BelongsToMany<U, R>> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .withPivot(["approved"])
        .withPivotValue("approved", true)
}
```

## Ordering by Intermediate Table Columns {#ordering-queries-via-intermediate-table-columns}

Use `orderByPivot("position", "asc")` or `orderByPivotDesc("created_at")` to order related models by a pivot column. Ordering applies to lazy and eager loads, including polymorphic many-to-many relationships.

Use `withTimestamps()` to select and maintain the pivot timestamps. Column names follow the parent model, with `created_at` and `updated_at` as fallbacks. Use `withTimestampColumns(Some("linked_at"), None)?` for a custom creation column with no update timestamp. Inserts initialize both configured columns and accept explicit values. Updating an existing link refreshes the update timestamp and preserves the creation timestamp unless the caller changes it explicitly.

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn latest_roles<U: Model, R: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .withTimestamps()
        .orderByPivotDesc("created_at")
}
```

## Defining Custom Intermediate Table Models {#defining-custom-intermediate-table-models}

Use `using::<Membership>()` on a many-to-many relationship to load and write intermediate rows as a typed Eloquent model. Declare the model’s casts and fillable attributes as usual. Read the result with `getRelation::<Membership>("membership")` when the accessor is renamed. The relation supplies the table, connection, and original link keys, including the morph type for polymorphic links. Use `incrementing(false)` for a composite-key table. An incrementing pivot requires an actual primary key column.

Relationship writes use the custom model’s lifecycle and cast rules. Use the relationship’s `withTimestamps` or `withTimestampColumns` to configure pivot timestamps. For soft deletes, promote the intermediate row to an ordinary Eloquent model.

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn roles<U: Model, R: Model, Membership: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .using::<Membership>()
        .withPivot(["approved"])
        .r#as("membership")
}
```

### Automatically Hydrating Pivot Relationships {#automatically-hydrating-pivot-relationships}

Declare the inverse names on your pivot model with `pivotInverseRelations()`. Call `chaperone()` after `using::<Membership>()` to infer names from the pivot keys and model types. Use `chaperoneWith(Some("owner"), Some("item"))?` for explicit names and `withoutChaperone()` to disable hydration.

Both lazy and eager loads populate the inverses from the models already loaded. Read them as `Option<Arc<T>>` through `getRelation`. The inverses hold snapshots of the endpoints at hydration time so Rust can serialize the graph without a reference cycle.

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn roles<U: Model, R: Model, Membership: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .using::<Membership>()
        .chaperone()
}
```

## Inserting and Updating Related Models

`HasMany::create` and `save` populate the foreign key before saving a related model. `BelongsTo::associate` and `dissociate` modify the child in memory. Call `save` on the child to persist the change.

For many-to-many links, use `attach`, `detach`, `sync`, or `syncWithoutDetaching`. Synchronization adds and removes links in one database transaction. Use a composite unique key over the two pivot foreign keys to prevent duplicate links.

Use `withPivot(["label"])` to select extra columns from the intermediate table. Read them using `getRelation::<Pivot>("pivot")` on the related model. Use `r#as("membership")` to change the accessor. `attachWith` writes additional columns when creating links, and `updateExistingPivot` updates an existing link and returns its affected-row count.

`syncWithPivotValues` applies the same attributes to every key. Use `syncWithPivotAttributes` with `(key, attributes)` pairs for individual values. Both return attached, detached, and updated keys. A failed sync rolls back its changes.

### Toggling associations {#toggling-associations}

Use `toggle` to remove existing links and insert missing links for the supplied keys. `toggleWithPivotAttributes` supplies per-key attributes for inserts. Both `toggle` and `toggleOrFail` run in one transaction and return attached and detached keys.

## Touching parent timestamps {#touching-parent-timestamps}

Declare a typed belongs-to relation in your model’s `touches()` method to update the owner’s timestamp when a child is inserted or saved with changes. OpenRust uses the persisted foreign key and the relationship’s scopes. A missing owner is skipped. The owner emits `Saved` and its own declared owners are touched recursively. Each owner is touched once per save. With `saveInTransaction`, owner queries, writes and events use that transaction, so declared owners must use the same connection.

In a `Model for Comment` implementation, declare the relationship to `Post`.

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::BelongsTo};

    Touches::default().belongsTo(BelongsTo::<Self, Post>::new("post_id", "id"))
}
```

Laravel declares these relationships by name in `$touches`. OpenRust uses typed relation descriptors. Laravel removes global scopes for the timestamp write. OpenRust keeps them on both the owner lookup and the write, so a tenant scope can prevent an out-of-scope update.

For a polymorphic owner, use `Touches::morphTo` with the same descriptor used to load the relationship. The persisted type selects the registered model, and the persisted ID is matched against that model’s owner key. Null type or ID values are skipped. An unregistered alias returns an error, so use `saveInTransaction` if the child write and the touch must roll back together.

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::MorphTo};

    Touches::default().morphTo(
        MorphTo::<Self>::new("commentable_type", "commentable_id")
            .target::<Post>("post", "id"),
    )
}
```

Enable `withTouchingParent()` on a many-to-many relation to update the declaring model’s timestamp when its links change. The parent update uses the same transaction and starts from persisted attributes. Reload the parent to read the new timestamp.

Enable `withTouchingRelated()` to refresh all models still linked after an attach, pivot update, detach, sync, or toggle changes the relationship. The update shares the pivot transaction and respects pivot filters, morph type, and related model scopes. It updates timestamps in bulk. Call `touch(&parent)` to refresh linked models explicitly, even on a descriptor without automatic touching.

To refresh related models when the declaring model itself changes, add the relationship to `Model::touches()` with `belongsToMany` or `morphToMany`. A dirty save updates linked timestamps in bulk and follows each related model’s own touch declarations without firing its `Saved` event for the bulk update. During pivot writes, a matching declaration on the parent refreshes the related models, and a matching inverse declaration on the related model refreshes the parent. OpenRust matches model types, pivot table, key mappings, morph alias, and pivot filters. Use the same descriptor for the declaration and the write. For the inverse, swap the parent and related keys. `withTouchingRelated()` and `withTouchingParent()` also enable touching explicitly.

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::BelongsToMany};

    Touches::default().belongsToMany(
        BelongsToMany::<Self, Role>::new("role_user", "user_id", "role_id", "id", "id"),
    )
}
```

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Model, relations::{BelongsToMany, ToggleChanges}}};

pub async fn toggle_roles<U: Model, R: Model>(user: &U, keys: &[Value]) -> DatabaseResult<ToggleChanges> {
    BelongsToMany::<U, R>::new("role_user", "user_id", "role_id", "id", "id")
        .withTouchingParent()
        .withTouchingRelated()
        .toggleOrFail(user, keys)
        .await
}
```
