# Eloquent 关系

## 定义关系

用显式键声明 `HasOne`、`HasMany` 或 `BelongsTo`。例如 `HasMany::<Author, Post>::new("id", "author_id")` 将作者的主键与文章的外键关联，`BelongsTo::<Post, Author>::new("author_id", "id")` 描述反向关系。用 `getResults` 查询单个模型的关联结果。

`BelongsToMany` 接收中间表、父模型与关联模型在中间表的外键，以及两个模型键。例如 `BelongsToMany::<Author, Post>::new("author_post", "author_id", "post_id", "id", "id")`。

### 设置所属模型 {#associating-an-owner}

通过 `.named("author")` 指定 `BelongsTo` 的缓存名称。`associate` 使用传入模型设置外键并缓存当前快照。`associateKey` 直接设置外键并使该缓存项失效，下次访问时重新查询。`dissociate` 及其别名 `disassociate` 将外键置空，并缓存已加载的空关系。其他已加载关系仍可读取。调用 `save().await?` 保存子模型。

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::BelongsTo}};

pub async fn set_author<P: Model, A: Model>(post: &mut P, author: &A) -> DatabaseResult<()> {
    let relation = BelongsTo::new("author_id", "id").named("author");
    relation.associate(post, author)?;
    post.save().await?;
    Ok(())
}
```

### 默认关联模型

可在 `BelongsTo`、`HasOne`、`HasOneThrough` 或 `MorphOne` 上调用 `withDefault`，在缺少关联记录时提供一个未保存的模型。闭包接收新模型和父模型。用 `|_, _| {}` 可保留框架初始化的属性。`HasOne` 会预填外键，`MorphOne` 还会预填类型别名。可从已加载关系读取默认实例。关系存在和计数查询仍以数据库记录为准。

```rust
use openrust::database::eloquent::{Model, relations::BelongsTo};

