# Collections

## Creating Collections

Use `Collection::make` for a sequence and `Collection::from_entries` for explicit keys. Collections keep iteration order. Integer keys and canonical decimal strings such as `"3"` refer to the same entry. Strings such as `"03"` keep their original spelling.

## Available Methods

Use `map`, `filter`, and `sort_by` to transform entries while keeping their keys. Call `values()` to assign consecutive integer keys. Use `group_by` to group entries and pass `true` as the second argument to retain original keys inside each group.

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

Callbacks receive typed Rust values and can project fields directly. `sort_by` uses the ordering of the projected type and preserves input order for ties. `avg` and `median` return `Option<f64>`.

## Keyed comparisons and position changes {#collection-key-operations}

`diff`, `intersect`, their association and key variants, and `duplicates` keep the original keys on returned entries. `shift` and `splice` renumber integer keys on the modified collection. `split` balances group sizes, and `split_in` fills fixed-size groups while preserving keys. `chunk_by` groups adjacent values.

```rust
use openrust::support::Collection;

let mut rows = Collection::from_entries([("4", "甲"), ("keep", "乙"), ("9", "丙")]);
assert_eq!(rows.shift(), Some("甲"));
assert_eq!(rows.to_json()?, r#"{"keep":"乙","0":"丙"}"#);
let existing = Collection::make(["乙"]);
let remaining = rows.diff(&existing).to_array()?;
assert_eq!(remaining, serde_json::json!(["丙"]));
```

## Typed transforms and nested JSON {#collection-projections}

Use typed projections for field comparison, grouping, and runtime type narrowing. `map_to_groups` returns collections, while `map_to_dictionary` returns vectors. Use `Collection<Value>` for dot paths, recursive merge and replacement. `random` samples without replacement and can preserve keys.

Use Rust extension traits for collection macros, direct callbacks for higher-order messages, and `dbg!(&collection.all())` for diagnostics. The iterator adapters on `lazy()` provide caching and time-based traversal.

```rust
use openrust::support::Collection;
use serde_json::json;

let first = Collection::from_value(json!({"team":{"tags":["甲"]}}));
let second = Collection::from_value(json!({"team":{"tags":["乙"]}}));
let merged = first.merge_recursive(second)?.dot_with_depth(Some(1))?.to_array()?;
assert_eq!(merged, json!({"team.tags":["甲","乙"]}));
```

## JSON Serialization

A collection serializes as a JSON array when its keys are consecutive from zero in iteration order. Other keys produce a JSON object. `to_json()` returns a JSON string or a serialization error, including for non-finite floats. A collection can also be passed directly to `ApiResponse::success`. Deserialize JSON directly into `Collection<T>` to retain object member order. `from_value` uses the order of the supplied `serde_json::Value`.

## Lazy Collections

