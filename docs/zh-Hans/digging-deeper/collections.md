# 集合

## 创建集合

用 `Collection::make` 创建顺序集合，用 `Collection::from_entries` 指定键。集合保留迭代顺序。整数键与 `"3"` 这样的规范十进制字符串对应同一个条目。`"03"` 这样的字符串保留原样。

## 可用方法

`map`、`filter` 和 `sort_by` 保留原键。调用 `values()` 可将键重排为从零开始的连续整数。用 `group_by` 分组，第二个参数传 `true` 可在组内保留原键。

```rust
use openrust::support::Collection;

fn main() {
    let amounts = Collection::from_entries([(0, 1), (3, 2), (7, 3)]);
    let mapped = amounts.map(|value, _| value * 10);
    let selected = mapped.filter(|value, _| *value >= 20);

    assert_eq!(selected.to_json().unwrap(), r#"{"3":20,"7":30}"#);
    assert_eq!(selected.values().to_json().unwrap(), "[20,30]");

    let groups = Collection::make(["apple", "apricot", "pear"])
        .group_by(|word, _| word.starts_with('a'), false);
    assert_eq!(groups.to_json().unwrap(), r#"{"1":["apple","apricot"],"0":["pear"]}"#);
}
```

回调接收有类型的 Rust 值，可直接访问字段。`sort_by` 使用回调返回值的类型排序，相等时保留输入顺序。`avg` 和 `median` 返回 `Option<f64>`。

## 保键比较与位置操作 {#collection-key-operations}

`diff`、`intersect` 及其关联键变体和 `duplicates` 保留返回项的原键。`shift` 和 `splice` 重新编号被修改集合的整数键。`split` 平衡各组长度，`split_in` 优先填满固定长度的组并保留键。`chunk_by` 按相邻值分组。

```rust
use openrust::support::Collection;

let mut rows = Collection::from_entries([("4", "甲"), ("keep", "乙"), ("9", "丙")]);
assert_eq!(rows.shift(), Some("甲"));
assert_eq!(rows.to_json()?, r#"{"keep":"乙","0":"丙"}"#);
let existing = Collection::make(["乙"]);
let remaining = rows.diff(&existing).to_array()?;
assert_eq!(remaining, serde_json::json!(["丙"]));
```

## 类型化变换与嵌套 JSON {#collection-projections}

用类型化投影完成字段比较、分组和运行时类型筛选。`map_to_groups` 返回分组集合，`map_to_dictionary` 返回分组向量。`Collection<Value>` 支持点路径、递归合并及替换。`random` 无放回抽样，可保留键。

集合扩展使用 Rust 扩展 trait，高阶消息对应直接回调，诊断可用 `dbg!(&collection.all())`。`lazy()` 的迭代器适配器提供缓存和基于时间的遍历。

```rust
use openrust::support::Collection;
use serde_json::json;

let first = Collection::from_value(json!({"team":{"tags":["甲"]}}));
let second = Collection::from_value(json!({"team":{"tags":["乙"]}}));
let merged = first.merge_recursive(second)?.dot_with_depth(Some(1))?.to_array()?;
assert_eq!(merged, json!({"team.tags":["甲","乙"]}));
```

## JSON 序列化

当键在迭代顺序中是从零开始的连续整数时，集合序列化为 JSON 数组。其他键生成 JSON 对象。`to_json()` 返回 JSON 字符串或序列化错误，非有限浮点数也会报错。集合可以直接传给 `ApiResponse::success`。直接反序列化到 `Collection<T>` 可保留 JSON 对象成员顺序。`from_value` 使用传入的 `serde_json::Value` 的顺序。

## 惰性集合

