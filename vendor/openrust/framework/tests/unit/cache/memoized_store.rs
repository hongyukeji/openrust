use super::{MemoizedStore, Observation, Repository, TagSet, Value};
use crate::cache::ArrayStore;
use std::sync::Arc;

fn store() -> MemoizedStore {
    MemoizedStore {
        repository: Arc::new(Repository::new(Arc::new(ArrayStore::new()))),
        state: Arc::default(),
        lifetime: None,
        tags: None,
    }
}

fn pending(store: &MemoizedStore, key: &str) -> Option<(u64, u64)> {
    match store.observe(key) {
        Observation::Miss(stamp) => stamp,
        Observation::Hit(_) => panic!("unexpected memoized hit"),
    }
}

#[test]
fn pending_read_cannot_repopulate_a_written_key() {
    let store = store();
    let stale = pending(&store, "key");
    let unrelated = pending(&store, "other");
    let write = store.begin_write("key");
    assert_eq!(pending(&store, "key"), None);
    drop(write);
    let old = Some(Value::from("old"));
    assert_eq!(store.record("key", stale, old.clone()), old);
    assert_eq!(store.record("other", unrelated, None), None);
    assert!(matches!(store.observe("other"), Observation::Hit(None)));
    let fresh = pending(&store, "key");
    assert_ne!(fresh, stale);
    assert_eq!(
        store.record("key", fresh, Some("new".into())),
        Some("new".into())
    );
}

#[test]
fn flush_invalidates_misses_and_reads_overlapping_flush() {
    let store = store();
    let miss = pending(&store, "miss");
    assert_eq!(store.record("miss", miss, None), None);
    let stale = pending(&store, "key");
    let flush = store.begin_flush();
    assert_eq!(pending(&store, "key"), None);
    drop(flush);
    assert_ne!(pending(&store, "miss"), miss);
    let old = Some(Value::from("old"));
    assert_eq!(store.record("key", stale, old.clone()), old);
    assert_ne!(pending(&store, "key"), stale);
}

#[test]
fn tagged_flush_invalidates_pending_reads_in_every_tag_group() {
    let parent = store();
    let tagged = |name| {
        let tags = Arc::new(TagSet::new(parent.repository.getStore(), &[name]));
        parent
            .state()
            .tagged
            .entry(tags.getNames().to_vec())
            .or_default();
        MemoizedStore {
            repository: Arc::clone(&parent.repository),
            state: Arc::clone(&parent.state),
            lifetime: None,
            tags: Some(tags),
        }
    };
    let first = tagged("first");
    let second = tagged("second");
    let stale = pending(&second, "key");
    let plain = pending(&parent, "key");
    let flush = first.begin_flush();
    let late = tagged("late");
    assert_eq!(pending(&late, "key"), None);
    drop(flush);
    second.record("key", stale, Some("stale".into()));
    assert_ne!(pending(&second, "key"), stale);
    parent.record("key", plain, Some("plain".into()));
    assert!(matches!(parent.observe("key"), Observation::Hit(Some(_))));
}
