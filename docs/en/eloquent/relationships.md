# Eloquent Relationships

## Defining Relationships

Declare `HasOne`, `HasMany`, or `BelongsTo` with explicit keys. For example, `HasMany::<Author, Post>::new("id", "author_id")` joins an author's key to the post's foreign key. `BelongsTo::<Post, Author>::new("author_id", "id")` describes the inverse. Use `get_results` to fetch the relation for a single model.

For `BelongsTo`, the owner key may be a column other than the primary key. Use a unique owner key so both loading modes resolve the same row. If several rows match, a single lazy access or `get_results` takes the first query result, while eager loading keeps the last row for each owner key. A foreign key of zero is valid. A null foreign key returns the default, when configured, without issuing a query. For an enum-cast column, use `set_enum` to store the backing value of a typed enum foreign key.

`BelongsToMany` takes the intermediate table, its parent and related foreign keys, and the two model keys. For an `author_post` table, use `BelongsToMany::<Author, Post>::new("author_post", "author_id", "post_id", "id", "id")`.

### Automatically Hydrating Parent Models on Children {#automatically-hydrating-parent-models-on-children}

Use `chaperone("author")` on `HasMany` or `HasOne` to cache the parent on each child. `MorphMany` and `MorphOne` cache a `MorphTarget` with the relation’s alias. The inverse name is explicit in Rust. Children share a parent snapshot without its loaded relations, so reading the inverse avoids a query and the relationship graph stays acyclic. `inverse` is an alias. Call `without_chaperone` or `without_inverse` to turn it off.

```rust
use openrust::database::eloquent::{Model, relations::HasMany};

pub fn posts<P: Model, R: Model>() -> HasMany<P, R> {
    HasMany::new("id", "author_id").chaperone("author")
}
```

### Scoped Relationships {#scoped-relationships}

Use `with_attributes` inside the relation’s `constrain` callback to add defaults to newly created models. Pass `true` to also add equality conditions to relation queries, or `false` to supply defaults only. Explicit model values take precedence. Direct and polymorphic relations assign their own parent keys and morph type.

```rust
use openrust::database::eloquent::{Attributes, Model, relations::HasMany};

pub fn featured_posts<P: Model, R: Model>() -> HasMany<P, R> {
    HasMany::new("id", "author_id").constrain(|query| {
        query.with_attributes(
            Attributes::from([("featured".into(), true.into())]),
            true,
        )
    })
}
```

### Associating an owner {#associating-an-owner}

Name a `BelongsTo` with `.named("author")` to identify its cache entry. `associate` sets the foreign key from the supplied owner and caches its current snapshot. `associate_key` sets the key directly and invalidates that cache entry so the next access reloads the owner. `dissociate` and its alias `disassociate` clear the foreign key and cache a loaded empty relationship. Other loaded relations remain available. Call `save().await?` to persist the child.

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

Use `is` and `is_not` to compare a candidate with a singular relationship. Pass the declaring model and `Some(candidate)`, or `None` for a missing candidate. `BelongsTo`, `MorphTo`, and ordinary `HasOne` or `MorphOne` compare relationship keys, table, and connection without loading rows. `MorphTo` resolves the stored alias first. `HasOne`, `MorphOne`, and `HasOneThrough` use `.await?`. A through relation or one-of-many selection checks the candidate with a SQL existence query.

```rust
use openrust::database::eloquent::{Model, relations::BelongsTo};

pub fn is_author<P: Model, A: Model>(post: &P, author: &A) -> bool {
    BelongsTo::new("author_id", "id").is(post, Some(author))
}
```

### Default Models

Use `with_default` on `BelongsTo`, `HasOne`, `HasOneThrough`, or `MorphOne` to supply an unsaved model when the related row is absent. The closure receives the new model and the parent. Use `|_, _| {}` for a default with no extra attributes. `HasOne` initializes the foreign key, and `MorphOne` also initializes the type alias. A fallback is available through the loaded relationship. Relationship existence and count queries still reflect stored rows.

```rust
use openrust::database::eloquent::{Model, relations::BelongsTo};

pub fn author_with_default() -> BelongsTo<Post, Author> {
    BelongsTo::new("author_id", "id").with_default(|author: &mut Author, _| {
        author.set_attribute("name", "Guest author");
    })
}
```

For a `MorphTo` relationship, register a typed `with_default` for each alias that needs a fallback. The callback receives the new unsaved target and its parent. A stored alias selects its own default when the key or matching row is missing. Use `default_for_missing_type` to explicitly choose a registered default for a null or empty type. An unknown alias with a foreign key still returns an error.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::MorphTo}};

pub fn subject_with_default<P: Model, T: Model>() -> DatabaseResult<MorphTo<P>> {
    MorphTo::new("subject_type", "subject_id")
        .target::<T>("post", "id")
        .with_default::<T>("post", |post, _| {
            post.set_attribute("title", "Unavailable");
        })?
        .default_for_missing_type("post")
}
```

### Has One of Many

Convert a `HasMany` or `MorphMany` descriptor with `one`, then choose `latest_of_many`, `oldest_of_many`, or `of_many`. With no column argument, the latest and oldest methods use the related primary key. When ranking by another column, a tied value is resolved using the maximum primary key unless you specify another tie rule. Use `where_candidates` to filter before ranking. SQL min and max ignore nulls, so a parent with only null ranking values has no result.

Pass several ranking columns as a slice, for example `oldest_of_many(&["score", "id"])`. Columns are evaluated from left to right. The shortcut applies the same aggregate to every supplied column. When the primary key is absent, the final tie-breaker uses its maximum value. An explicitly empty slice also selects the maximum key. Pass `None` to apply the shortcut’s own direction directly to the primary key.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_top_post<Author: Model, Post: Model>(
    connection: &Connection,
) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    Author::query_on(connection)
        .with("top_post", posts.one().latest_of_many(&["score", "id"]))
        .order_by("id", "asc")
        .get()
        .await
}
```

### Has One Through and Has Many Through

Use `HasOneThrough` or `HasManyThrough` to query through an intermediate model. The constructor takes the intermediate model's foreign key, the final model's foreign key, the parent model's local key, and the intermediate model's local key. For example, an author has comments through posts. The intermediate and related models must use the same joined database connection.

Soft-deleted intermediate rows are excluded. Call `with_trashed_parents` to include them. The related model's own soft-delete scope still applies.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasManyThrough},
};