Database rows can be consumed asynchronously with Query Builder’s [cursor](../database/queries#streaming-results). The cursor supports stopping early.

### Introduction

`LazyCollection` processes items from a Rust iterator as they are requested. Use it to process a large source incrementally. `take(2)` reads only the items needed to produce two results. A negative limit reads to the end and keeps a bounded tail.

### Creating Lazy Collections

Pass an iterator factory to `make` or `from_entries` for keyed input. Use `try_make` or `try_from_entries` for fallible input. Each traversal normally opens its own iterator and ends at its first error. A composed iterator opens its source when first polled. Use `get_iterator()` or iterate over `&collection` to read individual results.

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

### The Enumerable Contract

Both `Collection` and `LazyCollection` implement `Enumerable` with `map`, `filter`, `values`, `take`, `all`, and `collect`. Materializing a traversal with `all` or `collect` keeps the last value for a repeated key. Apply `values` before materializing to keep each occurrence. `Collection::lazy()` uses a materialized collection as its source.

### Lazy Collection Methods

`tap_each` runs a side effect for each requested item.

`chunk` emits one materialized `Collection` at a time and can preserve its keys. A lazy collection runs on the thread that owns it and can borrow local state.

#### Stopping at a Deadline {#lazy-deadline}

Use `take_until_timeout` with a monotonic `Instant`. A traversal that starts at or after the deadline leaves the source unopened. A read already started can yield an item after the deadline. The next pull ends the traversal before reading another item.

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

With `take_until_timeout_with`, an expired start calls the callback with `None`. When the deadline passes during iteration, the callback receives the last yielded value and key on the next pull. Normal exhaustion and source errors keep their own results.

#### Pacing and Heartbeats {#lazy-pacing}

`throttle` waits on the calling thread when iteration resumes. It subtracts time spent handling the previous item from the interval. Pulling to exhaustion resumes the iterator after the last item. Stopping early with `take` leaves that last wait unstarted.

`with_heartbeat` checks elapsed time after a successful read and runs the callback before yielding that item. The interval starts over after each heartbeat, and each traversal starts its own clock. A zero interval runs the callback for every successful item.

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

#### Grouping and Sliding Windows {#lazy-grouping-and-sliding}

`chunk_while` groups consecutive items. Its callback receives the next value, its key, and the current group. A false result starts the next group with that item. `sliding(size, step)` yields only complete windows. The window size and step must both be positive. Groups and windows preserve the source keys and are returned as materialized `Collection` values. Sliding windows require `Clone` so each window owns its values.

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

#### Selecting and Appending Items {#lazy-selection-and-append}

`slice(offset, length)` preserves source keys. A negative offset or length materializes the source first and applies the collection’s key rules. `nth(step, offset)` selects every n-th item after the offset and reindexes the results. `keys()` yields keys in encounter order, including repeated keys. `concat` takes a second `LazyCollection`, opens it after the first ends, and reindexes the combined values. `chunk_by` groups consecutive items using the typed value returned by a callback.

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

#### Expanding Nested Items {#lazy-expanding-nested-items}

`collapse` expands one level of nested lazy collections, materialized collections, vectors, or arrays and reindexes the values. `collapse_with_keys` keeps inner keys, and the last value for a repeated key wins on materialization. `flat_map` maps each item to a group and then collapses it. For JSON values, `flatten(Some(depth))` expands arrays and objects to the given depth and keeps scalars. Use `None` to expand all levels.

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

#### Pairing Sources {#lazy-pairing-sources}

`zip` pairs values by position. `zip_many` pairs the first source with several additional sources. Both preserve the longest source and represent missing values as `None`. `combine` takes keys from the values of the first source and values from the second. It returns `CollectionError::InvalidArgument` when traversal detects unequal lengths.

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

#### Deduplicating and Finding Matches {#lazy-deduplicating-and-finding}

`unique` and `unique_strict` use typed equality. `unique_by` deduplicates by the value your callback returns. They retain the first occurrence and its key. `contains` and `first_or_fail` stop at the first match. `has_sole` and `has_many` need at most two matches. `sole` materializes those first two matches by key before checking that one value remains. Use the `Where` forms to supply a predicate. Source errors encountered before a decision are returned.

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

#### Sorting and Finding Duplicates {#lazy-sorting-and-duplicates}

`sort`, `sort_by`, `sort_keys`, their descending variants, `reverse`, and `duplicates` materialize the source on the first pull. The last value for each repeated source key is used. Sorting and reversing preserve keys. `duplicates_by` yields repeated projection values under their original keys.

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

#### Aggregating Values {#lazy-aggregating-values}

`sum`, `avg`, `min`, `max`, and `reduce` traverse the source once and include repeated keys. `every` stops at the first failing item. `count_by` counts every occurrence and yields group counts after the source ends. `median` and `mode` materialize the source first, so the last value for a repeated key is used. Empty averages, extrema, medians, and modes return `None`.

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

#### Calculating a Percentage {#lazy-percentage}

`percentage(predicate, precision)` returns the share of entries for which the predicate returns true, rounded to the requested number of decimal places. It passes each value and key to the predicate in source order.

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let readings = LazyCollection::from_entries(|| [("a", 95), ("b", 82), ("c", 70)]);
    let passing = readings.percentage(|value, _| *value >= 80, 2)?;
    assert_eq!(passing, Some(66.67));
    Ok(())
}
```

The method traverses its source once. An empty source returns `None`, and a source error ends the traversal. The precision is a nonnegative integer.

#### Comparing and Updating Collections {#lazy-comparing-and-updating}

`diff`, `intersect`, and their key and association variants materialize both sources on the first pull. `union` keeps existing keys, while `merge` replaces matching string keys and reindexes numeric keys. `replace` reads the replacement source first, then applies each replacement to the first occurrence of its key in the primary source. It appends replacement keys that did not occur.

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

#### Selecting Keys and Projecting Items {#lazy-keys-and-projection}

`only` returns the first occurrence of each requested key in source order and stops when it has found them all. `except` materializes the source before excluding keys. `key_by`, `pluck_with_keys`, and `map_with_keys` can yield a key more than once. Materializing their result keeps the last value for each key. `group_by`, `map_to_dictionary`, and `map_to_groups` materialize the source on the first pull and return completed groups.

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

#### Looking Up Items and Neighbors {#lazy-looking-up-items}

`get` returns the first entry with the given key. `has` checks one key, `has_all` checks all requested keys, and `has_any` checks whether at least one exists. Rust compares normalized `ArrayKey` values, so `"08"` and `8` are distinct. PHP loose equality can match those two keys. `search` returns the first matching key. `before` and `after` return adjacent values in source order. `last` traverses the source to the end.

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

#### Fallbacks for Missing Values {#lazy-lookup-fallbacks}

Use `get_or_else`, `first_or_else`, `last_or_else`, or `value_or_else` to supply a fallback as a closure. Use `first_where_or_else` and `last_where_or_else` when the lookup checks a predicate.

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

The fallback runs once when the lookup finds no result. A present JSON null is a result, and source errors propagate to the caller. Pass a closure such as `|| default` to use an existing value.

#### Filtering Values and Fields {#lazy-filtering-values}

`r#where` and `where_in` compare the value returned by a typed projection. Their strict variants use the same Rust equality rules. `where_between` includes both endpoints. `where_not_between` tests whether a value is below the lower bound or above the upper bound. For JSON, `where_null_at` matches a missing path or explicit null, while `where_not_null_at` keeps present non-null values. These filters preserve keys and evaluate only the entries requested.

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

