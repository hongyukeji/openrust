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

## Keyed comparisons and position changes {#collection-key-operations}

`diff`, `intersect`, their association and key variants, and `duplicates` keep the original keys on returned entries. `shift` and `splice` renumber integer keys on the modified collection. `split` balances group sizes, and `splitIn` fills fixed-size groups while preserving keys. `chunkBy` groups adjacent values.

```rust
use openrust::support::Collection;

let mut rows = Collection::fromEntries([("4", "甲"), ("keep", "乙"), ("9", "丙")]);
assert_eq!(rows.shift(), Some("甲"));
assert_eq!(rows.toJson()?, r#"{"keep":"乙","0":"丙"}"#);
let existing = Collection::make(["乙"]);
let remaining = rows.diff(&existing).toArray()?;
assert_eq!(remaining, serde_json::json!(["丙"]));
```

## Typed transforms and nested JSON {#collection-projections}

Use typed projections for field comparison, grouping, and runtime type narrowing. `mapToGroups` returns collections, while `mapToDictionary` returns vectors. Use `Collection<Value>` for dot paths, recursive merge and replacement. `random` samples without replacement and can preserve keys.

Use Rust extension traits for collection macros, direct callbacks for higher-order messages, and `dbg!(&collection.all())` for diagnostics. The iterator adapters on `lazy()` provide caching and time-based traversal.

```rust
use openrust::support::Collection;
use serde_json::json;

let first = Collection::fromValue(json!({"team":{"tags":["甲"]}}));
let second = Collection::fromValue(json!({"team":{"tags":["乙"]}}));
let merged = first.mergeRecursive(second)?.dotWithDepth(Some(1))?.toArray()?;
assert_eq!(merged, json!({"team.tags":["甲","乙"]}));
```

## JSON Serialization

A collection serializes as a JSON array when its keys are consecutive from zero in iteration order. Other keys produce a JSON object. `toJson()` returns a JSON string or a serialization error, including for non-finite floats. A collection can also be passed directly to `ApiResponse::success`. Deserialize JSON directly into `Collection<T>` to retain object member order. `fromValue` uses the order of the supplied `serde_json::Value`.

## Lazy Collections

