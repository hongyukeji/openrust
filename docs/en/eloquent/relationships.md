# Eloquent Relationships

## Defining Relationships

Declare `HasOne`, `HasMany`, or `BelongsTo` with explicit keys. For example, `HasMany::<Author, Post>::new("id", "author_id")` joins an author's key to the post's foreign key. `BelongsTo::<Post, Author>::new("author_id", "id")` describes the inverse. Use `getResults` to fetch the relation for a single model.

`BelongsToMany` takes the intermediate table, its parent and related foreign keys, and the two model keys. For an `author_post` table, use `BelongsToMany::<Author, Post>::new("author_post", "author_id", "post_id", "id", "id")`.

### Automatically Hydrating Parent Models on Children {#automatically-hydrating-parent-models-on-children}

Use `chaperone("author")` on `HasMany` or `HasOne` to cache the parent on each child. `MorphMany` and `MorphOne` cache a `MorphTarget` with the relation’s alias. The inverse name is explicit in Rust. Children share a parent snapshot without its loaded relations, so reading the inverse avoids a query and the relationship graph stays acyclic. `inverse` is an alias. Call `withoutChaperone` or `withoutInverse` to turn it off.

```rust
use openrust::database::eloquent::{Model, relations::HasMany};

pub fn posts<P: Model, R: Model>() -> HasMany<P, R> {
    HasMany::new("id", "author_id").chaperone("author")
}
```

### Scoped Relationships {#scoped-relationships}

Use `withAttributes` inside the relation’s `constrain` callback to add defaults to newly created models. Pass `true` to also add equality conditions to relation queries, or `false` to supply defaults only. Explicit model values take precedence. Direct and polymorphic relations assign their own parent keys and morph type.

```rust
use openrust::database::eloquent::{Attributes, Model, relations::HasMany};

pub fn featured_posts<P: Model, R: Model>() -> HasMany<P, R> {
    HasMany::new("id", "author_id").constrain(|query| {
        query.withAttributes(
            Attributes::from([("featured".into(), true.into())]),
            true,
        )
    })
}
```

### Associating an owner {#associating-an-owner}

Name a `BelongsTo` with `.named("author")` to identify its cache entry. `associate` sets the foreign key from the supplied owner and caches its current snapshot. `associateKey` sets the key directly and invalidates that cache entry so the next access reloads the owner. `dissociate` and its alias `disassociate` clear the foreign key and cache a loaded empty relationship. Other loaded relations remain available. Call `save().await?` to persist the child.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::BelongsTo}};

pub async fn set_author<P: Model, A: Model>(post: &mut P, author: &A) -> DatabaseResult<()> {
    let relation = BelongsTo::new("author_id", "id").named("author");
    relation.associate(post, author)?;
    post.save().await?;
    Ok(())
}
```

### Comparing related models {#comparing-related-models}

Use `is` and `isNot` to compare a candidate with a singular relationship. Pass the declaring model and `Some(candidate)`, or `None` for a missing candidate. `BelongsTo`, `MorphTo`, and ordinary `HasOne` or `MorphOne` compare relationship keys, table, and connection without loading rows. `MorphTo` resolves the stored alias first. `HasOne`, `MorphOne`, and `HasOneThrough` use `.await?`. A through relation or one-of-many selection checks the candidate with a SQL existence query.

```rust
use openrust::database::eloquent::{Model, relations::BelongsTo};

pub fn is_author<P: Model, A: Model>(post: &P, author: &A) -> bool {
    BelongsTo::new("author_id", "id").is(post, Some(author))
}
```

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

For a `MorphTo` relationship, register a typed `withDefault` for each alias that needs a fallback. The callback receives the new unsaved target and its parent. A stored alias selects its own default when the key or matching row is missing. Use `defaultForMissingType` to explicitly choose a registered default for a null or empty type. An unknown alias with a foreign key still returns an error.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::MorphTo}};

pub fn subject_with_default<P: Model, T: Model>() -> DatabaseResult<MorphTo<P>> {
    MorphTo::new("subject_type", "subject_id")
        .target::<T>("post", "id")
        .withDefault::<T>("post", |post, _| {
            post.setAttribute("title", "Unavailable");
        })?
        .defaultForMissingType("post")
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

Reuse existing relationship keys with `Model::through(local).has(distant)`. Pass typed descriptors for both relationships. The result is a `HasManyThrough` if either descriptor is to-many, and a `HasOneThrough` otherwise. The composition carries the intermediate morph discriminator when present. Configure additional query conditions on the returned through relationship.

```rust
use openrust::database::eloquent::{Model, relations::{HasMany, HasManyThrough}};