数据库行可通过查询构建器的 [cursor](../database/queries#streaming-results) 异步读取。该入口支持提前停止读取。

### 简介

`LazyCollection` 按需从 Rust 迭代器读取条目，适合逐步处理大数据源。`take(2)` 只读取生成两个结果所需的条目。负数限制会遍历到末尾，仅保留指定大小的尾部。

### 创建惰性集合

将迭代器工厂传给 `make`，带键来源使用 `from_entries`。可能失败的来源使用 `try_make` 或 `try_from_entries`。普通遍历每次创建自己的迭代器，并在第一次错误时终止。组合后的迭代器在首次取值时才打开来源。可通过 `get_iterator()` 或遍历 `&collection` 逐项读取结果。

```rust
use std::{cell::Cell, io::{BufRead, BufReader}};

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let reads = Cell::new(0);
    let lines = LazyCollection::try_make(|| {
        BufReader::new(&b"INFO ready\nWARN slow\nWARN retry\n"[..])
            .lines()
            .inspect(|_| reads.set(reads.get() + 1))
            .map(|line| line.map_err(|error| CollectionError::Source(Box::new(error))))
    })
    .filter(|line, _| line.starts_with("WARN"))
    .values()
    .remember();

    assert_eq!(reads.get(), 0);
    assert_eq!(lines.clone().take(1).collect()?.to_json()?, r#"["WARN slow"]"#);
    assert_eq!(reads.get(), 2);
    assert_eq!(lines.collect()?.to_json()?, r#"["WARN slow","WARN retry"]"#);
    assert_eq!(reads.get(), 3);
    Ok(())
}
```

### Enumerable 契约

`Collection` 和 `LazyCollection` 都实现 `Enumerable`，提供 `map`、`filter`、`values`、`take`、`all` 和 `collect`。`all` 或 `collect` 收集遍历结果时，同一个键保留最后一个值。需要保留每次出现的条目时，先调用 `values`。`Collection::lazy()` 可将已收集的集合用作来源。

### 惰性集合方法

`tap_each` 在读取条目时执行副作用。

`chunk` 每次生成一个已收集的 `Collection`，可选择保留组内键。惰性集合在拥有它的线程运行，可以借用线程内的局部状态。

#### 截止时间与超时回调 {#lazy-deadline}

给 `take_until_timeout` 传入单调时钟的 `Instant`。遍历开始时已到截止时间，来源不会打开。已经开始的读取可以在截止后产出一项，下一次取值直接结束遍历。

```rust
use std::{
    cell::RefCell,
    thread,
    time::{Duration, Instant},
};

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let deadline = Instant::now() + Duration::from_millis(250);
    let notified = RefCell::new(None);
    let source = LazyCollection::from_entries(|| {
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        [("first", 1), ("later", 2)]
    })
    .take_until_timeout_with(deadline, |entry| {
        *notified.borrow_mut() = entry.map(|(value, key)| (*value, key.clone()));
    });

    let mut items = source.get_iterator();
    assert_eq!(items.next().transpose()?, Some(("first".into(), 1)));
    assert_eq!(*notified.borrow(), None);
    assert!(items.next().is_none());
    assert_eq!(*notified.borrow(), Some((1, "first".into())));
    Ok(())
}
```

使用 `take_until_timeout_with` 时，开始即超时会用 `None` 调用回调。遍历中达到截止时间时，下一次取值会把上一项的值和键传给回调。来源正常耗尽或读取失败时仍返回各自的结果。

#### 节流与心跳 {#lazy-pacing}

`throttle` 在继续迭代时让调用线程等待，从间隔中扣除处理上一项已花费的时间。读取到耗尽会在最后一项之后再推进一次迭代器。通过 `take` 提前停止时，最后一次等待尚未开始。

`with_heartbeat` 在成功读取后检查经过的时间，并在产出该项前执行回调。每次心跳后重新计时，每轮遍历也独立计时。间隔为零时，每个成功读取的条目都会触发回调。

```rust
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let beats = Cell::new(0);
    let interval = Duration::from_millis(2);
    let jobs = LazyCollection::make(|| ["import", "index"])
        .with_heartbeat(Duration::ZERO, || beats.set(beats.get() + 1))
        .throttle(interval);

    assert_eq!(beats.get(), 0);
    let started = Instant::now();
    assert_eq!(jobs.count()?, 2);
    assert!(started.elapsed() >= interval * 2);
    assert_eq!(beats.get(), 2);
    Ok(())
}
```

#### 分组和滑动窗口 {#lazy-grouping-and-sliding}

`chunk_while` 对连续条目分组。回调接收下一个值、它的键和当前组。返回 false 时，该条目成为新组的首项。`sliding(size, step)` 只产出完整窗口，窗口大小和步长都须为正数。分组和窗口保留原键，每组以实体化的 `Collection` 返回。滑动窗口要求值实现 `Clone`，让各窗口独立拥有值。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::make(|| [4, 4, 8, 8, 12]);
    let groups = readings
        .clone()
        .chunk_while(|reading, _, group| group.last() == Some(reading));
    assert_eq!(groups.count()?, 3);

    let windows = readings.sliding(2, 2)?;
    assert_eq!(windows.count()?, 2);
    assert_eq!(windows.first()?.unwrap().to_json()?, "[4,4]");
    Ok(())
}
```

#### 选取和追加条目 {#lazy-selection-and-append}

`slice(offset, length)` 保留原键。偏移或长度为负数时，先实体化来源并应用集合的键规则。`nth(step, offset)` 从偏移位置开始每隔指定步长选取一项，并重新编号。`keys()` 按读取顺序产出键，包括重复键。`concat` 待读完第一个惰性集合再打开第二个，并统一重排索引。`chunk_by` 按回调返回的类型化值对连续条目分组。

```rust
use openrust::support::{ArrayKey, CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("a", 4), ("b", 7), ("c", 11)]);
    assert_eq!(
        readings.clone().slice(1, Some(1)).all()?,
        vec![(ArrayKey::from("b"), 7)]
    );
    assert_eq!(readings.clone().keys().count()?, 3);
    assert_eq!(readings.clone().chunk_by(|value, _| value / 10).count()?, 2);

    let combined = readings.nth(2, 0)?.concat(LazyCollection::make(|| [15]));
    assert_eq!(combined.collect()?.to_json()?, "[4,11,15]");
    Ok(())
}
```

#### 展开嵌套条目 {#lazy-expanding-nested-items}

`collapse` 展开一层嵌套的惰性集合、实体集合、向量或数组，并重新编号。`collapse_with_keys` 保留内部键，实体化时同键的最后一个值生效。`flat_map` 将每个条目映射成一组，再展开。对于 JSON 值，`flatten(Some(depth))` 按指定深度展开数组和对象并保留标量。用 `None` 展开全部层级。

```rust
use openrust::support::{Collection, CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let groups = LazyCollection::make(|| {
        [
            Collection::from_entries([("first", 1)]),
            Collection::from_entries([("second", 2)]),
        ]
    });
    assert_eq!(
        groups.collapse_with_keys().collect()?.to_json()?,
        r#"{"first":1,"second":2}"#
    );

    let doubled = LazyCollection::make(|| [1, 3]).flat_map(|value, _| [*value, value * 2]);
    assert_eq!(doubled.collect()?.to_json()?, "[1,2,3,6]");

    let nested = LazyCollection::make(|| [json!([1, [2]]), json!(3)]);
    assert_eq!(nested.flatten(None).collect()?.to_json()?, "[1,2,3]");
    Ok(())
}
```

#### 配对多个来源 {#lazy-pairing-sources}

`zip` 按位置配对值。用 `zip_many` 将第一个来源与多个后续来源配对。两者都读取到最长来源结束，缺项用 `None` 表示。`combine` 将第一个来源的值用作键，与第二个来源的值配对。遍历发现长度不同时返回 `CollectionError::InvalidArgument`。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let numbers = LazyCollection::from([1, 2]);
    let rows = numbers.clone().zip(["one"]);
    assert_eq!(rows.collect()?.to_json()?, r#"[[1,"one"],[2,null]]"#);

    let labels = LazyCollection::from(["first", "second"]);
    assert_eq!(
        labels.combine(numbers).collect()?.to_json()?,
        r#"{"first":1,"second":2}"#
    );
    Ok(())
}
```

