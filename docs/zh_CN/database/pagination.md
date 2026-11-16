# 数据库分页

## 基本用法

`paginate(15)` 先查询总数，再读取当前页。只需要上一页和下一页时，使用 `simplePaginate(15)`，它多读一行来判断是否还有下一页。Query Builder 和 Eloquent 均支持这两种方法。Eloquent 默认按主键排序。Query Builder 可显式指定排序以保证跨页顺序稳定。

使用 `paginateWith` 或 `simplePaginateWith` 并传入 `PageOptions`，可以自定义页码参数名、查询列和页码。HTTP 从当前请求的查询字符串读取页码。HTTP 之外默认使用第 1 页。

## 游标分页

`cursorPaginate(15)` 比较排序列的值，并多读一行判断后续记录。下次请求传入返回的 `next_cursor` 或 `prev_cursor`。Query Builder 需按已查询、非空的标量列排序，最后使用唯一列打破并列。Eloquent 在未声明排序时使用主键。分组查询、联合查询和原始排序表达式使用偏移分页。

## 手工创建分页器 {#manually-creating-a-paginator}

数据已在内存时，将当前页切片和切片前的总数传给 `LengthAwarePaginator::new`。使用 `Paginator::new` 时，有下一页就额外传入一条记录。`CursorPaginator::new` 同样接收带预读记录的有序切片，还需要一个回调提取边界记录的命名排序键。请求上一页时，按反向查询顺序传入记录，分页器会恢复显示顺序。

```rust
use openrust::{
    database::Value,
    pagination::{Cursor, CursorPaginator, PaginationResult, PaginatorOptions},
};

fn manual_cursor_page(
    ordered: Vec<i64>,
    current: Option<&Cursor>,
) -> PaginationResult<CursorPaginator<String>> {
    let mut page = CursorPaginator::new(
        ordered,
        2,
        current,
        PaginatorOptions {
            cursorName: Some("position"),
            ..Default::default()
        },
        |id| [("id", Value::Integer(*id))],
    )?;
    page.withQueryString();
    Ok(page.through(|id| id.to_string()))
}
```

## 分页器实例方法 {#paginator-instance-methods}

用 `count()`、`onFirstPage()` 和 `onLastPage()` 获取页面状态。`through` 消耗原分页器并转换记录类型，同时保留分页元数据。游标仍使用转换前的排序键。

用 `withPath` 修改链接路径，`setPageName` 或 `setCursorName` 修改查询参数名，`withQueryString` 保留请求筛选条件，`fragment` 添加锚点。`getOptions()` 返回当前生效的链接选项。

## 关联模型分页 {#paginating-related-models}

在 `HasMany` 或 `MorphMany` 描述符上调用 `paginate`、`simplePaginate` 或 `cursorPaginate`，传入父模型和每页数量。带 `With` 的变体接受与 Eloquent 查询相同的选项。查询保留父键、适用的多态类型、已配置的筛选和模型作用域。父模型没有键时返回空页。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasMany}},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn related_comments<P: Model, R: Model>(
    article: &P,
    comments: &HasMany<P, R>,
) -> DatabaseResult<LengthAwarePaginator<R>> {
    comments.paginateWith(article, PageOptions {
        perPage: Some(15),
        pageName: "comments",
        ..Default::default()
    }).await
}
```

`HasOne`、`MorphOne`、`BelongsTo` 和 `MorphTo` 同样提供三种分页。单条关系保留已有的择一条件。分页返回已持久化的记录，缺少反向引用时返回保留请求分页元数据的空页。`MorphTo` 按多态类型选择已注册的目标，返回由 `MorphTarget` 组成的分页。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::{MorphTarget, MorphTo}}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn subject_page<P: Model>(
    child: &P,
    relation: &MorphTo<P>,
) -> DatabaseResult<CursorPaginator<MorphTarget>> {
    relation.cursorPaginateWith(child, CursorOptions {
        perPage: Some(15),
        cursorName: "subjects",
        ..Default::default()
    }).await
}
```

`HasManyThrough` 和 `HasOneThrough` 也支持这三种分页方法。查询使用中间表连接，默认排除已软删除的中间模型。调用 `withTrashedParents()` 可包含这些模型。默认只选择关联模型的列。连接表有同名列时，自定义排序应加表名前缀。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasManyThrough}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn through_page<P: Model, I: Model, R: Model>(
    parent: &P,
    relation: &HasManyThrough<P, I, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation.cursorPaginateWith(parent, CursorOptions {
        perPage: Some(15),
        cursorName: "reactions",
        ..Default::default()
    }).await
}
```

`BelongsToMany` 和 `MorphToMany` 支持相同的分页方法。当前页的每个模型都会加载 Pivot，支持自定义模型和访问器。游标分页会查询 `orderByPivot` 使用的字段以生成游标。通过 `withPivot` 指定要在 Pivot 上公开的额外字段。排序字段的组合必须唯一标识每一行。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::BelongsToMany}},
    pagination::CursorPaginator,
};

async fn pivot_page<P: Model, R: Model>(
    parent: &P,
    relation: BelongsToMany<P, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation
        .withPivot(["label"])
        .orderByPivot("weight", "asc")
        .orderByPivot("label_id", "asc")
        .cursorPaginate(parent, 15)
        .await
}
```

### Eloquent 分页的集合键 {#collection-keys-on-eloquent-pages}