pub fn author_with_default() -> BelongsTo<Post, Author> {
    BelongsTo::new("author_id", "id").withDefault(|author: &mut Author, _| {
        author.setAttribute("name", "Guest author");
    })
}
```

对 `MorphTo` 关系，为需要兜底的别名注册类型化 `withDefault`。闭包接收未保存的目标和父模型。外键为空或找不到匹配行时，已存储的别名选择自己的默认模型。用 `defaultForMissingType` 为类型为空的记录显式选择已登记的默认模型。外键存在但别名未知时仍返回错误。

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

### 从一对多中择一

用 `one` 转换 `HasMany` 或 `MorphMany`，再使用 `latestOfMany`、`oldestOfMany` 或 `ofMany`。未指定列时，最新和最早方法使用关联模型主键。按其他列排序且值相同时，默认选择主键最大的记录，也可指定其他规则。`whereCandidates` 在排序前筛选候选记录。SQL 最小值和最大值跳过空值，排序列全为空时该父模型没有结果。

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

### 穿透一对一和一对多

用 `HasOneThrough` 或 `HasManyThrough` 经由中间模型查询。构造器依次接收中间模型的外键、最终模型的外键、父模型的本地键和中间模型的本地键。例如通过作者的文章查询评论。中间模型和最终模型使用同一个执行 JOIN 的数据库连接。

默认排除已软删除的中间记录。用 `withTrashedParents` 将它们纳入查询，最终模型自身的软删除作用域仍生效。

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

### 从穿透一对多中择一

用 `one` 将 `HasManyThrough` 转为穿透单值关系，再调用 `latestOfMany`、`oldestOfMany` 或 `ofMany`。每个父模型的所有合格中间记录共同参与排序。用 `whereCandidates` 在排序前筛选最终模型。中间模型和最终模型各自的软删除规则仍然生效。

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

### 多态关系

用 `MorphOne` 或 `MorphMany` 根据 ID 和类型别名查询子模型。用 `MorphTo` 解析反向关系，并显式登记每个别名对应的模型类型。预加载按类型分组查询。用 `getRelation::<Option<MorphTarget>>("subject")` 读取加载结果，再对目标调用 `downcast_ref::<Post>()`。

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

`MorphToMany` 在中间表读写时使用类型字段。反向查询时交换中间表的两个键参数，继续使用同一类型别名。可通过 `MorphPivot` 读取中间表字段。

### 设置多态关联 {#associating-polymorphic-models}

使用 `named("subject")` 为 `MorphTo` 指定关系名，赋值后即可读取该关系。`associate` 设置别名和所有者键，并缓存传入模型的快照。`dissociate` 将两列置空，并缓存已加载的空关系。调用 `save().await?` 持久化父模型。其他已加载关系仍可读取。未命名描述符沿用清除关系缓存的行为。

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

### 动态关系 {#dynamic-relationships}

在服务提供者的 `boot` 方法中，通过 `Model::resolveRelationUsing` 登记命名解析器。闭包接收当前模型，并返回显式指定键的关系描述符。注册表按应用隔离。同一模型类型和名称再次登记时，新解析器用于后续加载。

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

使用 `getDynamicRelationOrLoad` 读取并缓存结果，或将 `DynamicRelation` 传给 `with`、`load` 和 `loadMissing`。Rust 需要显式指定描述符类型。解析器可为每个模型实例选择不同查询约束，因此动态预加载可能逐个父模型查询。已水合模型按其数据库连接所属的应用解析。

注册的关系支持存在查询时，可将同一个描述符传给 `whereHas`、`whereDoesntHave`、`hasCount` 和 `withCount`。解析器收到的是查询连接上的未保存模型，因此查询条件需在没有具体父模型记录时也能构建。

```rust
async fn orders_with_customer(
    connection: &openrust::database::Connection,
) -> DatabaseResult<openrust::database::eloquent::Collection<Order>> {
    let customer = eager_customer();
    Order::queryOn(connection).whereHas(&customer)?.get().await
}
```

组合模型需要共享命名解析器时，可通过 `inheritRelationUsing` 显式登记回退。第一个闭包提供基础模型，第二个闭包将关系适配到新的父模型类型。`HasMany::forParent` 会保留键和查询约束。本模型的登记优先，回退在同一应用内解析，循环回退会返回错误。

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

## 查询关系是否存在

`has` 和 `whereHas` 保留存在匹配关联记录的父模型。`whereDoesntHave` 保留没有匹配记录的父模型。这些方法在同一数据库连接中使用相关子查询。`withWhereHas` 用同一个关系进行筛选和预加载。

`hasCount` 将关联记录数与指定数字比较。对 `MorphTo` 使用 `whereHasMorph`、`whereDoesntHaveMorph` 或 `hasMorphCount` 时，显式传入类型别名。`morphWithCount` 登记目标类型，并为目标模型自己的关系加载计数。

用 `whereHasPath("posts.comments")` 筛选至少有一篇文章含评论的作者。用 `whereDoesntHavePath` 筛选没有完整匹配链的作者，包含没有文章的作者。`hasPathCount("posts.comments", ">=", 2)` 要求至少有一篇文章包含两条评论。前面各段判断存在，计数只比较最后一段。在模型的 `namedRelations()` 中通过 `nested` 登记可查询的描述符。

### 按关联列筛选 {#filtering-by-related-columns}

向 `whereRelation` 或 `whereDoesntHaveRelation` 传入类型化关系、列、比较运算符和值。缺失变体匹配没有任何关联行满足条件的父模型。`withWhereRelation` 还会预加载符合条件的关联行。`orWhere…` 变体为父查询增加备选条件。对 `MorphTo` 关系，使用 `whereMorphRelation` 或 `whereMorphDoesntHaveRelation`，并传入目标表含所选列的已登记别名。

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

### 筛选命名关系路径 {#filtering-named-relationship-paths}

用 `whereRelationPath` 和 `whereDoesntHaveRelationPath` 对已登记路径的最后一段施加条件。OR 变体将完整路径条件与父查询组合。`whereRelationPathWith` 的闭包接收一组条件和末级表限定名，自关联时该限定名可能是别名。用 `withWhereRelationPath` 或 `withWhereRelationPathWith` 将同一条件用于末级预加载。前面的关系照常加载。路径经过多态关系时，适用的已登记目标须支持剩余路径和相关列。

```rust
use openrust::database::{DatabaseResult, eloquent::{Builder, Model}};

