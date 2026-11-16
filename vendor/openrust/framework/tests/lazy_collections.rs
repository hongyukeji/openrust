use std::{
    cell::{Cell, RefCell},
    io, iter,
    time::{Duration, Instant},
};

use openrust::support::{ArrayKey, Collection, CollectionError, Enumerable, LazyCollection};

#[test]
fn pipelines_open_only_when_polled_and_replay_without_overreading() {
    let opens = Cell::new(0);
    let reads = Cell::new(0);
    let tapped = RefCell::new(Vec::new());
    let values = LazyCollection::fromEntries(|| {
        opens.set(opens.get() + 1);
        (1..).map(|value| {
            reads.set(reads.get() + 1);
            (value * 3, value)
        })
    })
    .filter(|value, _| value % 2 == 0)
    .map(|value, _| value * 10)
    .tapEach(|value, key| tapped.borrow_mut().push((key.clone(), *value)));

    let selected = values.clone().take(2);
    let mut iterator = selected.getIterator();
    assert_eq!(opens.get(), 0);
    assert_eq!(reads.get(), 0);
    assert!(tapped.borrow().is_empty());
    assert_eq!(iterator.next().unwrap().unwrap(), (6.into(), 20));
    assert_eq!(iterator.next().unwrap().unwrap(), (12.into(), 40));
    assert!(iterator.next().is_none());
    assert_eq!(reads.get(), 4);
    assert_eq!(
        selected.collect().unwrap().toJson().unwrap(),
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
    let values = LazyCollection::fromEntries(|| {
        opens.set(opens.get() + 1);
        (1..=4).map(|value| {
            reads.set(reads.get() + 1);
            ("same", value)
        })
    })
    .remember();
    assert_eq!(opens.get(), 1);
    assert_eq!(reads.get(), 0);

    let mut first = values.getIterator();
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
    let failing = LazyCollection::tryMake(|| {
        [Ok(1), Err(source_error()), Ok(3)]
            .into_iter()
            .inspect(|_| {
                reads.set(reads.get() + 1);
            })
    });
    let skipped = failing.clone().skip(10).filter(|_, _| false).take(-2);
    let mut iterator = skipped.getIterator();
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
    let values = LazyCollection::fromEntries(|| [("1", 1), ("05", 2), ("1", 3), ("last", 4)]);
    assert_eq!(
        values.clone().take(-2).all().unwrap(),
        vec![(1.into(), 3), ("last".into(), 4)]
    );
    let chunks = values.clone().chunk(2, true).collect().unwrap();
    assert_eq!(
        chunks.toJson().unwrap(),
        r#"[{"1":1,"05":2},{"1":3,"last":4}]"#
    );
    let chunks = values.clone().chunk(3, false).collect().unwrap();
    assert_eq!(chunks.toJson().unwrap(), "[[1,2,3],[4]]");
    assert_eq!(values.count().unwrap(), 4);
    assert_eq!(values.eager().unwrap().count().unwrap(), 3);

    let reads = Cell::new(0);
    let values = LazyCollection::make(|| (1..).inspect(|_| reads.set(reads.get() + 1)));
    let prefix = values
        .clone()
        .skipUntil(|value, _| *value >= 2)
        .takeUntil(|value, _| *value >= 4);
    assert_eq!(prefix.all().unwrap(), vec![(1.into(), 2), (2.into(), 3)]);
    assert_eq!(reads.get(), 4);
    assert_eq!(values.firstWhere(|value, _| *value == 2).unwrap(), Some(2));

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
        .takeUntilTimeoutWith(Instant::now(), |previous| {
            assert!(previous.is_none());
            notifications.set(notifications.get() + 1);
        });
    let mut iterator = expired.getIterator();
    assert_eq!(notifications.get(), 0);
    assert!(iterator.next().is_none());
    assert!(iterator.next().is_none());
    assert_eq!(notifications.get(), 1);
    assert_eq!(opens.get(), 0);

    let values = values
        .withHeartbeat(Duration::ZERO, || beats.set(beats.get() + 1))
        .throttle(Duration::from_millis(2));
    let started = Instant::now();
    assert_eq!(values.take(2).count().unwrap(), 2);
    assert!(started.elapsed() >= Duration::from_millis(2));
    assert_eq!(beats.get(), 2);
}

#[test]
fn chunk_while_reuses_the_boundary_item_and_preserves_keys() {
    let reads = Cell::new(0);
    let groups = LazyCollection::fromEntries(|| {
        [("first", 'A'), ("again", 'A'), ("next", 'B'), ("last", 'C')]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    })
    .chunkWhile(|value, _, group| group.last() == Some(value));
    assert_eq!(reads.get(), 0);
    let mut iterator = groups.getIterator();
    let (_, first) = iterator.next().unwrap().unwrap();
    assert_eq!(first.toJson().unwrap(), r#"{"first":"A","again":"A"}"#);
    assert_eq!(reads.get(), 3);
    let (_, second) = iterator.next().unwrap().unwrap();
    assert_eq!(second.toJson().unwrap(), r#"{"next":"B"}"#);
    assert_eq!(reads.get(), 4);
    let (_, last) = iterator.next().unwrap().unwrap();
    assert_eq!(last.toJson().unwrap(), r#"{"last":"C"}"#);
    assert!(iterator.next().is_none());
    assert_eq!(groups.count().unwrap(), 3);
}

#[test]
fn sliding_preserves_key_order_and_pulls_only_complete_windows() {
    let reads = Cell::new(0);
    let source = LazyCollection::fromEntries(|| {
        [("x", 1), ("x", 2), ("y", 3), ("z", 4), ("p", 5), ("q", 6)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let overlap = source.clone().sliding(2, 1).unwrap();
    assert_eq!(reads.get(), 0);
    let mut iterator = overlap.getIterator();
    let (_, first) = iterator.next().unwrap().unwrap();
    assert_eq!(first.toJson().unwrap(), r#"{"x":2,"y":3}"#);
    assert_eq!(reads.get(), 3);
    assert_eq!(
        iterator.next().unwrap().unwrap().1.toJson().unwrap(),
        r#"{"y":3,"z":4}"#
    );
    assert_eq!(reads.get(), 4);
    let gapped = source.clone().sliding(2, 3).unwrap().all().unwrap();
    assert_eq!(gapped.len(), 2);
    assert_eq!(gapped[1].1.toJson().unwrap(), r#"{"p":5,"q":6}"#);
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
    let source = LazyCollection::tryMake(|| [Ok(1), Ok(2), Err(source_error()), Ok(4)]);
    let chunk = source.clone().chunkWhile(|_, _, _| true);
    let mut groups = chunk.getIterator();
    assert!(matches!(
        groups.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(groups.next().is_none());

    let sliding = source.sliding(2, 3).unwrap();
    let mut windows = sliding.getIterator();
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
    let readings = LazyCollection::fromEntries(|| {
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

    let failing = LazyCollection::tryMake(|| [Ok(1), Err(source_error()), Ok(3)]);
    let sampled = failing.clone().nth(3, 0).unwrap();
    let mut iterator = sampled.getIterator();
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
    let keys = LazyCollection::fromEntries(|| [("same", 1), ("same", 2)]).keys();
    let suffix = LazyCollection::fromEntries(|| {
        opens.set(opens.get() + 1);
        [("tail", 3)]
    });
    let joined = LazyCollection::fromEntries(|| [("a", 1), ("a", 2)]).concat(suffix);
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
    let failing = LazyCollection::tryMake(|| [Ok(1), Err(source_error())]);
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
    let readings = LazyCollection::fromEntries(|| [("a", 4), ("b", 7), ("c", 11)]);
    let groups = readings
        .chunkBy(|value, key| {
            assert!(matches!(key, ArrayKey::String(_)));
            value / 10
        })
        .all()
        .unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].1.toJson().unwrap(), r#"{"a":4,"b":7}"#);
    assert_eq!(groups[1].1.toJson().unwrap(), r#"{"c":11}"#);
}

#[test]
fn lazy_collapse_preserves_or_reindexes_keys_without_opening_later_groups() {
    let opened = Cell::new(0);
    let groups = LazyCollection::make(|| {
        [
            LazyCollection::fromEntries(|| [("same", 1), ("first", 2)]),
            LazyCollection::fromEntries(|| {
                opened.set(opened.get() + 1);
                [("same", 3), ("last", 4)]
            }),
        ]
    });
    let keyed = groups.clone().collapseWithKeys();
    let mut iterator = keyed.getIterator();
    assert_eq!(iterator.next().unwrap().unwrap(), ("same".into(), 1));
    assert_eq!(iterator.next().unwrap().unwrap(), ("first".into(), 2));
    assert_eq!(opened.get(), 0);
    assert_eq!(iterator.next().unwrap().unwrap(), ("same".into(), 3));
    assert_eq!(opened.get(), 1);
    assert_eq!(
        keyed.collect().unwrap().toJson().unwrap(),
        r#"{"same":3,"first":2,"last":4}"#
    );
    assert_eq!(
        groups.collapse().collect().unwrap().toJson().unwrap(),
        "[1,2,3,4]"
    );
}

#[test]
fn lazy_flat_map_flattens_one_level_and_propagates_nested_failures() {
    let calls = Cell::new(0);
    let mapped = LazyCollection::make(|| [1, 2]).flatMap(|value, _| {
        calls.set(calls.get() + 1);
        Collection::fromEntries([("single", *value), ("double", value * 2)])
    });
    assert_eq!(mapped.clone().take(1).all().unwrap(), vec![(0.into(), 1)]);
    assert_eq!(calls.get(), 1);
    assert_eq!(mapped.collect().unwrap().toJson().unwrap(), "[1,2,2,4]");
    assert_eq!(
        LazyCollection::make(|| [[1, 2], [3, 4]])
            .collapse()
            .count()
            .unwrap(),
        4
    );
    let nested = LazyCollection::make(|| {
        [LazyCollection::tryMake(|| {
            [Ok(1), Err(source_error()), Ok(3)]
        })]
    });
    let flattened = nested.collapse();
    let mut iterator = flattened.getIterator();
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
    assert_eq!(one.collect().unwrap().toJson().unwrap(), "[[1,[2]],3,null]");
    assert_eq!(
        source.flatten(None).collect().unwrap().toJson().unwrap(),
        "[1,2,3,null]"
    );

    let failed = LazyCollection::tryMake(|| [Ok(json!([1])), Err(source_error())]).flatten(None);
    let mut iterator = failed.getIterator();
    assert_eq!(iterator.next().unwrap().unwrap(), (0.into(), json!(1)));
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
}

#[test]
fn lazy_zip_pads_to_the_longest_input_without_reading_ahead() {
    let reads = Cell::new(0);
    let left = LazyCollection::fromEntries(|| [("id", 1)]);
    let right = LazyCollection::make(|| {
        ["a", "b"]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let zipped = left.zip(right);
    assert_eq!(reads.get(), 0);
    let mut iterator = zipped.getIterator();
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
        .zipMany([vec![3], vec![4, 5, 6]])
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
        let mut iterator = invalid.getIterator();
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
    let left = LazyCollection::tryMake(|| [Ok("key"), Err(source_error())]);
    let right = LazyCollection::make(|| [1, 2].into_iter().inspect(|_| reads.set(reads.get() + 1)));
    let zip = left.clone().zip(right.clone());
    let mut zipped = zip.getIterator();
    assert!(matches!(zipped.next(), Some(Ok(_))));
    assert!(matches!(
        zipped.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(zipped.next().is_none());
    assert_eq!(reads.get(), 1);

    let combined = left.combine(right);
    let mut pairs = combined.getIterator();
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
    let source = LazyCollection::fromEntries(|| {
        [("a", 10), ("b", 11), ("c", 21), ("d", 10)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let unique = source.clone().uniqueBy(|value, _| value / 10);
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
    assert_eq!(source.uniqueStrict().count().unwrap(), 3);

    let failing = LazyCollection::tryMake(|| [Ok(10), Ok(10), Err(source_error())]);
    let distinct = failing.unique();
    let mut iterator = distinct.getIterator();
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
    let repeated = LazyCollection::fromEntries(|| {
        [("a", 1), ("a", 2), ("b", 3)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(repeated.sole().unwrap(), 2);
    assert_eq!(reads.get(), 2);
    assert!(!repeated.hasSole().unwrap());
    assert!(repeated.hasMany().unwrap());
    assert!(repeated.hasSoleWhere(|value, _| *value == 2).unwrap());
    assert_eq!(
        repeated
            .soleWhere(|_, key| key == &ArrayKey::from("b"))
            .unwrap(),
        3
    );
    assert!(repeated.hasManyWhere(|value, _| *value < 3).unwrap());
    assert!(matches!(
        repeated.soleWhere(|value, _| *value > 1),
        Err(CollectionError::MultipleItemsFound)
    ));
    assert!(matches!(
        repeated.soleWhere(|value, _| *value > 4),
        Err(CollectionError::ItemNotFound)
    ));
    assert!(LazyCollection::<i32>::empty().isEmpty().unwrap());
}

#[test]
fn lazy_first_and_contains_stop_on_the_first_match_and_propagate_errors() {
    let source = LazyCollection::tryMake(|| [Ok(10), Err(source_error()), Ok(30)]);
    assert!(source.isNotEmpty().unwrap());
    assert!(source.containsStrict(&10).unwrap());
    assert!(source.containsWhere(|value, _| *value == 10).unwrap());
    assert_eq!(source.firstOrFail().unwrap(), 10);
    assert_eq!(
        source.firstOrFailWhere(|value, _| *value == 10).unwrap(),
        10
    );
    assert!(matches!(
        source.firstOrFailWhere(|value, _| *value == 30),
        Err(CollectionError::Source(_))
    ));
    assert!(matches!(source.hasSole(), Err(CollectionError::Source(_))));
    assert!(matches!(source.hasMany(), Err(CollectionError::Source(_))));
    assert!(matches!(
        LazyCollection::<i32>::empty().firstOrFail(),
        Err(CollectionError::ItemNotFound)
    ));
}

#[test]
fn lazy_ordering_defers_materialization_preserves_keys_and_is_stable() {
    let reads = Cell::new(0);
    let entries = LazyCollection::fromEntries(|| {
        [("b", 3), ("a", 2), ("b", 1), ("c", 2)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let sorted = entries.clone().sort();
    assert_eq!(reads.get(), 0);
    let mut iterator = sorted.getIterator();
    assert_eq!(reads.get(), 0);
    assert_eq!(iterator.next().unwrap().unwrap(), ("b".into(), 1));
    assert_eq!(reads.get(), 4);
    assert_eq!(iterator.next().unwrap().unwrap(), ("a".into(), 2));
    assert_eq!(iterator.next().unwrap().unwrap(), ("c".into(), 2));
    assert!(iterator.next().is_none());
    assert_eq!(entries.clone().sortDesc().first().unwrap(), Some(2));
    assert_eq!(
        entries.clone().sortUsing(|a, b| b.cmp(a)).first().unwrap(),
        Some(2)
    );
    assert_eq!(
        entries
            .clone()
            .sortBy(|value, _| value % 2)
            .first()
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        entries
            .clone()
            .sortByDesc(|value, _| value % 2)
            .first()
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        entries
            .clone()
            .sortKeys()
            .getIterator()
            .next()
            .unwrap()
            .unwrap(),
        ("a".into(), 2)
    );
    assert_eq!(
        entries
            .clone()
            .sortKeysDesc()
            .getIterator()
            .next()
            .unwrap()
            .unwrap(),
        ("c".into(), 2)
    );
    assert_eq!(
        entries
            .clone()
            .sortKeysUsing(|a, b| b.cmp(a))
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
    let entries = LazyCollection::fromEntries(|| [("x", 3), ("y", 4), ("x", 4), ("z", 9)]);
    assert_eq!(
        entries.clone().duplicates().all().unwrap(),
        vec![("y".into(), 4)]
    );
    assert_eq!(
        entries.clone().duplicatesStrict().all().unwrap(),
        vec![("y".into(), 4)]
    );
    assert_eq!(
        entries.duplicatesBy(|value, _| value % 2).all().unwrap(),
        vec![("y".into(), 0)]
    );
}

#[test]
fn lazy_ordering_reports_source_errors_before_comparing_or_emitting() {
    let compares = Cell::new(0);
    let source = LazyCollection::tryMake(|| [Ok(3), Ok(1), Err(source_error()), Ok(2)]);
    let sorted = source.clone().sortUsing(|a, b| {
        compares.set(compares.get() + 1);
        a.cmp(b)
    });
    let mut iterator = sorted.getIterator();
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
    let source = LazyCollection::fromEntries(|| {
        [("x", 3), ("y", 5), ("x", 7)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(source.sum().unwrap(), 15);
    assert_eq!(reads.get(), 3);
    assert_eq!(source.sumBy(|value, _| value * value).unwrap(), 83);
    assert_eq!(source.average().unwrap(), Some(5.0));
    assert_eq!(
        source
            .avgBy(|value, _| (*value > 3).then_some(f64::from(*value)))
            .unwrap(),
        Some(6.0)
    );
    assert_eq!(source.min().unwrap(), Some(3));
    assert_eq!(source.max().unwrap(), Some(7));
    assert_eq!(source.minBy(|value, _| -*value).unwrap(), Some(-7));
    assert_eq!(source.maxBy(|value, _| -*value).unwrap(), Some(-3));
    assert_eq!(
        source
            .reduceSpread((0, 0), |sum, n, value, _| (sum + value, n + 1))
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
    let source = LazyCollection::fromEntries(|| [("x", 90), ("y", 5), ("x", 7), ("z", 5)]);
    let counts = source
        .clone()
        .countBy(|value, _| if *value == 5 { "05" } else { "5" });
    assert_eq!(counts.all().unwrap(), vec![(5.into(), 2), ("05".into(), 2)]);
    assert_eq!(source.median().unwrap(), Some(5.0));
    assert_eq!(
        source
            .medianBy(|value, _| (*value != 5).then_some(f64::from(*value)))
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
    let failing = LazyCollection::tryMake(|| [Ok(1), Err(source_error()), Ok(3)]);
    assert!(!failing.every(|value, _| *value > 3).unwrap());
    assert!(matches!(
        failing.every(|_, _| true),
        Err(CollectionError::Source(_))
    ));
    assert!(matches!(failing.sum(), Err(CollectionError::Source(_))));
    assert!(matches!(failing.median(), Err(CollectionError::Source(_))));
    let counts = failing.countBy(|value, _| *value);
    let mut iterator = counts.getIterator();
    assert!(matches!(
        iterator.next(),
        Some(Err(CollectionError::Source(_)))
    ));
    assert!(iterator.next().is_none());
}

#[test]
fn lazy_set_comparisons_materialize_both_inputs_and_preserve_keys() {
    let left = LazyCollection::fromEntries(|| [("a", 99), ("a", 2), ("B", 3), ("c", 7)]);
    let right = LazyCollection::fromEntries(|| [("a", 2), ("b", 3), ("d", 7)]);
    let by_key = |a: &ArrayKey, b: &ArrayKey| {
        a.to_string()
            .to_lowercase()
            .cmp(&b.to_string().to_lowercase())
    };
    assert!(left.clone().diff(right.clone()).all().unwrap().is_empty());
    assert_eq!(
        left.clone().diffAssoc(right.clone()).all().unwrap(),
        vec![("B".into(), 3), ("c".into(), 7)]
    );
    assert_eq!(
        left.clone().intersectAssoc(right.clone()).all().unwrap(),
        vec![("a".into(), 2)]
    );
    assert_eq!(
        left.clone()
            .intersectAssocUsing(right.clone(), by_key)
            .count()
            .unwrap(),
        2
    );
    assert_eq!(
        left.clone()
            .diffAssocUsing(right.clone(), by_key)
            .count()
            .unwrap(),
        1
    );
    assert_eq!(
        left.clone()
            .diffKeysUsing(right.clone(), by_key)
            .count()
            .unwrap(),
        1
    );
    assert_eq!(left.clone().diffKeys(right.clone()).count().unwrap(), 2);
    assert_eq!(
        left.clone().intersectByKeys(right.clone()).count().unwrap(),
        1
    );
    assert_eq!(left.clone().intersect(right).count().unwrap(), 3);
    let parity = |a: &i32, b: &i32| (a % 2).cmp(&(b % 2));
    assert_eq!(left.clone().intersectUsing([3], parity).count().unwrap(), 2);
    assert_eq!(left.diffUsing([3], parity).count().unwrap(), 1);
}

#[test]
fn lazy_merge_and_union_match_collection_key_rules() {
    let left = LazyCollection::fromEntries(|| [(ArrayKey::from(0), 2), ("x".into(), 3)]);
    let right =
        LazyCollection::fromEntries(|| [(ArrayKey::from(0), 4), ("x".into(), 5), ("y".into(), 6)]);
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
    let left = LazyCollection::fromEntries(|| {
        [("a", 1), ("a", 2), ("b", 3)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    let right = LazyCollection::fromEntries(|| [("a", 9), ("z", 10)]);
    let replaced = left.clone().replace(right);
    let mut values = replaced.getIterator();
    assert_eq!(reads.get(), 0);
    assert_eq!(values.next().unwrap().unwrap(), ("a".into(), 9));
    assert_eq!(reads.get(), 1);
    assert_eq!(values.next().unwrap().unwrap(), ("a".into(), 2));
    assert_eq!(values.next().unwrap().unwrap(), ("b".into(), 3));
    assert_eq!(values.next().unwrap().unwrap(), ("z".into(), 10));
    assert!(values.next().is_none());
    let failing = LazyCollection::tryMake(|| [Ok(4), Err(source_error())]);
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
    let source = LazyCollection::fromEntries(|| {
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
    let failing = LazyCollection::tryFromEntries(|| [Ok(("a", 1)), Err(source_error())]);
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
            .keyBy(|pair, _| pair.0)
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
            .pluckWithKeys(|pair| pair.0, |pair| pair.1)
            .values()
            .count()
            .unwrap(),
        3
    );
    let mapped = source.mapWithKeys(|pair, _| {
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
    let source = LazyCollection::fromEntries(|| [("a", 1), ("a", 4), ("b", 2), ("c", 3)]);
    let dictionary = source
        .clone()
        .mapToDictionary(|value, _| (value % 2, *value));
    assert_eq!(
        dictionary.all().unwrap(),
        vec![(0.into(), vec![4, 2]), (1.into(), vec![3])]
    );
    let groups = source.clone().mapToGroups(|value, _| (value % 2, *value));
    assert_eq!(groups.first().unwrap().unwrap().toJson().unwrap(), "[4,2]");
    let keyed = source.groupBy(|value, _| value % 2, true);
    assert_eq!(
        keyed.first().unwrap().unwrap().toJson().unwrap(),
        r#"{"a":4,"b":2}"#
    );
}

#[test]
fn lazy_key_lookups_stop_at_first_match_and_distinguish_normalized_keys() {
    let reads = Cell::new(0);
    let source = LazyCollection::fromEntries(|| {
        [("08", 1), ("8", 2), ("8", 3), ("end", 4)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(source.get(8).unwrap(), Some(2));
    assert_eq!(reads.get(), 2);
    assert_eq!(source.get("08").unwrap(), Some(1));
    assert!(source.has(8).unwrap());
    assert!(source.hasAll(["08", "8", "8"]).unwrap());
    assert!(source.hasAny(["missing", "08"]).unwrap());
    assert!(!source.has("missing").unwrap());
    assert!(source.hasAll(std::iter::empty::<ArrayKey>()).unwrap());
    assert!(
        !LazyCollection::<i32>::empty()
            .hasAll(std::iter::empty::<ArrayKey>())
            .unwrap()
    );
}

#[test]
fn lazy_neighbors_and_search_follow_occurrence_order() {
    let reads = Cell::new(0);
    let source = LazyCollection::fromEntries(|| {
        [("k", 1), ("k", 2), ("b", 3), ("c", 2)]
            .into_iter()
            .inspect(|_| reads.set(reads.get() + 1))
    });
    assert_eq!(source.search(&2).unwrap(), Some("k".into()));
    assert_eq!(reads.get(), 2);
    assert_eq!(
        source
            .searchWhere(|_, key| key == &ArrayKey::from("c"))
            .unwrap(),
        Some("c".into())
    );
    assert_eq!(source.before(&2).unwrap(), Some(1));
    assert_eq!(source.after(&2).unwrap(), Some(3));
    assert_eq!(source.before(&1).unwrap(), None);
    assert_eq!(
        source
            .afterWhere(|_, key| key == &ArrayKey::from("c"))
            .unwrap(),
        None
    );
    assert_eq!(source.last().unwrap(), Some(2));
    assert_eq!(source.lastWhere(|value, _| *value > 2).unwrap(), Some(3));
    assert!(source.some(|value, _| *value == 1).unwrap());
    assert!(source.doesntContainStrict(&10).unwrap());
}

#[test]
fn lazy_lookup_reports_errors_only_if_needed_for_the_result() {
    let failing = LazyCollection::tryFromEntries(|| [Ok(("first", 1)), Err(source_error())]);
    assert_eq!(failing.get("first").unwrap(), Some(1));
    assert!(failing.hasAny(["first", "missing"]).unwrap());
    assert!(matches!(
        failing.hasAll(["first", "missing"]),
        Err(CollectionError::Source(_))
    ));
    assert_eq!(failing.before(&1).unwrap(), None);
    assert!(matches!(failing.after(&1), Err(CollectionError::Source(_))));
    assert!(matches!(failing.last(), Err(CollectionError::Source(_))));
}
