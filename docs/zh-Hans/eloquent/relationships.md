# Eloquent 关系

## 定义关系

用显式键声明 `HasOne`、`HasMany` 或 `BelongsTo`。例如 `HasMany::<Author, Post>::new("id", "author_id")` 将作者的主键与文章的外键关联，`BelongsTo::<Post, Author>::new("author_id", "id")` 描述反向关系。用 `getResults` 查询单个模型的关联结果。

`BelongsTo` 的所有者键可使用主键之外的列。所有者键应唯一，才能让两种加载方式解析到同一行。若有多行匹配，单模型懒加载或 `getResults` 取查询的首行，批量预加载则为每个所有者键保留末行。外键为零时正常查询；外键为 NULL 时直接返回已配置的默认模型，无需查询。对已声明枚举转换的列，使用 `setEnum` 可存储类型化枚举外键的底层值。

`BelongsToMany` 接收中间表、父模型与关联模型在中间表的外键，以及两个模型键。例如 `BelongsToMany::<Author, Post>::new("author_post", "author_id", "post_id", "id", "id")`。

### 在子模型上自动加载父模型 {#automatically-hydrating-parent-models-on-children}

在 `HasMany` 或 `HasOne` 上使用 `chaperone("author")`，把父模型缓存到每个子模型上。`MorphMany` 和 `MorphOne` 缓存带关系别名的 `MorphTarget`。Rust 显式指定反向关系名。子模型共享省略已加载关系的父模型快照，反向读取无需查询，关系图也不会形成引用环。`inverse` 是同一入口的别名，可用 `withoutChaperone` 或 `withoutInverse` 关闭。

```rust
use openrust::database::eloquent::{Model, relations::HasMany};

pub fn posts<P: Model, R: Model>() -> HasMany<P, R> {
    HasMany::new("id", "author_id").chaperone("author")
}
```

### 带作用域的关系 {#scoped-relationships}

在关系的 `constrain` 闭包中调用 `with_attributes`，为创建的模型提供默认属性。传入 `true` 同时添加等值查询条件，传入 `false` 仅提供默认属性。模型显式设置的属性优先。直接和多态关系仍会设置各自的父键和类型。

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

### 比较关联模型 {#comparing-related-models}

使用 `is` 和 `isNot` 判断候选模型是否匹配单值关系。传入声明端模型和 `Some(candidate)`，候选为空时传 `None`。`BelongsTo`、`MorphTo` 及普通 `HasOne`、`MorphOne` 在内存中比较关联键、表和连接。`MorphTo` 先解析存储的别名。`HasOne`、`MorphOne` 和 `HasOneThrough` 调用时使用 `.await?`。穿透关系和多选一使用 SQL 存在查询确认候选。

```rust
use openrust::database::eloquent::{Model, relations::BelongsTo};

pub fn is_author<P: Model, A: Model>(post: &P, author: &A) -> bool {
    BelongsTo::new("author_id", "id").is(post, Some(author))
}
```

### 默认关联模型

可在 `BelongsTo`、`HasOne`、`HasOneThrough` 或 `MorphOne` 上调用 `with_default`，在缺少关联记录时提供一个未保存的模型。闭包接收新模型和父模型。用 `|_, _| {}` 可保留框架初始化的属性。`HasOne` 会预填外键，`MorphOne` 还会预填类型别名。可从已加载关系读取默认实例。关系存在和计数查询仍以数据库记录为准。

```rust
use openrust::database::eloquent::{Model, relations::BelongsTo};

pub fn author_with_default() -> BelongsTo<Post, Author> {
    BelongsTo::new("author_id", "id").with_default(|author: &mut Author, _| {
        author.setAttribute("name", "Guest author");
    })
}
```

对 `MorphTo` 关系，为需要兜底的别名注册类型化 `with_default`。闭包接收未保存的目标和父模型。外键为空或找不到匹配行时，已存储的别名选择自己的默认模型。用 `defaultForMissingType` 为类型为空的记录显式选择已登记的默认模型。外键存在但别名未知时仍返回错误。

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::MorphTo}};