#### 去重和查找匹配项 {#lazy-deduplicating-and-finding}

`unique` 和 `unique_strict` 使用类型化相等比较。`unique_by` 按回调返回的值去重。这些方法保留第一次出现的条目及其原键。`contains` 和 `first_or_fail` 在首次匹配时停止。`has_sole` 和 `has_many` 最多读取两条匹配。`sole` 先按键实体化前两条匹配，再检查是否只剩一个值。用带 `Where` 的方法传入谓词。得出结果前遇到的来源错误会原样返回。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("a", 10), ("b", 11), ("c", 21)]);
    let distinct = readings.clone().unique_by(|value, _| value / 10);
    assert_eq!(distinct.collect()?.to_json()?, r#"{"a":10,"c":21}"#);
    assert!(readings.contains(&11)?);
    assert!(readings.has_sole_where(|value, _| *value > 20)?);
    assert_eq!(readings.first_or_fail_where(|value, _| *value > 10)?, 11);
    Ok(())
}
```

#### 排序和查找重复项 {#lazy-sorting-and-duplicates}

`sort`、`sort_by`、`sort_keys` 及其降序版本、`reverse` 和 `duplicates` 都会在首次取值时实体化来源。同一来源键重复出现时使用最后一个值。排序和反转保留键。`duplicates_by` 返回重复的投影值，并保留它们的原键。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("a", 12), ("b", 7), ("c", 12)]);
    let sorted = readings.clone().sort();
    assert_eq!(sorted.collect()?.to_json()?, r#"{"b":7,"a":12,"c":12}"#);
    let duplicates = readings.duplicates_by(|value, _| value / 10);
    assert_eq!(duplicates.collect()?.to_json()?, r#"{"c":1}"#);
    Ok(())
}
```

