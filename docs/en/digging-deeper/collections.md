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

## Lazy Collections

### Introduction

`LazyCollection` processes items from a Rust iterator as they are requested. Use it to process a large source incrementally. `take(2)` reads only the items needed to produce two results. A negative limit reads to the end and keeps a bounded tail.

### Creating Lazy Collections

Pass an iterator factory to `make` or `fromEntries` for keyed input. Use `tryMake` or `tryFromEntries` for fallible input. Each traversal normally opens its own iterator and ends at its first error. A composed iterator opens its source when first polled. Use `getIterator()` or iterate over `&collection` to read individual results.

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

### The Enumerable Contract

Both `Collection` and `LazyCollection` implement `Enumerable` with `map`, `filter`, `values`, `take`, `all`, and `collect`. Materializing a traversal with `all` or `collect` keeps the last value for a repeated key. Apply `values` before materializing to keep each occurrence. `Collection::lazy()` uses a materialized collection as its source.

### Lazy Collection Methods

`tapEach` runs a side effect for each requested item. `takeUntilTimeout` checks a monotonic deadline between pulls. `takeUntilTimeoutWith` can also report the last item, or no item when the deadline has already passed. `throttle` sleeps on the calling thread for the remaining interval. `withHeartbeat` checks the interval during iteration. A synchronous source read can finish after the deadline.

`chunk` emits one materialized `Collection` at a time and can preserve its keys. A lazy collection runs on the thread that owns it and can borrow local state.

#### `remember()`

`remember()` creates one shared iterator and retains entries as they are read. Traversals replay entries by position and resume the source after the cached prefix. A source error also replays. Values must implement `Clone` so each traversal can own the values it receives. The cache is local to the collection and grows with the portion consumed.
