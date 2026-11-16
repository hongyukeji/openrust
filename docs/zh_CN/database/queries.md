# 查询构造器

## 执行数据库查询

在应用生命周期内使用 `DB::table("users")?`，或对具名连接调用 `connection.table("users")`。事务中使用 `transaction.table("users")`，让查询继续使用同一条写库物理连接。查询执行方法为异步，返回 `Result`。

`get` 返回行列表。`first` 和 `find` 返回可选行。`value` 返回单个值，找不到行时返回 `Value::Null`。`pluck` 返回一列的值列表。用 `exists` 或 `doesntExist` 判断是否存在匹配行。`useWritePdo` 将该查询发送到写库。

### 聚合

使用 `count("*")`、`min("column")`、`max("column")`、`sum("column")` 或 `avg("column")`。计数返回无符号整数，其他聚合保留驱动返回的值类型。MySQL 的精确小数以文本返回。空集合求和返回零。

## 流式读取结果 {#streaming-results}

在查询上调用 `cursor()` 逐行读取结果。首次轮询时开始查询，`try_next` 读取并解码一行，错误会由流返回。连接和查询游标沿用已配置的读写路由。丢弃非事务游标会释放连接租约。

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{Connection, DatabaseResult, Row};

async fn first_streamed_row(db: &Connection) -> DatabaseResult<Option<Row>> {
    let mut rows = db.table("reports").orderBy("id", "asc").cursor();
    rows.try_next().await
}
```

事务中应先读完游标，再执行下一条语句或提交。语句尚未执行完时丢弃游标会中断事务，丢弃该事务会回滚。

### 分批读取结果 {#chunking-results}

在已排序的查询上调用 `chunk(count, callback)`。异步回调收到非空批次和从 1 开始的页码。返回 `Ok(false)` 提前停止，返回 `Err` 传播错误。现有偏移量用于跳过开头的行，现有限制用于控制总行数。分批通过偏移量翻页，遍历期间修改匹配条件所涉及的行可能导致漏读或重复读取。

用 `chunkById(count, callback, column, alias)` 或 `chunkByIdDesc` 按唯一且非空的键逐批读取。将 `column` 设为 `None` 时使用 `id`。SQL 投影名与列名不同时，用 `alias` 指定投影名。偏移量仅用于首批，限制控制所有批次的总行数。处理期间保持分批键稳定。回调可在检查分批键前停止。用 `fromSub` 包装联合查询，使键值条件作用于全部分支。

```rust
use openrust::database::{Connection, DatabaseResult};

async fn process_by_id(db: &Connection) -> DatabaseResult<bool> {
    db.table("reports")
        .select(["id as batch_id"])
        .chunkByIdDesc(200, |rows, page| async move {
            println!("{}: {}", page, rows.len());
            Ok(true)
        }, None, Some("batch_id"))
        .await
}
```

用 `each(callback, count)` 逐条处理偏移量分批结果，每批索引从零开始。`eachById(callback, count, column, alias)` 按键升序分批读取，索引跨批递进。回调返回 `Ok(false)` 可立即停止，返回 `Err` 可传播错误。

用 `lazy(count)`、`lazyById(count, column, alias)` 或 `lazyByIdDesc` 将逐批读取的行作为异步流消费。读取流时会检查参数并返回数据库错误。丢弃流后即停止获取后续批次。按键读取的满批需要投影边界键，最后一批不足批量时可直接结束。`chunkMap(callback, count)` 异步映射每条记录，返回重新编号的结果集合。

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

### JSON 重叠 {#json-overlap}

`whereJsonOverlaps` 检查选中的 JSON 值是否与候选值共享元素或键值对。MySQL 使用 `JSON_OVERLAPS`。`whereJsonDoesntOverlap` 对谓词取反，两者均有 `orWhere` 变体。路径不存在或文档为 SQL NULL 时，MySQL 返回 SQL NULL，WHERE 会过滤该行，取反时也是如此。

```rust
use openrust::database::{Connection, DatabaseResult, Value};
use serde_json::json;

async fn eligible_user_ids(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection
        .table("users")
        .whereJsonOverlaps("options->languages", json!(["en", "fr"]))
        .orderBy("id", "asc")
        .pluck("id")
        .await
}
```

候选值可以是 JSON 值，也可以是含可信 SQL 和独立绑定的 `Expression`。SQLite 对这些谓词返回不支持操作的错误。

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

#### 冲突时赋值 {#upsert-conflict-assignments}

需要在冲突时同时使用插入值和指定值时，调用 `upsertWith`。`UpsertUpdate::FromInsert` 复制待插入记录里的指定列，`UpsertUpdate::Value` 绑定固定值，`UpsertUpdate::Expression` 接收可信 SQL 和独立绑定。没有冲突的记录按插入值和表默认值写入。

```rust
use openrust::database::{
    Connection, DatabaseResult,
    query::{Record, UpsertUpdate},
};

pub async fn import_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let row = Record::from([("code".into(), code.into()), ("stock".into(), stock.into())]);
    connection
        .table("items")
        .upsertWith(
            &[row],
            &["code"],
            &[
                UpsertUpdate::FromInsert("stock"),
                UpsertUpdate::Value("source", "inventory import".into()),
            ],
        )
        .await
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