Database rows can be consumed asynchronously with Query Builder’s [cursor](../database/queries#streaming-results). The cursor supports stopping early.

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

#### Grouping and Sliding Windows {#lazy-grouping-and-sliding}

`chunkWhile` groups consecutive items. Its callback receives the next value, its key, and the current group. A false result starts the next group with that item. `sliding(size, step)` yields only complete windows. The window size and step must both be positive. Groups and windows preserve the source keys and are returned as materialized `Collection` values. Sliding windows require `Clone` so each window owns its values.

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

#### Selecting and Appending Items {#lazy-selection-and-append}

`slice(offset, length)` preserves source keys. A negative offset or length materializes the source first and applies the collection’s key rules. `nth(step, offset)` selects every n-th item after the offset and reindexes the results. `keys()` yields keys in encounter order, including repeated keys. `concat` takes a second `LazyCollection`, opens it after the first ends, and reindexes the combined values. `chunkBy` groups consecutive items using the typed value returned by a callback.

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

#### Expanding Nested Items {#lazy-expanding-nested-items}

`collapse` expands one level of nested lazy collections, materialized collections, vectors, or arrays and reindexes the values. `collapseWithKeys` keeps inner keys, and the last value for a repeated key wins on materialization. `flatMap` maps each item to a group and then collapses it. For JSON values, `flatten(Some(depth))` expands arrays and objects to the given depth and keeps scalars. Use `None` to expand all levels.

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

#### Pairing Sources {#lazy-pairing-sources}

`zip` pairs values by position. `zipMany` pairs the first source with several additional sources. Both preserve the longest source and represent missing values as `None`. `combine` takes keys from the values of the first source and values from the second. It returns `CollectionError::InvalidArgument` when traversal detects unequal lengths.

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

#### Deduplicating and Finding Matches {#lazy-deduplicating-and-finding}

`unique` and `uniqueStrict` use typed equality. `uniqueBy` deduplicates by the value your callback returns. They retain the first occurrence and its key. `contains` and `firstOrFail` stop at the first match. `hasSole` and `hasMany` need at most two matches. `sole` materializes those first two matches by key before checking that one value remains. Use the `Where` forms to supply a predicate. Source errors encountered before a decision are returned.

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

#### Sorting and Finding Duplicates {#lazy-sorting-and-duplicates}

`sort`, `sortBy`, `sortKeys`, their descending variants, `reverse`, and `duplicates` materialize the source on the first pull. The last value for each repeated source key is used. Sorting and reversing preserve keys. `duplicatesBy` yields repeated projection values under their original keys.

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("a", 12), ("b", 7), ("c", 12)]);
    let sorted = readings.clone().sort();
    assert_eq!(sorted.collect()?.toJson()?, r#"{"b":7,"a":12,"c":12}"#);
    let duplicates = readings.duplicatesBy(|value, _| value / 10);
    assert_eq!(duplicates.collect()?.toJson()?, r#"{"c":1}"#);
    Ok(())
}
```

#### Aggregating Values {#lazy-aggregating-values}

`sum`, `avg`, `min`, `max`, and `reduce` traverse the source once and include repeated keys. `every` stops at the first failing item. `countBy` counts every occurrence and yields group counts after the source ends. `median` and `mode` materialize the source first, so the last value for a repeated key is used. Empty averages, extrema, medians, and modes return `None`.

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::fromEntries(|| [("x", 90), ("y", 5), ("x", 7), ("z", 5)]);
    assert_eq!(readings.sum()?, 107);
    assert_eq!(readings.median()?, Some(5.0));
    assert_eq!(readings.mode()?, Some(vec![5]));

    let counts = readings.countBy(|value, _| if *value == 5 { "low" } else { "high" });
    assert_eq!(counts.collect()?.toJson()?, r#"{"high":2,"low":2}"#);
    Ok(())
}
```

#### Comparing and Updating Collections {#lazy-comparing-and-updating}

`diff`, `intersect`, and their key and association variants materialize both sources on the first pull. `union` keeps existing keys, while `merge` replaces matching string keys and reindexes numeric keys. `replace` reads the replacement source first, then applies each replacement to the first occurrence of its key in the primary source. It appends replacement keys that did not occur.

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let current = LazyCollection::fromEntries(|| [("a", 1), ("b", 2)]);
    let incoming = LazyCollection::fromEntries(|| [("b", 2), ("c", 3)]);
    assert_eq!(
        current
            .clone()
            .diffAssoc(incoming.clone())
            .collect()?
            .toJson()?,
        r#"{"a":1}"#
    );
    assert_eq!(
        current.replace(incoming).collect()?.toJson()?,
        r#"{"a":1,"b":2,"c":3}"#
    );
    Ok(())
}
```

#### Selecting Keys and Projecting Items {#lazy-keys-and-projection}

`only` returns the first occurrence of each requested key in source order and stops when it has found them all. `except` materializes the source before excluding keys. `keyBy`, `pluckWithKeys`, and `mapWithKeys` can yield a key more than once. Materializing their result keeps the last value for each key. `groupBy`, `mapToDictionary`, and `mapToGroups` materialize the source on the first pull and return completed groups.

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let records = LazyCollection::fromEntries(|| [("a", 10), ("a", 12), ("b", 7)]);
    assert_eq!(records.clone().only(["a"]).first()?, Some(10));
    assert_eq!(records.except(["b"]).first()?, Some(12));

    let readings = LazyCollection::from([("a", 10), ("b", 7)]);
    let mapped = readings.mapWithKeys(|record, _| [(record.0, record.1 * 2)]);
    assert_eq!(mapped.collect()?.toJson()?, r#"{"a":20,"b":14}"#);
    Ok(())
}
```

#### `remember()`

`remember()` creates one shared iterator and retains entries as they are read. Traversals replay entries by position and resume the source after the cached prefix. A source error also replays. Values must implement `Clone` so each traversal can own the values it receives. The cache is local to the collection and grows with the portion consumed.
