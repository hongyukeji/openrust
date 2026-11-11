# 查询构造器

## 执行数据库查询

在应用生命周期内使用 `DB::table("users")?`，或对具名连接调用 `connection.table("users")`。事务中使用 `transaction.table("users")`，让查询继续使用同一条写库物理连接。查询执行方法为异步，返回 `Result`。

`get` 返回行列表。`first` 和 `find` 返回可选行。`value` 返回单个值，找不到行时返回 `Value::Null`。`pluck` 返回一列的值列表。用 `exists` 或 `doesntExist` 判断是否存在匹配行。`useWritePdo` 将该查询发送到写库。

### 聚合

使用 `count("*")`、`min("column")`、`max("column")`、`sum("column")` 或 `avg("column")`。计数返回无符号整数，其他聚合保留驱动返回的值类型。MySQL 的精确小数以文本返回。空集合求和返回零。

## SELECT 语句

用 `select` 指定返回列，用 `addSelect` 追加列。列名支持 `as` 别名。`distinct()` 去除重复行。语法编译器引用标识符并应用连接的表前缀。用 `selectRaw` 或 `DB::raw` 显式传入可信表达式。值单独绑定，标识符从应用预先定义的名称中选择。

## 基本 WHERE 子句

Rust 中 `where` 是关键字，使用 `r#where(column, operator, value)` 对应 Laravel 的 `where`。`orWhere` 用 OR 连接条件，用 `whereNested` 加括号分组，用 `whereNot` 否定分组。可用 `whereNull`、`whereNotNull`、`whereIn`、`whereNotIn`、`whereBetween` 或 `whereColumn` 表达相应比较。空 IN 不匹配任何行，空 NOT IN 匹配所有行。

`whereLike` 接受区分大小写参数。`whereDate` 和 `whereTime` 分别接收 `chrono::NaiveDate` 和 `chrono::NaiveTime`。可用 `toSql()` 和 `getBindings()` 查看 SQL 和按执行顺序排列的参数。

```rust
use openrust::database::{Connection, DatabaseResult, Row, Value};

pub async fn active_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["id", "name"])
        .r#where("active", "=", true)
        .whereNested(|query| {
            query
                .whereNull("archived_at")
                .orWhere("name", "=", "Admin")
        })
        .orderBy("id", "asc")
        .forPage(1, 20)
        .get()
        .await
}

pub async fn active_count(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("users")
        .r#where("active", "=", Value::Bool(true))
        .count("*")
        .await
}
```

## 排序、分组、限制和偏移

用 `orderBy("id", "asc")` 或 `orderByDesc("id")` 排序。`reorder()` 清除已有排序。用 `groupBy` 和 `having` 筛选分组。`limit` 和 `offset` 选取指定范围的行。`forPage(page, per_page)` 的页码从 1 开始。SQLite 和 MySQL 执行偏移查询时要求同时指定限制行数。分页时请指定能稳定确定行顺序的排序条件。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn departments(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["department"])
        .selectRaw("count(*) as total", Vec::new())
        .groupBy(&["department"])
        .having("total", ">=", 2)
        .orderByDesc("total")
        .get()
        .await
}
```