pub fn comments<P: Model, I: Model, R: Model>(
    posts: HasMany<P, I>,
    comments: HasMany<I, R>,
) -> HasManyThrough<P, I, R> {
    P::through(posts).has(comments)
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

### Custom polymorphic types {#custom-polymorphic-types}

Register application aliases with `MorphMap::morphMap` or use `enforceMorphMap` to require registration when resolving model aliases. `getMorphClass` returns the first alias for the model. With a permissive map, an unmapped model uses its Rust type name. `targetMapped` adds all registered aliases for a compiled target, including aliases retained when merging a map. `MorphOne::mapped`, `MorphMany::mapped` and `MorphToMany::mapped` use the model’s application alias for reads and writes. Explicit-alias descriptors remain available.

```rust
use openrust::{
    database::{Connection, DatabaseResult, eloquent::{
        Model, relations::{MorphMap, MorphMapEntry, MorphTo},
    }},
    foundation::Application,
};

pub fn subjects<P: Model, R: Model>(
    app: &Application,
    connection: &Connection,
) -> DatabaseResult<MorphTo<P>> {
    MorphMap::enforceMorphMap(app, [MorphMapEntry::model::<R>("post")], true)?;
    MorphTo::new("subject_type", "subject_id").targetMapped::<R>(connection, "id")
}
```

### Associating polymorphic models {#associating-polymorphic-models}

Name the `MorphTo` descriptor with `named("subject")` to update that relationship immediately. `associate` sets the stored alias and owner key and caches a snapshot of the supplied model. `dissociate` clears both columns and caches an empty relationship. Call `save().await?` to persist the parent. Other loaded relationships remain available. Unnamed descriptors retain the existing cache-clearing behavior.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::MorphTo}};

pub async fn set_subject<P: Model, T: Model>(parent: &mut P, post: &T) -> DatabaseResult<()> {
    let subject = MorphTo::new("subject_type", "subject_id")
        .named("subject")
        .target::<T>("post", "id");
    subject.associate(parent, "post", post)?;
    parent.save().await?;
    Ok(())
}
```

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

Use the same descriptor with `whereHas`, `whereDoesntHave`, `hasCount`, and `withCount` when the registered relation supports existence queries. The resolver receives an unsaved model on the query connection, so its conditions must work without an individual parent row.

```rust
async fn orders_with_customer(
    connection: &openrust::database::Connection,
) -> DatabaseResult<openrust::database::eloquent::Collection<Order>> {
    let customer = eager_customer();
    Order::queryOn(connection).whereHas(&customer)?.get().await
}
```

To share a named resolver between composed models, register an explicit fallback with `inheritRelationUsing`. The first closure supplies the base model, and the second adapts the relation to the new parent type. For `HasMany`, `forParent` carries the keys and query constraints forward. A local registration takes priority. Fallbacks resolve within the same application and reject cycles.

```rust
use openrust::database::eloquent::relations::HasMany;

fn inherit_items(app: &Application) -> DatabaseResult<()> {
    SpecialOrder::inheritRelationUsing(
        app,
        "items",
        |order| Order::from_state(order.state().clone()),
        HasMany::<Order, Item>::forParent,
    )
}
```

## Querying Relationship Existence

`has` and `whereHas` keep parents with a matching related row. Use `whereDoesntHave` to keep parents without a match. These methods use a correlated subquery on the same database connection. `withWhereHas` applies the same relation to filtering and eager loading.

`hasCount` compares the related row count with a number. For a `MorphTo` relation, pass explicit aliases to `whereHasMorph`, `whereDoesntHaveMorph`, or `hasMorphCount`. Use `morphWithCount` to register a target type with a count on one of its own relationships.

Use `whereHasPath("posts.comments")` to find authors with a post that has comments. Use `whereDoesntHavePath` to find authors with no complete matching chain, including authors with no posts. `hasPathCount("posts.comments", ">=", 2)` requires at least one post with two or more comments. Each earlier segment uses existence and the count applies to the final segment. Register queryable descriptors with `namedRelations().nested(...)` in the model declaration.

### Filtering by related columns {#filtering-by-related-columns}

Pass a typed relationship, column, operator, and value to `whereRelation` or `whereDoesntHaveRelation`. The absence variant matches parents with no related row satisfying the condition. `withWhereRelation` also eager loads the matching rows. Use the `orWhere…` variants to add alternatives to the parent query. For a `MorphTo` relationship, use `whereMorphRelation` or `whereMorphDoesntHaveRelation` and pass the registered aliases whose target tables expose the selected column.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_visible_posts<A: Model, P: Model>(
    connection: &Connection,
    posts: HasMany<A, P>,
) -> DatabaseResult<Collection<A>> {
    A::queryOn(connection)
        .withWhereRelation("posts", posts, "published", "=", true)?
        .get()
        .await
}
```

### Filtering a named relationship path {#filtering-named-relationship-paths}

Use `whereRelationPath` and `whereDoesntHaveRelationPath` to apply a condition to the final relation of a registered path. Their OR variants combine the entire path with the parent query. `whereRelationPathWith` receives a grouped set of conditions and the final table qualifier, which may be an alias for a self-relation. Use `withWhereRelationPath` or `withWhereRelationPathWith` to apply the same filter to the loaded final relation. Earlier relations load normally. For a polymorphic segment, each applicable registered target must support the remaining path and its columns.

