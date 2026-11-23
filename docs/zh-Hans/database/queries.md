# 查询构造器

## 执行数据库查询

在应用生命周期内使用 `Db::table("users")?`，或对具名连接调用 `connection.table("users")`。事务中使用 `transaction.table("users")`，让查询继续使用同一条写库物理连接。查询执行方法为异步，返回 `Result`。

`get` 返回行列表。`first` 和 `find` 返回可选行。`value` 返回单个值，找不到行时返回 `Value::Null`。`pluck` 返回一列的值列表。用 `exists` 或 `doesnt_exist` 判断是否存在匹配行。`use_write_pdo` 将该查询发送到写库。

### 聚合

使用 `count("*")`、`min("column")`、`max("column")`、`sum("column")` 或 `avg("column")`。计数返回无符号整数，其他聚合保留驱动返回的值类型。MySQL 的精确小数以文本返回。空集合求和返回零。

## 流式读取结果 {#streaming-results}

在查询上调用 `cursor()` 逐行读取结果。首次轮询时开始查询，`try_next` 读取并解码一行，错误会由流返回。连接和查询游标沿用已配置的读写路由。丢弃非事务游标会释放连接租约。

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{Connection, DatabaseResult, Row};

async fn first_streamed_row(db: &Connection) -> DatabaseResult<Option<Row>> {
    let mut rows = db.table("reports").order_by("id", "asc").cursor();
    rows.try_next().await
}
```

事务中应先读完游标，再执行下一条语句或提交。语句尚未执行完时丢弃游标会中断事务，丢弃该事务会回滚。

### 分批读取结果 {#chunking-results}

在已排序的查询上调用 `chunk(count, callback)`。异步回调收到非空批次和从 1 开始的页码。返回 `Ok(false)` 提前停止，返回 `Err` 传播错误。现有偏移量用于跳过开头的行，现有限制用于控制总行数。分批通过偏移量翻页，遍历期间修改匹配条件所涉及的行可能导致漏读或重复读取。

用 `chunk_by_id(count, callback, column, alias)` 或 `chunk_by_id_desc` 按唯一且非空的键逐批读取。将 `column` 设为 `None` 时使用 `id`。SQL 投影名与列名不同时，用 `alias` 指定投影名。偏移量仅用于首批，限制控制所有批次的总行数。处理期间保持分批键稳定。回调可在检查分批键前停止。用 `from_sub` 包装联合查询，使键值条件作用于全部分支。

```rust
use openrust::database::{Connection, DatabaseResult};

async fn process_by_id(db: &Connection) -> DatabaseResult<bool> {
    db.table("reports")
        .select(["id as batch_id"])
        .chunk_by_id_desc(200, |rows, page| async move {
            println!("{}: {}", page, rows.len());
            Ok(true)
        }, None, Some("batch_id"))
        .await
}
```

用 `each(callback, count)` 逐条处理偏移量分批结果，每批索引从零开始。`each_by_id(callback, count, column, alias)` 按键升序分批读取，索引跨批递进。回调返回 `Ok(false)` 可立即停止，返回 `Err` 可传播错误。

用 `lazy(count)`、`lazy_by_id(count, column, alias)` 或 `lazy_by_id_desc` 将逐批读取的行作为异步流消费。读取流时会检查参数并返回数据库错误。丢弃流后即停止获取后续批次。按键读取的满批需要投影边界键，最后一批不足批量时可直接结束。`chunk_map(callback, count)` 异步映射每条记录，返回重新编号的结果集合。

## SELECT 语句

用 `select` 指定返回列，用 `add_select` 追加列。列名支持 `as` 别名。`distinct()` 去除重复行。语法编译器引用标识符并应用连接的表前缀。用 `select_raw` 或 `Db::raw` 显式传入可信表达式。值单独绑定，标识符从应用预先定义的名称中选择。

### 索引提示 {#index-hints}

同一查询以最后设置的索引提示为准。MySQL 和 MariaDB 支持 `use_index`、`force_index` 和 `ignore_index`，可指定单个或以逗号分隔的多个名称。SQL Server 的 `force_index` 接收一个索引，并与同表的行锁提示合并。SQLite 的 `force_index` 对单个非 `PRIMARY` 索引生效。SQL Server 和 SQLite 忽略其余两种方法。PostgreSQL 忽略这三种提示。请使用应用定义的索引名称。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn indexed_items(connection: &Connection, score: i64) -> DatabaseResult<Vec<Row>> {
    connection
        .table("items")
        .force_index("idx_score")
        .r#where("score", "=", score)
        .order_by("id", "asc")
        .get()
        .await
}
```

