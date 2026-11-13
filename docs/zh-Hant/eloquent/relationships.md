# Eloquent 關聯

## 定義關聯

用明確的鍵宣告 `HasOne`、`HasMany` 或 `BelongsTo`。例如 `HasMany::<Author, Post>::new("id", "author_id")` 將作者主鍵與文章外鍵關聯，`BelongsTo::<Post, Author>::new("author_id", "id")` 描述反向關聯。用 `getResults` 查詢單一模型的關聯結果。

`BelongsToMany` 接收中介資料表、父模型與關聯模型在中介表的外鍵，以及兩個模型鍵。例如 `BelongsToMany::<Author, Post>::new("author_post", "author_id", "post_id", "id", "id")`。

### 預設關聯模型

可在 `BelongsTo`、`HasOne`、`HasOneThrough` 或 `MorphOne` 上呼叫 `withDefault`，在缺少關聯記錄時提供未儲存的模型。閉包接收新模型和父模型。用 `|_, _| {}` 可保留框架初始化的屬性。`HasOne` 會預填外鍵，`MorphOne` 還會預填型別別名。可從已載入關聯讀取預設實例。關聯存在與計數查詢仍以資料庫記錄為準。

```rust
use openrust::database::eloquent::{Model, relations::BelongsTo};

pub fn author_with_default() -> BelongsTo<Post, Author> {
    BelongsTo::new("author_id", "id").withDefault(|author: &mut Author, _| {
        author.setAttribute("name", "Guest author");
    })
}
```

### 從一對多中選一

使用 `one` 轉換 `HasMany` 或 `MorphMany`，再使用 `latestOfMany`、`oldestOfMany` 或 `ofMany`。未指定欄位時，最新和最早方法使用關聯模型主鍵。依其他欄位排序且值相同時，預設選擇主鍵最大的記錄，也能指定其他規則。`whereCandidates` 在排序前篩選候選記錄。SQL 最小值和最大值會略過空值，排序欄位全為空值時該父模型沒有結果。

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

### 穿透一對一和一對多

用 `HasOneThrough` 或 `HasManyThrough` 經由中間模型查詢。建構函式依序接收中間模型的外鍵、最終模型的外鍵、父模型的本機鍵和中間模型的本機鍵。例如透過作者的文章查詢留言。中間模型和最終模型使用同一個執行 JOIN 的資料庫連線。

預設排除已軟刪除的中間記錄。用 `withTrashedParents` 將它們納入查詢，最終模型本身的軟刪除作用域仍然生效。

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

### 從穿透一對多中選一

用 `one` 將 `HasManyThrough` 轉為穿透單值關聯，再呼叫 `latestOfMany`、`oldestOfMany` 或 `ofMany`。每個父模型的所有合格中間記錄共同參與排序。用 `whereCandidates` 在排序前篩選最終模型。中間模型和最終模型各自的軟刪除規則仍然生效。

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

### 多型關聯

用 `MorphOne` 或 `MorphMany` 依 ID 和類型別名查詢子模型。用 `MorphTo` 解析反向關聯，並明確登錄每個別名對應的模型類型。預先載入依類型分組查詢。用 `getRelation::<Option<MorphTarget>>("subject")` 讀取載入結果，再對目標呼叫 `downcast_ref::<Post>()`。

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

`MorphToMany` 在中介資料表讀寫時使用類型欄位。反向查詢時交換中介資料表的兩個鍵參數，繼續使用同一類型別名。可透過 `MorphPivot` 讀取中介資料表欄位。

## 查詢關聯是否存在

`has` 和 `whereHas` 保留存在符合關聯記錄的父模型。`whereDoesntHave` 保留沒有符合記錄的父模型。這些方法在同一資料庫連線使用相關子查詢。`withWhereHas` 使用同一個關聯進行篩選及預先載入。

`hasCount` 將關聯記錄數與指定數字比較。對 `MorphTo` 使用 `whereHasMorph`、`whereDoesntHaveMorph` 或 `hasMorphCount` 時，明確傳入類型別名。`morphWithCount` 登錄目標類型，並為目標模型本身的關聯載入計數。

### 查詢所有多型目標型別

使用非同步方法 `whereHasMorphAll`、`whereDoesntHaveMorphAll` 和 `hasMorphCountAll` 對應 Laravel 的萬用字元查詢。這些方法先從模型表讀取不同的多型別名，目前查詢的篩選條件和作用域不參與這次讀取。請在關聯上註冊已儲存的別名。查詢缺少目標的記錄時，`whereDoesntHaveMorphAll` 也會比對型別為空的記錄。

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

### 按關聯模型身分篩選

用 `whereBelongsTo` 傳入型別化關聯和所屬模型切片，依設定的所屬鍵比較外鍵。用 `whereAttachedTo` 查找透過多對多關聯連接的記錄。多型關聯可用 `identity` 取得型別和所屬鍵，再傳給 `whereMorphedTo` 或 `whereNotMorphedTo`。僅按別名篩選時用 `whereMorphType`，傳入 `None` 查找型別為空的記錄。