pub async fn authors_with_comments(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    let comments = HasManyThrough::<Author, Post, Comment>::new("author_id", "post_id", "id", "id");
    Author::query_on(connection).with("comments", comments).order_by("id", "asc").get().await
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

Convert a `HasManyThrough` relationship with `one`, then choose `latest_of_many`, `oldest_of_many`, or `of_many`. Ranking spans all eligible intermediate rows for each parent. `where_candidates` filters the final model before ranking. The intermediate and final model soft-delete rules still apply.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasManyThrough},
};

pub async fn authors_with_latest_comment(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    let comments = HasManyThrough::<Author, Post, Comment>::new("author_id", "post_id", "id", "id");
    Author::query_on(connection)
        .with("latest_comment", comments.one().latest_of_many(None))
        .order_by("id", "asc")
        .get()
        .await
}
```

### Polymorphic Relationships

Use `MorphOne` or `MorphMany` to query children using both an ID and a type alias. Use `MorphTo` to resolve the inverse relation, and register each alias with its model type. Eager loading queries each distinct type in a batch. Read the loaded value using `get_relation::<Option<MorphTarget>>("subject")` and use `downcast_ref::<Post>()` on the target.

Each registered alias can specify its own owner key. An empty type column has no target and can use the configured default. The text alias `"0"` is resolved like any other registered alias. For a non-unique owner key, direct or single-model lazy lookup uses the first query row, and eager loading keeps the last row for each key within each alias.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::MorphTo},
};

pub async fn comments_with_subjects<Comment: Model, Author: Model, Post: Model>(
    connection: &Connection,
) -> DatabaseResult<Collection<Comment>> {
    let subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id");
    Comment::query_on(connection).with("subject", subjects).order_by("id", "asc").get().await
}
```

`MorphToMany` adds a type discriminator to intermediate-table reads and writes. For the inverse direction, reverse the two pivot key arguments and use the same alias. The pivot value is available as `MorphPivot`.

### Custom polymorphic types {#custom-polymorphic-types}

Register application aliases with `MorphMap::morph_map` or use `enforce_morph_map` to require registration when resolving model aliases. `get_morph_class` returns the first alias for the model. With a permissive map, an unmapped model uses its Rust type name. `target_mapped` adds all registered aliases for a compiled target, including aliases retained when merging a map. `MorphOne::mapped`, `MorphMany::mapped` and `MorphToMany::mapped` use the model’s application alias for reads and writes. Explicit-alias descriptors remain available.

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
    MorphMap::enforce_morph_map(app, [MorphMapEntry::model::<R>("post")], true)?;
    MorphTo::new("subject_type", "subject_id").target_mapped::<R>(connection, "id")
}
```

### Associating polymorphic models {#associating-polymorphic-models}

Name the `MorphTo` descriptor with `named("subject")` to update that relationship immediately. `associate` sets the stored alias and owner key and caches a snapshot of the supplied model. `dissociate` clears both columns and caches an empty relationship. Call `save().await?` to persist the parent. Other loaded relationships remain available. Unnamed descriptors retain the existing cache-clearing behavior.

Association stores the owner key registered for the alias, including zero. When selecting a subset of owner columns, include that key for association and batch matching. A missing, null, or unsupported owner key and an empty alias return an error before the child or its relationship cache is changed.

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

Register a named resolver with `Model::resolve_relation_using` in a service provider’s `boot` method. The closure receives the current model and returns a relationship descriptor with explicit keys. Each application has its own registry. Registering the same name and model type again replaces the resolver for future loads.

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{DynamicRelation, Model, relations::BelongsTo}},
    foundation::Application,
};

type CustomerRelation = BelongsTo<Order, Customer>;

fn register(app: &Application) -> DatabaseResult<()> {
    Order::resolve_relation_using(app, "customer", |_| {
        CustomerRelation::new("customer_id", "id")
    })
}

fn eager_customer() -> DynamicRelation<CustomerRelation> {
    DynamicRelation::new("customer")
}
```

Use `get_dynamic_relation_or_load` for cached access, or use `DynamicRelation` with `with`, `load`, and `load_missing`. The descriptor type is explicit in Rust. The resolver can select different query constraints for each model instance, so dynamic eager loading can issue one query per parent. A hydrated model resolves against its connection’s application.

Use the same descriptor with `where_has`, `where_doesnt_have`, `has_count`, and `with_count` when the registered relation supports existence queries. The resolver receives an unsaved model on the query connection, so its conditions must work without an individual parent row.

```rust
async fn orders_with_customer(
    connection: &openrust::database::Connection,
) -> DatabaseResult<openrust::database::eloquent::Collection<Order>> {
    let customer = eager_customer();
    Order::query_on(connection).where_has(&customer)?.get().await
}
```

To share a named resolver between composed models, register an explicit fallback with `inherit_relation_using`. The first closure supplies the base model, and the second adapts the relation to the new parent type. For `HasMany`, `for_parent` carries the keys and query constraints forward. A local registration takes priority. Fallbacks resolve within the same application and reject cycles.

```rust
use openrust::database::eloquent::relations::HasMany;

fn inherit_items(app: &Application) -> DatabaseResult<()> {
    SpecialOrder::inherit_relation_using(
        app,
        "items",
        |order| Order::from_state(order.state().clone()),
        HasMany::<Order, Item>::for_parent,
    )
}
```

## Querying Relationship Existence

`has` and `where_has` keep parents with a matching related row. Use `where_doesnt_have` to keep parents without a match. These methods use a correlated subquery on the same database connection. `with_where_has` applies the same relation to filtering and eager loading.

`has_count` compares the related row count with a number. For a `MorphTo` relation, pass explicit aliases to `where_has_morph`, `where_doesnt_have_morph`, or `has_morph_count`. Use `morph_with_count` to register a target type with a count on one of its own relationships.

Use `where_has_path("posts.comments")` to find authors with a post that has comments. Use `where_doesnt_have_path` to find authors with no complete matching chain, including authors with no posts. `has_path_count("posts.comments", ">=", 2)` requires at least one post with two or more comments. Each earlier segment uses existence and the count applies to the final segment. Register queryable descriptors with `named_relations().nested(...)` in the model declaration.

### Filtering by related columns {#filtering-by-related-columns}