## 联结

使用 `join`、`left_join`、`right_join` 或 `cross_join`。需要多个 ON 条件时，给 `join_with` 或 `left_join_with` 传入闭包。`on` 比较列，`r#where` 绑定值。`join_sub` 可联结派生查询。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn items_in_region(connection: &Connection, region: &str) -> DatabaseResult<Vec<Row>> {
    let items = connection.table("items as i");
    let groups = items
        .subquery("groups as g")
        .select(["g.id"])
        .r#where("g.region", "=", region);

    items
        .join_sub(&groups, "regional", |join| join.on("i.group_id", "=", "regional.id"))?
        .select(["i.code", "i.stock"])
        .order_by("i.code", "asc")
        .get()
        .await
}
```

### 交叉联结与固定顺序联结 {#cross-and-straight-joins}

使用 `cross_join_sub` 将每一条外层记录与派生查询的每一条记录配对。MySQL 和 MariaDB 的 `straight_join` 保持左侧来源先于右侧来源。使用 `straight_join_where` 在联结条件中绑定值，`straight_join_with` 接收闭包，`straight_join_sub` 接收派生查询。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn label_paid_orders(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    let orders = connection.table("orders as o");
    let labels = orders.subquery("labels").r#where("active", "=", 1);

    orders
        .straight_join("payments as p", "o.id", "=", "p.order_id")
        .cross_join_sub(&labels, "label")?
        .r#where("p.state", "=", "paid")
        .select(["o.id", "label.code"])
        .order_by("label.code", "asc")
        .get()
        .await
}
```

### 关联子查询联结 {#lateral-joins}

PostgreSQL、MySQL 8.0.14+ 和 SQL Server 支持 `join_lateral` 与 `left_join_lateral`。在子查询中用 `where_column` 引用外层列。左联结保留未匹配的外层行，右侧列返回 null。PostgreSQL 与 MySQL 使用 `JOIN LATERAL`，SQL Server 使用 `CROSS APPLY` 或 `OUTER APPLY`。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn latest_posts(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    let users = connection.table("users as u");
    let posts = users
        .subquery("posts as p")
        .select(["p.id as post_id"])
        .where_column("p.user_id", "=", "u.id")
        .r#where("p.published", "=", 1)
        .order_by_desc("p.id")
        .limit(1);

    users
        .left_join_lateral(&posts, "latest")?
        .select(["u.id", "latest.post_id"])
        .order_by("u.id", "asc")
        .get()
        .await
}
```

## UNION

调用 `union(&other)?` 去重，或用 `union_all(&other)?` 保留重复行。合并前设置的排序或行数限制作用于第一条查询，合并后设置的作用于合并结果。

## 基本 WHERE 子句

Rust 中 `where` 是关键字，使用 `r#where(column, operator, value)` 对应 `where`。`or_where` 用 OR 连接条件，用 `where_nested` 加括号分组，用 `where_not` 否定分组。可用 `where_null`、`where_not_null`、`where_in`、`where_not_in`、`where_between` 或 `where_column` 表达相应比较。空 IN 不匹配任何行，空 NOT IN 匹配所有行。

