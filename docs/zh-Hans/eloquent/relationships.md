# Eloquent 关系

## 定义关系

用显式键声明 `HasOne`、`HasMany` 或 `BelongsTo`。例如 `HasMany::<Author, Post>::new("id", "author_id")` 将作者的主键与文章的外键关联，`BelongsTo::<Post, Author>::new("author_id", "id")` 描述反向关系。用 `getResults` 查询单个模型的关联结果。

`BelongsToMany` 接收中间表、父模型与关联模型在中间表的外键，以及两个模型键。例如 `BelongsToMany::<Author, Post>::new("author_post", "author_id", "post_id", "id", "id")`。

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

## 查询关系是否存在

`has` 和 `whereHas` 保留存在匹配关联记录的父模型。`whereDoesntHave` 保留没有匹配记录的父模型。这些方法在同一数据库连接中使用相关子查询。`withWhereHas` 用同一个关系进行筛选和预加载。

`hasCount` 将关联记录数与指定数字比较。对 `MorphTo` 使用 `whereHasMorph`、`whereDoesntHaveMorph` 或 `hasMorphCount` 时，显式传入类型别名。`morphWithCount` 登记目标类型，并为目标模型自己的关系加载计数。

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

### 加载多态目标分组

取得父模型后，可用 `loadMorph` 或 `loadMorphCount` 加载指定目标别名的类型化关系。返回的批次可读取加载的关系或计数。也可用 `morphModels` 打开指定分组并继续遍历关系链。每次调用选择一个别名，主键相同的不同目标类型仍可使用各自的关系。新增关系和计数属性随返回的批次保存。

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

### 批量加载关系链

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

## 新建和更新关联模型

`HasMany::create` 和 `save` 在保存前填入关联模型外键。`BelongsTo::associate` 和 `dissociate` 修改子模型的内存状态。调用子模型的 `save` 持久化修改。

多对多关联使用 `attach`、`detach`、`sync` 和 `syncWithoutDetaching`。同步在一个数据库事务内增删关联记录。可在中间表的两个外键上设置联合唯一键以防止重复。

用 `withPivot(["label"])` 读取中间表的附加列，再从关联模型的 `getRelation::<Pivot>("pivot")` 获取。用 `r#as("membership")` 修改访问名称。`attachWith` 建立关联时写入附加字段，`updateExistingPivot` 更新已有的中间表记录并返回受影响行数。

`syncWithPivotValues` 为所有键应用同一组属性。`syncWithPivotAttributes` 接收 `(键, 属性)` 对，为各键设置不同属性。两者均返回附加、解绑和更新的键。同步失败会回滚本次修改。
