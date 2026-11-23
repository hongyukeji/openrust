# 数据库分页

## 基本用法

`paginate(15)` 先查询总数，再读取当前页。只需要上一页和下一页时，使用 `simple_paginate(15)`，它多读一行来判断是否还有下一页。Query Builder 和 Eloquent 均支持这两种方法。Eloquent 默认按主键排序。Query Builder 可显式指定排序以保证跨页顺序稳定。

使用 `paginate_with` 或 `simple_paginate_with` 并传入 `PageOptions`，可以自定义页码参数名、查询列和页码。HTTP 从当前请求的查询字符串读取页码。HTTP 之外默认使用第 1 页。

用 `ModelMetadata::per_page(size)` 声明模型的默认页容量。省略 `per_page` 时，Eloquent 及关系查询采用所选模型的配置。显式页容量优先。Query Builder 默认每页 15 条。普通分页可通过 `PageOptions::total` 传入预计算总数，跳过计数查询。总数为零时直接返回空页，无需读取记录。如果总数来自缓存或其他服务，请先取得总数再构造选项。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn page_with_cached_total<M: Model>(total: u64) -> DatabaseResult<LengthAwarePaginator<M>> {
    M::query()?.paginate_with(PageOptions {
        total: Some(total),
        ..Default::default()
    }).await
}
```

普通分页也支持在 `PageOptions` 中设置回调。用 `total_using` 提供总数，或返回 `Ok(None)` 让查询执行计数。显式数值 `total` 优先。总数确定后，`per_page_using` 接收它并选定页容量，或返回 `Ok(None)` 沿用显式容量或模型默认值。回调报错会在读取页面记录前终止分页。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn adaptive_page<M: Model>() -> DatabaseResult<LengthAwarePaginator<M>> {
    M::query()?.paginate_with(PageOptions {
        per_page_using: Some(&|total| Ok((total > 100).then_some(50))),
        ..Default::default()
    }).await
}
```

## 游标分页

`cursor_paginate(15)` 比较排序列的值，并多读一行判断后续记录。下次请求传入返回的 `next_cursor` 或 `prev_cursor`。Query Builder 需按已查询、非空的标量列排序，最后使用唯一列打破并列。Eloquent 在未声明排序时使用主键。分组查询、联合查询和原始排序表达式使用偏移分页。

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
            cursor_name: Some("position"),
            ..Default::default()
        },
        |id| [("id", Value::Integer(*id))],
    )?;
    page.with_query_string();
    Ok(page.through(|id| id.to_string()))
}
```

## 分页器实例方法 {#paginator-instance-methods}

用 `count()`、`on_first_page()` 和 `on_last_page()` 获取页面状态。`through` 消耗原分页器并转换记录类型，同时保留分页元数据。游标仍使用转换前的排序键。

用 `with_path` 修改链接路径，`set_page_name` 或 `set_cursor_name` 修改查询参数名，`with_query_string` 保留请求筛选条件，`fragment` 添加锚点。`get_options()` 返回当前生效的链接选项。

一个响应包含多个分页器时，给每个分页器设置独立的查询参数。例如，分别将 `PageOptions.page_name` 设为 `articles` 和 `comments`。保留请求查询时，每个分页器生成链接只替换自己的页码，保留另一个页码和筛选项。用 `appends` 添加指定筛选项，用 `fragment` 指向对应结果区域。

## 关联模型分页 {#paginating-related-models}

在 `HasMany` 或 `MorphMany` 描述符上调用 `paginate`、`simple_paginate` 或 `cursor_paginate`，传入父模型和每页数量。带 `With` 的变体接受与 Eloquent 查询相同的选项。查询保留父键、适用的多态类型、已配置的筛选和模型作用域。父模型没有键时返回空页。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasMany}},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn related_comments<P: Model, R: Model>(
    article: &P,
    comments: &HasMany<P, R>,
) -> DatabaseResult<LengthAwarePaginator<R>> {
    comments.paginate_with(article, PageOptions {
        per_page: Some(15),
        page_name: "comments",
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
    relation.cursor_paginate_with(child, CursorOptions {
        per_page: Some(15),
        cursor_name: "subjects",
        ..Default::default()
    }).await
}
```

`HasManyThrough` 和 `HasOneThrough` 也支持这三种分页方法。查询使用中间表连接，默认排除已软删除的中间模型。调用 `with_trashed_parents()` 可包含这些模型。默认只选择关联模型的列。连接表有同名列时，自定义排序应加表名前缀。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasManyThrough}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn through_page<P: Model, I: Model, R: Model>(
    parent: &P,
    relation: &HasManyThrough<P, I, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation.cursor_paginate_with(parent, CursorOptions {
        per_page: Some(15),
        cursor_name: "reactions",
        ..Default::default()
    }).await
}
```

`BelongsToMany` 和 `MorphToMany` 支持相同的分页方法。当前页的每个模型都会加载 Pivot，支持自定义模型和访问器。游标分页会查询 `order_by_pivot` 使用的字段以生成游标。通过 `with_pivot` 指定要在 Pivot 上公开的额外字段。排序字段的组合必须唯一标识每一行。

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
        .with_pivot(["label"])
        .order_by_pivot("weight", "asc")
        .order_by_pivot("label_id", "asc")
        .cursor_paginate(parent, 15)
        .await
}
```

### Eloquent 分页的集合键 {#collection-keys-on-eloquent-pages}

Eloquent 在水合当前页并加载关系后调用 `Model::new_collection`。普通和简易分页保留自定义键，并可通过 `get_by_key` 查找记录。游标分页按位置重新编号为列表。`items()` 都支持按序遍历。