`where_like` 接受区分大小写参数。`where_date` 和 `where_time` 分别接收 `jiff::civil::Date` 和 `jiff::civil::Time`。可用 `to_sql()` 和 `get_bindings()` 查看 SQL 和按执行顺序排列的参数。

```rust
use openrust::database::{Connection, DatabaseResult, Row, Value};

pub async fn active_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["id", "name"])
        .r#where("active", "=", true)
        .where_nested(|query| {
            query
                .where_null("archived_at")
                .or_where("name", "=", "Admin")
        })
        .order_by("id", "asc")
        .for_page(1, 20)
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

用 `where_exists`、`where_not_exists`、`where_in_sub` 或 `where_sub` 引入子查询。`select_sub` 添加标量投影，`from_sub` 从派生表读取。事务中用外层构造器的 `subquery` 创建子查询，整条语句通过外层事务连接执行。

### MySQL 跨库子查询 {#mysql-cross-schema-subqueries}

两个 MySQL 连接配置的服务端点一致时，可以组合它们的查询构建器。框架会给子查询尚未限定的主表加上库名，保留已指定库名的表。额外的 JOIN 表或预先构造的派生查询需显式指定库名。

```rust
use openrust::database::{Connection, DatabaseResult, Value};

async fn active_customers(
    primary: &Connection,
    reporting: &Connection,
) -> DatabaseResult<Vec<Value>> {
    let active = reporting
        .table("customer_activity")
        .select(["customer_id"])
        .r#where("active", "=", 1);
    primary
        .table("customers")
        .where_in_sub("id", &active)?
        .order_by("id", "asc")
        .pluck("id")
        .await
}
```

MySQL 用外层连接的权限执行整条语句。外层连接的账号需要有权访问引用的表。

### 向量相似度子句 {#vector-similarity-clauses}

向量查询使用 PostgreSQL 的 `pgvector` 扩展或 MariaDB 11.7 及以上版本的余弦距离。启用 PostgreSQL 扩展、声明列和索引的方法见[向量列](./migrations.md#vector-columns)。

`where_vector_similar_to` 接收数值向量、0 到 1 的最低相似度，以及是否将最相似的行排在前面的标志。传入 `false` 可自行排序。相似度 1 匹配方向相同的向量。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn related_documents(
    connection: &Connection,
    embedding: &[f64],
) -> DatabaseResult<Vec<Row>> {
    connection
        .table("documents")
        .select(["id"])
        .select_vector_distance("embedding", embedding, Some("distance"))?
        .where_vector_similar_to("embedding", embedding, 0.7, true)?
        .limit(10)
        .get()
        .await
}
```

可独立使用 `select_vector_distance`、`where_vector_distance_less_than`、`or_where_vector_distance_less_than` 和 `order_by_vector_distance`。距离过滤包含等于上限的结果。查询 PostgreSQL 向量列时，结果以文本表示向量。MariaDB 返回向量的字节值。

Eloquent 构建器提供同样的向量方法。在 `where_nested` 里可以将距离谓词与其他条件分组。Eloquent 全局作用域约束整个分组，包括每个 `or_where_vector_distance_less_than` 分支。

```rust
use openrust::database::eloquent::Model;

let mut query = Document::query_on(&connection).select(["id"]);
query.where_nested(|group| {
    group
        .where_vector_distance_less_than("embedding", &primary, 0.3)
        .or_where_vector_distance_less_than("embedding", &alternate, 0.2)
});
let documents = query.order_by_vector_distance("embedding", &primary)?.get().await?;
```

#### 文本嵌入 {#text-embeddings}

在应用容器中注册 `Embeddings`。内置的 `HttpEmbeddingProvider` 接收兼容 OpenAI 嵌入协议的完整端点 URL、模型、令牌及可选维度。请从受信任的应用配置读取这些值。其他服务可实现 `EmbeddingProvider`，并通过 `Embeddings::new` 注册。

