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