Pass a typed relationship, column, operator, and value to `where_relation` or `where_doesnt_have_relation`. The absence variant matches parents with no related row satisfying the condition. `with_where_relation` also eager loads the matching rows. Use the `or_where…` variants to add alternatives to the parent query. For a `MorphTo` relationship, use `where_morph_relation` or `where_morph_doesnt_have_relation` and pass the registered aliases whose target tables expose the selected column.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_visible_posts<A: Model, P: Model>(
    connection: &Connection,
    posts: HasMany<A, P>,
) -> DatabaseResult<Collection<A>> {
    A::query_on(connection)
        .with_where_relation("posts", posts, "published", "=", true)?
        .get()
        .await
}
```

### Filtering a named relationship path {#filtering-named-relationship-paths}

Use `where_relation_path` and `where_doesnt_have_relation_path` to apply a condition to the final relation of a registered path. Their OR variants combine the entire path with the parent query. `where_relation_path_with` receives a grouped set of conditions and the final table qualifier, which may be an alias for a self-relation. Use `with_where_relation_path` or `with_where_relation_path_with` to apply the same filter to the loaded final relation. Earlier relations load normally. For a polymorphic segment, each applicable registered target must support the remaining path and its columns.

```rust
use openrust::database::{DatabaseResult, eloquent::{Builder, Model}};

pub fn matching_posts<M: Model>(
    query: Builder<'static, M>,
) -> DatabaseResult<Builder<'static, M>> {
    query.with_where_relation_path("posts.comments", "approved", "=", true)
}
```

Use the asynchronous `where_relation_path_all` or `where_doesnt_have_relation_path_all` to discover the stored aliases before applying a final column predicate. `with_where_relation_path_all` and `with_where_relation_path_all_with` share the discovered aliases between filtering and eager loading. Every discovered target must support the remaining path. Alias discovery reads the model table independently of the parent query's filters and scopes. The selected aliases are fixed for that builder.

```rust
use openrust::database::{DatabaseResult, eloquent::{Builder, Model}};

pub async fn approved_subject_comments<M: Model>(
    query: Builder<'static, M>,
) -> DatabaseResult<Builder<'static, M>> {
    query.with_where_relation_path_all("subject.comments", "approved", "=", true).await
}
```

### Querying All Related Models

Use the asynchronous `where_has_morph_all`, `where_doesnt_have_morph_all`, and `has_morph_count_all` methods for wildcard behavior. They first read distinct morph aliases from the model table, independently of the current query's filters and scopes. Register the stored aliases on the relation. When looking for absent targets, `where_doesnt_have_morph_all` also matches a null morph type.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::MorphTo},
};

pub async fn comments_with_existing_subject(connection: &Connection) -> DatabaseResult<Collection<Comment>> {
    let subjects = MorphTo::<Comment>::new("subject_type", "subject_id")
        .target::<Author>("author", "id")
        .target::<Post>("post", "id");
    Comment::query_on(connection)
        .where_has_morph_all(&subjects)
        .await?
        .order_by("id", "asc")
        .get()
        .await
}
```

### Filtering by Relationship Identity

Use `where_belongs_to` with a typed relation and a slice of owners. It compares the foreign key against the configured owner key. Use `where_attached_to` to match rows linked through a many-to-many relationship. For a polymorphic relation, use `identity` to obtain a type and owner key pair, then pass one or more pairs to `where_morphed_to` or `where_not_morphed_to`. Use `where_morph_type` to filter by alias alone or pass `None` for an unset type.

Use `or_where_belongs_to`, `or_where_attached_to`, `or_where_morphed_to`, or `or_where_not_morphed_to` to add a relationship identity as an alternative to the preceding conditions. `where_attached_to` and `or_where_attached_to` also accept a `MorphToMany` descriptor and apply its intermediate-table discriminator. Global scopes and soft-delete filters still apply.

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
    Comment::query_on(connection)
        .where_morphed_to(&subjects, &[subjects.identity("author", author)?, subjects.identity("post", post)?])?
        .get()
        .await
}
```

## Aggregating Related Models

Use `with_count`, `with_exists`, `with_sum`, `with_avg`, `with_min`, or `with_max` to select relation attributes in the parent query. The example exposes `posts_count`, `posts_exists`, and `posts_sum_score`. Use a name such as `"posts as published_count"` for an explicit alias. Apply any parent `select` before the aggregate methods. An empty sum is `Null`, and existence is a Boolean.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_post_totals(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    Author::query_on(connection)
        .with_count("posts", &posts)?
        .with_exists("posts", &posts)?
        .with_sum("posts", &posts, "score")?
        .order_by("id", "asc")
        .get()
        .await
}
```

### Deferred aggregate loading {#deferred-aggregate-loading}

Use `load_count`, `load_exists`, `load_sum`, `load_avg`, `load_min`, or `load_max` on an existing model. Use `load_aggregates` to fetch several values in one query. Results are stored as original attributes and unrelated pending edits stay dirty.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, RelationAggregate, RelationAggregateQuery, relations::HasMany},
};