```rust
use std::sync::Arc;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::query::embeddings::{Embeddings, HttpEmbeddingProvider},
};

let provider = HttpEmbeddingProvider::new(&endpoint, model, token, Some(1536))?;
app.container().instance(Arc::new(Embeddings::new(provider)))?;
```

Query Builder 和 Eloquent 中带 `Text` 后缀的方法先异步生成向量，再组装 SQL 子句。数值向量方法仍是同步的。缓存未命中时，查询文本会发送给配置的 Provider。成功结果按 Provider 标识与文本在应用默认缓存中保存 30 天。内置标识包含端点、模型、凭据和维度。

```rust
let documents = connection
    .table("documents")
    .select(["id"])
    .select_vector_distance_text("embedding", "Rust web frameworks", Some("distance"))
    .await?
    .where_vector_similar_to_text("embedding", "Rust web frameworks", 0.7, true)
    .await?
    .limit(10)
    .get()
    .await?;
```

## JSON WHERE 子句

用 `where_json_contains` 和 `where_json_doesnt_contain` 判断 JSON 是否包含指定值，用 `where_json_contains_key` 和 `where_json_doesnt_contain_key` 检查路径是否存在，用 `where_json_length` 比较长度。用 `->` 分隔路径段，也可附加 `groups[0]` 这样的索引。每个方法都有 `or_where` 变体。

```rust
use openrust::database::{Connection, DatabaseResult, Row};
use serde_json::json;

pub async fn matching_users(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .where_json_contains("options->languages", json!("en"))
        .where_json_contains_key("options->timezone")
        .where_json_length("options->languages", ">", 1)
        .get()
        .await
}
```

MySQL 和 PostgreSQL 支持标量、数组及对象候选值。SQLite 的包含条件接受标量。值为 JSON null 的键仍存在。PostgreSQL 对 `json` 和 `jsonb` 列使用原生 JSONB 包含运算，并用 `jsonb_array_length` 检查所选数组的长度。键存在条件支持对象键和数组索引，包括 `options->languages[-1]` 这样的负索引。键缺失或文档为 SQL NULL 时，存在条件返回 false。PostgreSQL 将路径段转义为 SQL 字面量，候选值和对象键分别绑定。

SQL Server 使用 `OPENJSON` 检查标量包含、键存在和长度。长度条件统计数组元素或对象成员，路径缺失或文档为 SQL NULL 时计数为零。成员名区分大小写，可用 `options->languages[0]` 这类路径检查数组位置。数据库兼容级别须为 130 或更高。

### JSON 重叠 {#json-overlap}

`where_json_overlaps` 检查选中的 JSON 值是否与候选值共享元素或键值对。MySQL 使用 `JSON_OVERLAPS`。`where_json_doesnt_overlap` 对谓词取反，两者均有 `or_where` 变体。路径不存在或文档为 SQL NULL 时，MySQL 返回 SQL NULL，WHERE 会过滤该行，取反时也是如此。

```rust
use openrust::database::{Connection, DatabaseResult, Value};
use serde_json::json;

async fn eligible_user_ids(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection
        .table("users")
        .where_json_overlaps("options->languages", json!(["en", "fr"]))
        .order_by("id", "asc")
        .pluck("id")
        .await
}
```

候选值可以是 JSON 值，也可以是含可信 SQL 和独立绑定的 `Expression`。SQLite 对这些谓词返回不支持操作的错误。

## 排序、分组、限制和偏移

