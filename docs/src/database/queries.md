# 查询构造器

使用 `Db::table` 从逻辑表名创建查询；值通过数据库绑定传递，列名由应用代码选择。连接与表名前缀配置见[数据库连接](connections.md)。

## 基本比较

`where` 是 Rust 关键字，默认相等条件使用 `r#where`；三个参数的显式比较使用 `where_compare`：

```rust,ignore
use openrust::database::query::WhereOperator;

let rows = db.table("users")?
    .r#where("active", true)?
    .where_compare("votes", WhereOperator::Ge, 100)?
    .get().await?;
```

`or_where_value` 是默认相等条件的 OR 形式，`or_where_compare` 是显式比较的 OR 形式；四个入口也适用于 `where_nested` 条件组。AND／OR 优先级与下面的布尔组合相同。

普通右值作为绑定传递，字符串不会被解释为列名或 SQL。右值也可显式使用可信 `RawSql` 或已有的标量查询构造器，后者保留原连接、事务与子查询来源约束。动态操作符通过 `parse::<WhereOperator>()` 解析固定列表；未知值明确报错，不执行 PHP 动态猜参。

已有查询放在右侧时，对应 Laravel 的子查询比较：挂接时捕获绑定，父查询编译读取子查询当前 SQL，不主动执行该子查询的 `before_query`。这与左侧标量操作数在接入时准备 SQL 的行为不同；不要依赖右侧回调替你追加绑定。后续修改造成 SQL 占位符与捕获绑定数量不符时明确拒绝执行。

`Option::<i64>::None` 等空值在相等比较中对应 `IS NULL`，在 `NotEq`／`NotEqBang` 中对应 `IS NOT NULL`。`NullSafeEq` 的非空比较由方言分别编译为 MySQL `<=>`、PostgreSQL `IS NOT DISTINCT FROM` 与 SQLite `IS`。不合法的操作符与 NULL 组合会在构造条件时拒绝。

基础比较与以下多列入口不代表全部 PHP `where` 重载已完成；普通数组条件、Eloquent／Relation 输入仍有后续对应范围。

## 多列条件与查询闭包

用 `MultiComparison::eq` 指定相同的等值条件，或用 `compare` 指定操作符：

```rust,ignore
use openrust::database::query::{MultiComparison, WhereOperator};

let rows = db.table("users")?
    .where_any(["name", "email"], MultiComparison::compare(WhereOperator::Like, "%Taylor%"))?
    .get().await?;
```

| 方法 | 组内条件 | 外层连接 |
| --- | --- | --- |
| `where_any`／`or_where_any` | OR | AND／OR |
| `where_all`／`or_where_all` | AND | AND／OR |
| `where_none`／`or_where_none` | NOT（OR） | AND／OR |

六个入口也可用于条件组。None 保留 SQL 三值逻辑：含 NULL 的结果可能仍为 UNKNOWN，不是逐列不等于。重复列保留原顺序与重复绑定，空列不增加条件或执行回调；参数准备阶段已非法的操作符与 NULL 组合仍会先报错。

Rust 没有 PHP 的可变实参与动态操作符猜参，`MultiComparison::Omitted` 明确表示省略操作符和值，`eq(None::<i64>)` 表示明确的 SQL NULL 等值。两者对列闭包有不同含义：

```rust,ignore
use openrust::database::query::{MultiColumn, MultiComparison};

let columns = [
    MultiColumn::callback(|query| query.r#where("name", "Taylor")),
    MultiColumn::from("email"),
];
let rows = db.table("users")?
    .where_all(columns, MultiComparison::Omitted)?
    .get().await?;
```

省略模式把列闭包作为继承外层 FROM 的嵌套条件；明确比较模式把同一个闭包作为没有 FROM 的新标量子查询，调用方按需通过 `from` 指定表。字符串、可信 `RawSql`、已有查询与闭包可以显式转换为 `MultiColumn` 后混排。列闭包按声明顺序调用一次；`MultiValue::callback` 作为右值时，每列重新配置一个同连接、同实际事务的空子查询，不提前共享一次性闭包结果。