pub async fn load_post_totals<P: Model, R: Model>(author: &mut P) -> DatabaseResult<()> {
    let posts = HasMany::<P, R>::new("id", "author_id");
    author.load_aggregates(&[
        RelationAggregateQuery::new("posts", &posts, RelationAggregate::Count),
        RelationAggregateQuery::new("posts", &posts, RelationAggregate::Exists),
    ]).await
}
```

### Default Relationship Counts

Implement `Model::with_default_counts` to add count projections whenever the model is read. This corresponds to `$with_count` property. The counts share the model's SELECT. An explicit `select` replaces the default projection.

```rust
fn with_default_counts(query: Builder<'static, Self>) -> DatabaseResult<Builder<'static, Self>> {
    query.with_count("posts", &HasMany::<Self, Post>::new("id", "author_id"))
}
```

### Eager loading by morph target {#morph-target-eager-loading}

Use `morph_with` on a registered target alias to declare typed eager loads. Use `morph_with_paths` for registered paths. These settings compose with existing target constraints and `morph_with_count` when they refer to the same target type and owner key. An unknown alias or a mismatched type returns an error from `morph_with`.

```rust
use openrust::database::{
    DatabaseResult, eloquent::{Model, relations::{HasMany, MorphTo}},
};

pub fn configure_subjects<M: Model, P: Model, C: Model>() -> DatabaseResult<MorphTo<M>> {
    MorphTo::new("subject_type", "subject_id")
        .target::<P>("post", "id")
        .morph_with::<P>("post", |loads| {
            loads.with("comments", HasMany::<P, C>::new("id", "post_id"))
        })
}
```

### Soft-deleted morph targets {#soft-deleted-morph-targets}

Use `with_trashed()`, `without_trashed()`, or `only_trashed()` on a `MorphTo` relation. These options affect loaded targets that declare a soft-delete column. Targets without one load normally, including in a mixed group. The options run before target-specific constraints. As in the framework, explicit live or deleted predicates accumulate when chained. A later `with_trashed()` removes the default scope and preserves those explicit predicates.

```rust
use openrust::database::eloquent::{Model, relations::MorphTo};

pub fn subject_including_deleted<M: Model, P: Model, A: Model>() -> MorphTo<M> {
    MorphTo::new("subject_type", "subject_id")
        .target::<P>("post", "id")
        .target::<A>("author", "id")
        .with_trashed()
}
```

### Loading Polymorphic Target Groups

Use `load_morph` to reload a typed relationship on one target group, or `load_morph_paths::<Post>("subject", &subjects, "post", &["comments.post"])` to reload registered paths. Loaded values are available through the parent relation and its JSON projection. A previously held target reference still points to its earlier snapshot. Both methods are available on models and collections. Use `load_morph_count`, `load_morph_aggregate`, or `load_morph_aggregates` to write deferred aggregates onto the cached targets, where they are available through the parent relation and its JSON projection. Choose an alias for each call. Aggregates for one alias share a query per key batch, and other aliases keep their own values. Single models also support `load_morph_aggregate` and `load_morph_aggregates`.

Single models also provide `load_morph_max`, `load_morph_min`, `load_morph_sum` and `load_morph_avg`. Pass `(name, morph, alias, related_name, relation, column)`; each method uses the same typed target and aggregate engine as `load_morph_aggregate`. Related-model global scopes remain active, an empty related set yields SQL NULL, and null or other target groups are left unchanged. Use `"comments as total"` for a custom result attribute. Driver-specific numeric result types are preserved; failures leave the cached target snapshot intact.

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
    let authors = comments.load_morph_count("subject", &subjects, "author", "posts", &posts).await?;
    Ok((0..authors.all().len())
        .filter_map(|index| authors.get_attribute(index, "posts_count").cloned())
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
    parents.load_morph_paths::<T>("subject", subjects, "post", &["comments.post"]).await
}

pub async fn load_subject_totals<M: Model, T: Model, R: Model>(
    parents: &mut Collection<M>,
    subjects: &MorphTo<M>,
    children: &HasMany<T, R>,
) -> DatabaseResult<()> {
    parents.load_morph_aggregates("subject", subjects, "post", &[
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
    Author::query_on(connection)
        .with("posts", HasMany::<Author, Post>::new("id", "author_id"))
        .order_by("id", "asc")
        .get()
        .await
}
```

To read loaded posts, use `get_relation::<RelatedCollection<Post>>("posts")`. Multi-model relations invoke the related model’s `new_collection` for each parent. The resulting keys are available via `keys` and `get_by_key`, and non-list keys are preserved in JSON objects. The collection holds `Arc<T>` snapshots so callers can retain a loaded model. Singular relations use `Option<Arc<T>>`. An absent relation name indicates the relation has yet to be loaded. Use `load` to load it after retrieval and `load_missing` to preserve values already loaded.

Consuming an autoload-enabled collection with `into_iter` or `to_base` lets the moved models share a batch of peer snapshots. The first missing access loads the peers together. Once it completes, the other models can read their results through `relation_loaded`, `get_relation`, and JSON serialization without another access or query. Explicitly unsetting the relationship or changing the model’s attributes or connection makes the next access query that model directly.

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
    for mut author in authors.with_relationship_autoloading() {
        counts.push(author.get_relation_or_load("posts", &posts).await?.len());
    }
    Ok(counts)
}
```

### Nested eager loading by path {#nested-eager-loading}

Register each segment in the corresponding model’s `named_relations()`. Pass paths such as `posts.comments` to `with_paths`. Shared prefixes in one call are loaded together, so sibling paths reuse the same parent query. Use `load_paths` on an existing model or collection. Unknown or malformed paths return an error.

Use `load_missing_paths(&["posts.comments"])` on a model or collection to fill gaps along registered paths. Already-loaded relations, including empty results, keep their values. Missing descendants are loaded in batches, including separate groups for polymorphic targets, and are visible in the root model’s serialization. When adding descendants to a shared related model, OpenRust installs a new snapshot in the loaded relationship. A previously held `Arc` continues to refer to the earlier snapshot.

Use `load_missing_path_with` when a missing terminal relationship needs a predicate. The closure receives the grouped query conditions and the terminal table qualifier. It can be combined with column selection on that path. Loaded results, including empty ones, are preserved. For a polymorphic path the closure must use columns available for each target it can reach.

```rust
use openrust::database::{DatabaseResult, eloquent::{Collection, Model}};

pub async fn fill_selected_comments<M: Model>(
    authors: &mut Collection<M>,
    comment_id: i64,
) -> DatabaseResult<()> {
    authors
        .load_missing_path_with("posts.comments:id,subject_id,body", move |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "=", comment_id)
        })
        .await
}
```

Pass several `MissingRelation` entries to `load_missing_paths_with` to configure paths in one call. Each entry can supply its own predicate. A parent shared by several paths is loaded together. If a path is repeated, the framework applies the later predicate first and then the earlier one to the same condition group.

```rust
use openrust::database::{DatabaseResult, eloquent::{Collection, MissingRelation, Model}};

pub async fn fill_post_relations<M: Model>(
    authors: &mut Collection<M>,
    comment_id: i64,
) -> DatabaseResult<()> {
    authors
        .load_missing_paths_with([
            MissingRelation::new("posts.author"),
            MissingRelation::new("posts.comments:id,subject_id,body").constrain(
                move |conditions, table| conditions.r#where(&format!("{table}.id"), "=", comment_id),
            ),
        ])
        .await
}
```



Register a `MorphTo` with `NamedRelations::morph_to` to use paths such as `subject.details` for eager loading, existence, absence, and terminal counts. Each registered target must declare `details`, and those declarations may use different relation types. Eager loading batches by target alias. Existence queries correlate each branch with its alias, and absence includes null or missing targets. Use typed per-target configuration when the child names differ.

Use the asynchronous `where_has_path_all`, `where_doesnt_have_path_all`, or `has_path_count_all` to discover the stored aliases at each polymorphic segment before building the query. Discovery reads distinct aliases from the model table independently of the outer query filters and scopes. Only discovered aliases need the child path. An unregistered stored alias returns an error.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, NamedRelations, relations::HasMany},
};

// In the Model implementation for Author:
fn named_relations() -> NamedRelations<Self> {
    NamedRelations::default().nested("posts", || HasMany::<Self, Post>::new("id", "author_id"))
}

// Post registers "comments" in its own namedRelations implementation.
async fn authors_with_comments(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    Author::query_on(connection).with_paths(&["posts.comments"])?.get().await
}
```

### Eager loading specific columns {#eager-loading-specific-columns}