pub fn subject_with_default<P: Model, T: Model>() -> DatabaseResult<MorphTo<P>> {
    MorphTo::new("subject_type", "subject_id")
        .target::<T>("post", "id")
        .with_default::<T>("post", |post, _| {
            post.setAttribute("title", "Unavailable");
        })?
        .defaultForMissingType("post")
}
```

### 从一对多中择一

用 `one` 转换 `HasMany` 或 `MorphMany`，再使用 `latestOfMany`、`oldestOfMany` 或 `ofMany`。未指定列时，最新和最早方法使用关联模型主键。按其他列排序且值相同时，默认选择主键最大的记录，也可指定其他规则。`whereCandidates` 在排序前筛选候选记录。SQL 最小值和最大值跳过空值，排序列全为空时该父模型没有结果。

要按多个列依次选择，可以传入切片，例如 `oldestOfMany(&["score", "id"])`。从左到右比较，每个显式列都使用该快捷方法对应的聚合。列表中省略主键时，最后用最大主键决胜。显式传入空切片也选择最大主键。传入 `None` 时，直接按该快捷方法的方向选择主键。

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Collection, Model, relations::HasMany},
};

pub async fn authors_with_top_post<Author: Model, Post: Model>(
    connection: &Connection,
) -> DatabaseResult<Collection<Author>> {
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    Author::queryOn(connection)
        .with("top_post", posts.one().latestOfMany(&["score", "id"]))
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

通过 `Model::through(local).has(distant)` 复用已有关系键。两段关系均传入类型化描述符。任一段为多值时返回 `HasManyThrough`，两段均为单值时返回 `HasOneThrough`。如果中间关系是多态关系，组合会保留其中间类型条件。附加查询条件可在返回的穿透关系上配置。

```rust
use openrust::database::eloquent::{Model, relations::{HasMany, HasManyThrough}};

pub fn comments<P: Model, I: Model, R: Model>(
    posts: HasMany<P, I>,
    comments: HasMany<I, R>,
) -> HasManyThrough<P, I, R> {
    P::through(posts).has(comments)
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

每个已登记别名可指定自己的所有者键。空字符串类型没有目标，可使用已配置的默认模型。文本别名 `"0"` 与其他已登记别名一样正常解析。所有者键存在重复值时，直接读取或单模型懒加载使用查询首行，预加载在各别名内按键保留末行。

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
    Comment::queryOn(connection).with("subject", subjects).orderBy("id", "asc").get().await
}
```

`MorphToMany` 在中间表读写时使用类型字段。反向查询时交换中间表的两个键参数，继续使用同一类型别名。可通过 `MorphPivot` 读取中间表字段。

### 自定义多态类型 {#custom-polymorphic-types}

使用 `MorphMap::morphMap` 注册应用别名，或使用 `enforceMorphMap` 要求模型解析别名前先注册。`getMorphClass` 返回模型的首个别名。未强制映射时，未注册模型使用 Rust 类型名。`targetMapped` 为已编译的目标模型接入全部已注册别名，包括合并映射时保留的旧别名。`MorphOne::mapped`、`MorphMany::mapped` 和 `MorphToMany::mapped` 在读写中使用模型所属应用的别名。仍可使用显式别名描述符。

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

### 设置多态关联 {#associating-polymorphic-models}

使用 `named("subject")` 为 `MorphTo` 指定关系名，赋值后即可读取该关系。`associate` 设置别名和所有者键，并缓存传入模型的快照。`dissociate` 将两列置空，并缓存已加载的空关系。调用 `save().await?` 持久化父模型。其他已加载关系仍可读取。未命名描述符沿用清除关系缓存的行为。

关联赋值使用该别名登记的所有者键，零值也会写入。只选择部分所有者列时，关联赋值和批量匹配都需要保留该键。所有者键缺失、为 NULL、类型不受支持，或类型别名为空时，调用会在修改子模型和关系缓存前返回错误。

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

使用异步方法 `whereHasMorphAll`、`whereDoesntHaveMorphAll` 和 `hasMorphCountAll` 对应 通配查询。它们先从模型表读取不同的多态别名，当前查询的筛选条件和作用域不参与这次读取。需要在关系上注册存储的别名。查询缺失目标时，`whereDoesntHaveMorphAll` 也匹配类型为空的记录。

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

### 延后加载关系聚合 {#deferred-aggregate-loading}

对已加载模型调用 `loadCount`、`loadExists`、`loadSum`、`loadAvg`、`loadMin` 或 `loadMax`。用 `loadAggregates` 可在一次查询中读取多个值。聚合结果写入原始属性，其他未保存的修改仍保留为脏值。

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

### 默认关系计数

在模型中实现 `Model::withDefaultCounts`，让读取模型时默认带上关系计数。它对应 `$withCount` 属性。计数和模型共用一次 SELECT。显式 `select` 会替换默认投影。

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

在 `MorphTo` 上使用 `with_trashed()`、`without_trashed()` 或 `only_trashed()`。这些选项作用于声明软删除列的加载目标。混合目标中未声明软删除列的模型照常加载。框架先应用这些选项，再应用各目标约束。链式调用会叠加显式的未删除或已删除条件。后续调用 `with_trashed()` 会移除默认作用域并保留这些显式条件。

```rust
use openrust::database::eloquent::{Model, relations::MorphTo};

