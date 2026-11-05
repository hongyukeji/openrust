# 查询构造器

使用 `Db::table` 从逻辑表名创建查询；值通过数据库绑定传递，列名由应用代码选择。连接与表名前缀配置见[数据库连接](connections.md)。

## 值区间与列区间

`where_between` 把上下界作为值绑定。每行具有自己的上下界时，使用 `where_between_columns`：

```rust,ignore
let rows = db.table("products")?
    .where_between_columns("weight", ["minimum_weight", "maximum_weight"])?
    .get().await?;
```

| 方法 | 连接方式 | 比较 |
| --- | --- | --- |
| `where_between_columns` | AND | BETWEEN |
| `where_not_between_columns` | AND | NOT BETWEEN |
| `or_where_between_columns` | OR | BETWEEN |
| `or_where_not_between_columns` | OR | NOT BETWEEN |

四个方法也可在 `where_nested` 的条件组中使用。BETWEEN 包含两端；NULL 沿数据库三值逻辑处理，不自动变为零。框架不会交换反向上下界。

上下界使用固定两个元素的 Rust 数组，不能像 PHP 数组一样传入任意数量。普通列名不增加值绑定，限定列名逐段引用；逻辑表名复用当前 Db 的前缀与显式映射。

## 表达式与标量子查询

`ColumnOperand` 接受列名、显式可信的 `RawSql` 和查询构造器。混合操作数时先转换类型：

```rust,ignore
use openrust::db::query::{ColumnOperand, RawSql};

let rows = db.table("products")?
    .where_between_columns("weight", [
        ColumnOperand::from("minimum_weight"),
        ColumnOperand::from(RawSql::new("100")),
    ])?
    .get().await?;
```

`RawSql` 原样进入 SQL，只用于应用控制的表达式；不能拼接客户端输入，也不提供额外的占位符模板接口。

```rust,ignore
let latest = db.table("measurements")?
    .select(&["weight"])?
    .where_eq("product_id", 42)?
    .order_by("id", openrust::db::SortDirection::Desc)?
    .limit(1);
let rows = db.table("products")?
    .where_between_columns(&latest, ["minimum_weight", "maximum_weight"])?
    .get().await?;
```

标量子查询须自行选择单列，并保证结果适合数据库的标量子查询规则。其已有绑定保留原顺序，不把列名转换成值。

子查询须使用同一 Db 句柄和同一事务引用；事务应来自该 Db 的事务回调。附着子查询后不能再改绑事务，包括改绑后又换回。嵌套组须来自当前闭包对应的 Db、表和事务；不能把另一查询捕获的条件组交回来。来源不一致明确报错，不静默在另一个库解释同名表。跨连接子查询与完整 FROM/别名能力仍有未实现部分。

## 布尔组合

AND 优先于 OR，只有显式 `where_nested` 才固定括号：

```rust,ignore
let query = db.table("products")?
    .where_eq("featured", true)?
    .or_where_between_columns("weight", ["minimum_weight", "maximum_weight"])?
    .where_eq("active", true)?;
```

这表示 `featured OR (weight BETWEEN minimum_weight AND maximum_weight AND active)`。要让 `active` 约束整个 OR，先把 OR 放入 `where_nested`，再添加 `where_eq("active", true)`。首个 OR 不加入恒真条件；空组不增加过滤。读取、计数、更新和删除复用同一条件链。

## 判断查询是否存在结果

只需判断存在性时使用 `exists` 或 `doesnt_exist`；两者借用查询，不消费构造器：

```rust,ignore
let query = db.table("orders")?.where_eq("status", "pending")?;
let has_pending = query.exists().await?;
let has_no_pending = query.doesnt_exist().await?;
```

它们以完整原查询生成 `SELECT EXISTS(...)`，保留投影、条件、排序、LIMIT/OFFSET、UNION 与绑定；投影值为 NULL 不表示没有行。使用显式事务时读取该事务，否则遵循读副本与 `with_primary` 作用域。数据库和来源校验错误继续返回错误，不解释为“没有结果”。完整 before-query callbacks、callback variants 和未实现后端仍有缺口。

## 按存在性选择回调

`exists_or` 在查询存在行时直接返回 `ExistsOrResult::True`，没有行时才调用异步闭包；`doesnt_exist_or` 在没有行时直接返回 `True`，有行时才调用闭包。每次只执行一次存在性查询，短路分支连闭包的 Future 都不创建。

```rust,ignore
use openrust::db::query::ExistsOrResult;

let result = query.exists_or(|| async { Ok("没有待处理订单") }).await?;
match result {
    ExistsOrResult::True => { /* 订单存在 */ }
    ExistsOrResult::Callback(message) => { /* 使用回调结果 */ }
}
```

枚举保留 PHP 的 `true | mixed` 两种结果；回调返回 `false`、空值或其他类型时仍是 `Callback`，不冒充短路结果。闭包可消费拥有的捕获值，数据库和闭包的错误继续返回错误。异步闭包是 Rust 数据库流程的语法适配。

完整 before-query 注册、预览消费、聚合克隆和共享子查询回调生命周期仍在实现，不能将以上条件入口当成该机制已经支持。

## 唯一示例

认证用户可读取 `GET /api/example/article-imports/timeline`。它使用既有文章导入记录，比较 `terminal_at_ms`、该行 `created_at_ms` 与服务器观察时点，分别统计正常和异常终结时间；未终结记录不计入，另一账户的数据不可见。显式 JWT profile 使用其具名 Guard，普通 JWT 路径保留对应认证方式。

时间线还通过 `exists` 返回当前账户是否有 open 导入；另一账户的在途记录不影响该标记。

本章只描述当前公开子范围，不代表完整 Laravel Query Builder、Model 或 Schema 已完成。固定对应来源为 Laravel `Builder::whereBetweenColumns` 与 Query Grammar，Rustdoc 提供类型与完整签名。