pub fn matching_posts<M: Model>(
    query: Builder<'static, M>,
) -> DatabaseResult<Builder<'static, M>> {
    query.withWhereRelationPath("posts.comments", "approved", "=", true)
}
```

使用异步方法 `whereRelationPathAll` 或 `whereDoesntHaveRelationPathAll`，先发现已存储的别名，再应用末级列条件。`withWhereRelationPathAll` 和 `withWhereRelationPathAllWith` 在筛选与预加载间共用这次发现的别名。发现的每种目标都必须支持剩余路径。发现过程直接读取模型表，独立于父查询的筛选和作用域。该查询构建器随后沿用这组别名。

```rust
use openrust::database::{DatabaseResult, eloquent::{Builder, Model}};

pub async fn approved_subject_comments<M: Model>(
    query: Builder<'static, M>,
) -> DatabaseResult<Builder<'static, M>> {
    query.withWhereRelationPathAll("subject.comments", "approved", "=", true).await
}
```

### 查询所有多态目标类型

使用异步方法 `whereHasMorphAll`、`whereDoesntHaveMorphAll` 和 `hasMorphCountAll` 对应 Laravel 的通配查询。它们先从模型表读取不同的多态别名，当前查询的筛选条件和作用域不参与这次读取。需要在关系上注册存储的别名。查询缺失目标时，`whereDoesntHaveMorphAll` 也匹配类型为空的记录。

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

### 按关联模型身份筛选

用 `whereBelongsTo` 传入类型化关系和所属模型切片，按配置的所属键比较外键。用 `whereAttachedTo` 查找通过多对多关系相连的记录。多态关系可用 `identity` 取得类型和所属键，再传给 `whereMorphedTo` 或 `whereNotMorphedTo`。仅按别名筛选时用 `whereMorphType`，传入 `None` 查找类型为空的记录。

用 `orWhereBelongsTo`、`orWhereAttachedTo`、`orWhereMorphedTo` 或 `orWhereNotMorphedTo` 将关系身份作为前述条件的备选条件。`whereAttachedTo` 和 `orWhereAttachedTo` 也接受 `MorphToMany` 描述符，并应用中间表的多态类型条件。全局作用域及软删除筛选继续生效。

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

## 聚合关联模型

用 `withCount`、`withExists`、`withSum`、`withAvg`、`withMin` 或 `withMax` 在父模型查询中选出关联属性。示例返回 `posts_count`、`posts_exists` 和 `posts_sum_score`。可用 `"posts as published_count"` 指定别名。先调用父模型的 `select`，再添加聚合。空集合的求和为 `Null`，存在结果为布尔值。

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

### 默认关系计数

在模型中实现 `Model::withDefaultCounts`，让读取模型时默认带上关系计数。它对应 Laravel 的 `$withCount` 属性。计数和模型共用一次 SELECT。显式 `select` 会替换默认投影。

```rust
fn withDefaultCounts(query: Builder<'static, Self>) -> DatabaseResult<Builder<'static, Self>> {
    query.withCount("posts", &HasMany::<Self, Post>::new("id", "author_id"))
}
```

### 按多态目标预加载 {#morph-target-eager-loading}

用 `morphWith` 为已注册的目标别名声明类型化预加载，也可用 `morphWithPaths` 指定已登记的路径。同一目标类型和所有者键上的预加载可与已有约束及 `morphWithCount` 组合。未知别名或类型不匹配时，`morphWith` 返回错误。

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

### 软删除的多态目标 {#soft-deleted-morph-targets}

在 `MorphTo` 上使用 `withTrashed()`、`withoutTrashed()` 或 `onlyTrashed()`。这些选项作用于声明软删除列的加载目标。混合目标中未声明软删除列的模型照常加载。框架先应用这些选项，再应用各目标约束。与 Laravel 一样，链式调用会叠加显式的未删除或已删除条件。后续调用 `withTrashed()` 会移除默认作用域并保留这些显式条件。

```rust
use openrust::database::eloquent::{Model, relations::MorphTo};

pub fn subject_including_deleted<M: Model, P: Model, A: Model>() -> MorphTo<M> {
    MorphTo::new("subject_type", "subject_id")
        .target::<P>("post", "id")
        .target::<A>("author", "id")
        .withTrashed()
}
```

### 加载多态目标分组

用 `loadMorph` 重载指定目标组的类型化关系，或用 `loadMorphPaths::<Post>("subject", &subjects, "post", &["comments.post"])` 重载已登记路径。父模型关系和 JSON 输出均可读取新结果，已持有的目标引用继续指向旧快照。模型和集合均支持这两个方法。用 `loadMorphCount`、`loadMorphAggregate` 或 `loadMorphAggregates` 将延后查询的聚合值写回已缓存的多态目标，随后可从父模型关系和 JSON 输出读取。每次调用选择一个别名，同一别名的多个聚合按主键批量合并查询，其他别名保留各自的值。单模型也支持 `loadMorphAggregate` 和 `loadMorphAggregates`。

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

## 预加载

向 `with` 传入关系名和描述符。父键不超过 500 个时，两次 SELECT 即可查出作者及文章。加载保持父模型顺序，没有文章的作者会得到空结果。示例中的 `Author` 和 `Post` 实现了 [Model](./getting-started.md)。

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

使用 `getRelation::<Vec<Arc<Post>>>("posts")` 读取已加载文章。单条关系使用 `Option<Arc<T>>`。关系名不存在表示尚未加载。多个父模型引用同一个关联模型时，通过 `Arc` 共享。使用模型或集合的 `load` 延后加载，使用集合的 `loadMissing` 保留已加载结果。

通过 `into_iter` 或 `toBase` 消费已启用自动预加载的集合后，移出的模型共享同批模型快照。首次访问缺失关系时批量查询。查询完成后，其他模型可通过 `relationLoaded`、`getRelation` 和 JSON 序列化直接看到各自的结果，无需另行加载。显式取消关系加载，或修改模型属性、连接后，下次访问会直接查询该模型。

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

### 按路径嵌套预加载 {#nested-eager-loading}

在每一级模型的 `namedRelations()` 中登记对应关系，再将 `posts.comments` 这样的路径传给 `withPaths`。同一次调用中，共用前缀的路径复用父级查询。已有模型或集合可用 `loadPaths`。未知或格式错误的路径返回错误。

在模型或集合上调用 `loadMissingPaths(&["posts.comments"])`，可沿已登记路径补齐缺失关系。已加载结果会保留，包括空结果。缺失的后代按批加载，多态目标按类型分组，根模型序列化时可读到新增结果。需要给共享的相关模型补齐后代时，OpenRust 会在关系缓存中安装新快照，之前持有的 `Arc` 仍指向旧快照。

用 `NamedRelations::morphTo` 登记后，`subject.details` 支持预加载、存在、缺失和末级计数。每个已登记目标都要声明 `details`，各目标可使用不同的关系类型。预加载按目标别名分批，缺失查询包含空值或找不到目标的记录。子关系名称不同时，可使用各目标的类型化配置。

使用异步的 `whereHasPathAll`、`whereDoesntHavePathAll` 或 `hasPathCountAll`，可在构建查询前发现每个多态路径段实际存储的别名。发现查询从模型表读取不同别名，独立于外层查询的筛选和作用域。只有发现的目标需要声明子路径，遇到已存储但未登记的别名会返回错误。

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

### 默认预加载

在模型中实现 `Model::withDefaultRelations`，声明默认加载的类型化关系。它对应 Laravel 模型的 `$with` 属性。

```rust
fn withDefaultRelations(relations: EagerLoads<Self>) -> EagerLoads<Self> {
    relations.with("posts", HasMany::<Self, Post>::new("id", "author_id"))
}
```

单次查询可用 `without` 移除指定关系，或用 `withOnly` 替换本次预加载列表。用 `with` 声明同名关系时，只覆盖当前查询中该关系的定义。

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

### 约束预加载

用 `constrain` 配置关联模型的查询，并在闭包中使用 `with` 接入嵌套预加载。示例先筛选作者，再加载匹配的文章及每篇文章的作者。关联查询中的 `limit` 和 `offset` 按父模型分别应用。用 `orderBy` 指定每组记录的顺序。

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

### 自动预加载

在集合上调用 `withRelationshipAutoloading` 后，通过异步 `getRelationOrLoad` 读取模型的类型化关系。首次访问尚未加载的关系时，框架对当前集合执行批量加载。后续访问复用已加载的值，包括空结果。可让模型的 `Model::automaticallyEagerLoadRelationships` 返回 `true` 来默认启用。

通过 `modelAt(index)` 从集合借用模型，再在模型上调用 `getRelationOrLoad`。索引越界返回 `None`。异步加载期间持有集合的可变借用，借用结束后可从同一集合的其他模型读取已加载的值。

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

### 批量加载关系链

从已启用自动预加载的集合移出的模型调用 `getRelationOrLoad` 时，返回的关联模型也能继续批量加载。预先加载的关系同样适用，多态目标按别名分组。需要在借用结束后保留关联模型时，可复制状态来构造独立模型。独立模型共享批次。新加载的后代关系也可通过已关联的 `Arc` 模型读取，序列化根模型时同样可见。直接修改属性只影响当前拥有的模型。

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

在从集合借用的模型上调用 `relatedModelAt` 可访问单个关联模型。启用自动预加载后，该模型缺失的关系会在同一父关系下的所有关联模型上批量加载，结果写回根集合，供后续读取和序列化。关闭自动预加载时，访问遵循所选模型的延迟加载规则。关系为空或位置越界时返回 `None`。

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

用 `relatedModels` 打开经单值或多值关系取得的模型批次。它复用已加载的值。在批次上调用 `getRelationOrLoad`，可跨批次读取下一层关系，也可以继续调用 `relatedModels` 深入下一层。批次读取的结果在该批次存续期间缓存。共享实例只出现一次，空关系不会加入模型。

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

借用的关联模型可以继续调用 `relatedModelAt` 或 `morphModel`，每一层都保留对根集合的访问。例如，从评论的多态目标取出文章，再取出文章的作者并读取其文章。首次访问缺失关系会在根集合范围内批量加载该层，结果供后续读取和序列化。

父关系为多态关系时，在借用的根模型上调用 `morphModel::<Post>("subject", &subjects, "post")`。该方法验证别名和目标类型，当前根模型没有该别名的目标时返回 `None`。首次访问缺失关系时，自动预加载仅批量处理所选别名的目标，结果写回根集合。

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

### 禁止延迟加载

覆盖 `Model::preventsLazyLoading`，可在读取已保存模型的关系时阻止隐式单条查询。已加载关系、显式 `load` 和集合自动批量加载仍可使用。默认策略允许未保存或刚创建的模型加载关系。

```rust
fn preventsLazyLoading(&self) -> bool {
    true
}
```

可覆盖 `handleLazyLoadingViolationUsing` 处理类型化的 `DatabaseError::LazyLoadingViolation`。记录违规后返回 `Ok(())`，即可继续执行查询。

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::{Model, relations::HasMany}};

pub async fn author_post_count(connection: &Connection, id: i64) -> DatabaseResult<usize> {
    let mut author = Author::queryOn(connection).findOrFail(id).await?;
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    author.load("posts", &posts).await?;
    Ok(author.getRelationOrLoad("posts", &posts).await?.len())
}
```

## 按中间表列过滤 {#filtering-queries-via-intermediate-table-columns}

用 `wherePivot` 比较列值，用 `wherePivotIn` 或 `wherePivotNotIn` 匹配列表，用 `wherePivotBetween` 或 `wherePivotNotBetween` 匹配含端点的区间，用 `wherePivotNull` 或 `wherePivotNotNull` 检查空值。对应的 `orWherePivot…` 方法在同一归属和多态范围内组合条件。过滤适用于关系加载、存在性查询，以及更新、解除关联和同步时对已有记录的选择。

用 `withPivotValue` 添加等值过滤，并在 `attach` 或 `sync` 创建关联时填入同一个值。显式写入属性优先。默认值必须非空，且不能覆盖关系键。

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::BelongsToMany}};

