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

## JSON 序列化

当键在迭代顺序中是从零开始的连续整数时，集合序列化为 JSON 数组。其他键生成 JSON 对象。`toJson()` 返回 JSON 字符串或序列化错误，非有限浮点数也会报错。集合可以直接传给 `ApiResponse::success`。直接反序列化到 `Collection<T>` 可保留 JSON 对象成员顺序。`fromValue` 使用传入的 `serde_json::Value` 的顺序。

## 惰性集合

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

#### `remember()`

`remember()` 创建一个共享迭代器，按需保留已读条目。每次遍历按位置重放，超过缓存前缀时继续从原来源读取。来源错误也会重放。值须实现 `Clone`，以便每次遍历取得拥有所有权的值。缓存属于集合本身，随读取量增长。
