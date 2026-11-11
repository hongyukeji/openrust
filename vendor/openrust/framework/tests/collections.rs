use std::cell::Cell;

use openrust::support::{ArrayKey, Collection, CollectionError, Enumerable};
use serde::{Serialize, Serializer};
use serde_json::json;

#[test]
fn keys_follow_php_integer_normalization_append_and_replacement_order() {
    let mut values = Collection::fromEntries([
        ("-5", "negative"),
        ("1", "first"),
        ("01", "leading zero"),
        ("+1", "plus"),
        ("-0", "signed zero"),
        ("1", "replacement"),
    ]);
    assert_eq!(values.keys().toJson().unwrap(), r#"[-5,1,"01","+1","-0"]"#);
    assert_eq!(values.get(1), Some(&"replacement"));
    values.put(7, "later").forget(7);
    values.push("appended").unwrap();
    assert_eq!(values.get(8), Some(&"appended"));
    assert_eq!(values.pop(), Some("appended"));
    values.push("reused").unwrap();
    assert_eq!(values.get(8), Some(&"reused"));

    let mut negative = Collection::fromEntries([(-5, 1)]);
    negative.push(2).unwrap();
    assert_eq!(negative.toJson().unwrap(), r#"{"-5":1,"-4":2}"#);

    let mut smallest = Collection::fromEntries([(i64::MIN, 1)]);
    assert_eq!(smallest.pop(), Some(1));
    smallest.push(2).unwrap();
    assert_eq!(smallest.get(i64::MIN), Some(&2));
    let mut largest = Collection::fromEntries([(i64::MAX, 1)]);
    assert!(matches!(
        largest.push(2),
        Err(CollectionError::IndexOverflow)
    ));
    assert_eq!(largest.count(), 1);
}

#[test]
fn mapping_filtering_and_plucking_keep_the_upstream_key_shapes() {
    let source = Collection::fromEntries([(0, 1), (3, 2), (7, 3)]);
    let mapped = source
        .clone()
        .map(|value, _| value * 10)
        .filter(|value, _| *value >= 20);
    assert_eq!(mapped.toJson().unwrap(), r#"{"3":20,"7":30}"#);
    assert_eq!(mapped.values().toJson().unwrap(), "[20,30]");
    assert_eq!(source.pluck(|value| value * 2).toJson().unwrap(), "[2,4,6]");
    let collisions = source
        .clone()
        .mapWithKeys(|value, _| [("same", value * 2), ("x", *value)]);
    assert_eq!(collisions.toJson().unwrap(), r#"{"same":6,"x":3}"#);
    let rekeyed = source.clone().keyBy(|value, _| *value % 2);
    assert_eq!(rekeyed.toJson().unwrap(), r#"{"1":3,"0":2}"#);
    assert_eq!(
        source.clone().only([3, 7]).toJson().unwrap(),
        r#"{"3":2,"7":3}"#
    );
    assert_eq!(
        source.clone().except([3]).toJson().unwrap(),
        r#"{"0":1,"7":3}"#
    );

    let visits = Cell::new(0);
    let failure = source.tryMap(|value, _| {
        visits.set(visits.get() + 1);
        if *value == 2 {
            Err("failed")
        } else {
            Ok(*value)
        }
    });
    assert_eq!(failure.unwrap_err(), "failed");
    assert_eq!(visits.get(), 2);
}

#[test]
fn grouping_partitioning_and_stable_sorting_preserve_order() {
    let source =
        Collection::fromEntries([("one", (2, "A")), ("two", (1, "B")), ("three", (2, "C"))]);
    let calls = Cell::new(0);
    let sorted = source.clone().sortBy(|(value, _), _| {
        calls.set(calls.get() + 1);
        *value
    });
    assert_eq!(calls.get(), 3);
    assert_eq!(sorted.keys().toJson().unwrap(), r#"["two","one","three"]"#);
    assert_eq!(
        source
            .clone()
            .sortByDesc(|(value, _), _| *value)
            .keys()
            .toJson()
            .unwrap(),
        r#"["one","three","two"]"#
    );
    let groups = source.clone().groupBy(|(value, _), _| *value, true);
    assert_eq!(
        groups.toJson().unwrap(),
        r#"{"2":{"one":[2,"A"],"three":[2,"C"]},"1":{"two":[1,"B"]}}"#
    );
    let many = source
        .clone()
        .groupByMany(|(value, _), _| [0, *value], false);
    assert_eq!(many.get(0).unwrap().count(), 3);
    assert_eq!(
        many.get(2)
            .unwrap()
            .pluck(|(_, name)| *name)
            .toJson()
            .unwrap(),
        r#"["A","C"]"#
    );
    let (even, odd) = source.partition(|(value, _), _| value % 2 == 0);
    assert_eq!(even.keys().toJson().unwrap(), r#"["one","three"]"#);
    assert_eq!(odd.keys().toJson().unwrap(), r#"["two"]"#);
}

#[test]
fn aggregates_boundaries_and_shared_enumerable_contract() {
    let source = Collection::fromEntries([(4, 9), (5, 3), (8, 1), (9, 7)]);
    assert_eq!(source.sum(), 20);
    assert_eq!(source.avg(), Some(5.0));
    assert_eq!(source.median(), Some(5.0));
    assert_eq!(source.min(), Some(&1));
    assert_eq!(source.max(), Some(&9));
    assert_eq!(
        source.avgBy(|value, _| (*value > 3).then_some(*value as f64)),
        Some(8.0)
    );
    assert_eq!(
        source.clone().slice(-3, Some(-1)).toJson().unwrap(),
        r#"{"5":3,"8":1}"#
    );
    assert_eq!(
        source.clone().take(-2).toJson().unwrap(),
        r#"{"8":1,"9":7}"#
    );
    assert_eq!(source.clone().take(0).toJson().unwrap(), "[]");
    assert_eq!(
        source.clone().chunk(2).toJson().unwrap(),
        r#"[{"4":9,"5":3},{"8":1,"9":7}]"#
    );
    assert!(matches!(
        source.sliding(0, 1),
        Err(CollectionError::InvalidArgument)
    ));
    assert_eq!(source.sliding(3, 2).unwrap().count(), 1);
    assert!(matches!(
        source.sole(),
        Err(CollectionError::MultipleItemsFound)
    ));
    assert!(matches!(
        Collection::<i32>::new().firstOrFail(),
        Err(CollectionError::ItemNotFound)
    ));
    assert_eq!(source.soleWhere(|value, _| *value < 3).unwrap(), &1);

    fn adapt<'a>(items: impl Enumerable<'a, Item = i64>) -> Collection<i64> {
        items
            .map(|item, _| item * 2)
            .filter(|item, _| *item < 10)
            .values()
            .take(1)
            .collect()
            .unwrap()
    }
    assert_eq!(adapt(source).toJson().unwrap(), "[6]");
}

#[test]
fn merging_and_json_round_trip_keep_php_array_shape() {
    let left = Collection::fromEntries([(ArrayKey::from(7), "old"), ("keep".into(), "left")]);
    let right = Collection::fromEntries([(ArrayKey::from(7), "new"), ("keep".into(), "right")]);
    assert_eq!(
        left.clone().merge(right.clone()).toJson().unwrap(),
        r#"{"0":"old","keep":"right","1":"new"}"#
    );
    assert_eq!(
        left.union(right).toJson().unwrap(),
        r#"{"7":"old","keep":"left"}"#
    );
    let decoded: Collection<String> =
        serde_json::from_str(r#"{"z":"first","2":"number","02":"string","z":"replacement"}"#)
            .unwrap();
    assert_eq!(
        decoded.toJson().unwrap(),
        r#"{"z":"replacement","2":"number","02":"string"}"#
    );
    let sequential: Collection<i32> = serde_json::from_str("[1,2,3]").unwrap();
    assert_eq!(sequential.toArray().unwrap(), json!([1, 2, 3]));
}

#[test]
fn nested_nonfinite_values_fail_serialization_without_repeating_application_serializer() {
    struct Custom<'a>(&'a Cell<usize>);
    impl Serialize for Custom<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.0.set(self.0.get() + 1);
            vec![Some(f64::NAN)].serialize(serializer)
        }
    }
    let calls = Cell::new(0);
    let values = Collection::make([Custom(&calls)]);
    assert_eq!(calls.get(), 0);
    assert!(matches!(
        values.toJson(),
        Err(CollectionError::Serialization(_))
    ));
    assert_eq!(calls.get(), 1);
    assert!(serde_json::to_string(&values).is_err());
    assert_eq!(calls.get(), 2);
}