pub fn approved_roles<U: Model, R: Model>() -> DatabaseResult<BelongsToMany<U, R>> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .withPivot(["approved"])
        .withPivotValue("approved", true)
}
```

## 按中间表列排序 {#ordering-queries-via-intermediate-table-columns}

使用 `orderByPivot("position", "asc")` 或 `orderByPivotDesc("created_at")` 按中间表列排列关联模型。惰性加载和预加载都支持此排序，包括多态多对多关系。

使用 `withTimestamps()` 读取并维护中间表时间戳。列名沿用父模型，缺省为 `created_at` 和 `updated_at`。用 `withTimestampColumns(Some("linked_at"), None)?` 指定自定义创建列并省略更新列。插入会填充配置的时间戳列，显式值优先。更新已有关系时自动刷新更新时间，创建时间仅在调用方显式修改时改变。

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn latest_roles<U: Model, R: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .withTimestamps()
        .orderByPivotDesc("created_at")
}
```

## 定义自定义中间表模型 {#defining-custom-intermediate-table-models}

在多对多关系中调用 `using::<Membership>()`，用类型化 Eloquent 模型加载和写入中间行。模型可声明属性转换和批量赋值字段。重命名访问器后，用 `getRelation::<Membership>("membership")` 读取。关系提供表名、连接和原始关联键，多态关系还提供类型判别列。复合键表用 `incrementing(false)`，自增模型需要实际的主键列。