#### 聚合值 {#lazy-aggregating-values}

`sum`、`avg`、`min`、`max` 和 `reduce` 单次遍历来源，重复键的各次出现都参与计算。`every` 遇到第一个不满足条件的条目即停止。`count_by` 逐条计数，读完来源后产出各组计数。`median` 和 `mode` 先实体化来源，同键以最后一个值为准。空来源的平均值、最值、中位数和众数返回 `None`。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("x", 90), ("y", 5), ("x", 7), ("z", 5)]);
    assert_eq!(readings.sum()?, 107);
    assert_eq!(readings.median()?, Some(5.0));
    assert_eq!(readings.mode()?, Some(vec![5]));

    let counts = readings.count_by(|value, _| if *value == 5 { "low" } else { "high" });
    assert_eq!(counts.collect()?.to_json()?, r#"{"high":2,"low":2}"#);
    Ok(())
}
```

#### 计算匹配占比 {#lazy-percentage}

`percentage(predicate, precision)` 计算满足判定条件的条目百分比，并按指定小数位舍入。判定函数按来源顺序接收每个条目的值和键。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("a", 95), ("b", 82), ("c", 70)]);
    let passing = readings.percentage(|value, _| *value >= 80, 2)?;
    assert_eq!(passing, Some(66.67));
    Ok(())
}
```

该方法只遍历来源一次。空来源返回 `None`，来源错误会结束遍历。精度使用非负整数。

#### 比较和更新集合 {#lazy-comparing-and-updating}

`diff`、`intersect` 及其按键和关联比较的变体，在首次取值时实体化两个来源。`union` 保留已有键对应的值。`merge` 覆盖同名字符串键，并重排数字键。`replace` 先读完替换来源，再替换主来源中每个键首次出现的值，最后追加主来源未出现的替换键。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let current = LazyCollection::from_entries(|| [("a", 1), ("b", 2)]);
    let incoming = LazyCollection::from_entries(|| [("b", 2), ("c", 3)]);
    assert_eq!(
        current
            .clone()
            .diff_assoc(incoming.clone())
            .collect()?
            .to_json()?,
        r#"{"a":1}"#
    );
    assert_eq!(
        current.replace(incoming).collect()?.to_json()?,
        r#"{"a":1,"b":2,"c":3}"#
    );
    Ok(())
}
```

#### 按键筛选和投影 {#lazy-keys-and-projection}

`only` 按来源顺序返回每个请求键首次出现的条目，全部找到后停止。`except` 先实体化来源，再排除指定键。`key_by`、`pluck_with_keys` 和 `map_with_keys` 可多次产出同一键，结果实体化时以最后一个值为准。`group_by`、`map_to_dictionary` 和 `map_to_groups` 在首次取值时实体化来源，再返回完整分组。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let records = LazyCollection::from_entries(|| [("a", 10), ("a", 12), ("b", 7)]);
    assert_eq!(records.clone().only(["a"]).first()?, Some(10));
    assert_eq!(records.except(["b"]).first()?, Some(12));

    let readings = LazyCollection::from([("a", 10), ("b", 7)]);
    let mapped = readings.map_with_keys(|record, _| [(record.0, record.1 * 2)]);
    assert_eq!(mapped.collect()?.to_json()?, r#"{"a":20,"b":14}"#);
    Ok(())
}
```