```rust
use openrust::database::{DatabaseResult, eloquent::{Builder, Model}};

pub fn matching_posts<M: Model>(
    query: Builder<'static, M>,
) -> DatabaseResult<Builder<'static, M>> {
    query.withWhereRelationPath("posts.comments", "approved", "=", true)
}
```

Use the asynchronous `whereRelationPathAll` or `whereDoesntHaveRelationPathAll` to discover the stored aliases before applying a final column predicate. `withWhereRelationPathAll` and `withWhereRelationPathAllWith` share the discovered aliases between filtering and eager loading. Every discovered target must support the remaining path. Alias discovery reads the model table independently of the parent query's filters and scopes. The selected aliases are fixed for that builder.

```rust
use openrust::database::{DatabaseResult, eloquent::{Builder, Model}};

pub async fn approved_subject_comments<M: Model>(
    query: Builder<'static, M>,
) -> DatabaseResult<Builder<'static, M>> {
    query.withWhereRelationPathAll("subject.comments", "approved", "=", true).await
}
```

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

### Deferred aggregate loading {#deferred-aggregate-loading}

Use `loadCount`, `loadExists`, `loadSum`, `loadAvg`, `loadMin`, or `loadMax` on an existing model. Use `loadAggregates` to fetch several values in one query. Results are stored as original attributes and unrelated pending edits stay dirty.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, RelationAggregate, RelationAggregateQuery, relations::HasMany},
};