pub fn subject_including_deleted<M: Model, P: Model, A: Model>() -> MorphTo<M> {
    MorphTo::new("subject_type", "subject_id")
        .target::<P>("post", "id")
        .target::<A>("author", "id")
        .with_trashed()
}
```

### 加载多态目标分组

用 `loadMorph` 重载指定目标组的类型化关系，或用 `loadMorphPaths::<Post>("subject", &subjects, "post", &["comments.post"])` 重载已登记路径。父模型关系和 JSON 输出均可读取新结果，已持有的目标引用继续指向旧快照。模型和集合均支持这两个方法。用 `loadMorphCount`、`loadMorphAggregate` 或 `loadMorphAggregates` 将延后查询的聚合值写回已缓存的多态目标，随后可从父模型关系和 JSON 输出读取。每次调用选择一个别名，同一别名的多个聚合按主键批量合并查询，其他别名保留各自的值。单模型也支持 `loadMorphAggregate` 和 `loadMorphAggregates`。

单模型还提供 `loadMorphMax`、`loadMorphMin`、`loadMorphSum`、`loadMorphAvg`，依次传入 `(name, morph, alias, related_name, relation, column)`。四个方法复用 `loadMorphAggregate` 的类型化目标与聚合机制，保留关联模型全局作用域；无关联行返回 SQL NULL，空目标或其他目标分组保持原状。`"comments as total"` 可指定结果属性名。数值类型由驱动决定，查询失败时保留原缓存目标快照。

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

使用 `getRelation::<RelatedCollection<Post>>("posts")` 读取已加载文章。多值关系对每个父模型调用关联模型的 `newCollection`，用 `keys` 和 `getByKey` 访问集合键。非列表键在 JSON 中保留为对象键。集合持有 `Arc<T>` 快照，调用方可以保留已加载的模型。单条关系使用 `Option<Arc<T>>`。关系名不存在表示尚未加载。查询后用 `load` 加载，用 `loadMissing` 保留已加载的结果。

通过 `into_iter` 或 `toBase` 消费已启用自动预加载的集合后，移出的模型共享同批模型快照。首次访问缺失关系时批量查询。查询完成后，其他模型可通过 `relation_loaded`、`getRelation` 和 JSON 序列化直接看到各自的结果，无需另行加载。显式取消关系加载，或修改模型属性、连接后，下次访问会直接查询该模型。

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

在模型或集合上调用 `load_missing_paths(&["posts.comments"])`，可沿已登记路径补齐缺失关系。已加载结果会保留，包括空结果。缺失的后代按批加载，多态目标按类型分组，根模型序列化时可读到新增结果。需要给共享的相关模型补齐后代时，OpenRust 会在关系缓存中安装新快照，之前持有的 `Arc` 仍指向旧快照。

末级缺失关系需要筛选时，使用 `loadMissingPathWith`。闭包接收分组查询条件和末级关系的表限定名，可与该路径的列选择一起使用。已加载的结果会保留，包括空结果。多态路径的闭包需使用每个可达目标都具备的列。

```rust
use openrust::database::{DatabaseResult, eloquent::{Collection, Model}};