Eloquent 在水合当前页并加载关系后调用 `Model::newCollection`。普通和简易分页保留自定义键，并可通过 `getByKey` 查找记录。游标分页按位置重新编号为列表。`items()` 都支持按序遍历。

```rust
use openrust::{
    database::eloquent::Model,
    pagination::LengthAwarePaginator,
};

fn find_on_page<'a, M: Model>(
    page: &'a LengthAwarePaginator<M>,
    key: &str,
) -> Option<&'a M> {
    page.getByKey(key)
}
```

### 加载当前页的多态关系 {#loading-polymorphic-relations-on-the-current-page}

在 Eloquent 分页器上调用 `loadMorph` 或 `loadMorphCount`，可为指定的多态目标加载下级关系或关系计数。方法只处理当前页，并返回同一个分页器供链式调用。

```rust
use openrust::{
    database::{
        DatabaseResult,
        eloquent::{Model, relations::{ExistenceRelation, MorphTo}},
    },
    pagination::LengthAwarePaginator,
};

async fn load_page_subjects<M: Model, T: Model>(
    page: &mut LengthAwarePaginator<M>,
    subject: &MorphTo<M>,
    comments: &impl ExistenceRelation<Parent = T>,
) -> DatabaseResult<()> {
    page.loadMorph("subject", subject, "article", "comments", comments)
        .await?
        .loadMorphCount("subject", subject, "article", "comments", comments)
        .await?;
    Ok(())
}
```

## 显示分页结果 {#displaying-pagination-results}

用分页器的 `links()` 创建 Blade 视图。默认 Tailwind 视图为 `paginate` 显示记录范围和页码链接。`simplePaginate` 和 `cursorPaginate` 显示上一页和下一页。结果只有一页时不输出导航。`onEachSide` 可调整页码窗口。

```rust
use openrust::{
    contracts::view::View as _,
    pagination::LengthAwarePaginator,
    view::ViewResult,
};

fn pagination_navigation<T>(page: &LengthAwarePaginator<T>) -> ViewResult<String> {
    page.links()?.render()
}
```

使用 `linksWith("articles.navigation", data)` 指定自定义视图，也可以在 `resources/views/vendor/pagination/` 下放置 `tailwind.blade.html` 或 `simple-tailwind.blade.html` 覆盖默认视图。自定义视图会收到 `paginator` 元数据，带总数的分页还会传入页码 `elements`。在 `lang/{locale}/pagination.json` 配置 `previous` 和 `next`，即可本地化导航和 JSON 链接标签。

## 自定义分页视图 {#customizing-the-pagination-view}

在应用服务提供者的 `boot` 中，用 `Paginator::defaultView("articles.navigation")` 指定带页码的视图，用 `Paginator::defaultSimpleView("articles.simple-navigation")` 指定简易和游标分页的视图。设置只作用于当前应用。调用 `linksWith` 时指定的视图优先。

用 `Paginator::useBootstrapFive()`、`useBootstrapFour()` 或 `useBootstrapThree()` 同时设置两类视图。`useBootstrap()` 选择 Bootstrap 4，`useTailwind()` 恢复 Tailwind 默认视图。应用需要加载对应的 CSS。

```rust
use openrust::{
    foundation::Application,
    http::HttpFuture,
    pagination::Paginator,
    support::{ProviderResult, ServiceProvider},
};

struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    fn boot(&mut self, _: &Application) -> HttpFuture<'_, ProviderResult> {
        Box::pin(async {
            Paginator::useBootstrapFive()?;
            Ok(())
        })
    }
}
```

使用 `pagination::semantic-ui` 可指定 Semantic UI 页码视图。命令将九个内置模板发布到 `resources/views/vendor/pagination`。修改已发布的模板即可覆盖内置视图。加上 `--force` 可覆盖已有文件。

```bash
./artisan vendor:publish --tag=laravel-pagination
```

## 分页窗口和 JSON {#pagination-windows-and-json}

用 `UrlWindow::make(&page)` 取得根据 `onEachSide` 算出的开头、滑动窗口和结尾链接。每段按顺序保存 `(页码, URL)`。`linkCollection()` 返回平铺的页码、省略号，以及已翻译的上一页和下一页标签。

```rust
use openrust::pagination::{LengthAwarePaginator, UrlWindow, UrlWindowLinks};

fn numbered_links<T>(page: &LengthAwarePaginator<T>) -> UrlWindowLinks {
    UrlWindow::make(page)
}
```

`toArray()` 返回 JSON 对象。`toJson()` 和 `toPrettyJson()` 编码相同的元数据与记录。手工调用 `LengthAwarePaginator::new` 可传入页容量零，`lastPage()` 按每页一条计算。查询分页仍要求页容量为正。

## JSON 和分页链接

当分页项实现 Serde 的 `Serialize` 时，可直接序列化分页器。用 `map` 将数据库行或 Eloquent 模型转换为响应项，保留分页元数据。`withQueryString` 保留筛选条件，`appends` 添加选定参数。链接使用请求相对路径。

```rust
use openrust::database::{DB, DatabaseResult};

async fn article_ids(table: &str) -> DatabaseResult<impl serde::Serialize> {
    let mut page = DB::table(table)?
        .orderBy("id", "asc")
        .paginate(15)
        .await?;
    page.withQueryString();
    Ok(page.map(|row| match row.get("id") {
        Some(openrust::database::Value::Integer(id)) => Some(id.to_string()),
        Some(openrust::database::Value::Unsigned(id)) => Some(id.to_string()),
        _ => None,
    }))
}
```