pub async fn load_post_totals<P: Model, R: Model>(author: &mut P) -> DatabaseResult<()> {
    let posts = HasMany::<P, R>::new("id", "author_id");
    author.loadAggregates(&[
        RelationAggregateQuery::new("posts", &posts, RelationAggregate::Count),
        RelationAggregateQuery::new("posts", &posts, RelationAggregate::Exists),
    ]).await
}
```

### Default Relationship Counts

Implement `Model::withDefaultCounts` to add count projections whenever the model is read. This corresponds to Laravel's `$withCount` property. The counts share the model's SELECT. An explicit `select` replaces the default projection.

```rust
fn withDefaultCounts(query: Builder<'static, Self>) -> DatabaseResult<Builder<'static, Self>> {
    query.withCount("posts", &HasMany::<Self, Post>::new("id", "author_id"))
}
```

### Eager loading by morph target {#morph-target-eager-loading}

Use `morphWith` on a registered target alias to declare typed eager loads. Use `morphWithPaths` for registered paths. These settings compose with existing target constraints and `morphWithCount` when they refer to the same target type and owner key. An unknown alias or a mismatched type returns an error from `morphWith`.

```rust
use openrust::database::{
    DatabaseResult, eloquent::{Model, relations::{HasMany, MorphTo}},
};

pub fn configure_subjects<M: Model, P: Model, C: Model>() -> DatabaseResult<MorphTo<M>> {
    MorphTo::new("subject_type", "subject_id")
        .target::<P>("post", "id")
        .morphWith::<P>("post", |loads| {
            loads.with("comments", HasMany::<P, C>::new("id", "post_id"))
        })
}
```

### Soft-deleted morph targets {#soft-deleted-morph-targets}

Use `withTrashed()`, `withoutTrashed()`, or `onlyTrashed()` on a `MorphTo` relation. These options affect loaded targets that declare a soft-delete column. Targets without one load normally, including in a mixed group. The options run before target-specific constraints. As in Laravel, explicit live or deleted predicates accumulate when chained. A later `withTrashed()` removes the default scope and preserves those explicit predicates.

```rust
use openrust::database::eloquent::{Model, relations::MorphTo};

pub fn subject_including_deleted<M: Model, P: Model, A: Model>() -> MorphTo<M> {
    MorphTo::new("subject_type", "subject_id")
        .target::<P>("post", "id")
        .target::<A>("author", "id")
        .withTrashed()
}
```

### Loading Polymorphic Target Groups

Use `loadMorph` to reload a typed relationship on one target group, or `loadMorphPaths::<Post>("subject", &subjects, "post", &["comments.post"])` to reload registered paths. Loaded values are available through the parent relation and its JSON projection. A previously held target reference still points to its earlier snapshot. Both methods are available on models and collections. Use `loadMorphCount`, `loadMorphAggregate`, or `loadMorphAggregates` to write deferred aggregates onto the cached targets, where they are available through the parent relation and its JSON projection. Choose an alias for each call. Aggregates for one alias share a query per key batch, and other aliases keep their own values. Single models also support `loadMorphAggregate` and `loadMorphAggregates`.

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

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model, RelationAggregate, RelationAggregateQuery, relations::{HasMany, MorphTo}},
};

pub async fn load_subject_paths<M: Model, T: Model>(
    parents: &mut Collection<M>,
    subjects: &MorphTo<M>,
) -> DatabaseResult<()> {
    parents.loadMorphPaths::<T>("subject", subjects, "post", &["comments.post"]).await
}

pub async fn load_subject_totals<M: Model, T: Model, R: Model>(
    parents: &mut Collection<M>,
    subjects: &MorphTo<M>,
    children: &HasMany<T, R>,
) -> DatabaseResult<()> {
    parents.loadMorphAggregates("subject", subjects, "post", &[
        RelationAggregateQuery::new("comments as has_comments", children, RelationAggregate::Exists),
        RelationAggregateQuery::new("comments", children, RelationAggregate::Count),
    ]).await
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

To read loaded posts, use `getRelation::<RelatedCollection<Post>>("posts")`. Multi-model relations invoke the related model’s `newCollection` for each parent. The resulting keys are available via `keys` and `getByKey`, and non-list keys are preserved in JSON objects. The collection holds `Arc<T>` snapshots so callers can retain a loaded model. Singular relations use `Option<Arc<T>>`. An absent relation name indicates the relation has yet to be loaded. Use `load` to load it after retrieval and `loadMissing` to preserve values already loaded.

Consuming an autoload-enabled collection with `into_iter` or `toBase` lets the moved models share a batch of peer snapshots. The first missing access loads the peers together. Once it completes, the other models can read their results through `relationLoaded`, `getRelation`, and JSON serialization without another access or query. Explicitly unsetting the relationship or changing the model’s attributes or connection makes the next access query that model directly.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn moved_author_post_counts<Author: Model, Post: Model>(
    authors: Collection<Author>,
) -> DatabaseResult<Vec<usize>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let mut counts = Vec::new();
    for mut author in authors.withRelationshipAutoloading() {
        counts.push(author.getRelationOrLoad("posts", &posts).await?.len());
    }
    Ok(counts)
}
```

### Nested eager loading by path {#nested-eager-loading}

Register each segment in the corresponding model’s `namedRelations()`. Pass paths such as `posts.comments` to `withPaths`. Shared prefixes in one call are loaded together, so sibling paths reuse the same parent query. Use `loadPaths` on an existing model or collection. Unknown or malformed paths return an error.

Use `loadMissingPaths(&["posts.comments"])` on a model or collection to fill gaps along registered paths. Already-loaded relations, including empty results, keep their values. Missing descendants are loaded in batches, including separate groups for polymorphic targets, and are visible in the root model’s serialization. When adding descendants to a shared related model, OpenRust installs a new snapshot in the loaded relationship. A previously held `Arc` continues to refer to the earlier snapshot.

Register a `MorphTo` with `NamedRelations::morphTo` to use paths such as `subject.details` for eager loading, existence, absence, and terminal counts. Each registered target must declare `details`, and those declarations may use different relation types. Eager loading batches by target alias. Existence queries correlate each branch with its alias, and absence includes null or missing targets. Use typed per-target configuration when the child names differ.

Use the asynchronous `whereHasPathAll`, `whereDoesntHavePathAll`, or `hasPathCountAll` to discover the stored aliases at each polymorphic segment before building the query. Discovery reads distinct aliases from the model table independently of the outer query filters and scopes. Only discovered aliases need the child path. An unregistered stored alias returns an error.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, NamedRelations, relations::HasMany},
};

// In the Model implementation for Author:
fn namedRelations() -> NamedRelations<Self> {
    NamedRelations::default().nested("posts", || HasMany::<Self, Post>::new("id", "author_id"))
}

// Post registers "comments" in its own namedRelations implementation.
async fn authors_with_comments(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    Author::queryOn(connection).withPaths(&["posts.comments"])?.get().await
}
```

### Eager loading specific columns {#eager-loading-specific-columns}

Add a colon and comma-separated columns after the final relation in each path. Use a separate entry to select columns on an intermediate relation. Include the primary and foreign keys needed to match the models and to load any deeper relationships. For a polymorphic target, the listed columns must be available on each selected target. `loadMissingPaths` keeps existing relation values.

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::{Collection, Model}};

pub async fn authors_with_selected_columns<A: Model>(
    connection: &Connection,
) -> DatabaseResult<Collection<A>> {
    A::queryOn(connection)
        .withPaths(&["posts:id,author_id,title", "posts.comments:id,subject_id,body"])?
        .get()
        .await
}
```

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

