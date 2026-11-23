use std::{
    cell::{Cell, RefCell},
    io, iter,
    time::{Duration, Instant},
};

use openrust::support::{
    ArrayKey, Collection, CollectionError, CollectionResult, Enumerable, LazyCollection,
};

#[test]
fn pipelines_open_only_when_polled_and_replay_without_overreading() {
    let opens = Cell::new(0);
    let reads = Cell::new(0);
    let tapped = RefCell::new(Vec::new());
    let values = LazyCollection::from_entries(|| {
        opens.set(opens.get() + 1);
        (1..).map(|value| {
            reads.set(reads.get() + 1);
            (value * 3, value)
        })
    })
    .filter(|value, _| value % 2 == 0)
    .map(|value, _| value * 10)
    .tap_each(|value, key| tapped.borrow_mut().push((key.clone(), *value)));

    let selected = values.clone().take(2);
    let mut iterator = selected.get_iterator();
    assert_eq!(opens.get(), 0);
    assert_eq!(reads.get(), 0);
    assert!(tapped.borrow().is_empty());
    assert_eq!(iterator.next().unwrap().unwrap(), (6.into(), 20));
    assert_eq!(iterator.next().unwrap().unwrap(), (12.into(), 40));
    assert!(iterator.next().is_none());
    assert_eq!(reads.get(), 4);
    assert_eq!(
        selected.collect().unwrap().to_json().unwrap(),
        r#"{"6":20,"12":40}"#
    );
    assert_eq!(opens.get(), 2);
    assert_eq!(reads.get(), 8);
    assert_eq!(
        tapped.borrow().as_slice(),
        &[
            (6.into(), 20),
            (12.into(), 40),
            (6.into(), 20),
            (12.into(), 40)
        ]
    );

    assert!(values.take(0).all().unwrap().is_empty());
    assert_eq!(reads.get(), 8);
}

#[test]
fn remembered_runners_replay_positions_including_duplicate_keys() {
    let opens = Cell::new(0);
    let reads = Cell::new(0);
    let values = LazyCollection::from_entries(|| {
        opens.set(opens.get() + 1);
        (1..=4).map(|value| {
            reads.set(reads.get() + 1);
            ("same", value)
        })
    })
    .remember();
    assert_eq!(opens.get(), 1);
    assert_eq!(reads.get(), 0);

    let mut first = values.get_iterator();
    let mut second = (&values).into_iter();
    assert_eq!(first.next().unwrap().unwrap(), ("same".into(), 1));
    assert_eq!(second.next().unwrap().unwrap(), ("same".into(), 1));
    assert_eq!(second.next().unwrap().unwrap(), ("same".into(), 2));
    assert_eq!(second.next().unwrap().unwrap(), ("same".into(), 3));
    assert_eq!(first.next().unwrap().unwrap(), ("same".into(), 2));
    assert_eq!(reads.get(), 3);
    drop(second);
    assert_eq!(
        values.clone().values().all().unwrap(),
        vec![(0.into(), 1), (1.into(), 2), (2.into(), 3), (3.into(), 4)]
    );
    assert_eq!(values.all().unwrap(), vec![("same".into(), 4)]);
    assert_eq!(opens.get(), 1);
    assert_eq!(reads.get(), 4);
}

fn source_error() -> CollectionError {
    CollectionError::Source(Box::new(io::Error::other("source stopped")))
}

#[test]
fn errors_are_terminal_preserved_by_adapters_and_replayed_from_cache() {
    use std::error::Error as _;

    let reads = Cell::new(0);
    let failing = LazyCollection::try_make(|| {
        [Ok(1), Err(source_error()), Ok(3)]
            .into_iter()
            .inspect(|_| {
                reads.set(reads.get() + 1);
            })
    });
    let skipped = failing.clone().skip(10).filter(|_, _| false).take(-2);
    let mut iterator = skipped.get_iterator();
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
    assert_eq!(reads.get(), 2);

    let remembered = failing.remember();
    assert_eq!(remembered.first().unwrap(), Some(1));
    let left = LazyCollection::collect(&remembered).unwrap_err();
    let right = remembered.all().unwrap_err();
    let (CollectionError::Shared(left), CollectionError::Shared(right)) = (left, right) else {
        panic!("expected the remembered source failure");
    };
    assert!(std::sync::Arc::ptr_eq(&left, &right));
    assert_eq!(left.source().unwrap().to_string(), "source stopped");
    assert_eq!(reads.get(), 4);
}

#[test]
fn keyed_tail_chunks_and_eager_conversion_follow_collection_rules() {
    let values = LazyCollection::from_entries(|| [("1", 1), ("05", 2), ("1", 3), ("last", 4)]);
    assert_eq!(
        values.clone().take(-2).all().unwrap(),
        vec![(1.into(), 3), ("last".into(), 4)]
    );
    let chunks = values.clone().chunk(2, true).collect().unwrap();
    assert_eq!(
        chunks.to_json().unwrap(),
        r#"[{"1":1,"05":2},{"1":3,"last":4}]"#
    );
    let chunks = values.clone().chunk(3, false).collect().unwrap();
    assert_eq!(chunks.to_json().unwrap(), "[[1,2,3],[4]]");
    assert_eq!(values.count().unwrap(), 4);
    assert_eq!(values.eager().unwrap().count().unwrap(), 3);

    let reads = Cell::new(0);
    let values = LazyCollection::make(|| (1..).inspect(|_| reads.set(reads.get() + 1)));
    let prefix = values
        .clone()
        .skip_until(|value, _| *value >= 2)
        .take_until(|value, _| *value >= 4);
    assert_eq!(prefix.all().unwrap(), vec![(1.into(), 2), (2.into(), 3)]);
    assert_eq!(reads.get(), 4);
    assert_eq!(values.first_where(|value, _| *value == 2).unwrap(), Some(2));

    fn first_two<'source>(values: impl Enumerable<'source, Item = i32>) -> Vec<(ArrayKey, i32)> {
        values.take(2).map(|value, _| value * 2).all().unwrap()
    }
    assert_eq!(
        first_two(Collection::make(1..=3).lazy()),
        first_two(Collection::make(1..=3))
    );
}

#[test]
fn dropping_short_circuited_readers_closes_the_source() {
    struct Reader<'a>(&'a Cell<usize>);
    impl Drop for Reader<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let dropped = Cell::new(0);
    let values = LazyCollection::make(|| {
        let reader = Reader(&dropped);
        iter::from_fn(move || {
            let _reader = &reader;
            Some(1)
        })
    });
    assert_eq!(values.clone().take(1).first().unwrap(), Some(1));
    assert_eq!(dropped.get(), 1);
    let remembered = values.remember();
    assert_eq!(remembered.first().unwrap(), Some(1));
    assert_eq!(dropped.get(), 1);
    drop(remembered);
    assert_eq!(dropped.get(), 2);
}

#[test]
fn deadlines_and_heartbeats_execute_only_during_traversal() {
    let opens = Cell::new(0);
    let notifications = Cell::new(0);
    let beats = Cell::new(0);
    let values = LazyCollection::make(|| {
        opens.set(opens.get() + 1);
        1..=3
    });
    let expired = values
        .clone()
        .take_until_timeout_with(Instant::now(), |previous| {
            assert!(previous.is_none());
            notifications.set(notifications.get() + 1);
        });
    let mut iterator = expired.get_iterator();
    assert_eq!(notifications.get(), 0);
    assert!(iterator.next().is_none());
    assert!(iterator.next().is_none());
    assert_eq!(notifications.get(), 1);
    assert_eq!(opens.get(), 0);

    let values = values
        .with_heartbeat(Duration::ZERO, || beats.set(beats.get() + 1))
        .throttle(Duration::from_millis(2));
    let started = Instant::now();
    assert_eq!(values.take(2).count().unwrap(), 2);
    assert!(started.elapsed() >= Duration::from_millis(2));
    assert_eq!(beats.get(), 2);
}