pub async fn fill_selected_comments<M: Model>(
    authors: &mut Collection<M>,
    comment_id: i64,
) -> DatabaseResult<()> {
    authors
        .loadMissingPathWith("posts.comments:id,subject_id,body", move |conditions, table| {
            conditions.r#where(&format!("{table}.id"), "=", comment_id)
        })
        .await
}
```

一次配置多条路径时，将多个 `MissingRelation` 条目传给 `loadMissingPathsWith`。各条目可设置各自的谓词，多条路径共用的父关系只批量加载一次。重复路径按 顺序将后声明、先声明的谓词依次应用于同一条件组。

```rust
use openrust::database::{DatabaseResult, eloquent::{Collection, MissingRelation, Model}};

pub async fn fill_post_relations<M: Model>(
    authors: &mut Collection<M>,
    comment_id: i64,
) -> DatabaseResult<()> {
    authors
        .loadMissingPathsWith([
            MissingRelation::new("posts.author"),
            MissingRelation::new("posts.comments:id,subject_id,body").constrain(
                move |conditions, table| conditions.r#where(&format!("{table}.id"), "=", comment_id),
            ),
        ])
        .await
}
```



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

### 预加载特定列 {#eager-loading-specific-columns}

在每条路径的末级关系名后加冒号，再列出逗号分隔的字段。若需选择中间关系的列，将中间关系单独列出。保留模型匹配和下一层加载所需的主键及外键。多态关系的各目标都需具有所选列。`load_missing_paths` 保留已加载的关系值。

同一次调用中，中间关系的显式列配置放在嵌套路径前后都有效。如果同一路径重复指定列，采用首次显式配置。`load_missing_paths` 会保留已有关系加载时使用的列。

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

pub async fn fill_missing_comments<A: Model>(
    authors: &mut Collection<A>,
) -> DatabaseResult<()> {
    authors
        .load_missing_paths(&["posts:id,author_id", "posts.comments:id,subject_id,body"])
        .await
}
```

### 默认预加载

在模型中实现 `Model::withDefaultRelations`，声明默认加载的类型化关系。它对应框架模型的 `$with` 属性。

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

直接、穿透和多对多关系预加载时，数据库按每个父模型分别应用限制和偏移量。加载结果保留 Pivot 属性，模型中不包含用于计算顺序的行号。

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

在 `BelongsToMany` 或 `MorphToMany` 上，用 `wherePivotWith` 和 `orWherePivotWith` 添加分组条件。闭包接收查询 `Conditions`，可交给可复用的作用域函数。组内未限定的列引用中间表，嵌套组也适用。

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
        .wherePivot("role", "=", "admin")
        .orWherePivotWith(active)
}
```


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

自定义 Pivot 新行先由模型的 `new_instance` 初始化默认属性，再写入关系键、Pivot 默认值及显式值，随后触发 `Saving` 和 `Creating`。更新时，原始属性取自数据库已有行，脏属性记录本次待写入的变更。

自定义 Pivot 水合时，已选择的数据库列覆盖模型默认值，包括 SQL NULL。未选的普通属性可保留模型默认值。关系标识取自数据库行，水合后的值用于原始属性快照、变更跟踪和 JSON 输出。

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

### 递归保存模型与关系 {#recursively-saving-models-and-relationships}

调用 `push` 保存模型及其缓存中已加载的关联模型。可用 `setRelation` 把编辑后的快照放入缓存。保存按关系缓存顺序执行，遇到模型取消保存或返回错误时停止，前序写入已经提交。`pushQuietly` 抑制整次遍历的模型事件。从根模型重新读取关系即可取得已保存的快照，先前持有的 `Arc` 仍指向原快照。

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

### 批量保存关联模型 {#saving-related-models-in-batches}

直接和多态的一对一、一对多关系支持 `saveMany`，按顺序保存可变切片中的模型。`createMany` 接收属性映射，按输入顺序返回新模型。带 `Quietly` 后缀的方法会抑制操作期间的模型事件。每个模型单独写入，后续写入报错时，先前已提交的写入仍保留。

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

可信属性需要绕过模型批量赋值限制时，使用 `forceCreate` 或 `forceCreateMany`。属性转换和修改器仍会运行，关系会填入自己的外键及多态类型。使用 `forceCreateQuietly` 或 `forceCreateManyQuietly` 可抑制模型事件。

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

多对多关联使用 `attach`、`detach`、`sync` 和 `syncWithoutDetaching`。同步在一个数据库事务内增删关联记录。可在中间表的两个外键上设置联合唯一键以防止重复。

用 `withPivot(["label"])` 读取中间表的附加列，再从关联模型的 `getRelation::<Pivot>("pivot")` 获取。用 `r#as("membership")` 修改访问名称。`attachWith` 建立关联时写入附加字段，`updateExistingPivot` 更新已有的中间表记录并返回受影响行数。