Add a colon and comma-separated columns after the final relation in each path. Use a separate entry to select columns on an intermediate relation. Include the primary and foreign keys needed to match the models and to load any deeper relationships. For a polymorphic target, the listed columns must be available on each selected target. `load_missing_paths` keeps existing relation values.

An explicit selection for an intermediate relation applies even when it follows a nested path in the same call. If several entries select columns for the same relation path, the first explicit selection wins. For `load_missing_paths`, an existing relation keeps the columns with which it was loaded.

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::{Collection, Model}};

pub async fn authors_with_selected_columns<A: Model>(
    connection: &Connection,
) -> DatabaseResult<Collection<A>> {
    A::query_on(connection)
        .with_paths(&["posts:id,author_id,title", "posts.comments:id,subject_id,body"])?
        .get()
        .await
}

pub async fn fill_missing_comments<A: Model>(
    authors: &mut Collection<A>,
) -> DatabaseResult<()> {
    authors
        .load_missing_paths(&["posts:id,author_id", "posts.comments:id,subject_id,body"])
        .await
}
```

### Eager Loading by Default

Implement `Model::with_default_relations` to declare the typed relationships loaded by default. This is the model's equivalent of `$with` property.

```rust
fn with_default_relations(relations: EagerLoads<Self>) -> EagerLoads<Self> {
    relations.with("posts", HasMany::<Self, Post>::new("id", "author_id"))
}
```

For one query, use `without` to remove named relationships or `with_only` to replace the eager loads. Calling `with` with the same name replaces that relationship's definition for the current query.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_only_mentees(connection: &Connection) -> DatabaseResult<Collection<Author>> {
    Author::query_on(connection)
        .with_only(|relations| relations.with("mentees", HasMany::<Author, Author>::new("id", "mentor_id")))
        .order_by("id", "asc")
        .get()
        .await
}
```

### Constraining Eager Loads

Use `constrain` to configure the related model's query. Call `with` in that closure for nested eager loading. The example filters authors and then loads their matching posts and each post's author. In a related query, `limit` and `offset` apply separately to each parent. Add `order_by` for a predictable choice of related rows.

For direct, through, and many-to-many eager loads, the database applies the requested limit and offset within each parent’s group. Pivot attributes remain available and the row rank is omitted from the loaded models.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::{BelongsTo, HasMany}},
};

pub async fn authors_with_matching_posts<Author: Model, Post: Model>(
    connection: &Connection,
) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id").constrain(|mut query| {
        query.r#where("title", "like", "First%");
        query.with("author", BelongsTo::<Post, Author>::new("author_id", "id"))
    });

    Author::query_on(connection).with_where_has("posts", posts)?.get().await
}
```

### Automatic Eager Loading

Call `with_relationship_autoloading` on a collection, then await `get_relation_or_load` to access a model's typed relationship. The first missing access batches the load across that collection. Previously loaded values, including empty results, are reused. Enable it by default for a model by returning `true` from `Model::automatically_eager_load_relationships`.

Use `model_at(index)` to borrow a model from its collection and call `get_relation_or_load` on that model. An invalid index returns `None`. The mutable borrow keeps the current collection available across the async call, so loaded values are visible on its siblings after the borrow ends.

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Model, relations::HasMany},
};

pub async fn author_post_counts(connection: &Connection) -> DatabaseResult<Vec<usize>> {
    let mut authors = Author::query_on(connection)
        .order_by("id", "asc")
        .get()
        .await?
        .with_relationship_autoloading();
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let mut counts = Vec::with_capacity(authors.len());
    for index in 0..authors.len() {
        if let Some(mut author) = authors.model_at(index) {
            counts.push(author.get_relation_or_load("posts", &posts).await?.len());
        }
    }
    Ok(counts)
}
```

### Loading a Relationship Chain

When an owned model comes from an autoload-enabled collection, `get_relation_or_load` also prepares the returned models for batched access. This applies to previously eager-loaded relations and to polymorphic targets grouped by alias. To keep a related model beyond the borrow, copy its state into an owned model. The owned model shares the batch. Newly loaded descendant relationships are also visible through the linked `Arc` models, including when serializing the root model. Direct attribute edits remain local to the owned model.

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
    for mut author in authors.with_relationship_autoloading() {
        for post in author.get_relation_or_load("posts", &posts).await? {
            let mut post = Post::from_state(post.state().clone());
            counts.push(post.get_relation_or_load("comments", &comments).await?.len());
        }
    }
    Ok(counts)
}
```

Use `related_model_at` on a model borrowed from the collection to access one related model. With automatic eager loading enabled, a missing relationship on that model is loaded across all related models reached by the same parent relationship. The root collection retains the results for later reads and serialization. With automatic eager loading disabled, the access follows the selected model’s lazy-loading policy. An empty relationship or out-of-range position returns `None`.

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
    let Some(mut author) = authors.model_at(0) else {
        return Ok(0);
    };
    let Some(mut post) = author.related_model_at("posts", &posts, 0).await? else {
        return Ok(0);
    };
    Ok(post.get_relation_or_load("comments", &comments).await?.len())
}
```

Use `related_models` to open a batch of models reached through a singular or plural relationship. It reuses already-loaded values. Call `get_relation_or_load` on that batch to fetch the next relationship across its models, or call `related_models` again to traverse another level. Results fetched on a batch are cached for that batch's lifetime. Shared instances appear once, and empty relationships contribute no models.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, relations::HasMany},
};

pub async fn count_post_comments(authors: &mut Collection<Author>) -> DatabaseResult<usize> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    let comments = HasMany::<Post, Comment>::new("id", "post_id");
    let mut posts = authors.related_models("posts", &posts).await?;
    Ok(posts.related_models("comments", &comments).await?.all().len())
}
```

A borrowed related model can call `related_model_at` or `morph_model` again. Each step retains the original collection. For example, follow a comment’s subject to a post, then the post’s author, and access the author’s posts. The first missing access batches that step across the root collection and makes the result available to subsequent reads and serialization.

For a polymorphic parent relationship, call `morph_model::<Post>("subject", &subjects, "post")` on a borrowed root model. It validates the alias and target type and returns `None` when the root has no target of that alias. On first access to a missing relationship, automatic eager loading batches only targets with the selected alias. Results are written back to the root collection.

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
    let Some(mut root) = roots.model_at(0) else {
        return Ok(0);
    };
    let Some(mut post) = root.morph_model::<Post>("subject", &subjects, "post").await? else {
        return Ok(0);
    };
    let Some(mut author) = post.related_model_at("author", &author, 0).await? else {
        return Ok(0);
    };
    Ok(author.get_relation_or_load("posts", &posts).await?.len())
}
```