关系写入采用自定义模型的生命周期和属性转换。通过关系的 `withTimestamps` 或 `withTimestampColumns` 配置时间戳。需要软删除时，将中间行建模为普通 Eloquent 模型。

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn roles<U: Model, R: Model, Membership: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .using::<Membership>()
        .withPivot(["approved"])
        .r#as("membership")
}
```

### 自动水合中间表反向关系 {#automatically-hydrating-pivot-relationships}

在中间表模型的 `pivotInverseRelations()` 中声明反向关系名称。在 `using::<Membership>()` 之后调用 `chaperone()`，从中间表外键和模型类型推断名称。用 `chaperoneWith(Some("owner"), Some("item"))?` 显式指定名称，用 `withoutChaperone()` 关闭水合。

惰性加载和预加载均从当前已加载的模型填充反向关系。通过 `getRelation` 读取 `Option<Arc<T>>`。反向关系保存水合时的模型快照，使 Rust 能序列化该关系图并避免循环引用。

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn roles<U: Model, R: Model, Membership: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .using::<Membership>()
        .chaperone()
}
```

## 新建和更新关联模型

`HasMany::create` 和 `save` 在保存前填入关联模型外键。`BelongsTo::associate` 和 `dissociate` 修改子模型的内存状态。调用子模型的 `save` 持久化修改。

