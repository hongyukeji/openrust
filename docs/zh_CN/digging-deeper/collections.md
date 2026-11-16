# 集合

## 创建集合

用 `Collection::make` 创建顺序集合，用 `Collection::fromEntries` 指定键。集合保留迭代顺序。整数键与 `"3"` 这样的规范十进制字符串对应同一个条目。`"03"` 这样的字符串保留原样。

## 可用方法

`map`、`filter` 和 `sortBy` 保留原键。调用 `values()` 可将键重排为从零开始的连续整数。用 `groupBy` 分组，第二个参数传 `true` 可在组内保留原键。

```rust
use openrust::support::Collection;

fn main() {
    let amounts = Collection::fromEntries([(0, 1), (3, 2), (7, 3)]);
    let mapped = amounts.map(|value, _| value * 10);
    let selected = mapped.filter(|value, _| *value >= 20);

    assert_eq!(selected.toJson().unwrap(), r#"{"3":20,"7":30}"#);
    assert_eq!(selected.values().toJson().unwrap(), "[20,30]");

    let groups = Collection::make(["apple", "apricot", "pear"])
        .groupBy(|word, _| word.starts_with('a'), false);
    assert_eq!(groups.toJson().unwrap(), r#"{"1":["apple","apricot"],"0":["pear"]}"#);
}
```

回调接收有类型的 Rust 值，可直接访问字段。`sortBy` 使用回调返回值的类型排序，相等时保留输入顺序。`avg` 和 `median` 返回 `Option<f64>`。

## 保键比较与位置操作 {#collection-key-operations}

`diff`、`intersect` 及其关联键变体和 `duplicates` 保留返回项的原键。`shift` 和 `splice` 重新编号被修改集合的整数键。`split` 平衡各组长度，`splitIn` 优先填满固定长度的组并保留键。`chunkBy` 按相邻值分组。

```rust
use openrust::support::Collection;

let mut rows = Collection::fromEntries([("4", "甲"), ("keep", "乙"), ("9", "丙")]);
assert_eq!(rows.shift(), Some("甲"));
assert_eq!(rows.toJson()?, r#"{"keep":"乙","0":"丙"}"#);
let existing = Collection::make(["乙"]);
let remaining = rows.diff(&existing).toArray()?;
assert_eq!(remaining, serde_json::json!(["丙"]));
```

## 类型化变换与嵌套 JSON {#collection-projections}

用类型化投影完成字段比较、分组和运行时类型筛选。`mapToGroups` 返回分组集合，`mapToDictionary` 返回分组向量。`Collection<Value>` 支持点路径、递归合并及替换。`random` 无放回抽样，可保留键。

集合扩展使用 Rust 扩展 trait，高阶消息对应直接回调，诊断可用 `dbg!(&collection.all())`。`lazy()` 的迭代器适配器提供缓存和基于时间的遍历。

```rust
use openrust::support::Collection;
use serde_json::json;

let first = Collection::fromValue(json!({"team":{"tags":["甲"]}}));
let second = Collection::fromValue(json!({"team":{"tags":["乙"]}}));
let merged = first.mergeRecursive(second)?.dotWithDepth(Some(1))?.toArray()?;
assert_eq!(merged, json!({"team.tags":["甲","乙"]}));
```

## JSON 序列化

当键在迭代顺序中是从零开始的连续整数时，集合序列化为 JSON 数组。其他键生成 JSON 对象。`toJson()` 返回 JSON 字符串或序列化错误，非有限浮点数也会报错。集合可以直接传给 `ApiResponse::success`。直接反序列化到 `Collection<T>` 可保留 JSON 对象成员顺序。`fromValue` 使用传入的 `serde_json::Value` 的顺序。

## 惰性集合