```rust
use openrust::{
    database::eloquent::Model,
    pagination::LengthAwarePaginator,
};

fn find_on_page<'a, M: Model>(
    page: &'a LengthAwarePaginator<M>,
    key: &str,
) -> Option<&'a M> {
    page.get_by_key(key)
}
```

### 加载当前页的多态关系 {#loading-polymorphic-relations-on-the-current-page}

在 Eloquent 分页器上调用 `load_morph` 或 `load_morph_count`，可为指定的多态目标加载下级关系或关系计数。方法只处理当前页，并返回同一个分页器供链式调用。

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
    page.load_morph("subject", subject, "article", "comments", comments)
        .await?
        .load_morph_count("subject", subject, "article", "comments", comments)
        .await?;
    Ok(())
}
```

## 显示分页结果 {#displaying-pagination-results}

用分页器的 `links()` 创建 Blade 视图。默认 Tailwind 视图为 `paginate` 显示记录范围和页码链接。`simple_paginate` 和 `cursor_paginate` 显示上一页和下一页。结果只有一页时不输出导航。`on_each_side` 可调整页码窗口。

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

使用 `links_with("articles.navigation", data)` 指定自定义视图，也可以在 `resources/views/vendor/pagination/` 下放置 `tailwind.blade.html` 或 `simple-tailwind.blade.html` 覆盖默认视图。自定义视图会收到 `paginator` 元数据，带总数的分页还会传入页码 `elements`。在 `lang/{locale}/pagination.json` 配置 `previous` 和 `next`，即可本地化导航和 JSON 链接标签。

## 自定义分页视图 {#customizing-the-pagination-view}

在应用服务提供者的 `boot` 中，用 `Paginator::default_view("articles.navigation")` 指定带页码的视图，用 `Paginator::default_simple_view("articles.simple-navigation")` 指定简易和游标分页的视图。设置只作用于当前应用。调用 `links_with` 时指定的视图优先。

用 `Paginator::use_bootstrap_five()`、`use_bootstrap_four()` 或 `use_bootstrap_three()` 同时设置两类视图。`use_bootstrap()` 选择 Bootstrap 4，`use_tailwind()` 恢复 Tailwind 默认视图。应用需要加载对应的 CSS。

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
            Paginator::use_bootstrap_five()?;
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

用 `UrlWindow::make(&page)` 取得根据 `on_each_side` 算出的开头、滑动窗口和结尾链接。每段按顺序保存 `(页码, Url)`。`link_collection()` 返回平铺的页码、省略号，以及已翻译的上一页和下一页标签。

```rust
use openrust::pagination::{LengthAwarePaginator, UrlWindow, UrlWindowLinks};

fn numbered_links<T>(page: &LengthAwarePaginator<T>) -> UrlWindowLinks {
    UrlWindow::make(page)
}
```

`to_array()` 返回 JSON 对象。`to_json()` 和 `to_pretty_json()` 编码相同的元数据与记录。手工调用 `LengthAwarePaginator::new` 可传入页容量零，`last_page()` 按每页一条计算。查询分页仍要求页容量为正。

## JSON 和分页链接

当分页项实现 Serde 的 `Serialize` 时，可直接序列化分页器。用 `map` 将数据库行或 Eloquent 模型转换为响应项，保留分页元数据。`with_query_string` 保留筛选条件，`appends` 添加选定参数。链接使用请求相对路径。

```rust
use openrust::database::{Db, DatabaseResult};

async fn article_ids(table: &str) -> DatabaseResult<impl serde::Serialize> {
    let mut page = Db::table(table)?
        .order_by("id", "asc")
        .paginate(15)
        .await?;
    page.with_query_string();
    Ok(page.map(|row| match row.get("id") {
        Some(openrust::database::Value::Integer(id)) => Some(id.to_string()),
        Some(openrust::database::Value::Unsigned(id)) => Some(id.to_string()),
        _ => None,
    }))
}
```

## 自定义分页解析器 {#customizing-pagination-resolvers}

在服务提供者的 `boot` 中登记 `Paginator::current_page_resolver`、`current_path_resolver` 和 `query_string_resolver`。`LengthAwarePaginator` 共享这些设置。游标分页使用 `CursorPaginator::current_cursor_resolver`。页码和游标回调接收所配置的参数名；游标回调返回 `None` 时从第一页开始。

```rust
Paginator::current_page_resolver(|name| if name == "entries" { 2 } else { 1 })?;
Paginator::current_path_resolver(|| "/articles".to_owned())?;
Paginator::query_string_resolver(|| vec![("category".into(), "rust".into())])?;
```

解析器作用于当前应用中的手工分页、查询构建器、模型及关联分页。显式传入的页码、游标和路径优先。`with_query_string()` 在调用时读取查询解析器，同名值覆盖先前 `appends` 的参数，排除分页位置参数。查询回调返回字符串键值对或 `None`。`None` 和空列表都会关闭请求查询参数的追加；`resolve_query_string` 区分二者，显式返回 `None` 时不再使用后备值。嵌套值由应用编码。

`resolve_current_page(name, default)`、`resolve_current_path(default)`、`resolve_query_string(default)` 及 `CursorPaginator::resolve_current_cursor(name, default)` 提供相同的解析入口。应用请求之外没有对应回调时返回传入的默认值；默认请求来源中缺少或无效页码使用一，缺少或无效游标从头开始。默认链接路径仍为请求相对路径。

`Paginator::view_factory_resolver` 接收返回 `ViewResult<Arc<openrust::view::Factory>>` 的回调。`Paginator::view_factory`、`LengthAwarePaginator::view_factory` 及 `CursorPaginator::view_factory` 返回共享工厂，分页视图渲染也使用该工厂。回调要求 `Send + Sync + 'static`，在当前应用作用域中登记；没有可用应用容器时登记返回错误。