多对多关联使用 `attach`、`detach`、`sync` 和 `syncWithoutDetaching`。同步在一个数据库事务内增删关联记录。可在中间表的两个外键上设置联合唯一键以防止重复。

用 `withPivot(["label"])` 读取中间表的附加列，再从关联模型的 `getRelation::<Pivot>("pivot")` 获取。用 `r#as("membership")` 修改访问名称。`attachWith` 建立关联时写入附加字段，`updateExistingPivot` 更新已有的中间表记录并返回受影响行数。

`syncWithPivotValues` 为所有键应用同一组属性。`syncWithPivotAttributes` 接收 `(键, 属性)` 对，为各键设置不同属性。两者均返回附加、解绑和更新的键。同步失败会回滚本次修改。

### 切换关联 {#toggling-associations}

用 `toggle` 对给定键解除已有关系、建立缺失关系。`toggleWithPivotAttributes` 可为每个新增关联传入属性。`toggle` 和 `toggleOrFail` 均在同一事务内执行，返回新增和解除的键。

## 触碰父模型时间戳 {#touching-parent-timestamps}

在模型的 `touches()` 方法中声明所属关系。新增子模型或保存有变更的子模型时，框架使用已持久化的外键和关系查询作用域更新所属模型的时间戳。找不到所属模型时跳过。所属模型触发 `Saved`，并递归触碰其声明的所属模型。每次保存对同一所属模型只触碰一次。使用 `saveInTransaction` 时，所属模型的查询、写入和事件共享该事务，要求使用同一个连接。