数据库行可通过查询构建器的 [cursor](../database/queries#streaming-results) 异步读取。该入口支持提前停止读取。

### 简介

`LazyCollection` 按需从 Rust 迭代器读取条目，适合逐步处理大数据源。`take(2)` 只读取生成两个结果所需的条目。负数限制会遍历到末尾，仅保留指定大小的尾部。

### 创建惰性集合

将迭代器工厂传给 `make`，带键来源使用 `fromEntries`。可能失败的来源使用 `tryMake` 或 `tryFromEntries`。普通遍历每次创建自己的迭代器，并在第一次错误时终止。组合后的迭代器在首次取值时才打开来源。可通过 `getIterator()` 或遍历 `&collection` 逐项读取结果。

```rust
use std::{cell::Cell, io::{BufRead, BufReader}};

use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let reads = Cell::new(0);
    let lines = LazyCollection::tryMake(|| {
        BufReader::new(&b"INFO ready\nWARN slow\nWARN retry\n"[..])
            .lines()
            .inspect(|_| reads.set(reads.get() + 1))
            .map(|line| line.map_err(|error| CollectionError::Source(Box::new(error))))
    })
    .filter(|line, _| line.starts_with("WARN"))
    .values()
    .remember();

    assert_eq!(reads.get(), 0);
    assert_eq!(lines.clone().take(1).collect()?.toJson()?, r#"["WARN slow"]"#);
    assert_eq!(reads.get(), 2);
    assert_eq!(lines.collect()?.toJson()?, r#"["WARN slow","WARN retry"]"#);
    assert_eq!(reads.get(), 3);
    Ok(())
}
```

### Enumerable 契约

`Collection` 和 `LazyCollection` 都实现 `Enumerable`，提供 `map`、`filter`、`values`、`take`、`all` 和 `collect`。`all` 或 `collect` 收集遍历结果时，同一个键保留最后一个值。需要保留每次出现的条目时，先调用 `values`。`Collection::lazy()` 可将已收集的集合用作来源。

### 惰性集合方法

`tapEach` 在读取条目时执行副作用。`takeUntilTimeout` 在两次读取之间检查单调时钟的截止时间。`takeUntilTimeoutWith` 还可回传最后一个条目，若起初就超时则回传空值。`throttle` 在调用线程等待剩余间隔。`withHeartbeat` 在遍历时检查间隔。同步读取可能在截止时间之后才返回。

`chunk` 每次生成一个已收集的 `Collection`，可选择保留组内键。惰性集合在拥有它的线程运行，可以借用线程内的局部状态。

#### 分组和滑动窗口 {#lazy-grouping-and-sliding}

`chunkWhile` 对连续条目分组。回调接收下一个值、它的键和当前组。返回 false 时，该条目成为新组的首项。`sliding(size, step)` 只产出完整窗口，窗口大小和步长都须为正数。分组和窗口保留原键，每组以实体化的 `Collection` 返回。滑动窗口要求值实现 `Clone`，让各窗口独立拥有值。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::make(|| [4, 4, 8, 8, 12]);
    let groups = readings
        .clone()
        .chunkWhile(|reading, _, group| group.last() == Some(reading));
    assert_eq!(groups.count()?, 3);

    let windows = readings.sliding(2, 2)?;
    assert_eq!(windows.count()?, 2);
    assert_eq!(windows.first()?.unwrap().toJson()?, "[4,4]");
    Ok(())
}
```

#### 选取和追加条目 {#lazy-selection-and-append}

`slice(offset, length)` 保留原键。偏移或长度为负数时，先实体化来源并应用集合的键规则。`nth(step, offset)` 从偏移位置开始每隔指定步长选取一项，并重新编号。`keys()` 按读取顺序产出键，包括重复键。`concat` 待读完第一个惰性集合再打开第二个，并统一重排索引。`chunkBy` 按回调返回的类型化值对连续条目分组。

```rust
use openrust::support::{ArrayKey, CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 4), ("b", 7), ("c", 11)]);
    assert_eq!(
        readings.clone().slice(1, Some(1)).all()?,
        vec![(ArrayKey::from("b"), 7)]
    );
    assert_eq!(readings.clone().keys().count()?, 3);
    assert_eq!(readings.clone().chunkBy(|value, _| value / 10).count()?, 2);

    let combined = readings.nth(2, 0)?.concat(LazyCollection::make(|| [15]));
    assert_eq!(combined.collect()?.toJson()?, "[4,11,15]");
    Ok(())
}
```

#### 展开嵌套条目 {#lazy-expanding-nested-items}

`collapse` 展开一层嵌套的惰性集合、实体集合、向量或数组，并重新编号。`collapseWithKeys` 保留内部键，实体化时同键的最后一个值生效。`flatMap` 将每个条目映射成一组，再展开。对于 JSON 值，`flatten(Some(depth))` 按指定深度展开数组和对象并保留标量。用 `None` 展开全部层级。

```rust
use openrust::support::{Collection, CollectionError, LazyCollection};
use serde_json::json;

