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
