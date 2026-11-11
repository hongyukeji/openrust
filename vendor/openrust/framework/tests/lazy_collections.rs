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