fn main() -> Result<(), CollectionError> {
    let groups = LazyCollection::make(|| {
        [
            Collection::fromEntries([("first", 1)]),
            Collection::fromEntries([("second", 2)]),
        ]
    });
    assert_eq!(
        groups.collapseWithKeys().collect()?.toJson()?,
        r#"{"first":1,"second":2}"#
    );

    let doubled = LazyCollection::make(|| [1, 3]).flatMap(|value, _| [*value, value * 2]);
    assert_eq!(doubled.collect()?.toJson()?, "[1,2,3,6]");

    let nested = LazyCollection::make(|| [json!([1, [2]]), json!(3)]);
    assert_eq!(nested.flatten(None).collect()?.toJson()?, "[1,2,3]");
    Ok(())
}
```

#### 配对多个来源 {#lazy-pairing-sources}

`zip` 按位置配对值。用 `zipMany` 将第一个来源与多个后续来源配对。两者都读取到最长来源结束，缺项用 `None` 表示。`combine` 将第一个来源的值用作键，与第二个来源的值配对。遍历发现长度不同时返回 `CollectionError::InvalidArgument`。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let numbers = LazyCollection::from([1, 2]);
    let rows = numbers.clone().zip(["one"]);
    assert_eq!(rows.collect()?.toJson()?, r#"[[1,"one"],[2,null]]"#);

    let labels = LazyCollection::from(["first", "second"]);
    assert_eq!(
        labels.combine(numbers).collect()?.toJson()?,
        r#"{"first":1,"second":2}"#
    );
    Ok(())
}
```

#### 去重和查找匹配项 {#lazy-deduplicating-and-finding}

`unique` 和 `uniqueStrict` 使用类型化相等比较。`uniqueBy` 按回调返回的值去重。这些方法保留第一次出现的条目及其原键。`contains` 和 `firstOrFail` 在首次匹配时停止。`hasSole` 和 `hasMany` 最多读取两条匹配。`sole` 先按键实体化前两条匹配，再检查是否只剩一个值。用带 `Where` 的方法传入谓词。得出结果前遇到的来源错误会原样返回。

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 10), ("b", 11), ("c", 21)]);
    let distinct = readings.clone().uniqueBy(|value, _| value / 10);
    assert_eq!(distinct.collect()?.toJson()?, r#"{"a":10,"c":21}"#);
    assert!(readings.contains(&11)?);
    assert!(readings.hasSoleWhere(|value, _| *value > 20)?);
    assert_eq!(readings.firstOrFailWhere(|value, _| *value > 10)?, 11);
    Ok(())
}
```

#### `remember()`

`remember()` 创建一个共享迭代器，按需保留已读条目。每次遍历按位置重放，超过缓存前缀时继续从原来源读取。来源错误也会重放。值须实现 `Clone`，以便每次遍历取得拥有所有权的值。缓存属于集合本身，随读取量增长。