在 `Model for Comment` 的实现中声明指向 `Post` 的关系。

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::BelongsTo};

    Touches::default().belongsTo(BelongsTo::<Self, Post>::new("post_id", "id"))
}
```

Laravel 在 `$touches` 中列出关系名称。OpenRust 使用带类型的关系描述符。 Laravel 写入时间戳时移除全局作用域。OpenRust 在查询和写入时都保留全局作用域，因此租户作用域可以阻止越界更新。

所属模型是多态关系时，通过 `Touches::morphTo` 传入加载关系时使用的描述符。框架根据已持久化的类型选择已注册模型，再用已持久化的 ID 匹配该模型的所属键。类型或 ID 为空时跳过。未注册的别名会返回错误，需要让子模型写入和时间戳更新一并回滚时，请使用 `saveInTransaction`。

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::MorphTo};

    Touches::default().morphTo(
        MorphTo::<Self>::new("commentable_type", "commentable_id")
            .target::<Post>("post", "id"),
    )
}
```

在多对多关系上使用 `withTouchingParent()`，关联发生变更时更新声明端模型的时间戳。更新从已持久化的属性开始，并与中间表写入共用事务。重新加载父模型后可读取新时间戳。

使用 `withTouchingRelated()` 后，附加、更新中间表、解除、同步或切换产生变更时，框架批量更新操作后仍有关联的模型。更新时间戳与中间表写入共用事务，并遵循中间表筛选、多态类型和相关模型的作用域。可通过 `touch(&parent)` 主动刷新相关模型，即使关系未启用自动触碰。

需要在声明端模型自身发生变更时刷新相关模型，可在 `Model::touches()` 中通过 `belongsToMany` 或 `morphToMany` 声明关系。有变更的保存会批量更新时间戳，并沿相关模型自己的声明递归传播。批量更新不会触发相关模型的 `Saved` 事件。中间表写入时，父模型上的匹配声明会刷新相关模型，相关模型上的匹配反向声明会刷新父模型。OpenRust 按模型类型、中间表、键映射、多态别名和中间表过滤识别关系。声明和写入请使用同一描述符，反向关系需交换两端的键。也可使用 `withTouchingRelated()` 和 `withTouchingParent()` 显式启用。

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