用 `order_by("id", "asc")` 或 `order_by_desc("id")` 排序。`reorder()` 清除已有排序。用 `group_by` 和 `having` 筛选分组。`limit` 和 `offset` 选取指定范围的行。`for_page(page, per_page)` 的页码从 1 开始。SQLite 和 MySQL 执行偏移查询时要求同时指定限制行数。分页时请指定能稳定确定行顺序的排序条件。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn departments(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection
        .table("users")
        .select(["department"])
        .select_raw("count(*) as total", Vec::new())
        .group_by(&["department"])
        .having("total", ">=", 2)
        .order_by_desc("total")
        .get()
        .await
}
```

### 值范围与指定顺序 {#value-ranges-and-ordering}

用 `where_value_between(value, [lower, upper])` 将绑定值与每行的两个列边界比较，范围包含端点。取反用 `where_value_not_between`，两种形式都有 OR 变体。

`in_order_of` 让列出的值按指定顺序排在前面。列表外的值和空值同属最后一档。需要固定同档顺序时再加一项排序。空列表保留已有排序。可信 SQL 操作数可用 `Expression` 并单独绑定参数。

```rust
use openrust::database::{Connection, DatabaseResult, Value};

pub async fn ordered_matches(connection: &Connection) -> DatabaseResult<Vec<Value>> {
    connection.table("order_ranges")
        .where_value_between(5, ["lower_bound", "upper_bound"])
        .in_order_of("status", vec!["active".into(), "pending".into()])
        .order_by("id", "asc")
        .pluck("id")
        .await
}
```

### 限制每组返回的行数 {#group-limit}

调用 `group_limit(count, column)` 限制分区列每个值对应的返回行数。用 `order_by` 决定每组保留哪些行。需要确定顺序时，加入唯一列作为同值排序条件。`offset` 在每组内跳过指定行数，分组限额为零时返回空结果。

查询构造器使用 `row_number()` 排名，返回的行隐藏内部排名列。需要对 UNION 结果分组限额时，先把 UNION 构造成派生表。

```rust
use openrust::database::{Connection, DatabaseResult, Row};

pub async fn second_per_category(connection: &Connection) -> DatabaseResult<Vec<Row>> {
    connection.table("group_items")
        .r#where("active", "=", true)
        .order_by_desc("score")
        .order_by("id", "asc")
        .group_limit(1, "category")
        .offset(1)
        .get()
        .await
}
```

## 插入语句

`insert` 接收一个或多个 `Record`，批量记录使用相同的列。`insert_or_ignore` 使用驱动原生的忽略行为，并返回影响行数。`insert_using` 接收目标列和选择查询。`insert_get_id` 从同一次执行结果取得主键。

### 生成的主键与插入行 {#insert-results}

在 PostgreSQL 上使用 `insert_get_id_for(record, "item_key")` 返回指定主键列。其他驱动沿用原生生成 ID 的方式。PostgreSQL 的 `insert_or_ignore` 和 `insert_or_ignore_using` 使用 `ON CONFLICT DO NOTHING`。SQL Server 对这两种忽略冲突写入返回不支持。

PostgreSQL 和 SQLite 上的 `insert_or_ignore_returning` 只返回新插入的行。指定返回列，并可指定要忽略冲突的唯一列。其他约束发生冲突时仍返回错误。

```rust
use openrust::database::{Connection, DatabaseResult, Row, query::Record};

pub async fn register_item(connection: &Connection, code: &str) -> DatabaseResult<Vec<Row>> {
    let item = Record::from([("code".into(), code.into())]);

    connection
        .table("items")
        .insert_or_ignore_returning(&[item], &["item_key", "code"], Some(&["code"]))
        .await
}
```

### Upsert

`upsert` 接收记录、冲突列和需要更新的列。SQLite 使用传入的冲突列。MySQL 通过表上的唯一键判断冲突。

```rust
use openrust::database::{Connection, DatabaseResult, query::Record};

pub async fn save_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let mut transaction = connection.begin_transaction().await?;
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

需要在冲突时同时使用插入值和指定值时，调用 `upsert_with`。`UpsertUpdate::FromInsert` 复制待插入记录里的指定列，`UpsertUpdate::Value` 绑定固定值，`UpsertUpdate::Expression` 接收可信 SQL 和独立绑定。没有冲突的记录按插入值和表默认值写入。