`syncWithPivotValues` 为所有键应用同一组属性。`syncWithPivotAttributes` 接收 `(键, 属性)` 对，为各键设置不同属性。两者均返回附加、解绑和更新的键。同步失败会回滚本次修改。

调用 `sync` 或 `toggle` 时，可用 `CastableEnum::intoValue()` 传入枚举的数据库键。关联模型为关联键声明整数、字符串或枚举转换时，同步按该存储类型匹配并报告变动。文本键会保留 UUID、前导零和超出有符号整数范围的数字。

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
    let keys = [selected.intoValue(), 6_i64.into()];
    roles.sync(user, &keys).await?;
    Ok(())
}
```

### 保存和创建多对多模型 {#saving-many-to-many-models}

用 `save` 保存并挂载关联模型，用 `create` 从属性创建并挂载。Pivot 属性单独传入。每个模型与其关联在同一事务写入。`saveMany` 和 `createMany` 按输入位置对应 Pivot 属性，在批量成功后更新已配置的关联时间戳。后续记录失败时保留先前已提交的记录。带 `Quietly` 后缀的方法抑制模型事件。`forceCreate` 和 `forceCreateQuietly` 绕过批量赋值保护，仍执行属性转换。

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

### 切换关联 {#toggling-associations}

用 `toggle` 对给定键解除已有关系、建立缺失关系。`toggleWithPivotAttributes` 可为每个新增关联传入属性。`toggle` 和 `toggleOrFail` 均在同一事务内执行，返回新增和解除的键。

### 事务化 Pivot 写入 {#transactional-pivot-writes}

使用 `attachOrFail`、`detachOrFail` 和 `updateExistingPivotOrFail`，在同一事务中执行写入、自定义 Pivot 事件和已配置的时间戳更新。`syncOrFail`、`syncWithoutDetachingOrFail` 和 `syncWithPivotValuesOrFail` 复用普通同步方法的事务。多态多对多关系也可以调用这些方法。

模型监听器返回错误时，事务会回滚此操作中的关系写入和监听器通过同一事务产生的写入。返回 `Cancel` 会跳过对应行。需要中止整项操作时，返回错误。

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

### 创建或挂载关联模型 {#creating-or-attaching-related-models}

在多对多关系上调用 `createOrFirst`，在同一事务内创建关联模型并写入中间表。分别传入匹配属性、创建时的附加属性和 Pivot 属性。发生唯一键冲突时，从写连接回查模型并补上关联。已存在的关联保留原 Pivot 属性。请为匹配属性建立唯一约束。

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

`firstOrCreate` 先查当前关系，再查可挂载的现有模型，两次均未命中时创建模型。`updateOrCreate` 沿用同一查找顺序，并将提供的值保存到现有模型。两个方法都只在新增关联时写入 Pivot 属性。

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

直接、多态和穿透关系也提供 `firstOrNew`、`firstOrCreate`、`createOrFirst` 和 `updateOrCreate`。`firstOrNew` 未命中时返回未保存的模型。直接和多态关系接受延迟值，并填入关系键。穿透关系接受属性映射，创建时使用调用方提供的外键。`restoreOrCreate` 和 `createOrRestore` 会查找软删除的关联模型，并通过模型生命周期恢复。软删除的中间模型仍由 `withTrashedParents` 控制。

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

### 批量写入关联模型 {#upserting-related-models}

直接和多态的一对一、一对多关系支持 `upsert`。每行都会填入父键，多态关系还会填入类型。整个批次由一条 SQL 写入，跳过单个模型的事件和转换。Eloquent 按模型配置补齐缺少的 ID 和时间戳。

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

若要隔离冲突，唯一索引应包含父键，多态关系还应包含类型。SQLite 使用传入的冲突列。MySQL 使用表的主键和唯一索引判定冲突。关系上的普通查询条件不会限制冲突范围。

穿透关系和多对多关系将 `upsert` 转发到关联模型的数据表。穿透关系的每行需提供指向中间模型的外键。普通和多态多对多关系若需写入中间表关联，可单独调用 `attach` 或 `sync`。

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

`BelongsTo` 将 `upsert` 转发到所属模型表。`MorphTo` 按声明模型上已注册的多态别名选择目标表。OpenRust 要求非空批次具有有效别名。每次调用都使用传入的冲突列，声明模型的关联保持原样。

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

### 恢复关联模型 {#restoring-related-models}

`restoreOrCreate` 先查找关联模型，查找范围包含软删除记录，未命中时才插入。`createOrRestore` 先尝试插入，遇到唯一键冲突时回查匹配记录。两者都通过模型生命周期恢复记录。用 `RelatedValues::deferred` 可将属性计算延迟到插入或更新时。

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

### 创建或递增关联模型的计数器 {#incrementing-related-counters}

使用 `incrementOrCreate` 挂载匹配的模型，或以初始计数创建模型。已有模型由数据库原子加上步长，并在同一次更新写入额外模型属性。递增触发 `Updating` 和 `Updated`。请为匹配属性建立唯一约束。Rust 接口显式接收 SQL 数值作为初始计数和步长。

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

`HasOne`、`HasMany`、`MorphOne`、`MorphMany`、`HasOneThrough` 和 `HasManyThrough` 也提供同名方法。直接关系在创建时填入父模型外键，多态关系还会填入类型。穿透关系通过连接查询模型。创建时需要传入指向现有中间模型的外键。

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

### 按主键查找或构造关联模型 {#finding-or-making-a-related-model}

用 `findOrNew` 在关系约束内按关联模型主键查找。找不到时返回未保存的模型，查找用的主键不会写入。直接及多态子关系补齐父键、多态标识和创建默认值。穿透和多对多关系返回绑定对应连接的未关联模型。多对多命中时保留 Pivot。

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

用 `find`、`findOrFail` 或 `findSole` 按关联主键查询。用 `sole` 要求关系恰好匹配一条记录。两种唯一查询最多读取两条候选记录，并区分模型缺失和结果重复。`findOr` 和 `firstOr` 仅在查询成功且没有匹配时运行异步回调，回调返回同一种模型。

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

用 `findMany(parent, keys)` 批量查找关联主键。结果沿用关系查询顺序，保留自定义集合键及已加载的 Pivot。输入中的重复键不会重复返回同一行。缺失、软删除和不在关系范围内的记录会被略过。空输入直接返回模型的空集合，无需查询。

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

要求每个请求主键都存在时，用 `findManyOrFail(parent, keys)`。如果有键缺失，可以用 `findManyOr(parent, keys, fallback)` 提供返回集合的异步回调。OpenRust 逐个核对主键，重复的 Pivot 行不会掩盖缺失的键。查询错误直接传播给调用方。

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

### 关系游标 {#relationship-cursors}

在直接、穿透或多对多关系上调用 `cursor(parent)` 可逐行读取关联模型。游标沿用父键、多态类型及中间表条件。多对多游标在返回每个模型前为它加载 Pivot。连接占用、提前结束和预加载遵循模型游标的规则。

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

反向多态关系通过 `MorphTo::cursor(parent)` 按父模型的多态类型选择已注册的模型，逐行返回 `MorphTarget`。可用 `alias()` 获取别名，或用 `downcast_ref::<M>()` 取得类型化模型。缺少键或类型时返回空流，有键但类型未注册时返回错误。游标只返回数据库记录。需要关系的默认模型时，使用关系加载接口。

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

### 分批读取结果 {#chunking-results}

在直接子关系、穿透关系或多对多关系上调用 `chunk(parent, count, callback)`。每批都遵守关系约束和配置的排序。多对多关系的模型在交给回调前已装配 Pivot。回调遵循 [Eloquent 分批规则](getting-started.md#chunking-results)。

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

反向关系可调用 `BelongsTo::chunk(child, count, callback)` 或 `MorphTo::chunk(parent, count, callback)`。回调收到已持久化的模型。`BelongsTo` 提供类型化的 Eloquent 集合。`MorphTo` 按多态类型选择已注册的目标，提供保留目标模型集合键的通用 `Collection<MorphTarget>`。可用 `alias()` 判断别名，或用 `downcast_ref::<M>()` 取得具体模型。缺少关联记录时没有批次。默认模型由关系加载接口提供。

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

直接、穿透、反向、多态及多对多关系均可调用 `chunkById(parent, count, callback, column, alias)` 或 `chunkByIdDesc`。按键分批保留关系约束和已有的单条选择，默认使用关联键。穿透关系默认使用关联模型的主键名作为投影别名。多对多回调收到带 Pivot 的模型。`MorphTo` 回调与 `chunk` 一样收到保留集合键的 `MorphTarget` 集合。

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::MorphTo}};

async fn process_subject_by_id<P: Model>(parent: &P, subject: &MorphTo<P>) -> DatabaseResult<bool> {
    subject.chunkByIdDesc(parent, 100, |targets, _page| async move {
        for (_, target) in targets {
            println!("{}", target.alias());
        }
        Ok(true)
    }, None, None).await
}
```