Use `modelAt(index)` to borrow a model from its collection and call `getRelationOrLoad` on that model. An invalid index returns `None`. The mutable borrow keeps the current collection available across the async call, so loaded values are visible on its siblings after the borrow ends.

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
        if let Some(mut author) = authors.modelAt(index) {
            counts.push(author.getRelationOrLoad("posts", &posts).await?.len());
        }
    }
    Ok(counts)
}
```

### Loading a Relationship Chain

When an owned model comes from an autoload-enabled collection, `getRelationOrLoad` also prepares the returned models for batched access. This applies to previously eager-loaded relations and to polymorphic targets grouped by alias. To keep a related model beyond the borrow, copy its state into an owned model. The owned model shares the batch. Newly loaded descendant relationships are also visible through the linked `Arc` models, including when serializing the root model. Direct attribute edits remain local to the owned model.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn moved_post_comment_counts<Author: Model, Post: Model, Comment: Model>(
    authors: Collection<Author>,
) -> DatabaseResult<Vec<usize>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let comments = HasMany::<Post, Comment>::new("id", "post_id");
    let mut counts = Vec::new();
    for mut author in authors.withRelationshipAutoloading() {
        for post in author.getRelationOrLoad("posts", &posts).await? {
            let mut post = Post::from_state(post.state().clone());
            counts.push(post.getRelationOrLoad("comments", &comments).await?.len());
        }
    }
    Ok(counts)
}
```

Use `relatedModelAt` on a model borrowed from the collection to access one related model. With automatic eager loading enabled, a missing relationship on that model is loaded across all related models reached by the same parent relationship. The root collection retains the results for later reads and serialization. With automatic eager loading disabled, the access follows the selected model’s lazy-loading policy. An empty relationship or out-of-range position returns `None`.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn first_comment_count<Author: Model, Post: Model, Comment: Model>(
    authors: &mut Collection<Author>,
) -> DatabaseResult<usize> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let comments = HasMany::<Post, Comment>::new("id", "post_id");
    let Some(mut author) = authors.modelAt(0) else {
        return Ok(0);
    };
    let Some(mut post) = author.relatedModelAt("posts", &posts, 0).await? else {
        return Ok(0);
    };
    Ok(post.getRelationOrLoad("comments", &comments).await?.len())
}
```

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

A borrowed related model can call `relatedModelAt` or `morphModel` again. Each step retains the original collection. For example, follow a comment’s subject to a post, then the post’s author, and access the author’s posts. The first missing access batches that step across the root collection and makes the result available to subsequent reads and serialization.

For a polymorphic parent relationship, call `morphModel::<Post>("subject", &subjects, "post")` on a borrowed root model. It validates the alias and target type and returns `None` when the root has no target of that alias. On first access to a missing relationship, automatic eager loading batches only targets with the selected alias. Results are written back to the root collection.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model, relations::{BelongsTo, HasMany, MorphTo}},
};

pub async fn first_subject_author_post_count<Root: Model, Post: Model, Author: Model>(
    roots: &mut Collection<Root>,
) -> DatabaseResult<usize> {
    let subjects = MorphTo::<Root>::new("subject_type", "subject_id")
        .target::<Post>("post", "id");
    let author = BelongsTo::<Post, Author>::new("author_id", "id");
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let Some(mut root) = roots.modelAt(0) else {
        return Ok(0);
    };
    let Some(mut post) = root.morphModel::<Post>("subject", &subjects, "post").await? else {
        return Ok(0);
    };
    let Some(mut author) = post.relatedModelAt("author", &author, 0).await? else {
        return Ok(0);
    };
    Ok(author.getRelationOrLoad("posts", &posts).await?.len())
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

### Recursively saving models and relationships {#recursively-saving-models-and-relationships}

Call `push` to save a model and the related models already in its cache. `setRelation` can place an edited snapshot in that cache. The save visits relations in their cache order and stops if a model cancels or returns an error. Earlier writes remain committed. `pushQuietly` suppresses model events across the traversal. Re-read the relation from the root for saved snapshots. Previously retained `Arc` references still point to the original snapshots.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, RelatedCollection, relations::HasMany},
};

pub async fn save_posts<P: Model, R: Model>(
    author: &mut P,
    posts: RelatedCollection<R>,
) -> DatabaseResult<bool> {
    author.setRelation::<HasMany<P, R>>("posts", posts);
    author.push().await
}
```

### Saving related models in batches {#saving-related-models-in-batches}

For direct and polymorphic one-to-one or one-to-many relations, `saveMany` saves a mutable slice in order. `createMany` accepts attribute maps and returns the new models in input order. The `Quietly` variants suppress model events for the entire operation. Each model is written separately. If a later write returns an error, earlier writes remain committed.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::HasMany}};

pub async fn create_posts<P: Model, R: Model>(
    posts: &HasMany<P, R>,
    author: &P,
) -> DatabaseResult<Vec<R>> {
    posts.createManyQuietly(author, [
        Attributes::from([("title".into(), "First post".into())]),
        Attributes::from([("title".into(), "Second post".into())]),
    ]).await
}
```

Use `forceCreate` or `forceCreateMany` for trusted attributes that must bypass the model’s mass-assignment policy. Casts and mutators still run, and the relationship sets its own foreign key and morph type. Use `forceCreateQuietly` or `forceCreateManyQuietly` to suppress model events.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::HasMany}};

pub async fn import_post<P: Model, R: Model>(
    posts: &HasMany<P, R>,
    author: &P,
) -> DatabaseResult<R> {
    posts.forceCreateQuietly(author, Attributes::from([
        ("id".into(), 42.into()),
        ("title".into(), "Imported post".into()),
    ])).await
}
```