```rust
use openrust::database::{
    Connection, DatabaseResult,
    query::{Record, UpsertUpdate},
};

pub async fn import_stock(connection: &Connection, code: &str, stock: i64) -> DatabaseResult<u64> {
    let row = Record::from([("code".into(), code.into()), ("stock".into(), stock.into())]);
    connection
        .table("items")
        .upsert_with(
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

`update` 返回影响行数。`update_or_insert` 先检查记录是否存在，再执行写入。需要在一条 SQL 内处理插入和冲突时使用 `upsert`。`increment` 和 `decrement` 在 SQL 中运算，并可附带更新其他列。

PostgreSQL 的联结或限额更新和删除通过 `ctid` 子查询选出目标行。

### MySQL 与 MariaDB 的影响行数 {#mysql-affected-rows}

MySQL 和 MariaDB 的更新返回实际修改的行数。把匹配行更新为原值时返回 `0`。upsert 插入新行时返回 `1`，修改重复行时返回 `2`，重复行未变化时返回 `0`。在连接配置中设置 `"found_rows": true` 可改为统计匹配行。

### 更新表达式与标量子查询 {#update-expressions}

使用 `update_with` 混合绑定值、可信 SQL `Expression` 和 `UpdateValue::Subquery` 赋值。通过目标构造器的 `subquery` 创建关联子查询，数据库对每条匹配记录求值，并按自身规则处理空结果或多行结果。MySQL、MariaDB 和 PostgreSQL 的 JSON 路径支持表达式。SQLite 合并补丁接收字面值。

```rust
use std::collections::BTreeMap;

use openrust::database::{
    Connection, DatabaseResult,
    query::{Expression, UpdateValue},
};

pub async fn refresh_credits(connection: &Connection, id: i64) -> DatabaseResult<u64> {
    let accounts = connection.table("accounts as a");
    let credits = accounts
        .subquery("entries as e")
        .select_raw("sum(amount)", vec![])
        .where_column("e.account_id", "=", "a.id")
        .r#where("e.state", "=", "posted");

    accounts
        .r#where("a.id", "=", id)
        .update_with(BTreeMap::from([
            ("credits".into(), UpdateValue::Subquery(&credits)),
            (
                "note".into(),
                Expression::new("coalesce(?, 'pending')")
                    .with_bindings(vec!["refreshed".into()])
                    .into(),
            ),
        ]))
        .await
}
```

### PostgreSQL 联结来源更新 {#postgresql-update-from}

在 PostgreSQL 上调用 `update_from`，根据联结来源更新目标表。PostgreSQL 在 `FROM` 中列出来源，并将联结条件接在 `WHERE` 的查询条件之后。这种写法会忽略排序、限额和偏移。绑定普通值可使用 `Record`。需要混合绑定值和可信 SQL 表达式时使用 `ValueOperand`。

```rust
use openrust::database::{Connection, DatabaseResult, query::Record};

pub async fn verify_paid_orders(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("orders as o")
        .join("payments as p", "o.id", "=", "p.order_id")
        .r#where("p.state", "=", "paid")
        .update_from(Record::from([("status".into(), "verified".into())]))
        .await
}
```

### SQL Server 联结与限额写入 {#sql-server-bounded-writes}

联结更新和删除使用 `FROM` 指定目标表或其别名。无联结时，正数限额使用 `TOP`。有排序时，SQL Server 通过带排序的 `TOP` 子查询确定目标行。按 行为，带联结或正数偏移的写入会忽略限额，限额为零也会省略 `TOP`。

```rust
use std::collections::BTreeMap;
use openrust::database::{Connection, DatabaseResult};