构造失败不会接入半个条件组，用户回调自身的外部副作用不回滚。每份右侧子查询分别核对实时 SQL 与挂接绑定的数量，两份查询一增一减也不能抵消；跨数据库、事务与失效嵌套来源不会因包装成组而恢复有效。

## 空查询与原始投影

`Db::query()` 创建真正没有 FROM 的查询；`from` 使用与 `Db::table` 相同的逻辑表名映射。没有 FROM 的写操作明确拒绝。使用 `select_raw` 表达可信 SQL 投影，并把值单独绑定：

```rust,ignore
use openrust::database::query::RawSql;

let rows = db.query()
    .select_raw(RawSql::new("? AS amount"), vec![7_i64.into()])?
    .get().await?;
```

`select_raw` 和 `where_raw` 的绑定模板统一使用 `?`，按出现顺序取值。SQL 字符串、引用标识符和普通注释内的问号不消耗绑定。PostgreSQL 的 JSON 问号操作符用 `??`，其他后端在执行代码区拒绝 `??`。原生编号或命名参数不能混入这条模板。`select_raw` 追加显式投影，第一次显式投影替换缺省 `*`。普通 `select` 替换已选投影，计数和分页不会丢失原投影绑定。

PostgreSQL 普通字符串的单引号前出现奇数个连续反斜线时，查询返回 `query_raw_sql_mode_ambiguous`。可使用值绑定、美元引用或显式 `E` 字符串。显式 `E` 字符串以换行衔接时继续使用原转义规则。MySQL 对字符串中的反斜线也要求使用绑定，避免会话转义模式改变边界。带版本门控的 MySQL 执行注释中无法绑定参数。当前仅编译词法边界可确定的模板。

### JSON 路径比较

MySQL、PostgreSQL 与 SQLite 的 JSON 列可以使用箭头路径；字符串值仍作为绑定，布尔值按目标方言的 JSON 比较规则处理：

```rust,ignore
let rows = db.table("users")?
    .r#where("preferences->dining->meal", "salad")?
    .r#where("options->enabled", true)?
    .get().await?;
```

数组路径保留目标方言对键和索引的区别，不能把所有数字外观的键都当作数组索引。MySQL 的 JSON NULL／缺失路径使用提取值与 JSON 类型的联合判定。普通路径由框架引用和转义；`RawSql` 仍只用于应用选择的可信表达式。

## 原始 WHERE 条件

`where_raw` 用于应用审查过的固定 SQL 片段。将外部值放进绑定列表，并保留账户等普通条件：

```rust,ignore
use openrust::database::query::RawSql;

let query = db.table("records")?
    .where_eq("owner_id", owner_id)?
    .where_raw(RawSql::new("priority >= ? AND priority < ?"), vec![min.into(), max.into()])
    .where_eq("status", "visible")?;
let rows = query.get().await?;
```

这里的 WHERE 绑定顺序为账户、下限、上限和状态。相同条件也用于计数、更新和删除。参数缺少或多余时，读取绑定、编译和执行都会报告 `query_raw_bindings_invalid`。

## JSON 包含

`where_json_contains` 接受列名、限定列或箭头路径，查找值使用 `serde_json::Value` 绑定。MySQL 把路径传给 `JSON_CONTAINS`，PostgreSQL 取出 JSON 节点并转换为 `jsonb` 后检查包含，SQLite 把路径传给 `json_each`。在查询和 `where_nested` 分组内都可用 `or_where_json_contains`、`where_json_doesnt_contain` 和 `or_where_json_doesnt_contain`。否定只作用于当前谓词。MySQL 与 PostgreSQL 中，缺失路径或 SQL `NULL` 得到未知值，否定仍不匹配；SQLite 中不存在可迭代元素时，否定的 `exists` 匹配。

```rust
use openrust::{Db, Result};

pub async fn count_tagged(db: &Db, owner_id: i64) -> Result<u64> {
    db.table("records")?
        .where_eq("owner_id", owner_id)?
        .where_nested(|group| {
            group
                .where_json_contains("records.metadata->tags", serde_json::json!("typed"))?
                .or_where_json_contains("records.metadata->labels", serde_json::json!("typed"))
        })?
        .count()
        .await
}
```