关系提供 `each(parent, callback, count)` 和 `eachById(parent, callback, count, column, alias)`。回调沿用 [Eloquent 分批](getting-started.md#chunking-results) 的模型、预加载、Pivot 和集合键。`MorphTo` 回调收到 `MorphTarget`。

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::HasMany}};

async fn visit_children<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
) -> DatabaseResult<bool> {
    children.eachById(parent, |_model, index| async move {
        println!("{}", index);
        Ok(true)
    }, 200, None, None).await
}
```

关系支持 `lazy(parent, count)`、`lazyById(parent, count, column, alias)`、`lazyByIdDesc` 和 `chunkMap(parent, callback, count)`。它们保留关系过滤条件和已水合的 Pivot。`MorphTo` 流返回 `MorphTarget`。

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{DatabaseResult, eloquent::{Model, relations::HasMany}};

async fn visit_latest<P: Model, R: Model>(
    parent: &P,
    children: &HasMany<P, R>,
) -> DatabaseResult<Option<R>> {
    let mut models = children.lazyByIdDesc(parent, 200, None, None);
    models.try_next().await
}
```

## 触碰父模型时间戳 {#touching-parent-timestamps}

在模型的 `touches()` 方法中声明所属关系。新增子模型或保存有变更的子模型时，框架使用已持久化的外键和关系查询作用域更新所属模型的时间戳。找不到所属模型时跳过。所属模型触发 `Saved`，并递归触碰其声明的所属模型。每次保存对同一所属模型只触碰一次。使用 `saveInTransaction` 时，所属模型的查询、写入和事件共享该事务，要求使用同一个连接。

在 `Model for Comment` 的实现中声明指向 `Post` 的关系。

```rust
fn touches() -> openrust::database::eloquent::Touches<Self> {
    use openrust::database::eloquent::{Touches, relations::BelongsTo};

    Touches::default().belongsTo(BelongsTo::<Self, Post>::new("post_id", "id"))
}
```

OpenRust 使用带类型的关系描述符传递时间戳。OpenRust 在查询和写入时都保留全局作用域，因此租户作用域可以阻止越界更新。

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