#[test]
fn chunk_while_reuses_the_boundary_item_and_preserves_keys() {
    let reads = Cell::new(0);
    let groups = LazyCollection::from_entries(|| {
        [("first", 'A'), ("again", 'A'), ("next", 'B'), ("last", 'C')]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    })
    .chunk_while(|value, _, group| group.last() == Some(value));
    assert_eq!(reads.get(), 0);
    let mut iterator = groups.get_iterator();
    let (_, first) = iterator.next().unwrap().unwrap();
    assert_eq!(first.to_json().unwrap(), r#"{"first":"A","again":"A"}"#);
    assert_eq!(reads.get(), 3);
    let (_, second) = iterator.next().unwrap().unwrap();
    assert_eq!(second.to_json().unwrap(), r#"{"next":"B"}"#);
    assert_eq!(reads.get(), 4);
    let (_, last) = iterator.next().unwrap().unwrap();
    assert_eq!(last.to_json().unwrap(), r#"{"last":"C"}"#);
    assert!(iterator.next().is_none());
    assert_eq!(groups.count().unwrap(), 3);
}

#[test]
fn sliding_preserves_key_order_and_pulls_only_complete_windows() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("x", 1), ("x", 2), ("y", 3), ("z", 4), ("p", 5), ("q", 6)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let overlap = source.clone().sliding(2, 1).unwrap();
    assert_eq!(reads.get(), 0);
    let mut iterator = overlap.get_iterator();
    let (_, first) = iterator.next().unwrap().unwrap();
    assert_eq!(first.to_json().unwrap(), r#"{"x":2,"y":3}"#);
    assert_eq!(reads.get(), 3);
    assert_eq!(
        iterator.next().unwrap().unwrap().1.to_json().unwrap(),
        r#"{"y":3,"z":4}"#
    );
    assert_eq!(reads.get(), 4);
    let gapped = source.clone().sliding(2, 3).unwrap().all().unwrap();
    assert_eq!(gapped.len(), 2);
    assert_eq!(gapped[1].1.to_json().unwrap(), r#"{"p":5,"q":6}"#);
    let short = source.clone().sliding(6, 1).unwrap();
    assert_eq!(short.count().unwrap(), 0);
    assert!(matches!(
        source.clone().sliding(0, 1),
        Err(CollectionError::InvalidArgument)
    ));
    assert!(matches!(
        source.sliding(2, 0),
        Err(CollectionError::InvalidArgument)
    ));
}

#[test]
fn lazy_grouping_propagates_errors_during_a_group_or_a_window_gap() {
    let source = LazyCollection::try_make(|| [Ok(1), Ok(2), Err(source_error()), Ok(4)]);
    let chunk = source.clone().chunk_while(|_, _, _| true);
    let mut groups = chunk.get_iterator();
    assert!(matches!(
        groups.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(groups.next().is_none());

    let sliding = source.sliding(2, 3).unwrap();
    let mut windows = sliding.get_iterator();
    assert!(matches!(windows.next(), Some(Ok(_))));
    assert!(matches!(
        windows.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(windows.next().is_none());
}

#[test]
fn lazy_slice_and_nth_preserve_keys_or_reindex_positions() {
    let reads = Cell::new(0);
    let readings = LazyCollection::from_entries(|| {
        [("first", 4), ("same", 7), ("same", 8), ("last", 11)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let selected = readings.clone().nth(2, 1).unwrap().take(1);
    assert_eq!(reads.get(), 0);
    assert_eq!(selected.all().unwrap(), vec![(0.into(), 7)]);
    assert_eq!(reads.get(), 2);
    assert_eq!(
        readings.clone().slice(1, Some(2)).values().all().unwrap(),
        vec![(0.into(), 7), (1.into(), 8)]
    );
    assert_eq!(
        readings.clone().slice(0, Some(-1)).all().unwrap(),
        vec![("first".into(), 4), ("same".into(), 8)]
    );
    assert_eq!(
        readings.clone().nth(2, -3).unwrap().all().unwrap(),
        vec![(0.into(), 4), (1.into(), 11)]
    );
    assert!(matches!(
        readings.nth(0, 0),
        Err(CollectionError::InvalidArgument)
    ));

    let failing = LazyCollection::try_make(|| [Ok(1), Err(source_error()), Ok(3)]);
    let sampled = failing.clone().nth(3, 0).unwrap();
    let mut iterator = sampled.get_iterator();
    assert_eq!(iterator.next().unwrap().unwrap(), (0.into(), 1));
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
    assert!(matches!(
        failing.nth(1, -1).unwrap().first(),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_keys_and_concat_preserve_occurrences_and_defer_the_second_source() {
    let opens = Cell::new(0);
    let keys = LazyCollection::from_entries(|| [("same", 1), ("same", 2)]).keys();
    let suffix = LazyCollection::from_entries(|| {
        opens.set(opens.get() + 1);
        [("tail", 3)]
    });
    let joined = LazyCollection::from_entries(|| [("a", 1), ("a", 2)]).concat(suffix);
    assert_eq!(
        keys.all().unwrap(),
        vec![(0.into(), "same".into()), (1.into(), "same".into())]
    );
    assert_eq!(
        joined.clone().take(2).all().unwrap(),
        vec![(0.into(), 1), (1.into(), 2)]
    );
    assert_eq!(opens.get(), 0);
    assert_eq!(
        joined.all().unwrap(),
        vec![(0.into(), 1), (1.into(), 2), (2.into(), 3)]
    );
    assert_eq!(opens.get(), 1);
    let failing = LazyCollection::try_make(|| [Ok(1), Err(source_error())]);
    let result = failing
        .concat(LazyCollection::make(|| {
            opens.set(opens.get() + 1);
            [4]
        }))
        .all();
    assert!(matches!(result, Err(CollectionError::Source(_))));
    assert_eq!(opens.get(), 1);
}

#[test]
fn chunk_by_projects_typed_values_and_keys() {
    let readings = LazyCollection::from_entries(|| [("a", 4), ("b", 7), ("c", 11)]);
    let groups = readings
        .chunk_by(|value, key| {
            assert!(matches!(key, ArrayKey::String(_)));
            value / 10
        })
        .all()
        .unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].1.to_json().unwrap(), r#"{"a":4,"b":7}"#);
    assert_eq!(groups[1].1.to_json().unwrap(), r#"{"c":11}"#);
}

#[test]
fn lazy_collapse_preserves_or_reindexes_keys_without_opening_later_groups() {
    let opened = Cell::new(0);
    let groups = LazyCollection::make(|| {
        [
            LazyCollection::from_entries(|| [("same", 1), ("first", 2)]),
            LazyCollection::from_entries(|| {
                opened.set(opened.get() + 1);
                [("same", 3), ("last", 4)]
            }),
        ]
    });
    let keyed = groups.clone().collapse_with_keys();
    let mut iterator = keyed.get_iterator();
    assert_eq!(iterator.next().unwrap().unwrap(), ("same".into(), 1));
    assert_eq!(iterator.next().unwrap().unwrap(), ("first".into(), 2));
    assert_eq!(opened.get(), 0);
    assert_eq!(iterator.next().unwrap().unwrap(), ("same".into(), 3));
    assert_eq!(opened.get(), 1);
    assert_eq!(
        keyed.collect().unwrap().to_json().unwrap(),
        r#"{"same":3,"first":2,"last":4}"#
    );
    assert_eq!(
        groups.collapse().collect().unwrap().to_json().unwrap(),
        "[1,2,3,4]"
    );
}

#[test]
fn lazy_flat_map_flattens_one_level_and_propagates_nested_failures() {
    let calls = Cell::new(0);
    let mapped = LazyCollection::make(|| [1, 2]).flat_map(|value, _| {
        calls.set(calls.get() + 1);
        Collection::from_entries([("single", *value), ("double", value * 2)])
    });
    assert_eq!(mapped.clone().take(1).all().unwrap(), vec![(0.into(), 1)]);
    assert_eq!(calls.get(), 1);
    assert_eq!(mapped.collect().unwrap().to_json().unwrap(), "[1,2,2,4]");
    assert_eq!(
        LazyCollection::make(|| [[1, 2], [3, 4]])
            .collapse()
            .count()
            .unwrap(),
        4
    );
    let nested = LazyCollection::make(|| {
        [LazyCollection::try_make(|| {
            [Ok(1), Err(source_error()), Ok(3)]
        })]
    });
    let flattened = nested.collapse();
    let mut iterator = flattened.get_iterator();
    assert_eq!(iterator.next().unwrap().unwrap(), (0.into(), 1));
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
}

#[test]
fn lazy_flatten_respects_depth_and_keeps_scalar_values() {
    use serde_json::json;

    let read = Cell::new(0);
    let source = LazyCollection::make(|| {
        [json!({"a": [1, [2]], "b": 3}), json!(null)]
            .into_iter()
            .inspect(|_| read.set(read.get() + 1))
    });
    let one = source.clone().flatten(Some(1));
    assert_eq!(one.clone().take(1).first().unwrap(), Some(json!([1, [2]])));
    assert_eq!(read.get(), 1);
    assert_eq!(
        one.collect().unwrap().to_json().unwrap(),
        "[[1,[2]],3,null]"
    );
    assert_eq!(
        source.flatten(None).collect().unwrap().to_json().unwrap(),
        "[1,2,3,null]"
    );

    let failed = LazyCollection::try_make(|| [Ok(json!([1])), Err(source_error())]).flatten(None);
    let mut iterator = failed.get_iterator();
    assert_eq!(iterator.next().unwrap().unwrap(), (0.into(), json!(1)));
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
}

#[test]
fn lazy_zip_pads_to_the_longest_input_without_reading_ahead() {
    let reads = Cell::new(0);
    let left = LazyCollection::from_entries(|| [("id", 1)]);
    let right = LazyCollection::make(|| {
        ["a", "b"]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let zipped = left.zip(right);
    assert_eq!(reads.get(), 0);
    let mut iterator = zipped.get_iterator();
    assert_eq!(
        iterator.next().unwrap().unwrap(),
        (0.into(), (Some(1), Some("a")))
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(
        iterator.next().unwrap().unwrap(),
        (1.into(), (None, Some("b")))
    );
    assert!(iterator.next().is_none());

    let multiple = LazyCollection::from([1, 2])
        .zip_many([vec![3], vec![4, 5, 6]])
        .all()
        .unwrap();
    assert_eq!(
        multiple,
        vec![
            (0.into(), (Some(1), vec![Some(3), Some(4)])),
            (1.into(), (Some(2), vec![None, Some(5)])),
            (2.into(), (None, vec![None, Some(6)])),
        ]
    );
}

#[test]
fn lazy_combine_reuses_values_as_keys_and_reports_mismatches_on_pull() {
    let keyed = LazyCollection::from(["09", "8", "8"]).combine([5, 6, 7]);
    assert_eq!(
        keyed.clone().values().all().unwrap(),
        vec![(0.into(), 5), (1.into(), 6), (2.into(), 7)]
    );
    assert_eq!(keyed.all().unwrap(), vec![("09".into(), 5), (8.into(), 7)]);
    for invalid in [
        LazyCollection::from(["a", "b"]).combine([1]),
        LazyCollection::from(["a"]).combine([1, 2]),
    ] {
        let mut iterator = invalid.get_iterator();
        assert_eq!(iterator.next().unwrap().unwrap(), ("a".into(), 1));
        assert!(matches!(
            iterator.next(),
            Some(Err(CollectionError::InvalidArgument))
        ));
        assert!(iterator.next().is_none());
    }
}

#[test]
fn lazy_pairing_propagates_source_errors_without_pulling_later_columns() {
    let reads = Cell::new(0);
    let left = LazyCollection::try_make(|| [Ok("key"), Err(source_error())]);
    let right = LazyCollection::make(|| [1, 2].into_iter().inspect(|_| reads.set(reads.get() + 1)));
    let zip = left.clone().zip(right.clone());
    let mut zipped = zip.get_iterator();
    assert!(matches!(zipped.next(), Some(Ok(_))));
    assert!(matches!(
        zipped.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(zipped.next().is_none());
    assert_eq!(reads.get(), 1);

    let combined = left.combine(right);
    let mut pairs = combined.get_iterator();
    assert!(matches!(pairs.next(), Some(Ok(_))));
    assert!(matches!(
        pairs.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert_eq!(reads.get(), 2);
}

#[test]
fn lazy_unique_keeps_first_keys_and_restarts_each_traversal() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("a", 10), ("b", 11), ("c", 21), ("d", 10)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let unique = source.clone().unique_by(|value, _| value / 10);
    assert_eq!(reads.get(), 0);
    assert_eq!(
        unique.clone().take(1).all().unwrap(),
        vec![("a".into(), 10)]
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(
        LazyCollection::all(&unique).unwrap(),
        vec![("a".into(), 10), ("c".into(), 21)]
    );
    assert_eq!(
        LazyCollection::all(&unique).unwrap(),
        vec![("a".into(), 10), ("c".into(), 21)]
    );
    assert_eq!(source.unique_strict().count().unwrap(), 3);

    let failing = LazyCollection::try_make(|| [Ok(10), Ok(10), Err(source_error())]);
    let distinct = failing.unique();
    let mut iterator = distinct.get_iterator();
    assert_eq!(iterator.next().unwrap().unwrap(), (0.into(), 10));
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
}

#[test]
fn lazy_cardinality_checks_short_circuit_and_follow_keyed_sole_semantics() {
    let reads = Cell::new(0);
    let repeated = LazyCollection::from_entries(|| {
        [("a", 1), ("a", 2), ("b", 3)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(repeated.sole().unwrap(), 2);
    assert_eq!(reads.get(), 2);
    assert!(!repeated.has_sole().unwrap());
    assert!(repeated.has_many().unwrap());
    assert!(repeated.has_sole_where(|value, _| *value == 2).unwrap());
    assert_eq!(
        repeated
            .sole_where(|_, key| key == &ArrayKey::from("b"))
            .unwrap(),
        3
    );
    assert!(repeated.has_many_where(|value, _| *value < 3).unwrap());
    assert!(matches!(
        repeated.sole_where(|value, _| *value > 1),
        Err(CollectionError::MultipleItemsFound)
    ));
    assert!(matches!(
        repeated.sole_where(|value, _| *value > 4),
        Err(CollectionError::ItemNotFound)
    ));
    assert!(LazyCollection::<i32>::empty().is_empty().unwrap());
}

#[test]
fn lazy_first_and_contains_stop_on_the_first_match_and_propagate_errors() {
    let source = LazyCollection::try_make(|| [Ok(10), Err(source_error()), Ok(30)]);
    assert!(source.is_not_empty().unwrap());
    assert!(source.contains_strict(&10).unwrap());
    assert!(source.contains_where(|value, _| *value == 10).unwrap());
    assert_eq!(source.first_or_fail().unwrap(), 10);
    assert_eq!(
        source.first_or_fail_where(|value, _| *value == 10).unwrap(),
        10
    );
    assert!(matches!(
        source.first_or_fail_where(|value, _| *value == 30),
        Err(CollectionError::Source(_))
    ));
    assert!(matches!(source.has_sole(), Err(CollectionError::Source(_))));
    assert!(matches!(source.has_many(), Err(CollectionError::Source(_))));
    assert!(matches!(
        LazyCollection::<i32>::empty().first_or_fail(),
        Err(CollectionError::ItemNotFound)
    ));
}

#[test]
fn lazy_ordering_defers_materialization_preserves_keys_and_is_stable() {
    let reads = Cell::new(0);
    let entries = LazyCollection::from_entries(|| {
        [("b", 3), ("a", 2), ("b", 1), ("c", 2)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let sorted = entries.clone().sort();
    assert_eq!(reads.get(), 0);
    let mut iterator = sorted.get_iterator();
    assert_eq!(reads.get(), 0);
    assert_eq!(iterator.next().unwrap().unwrap(), ("b".into(), 1));
    assert_eq!(reads.get(), 4);
    assert_eq!(iterator.next().unwrap().unwrap(), ("a".into(), 2));
    assert_eq!(iterator.next().unwrap().unwrap(), ("c".into(), 2));
    assert!(iterator.next().is_none());
    assert_eq!(entries.clone().sort_desc().first().unwrap(), Some(2));
    assert_eq!(
        entries.clone().sort_using(|a, b| b.cmp(a)).first().unwrap(),
        Some(2)
    );
    assert_eq!(
        entries
            .clone()
            .sort_by(|value, _| value % 2)
            .first()
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        entries
            .clone()
            .sort_by_desc(|value, _| value % 2)
            .first()
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        entries
            .clone()
            .sort_keys()
            .get_iterator()
            .next()
            .unwrap()
            .unwrap(),
        ("a".into(), 2)
    );
    assert_eq!(
        entries
            .clone()
            .sort_keys_desc()
            .get_iterator()
            .next()
            .unwrap()
            .unwrap(),
        ("c".into(), 2)
    );
    assert_eq!(
        entries
            .clone()
            .sort_keys_using(|a, b| b.cmp(a))
            .all()
            .unwrap(),
        vec![("c".into(), 2), ("b".into(), 1), ("a".into(), 2)]
    );
    assert_eq!(
        entries.reverse().all().unwrap(),
        vec![("c".into(), 2), ("a".into(), 2), ("b".into(), 1)]
    );
}

#[test]
fn lazy_duplicates_use_final_keys_and_projected_values() {
    let entries = LazyCollection::from_entries(|| [("x", 3), ("y", 4), ("x", 4), ("z", 9)]);
    assert_eq!(
        entries.clone().duplicates().all().unwrap(),
        vec![("y".into(), 4)]
    );
    assert_eq!(
        entries.clone().duplicates_strict().all().unwrap(),
        vec![("y".into(), 4)]
    );
    assert_eq!(
        entries.duplicates_by(|value, _| value % 2).all().unwrap(),
        vec![("y".into(), 0)]
    );
}

#[test]
fn lazy_ordering_reports_source_errors_before_comparing_or_emitting() {
    let compares = Cell::new(0);
    let source = LazyCollection::try_make(|| [Ok(3), Ok(1), Err(source_error()), Ok(2)]);
    let sorted = source.clone().sort_using(|a, b| {
        compares.set(compares.get() + 1);
        a.cmp(b)
    });
    let mut iterator = sorted.get_iterator();
    assert_eq!(compares.get(), 0);
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
    assert_eq!(compares.get(), 0);
    assert!(matches!(
        source.duplicates().first(),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_aggregates_count_every_occurrence_and_keep_typed_empty_defaults() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("x", 3), ("y", 5), ("x", 7)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(source.sum().unwrap(), 15);
    assert_eq!(reads.get(), 3);
    assert_eq!(source.sum_by(|value, _| value * value).unwrap(), 83);
    assert_eq!(source.average().unwrap(), Some(5.0));
    assert_eq!(
        source
            .avg_by(|value, _| (*value > 3).then_some(f64::from(*value)))
            .unwrap(),
        Some(6.0)
    );
    assert_eq!(source.min().unwrap(), Some(3));
    assert_eq!(source.max().unwrap(), Some(7));
    assert_eq!(source.min_by(|value, _| -*value).unwrap(), Some(-7));
    assert_eq!(source.max_by(|value, _| -*value).unwrap(), Some(-3));
    assert_eq!(
        source
            .reduce_spread((0, 0), |sum, n, value, _| (sum + value, n + 1))
            .unwrap(),
        (15, 3)
    );

    let empty = LazyCollection::<i32>::empty();
    assert_eq!(empty.sum().unwrap(), 0);
    assert_eq!(empty.avg().unwrap(), None);
    assert_eq!(empty.min().unwrap(), None);
    assert_eq!(empty.max().unwrap(), None);
    assert_eq!(empty.median().unwrap(), None);
    assert_eq!(empty.mode().unwrap(), None);
    assert!(empty.every(|_, _| false).unwrap());
}

#[test]
fn lazy_count_by_counts_repeated_keys_and_median_and_mode_materialize() {
    let source = LazyCollection::from_entries(|| [("x", 90), ("y", 5), ("x", 7), ("z", 5)]);
    let counts = source
        .clone()
        .count_by(|value, _| if *value == 5 { "05" } else { "5" });
    assert_eq!(counts.all().unwrap(), vec![(5.into(), 2), ("05".into(), 2)]);
    assert_eq!(source.median().unwrap(), Some(5.0));
    assert_eq!(
        source
            .median_by(|value, _| (*value != 5).then_some(f64::from(*value)))
            .unwrap(),
        Some(7.0)
    );
    assert_eq!(source.mode().unwrap(), Some(vec![5]));
    assert_eq!(
        LazyCollection::from([7, 5, 7, 5]).mode().unwrap(),
        Some(vec![7, 5])
    );
}

#[test]
fn lazy_aggregate_failures_propagate_and_every_short_circuits() {
    let failing = LazyCollection::try_make(|| [Ok(1), Err(source_error()), Ok(3)]);
    assert!(!failing.every(|value, _| *value > 3).unwrap());
    assert!(matches!(
        failing.every(|_, _| true),
        Err(CollectionError::Source(_))
    ));
    assert!(matches!(failing.sum(), Err(CollectionError::Source(_))));
    assert!(matches!(failing.median(), Err(CollectionError::Source(_))));
    let counts = failing.count_by(|value, _| *value);
    let mut iterator = counts.get_iterator();
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
}

#[test]
fn lazy_set_comparisons_materialize_both_inputs_and_preserve_keys() {
    let left = LazyCollection::from_entries(|| [("a", 99), ("a", 2), ("B", 3), ("c", 7)]);
    let right = LazyCollection::from_entries(|| [("a", 2), ("b", 3), ("d", 7)]);
    let by_key = |a: &ArrayKey, b: &ArrayKey| {
        a.to_string()
            .to_lowercase()
            .cmp(&b.to_string().to_lowercase())
    };
    assert!(left.clone().diff(right.clone()).all().unwrap().is_empty());
    assert_eq!(
        left.clone().diff_assoc(right.clone()).all().unwrap(),
        vec![("B".into(), 3), ("c".into(), 7)]
    );
    assert_eq!(
        left.clone().intersect_assoc(right.clone()).all().unwrap(),
        vec![("a".into(), 2)]
    );
    assert_eq!(
        left.clone()
            .intersect_assoc_using(right.clone(), by_key)
            .count()
            .unwrap(),
        2
    );
    assert_eq!(
        left.clone()
            .diff_assoc_using(right.clone(), by_key)
            .count()
            .unwrap(),
        1
    );
    assert_eq!(
        left.clone()
            .diff_keys_using(right.clone(), by_key)
            .count()
            .unwrap(),
        1
    );
    assert_eq!(left.clone().diff_keys(right.clone()).count().unwrap(), 2);
    assert_eq!(
        left.clone()
            .intersect_by_keys(right.clone())
            .count()
            .unwrap(),
        1
    );
    assert_eq!(left.clone().intersect(right).count().unwrap(), 3);
    let parity = |a: &i32, b: &i32| (a % 2).cmp(&(b % 2));
    assert_eq!(
        left.clone().intersect_using([3], parity).count().unwrap(),
        2
    );
    assert_eq!(left.diff_using([3], parity).count().unwrap(), 1);
}

#[test]
fn lazy_merge_and_union_match_collection_key_rules() {
    let left = LazyCollection::from_entries(|| [(ArrayKey::from(0), 2), ("x".into(), 3)]);
    let right =
        LazyCollection::from_entries(|| [(ArrayKey::from(0), 4), ("x".into(), 5), ("y".into(), 6)]);
    assert_eq!(
        left.clone().merge(right.clone()).all().unwrap(),
        vec![
            (0.into(), 2),
            ("x".into(), 5),
            (1.into(), 4),
            ("y".into(), 6)
        ]
    );
    assert_eq!(
        left.union(right).all().unwrap(),
        vec![(0.into(), 2), ("x".into(), 3), ("y".into(), 6)]
    );
}

#[test]
fn lazy_replace_applies_each_key_once_and_defers_reading_primary_entries() {
    let reads = Cell::new(0);
    let left = LazyCollection::from_entries(|| {
        [("a", 1), ("a", 2), ("b", 3)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let right = LazyCollection::from_entries(|| [("a", 9), ("z", 10)]);
    let replaced = left.clone().replace(right);
    let mut values = replaced.get_iterator();
    assert_eq!(reads.get(), 0);
    assert_eq!(values.next().unwrap().unwrap(), ("a".into(), 9));
    assert_eq!(reads.get(), 1);
    assert_eq!(values.next().unwrap().unwrap(), ("a".into(), 2));
    assert_eq!(values.next().unwrap().unwrap(), ("b".into(), 3));
    assert_eq!(values.next().unwrap().unwrap(), ("z".into(), 10));
    assert!(values.next().is_none());
    let failing = LazyCollection::try_make(|| [Ok(4), Err(source_error())]);
    assert!(matches!(
        left.clone().replace(failing.clone()).first(),
        Err(CollectionError::Source(_))
    ));
    assert_eq!(reads.get(), 3);
    assert!(matches!(
        left.merge(failing).first(),
        Err(CollectionError::Source(_))
    ));
    assert_eq!(reads.get(), 6);
}

#[test]
fn lazy_only_stops_after_first_match_for_every_key_but_except_materializes() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("a", 1), ("a", 2), ("b", 3), ("c", 4)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(
        source.clone().only(["b", "a", "a"]).all().unwrap(),
        vec![("a".into(), 1), ("b".into(), 3)]
    );
    assert_eq!(reads.get(), 3);
    assert_eq!(
        source.except(["b"]).all().unwrap(),
        vec![("a".into(), 2), ("c".into(), 4)]
    );
    assert_eq!(reads.get(), 7);
    let failing = LazyCollection::try_from_entries(|| [Ok(("a", 1)), Err(source_error())]);
    assert_eq!(
        failing.clone().only(["a"]).all().unwrap(),
        vec![("a".into(), 1)]
    );
    assert!(matches!(
        failing.only(["b"]).first(),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_keyed_projections_preserve_occurrences_and_defer_the_next_source_item() {
    let projected = Cell::new(0);
    let source = LazyCollection::make(|| [("8", 1), ("8", 2), ("09", 3)]);
    assert_eq!(
        source
            .clone()
            .key_by(|pair, _| pair.0)
            .collect()
            .unwrap()
            .count(),
        2
    );
    assert_eq!(
        source.clone().pluck(|pair| pair.1).all().unwrap(),
        vec![(0.into(), 1), (1.into(), 2), (2.into(), 3)]
    );
    assert_eq!(
        source
            .clone()
            .pluck_with_keys(|pair| pair.0, |pair| pair.1)
            .values()
            .count()
            .unwrap(),
        3
    );
    let mapped = source.map_with_keys(|pair, _| {
        projected.set(projected.get() + 1);
        [(pair.0, pair.1), ("extra", 2 * pair.1)]
    });
    assert_eq!(
        mapped.clone().take(2).all().unwrap(),
        vec![(8.into(), 1), ("extra".into(), 2)]
    );
    assert_eq!(projected.get(), 1);
    assert_eq!(
        mapped.all().unwrap(),
        vec![(8.into(), 2), ("extra".into(), 6), ("09".into(), 3)]
    );
}

#[test]
fn lazy_groups_materialize_with_key_coalescing_before_projection() {
    let source = LazyCollection::from_entries(|| [("a", 1), ("a", 4), ("b", 2), ("c", 3)]);
    let dictionary = source
        .clone()
        .map_to_dictionary(|value, _| (value % 2, *value));
    assert_eq!(
        dictionary.all().unwrap(),
        vec![(0.into(), vec![4, 2]), (1.into(), vec![3])]
    );
    let groups = source.clone().map_to_groups(|value, _| (value % 2, *value));
    assert_eq!(groups.first().unwrap().unwrap().to_json().unwrap(), "[4,2]");
    let keyed = source.group_by(|value, _| value % 2, true);
    assert_eq!(
        keyed.first().unwrap().unwrap().to_json().unwrap(),
        r#"{"a":4,"b":2}"#
    );
}

#[test]
fn lazy_key_lookups_stop_at_first_match_and_distinguish_normalized_keys() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("08", 1), ("8", 2), ("8", 3), ("end", 4)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(source.get(8).unwrap(), Some(2));
    assert_eq!(reads.get(), 2);
    assert_eq!(source.get("08").unwrap(), Some(1));
    assert!(source.has(8).unwrap());
    assert!(source.has_all(["08", "8", "8"]).unwrap());
    assert!(source.has_any(["missing", "08"]).unwrap());
    assert!(!source.has("missing").unwrap());
    assert!(source.has_all(std::iter::empty::<ArrayKey>()).unwrap());
    assert!(
        !LazyCollection::<i32>::empty()
            .has_all(std::iter::empty::<ArrayKey>())
            .unwrap()
    );
}

#[test]
fn lazy_neighbors_and_search_follow_occurrence_order() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("k", 1), ("k", 2), ("b", 3), ("c", 2)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(source.search(&2).unwrap(), Some("k".into()));
    assert_eq!(reads.get(), 2);
    assert_eq!(
        source
            .search_where(|_, key| key == &ArrayKey::from("c"))
            .unwrap(),
        Some("c".into())
    );
    assert_eq!(source.before(&2).unwrap(), Some(1));
    assert_eq!(source.after(&2).unwrap(), Some(3));
    assert_eq!(source.before(&1).unwrap(), None);
    assert_eq!(
        source
            .after_where(|_, key| key == &ArrayKey::from("c"))
            .unwrap(),
        None
    );
    assert_eq!(source.last().unwrap(), Some(2));
    assert_eq!(source.last_where(|value, _| *value > 2).unwrap(), Some(3));
    assert!(source.some(|value, _| *value == 1).unwrap());
    assert!(source.doesnt_contain_strict(&10).unwrap());
}

#[test]
fn lazy_lookup_reports_errors_only_if_needed_for_the_result() {
    let failing = LazyCollection::try_from_entries(|| [Ok(("first", 1)), Err(source_error())]);
    assert_eq!(failing.get("first").unwrap(), Some(1));
    assert!(failing.has_any(["first", "missing"]).unwrap());
    assert!(matches!(
        failing.has_all(["first", "missing"]),
        Err(CollectionError::Source(_))
    ));
    assert_eq!(failing.before(&1).unwrap(), None);
    assert!(matches!(failing.after(&1), Err(CollectionError::Source(_))));
    assert!(matches!(failing.last(), Err(CollectionError::Source(_))));
}

#[test]
fn lazy_filters_preserve_occurrences_and_stop_at_the_requested_match() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("low", 0), ("repeat", 2), ("repeat", 2), ("high", 4)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let matches = source.clone().where_strict(|value| *value, 2);
    assert_eq!(reads.get(), 0);
    assert_eq!(matches.clone().take(1).first().unwrap(), Some(2));
    assert_eq!(reads.get(), 2);
    assert_eq!(matches.count().unwrap(), 2);
    assert_eq!(
        source
            .clone()
            .where_in_strict(|value| *value, &[0, 4])
            .count()
            .unwrap(),
        2
    );
    assert_eq!(
        source
            .clone()
            .where_not_in_strict(|value| *value, &[2])
            .count()
            .unwrap(),
        2
    );
    assert_eq!(
        source
            .clone()
            .where_between(|value| *value, 2..=4)
            .count()
            .unwrap(),
        3
    );
    assert_eq!(
        source
            .where_not_between(|value| *value, 2..=4)
            .first()
            .unwrap(),
        Some(0)
    );

    let floats = LazyCollection::from([f64::NAN, -1.0, 0.0, 1.0, 2.0]);
    assert_eq!(
        floats
            .clone()
            .where_between(|value| *value, 0.0..=1.0)
            .count()
            .unwrap(),
        2
    );
    assert_eq!(
        floats
            .where_not_between(|value| *value, 0.0..=1.0)
            .count()
            .unwrap(),
        2
    );
}

#[test]
fn lazy_null_filters_distinguish_null_from_false_and_missing_paths() {
    use serde_json::json;
    let source = LazyCollection::from([
        json!({"profile":{"active":null}}),
        json!({"profile":{}}),
        json!({"profile":{"active":false}}),
        json!(null),
    ]);
    let missing = source.clone().where_null_at("profile.active");
    assert_eq!(missing.count().unwrap(), 3);
    assert_eq!(
        source
            .clone()
            .where_not_null_at("profile.active")
            .count()
            .unwrap(),
        1
    );
    assert_eq!(source.clone().where_null().count().unwrap(), 1);
    assert_eq!(source.where_not_null().count().unwrap(), 3);

    let typed = LazyCollection::from_entries(|| [("empty", None), ("not_empty", Some(false))]);
    assert_eq!(
        typed.clone().where_null().all().unwrap(),
        vec![("empty".into(), None)]
    );
    assert_eq!(
        typed.where_not_null().all().unwrap(),
        vec![("not_empty".into(), Some(false))]
    );
}

#[test]
fn lazy_downcasting_and_predicates_propagate_source_errors() {
    use std::any::Any;
    let source = LazyCollection::try_from_entries(|| {
        [
            Ok(("discard", Box::new(3_i32) as Box<dyn Any>)),
            Ok(("keep", Box::new(String::from("ready")) as Box<dyn Any>)),
            Err(source_error()),
        ]
    });
    let filtered = source.where_instance_of::<String>();
    let mut iterator = filtered.get_iterator();
    assert_eq!(
        iterator.next().unwrap().unwrap(),
        ("keep".into(), Box::new(String::from("ready")))
    );
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
    let source = LazyCollection::try_make(|| [Ok(1), Err(source_error())]);
    assert!(matches!(
        source.where_in(|value| *value, &[2]).first(),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_page_preserves_occurrences_and_reads_only_through_the_page() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("a", 1), ("b", 2), ("b", 3), ("c", 4), ("d", 5)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let page = source.clone().for_page(2, 2);
    assert_eq!(reads.get(), 0);
    assert_eq!(
        page.get_iterator().collect::<Result<Vec<_>, _>>().unwrap(),
        vec![("b".into(), 3), ("c".into(), 4)]
    );
    assert_eq!(reads.get(), 4);
    assert_eq!(source.clone().for_page(0, 1).first().unwrap(), Some(1));
    assert_eq!(source.for_page(usize::MAX, 0).count().unwrap(), 0);
    assert_eq!(reads.get(), 5);
}

#[test]
fn lazy_split_and_partition_preserve_their_distinct_materialization_rules() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("a", 1), ("a", 2), ("b", 3), ("c", 4), ("d", 5)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert!(matches!(
        source.clone().split(0),
        Err(CollectionError::InvalidArgument)
    ));
    assert!(matches!(
        source.clone().split_in(0),
        Err(CollectionError::InvalidArgument)
    ));
    let split = source.clone().split(3).unwrap();
    assert_eq!(reads.get(), 0);
    assert_eq!(
        split.first().unwrap().unwrap().to_json().unwrap(),
        r#"{"a":2,"b":3}"#
    );
    assert_eq!(reads.get(), 5);
    let chunks = source.clone().split_in(2).unwrap();
    assert_eq!(reads.get(), 10);
    assert_eq!(
        chunks.first().unwrap().unwrap().to_json().unwrap(),
        r#"{"a":2,"b":3,"c":4}"#
    );
    assert_eq!(reads.get(), 14);
    let (evens, odds) = source.partition(|value, _| value % 2 == 0).unwrap();
    assert_eq!(evens.to_json().unwrap(), r#"{"a":2,"c":4}"#);
    assert_eq!(odds.to_json().unwrap(), r#"{"a":1,"b":3,"d":5}"#);
}

#[test]
fn lazy_pad_retains_right_keys_and_uses_materialized_left_padding() {
    let source = LazyCollection::from_entries(|| [(ArrayKey::from(8), "x"), ("tag".into(), "y")]);
    assert_eq!(
        source.clone().pad(4, "pad").all().unwrap(),
        vec![
            (8.into(), "x"),
            ("tag".into(), "y"),
            (9.into(), "pad"),
            (10.into(), "pad")
        ]
    );
    assert_eq!(
        source.pad(-3, "pad").all().unwrap(),
        vec![(0.into(), "pad"), (1.into(), "x"), ("tag".into(), "y")]
    );
    let failing = LazyCollection::try_make(|| [Ok(1), Err(source_error())]);
    assert_eq!(failing.clone().pad(3, 0).first().unwrap(), Some(1));
    assert!(matches!(
        failing.clone().pad(-3, 0).first(),
        Err(CollectionError::Source(_))
    ));
    assert!(matches!(
        failing.partition(|_, _| true),
        Err(CollectionError::Source(_))
    ));
    let overflow = LazyCollection::from_entries(|| [(i64::MAX, "value")]).pad(2, "pad");
    let mut values = overflow.get_iterator();
    assert_eq!(values.next().unwrap().unwrap(), (i64::MAX.into(), "value"));
    assert!(matches!(
        values.next(),
        Some(Err(CollectionError::IndexOverflow))
    ));
}

#[test]
fn lazy_json_preserves_shapes_and_replays_deserialized_sources() {
    use serde_json::json;
    let reads = Cell::new(0);
    let keyed = LazyCollection::from_entries(|| {
        [
            (ArrayKey::from(2), "old"),
            ("08".into(), "keep"),
            (2.into(), "new"),
        ]
        .into_iter()
        .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(keyed.to_json().unwrap(), r#"{"2":"new","08":"keep"}"#);
    assert_eq!(reads.get(), 3);
    assert_eq!(
        keyed.json_serialize().unwrap(),
        json!({"2":"new","08":"keep"})
    );
    assert_eq!(
        keyed.to_pretty_json().unwrap(),
        "{\n  \"2\": \"new\",\n  \"08\": \"keep\"\n}"
    );
    assert_eq!(
        serde_json::to_value(&keyed).unwrap(),
        keyed.to_array().unwrap()
    );
    assert_eq!(keyed.values().to_json().unwrap(), r#"["old","keep","new"]"#);

    let decoded = LazyCollection::<i32>::from_json(r#"{"8":1,"08":2}"#).unwrap();
    assert_eq!(decoded.to_json().unwrap(), r#"{"8":1,"08":2}"#);
    assert_eq!(decoded.to_json().unwrap(), r#"{"8":1,"08":2}"#);
    assert!(LazyCollection::<i32>::from_json("7").is_err());
    let nested =
        serde_json::from_str::<LazyCollection<'_, LazyCollection<'_, i32>>>("[[1,2],[]]").unwrap();
    assert_eq!(serde_json::to_string(&nested).unwrap(), "[[1,2],[]]");
}

#[test]
fn lazy_serialization_keeps_read_errors_separate_from_encoding_errors() {
    let failing = LazyCollection::try_make(|| [Ok(1), Err(source_error())]);
    assert!(matches!(failing.to_json(), Err(CollectionError::Source(_))));
    assert!(serde_json::to_string(&failing).is_err());
    let nonfinite = LazyCollection::from([1.0, f64::NAN]);
    assert!(matches!(
        nonfinite.to_json(),
        Err(CollectionError::Serialization(_))
    ));
    assert!(matches!(
        nonfinite.to_array(),
        Err(CollectionError::Serialization(_))
    ));
}

#[test]
fn lazy_json_paths_and_join_follow_materialization_boundaries() {
    use serde_json::json;
    let read = Cell::new(0);
    let source = LazyCollection::make(|| {
        [
            json!({}),
            json!({"profile":{"active":null}}),
            json!({"profile":{"active":true}}),
        ]
        .into_iter()
        .inspect(|_| read.set(read.get() + 1))
    });
    assert_eq!(source.value("profile.active").unwrap(), Some(json!(null)));
    assert_eq!(read.get(), 2);
    let dotted = source.dot();
    assert_eq!(read.get(), 2);
    assert_eq!(dotted.get("2.profile.active").unwrap(), Some(json!(true)));
    assert_eq!(
        dotted.undot().get(2).unwrap(),
        Some(json!({"profile":{"active":true}}))
    );
    let names = LazyCollection::from_entries(|| {
        [("a", "old"), ("a", "new"), ("b", "second"), ("c", "third")]
    });
    assert_eq!(names.join(", ", " and ").unwrap(), "new, second and third");
    assert_eq!(names.implode("/").unwrap(), "new/second/third");
}

#[test]
fn lazy_times_invokes_callbacks_only_for_requested_one_based_positions() {
    let calls = RefCell::new(Vec::new());
    let source = LazyCollection::times(usize::MAX, |index| {
        calls.borrow_mut().push(index);
        index * 3
    });
    let first = source.clone().take(2);
    let mut iterator = first.get_iterator();
    assert!(calls.borrow().is_empty());
    assert_eq!(iterator.next().unwrap().unwrap(), (0.into(), 3));
    assert_eq!(iterator.next().unwrap().unwrap(), (1.into(), 6));
    assert!(iterator.next().is_none());
    assert_eq!(calls.borrow().as_slice(), &[1, 2]);
    assert_eq!(source.first().unwrap(), Some(3));
    assert_eq!(calls.borrow().as_slice(), &[1, 2, 1]);
    assert_eq!(LazyCollection::times(0, |index| index).count().unwrap(), 0);
}

#[test]
fn lazy_range_is_inclusive_in_both_directions_and_handles_integer_limits() {
    use std::num::NonZeroUsize;
    let two = NonZeroUsize::new(2).unwrap();
    let one = NonZeroUsize::MIN;
    assert_eq!(
        LazyCollection::range(2, 6, two).to_json().unwrap(),
        "[2,4,6]"
    );
    assert_eq!(
        LazyCollection::range(6, 1, two).to_json().unwrap(),
        "[6,4,2]"
    );
    assert_eq!(LazyCollection::range(9, 9, two).to_json().unwrap(), "[9]");
    assert_eq!(
        LazyCollection::range(i64::MIN, i64::MAX, one)
            .take(3)
            .unwrap()
            .unwrap(),
        vec![
            (0.into(), i64::MIN),
            (1.into(), i64::MIN + 1),
            (2.into(), i64::MIN + 2)
        ]
    );
    assert_eq!(
        LazyCollection::range(i64::MAX, i64::MIN, two)
            .take(2)
            .unwrap()
            .unwrap(),
        vec![(0.into(), i64::MAX), (1.into(), i64::MAX - 2)]
    );
    assert_eq!(
        LazyCollection::range(i64::MAX, i64::MAX, one)
            .count()
            .unwrap(),
        1
    );
    assert_eq!(
        LazyCollection::range(i64::MIN, i64::MIN + 1, two)
            .count()
            .unwrap(),
        1
    );
}

#[test]
fn lazy_wrapping_replays_values_and_unwrap_propagates_errors() {
    use serde_json::json;
    assert_eq!(
        LazyCollection::wrap(Some("entry")).unwrap().unwrap(),
        vec![(0.into(), "entry")]
    );
    assert!(
        LazyCollection::<i32>::wrap(None)
            .unwrap()
            .unwrap()
            .is_empty()
    );
    let json = LazyCollection::from_value(json!({"08":2,"8":3}));
    assert_eq!(json.to_json().unwrap(), r#"{"08":2,"8":3}"#);
    assert_eq!(
        json.clone().unwrap().unwrap(),
        vec![("08".into(), json!(2)), (8.into(), json!(3))]
    );
    assert_eq!(
        LazyCollection::from_value(json!(42)).to_json().unwrap(),
        "[42]"
    );
    assert_eq!(LazyCollection::from_value(json!(null)).count().unwrap(), 0);
    assert!(matches!(
        LazyCollection::try_make(|| [Ok(1), Err(source_error())]).unwrap(),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_conditional_pipes_and_tap_preserve_the_source_until_pulled() {
    let reads = Cell::new(0);
    let tapped = Cell::new(0);
    let source = LazyCollection::make(|| (1..).inspect(|_| reads.set(reads.get() + 1)));
    let stages: [fn(_) -> _; 2] = [
        |items: LazyCollection<'_, i32>| items.map(|value, _| value * 3),
        |items: LazyCollection<'_, i32>| items.take(2),
    ];
    let stage = source
        .when(false, |_| panic!("unchosen branch"))
        .when_else(true, |items| items.skip(1), |_| panic!("unchosen fallback"))
        .unless(true, |_| panic!("unchosen unless"))
        .unless_else(true, |_| panic!("unchosen unless branch"), |items| items)
        .tap(|_| tapped.set(tapped.get() + 1))
        .pipe_through(stages);
    assert_eq!(reads.get(), 0);
    assert_eq!(tapped.get(), 1);
    assert_eq!(
        stage.pipe(|items| items.values().to_json()).unwrap(),
        "[6,9]"
    );
    assert_eq!(reads.get(), 3);

    struct Wrapped<'a>(LazyCollection<'a, i32>);
    impl<'a> From<LazyCollection<'a, i32>> for Wrapped<'a> {
        fn from(items: LazyCollection<'a, i32>) -> Self {
            Self(items)
        }
    }
    let wrapped: Wrapped<'_> = LazyCollection::from([7, 8]).pipe_into();
    assert_eq!(wrapped.0.first().unwrap(), Some(7));
}

#[test]
fn lazy_emptiness_branches_probe_once_and_propagate_the_probe_error() {
    let reads = Cell::new(0);
    let source =
        LazyCollection::make(|| [2, 3].into_iter().inspect(|_| reads.set(reads.get() + 1)));
    let sorted = source
        .when_empty_else(|_| panic!("not empty"), |items| items.take(1))
        .unwrap();
    assert_eq!(reads.get(), 1);
    assert_eq!(sorted.first().unwrap(), Some(2));
    assert_eq!(reads.get(), 2);
    let fallback = LazyCollection::<i32>::empty()
        .when_not_empty_else(|_| panic!("empty"), |_| LazyCollection::from([5]))
        .unwrap();
    assert_eq!(fallback.first().unwrap(), Some(5));
    assert_eq!(
        LazyCollection::<i32>::empty()
            .unless_not_empty(|_| LazyCollection::from([7]))
            .unwrap()
            .first()
            .unwrap(),
        Some(7)
    );
    let failing = LazyCollection::try_make(|| [Err::<i32, _>(source_error())]);
    assert!(matches!(
        failing.unless_empty(|_| panic!("source failed")),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_validation_and_owned_reductions_preserve_repeated_occurrences() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("a", 1), ("a", 2), ("b", 3)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert!(matches!(
        source.clone().ensure(|value, _| *value < 2),
        Err(CollectionError::InvalidArgument)
    ));
    assert_eq!(reads.get(), 2);
    let valid = source.ensure(|value, _| *value < 4).unwrap();
    assert_eq!(
        valid
            .reduce_with_keys(0, |sum, value, _| sum + value)
            .unwrap(),
        6
    );
    assert_eq!(
        valid
            .reduce_into(Vec::new(), |values, value, key| values
                .push((key.clone(), *value)))
            .unwrap(),
        vec![("a".into(), 1), ("a".into(), 2), ("b".into(), 3)]
    );
    let failing = LazyCollection::try_make(|| [Ok(1), Err(source_error())]);
    assert!(matches!(
        failing.ensure(|_, _| true),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_flip_and_spread_transform_items_without_materializing() {
    let keys = LazyCollection::from_entries(|| {
        [
            ("a", ArrayKey::from("8")),
            ("b", "8".into()),
            ("c", "08".into()),
        ]
    });
    let flipped = keys.flip();
    assert_eq!(flipped.count().unwrap(), 3);
    assert_eq!(
        flipped.collect().unwrap().to_json().unwrap(),
        r#"{"8":"b","08":"c"}"#
    );

    let calls = Cell::new(0);
    let rows = LazyCollection::from_entries(|| [("x", vec![2_u8, 3]), ("y", vec![4, 5])]);
    let sums = rows
        .clone()
        .map_spread(|items, _| {
            calls.set(calls.get() + 1);
            items.iter().sum::<u8>()
        })
        .map_into::<u16>();
    assert_eq!(calls.get(), 0);
    assert_eq!(sums.first().unwrap(), Some(5));
    assert_eq!(calls.get(), 1);
    rows.each_spread(|items, key| {
        assert_eq!(items, [2, 3]);
        assert_eq!(key, &ArrayKey::from("x"));
        false
    })
    .unwrap();
}

#[test]
fn lazy_json_select_is_row_based_and_recursive_updates_reuse_key_rules() {
    use serde_json::json;
    let reads = Cell::new(0);
    let rows = LazyCollection::from_entries(|| {
        [
            ("same", json!({"a":1,"b":2})),
            ("same", json!({"a":null})),
            ("last", json!({"b":3})),
        ]
        .into_iter()
        .inspect(|_| reads.set(reads.get() + 1))
    });
    let selected = rows.select(&["a"]);
    assert_eq!(reads.get(), 0);
    assert_eq!(selected.first().unwrap(), Some(json!({"a":1})));
    assert_eq!(reads.get(), 1);
    assert_eq!(selected.to_array().unwrap(), json!([{"a":1},{"a":null},[]]));

    let base = LazyCollection::from_value(json!({"profile":{"name":"A","roles":["edit"]}}));
    let other = LazyCollection::from_value(json!({"profile":{"name":"B","roles":["view"]}}));
    assert_eq!(
        base.clone()
            .merge_recursive(other.clone())
            .to_array()
            .unwrap(),
        json!({"profile":{"name":["A","B"],"roles":["edit","view"]}})
    );
    assert_eq!(
        base.replace_recursive(other).to_array().unwrap(),
        json!({"profile":{"name":"B","roles":["view"]}})
    );
}

#[test]
fn lazy_multiply_coalesces_input_before_repetition_and_transforms_propagate_errors() {
    let source = LazyCollection::from_entries(|| [("same", 1), ("same", 2), ("other", 3)]);
    assert_eq!(source.multiply(2).to_json().unwrap(), "[2,3,2,3]");
    let failing =
        LazyCollection::try_make(|| [Ok(serde_json::json!({"a":1})), Err(source_error())]);
    let projected = failing.clone().select(&["a"]);
    let mut values = projected.get_iterator();
    assert!(matches!(values.next(), Some(Ok(_))));
    assert!(matches!(
        values.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(matches!(
        LazyCollection::from_value(serde_json::json!({"a":0}))
            .replace_recursive(failing)
            .first(),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_cross_join_defers_source_reads_and_matches_product_order() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("repeat", 99), ("repeat", 1), ("other", 2)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let columns: &[&[_]] = &[&[3, 4], &[5]];
    let product = source.cross_join(columns);
    assert_eq!(reads.get(), 0);
    assert_eq!(product.first().unwrap(), Some(vec![1, 3, 5]));
    assert_eq!(reads.get(), 3);
    assert_eq!(
        product.to_json().unwrap(),
        "[[1,3,5],[1,4,5],[2,3,5],[2,4,5]]"
    );
    assert_eq!(
        LazyCollection::from([1, 2])
            .cross_join(&[&[]])
            .count()
            .unwrap(),
        0
    );
}

#[test]
fn lazy_random_is_immediate_and_replays_the_selected_values() {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("x", 10), ("y", 20), ("z", 30)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let retained = source.random(2, true).unwrap();
    assert_eq!(reads.get(), 3);
    let sampled = LazyCollection::all(&retained).unwrap();
    assert_eq!(sampled.len(), 2);
    assert!(sampled.windows(2).all(|pair| pair[0].1 < pair[1].1));
    assert!(sampled.iter().all(|(key, value)| {
        [("x".into(), 10), ("y".into(), 20), ("z".into(), 30)].contains(&(key.clone(), *value))
    }));
    assert_eq!(LazyCollection::all(&retained).unwrap(), sampled);
    assert_eq!(reads.get(), 3);
    assert_eq!(
        source.random(3, false).unwrap().to_json().unwrap(),
        "[10,20,30]"
    );
    assert!(matches!(
        source.random(4, true),
        Err(CollectionError::InvalidArgument)
    ));
    assert!(matches!(
        LazyCollection::<i32>::empty().rand_item(),
        Err(CollectionError::ItemNotFound)
    ));
    assert_eq!(LazyCollection::from([7]).rand_item().unwrap(), 7);
}

#[test]
fn lazy_shuffle_preserves_membership_and_propagates_source_errors() {
    let reads = Cell::new(0);
    let shuffled = LazyCollection::make(|| {
        [3, 4, 5]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    })
    .shuffle();
    let mut iterator = shuffled.get_iterator();
    assert_eq!(reads.get(), 0);
    let first = iterator.next().unwrap().unwrap();
    assert_eq!(reads.get(), 3);
    let mut values = vec![first.1];
    values.extend(iterator.map(|entry| entry.unwrap().1));
    values.sort_unstable();
    assert_eq!(values, [3, 4, 5]);

    let failing = LazyCollection::try_make(|| [Ok(1), Err(source_error())]);
    assert!(matches!(
        failing.clone().shuffle().first(),
        Err(CollectionError::Source(_))
    ));
    assert!(matches!(
        failing.clone().cross_join(&[&[2]]).first(),
        Err(CollectionError::Source(_))
    ));
    assert!(matches!(
        failing.random(0, false),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_percentage_counts_once_in_key_order_and_rounds() -> CollectionResult<()> {
    let opens = Cell::new(0);
    let calls = Cell::new(0);
    let entries = LazyCollection::from_entries(|| {
        opens.set(opens.get() + 1);
        [("one", 1), ("two", 2), ("three", 3)]
    });
    assert_eq!(
        entries.percentage(
            |value, key| {
                calls.set(calls.get() + 1);
                *value == 1 && key == &ArrayKey::from("one")
            },
            2
        )?,
        Some(33.33)
    );
    assert_eq!((opens.get(), calls.get()), (1, 3));
    assert_eq!(entries.percentage(|_, _| false, 2)?, Some(0.0));
    assert_eq!(entries.percentage(|_, _| true, 2)?, Some(100.0));
    assert_eq!(
        LazyCollection::make(|| 0..32).percentage(|value, _| *value == 0, 2)?,
        Some(3.13)
    );
    assert_eq!(
        LazyCollection::<i32>::from([]).percentage(|_, _| panic!("empty source"), 2)?,
        None
    );
    Ok(())
}

#[test]
fn lazy_percentage_propagates_an_error_without_reading_remaining_entries() {
    let reads = Cell::new(0);
    let entries = LazyCollection::try_make(|| {
        [Ok(1), Err(CollectionError::InvalidArgument), Ok(3)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let mut calls = 0;
    let result = entries.percentage(
        |_, _| {
            calls += 1;
            true
        },
        0,
    );
    assert!(matches!(result, Err(CollectionError::InvalidArgument)));
    assert_eq!(calls, 1);
    assert_eq!(reads.get(), 2);
}

#[test]
fn lazy_caching_iterator_looks_ahead_and_caches_only_yielded_keys() -> CollectionResult<()> {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("a", 1), ("a", 2), ("b", 3)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let mut traversal = source.get_caching_iterator_with_full_cache();
    assert_eq!(reads.get(), 0);
    assert_eq!(traversal.next().transpose()?, Some(("a".into(), 1)));
    assert!(traversal.has_next());
    assert_eq!(reads.get(), 2);
    assert_eq!(traversal.get_cache().unwrap().get("a"), Some(&1));
    assert_eq!(traversal.next().transpose()?, Some(("a".into(), 2)));
    assert_eq!(reads.get(), 3);
    assert_eq!(traversal.get_cache().unwrap().get("a"), Some(&2));
    assert!(!traversal.get_cache().unwrap().has("b"));
    assert_eq!(traversal.next().transpose()?, Some(("b".into(), 3)));
    assert!(!traversal.has_next());
    assert_eq!(traversal.get_cache().unwrap().get("b"), Some(&3));
    assert!(traversal.next().is_none());

    struct NoClone;
    let single = LazyCollection::make(|| [NoClone]);
    let mut lookahead = single.get_caching_iterator();
    assert!(lookahead.has_next());
    assert!(lookahead.next().transpose()?.is_some());
    assert!(lookahead.get_cache().is_none());
    assert!(!lookahead.has_next());
    assert!(single.contains_one_item()?);
    assert!(!single.contains_many_items()?);
    assert!(source.contains_many_items()?);
    Ok(())
}

#[test]
fn lazy_caching_iterator_preserves_the_entry_before_a_source_error() -> CollectionResult<()> {
    let reads = Cell::new(0);
    let source = LazyCollection::try_make(|| {
        [Ok(1), Err(CollectionError::InvalidArgument), Ok(2)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let mut traversal = source.get_caching_iterator_with_full_cache();
    assert_eq!(traversal.next().transpose()?, Some((0.into(), 1)));
    assert_eq!(reads.get(), 2);
    assert!(traversal.has_next());
    assert!(matches!(
        traversal.next(),
        Some(Err(CollectionError::InvalidArgument))
    ));
    assert!(!traversal.has_next());
    assert_eq!(reads.get(), 2);
    assert_eq!(traversal.get_cache().unwrap().count(), 1);
    Ok(())
}

#[test]
fn lazy_dump_materializes_the_snapshot_and_returns_the_original_source() -> CollectionResult<()> {
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("same", "first"), ("same", "last"), ("other", "second")]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let mut bytes = Vec::new();
    let source = source.dump_to(&mut bytes, &[&"context marker", &17])?;
    assert_eq!(reads.get(), 3);
    let output = String::from_utf8(bytes).unwrap();
    assert!(output.contains("same") && output.contains("other"));
    assert!(output.contains("last") && output.contains("second"));
    assert!(!output.contains("first"));
    assert!(output.ends_with("\"context marker\"\n17\n"));
    assert_eq!(source.first()?, Some("first"));
    assert_eq!(reads.get(), 4);
    Ok(())
}

#[test]
fn lazy_dump_propagates_source_and_write_failures() {
    let failing = LazyCollection::try_make(|| [Ok(1), Err(CollectionError::InvalidArgument)]);
    let mut output = Vec::new();
    assert!(matches!(
        failing.dump_to(&mut output, &[&"context"]),
        Err(CollectionError::InvalidArgument)
    ));
    assert!(output.is_empty());

    struct BrokenWriter;
    impl io::Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let result = LazyCollection::from([1]).dump_to(BrokenWriter, &[]);
    assert!(matches!(
        result,
        Err(CollectionError::Source(error))
            if error.downcast_ref::<io::Error>().is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    ));
}

#[test]
fn lazy_string_escape_only_applies_during_string_conversion() -> CollectionResult<()> {
    let reads = Cell::new(0);
    let values = LazyCollection::from_entries(|| {
        [("a&b", "<>\"' &lt;")]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let escaped = values.clone().escape_when_casting_to_string(true);
    assert_eq!(reads.get(), 0);
    assert_eq!(
        escaped.to_string()?,
        r#"{&quot;a&amp;b&quot;:&quot;&lt;&gt;\&quot;&#039; &amp;lt;&quot;}"#
    );
    assert_eq!(reads.get(), 1);
    let raw = r#"{"a&b":"<>\"' &lt;"}"#;
    assert_eq!(escaped.to_json()?, raw);
    assert_eq!(values.to_string()?, raw);
    assert_eq!(
        escaped
            .clone()
            .escape_when_casting_to_string(false)
            .to_string()?,
        raw
    );
    assert_eq!(escaped.map(|value, _| *value).to_string()?, raw);
    Ok(())
}

#[test]
fn lazy_string_escape_preserves_source_errors() {
    let values = LazyCollection::<i32>::try_make(|| [Err(CollectionError::InvalidArgument)]);
    assert!(matches!(
        values.escape_when_casting_to_string(true).to_string(),
        Err(CollectionError::InvalidArgument)
    ));
}

#[test]
fn lazy_dd_carries_the_snapshot_and_preserves_source_errors() {
    let result = LazyCollection::from(["diagnostic"]).dd_with(&[&"context marker"]);
    assert!(matches!(
        result,
        Err(CollectionError::DumpAndDie(output))
            if output.contains("diagnostic") && output.ends_with("\"context marker\"\n")
    ));
    let failure = LazyCollection::<i32>::try_make(|| [Err(CollectionError::InvalidArgument)]);
    assert!(matches!(
        failure.dd(),
        Err(CollectionError::InvalidArgument)
    ));
}

#[test]
fn higher_order_proxy_preserves_the_operations_traversal_and_keys() {
    struct Task {
        name: &'static str,
        attempts: u32,
    }

    impl Task {
        fn exhausted(&self, limit: u32) -> bool {
            self.attempts >= limit
        }
    }

    let reads = Cell::new(0);
    let tasks = LazyCollection::from_entries(|| {
        [("first", "ingest", 1), ("second", "notify", 3)]
            .into_iter()
            .map(|(key, name, attempts)| {
                reads.set(reads.get() + 1);
                (key, Task { name, attempts })
            })
    });

    let names = tasks
        .clone()
        .proxy(LazyCollection::map)
        .call(|task, _| task.name);
    let limit = 3;
    let exhausted = tasks
        .clone()
        .proxy(LazyCollection::filter)
        .call(move |task, _| task.exhausted(limit));

    assert_eq!(reads.get(), 0);
    assert_eq!(
        names.take(1).get_iterator().next().unwrap().unwrap(),
        ("first".into(), "ingest")
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(
        exhausted
            .map(|task, _| task.name)
            .get_iterator()
            .collect::<CollectionResult<Vec<_>>>()
            .unwrap(),
        vec![("second".into(), "notify")]
    );
    assert_eq!(
        tasks
            .proxy(|items: LazyCollection<'_, Task>, project| items.sum_by(project))
            .call(|task: &Task, _: &ArrayKey| task.attempts)
            .unwrap(),
        4
    );
}

#[test]
fn higher_order_proxy_propagates_source_errors_without_projecting_them() {
    let projected = Cell::new(0);
    let values = LazyCollection::try_make(|| [Ok(1), Err(CollectionError::InvalidArgument)]);
    let mapped = values.proxy(LazyCollection::map).call(|value, _| {
        projected.set(projected.get() + 1);
        *value
    });

    assert_eq!(projected.get(), 0);
    assert!(matches!(
        mapped.all(),
        Err(CollectionError::InvalidArgument)
    ));
    assert_eq!(projected.get(), 1);
}

#[test]
fn lazy_implode_projects_only_the_materialized_entries() {
    struct Task {
        name: &'static str,
    }

    let projected = RefCell::new(Vec::new());
    let tasks = LazyCollection::from_entries(|| {
        [("first", "draft"), ("next", "ship"), ("first", "review")]
            .map(|(key, name)| (key, Task { name }))
    });
    assert_eq!(
        tasks
            .implode_by(
                |task, key| {
                    projected.borrow_mut().push((key.clone(), task.name));
                    task.name
                },
                " → ",
            )
            .unwrap(),
        "review → ship"
    );
    assert_eq!(
        projected.borrow().as_slice(),
        &[("first".into(), "review"), ("next".into(), "ship")]
    );
    assert_eq!(
        LazyCollection::<Task>::empty()
            .implode_by(
                |_, _| -> &str { panic!("empty collection has no projection") },
                "-"
            )
            .unwrap(),
        ""
    );

    let tasks = LazyCollection::try_make(|| [Ok(Task { name: "draft" }), Err(source_error())]);
    assert!(matches!(
        tasks.implode_by(
            |_, _| -> &str { panic!("source failed before projection") },
            "-"
        ),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_lookup_fallbacks_run_only_on_miss_and_preserve_lookup_order() {
    let calls = Cell::new(0);
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [("a", 10), ("b", 11), ("a", 20)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let fallback = || {
        calls.set(calls.get() + 1);
        99
    };

    assert_eq!(source.get_or_else("a", fallback).unwrap(), 10);
    assert_eq!(reads.get(), 1);
    assert_eq!(source.first_or_else(fallback).unwrap(), 10);
    assert_eq!(reads.get(), 2);
    assert_eq!(
        source
            .first_where_or_else(|_, key| key == &"b".into(), fallback)
            .unwrap(),
        11
    );
    assert_eq!(reads.get(), 4);
    assert_eq!(
        source
            .last_where_or_else(|value, _| value % 10 == 0, fallback)
            .unwrap(),
        20
    );
    assert_eq!(reads.get(), 7);
    assert_eq!(source.last_or_else(fallback).unwrap(), 20);
    assert_eq!(reads.get(), 10);
    assert_eq!(calls.get(), 0);

    assert_eq!(source.get_or_else("missing", fallback).unwrap(), 99);
    assert_eq!(
        source.first_where_or_else(|_, _| false, fallback).unwrap(),
        99
    );
    assert_eq!(
        source.last_where_or_else(|_, _| false, fallback).unwrap(),
        99
    );
    assert_eq!(LazyCollection::empty().first_or_else(fallback).unwrap(), 99);
    assert_eq!(LazyCollection::empty().last_or_else(fallback).unwrap(), 99);
    assert_eq!(calls.get(), 5);
    let owned = String::from("computed once");
    assert_eq!(
        LazyCollection::empty()
            .first_or_else(move || owned)
            .unwrap(),
        "computed once"
    );
}

#[test]
fn lazy_lookup_fallbacks_preserve_null_and_source_errors() {
    use serde_json::{Value, json};

    let values = LazyCollection::from_entries(|| {
        [
            ("first", json!({})),
            ("second", json!({"profile":{"name":null}})),
            ("third", json!({"profile":{"name":"Taylor"}})),
        ]
    });
    let never = || -> Value { panic!("a source error or present value precedes fallback") };
    assert_eq!(
        values.value_or_else("profile.name", never).unwrap(),
        Value::Null
    );
    assert_eq!(
        values
            .value_or_else("profile.email", || json!("unknown"))
            .unwrap(),
        json!("unknown")
    );
    assert_eq!(
        LazyCollection::from([Value::Null])
            .get_or_else(0, never)
            .unwrap(),
        Value::Null
    );

    let broken = LazyCollection::try_from_entries(|| {
        [
            Ok(("hit", json!({"profile":{"name":null}}))),
            Err(source_error()),
        ]
    });
    assert_eq!(
        broken.get_or_else("hit", never).unwrap(),
        json!({"profile":{"name":null}})
    );
    assert_eq!(
        broken.value_or_else("profile.name", never).unwrap(),
        Value::Null
    );
    for error in [
        broken.get_or_else("missing", never),
        broken.first_where_or_else(|_, _| false, never),
        broken.last_where_or_else(|_, _| true, never),
        broken.last_or_else(never),
        broken.value_or_else("profile.email", never),
    ] {
        assert!(matches!(error, Err(CollectionError::Source(_))));
    }
    assert!(matches!(
        LazyCollection::<Value>::try_make(|| [Err(source_error())]).first_or_else(never),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_value_boundaries_are_strict_and_keep_keys() {
    use serde_json::json;
    let reads = Cell::new(0);
    let source = LazyCollection::from_entries(|| {
        [
            ("string", json!("2")),
            ("float", json!(2.0)),
            ("integer", json!(2)),
            ("tail", json!(3)),
        ]
        .into_iter()
        .inspect(|_| reads.set(reads.get() + 1))
    });

    let before = source.clone().take_until_value(json!(2));
    let from = source.clone().skip_until_value(json!(2));
    assert_eq!(reads.get(), 0);
    assert_eq!(
        before.all().unwrap(),
        vec![("string".into(), json!("2")), ("float".into(), json!(2.0))]
    );
    assert_eq!(reads.get(), 3);
    assert_eq!(
        from.all().unwrap(),
        vec![("integer".into(), json!(2)), ("tail".into(), json!(3))]
    );
    assert_eq!(reads.get(), 7);
    assert!(source.skip_until_value(json!(99)).all().unwrap().is_empty());

    let source = LazyCollection::from_entries(|| [("a", 1), ("b", 1), ("c", 2), ("d", 1)]);
    assert_eq!(
        source.clone().take_while_value(1).all().unwrap(),
        vec![("a".into(), 1), ("b".into(), 1)]
    );
    assert_eq!(
        source.skip_while_value(1).all().unwrap(),
        vec![("c".into(), 2), ("d".into(), 1)]
    );
    let failing = LazyCollection::try_make(|| [Ok(1), Err(source_error()), Ok(2)]);
    assert!(matches!(
        failing.clone().skip_until_value(2).all(),
        Err(CollectionError::Source(_))
    ));
    assert!(matches!(
        failing.take_while_value(1).all(),
        Err(CollectionError::Source(_))
    ));
}

#[test]
fn lazy_truthy_filter_reuses_php_json_truthiness_and_stops_at_errors() {
    use serde_json::json;

    let reads = Cell::new(0);
    let source = LazyCollection::try_from_entries(|| {
        [
            Ok(("null", json!(null))),
            Ok(("false", json!(false))),
            Ok(("zero", json!(0))),
            Ok(("zero_float", json!(-0.0))),
            Ok(("empty_text", json!(""))),
            Ok(("zero_text", json!("0"))),
            Ok(("empty_array", json!([]))),
            Ok(("empty_map", json!({}))),
            Ok(("text", json!("00"))),
            Ok(("array", json!([false]))),
            Ok(("map", json!({"present":false}))),
            Err(source_error()),
            Ok(("unread", json!(true))),
        ]
        .into_iter()
        .inspect(|_| reads.set(reads.get() + 1))
    })
    .filter_truthy();
    assert_eq!(reads.get(), 0);
    let mut iterator = source.get_iterator();
    assert_eq!(
        iterator.next().unwrap().unwrap(),
        ("text".into(), json!("00"))
    );
    assert_eq!(reads.get(), 9);
    assert_eq!(
        iterator.next().unwrap().unwrap(),
        ("array".into(), json!([false]))
    );
    assert_eq!(
        iterator.next().unwrap().unwrap(),
        ("map".into(), json!({"present":false}))
    );
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
    assert_eq!(reads.get(), 12);
}

#[test]
fn deadline_allows_the_first_item_when_opening_the_source_crosses_it() {
    let reads = Cell::new(0);
    let deadline = Instant::now() + Duration::from_millis(250);
    let source = LazyCollection::make(|| {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        [1, 2].into_iter().inspect(|_| reads.set(reads.get() + 1))
    })
    .take_until_timeout(deadline);
    let mut iterator = source.get_iterator();
    assert_eq!(reads.get(), 0);
    assert_eq!(iterator.next().unwrap().unwrap(), (0.into(), 1));
    assert!(iterator.next().is_none());
    assert_eq!(reads.get(), 1);
}

#[test]
fn deadline_notifies_on_resume_with_the_last_item_and_does_not_overread() {
    let reads = Cell::new(0);
    let notifications = RefCell::new(Vec::new());
    let deadline = Instant::now() + Duration::from_millis(250);
    let source = LazyCollection::from_entries(|| {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        [("first", 1), ("later", 2)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    })
    .take_until_timeout_with(deadline, |item| {
        notifications
            .borrow_mut()
            .push(item.map(|(value, key)| (*value, key.clone())));
    });
    let mut iterator = source.get_iterator();
    assert_eq!(iterator.next().unwrap().unwrap(), ("first".into(), 1));
    assert!(notifications.borrow().is_empty());
    assert!(iterator.next().is_none());
    assert!(iterator.next().is_none());
    assert_eq!(reads.get(), 1);
    assert_eq!(
        notifications.borrow().as_slice(),
        &[Some((1, "first".into()))]
    );
}

#[test]
fn deadline_does_not_call_back_for_empty_or_failing_initial_reads() {
    let deadline = Instant::now() + Duration::from_millis(250);
    let empty = LazyCollection::<i32>::make(|| {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        []
    })
    .take_until_timeout_with(deadline, |_| panic!("empty source did not yield an item"));
    assert!(empty.all().unwrap().is_empty());

    let deadline = Instant::now() + Duration::from_millis(250);
    let failing = LazyCollection::<i32>::try_make(|| {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        [Err(source_error())]
    })
    .take_until_timeout_with(deadline, |_| panic!("failed source did not yield an item"));
    let mut iterator = failing.get_iterator();
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
}