pub async fn activate_latest(connection: &Connection) -> DatabaseResult<u64> {
    connection
        .table("event_archive")
        .r#where("score", "=", 0)
        .order_by("id", "desc")
        .limit(1)
        .update(BTreeMap::from([("score".into(), 1.into())]))
        .await
}
```

### 更新 JSON 列 {#updating-json-columns}

向 `update` 传入形如 `options->preferences->enabled` 的键。MySQL 使用 `JSON_SET`，支持数组下标。SQLite 使用合并补丁，值为 null 的成员会删除对应键。

PostgreSQL 对 `json` 和 `jsonb` 列使用 `jsonb_set`，路径段和新的 JSON 值分别绑定。对象键用箭头分隔，数组索引用方括号，支持负索引。路径段内的点属于键名。同一列的多个路径按记录的键顺序在一个赋值中应用。JSON null 会写入成员。缺失的中间对象保持原状，SQL NULL 文档仍为 SQL NULL。

在 SQL Server 上，向 `update` 传入 JSON 路径会在执行 SQL 前返回不支持操作的错误。仍可传入列名和 `Value::Json` 值来替换整份文档。

```rust
use std::collections::BTreeMap;
use openrust::database::{Connection, DatabaseResult};

pub async fn enable_notifications(connection: &Connection, id: i64) -> DatabaseResult<u64> {
    connection
        .table("users")
        .r#where("id", "=", id)
        .update(BTreeMap::from([
            ("options->preferences->enabled".into(), true.into()),
            ("options->preferences->channel".into(), "email".into()),
        ]))
        .await
}
```

## 删除语句

设置条件后调用 `delete()` 返回影响行数。未设置条件时，删除作用于整张表。`truncate()` 清空表并重置自增主键。MySQL 的该语句会隐式提交，请在事务外运行。

### PostgreSQL 清空表 {#postgresql-truncate}

PostgreSQL 默认使用 `TRUNCATE … RESTART IDENTITY CASCADE`，清空目标表及通过外键引用它的表，并重置这些表拥有的序列。在 PostgreSQL 连接中将 `cascade_on_truncate` 设为 `false` 时，若有其他表引用目标表，PostgreSQL 会拒绝清空。通过事务句柄执行时，回滚会恢复数据及序列。

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn reset_events(connection: &Connection) -> DatabaseResult<()> {
    connection.table("event_archive").truncate().await
}
```

### SQL Server 清空表 {#sql-server-truncate}

SQL Server 执行原生 `TRUNCATE TABLE`，重置标识列起始值。若目标表被其他表的外键引用，数据库会拒绝清空。通过事务句柄执行时，回滚会恢复数据和标识列状态。

## 悲观锁

在事务内使用 `lock_for_update` 或 `shared_lock`。`lock(true)` 和 `lock(false)` 对应相同的模式。加锁查询使用写连接。SQLite 采用自身的事务锁定方式，省略行锁子句。

### 原生锁定子句 {#native-locking-clauses}

可向 `lock` 传入应用定义的 SQL 子句。MySQL 和 PostgreSQL 可用 `FOR UPDATE SKIP LOCKED`，SQL Server 可用 `WITH (UPDLOCK, ROWLOCK, READPAST)`。SQL Server 同时使用 `force_index` 时，自定义锁必须写成 `WITH (...)` 列表。请勿将请求输入拼入子句。

```rust
use openrust::database::{DatabaseResult, Row, Transaction};

pub async fn next_unlocked(
    transaction: &mut Transaction,
    lock_clause: &'static str,
) -> DatabaseResult<Option<Row>> {
    transaction
        .table("job_slots")
        .order_by("id", "asc")
        .lock(lock_clause)
        .first()
        .await
}
```

## 执行回调与 SQL 展开

`before_query` 返回修改后的 Builder，成功应用后清空，读取与写入都调用。失败保留回调，允许下次重试。`after_query` 接收并返回 `Vec<Row>`，空集合表示空结果。`to_raw_sql` 复用驱动的绑定展开，仅用于调试；执行仍使用参数绑定。`to_sql` / `to_raw_sql` 会应用执行前回调，因此需要 `&mut self`；嵌入子查询也需要 `&mut Builder`，防止重复执行回调。旧调用端需声明 `let mut query` 并传入 `&mut query`。