#### 查找条目及相邻值 {#lazy-looking-up-items}

`get` 返回指定键首次出现的条目。`has` 检查单个键，`has_all` 检查所有指定键，`has_any` 检查是否存在任意指定键。PHP 的宽松相等可以匹配 `"08"` 和 `8`，Rust 按归一化后的 `ArrayKey` 比较，两者是不同的键。`search` 返回首个匹配的键。`before` 和 `after` 按来源顺序返回相邻值。`last` 遍历到来源结束。

```rust
use openrust::support::{ArrayKey, CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("08", 10), ("8", 12), ("end", 7)]);
    assert_eq!(readings.get(8)?, Some(12));
    assert!(readings.has_all(["08", "end"])?);
    assert_eq!(readings.search(&12)?, Some(ArrayKey::from(8)));
    assert_eq!(readings.before(&12)?, Some(10));
    assert_eq!(readings.after(&12)?, Some(7));
    assert_eq!(readings.last()?, Some(7));
    Ok(())
}
```

#### 缺失值回退 {#lazy-lookup-fallbacks}

用 `get_or_else`、`first_or_else`、`last_or_else` 或 `value_or_else` 传入回退闭包。需要按条件查找时，用 `first_where_or_else` 和 `last_where_or_else`。

```rust
use openrust::support::{CollectionError, LazyCollection};
use serde_json::{Value, json};

fn main() -> Result<(), CollectionError> {
    let rows = LazyCollection::from_entries(|| {
        [
            ("draft", json!({"profile":{"name":null}})),
            ("published", json!({"profile":{"name":"Taylor"}})),
        ]
    });

    let row = rows.get_or_else("archived", || json!({"status":"missing"}))?;
    assert_eq!(row, json!({"status":"missing"}));
    assert_eq!(
        rows.value_or_else("profile.name", || json!("anonymous"))?,
        Value::Null
    );
    let row = rows.last_where_or_else(
        |_, key| key == &"published".into(),
        || json!({"status":"missing"}),
    )?;
    assert_eq!(row["profile"]["name"], "Taylor");
    Ok(())
}
```

查找无结果时回退闭包执行一次。路径上已有的 JSON null 仍是查找结果，来源错误会返回调用方。也可以用 `|| default` 传入已有值。

#### 筛选值和字段 {#lazy-filtering-values}

`r#where` 和 `where_in` 比较类型化投影返回的值。严格版本使用相同的 Rust 相等规则。`where_between` 包含两个端点。`where_not_between` 匹配低于下界或高于上界的值。对 JSON，`where_null_at` 匹配缺失路径或显式空值，`where_not_null_at` 保留存在且非空的值。这些过滤方法按需读取，并保留原键。

```rust
use openrust::support::{CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("low", 1), ("same", 3), ("same", 4)]);
    let selected = readings.where_between(|value| *value, 3..=4).values();
    assert_eq!(selected.collect()?.to_json()?, "[3,4]");

    let records = LazyCollection::from([json!({}), json!({"status": false})]);
    assert_eq!(records.where_not_null_at("status").count()?, 1);
    Ok(())
}
```

#### 按值选择遍历边界 {#lazy-value-boundaries}

用 `take_until_value`、`take_while_value`、`skip_until_value` 和 `skip_while_value` 直接比较类型化的值。比较沿用 Rust 的 `PartialEq`。`take_until_value` 排除首个匹配项，`skip_until_value` 从首个匹配项开始返回。

```rust
use openrust::support::{CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| {
        [
            ("text", json!("2")),
            ("marker", json!(2)),
            ("hidden", json!("0")),
            ("tail", json!(3)),
        ]
    });

    let before = readings.clone().take_until_value(json!(2));
    let from = readings.skip_until_value(json!(2)).filter_truthy();
    assert_eq!(before.to_json()?, r#"{"text":"2"}"#);
    assert_eq!(from.to_json()?, r#"{"marker":2,"tail":3}"#);
    Ok(())
}
```

