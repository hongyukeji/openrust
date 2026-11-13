# 查询构造器

## 执行数据库查询

在应用生命周期内使用 `DB::table("users")?`，或对具名连接调用 `connection.table("users")`。事务中使用 `transaction.table("users")`，让查询继续使用同一条写库物理连接。查询执行方法为异步，返回 `Result`。

`get` 返回行列表。`first` 和 `find` 返回可选行。`value` 返回单个值，找不到行时返回 `Value::Null`。`pluck` 返回一列的值列表。用 `exists` 或 `doesntExist` 判断是否存在匹配行。`useWritePdo` 将该查询发送到写库。

### 聚合

使用 `count("*")`、`min("column")`、`max("column")`、`sum("column")` 或 `avg("column")`。计数返回无符号整数，其他聚合保留驱动返回的值类型。MySQL 的精确小数以文本返回。空集合求和返回零。

## SELECT 语句

用 `select` 指定返回列，用 `addSelect` 追加列。列名支持 `as` 别名。`distinct()` 去除重复行。语法编译器引用标识符并应用连接的表前缀。用 `selectRaw` 或 `DB::raw` 显式传入可信表达式。值单独绑定，标识符从应用预先定义的名称中选择。

## 联结

使用 `join`、`leftJoin`、`rightJoin` 或 `crossJoin`。需要多个 ON 条件时，给 `joinWith` 或 `leftJoinWith` 传入闭包。`on` 比较列，`r#where` 绑定值。`joinSub` 可联结派生查询。MySQL 支持相关 `joinLateral` 和 `leftJoinLateral`。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn items_in_region(connection: &Connection, region: &str) -> DatabaseResult<Vec<Row>> {
    let items = connection.table("items as i");
    let groups = items
        .subquery("groups as g")
        .select(["g.id"])
        .r#where("g.region", "=", region);

    items
        .joinSub(&groups, "regional", |join| join.on("i.group_id", "=", "regional.id"))?
        .select(["i.code", "i.stock"])
        .orderBy("i.code", "asc")
        .get()
        .await
}
```

## UNION

调用 `union(&other)?` 去重，或用 `unionAll(&other)?` 保留重复行。合并前设置的排序或行数限制作用于第一条查询，合并后设置的作用于合并结果。

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

## 高级 WHERE 子句

在同一具名连接上，用 `whereExists`、`whereNotExists`、`whereInSub` 或 `whereSub` 引入子查询。`selectSub` 添加标量投影，`fromSub` 从派生表读取。事务中用外层构造器的 `subquery` 创建子查询，整条语句通过外层事务连接执行。

## JSON WHERE 子句

用 `whereJsonContains` 和 `whereJsonDoesntContain` 判断 JSON 是否包含指定值，用 `whereJsonContainsKey` 和 `whereJsonDoesntContainKey` 检查路径是否存在，用 `whereJsonLength` 比较长度。用 `->` 分隔路径段，也可附加 `groups[0]` 这样的索引。每个方法都有 `orWhere` 变体。

```rust
use openrust::database::{Connection, DatabaseResult, Row};
use serde_json::json;

pub async fn matching_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .whereJsonContains("options->languages", json!("en"))
        .whereJsonContainsKey("options->timezone")
        .whereJsonLength("options->languages", ">", 1)
        .get()
        .await
}
```

MySQL 支持标量和数组候选值。SQLite 的包含条件接受标量。值为 JSON null 的键依然存在。路径和值均通过参数绑定传入 SQL。

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

## 插入语句

`insert` 接收一个或多个 `Record`，批量记录使用相同的列。`insertOrIgnore` 使用驱动原生的忽略行为，并返回影响行数。`insertUsing` 接收目标列和选择查询。`insertGetId` 从同一次执行结果取得主键。

### Upsert

`upsert` 接收记录、冲突列和需要更新的列。SQLite 使用传入的冲突列。MySQL 通过表上的唯一键判断冲突。

```rust
use openrust::database::{Connection, DatabaseResult, query::Record};

pub async fn save_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let mut transaction = connection.beginTransaction().await?;
    let row = Record::from([("code".into(), code.into()), ("stock".into(), stock.into())]);
    let affected = transaction
        .table("items")
        .upsert(&[row], &["code"], Some(&["stock"]))
        .await?;
    transaction.commit().await?;
    Ok(affected)
}
```

## 更新语句

`update` 返回影响行数。`updateOrInsert` 先检查记录是否存在，再执行写入。需要在一条 SQL 内处理插入和冲突时使用 `upsert`。`increment` 和 `decrement` 在 SQL 中运算，并可附带更新其他列。

### 更新 JSON 列

向 `update` 传入形如 `options->preferences->enabled` 的键。MySQL 使用 `JSON_SET`，支持数组下标。SQLite 使用合并补丁，值为 null 的成员会删除对应键。

## 删除语句

设置条件后调用 `delete()` 返回影响行数。未设置条件时，删除作用于整张表。`truncate()` 清空表并重置自增主键。MySQL 的该语句会隐式提交，请在事务外运行。

## 悲观锁

在事务中使用 `lockForUpdate` 或 `sharedLock`。MySQL 持有行锁直至提交或回滚。SQLite 采用自身的事务锁机制，没有对应的行锁子句。