#### Matching Boundary Values {#lazy-value-boundaries}

Use `take_until_value`, `take_while_value`, `skip_until_value`, and `skip_while_value` to compare items directly with a typed value. The comparison uses Rust’s `PartialEq`. The first matching item is excluded by `take_until_value` and included by `skip_until_value`.

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

For JSON values, `filter_truthy()` applies PHP truthiness with JSON objects treated as associative arrays. It skips null, false, zero, the empty string, the string `0`, and empty arrays or maps. All these operations read on demand, preserve keys, and propagate source errors.

#### Paging, Splitting, and Padding {#lazy-paging-and-partition}

`for_page` uses one-based page numbers and preserves source keys. `split` materializes on the first pull and balances the groups. `split_in` counts the source on call, then traverses it again on demand to fill the earlier groups. `partition` traverses once and returns the passing and failing groups as two materialized collections. A positive `pad` size adds items on the right after the source ends. A negative size materializes and pads on the left. Rust reports index overflow if the right padding cannot be assigned an integer key.

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

#### Serializing Values {#lazy-serializing-values}

`to_array` and `json_serialize` return a JSON value. `to_json` and `to_pretty_json` return encoded text. Each call materializes the source. Consecutive integer keys starting at zero produce an array, and sparse or named keys produce an object. The last value wins for repeated keys. Use `values()` first to retain each occurrence. `from_json` decodes arrays or objects into a repeatable source. Nested lazy collections participate in Serde serialization.

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

#### Joining Projected Values {#lazy-implode-by}

Use `implode(glue)` for displayable items. For records, use `implode_by(project, glue)` to select a field or format each item. The projection also receives the entry key. Pass an empty separator to concatenate the results directly.

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

A lazy collection materializes its entries before applying the projection. The last item for a repeated key wins and retains the key's original position. A source error returns before any projection runs.

#### Generating and Wrapping Sources {#lazy-generating-sources}

`range` generates integers from the first endpoint toward the second, including the endpoint if the step reaches it. Pass a positive `NonZeroUsize` step. `times` calls the callback with one-based positions and assigns zero-based keys. Calling `take` limits generation. `wrap(Some(item))` yields one item and `wrap(None)` yields none. `from_value` accepts JSON values, and `unwrap` materializes keyed entries.

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

#### Conditionals and Pipelines {#lazy-conditionals-and-pipelines}

`when` and `unless` select a callback immediately. `when_empty` and `when_not_empty` probe the first item and can return a source error. `when_empty_else` and `when_not_empty_else` accept a fallback. `pipe` can change the result type, while `pipe_through` chains stages that return the same collection type. `tap` inspects the collection at the call site. `ensure` validates every item with a predicate and stops at the first failure. A later traversal opens the source again unless you use `remember()`.

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