用 `orWhereBelongsTo`、`orWhereAttachedTo`、`orWhereMorphedTo` 或 `orWhereNotMorphedTo` 將關聯身分作為前述條件的替代條件。`whereAttachedTo` 和 `orWhereAttachedTo` 也接受 `MorphToMany` 描述符，並套用中介資料表的多型型別條件。全域作用域與軟刪除篩選繼續生效。

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

## 聚合關聯模型

用 `withCount`、`withExists`、`withSum`、`withAvg`、`withMin` 或 `withMax` 在父模型查詢中選出關聯屬性。範例回傳 `posts_count`、`posts_exists` 和 `posts_sum_score`。可用 `"posts as published_count"` 指定別名。先呼叫父模型的 `select`，再新增聚合。空集合的總和為 `Null`，存在結果為布林值。

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

### 預設關聯計數

在模型中實作 `Model::withDefaultCounts`，讓讀取模型時預設帶上關聯計數。它對應 Laravel 的 `$withCount` 屬性。計數和模型共用一次 SELECT。明確呼叫 `select` 會取代預設投影。

```rust
fn withDefaultCounts(query: Builder<'static, Self>) -> DatabaseResult<Builder<'static, Self>> {
    query.withCount("posts", &HasMany::<Self, Post>::new("id", "author_id"))
}
```

### 載入多型目標分組

取得父模型後，可用 `loadMorph` 或 `loadMorphCount` 載入指定目標別名的型別化關聯。回傳的批次可讀取載入的關聯或計數。也可用 `morphModels` 開啟指定分組並繼續巡覽關聯鏈。每次呼叫選擇一個別名，主鍵相同的不同目標型別仍可使用各自的關聯。新增關聯和計數屬性隨回傳的批次保存。

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

## 預先載入

將關聯名稱與描述符傳給 `with`。父鍵不超過 500 個時，兩次 SELECT 即可查出作者及文章。載入會保留父模型順序，沒有文章的作者會得到空結果。範例中的 `Author` 和 `Post` 實作了 [Model](./getting-started.md)。

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

使用 `getRelation::<Vec<Arc<Post>>>("posts")` 讀取已載入文章。單筆關聯使用 `Option<Arc<T>>`。關聯名稱不存在表示尚未載入。多個父模型參照同一個關聯模型時，透過 `Arc` 共用。用模型或集合的 `load` 延後載入，使用集合的 `loadMissing` 保留已載入結果。

### 預設預先載入

在模型中實作 `Model::withDefaultRelations`，宣告預設載入的型別化關聯。它對應 Laravel 模型的 `$with` 屬性。

```rust
fn withDefaultRelations(relations: EagerLoads<Self>) -> EagerLoads<Self> {
    relations.with("posts", HasMany::<Self, Post>::new("id", "author_id"))
}
```

單次查詢可用 `without` 移除指定關聯，或用 `withOnly` 取代本次預先載入清單。用 `with` 宣告同名關聯時，只覆寫目前查詢中該關聯的定義。

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

### 約束預先載入

用 `constrain` 設定關聯模型的查詢，並在閉包中使用 `with` 預先載入巢狀關聯。範例先篩選作者，再載入符合的文章及每篇文章的作者。關聯查詢中的 `limit` 和 `offset` 依父模型分別套用。用 `orderBy` 指定每組記錄的順序。

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

### 自動預先載入

在集合上呼叫 `withRelationshipAutoloading` 後，透過非同步 `getRelationOrLoad` 讀取模型的型別化關聯。首次存取尚未載入的關聯時，框架對目前集合執行批次載入。後續存取會沿用已載入的值，包含空結果。可讓模型的 `Model::automaticallyEagerLoadRelationships` 回傳 `true` 來預設啟用。

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

### 批次載入關聯鏈

用 `relatedModels` 開啟經單值或多值關聯取得的模型批次。它沿用已載入的值。在批次上呼叫 `getRelationOrLoad`，可跨批次讀取下一層關聯，也可以繼續呼叫 `relatedModels` 深入下一層。批次讀取的結果在該批次存續期間快取。共享實例只出現一次，空關聯不會加入模型。

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

### 禁止延遲載入

覆寫 `Model::preventsLazyLoading`，可在讀取已儲存模型的關聯時阻止隱式單筆查詢。已載入關聯、明確呼叫 `load` 和集合自動批次載入仍可使用。預設策略允許未儲存或剛建立的模型載入關聯。

```rust
fn preventsLazyLoading(&self) -> bool {
    true
}
```

可覆寫 `handleLazyLoadingViolationUsing` 處理型別化的 `DatabaseError::LazyLoadingViolation`。記錄違規後回傳 `Ok(())`，即可繼續執行查詢。

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::{Model, relations::HasMany}};