For many-to-many links, use `attach`, `detach`, `sync`, or `syncWithoutDetaching`. Synchronization adds and removes links in one database transaction. Use a composite unique key over the two pivot foreign keys to prevent duplicate links.

Use `withPivot(["label"])` to select extra columns from the intermediate table. Read them using `getRelation::<Pivot>("pivot")` on the related model. Use `r#as("membership")` to change the accessor. `attachWith` writes additional columns when creating links, and `updateExistingPivot` updates an existing link and returns its affected-row count.

`syncWithPivotValues` applies the same attributes to every key. Use `syncWithPivotAttributes` with `(key, attributes)` pairs for individual values. Both return attached, detached, and updated keys. A failed sync rolls back its changes.

### Saving and creating many-to-many models {#saving-many-to-many-models}

Use `save` to persist a related model and attach it, or `create` to construct it from attributes and attach it. Supply pivot attributes separately. Each model and its link are written in one transaction. `saveMany` and `createMany` match pivot attributes by input position and touch configured endpoints after the batch succeeds. Earlier rows remain committed if a later row fails. The `Quietly` variants suppress model events. `forceCreate` and `forceCreateQuietly` bypass mass-assignment guards while still running casts.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::BelongsToMany}};

pub async fn create_roles<P: Model, R: Model>(
    roles: &BelongsToMany<P, R>,
    user: &P,
) -> DatabaseResult<Vec<R>> {
    roles.createMany(
        user,
        [Attributes::from([("name".into(), "Editor".into())])],
        &[Attributes::from([("source".into(), "manual".into())])],
    ).await
}
```

### Toggling associations {#toggling-associations}

Use `toggle` to remove existing links and insert missing links for the supplied keys. `toggleWithPivotAttributes` supplies per-key attributes for inserts. Both `toggle` and `toggleOrFail` run in one transaction and return attached and detached keys.

### Transactional pivot writes {#transactional-pivot-writes}

Use `attachOrFail`, `detachOrFail`, and `updateExistingPivotOrFail` to run the write, custom pivot events, and configured touches in one transaction. `syncOrFail`, `syncWithoutDetachingOrFail`, and `syncWithPivotValuesOrFail` share the transaction already used by the ordinary sync methods. The same methods work on polymorphic many-to-many relations.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Attributes, Model, relations::BelongsToMany},
};

pub async fn add_roles<P: Model, R: Model>(
    relation: &BelongsToMany<P, R>,
    user: &P,
    keys: &[Value],
) -> DatabaseResult<()> {
    relation.attachOrFail(user, keys, Attributes::new()).await
}
```

### Creating or attaching a related model {#creating-or-attaching-related-models}

Use `createOrFirst` on a many-to-many relation to insert a related model and attach it in one transaction. Pass the lookup attributes, additional creation values, and pivot attributes separately. On a unique conflict, the framework uses the write connection to find the model and attaches it if needed. A matching existing link keeps its pivot values. Back the lookup attributes with a unique constraint.

```rust
use std::sync::Arc;
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, relations::BelongsToMany},
};

pub async fn ensure_role<P: Model, R: Model>(
    relation: &BelongsToMany<P, R>,
    user: &P,
) -> DatabaseResult<Arc<R>> {
    relation.createOrFirst(
        user,
        Attributes::from([("slug".into(), "editor".into())]),
        Attributes::from([("name".into(), "Editor".into())]),
        Attributes::new(),
    ).await
}
```

`firstOrCreate` first searches the current relation, then looks for an existing model to attach, and creates a model if neither lookup matches. `updateOrCreate` follows the same lookup order and applies the supplied values to an existing related model. Both apply pivot attributes only when adding a link.

```rust
use std::sync::Arc;
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, relations::BelongsToMany},
};

pub async fn update_role<P: Model, R: Model>(
    relation: &BelongsToMany<P, R>,
    user: &P,
) -> DatabaseResult<Arc<R>> {
    relation.updateOrCreate(
        user,
        Attributes::from([("slug".into(), "editor".into())]),
        Attributes::from([("name".into(), "Editor".into())]),
        Attributes::new(),
    ).await
}
```

Direct, polymorphic, and through relations also provide `firstOrNew`, `firstOrCreate`, `createOrFirst`, and `updateOrCreate`. `firstOrNew` returns an unsaved model on a miss. Direct and polymorphic relations accept deferred values and populate their own relation keys. Through relations take attribute maps and use a supplied foreign key when creating a related row. `restoreOrCreate` and `createOrRestore` include soft-deleted related rows and restore them through the model lifecycle. Deleted intermediate rows remain subject to `withTrashedParents`.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::HasMany}};