#### Transforming Records and Nested Rows {#lazy-transforming-records}

`select` extracts the requested JSON fields from each row and reindexes the rows. `flip` exchanges typed `ArrayKey` values and keys. `map_spread` and `each_spread` pass a row slice and its key to a callback. `map_into` converts each value with `From`. `merge_recursive`, `replace_recursive`, and `multiply` materialize their inputs on the first pull.

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

#### Combining and Sampling Values {#lazy-combining-and-sampling}

`cross_join` produces the Cartesian product with the supplied slices. It materializes the primary source on the first pull. `shuffle` also materializes on the first pull and reindexes the shuffled values. `rand_item` and `random` sample at the call site. `random(count, preserve_keys)` samples without replacement and returns a collection that replays the chosen entries on every traversal.

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

#### Caching One Traversal {#lazy-caching-iterator}

`get_caching_iterator()` keeps one result of lookahead. Call `has_next()` to check for a pending result, then advance with `next()`. A source error discovered by lookahead is returned when you advance to that result.

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

Use `get_caching_iterator_with_full_cache()` for full-cache behavior. It requires cloneable values and keeps the last yielded value for each key. `get_cache()` returns the entries already yielded. Use `remember()` to replay a shared source across several traversals.

The older `contains_one_item()` and `contains_many_items()` names call `has_sole()` and `has_many()`. Use `has_sole_where()` to test for a single matching item with a predicate.

#### Escaping the String Representation {#lazy-escaped-string}

`to_string()` renders the collection as JSON text. Enable `escape_when_casting_to_string(true)` to HTML-escape that text, including existing entities. Call `to_string()` explicitly so source and serialization errors can be returned.

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let values = LazyCollection::from(["<tag> &lt;"]).escape_when_casting_to_string(true);
    assert_eq!(values.to_string()?, "[&quot;&lt;tag&gt; &amp;lt;&quot;]");
    assert_eq!(values.to_json()?, r#"["<tag> &lt;"]"#);
    Ok(())
}
```

The setting applies to string conversion. `to_json()` and `to_pretty_json()` continue to return JSON.

#### Dumping a Lazy Collection {#lazy-dump}

Use `dump()` to print a keyed snapshot to standard output and continue the chain. `dump_with(&[...])` prints additional debug values after the entries. Rust formats both with `Debug`.

```rust
use openrust::support::{CollectionError, LazyCollection};

fn main() -> Result<(), CollectionError> {
    let values = LazyCollection::from([1, 2, 3]).dump_with(&[&"before mapping"])?;
    assert_eq!(values.map(|value, _| value * 2).first()?, Some(2));
    Ok(())
}
```

Use `dump_to(writer, context)` to choose the output. The snapshot reads the complete source and keeps the last value for a repeated key. A subsequent traversal opens the original source again. Source or output errors are returned to the caller.

#### Dumping and Stopping Execution {#lazy-dump-and-stop}

Use `dd()` or `dd_with(&[...])` with `?` to send the dump to the current execution boundary. In an Artisan command returning `ConsoleError`, the dump goes to standard output and the command exits with status 1.

```rust
use openrust::{console::ConsoleError, support::LazyCollection};

fn inspect_import() -> Result<i32, ConsoleError> {
    let rows = LazyCollection::from(["pending", "imported"]);
    match rows.dd_with(&[&"import stopped"])? {}
}
```

In a handler returning `HttpError`, the dump becomes a plain-text HTTP 500 response. The HTTP server continues serving other requests. Rust uses its normal result propagation to stop the current command or handler.

#### Higher-Order Messages {#lazy-higher-order-proxy}

Bind a collection operation with `proxy`, then pass a typed callback to `call` to read a field or call a method on each item. For example, `proxy(LazyCollection::map).call(...)` corresponds to `$tasks->map->name`. Method arguments can be captured by the callback.

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

Rust resolves the collection operation and item access at compile time. To use a custom operation, pass its function or a closure to `proxy`. The selected operation determines when the source is traversed, how keys are kept, and how errors propagate.

#### `remember()`

`remember()` creates one shared iterator and retains entries as they are read. Traversals replay entries by position and resume the source after the cached prefix. A source error also replays. Values must implement `Clone` so each traversal can own the values it receives. The cache is local to the collection and grows with the portion consumed.
