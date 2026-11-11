# Collections

## Creating Collections

Use `Collection::make` for a sequence and `Collection::fromEntries` for explicit keys. Collections keep iteration order. Integer keys and canonical decimal strings such as `"3"` refer to the same entry. Strings such as `"03"` keep their original spelling.

## Available Methods

Use `map`, `filter`, and `sortBy` to transform entries while keeping their keys. Call `values()` to assign consecutive integer keys. Use `groupBy` to group entries and pass `true` as the second argument to retain original keys inside each group.

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

Callbacks receive typed Rust values and can project fields directly. `sortBy` uses the ordering of the projected type and preserves input order for ties. `avg` and `median` return `Option<f64>`.

## JSON Serialization

A collection serializes as a JSON array when its keys are consecutive from zero in iteration order. Other keys produce a JSON object. `toJson()` returns a JSON string or a serialization error, including for non-finite floats. A collection can also be passed directly to `ApiResponse::success`. Deserialize JSON directly into `Collection<T>` to retain object member order. `fromValue` uses the order of the supplied `serde_json::Value`.