### Preventing Lazy Loading

Override `Model::prevents_lazy_loading` to reject implicit single-model relation queries when accessing persisted models. Already-loaded relations, explicit `load`, and collection autoloading remain available. The default policy allows unsaved and recently created models.

```rust
fn prevents_lazy_loading(&self) -> bool {
    true
}
```

Override `handle_lazy_loading_violation_using` to handle the typed `DatabaseError::LazyLoadingViolation`. Return `Ok(())` to allow the query to proceed after recording the violation.

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::{Model, relations::HasMany}};

pub async fn author_post_count(connection: &Connection, id: i64) -> DatabaseResult<usize> {
    let mut author = Author::query_on(connection).find_or_fail(id).await?;
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    author.load("posts", &posts).await?;
    Ok(author.get_relation_or_load("posts", &posts).await?.len())
}
```

## Filtering by Intermediate Table Columns {#filtering-queries-via-intermediate-table-columns}

Use `where_pivot` for a comparison, `where_pivot_in` or `where_pivot_not_in` for a list, `where_pivot_between` or `where_pivot_not_between` for an inclusive range, and `where_pivot_null` or `where_pivot_not_null` for null checks. The corresponding `or_where_pivot…` methods combine alternatives inside the same owner and morph scope. Filters apply to loading, existence queries, and selecting existing links for updates, detach, and sync.

Use `where_pivot_with` and `or_where_pivot_with` for grouped conditions on `BelongsToMany` or `MorphToMany`. The closure receives query `Conditions`, which can be passed to a reusable scope function. Unqualified columns inside the group refer to the pivot table, including inside nested groups.

```rust
use openrust::database::{
    eloquent::{Model, relations::BelongsToMany},
    query::Conditions,
};

fn active(conditions: Conditions) -> Conditions {
    conditions.r#where("muted", "=", false)
}

pub fn visible_members<P: Model, R: Model>() -> BelongsToMany<P, R> {
    BelongsToMany::new("project_user", "project_id", "user_id", "id", "id")
        .where_pivot("role", "=", "admin")
        .or_where_pivot_with(active)
}
```


Use `with_pivot_value` to apply an equality filter and supply the same value when inserting a link through `attach` or `sync`. Explicit insert attributes take precedence. The default must be non-null and cannot replace relationship keys.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::BelongsToMany}};

pub fn approved_roles<U: Model, R: Model>() -> DatabaseResult<BelongsToMany<U, R>> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .with_pivot(["approved"])
        .with_pivot_value("approved", true)
}
```

## Ordering by Intermediate Table Columns {#ordering-queries-via-intermediate-table-columns}

Use `order_by_pivot("position", "asc")` or `order_by_pivot_desc("created_at")` to order related models by a pivot column. Ordering applies to lazy and eager loads, including polymorphic many-to-many relationships.

Use `with_timestamps()` to select and maintain the pivot timestamps. Column names follow the parent model, with `created_at` and `updated_at` as fallbacks. Use `with_timestamp_columns(Some("linked_at"), None)?` for a custom creation column with no update timestamp. Inserts initialize both configured columns and accept explicit values. Updating an existing link refreshes the update timestamp and preserves the creation timestamp unless the caller changes it explicitly.

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn latest_roles<U: Model, R: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .with_timestamps()
        .order_by_pivot_desc("created_at")
}
```

## Defining Custom Intermediate Table Models {#defining-custom-intermediate-table-models}

Use `using::<Membership>()` on a many-to-many relationship to load and write intermediate rows as a typed Eloquent model. Declare the model’s casts and fillable attributes as usual. Read the result with `get_relation::<Membership>("membership")` when the accessor is renamed. The relation supplies the table, connection, and original link keys, including the morph type for polymorphic links. Use `incrementing(false)` for a composite-key table. An incrementing pivot requires an actual primary key column.

New custom pivot rows start with the model’s `new_instance` defaults. Relationship keys, pivot defaults, and explicit values are then applied before `Saving` and `Creating` run. On update, the original attributes come from the existing row and the dirty set reflects the pending changes.

When a custom pivot is hydrated, selected database values override model defaults, including SQL NULL. Unselected non-key fields can keep their model defaults. Relationship identity comes from the database row, and the hydrated values form the original snapshot used for change tracking and JSON output.

Relationship writes use the custom model’s lifecycle and cast rules. Use the relationship’s `with_timestamps` or `with_timestamp_columns` to configure pivot timestamps. For soft deletes, promote the intermediate row to an ordinary Eloquent model.

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn roles<U: Model, R: Model, Membership: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .using::<Membership>()
        .with_pivot(["approved"])
        .r#as("membership")
}
```

### Automatically Hydrating Pivot Relationships {#automatically-hydrating-pivot-relationships}

Declare the inverse names on your pivot model with `pivot_inverse_relations()`. Call `chaperone()` after `using::<Membership>()` to infer names from the pivot keys and model types. Use `chaperone_with(Some("owner"), Some("item"))?` for explicit names and `without_chaperone()` to disable hydration.

Both lazy and eager loads populate the inverses from the models already loaded. Read them as `Option<Arc<T>>` through `get_relation`. The inverses hold snapshots of the endpoints at hydration time so Rust can serialize the graph without a reference cycle.

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

Call `push` to save a model and the related models already in its cache. `set_relation` can place an edited snapshot in that cache. The save visits relations in their cache order and stops if a model cancels or returns an error. Earlier writes remain committed. `push_quietly` suppresses model events across the traversal. Re-read the relation from the root for saved snapshots. Previously retained `Arc` references still point to the original snapshots.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, RelatedCollection, relations::HasMany},
};

pub async fn save_posts<P: Model, R: Model>(
    author: &mut P,
    posts: RelatedCollection<R>,
) -> DatabaseResult<bool> {
    author.set_relation::<HasMany<P, R>>("posts", posts);
    author.push().await
}
```

### Saving related models in batches {#saving-related-models-in-batches}

For direct and polymorphic one-to-one or one-to-many relations, `save_many` saves a mutable slice in order. `create_many` accepts attribute maps and returns the new models in input order. The `Quietly` variants suppress model events for the entire operation. Each model is written separately. If a later write returns an error, earlier writes remain committed.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::HasMany}};

pub async fn create_posts<P: Model, R: Model>(
    posts: &HasMany<P, R>,
    author: &P,
) -> DatabaseResult<Vec<R>> {
    posts.create_many_quietly(author, [
        Attributes::from([("title".into(), "First post".into())]),
        Attributes::from([("title".into(), "Second post".into())]),
    ]).await
}
```