JSON 值可用 `filter_truthy()` 按 PHP 真值规则过滤，JSON 对象按关联数组处理。它会跳过 null、false、零、空字符串、字符串 `0`、空数组和空映射。这些操作都按需读取、保留原键并传播来源错误。

#### 分页、拆分和填充 {#lazy-paging-and-partition}

`for_page` 的页码从 1 开始，保留来源键。`split` 在首次取值时实体化来源并均衡分组。`split_in` 调用时先计数，再按需遍历以填满前面的组。`partition` 单次遍历，返回通过与未通过的两个实体集合。正数 `pad` 在来源结束后从右侧补齐，负数则先实体化并从左侧补齐。右侧填充无法分配整数键时，Rust 返回索引溢出错误。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from([10, 20, 30, 40, 50]);
    let page = readings.clone().for_page(2, 2).values();
    assert_eq!(page.collect()?.to_json()?, "[30,40]");

    let (passed, failed) = readings.partition(|value, _| *value >= 30)?;
    assert_eq!(passed.count(), 3);
    assert_eq!(failed.count(), 2);
    let padded = readings.pad(6, 0);
    assert_eq!(padded.collect()?.to_json()?, "[10,20,30,40,50,0]");
    Ok(())
}
```

#### 序列化值 {#lazy-serializing-values}

`to_array` 和 `json_serialize` 返回 JSON 值。`to_json` 与 `to_pretty_json` 返回编码后的文本。每次调用都会实体化来源。从零开始的连续整型键生成数组，稀疏键或命名键生成对象。重复键保留最后一个值。先调用 `values()` 可保留每次出现。`from_json` 将数组或对象解码成可重复遍历的来源。嵌套的惰性集合支持 Serde 序列化。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("a", 10), ("a", 12), ("b", 7)]);
    assert_eq!(readings.to_json()?, r#"{"a":12,"b":7}"#);
    assert_eq!(readings.values().to_json()?, "[10,12,7]");

    let decoded = LazyCollection::<i32>::from_json("[4,5]")?;
    assert_eq!(decoded.to_json()?, "[4,5]");
    Ok(())
}
```

#### 拼接投影值 {#lazy-implode-by}

用 `implode(glue)` 拼接可显示的值。对记录使用 `implode_by(project, glue)`，可选择字段或格式化每一项，回调也能读取条目的键。传入空分隔符可直接拼接。

```rust
use openrust::support::{CollectionError, LazyCollection};

struct Order {
    product: &'static str,
}

fn main() -> Result<(), CollectionError> {
    let orders = LazyCollection::from_entries(|| {
        [("a", "Desk"), ("b", "Lamp"), ("a", "Chair")]
            .map(|(key, product)| (key, Order { product }))
    });

    assert_eq!(
        orders.implode_by(|order, _| order.product, ", ")?,
        "Chair, Lamp"
    );
    assert_eq!(
        orders.implode_by(|order, key| format!("{key}:{}", order.product), " / ")?,
        "a:Chair / b:Lamp"
    );
    Ok(())
}
```

惰性集合先实体化，再调用投影。重复键最后一次出现的条目生效，键保持首次出现的位置。读取来源时发生错误会在执行投影前返回。

#### 生成和包装来源 {#lazy-generating-sources}

`range` 从起点向终点生成整数。步长恰好到达终点时包含终点。步长用正数 `NonZeroUsize` 表示。`times` 将从 1 开始的位置传给回调，结果键从 0 开始。`take` 可限制生成数量。`wrap(Some(item))` 产出一项，`wrap(None)` 产生空来源。`from_value` 接收 JSON 值，`unwrap` 实体化为带键条目。

```rust
use std::num::NonZeroUsize;

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let numbers = LazyCollection::range(9, 1, NonZeroUsize::new(2).unwrap());
    assert_eq!(numbers.take(3).to_json()?, "[9,7,5]");

    let readings = LazyCollection::times(10_000, |index| index * 5);
    assert_eq!(readings.take(2).to_json()?, "[5,10]");
    assert_eq!(
        LazyCollection::wrap(Some("ready")).to_json()?,
        r#"["ready"]"#
    );
    Ok(())
}
```