pub async fn ensure_post<P: Model, R: Model>(
    posts: &HasMany<P, R>,
    author: &P,
) -> DatabaseResult<R> {
    posts.firstOrCreate(
        author,
        Attributes::from([("slug".into(), "welcome".into())]),
        Attributes::from([("title".into(), "Welcome".into())]),
    ).await
}
```

### Bulk upserting related models {#upserting-related-models}

Direct and polymorphic one-to-one and one-to-many relations provide `upsert`. Each input row receives the parent key and, for a polymorphic relation, the morph type. The write runs as one database statement and skips per-model events and casts. Eloquent supplies missing IDs and timestamps where configured.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::HasMany}};

pub async fn upsert_posts<P: Model, R: Model>(
    posts: &HasMany<P, R>,
    author: &P,
) -> DatabaseResult<u64> {
    posts.upsert(
        author,
        vec![Attributes::from([
            ("slug".into(), "welcome".into()),
            ("title".into(), "Welcome".into()),
        ])],
        &["author_id", "slug"],
        Some(&["title"]),
    ).await
}
```

Use a unique index that includes the parent key and, for polymorphic relations, the morph type to keep conflicts separate. SQLite uses the supplied conflict columns. MySQL detects conflicts using the table’s primary and unique indexes. An ordinary relationship query filter does not scope an upsert conflict.

Through and many-to-many relations forward `upsert` to the related model’s table. For through relations, include the intermediate foreign key in each row. For ordinary and polymorphic many-to-many relations, use `attach` or `sync` separately when you need to write links to the pivot table.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, relations::HasManyThrough},
};

pub async fn upsert_comments<P: Model, I: Model, R: Model>(
    comments: &HasManyThrough<P, I, R>,
    author: &P,
    post_id: i64,
) -> DatabaseResult<u64> {
    comments.upsert(
        author,
        vec![Attributes::from([
            ("post_id".into(), post_id.into()),
            ("slug".into(), "welcome".into()),
            ("body".into(), "Welcome".into()),
        ])],
        &["post_id", "slug"],
        Some(&["body"]),
    ).await
}
```

`BelongsTo` forwards `upsert` to the owner table. `MorphTo` selects the owner table from the declaring model’s registered morph alias. A nonempty batch requires a valid alias in OpenRust. Each call uses the supplied conflict columns and leaves the declaring model’s association as it was.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, relations::MorphTo},
};

pub async fn upsert_owners<P: Model>(
    owner: &MorphTo<P>,
    comment: &P,
    rows: Vec<Attributes>,
) -> DatabaseResult<u64> {
    owner.upsert(comment, rows, &["id"], None).await
}
```

### Restoring related models {#restoring-related-models}

Use `restoreOrCreate` to look up a related model, including soft-deleted rows, before inserting. `createOrRestore` attempts insertion first and recovers a matching row on a unique conflict. Both restore the model through its normal lifecycle. Pass `RelatedValues::deferred` to compute model values only when an insert or update needs them.

```rust
use std::sync::Arc;
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, relations::{BelongsToMany, RelatedValues}},
};

pub async fn restore_role<P: Model, R: Model>(
    relation: &BelongsToMany<P, R>,
    user: &P,
) -> DatabaseResult<Arc<R>> {
    relation.restoreOrCreate(
        user,
        Attributes::from([("slug".into(), "editor".into())]),
        RelatedValues::deferred(|| Attributes::from([("name".into(), "Editor".into())])),
        Attributes::new(),
    ).await
}
```

### Creating or incrementing a related counter {#incrementing-related-counters}

Use `incrementOrCreate` to attach a matching model or create it with an initial counter. For an existing model, the database adds the step atomically and writes any extra model attributes in the same update. The counter update fires `Updating` and `Updated`. Give the lookup attributes a unique constraint. The initial counter and step are explicit SQL numbers in Rust.

```rust
use std::sync::Arc;
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, relations::BelongsToMany},
};

pub async fn count_tag<P: Model, R: Model>(
    tags: &BelongsToMany<P, R>,
    post: &P,
) -> DatabaseResult<Arc<R>> {
    tags.incrementOrCreate(
        post,
        Attributes::from([("name".into(), "rust".into())]),
        "count",
        1_i64,
        1_i64,
        Attributes::new(),
    ).await
}
```

The same method is available on `HasOne`, `HasMany`, `MorphOne`, `MorphMany`, `HasOneThrough`, and `HasManyThrough`. Direct relations set the parent foreign key on creation. Polymorphic relations also set the discriminator. Through relations use the join for lookup. When creating through a relation, supply the related row’s foreign key to an existing intermediate model.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::HasMany}};