pub async fn author_post_count(connection: &Connection, id: i64) -> DatabaseResult<usize> {
    let mut author = Author::queryOn(connection).findOrFail(id).await?;
    let posts = HasMany::<Author, Post>::new("id", "author_id");
    author.load("posts", &posts).await?;
    Ok(author.getRelationOrLoad("posts", &posts).await?.len())
}
```

## 依中介資料表欄位篩選 {#filtering-queries-via-intermediate-table-columns}

用 `wherePivot` 比較欄位值，以 `wherePivotIn` 或 `wherePivotNotIn` 比對清單，以 `wherePivotBetween` 或 `wherePivotNotBetween` 比對包含端點的區間，以 `wherePivotNull` 或 `wherePivotNotNull` 檢查空值。對應的 `orWherePivot…` 方法在同一歸屬與多型範圍內組合條件。篩選適用於關聯載入、存在性查詢，以及更新、解除關聯與同步時對既有記錄的選取。

用 `withPivotValue` 加入等值篩選，並於 `attach` 或 `sync` 建立關聯時填入同一個值。明確寫入的屬性優先。預設值必須非空，且不能覆寫關聯鍵。

```rust
use openrust::database::{DatabaseResult, eloquent::{Model, relations::BelongsToMany}};

pub fn approved_roles<U: Model, R: Model>() -> DatabaseResult<BelongsToMany<U, R>> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .withPivot(["approved"])
        .withPivotValue("approved", true)
}
```

## 依中介資料表欄位排序 {#ordering-queries-via-intermediate-table-columns}

使用 `orderByPivot("position", "asc")` 或 `orderByPivotDesc("created_at")` 依中介資料表欄位排列關聯模型。延遲載入和預先載入皆支援此排序，包括多型多對多關聯。

使用 `withTimestamps()` 讀取並維護中介資料表時間戳記。欄位名稱沿用父模型，預設為 `created_at` 和 `updated_at`。使用 `withTimestampColumns(Some("linked_at"), None)?` 指定自訂建立欄位並省略更新欄位。插入會填入設定的時間戳記欄位，明確指定的值優先。更新既有關聯時會刷新更新時間，建立時間只在呼叫方明確修改時改變。

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn latest_roles<U: Model, R: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .withTimestamps()
        .orderByPivotDesc("created_at")
}
```

## 定義自訂中介資料表模型 {#defining-custom-intermediate-table-models}

在多對多關聯呼叫 `using::<Membership>()`，以具型別的 Eloquent 模型載入及寫入中介資料列。模型可宣告屬性轉換與批次賦值欄位。重新命名存取器後，以 `getRelation::<Membership>("membership")` 讀取。關聯提供資料表、連線與原始關聯鍵，多型關聯也提供型別判別欄位。複合鍵資料表使用 `incrementing(false)`，自動遞增模型需要實際的主鍵欄位。

關聯寫入採用自訂模型的生命週期與屬性轉換。透過關聯的 `withTimestamps` 或 `withTimestampColumns` 設定時間戳記。需要軟刪除時，將中介資料列定義為一般 Eloquent 模型。

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn roles<U: Model, R: Model, Membership: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .using::<Membership>()
        .withPivot(["approved"])
        .r#as("membership")
}
```

### 自動水合中介資料表反向關聯 {#automatically-hydrating-pivot-relationships}

在中介模型的 `pivotInverseRelations()` 中宣告反向關聯名稱。在 `using::<Membership>()` 之後呼叫 `chaperone()`，從中介資料表外鍵與模型型別推斷名稱。以 `chaperoneWith(Some("owner"), Some("item"))?` 明確指定名稱，並以 `withoutChaperone()` 關閉水合。

延遲載入和預先載入皆由目前已載入的模型填入反向關聯。透過 `getRelation` 讀取 `Option<Arc<T>>`。反向關聯保存水合時的模型快照，使 Rust 能序列化關聯圖並避免循環參照。

```rust
use openrust::database::eloquent::{Model, relations::BelongsToMany};

pub fn roles<U: Model, R: Model, Membership: Model>() -> BelongsToMany<U, R> {
    BelongsToMany::new("role_user", "user_id", "role_id", "id", "id")
        .using::<Membership>()
        .chaperone()
}
```

## 建立和更新關聯模型

`HasMany::create` 和 `save` 在儲存前填入關聯模型外鍵。`BelongsTo::associate` 和 `dissociate` 修改子模型的記憶體狀態。呼叫子模型的 `save` 持久化修改。

多對多關聯使用 `attach`、`detach`、`sync` 和 `syncWithoutDetaching`。同步在一個資料庫交易內新增及刪除關聯記錄。可在中介表的兩個外鍵上設定複合唯一鍵以避免重複。

用 `withPivot(["label"])` 讀取中介資料表的附加欄位，再從關聯模型的 `getRelation::<Pivot>("pivot")` 取得。用 `r#as("membership")` 變更存取名稱。`attachWith` 建立關聯時寫入附加欄位，`updateExistingPivot` 更新既有中介記錄並回傳受影響列數。

`syncWithPivotValues` 為所有鍵套用同一組屬性。`syncWithPivotAttributes` 接收 `(鍵, 屬性)` 配對，為各鍵設定不同屬性。兩者均回傳附加、解除綁定和更新的鍵。同步失敗會復原本次修改。