Use `force_create` or `force_create_many` for trusted attributes that must bypass the model’s mass-assignment policy. Casts and mutators still run, and the relationship sets its own foreign key and morph type. Use `force_create_quietly` or `force_create_many_quietly` to suppress model events.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::HasMany}};

pub async fn import_post<P: Model, R: Model>(
    posts: &HasMany<P, R>,
    author: &P,
) -> DatabaseResult<R> {
    posts.force_create_quietly(author, Attributes::from([
        ("id".into(), 42.into()),
        ("title".into(), "Imported post".into()),
    ])).await
}
```

For many-to-many links, use `attach`, `detach`, `sync`, or `sync_without_detaching`. Synchronization adds and removes links in one database transaction. Use a composite unique key over the two pivot foreign keys to prevent duplicate links.

Use `with_pivot(["label"])` to select extra columns from the intermediate table. Read them using `get_relation::<Pivot>("pivot")` on the related model. Use `r#as("membership")` to change the accessor. `attach_with` writes additional columns when creating links, and `update_existing_pivot` updates an existing link and returns its affected-row count.

`sync_with_pivot_values` applies the same attributes to every key. Use `sync_with_pivot_attributes` with `(key, attributes)` pairs for individual values. Both return attached, detached, and updated keys. A failed sync rolls back its changes.

For `sync` and `toggle`, pass database-backed enum keys via `CastableEnum::into_value()`. If the related model declares an integer, string, or enum cast for the related key, synchronization uses that backing type for matching and change reports. Text keys retain UUIDs, leading zeros, and digits outside the signed integer range.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, casts::CastableEnum, relations::BelongsToMany},
};

async fn sync_roles<P: Model, R: Model, E: CastableEnum>(
    user: &P,
    roles: &BelongsToMany<P, R>,
    selected: E,
) -> DatabaseResult<()> {
    let keys = [selected.into_value(), 6_i64.into()];
    roles.sync(user, &keys).await?;
    Ok(())
}
```

### Saving and creating many-to-many models {#saving-many-to-many-models}

Use `save` to persist a related model and attach it, or `create` to construct it from attributes and attach it. Supply pivot attributes separately. Each model and its link are written in one transaction. `save_many` and `create_many` match pivot attributes by input position and touch configured endpoints after the batch succeeds. Earlier rows remain committed if a later row fails. The `Quietly` variants suppress model events. `force_create` and `force_create_quietly` bypass mass-assignment guards while still running casts.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::BelongsToMany}};

pub async fn create_roles<P: Model, R: Model>(
    roles: &BelongsToMany<P, R>,
    user: &P,
) -> DatabaseResult<Vec<R>> {
    roles.create_many(
        user,
        [Attributes::from([("name".into(), "Editor".into())])],
        &[Attributes::from([("source".into(), "manual".into())])],
    ).await
}
```

### Toggling associations {#toggling-associations}

Use `toggle` to remove existing links and insert missing links for the supplied keys. `toggle_with_pivot_attributes` supplies per-key attributes for inserts. Both `toggle` and `toggle_or_fail` run in one transaction and return attached and detached keys.

### Transactional pivot writes {#transactional-pivot-writes}

Use `attach_or_fail`, `detach_or_fail`, and `update_existing_pivot_or_fail` to run the write, custom pivot events, and configured touches in one transaction. `sync_or_fail`, `sync_without_detaching_or_fail`, and `sync_with_pivot_values_or_fail` share the transaction already used by the ordinary sync methods. The same methods work on polymorphic many-to-many relations.

If a model listener returns an error, the transaction rolls back its database writes and the other changes in that operation. A listener returning `Cancel` skips the affected row. When cancellation must abort the whole operation, return an error.

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
    relation.attach_or_fail(user, keys, Attributes::new()).await
}
```

### Creating or attaching a related model {#creating-or-attaching-related-models}

Use `create_or_first` on a many-to-many relation to insert a related model and attach it in one transaction. Pass the lookup attributes, additional creation values, and pivot attributes separately. On a unique conflict, the framework uses the write connection to find the model and attaches it if needed. A matching existing link keeps its pivot values. Back the lookup attributes with a unique constraint.

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
    relation.create_or_first(
        user,
        Attributes::from([("slug".into(), "editor".into())]),
        Attributes::from([("name".into(), "Editor".into())]),
        Attributes::new(),
    ).await
}
```

`first_or_create` first searches the current relation, then looks for an existing model to attach, and creates a model if neither lookup matches. `update_or_create` follows the same lookup order and applies the supplied values to an existing related model. Both apply pivot attributes only when adding a link.

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
    relation.update_or_create(
        user,
        Attributes::from([("slug".into(), "editor".into())]),
        Attributes::from([("name".into(), "Editor".into())]),
        Attributes::new(),
    ).await
}
```

Direct, polymorphic, and through relations also provide `first_or_new`, `first_or_create`, `create_or_first`, and `update_or_create`. `first_or_new` returns an unsaved model on a miss. Direct and polymorphic relations accept deferred values and populate their own relation keys. Through relations take attribute maps and use a supplied foreign key when creating a related row. `restore_or_create` and `create_or_restore` include soft-deleted related rows and restore them through the model lifecycle. Deleted intermediate rows remain subject to `with_trashed_parents`.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, relations::HasMany}};

pub async fn ensure_post<P: Model, R: Model>(
    posts: &HasMany<P, R>,
    author: &P,
) -> DatabaseResult<R> {
    posts.first_or_create(
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

Use `restore_or_create` to look up a related model, including soft-deleted rows, before inserting. `create_or_restore` attempts insertion first and recovers a matching row on a unique conflict. Both restore the model through its normal lifecycle. Pass `RelatedValues::deferred` to compute model values only when an insert or update needs them.

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
    relation.restore_or_create(
        user,
        Attributes::from([("slug".into(), "editor".into())]),
        RelatedValues::deferred(|| Attributes::from([("name".into(), "Editor".into())])),
        Attributes::new(),
    ).await
}
```

### Creating or incrementing a related counter {#incrementing-related-counters}