SQLite 按固定 Laravel 的 `json_each` 和 `IS` 比较处理标量。整数直接绑定，布尔值绑定为 0 或 1，浮点及超出有符号 64 位范围的整数按固定 PDO 精度转换为文本。JSON `null` 可匹配文档中的 JSON 空值，SQL `NULL` 列没有可匹配项。SQLite 拒绝数组和对象查找值，确保相邻条件各自保留自己的绑定。

SQLite JSON 查询的固定 Laravel 文档基线为 3.39.0+。连接器的总体最低目标仍为 3.26.0，旧版执行本查询需要引擎提供 JSON1。列名 `value` 或 `key` 与 `json_each` 同名时，使用 `records.value` 一类的限定列。路径选择器使用箭头和数组下标，沿用各数据库的 JSON 路径语法。 [固定 SQLite Grammar](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Database/Query/Grammars/SQLiteGrammar.php#L213-L229) · [固定 JSON 查询文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md#json-where-clauses)

唯一示例的记录列表用 `metadata_contains` 传入 URL 编码的 JSON 文本。MySQL 和 PostgreSQL 可查找 `{"tags":["typed"]}`，SQLite 可查找标量 `"typed"`。账户条件与包含条件始终组合，分页总数使用相同筛选。缺省和归一化后的空文本不筛选，非法 JSON、超过 4096 字符的文本或 SQLite 的结构化查找值返回字段验证错误。

同一列表接受 `metadata_tag=typed`，以普通字符串查找 `metadata.tags` 或 `metadata.labels`。两个路径放在同一个 OR 分组中，账户条件作用于整个分组。

### JSON 路径存在

`where_json_contains_key` 检查路径是否存在。值为 JSON `null` 的成员仍然匹配。缺失路径和 SQL `NULL` 由 `where_json_doesnt_contain_key` 匹配。查询与 `where_nested` 分组均支持这两个方法及其 `or_where_` 变体。它们不增加参数绑定，计数、更新与删除使用相同条件。 [固定 Laravel Builder](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Database/Query/Builder.php#L2442-L2482)

```rust
use openrust::{Db, Result};

pub async fn count_with_tags(db: &Db, owner: i64) -> Result<u64> {
    db.table("records")?
        .where_eq("owner_id", owner)?
        .where_json_contains_key("records.metadata->tags")?
        .count()
        .await
}
```

使用应用声明的路径选择器，并保留数据库方言的数组和文本键规则。MySQL 和 PostgreSQL 要求显式箭头路径，避免执行上游对裸列生成的无效 SQL。SQLite 的裸列形式检查整个 JSON 值。PostgreSQL 文本末段沿用 `jsonb ?`，可匹配对象键、数组字符串元素或同值 JSON 字符串。整数末段支持负下标，计算长度前先检查数组类型。唯一示例可用 `metadata_key=tags` 或 `metadata_key=labels` 筛选，入口仅接受预设键。

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
use openrust::database::query::{ColumnOperand, RawSql};

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
    .order_by("id", openrust::database::SortDirection::Desc)?
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
use openrust::database::query::ExistsOrResult;

let result = query.exists_or(|| async { Ok("没有待处理订单") }).await?;
match result {
    ExistsOrResult::True => { /* 订单存在 */ }
    ExistsOrResult::Callback(message) => { /* 使用回调结果 */ }
}
```

枚举保留 PHP 的 `true | mixed` 两种结果；回调返回 `false`、空值或其他类型时仍是 `Callback`，不冒充短路结果。闭包可消费拥有的捕获值，数据库和闭包的错误继续返回错误。异步闭包是 Rust 数据库流程的语法适配。

before-query 的执行与克隆规则见下一节，存在性条件回调仍只决定自己的惰性分支。

## 执行前修改查询

`before_query` 顺序登记同步修改回调，回调返回 `Result<()>`。回调可以借用局部数据；构造器保存的事务必须覆盖这些捕获的存活期。需要绑定局部事务时，先在该事务作用域中创建或绑定构造器，再登记回调。

```rust,ignore
let minimum_id = 10_i64;
let query = db.table("orders")?.before_query(|query| {
    *query = query.clone().where_gt("id", minimum_id)?;
    Ok(())
});
let sql = query.to_sql()?;
let bindings = query.get_bindings()?;
let rows = query.get().await?;
```

fluent 方法按 Rust 所有权返回新的值，因此在回调内显式构造并赋回。修改会写回同一原查询对象；成功后消费本次回调列表，但保留条件和绑定。SQL 预览 `to_sql` 也会执行并消费回调，`get_bindings` 只读取当前绑定。上例后续 `get` 复用已经修改的查询。

回调返回错误时，不执行后续回调或数据库操作，已发生的修改和登记列表保留；重试会再次执行原列表。回调全部成功而数据库失败时，列表已经消费。`get` 与 `pluck` 成功后恢复调用前的投影，错误或取消则保留当时投影；`first` 与 `value` 会在原查询保留 `limit(1)`。

普通 `clone()` 复制父查询状态和回调列表，克隆与原查询各自消费。`count` 在聚合克隆上执行；分页总数为零时只执行计数克隆，非零时再准备原查询获取当前页。`simple_paginate` 只准备原查询一次。`chunk_by_id` 在每个实际读取的块上准备克隆，遵循总 limit 与第一块 offset，limit 为零时不查询。

子查询各有对应的准备时机：

| 入口 | 子查询自身回调 |
| --- | --- |
| 标量列操作数 | 接入时执行并冻结 |
| `where_exists` | 外层编译不执行子查询自身回调 |
| `union` | 外层 SQL 编译时执行 |

`where_exists` 与 `union` 保留同一子查询对象，父查询克隆也共享这个子节点。父查询在接入时保存绑定；后来子查询回调改动绑定不会自动修复父查询的绑定计划，数量不匹配明确拒绝。需要相应绑定时，在接入父查询前先准备子查询。`using_transaction` 为新的事务借用返回独立句柄，已引用旧子节点的父查询保持原上下文。

取消会释放构造器执行租约，但不保证驱动在任意网络协议阶段都可复用。MySQL 冷预处理未完成时，驱动补丁将该物理连接判为不可复用：后续查询、提交和显式回滚会返回错误；销毁或归还事务后，池淘汰连接并补建。取消瞬间不保证服务器已回滚，继续持有废事务仍可能持锁。预热语句和服务器已确认行锁等待的取消保留原有恢复路径。

根项目和 CLI 生成应用自动登记该临时驱动补丁；Cargo 只读取最终 workspace 根的 `[patch.crates-io]`，手写独立应用须在自己的根清单登记 `sqlx-mysql = { path = "/path/to/openrust/vendor/launchbadge/sqlx-mysql" }`。SQLx 暂固定为 0.9.0；升级前须确认对应上游修复及补丁实际解析结果。

单个查询在准备与 I/O 期间有执行租约，并发或递归准备明确报忙；锁不会跨用户回调或异步等待。惰性存在性回退在查询租约释放后运行，仍可只读借用同一构造器。共享子查询图拒绝自引用和环，允许重复引用同一个无环子节点。当前回调不能用整对象替换切换 Db、物理表或实际事务；完整 FROM/跨连接查询 API 仍有未实现部分。

## 唯一示例

认证用户可读取 `GET /api/example/article-imports/timeline`。它使用既有文章导入记录，比较 `terminal_at_ms`、该行 `created_at_ms` 与服务器观察时点，分别统计正常和异常终结时间；未终结记录不计入，另一账户的数据不可见。显式 JWT profile 使用其具名 Guard，普通 JWT 路径保留对应认证方式。

可选 `search` 在导入标识与来源名称上做包含式 LIKE 检索，`mode=any/all/none` 选择同一括号组的组合方式；检索值作为绑定，`%`、`_` 保留 LIKE 通配符含义。缺省或空检索不改变原范围，非空检索最多128字节；账户范围始终约束整个检索组。

时间线通过执行前回调追加当前账户条件，普通查询克隆各自消费该范围；`exists_or` 返回是否有 open 导入，另一账户的在途记录不影响标记。

本章只描述当前公开子范围，不代表完整 Laravel Query Builder、Model 或 Schema 已完成。固定对应来源为 Laravel `Builder::whereBetweenColumns` 与 Query Grammar，Rustdoc 提供类型与完整签名。

## 插入并取得主键

```rust,ignore
let id = db.table("orders")?
    .insert_get_id(vec![("title", "first order".into())], None).await?;
```

`insert_get_id` 允许空值列表，插入一行数据库默认值。MySQL/SQLite从本次执行结果取自增键，SQLite保留负rowid；PostgreSQL使用同次`RETURNING`，第二个参数可选择返回列，`None`、空字符串或`"0"`均选择`id`。其他两驱动不使用该参数。它执行原查询的`before_query`回调，写入主连接，或使用`using_transaction`借用的原事务。

返回类型为`sea_orm::Value`，按固定Laravel Processor的PHP64位规则处理：数值字符串（例如`"000123"`）返回整数123，非数值文本保留；MySQL超有符号范围的原生ID按PDO字符串的规则封顶。不要把这个返回转换当成模型原始主键读取的规则。按固定PHP8.5默认框架错误处理，越界及非有限浮点转换返回错误；INSERT此时可能已经完成，需要原子失败语义时显式使用事务。PostgreSQL BYTEA保留Rust字节值，不能声称模拟PHP资源句柄。

实现依赖仓内固定SeaORM安全结果访问器；手写独立consumer需继承根`[patch.crates-io] sea-orm`路径，生成项目会自动写入规范绝对路径。普通`last_insert_id`旧访问器未改，不能用它替代新路径。当前SQL列参数仍遵守Query的标识符限制，完整Eloquent、全部SQL方言和发布分发边界按支持范围验收。

## SQLite 写入目标

带 `join` 或显式 `limit`（包括零）的 `update` / `delete`，按原查询的 `rowid` 子查询选择目标；排序、偏移、条件和 UNION 分支保留，各 UNION 子枝的分页只作用于该枝。根查询的读取投影不参与写入目标。 联表写入只有 OFFSET 时会传播 SQLite 的语法错误。需要偏移时同时指定 LIMIT。

以下写入选择该账户中 ID 最小的一条待处理记录。只传 `order_by` 会保留排序条件的直接写入语义，因此限写一行时需要显式设置 `limit`。

```rust,ignore
use openrust::database::SortDirection;

let updated = db.table("orders")?
    .where_eq("account_id", account_id)?
    .where_eq("state", "pending")?
    .order_by("id", SortDirection::Asc)?
    .limit(1)
    .update(vec![("state", "processing".into())]).await?;
```

仅排序或偏移、不带联表或 limit 时，沿固定官方的直接写入路径，不把读取分页扩展为写入限制。直接路径遇到未使用的非 select 业务绑定，返回 `query_sqlite_unused_write_bindings`；这是为阻止 SQLx 忽略多余参数而在执行前进行的 Rust 驱动适配。

用户定义的 `rowid` 列可能遮蔽 SQLite 隐藏行号，多个同值行仍可能被写入；不保证物理唯一行。`WITHOUT ROWID` 表缺少该名称时传播数据库错误，不回退主键。这些写目标行为已编码，当前及最低 SQLite 版本的运行验收仍属发布前待验范围。

## SQLite 共享租约与业务事务

`SharedLocks` 是框架增强的数据库租约，不等同于 Laravel 官方 Cache Lock。启用 `db-sqlite` 并迁移 `CoordinationMigration` 后，可用于同机 SQLite 文件的任务协调；调用方须显式续约，未到期的竞争返回 `Busy`，到期接管会增加 fencing generation。

保护业务写入时，在同一个 `Db::transaction` 中调用 `assert_current_in_transaction`，并通过 `QueryBuilder::using_transaction` 执行业务写入。SQLite 校验采用带 owner、fencing 和期限条件的写操作，持有写锁直到事务提交或回滚；普通 SELECT 不能替代该校验。旧 owner、错误 fencing 或过期租约均拒绝保护写入。

BUSY、旧读快照无法升级或执行错误会向调用方传播，不悄悄重启事务。业务失败回滚数据，但持久租约仍按原 TTL 归属，由调用方释放或等待到期；取消不保证立即转移租约。此能力不承诺 SQLite 多机共享，也不表示 SQLite 请求幂等、数据库队列或调度器的完整链路已经实现。