#### 条件分支和管道 {#lazy-conditionals-and-pipelines}

`when` 和 `unless` 立即选择回调。`when_empty` 与 `when_not_empty` 会探测第一项并可能返回来源错误。`when_empty_else` 和 `when_not_empty_else` 接收备用回调。`pipe` 可改变返回类型，`pipe_through` 串接返回同一集合类型的步骤。`tap` 在调用处检查集合。`ensure` 使用谓词逐条校验，首次不满足时停止。后续遍历会再次打开来源，可使用 `remember()` 重放已读条目。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::<i32>::empty()
        .when_empty(|_| LazyCollection::from([3, 5]))?
        .when(true, |items| items.map(|value, _| value * 2))
        .ensure(|value, _| *value <= 10)?;

    let total = readings.pipe(|items| items.reduce_with_keys(0, |sum, value, _| sum + value))?;
    assert_eq!(total, 16);
    Ok(())
}
```

#### 转换记录和嵌套行 {#lazy-transforming-records}

`select` 从每条记录中提取指定 JSON 字段，并重新编号。`flip` 交换类型为 `ArrayKey` 的值与键。`map_spread` 和 `each_spread` 把行切片及其键传给回调。`map_into` 通过 `From` 转换每个值。`merge_recursive`、`replace_recursive` 和 `multiply` 在首次取值时实体化输入。

```rust
use openrust::support::{ArrayKey, CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let fields = LazyCollection::from_entries(|| [("active", ArrayKey::from("status"))]);
    assert_eq!(fields.flip().to_json()?, r#"{"status":"active"}"#);

    let records = LazyCollection::from([json!({"status":"active","internal":42})]);
    assert_eq!(
        records.select(&["status"]).to_json()?,
        r#"[{"status":"active"}]"#
    );
    let rows = LazyCollection::from([vec![2_u8, 3]]);
    assert_eq!(
        rows.map_spread(|row, _| row.iter().sum::<u8>())
            .map_into::<u16>()
            .first()?,
        Some(5)
    );
    Ok(())
}
```

#### 组合和抽样 {#lazy-combining-and-sampling}

`cross_join` 与传入的切片计算笛卡尔积，在首次取值时实体化主来源。`shuffle` 同样在首次取值时实体化，并重新编号打乱后的值。`rand_item` 和 `random` 在调用时抽样。`random(count, preserve_keys)` 不放回抽样，返回的集合每次遍历都重放已选条目。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let pairs = LazyCollection::from([1, 2]).cross_join(&[&[10, 20]]);
    assert_eq!(pairs.first()?, Some(vec![1, 10]));

    let names = LazyCollection::from_entries(|| [("x", 4), ("y", 7), ("z", 9)]);
    let sample = names.random(2, true)?;
    let selected = sample.to_json()?;
    assert_eq!(sample.count()?, 2);
    assert_eq!(sample.to_json()?, selected);
    Ok(())
}
```

#### 单次遍历的缓存 {#lazy-caching-iterator}

`get_caching_iterator()` 会预读一项。用 `has_next()` 检查是否还有结果，再用 `next()` 前进。预读发现的来源错误会在前进到该项时返回。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let numbers = LazyCollection::from_entries(|| [("a", 1), ("a", 2), ("b", 3)]);
    let mut iterator = numbers.get_caching_iterator_with_full_cache();

    assert_eq!(iterator.next().transpose()?, Some(("a".into(), 1)));
    assert!(iterator.has_next());
    assert_eq!(
        iterator.get_cache().and_then(|cache| cache.get("a")),
        Some(&1)
    );

    assert_eq!(iterator.next().transpose()?, Some(("a".into(), 2)));
    assert_eq!(
        iterator.get_cache().and_then(|cache| cache.get("a")),
        Some(&2)
    );
    Ok(())
}
```

用 `get_caching_iterator_with_full_cache()` 启用 完整缓存行为。条目值需要支持克隆，每个键保留最近返回的值。`get_cache()` 提供已返回的条目。要在多次遍历间复用同一来源，请用 `remember()`。

旧名称 `contains_one_item()` 和 `contains_many_items()` 分别调用 `has_sole()` 和 `has_many()`。要检查是否恰有一项符合条件，使用 `has_sole_where()`。

#### 转义字符串表示 {#lazy-escaped-string}

`to_string()` 把集合转换成 JSON 文本。启用 `escape_when_casting_to_string(true)` 后，会对文本做 HTML 转义，已存在的实体也会转义。显式调用 `to_string()` 可以接收来源和序列化错误。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let values = LazyCollection::from(["<tag> &lt;"]).escape_when_casting_to_string(true);
    assert_eq!(values.to_string()?, "[&quot;&lt;tag&gt; &amp;lt;&quot;]");
    assert_eq!(values.to_json()?, r#"["<tag> &lt;"]"#);
    Ok(())
}
```