Use `increment_or_create` to attach a matching model or create it with an initial counter. For an existing model, the database adds the step atomically and writes any extra model attributes in the same update. The counter update fires `Updating` and `Updated`. Give the lookup attributes a unique constraint. The initial counter and step are explicit SQL numbers in Rust.

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
    tags.increment_or_create(
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
    views.increment_or_create(
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

Use `find_or_new` to look up a related primary key under the relationship’s constraints. If the key is absent, the method returns an unsaved model without assigning the requested primary key. Direct and polymorphic child relations apply the parent key, morph discriminator and configured creation defaults. Through and many-to-many relations return an unattached model on the relevant connection. A matching many-to-many model includes its Pivot.

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
    children.find_or_new(parent, id).await
}
```

Use `find`, `find_or_fail` or `find_sole` to read a related primary key. Use `sole` to require exactly one result for the relationship. Both unique-result methods read at most two candidates and distinguish a missing model from multiple matches. `find_or` and `first_or` run an async fallback only if the query succeeds without a match. The fallback returns the same model type.

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
    children.find_or(parent, id, || async {
        children.make(parent, Attributes::new()).await
    }).await
}
```

Use `find_many(parent, keys)` to find a batch of related primary keys. It returns the matching models in relationship query order, with any custom collection keys and loaded Pivot data. Repeated input keys do not repeat a matching row. Missing, soft-deleted, or out-of-scope records are omitted. An empty input returns the model’s empty collection without a query.

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
    children.find_many(parent, keys).await
}
```

Use `find_many_or_fail(parent, keys)` when every requested primary key is required. Use `find_many_or(parent, keys, fallback)` to supply an async fallback returning a collection when any key is missing. OpenRust checks coverage by primary key, so repeated Pivot rows cannot hide a missing key. Query errors propagate to the caller.

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
    children.find_many_or_fail(parent, keys).await
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

Call `chunk_by_id(parent, count, callback, column, alias)` or `chunk_by_id_desc` on direct, through, inverse, polymorphic and many-to-many relationships. The seek query keeps all relation constraints, including an existing one-of-many selection, and uses the related key by default. A through relationship uses the related primary key’s name as the default projection alias. Many-to-many callbacks receive models with a hydrated Pivot. A `MorphTo` callback receives the same keyed collection of `MorphTarget` values as `chunk`.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::MorphTo}};

async fn process_subject_by_id<P: Model>(parent: &P, subject: &MorphTo<P>) -> DatabaseResult<bool> {
    subject.chunk_by_id_desc(parent, 100, |targets, _page| async move {
        for (_, target) in targets {
            println!("{}", target.alias());
        }
        Ok(true)
    }, None, None).await
}
```

Relationships expose `each(parent, callback, count)` and `each_by_id(parent, callback, count, column, alias)`. The callback sees the same model, loaded relations, Pivot and collection key as the [Eloquent batch](getting-started.md#chunking-results). A `MorphTo` callback receives a `MorphTarget`.

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::HasMany}};

async fn visit_children<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
) -> DatabaseResult<bool> {
    children.each_by_id(parent, |_model, index| async move {
        println!("{}", index);
        Ok(true)
    }, 200, None, None).await
}
```

Relations support `lazy(parent, count)`, `lazy_by_id(parent, count, column, alias)`, `lazy_by_id_desc` and `chunk_map(parent, callback, count)`. They retain relation filters and hydrated Pivot data. A `MorphTo` stream yields `MorphTarget` values.

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{DatabaseResult, eloquent::{Model, relations::HasMany}};

async fn visit_latest<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
) -> DatabaseResult<Option<R>> {
    let mut models = children.lazy_by_id_desc(parent, 200, None, None);
    models.try_next().await
}
```

## Touching parent timestamps {#touching-parent-timestamps}

To refresh one owner explicitly, call `BelongsTo::<Comment, Post>::new("post_id", "id").touch(&comment).await?`. It updates the owner timestamp through the relationship key without loading the owner. A missing foreign key or disabled timestamp touching skips the update. The update bypasses global scopes, including the soft-delete scope.

Declare a typed belongs-to relation in your model’s `touches()` method to update the owner’s timestamp when a child is inserted or saved with changes. OpenRust uses the persisted foreign key and the relationship’s scopes. A missing owner is skipped. The owner emits `Saved` and its own declared owners are touched recursively. Each owner is touched once per save. With `save_in_transaction`, owner queries, writes and events use that transaction, so declared owners must use the same connection.

In a `Model for Comment` implementation, declare the relationship to `Post`.

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::BelongsTo};

    Touches::default().belongs_to(BelongsTo::<Self, Post>::new("post_id", "id"))
}
```

OpenRust uses typed relation descriptors for timestamp propagation. OpenRust keeps them on both the owner lookup and the write, so a tenant scope can prevent an out-of-scope update.

For a polymorphic owner, use `Touches::morph_to` with the same descriptor used to load the relationship. The persisted type selects the registered model, and the persisted ID is matched against that model’s owner key. Null type or ID values are skipped. An unregistered alias returns an error, so use `save_in_transaction` if the child write and the touch must roll back together.

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::MorphTo};

    Touches::default().morph_to(
        MorphTo::<Self>::new("commentable_type", "commentable_id")
            .target::<Post>("post", "id"),
    )
}
```

Enable `with_touching_parent()` on a many-to-many relation to update the declaring model’s timestamp when its links change. The parent update uses the same transaction and starts from persisted attributes. Reload the parent to read the new timestamp.

Enable `with_touching_related()` to refresh all models still linked after an attach, pivot update, detach, sync, or toggle changes the relationship. The update shares the pivot transaction and respects pivot filters, morph type, and related model scopes. It updates timestamps in bulk. Call `touch(&parent)` to refresh linked models explicitly, even on a descriptor without automatic touching.

To refresh related models when the declaring model itself changes, add the relationship to `Model::touches()` with `belongs_to_many` or `morph_to_many`. A dirty save updates linked timestamps in bulk and follows each related model’s own touch declarations without firing its `Saved` event for the bulk update. During pivot writes, a matching declaration on the parent refreshes the related models, and a matching inverse declaration on the related model refreshes the parent. OpenRust matches model types, pivot table, key mappings, morph alias, and pivot filters. Use the same descriptor for the declaration and the write. For the inverse, swap the parent and related keys. `with_touching_related()` and `with_touching_parent()` also enable touching explicitly.

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::BelongsToMany};

    Touches::default().belongs_to_many(
        BelongsToMany::<Self, Role>::new("role_user", "user_id", "role_id", "id", "id"),
    )
}
```

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Model, relations::{BelongsToMany, ToggleChanges}}};

pub async fn toggle_roles<U: Model, R: Model>(user: &U, keys: &[Value]) -> DatabaseResult<ToggleChanges> {
    BelongsToMany::<U, R>::new("role_user", "user_id", "role_id", "id", "id")
        .with_touching_parent()
        .with_touching_related()
        .toggle_or_fail(user, keys)
        .await
}
```