```rust
let mut query = connection.table("users")
    .before_query(|query| Ok(query.r#where("active", "=", true)));
let sql = query.to_raw_sql()?;
let rows = query.after_query(|rows| Ok(rows)).get().await?;
```

## 绑定类别 {#binding-categories}

`get_raw_bindings()` 按顺序返回 `select`、`from`、`join`、`where`、`group_by`、`having`、`order`、`union`、`unionOrder` 九类绑定。`set_bindings(values, kind)` 替换一类，`add_binding(value_or_values, kind)` 追加单值或一组值。类别传 `None` 默认使用 `where`，也可传 `Some("join")`；未知类别返回错误。这些方法通过 `Result` 消费并返回查询构建器。

```rust
let query = connection.table("users")
    .where_raw("id > ? and id < ?", Vec::new())
    .add_binding([1_i64, 10], None)?
    .set_bindings(vec![2.into(), 9.into()], None)?
    .r#where("active", "=", true);
let bindings = query.get_raw_bindings()?;
assert_eq!(bindings["where"], vec![2.into(), 9.into(), true.into()]);
```

手动替换后，新条件仍在已有绑定后追加参数。`select` 清空投影绑定，`reorder` 同时清空普通与联合排序绑定。`merge_bindings(&other)` 按类别追加绑定，不复制 SQL，须确保 SQL 中有对应占位符。所有类型化值始终使用参数绑定，包括包含 SQL 文本的值。`get_bindings()` 返回编译后实际执行的参数；分组限量查询会将排序参数移至窗口投影，并追加绑定的数值限额。`get_raw_bindings()` 则保留原始逻辑类别。

## 补充查询条件 {#additional-query-conditions}

`from_raw(sql, bindings)` 设置可信 SQL 来源。`join_where`、`left_join_where`、`right_join_where` 将连接列与绑定值比较。原始 SQL 由应用代码提供，用户输入通过绑定传入。

`where_binary`、`where_not_binary` 及其 `or` 变体使用 MySQL 二进制比较，其他驱动返回不支持错误。四种 `where_integer_in_raw` / `where_integer_not_in_raw` 变体接收 `Vec<i64>`；正向空集不匹配任何行，反向空集匹配全部行。`where_row_values` 和 `or_where_row_values` 返回 `Result`，列和值数量不一致或运算符非法时立即报错，所有值均通过绑定传入。

`having_null` / `having_not_null` 及其 `or` 变体接收列切片。`having_between` / `having_not_between` 及其 `or` 变体接收两个边界值。`having_nested` 和 `or_having_nested` 的回调接收查询构造器，返回的 HAVING 条件合并为一组；空回调不添加条件。

`in_random_order(None)` 使用驱动的随机函数，`Some(seed)` 为 MySQL 指定种子。`reorder()` 同时清除 union 排序，`reorder_desc(column)` 重新按指定列降序。`for_page_before_id(per_page, last_id, column)` 与 `for_page_after_id` 替换该列的排序并保留其他排序；`last_id` 传 `None` 时只过滤空 ID，不添加 ID 边界。

`raw_value(expression, bindings)` 追加原始投影，取首行首列；没有结果返回 `Value::Null`，既有投影仍优先。PostgreSQL 可将无类型的参数投影推断为文本，因此 `select ?` 可能返回 `Value::Text`；需要指定结果类型时显式使用 SQL 类型转换。

`exists_or` 和 `doesnt_exist_or` 接收返回 `DatabaseResult<T>` 的惰性异步回调。条件成立返回 `query::ExistsOr::Satisfied`，否则执行回调并返回 `Callback(T)`。查询失败不调用回调，回调错误继续向上传播。