pub async fn count_view<P: Model, R: Model>(
    views: &HasMany<P, R>,
    article: &P,
) -> DatabaseResult<R> {
    views.incrementOrCreate(
        article,
        Attributes::from([("source".into(), "web".into())]),
        "count",
        1_i64,
        1_i64,
        Attributes::new(),
    ).await
}
```

### Finding or Making a Related Model {#finding-or-making-a-related-model}

Use `findOrNew` to look up a related primary key under the relationship’s constraints. If the key is absent, the method returns an unsaved model without assigning the requested primary key. Direct and polymorphic child relations apply the parent key, morph discriminator and configured creation defaults. Through and many-to-many relations return an unattached model on the relevant connection. A matching many-to-many model includes its Pivot.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Model, relations::HasMany},
};

async fn find_or_draft<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
    id: Value,
) -> DatabaseResult<R> {
    children.findOrNew(parent, id).await
}
```

Use `find`, `findOrFail` or `findSole` to read a related primary key. Use `sole` to require exactly one result for the relationship. Both unique-result methods read at most two candidates and distinguish a missing model from multiple matches. `findOr` and `firstOr` run an async fallback only if the query succeeds without a match. The fallback returns the same model type.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Attributes, Model, relations::HasMany},
};

async fn find_or_prepare<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
    id: Value,
) -> DatabaseResult<R> {
    children.findOr(parent, id, || async {
        children.make(parent, Attributes::new()).await
    }).await
}
```

Use `findMany(parent, keys)` to find a batch of related primary keys. It returns the matching models in relationship query order, with any custom collection keys and loaded Pivot data. Repeated input keys do not repeat a matching row. Missing, soft-deleted, or out-of-scope records are omitted. An empty input returns the model’s empty collection without a query.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Collection, Model, relations::HasMany},
};

async fn find_related_batch<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
    keys: &[Value],
) -> DatabaseResult<Collection<R>> {
    children.findMany(parent, keys).await
}
```

Use `findManyOrFail(parent, keys)` when every requested primary key is required. Use `findManyOr(parent, keys, fallback)` to supply an async fallback returning a collection when any key is missing. OpenRust checks coverage by primary key, so repeated Pivot rows cannot hide a missing key. Query errors propagate to the caller.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Collection, Model, relations::HasMany},
};

async fn require_related_batch<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
    keys: &[Value],
) -> DatabaseResult<Collection<R>> {
    children.findManyOrFail(parent, keys).await
}
```

### Relationship Cursors {#relationship-cursors}

Call `cursor(parent)` on a direct, through, or many-to-many relationship to stream its models. The cursor applies the parent key, morph discriminator and intermediate-table conditions. Many-to-many cursors load the Pivot on each model before yielding it. Use the model cursor rules for connection lifetime, early termination and eager loading.

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{DatabaseResult, eloquent::{Model, relations::HasMany}};

async fn first_child<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
) -> DatabaseResult<Option<R>> {
    let mut models = children.cursor(parent);
    models.try_next().await
}
```

For an inverse polymorphic relationship, `MorphTo::cursor(parent)` selects the registered model by the parent’s morph type and yields `MorphTarget` values. Use `alias()` or `downcast_ref::<M>()` to access a target. A missing key or type yields an empty stream, and an unknown type with a key returns an error. The stream yields database rows only. Configured default models are available through relationship loading.

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{DatabaseResult, eloquent::{Model, relations::{MorphTarget, MorphTo}}};

async fn first_target<P: Model>(
    parent: &P,
    subject: &MorphTo<P>,
) -> DatabaseResult<Option<MorphTarget>> {
    let mut targets = subject.cursor(parent);
    targets.try_next().await
}
```

### Chunking Results {#chunking-results}

Call `chunk(parent, count, callback)` on a direct child, through or many-to-many relationship. Each batch preserves the relation constraints and the configured order. For many-to-many relationships, models supplied to the callback already have their Pivot. The callback follows the [Eloquent chunking rules](getting-started.md#chunking-results).

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::HasMany}};

async fn process_children<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
) -> DatabaseResult<bool> {
    children.chunk(parent, 200, |models, page| async move {
        println!("{}: {}", page, models.len());
        Ok(true)
    }).await
}
```

For inverse relationships, use `BelongsTo::chunk(child, count, callback)` or `MorphTo::chunk(parent, count, callback)`. The callbacks receive persisted models. `BelongsTo` supplies a typed Eloquent collection. `MorphTo` selects the registered target using the morph type and supplies a support `Collection<MorphTarget>` that preserves the selected model’s collection keys. Use `alias()` or `downcast_ref::<M>()` on each target. Missing references produce zero batches. Configured default models are available through relationship loading.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::MorphTo}};

async fn process_subject<P: Model>(parent: &P, subject: &MorphTo<P>) -> DatabaseResult<bool> {
    subject.chunk(parent, 100, |targets, _page| async move {
        for (_, target) in targets {
            println!("{}", target.alias());
        }
        Ok(true)
    }).await
}
```

Many-to-many relationships support `chunkById(parent, count, callback, column, alias)` and `chunkByIdDesc`. They default to the qualified related key and hydrate the Pivot for every model before invoking the callback.

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
