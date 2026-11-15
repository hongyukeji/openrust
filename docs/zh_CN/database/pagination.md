# 数据库分页

## 基本用法

`paginate(15)` 先查询总数，再读取当前页。只需要上一页和下一页时，使用 `simplePaginate(15)`，它多读一行来判断是否还有下一页。Query Builder 和 Eloquent 均支持这两种方法。Eloquent 默认按主键排序。Query Builder 可显式指定排序以保证跨页顺序稳定。

使用 `paginateWith` 或 `simplePaginateWith` 并传入 `PageOptions`，可以自定义页码参数名、查询列和页码。HTTP 从当前请求的查询字符串读取页码。HTTP 之外默认使用第 1 页。

## 游标分页

`cursorPaginate(15)` 比较排序列的值，并多读一行判断后续记录。下次请求传入返回的 `next_cursor` 或 `prev_cursor`。Query Builder 需按已查询、非空的标量列排序，最后使用唯一列打破并列。Eloquent 在未声明排序时使用主键。分组查询、联合查询和原始排序表达式使用偏移分页。

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