该开关作用于字符串转换。`to_json()` 和 `to_pretty_json()` 仍返回 JSON。

#### 输出惰性集合的调试内容 {#lazy-dump}

用 `dump()` 把带键的快照写到标准输出，然后继续链式调用。`dump_with(&[...])` 在条目之后输出额外的调试值。Rust 用 `Debug` 格式输出两者。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let values = LazyCollection::from([1, 2, 3]).dump_with(&[&"before mapping"])?;
    assert_eq!(values.map(|value, _| value * 2).first()?, Some(2));
    Ok(())
}
```

用 `dump_to(writer, context)` 指定输出。生成快照时会读完整个来源，重复键保留最后一个值。后续遍历会重新打开原始来源。来源或输出错误会返回给调用方。

#### 输出并停止执行 {#lazy-dump-and-stop}

用 `dd()` 或 `dd_with(&[...])` 配合 `?`，把调试快照交给当前执行边界。Artisan 命令返回 `ConsoleError` 时，快照写到标准输出，命令以状态码 1 退出。

```rust
use openrust::{console::ConsoleError, support::LazyCollection};

fn inspect_import() -> Result<i32, ConsoleError> {
    let rows = LazyCollection::from(["pending", "imported"]);
    match rows.dd_with(&[&"import stopped"])? {}
}
```

HTTP 处理器返回 `HttpError` 时，快照成为纯文本 HTTP 500 响应。服务进程继续处理其他请求。Rust 通过正常的结果传播结束当前命令或处理器。

#### 高阶消息 {#lazy-higher-order-proxy}

用 `proxy` 绑定集合操作，再用 `call` 传入类型化回调，读取各项的字段或调用其方法。`proxy(LazyCollection::map).call(...)` 对应 `$tasks->map->name`。回调可以捕获传给方法的参数。

```rust
use openrust::support::LazyCollection;

struct Task {
    name: &'static str,
    attempts: u32,
}

impl Task {
    fn exhausted(&self, limit: u32) -> bool {
        self.attempts >= limit
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tasks = LazyCollection::make(|| {
        [
            Task {
                name: "ingest",
                attempts: 1,
            },
            Task {
                name: "notify",
                attempts: 3,
            },
        ]
    });
    let names = tasks
        .clone()
        .proxy(LazyCollection::map)
        .call(|task, _| task.name);
    let retry_limit = 3;
    let exhausted = tasks
        .proxy(LazyCollection::filter)
        .call(move |task, _| task.exhausted(retry_limit));

    assert_eq!(
        names.all()?,
        vec![(0.into(), "ingest"), (1.into(), "notify")]
    );
    assert_eq!(
        exhausted.map(|task, _| task.name).all()?,
        vec![(1.into(), "notify")]
    );
    Ok(())
}
```

Rust 在编译期解析集合操作和数据项访问。自定义操作可将函数或闭包传给 `proxy`。选定的操作决定何时遍历来源、如何保留键以及如何传播错误。

#### `remember()`

`remember()` 创建一个共享迭代器，按需保留已读条目。每次遍历按位置重放，超过缓存前缀时继续从原来源读取。来源错误也会重放。值须实现 `Clone`，以便每次遍历取得拥有所有权的值。缓存属于集合本身，随读取量增长。
