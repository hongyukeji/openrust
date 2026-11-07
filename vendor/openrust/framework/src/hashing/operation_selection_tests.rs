//! 最终叶选择回归：未验证链不发布，失败可重试且不留下候选强环。

use super::*;
use std::sync::{
    Barrier,
    atomic::{AtomicUsize, Ordering},
};

fn manager() -> Result<Arc<HashManager>> {
    Ok(Arc::new(HashManager::new(HashingConfig {
        driver: "next".into(),
        ..HashingConfig::default()
    })?))
}
fn leaf() -> Result<Arc<dyn Hasher>> {
    Ok(Arc::new(ArgonHasher::new(
        super::super::ArgonConfig::default(),
    )?))
}
fn link(from: &HashManager, to: &Arc<HashManager>) -> Result<()> {
    let weak = Arc::downgrade(to);
    from.extend("next", move || {
        weak.upgrade()
            .map(|value| value as Arc<dyn Hasher>)
            .ok_or_else(|| failure("test_manager_missing", "测试管理器已释放"))
    })
}
fn assert_empty(manager: &HashManager) -> Result<()> {
    let state = manager
        .state
        .lock()
        .map_err(|_| failure("test_poisoned", "测试锁失效"))?;
    assert!(state.drivers.is_empty());
    assert!(state.creating.is_empty());
    Ok(())
}

#[test]
fn one_and_three_layers_publish_original_candidates_and_keep_cached_identity() -> Result<()> {
    let end = manager()?;
    let concrete = leaf()?;
    let candidate = Arc::clone(&concrete);
    end.extend("next", move || Ok(Arc::clone(&candidate)))?;
    assert!(Arc::ptr_eq(&end.driver_for_operation()?, &concrete));
    let middle = manager()?;
    let root = manager()?;
    link(&middle, &end)?;
    link(&root, &middle)?;
    assert!(Arc::ptr_eq(&root.driver_for_operation()?, &concrete));
    assert!(Arc::ptr_eq(
        &root.driver(None)?,
        &(Arc::clone(&middle) as Arc<dyn Hasher>)
    ));
    assert!(Arc::ptr_eq(
        &middle.driver(None)?,
        &(Arc::clone(&end) as Arc<dyn Hasher>)
    ));
    assert!(Arc::ptr_eq(&root.driver_for_operation()?, &concrete));
    Ok(())
}

#[test]
fn indirect_cycle_is_unpublished_releases_candidates_and_can_be_repaired() -> Result<()> {
    let first = manager()?;
    let second = manager()?;
    let first_weak = Arc::downgrade(&first);
    let second_weak = Arc::downgrade(&second);
    link(&first, &second)?;
    link(&second, &first)?;
    assert!(
        matches!(first.driver_for_operation(), Err(error) if error.code() == "hashing_driver_self_reference")
    );
    assert_empty(&first)?;
    assert_empty(&second)?;
    let concrete = leaf()?;
    let candidate = Arc::clone(&concrete);
    second.extend("next", move || Ok(Arc::clone(&candidate)))?;
    assert!(Arc::ptr_eq(&first.driver_for_operation()?, &concrete));
    drop(first);
    drop(second);
    assert!(first_weak.upgrade().is_none());
    assert!(second_weak.upgrade().is_none());
    Ok(())
}

#[test]
fn direct_self_keeps_original_sentinel_until_explicit_extend() -> Result<()> {
    let root = manager()?;
    link(&root, &root)?;
    for _ in 0..2 {
        assert!(
            matches!(root.driver_for_operation(), Err(error) if error.code() == "hashing_driver_self_reference")
        );
        assert_empty(&root)?;
    }
    root.extend("next", leaf)?;
    assert!(root.driver_for_operation()?.as_hash_manager().is_none());
    Ok(())
}

#[test]
fn previously_cached_cycle_is_rejected_without_mutating_cache() -> Result<()> {
    let first = manager()?;
    let second = manager()?;
    link(&first, &second)?;
    link(&second, &first)?;
    let first_edge = first.driver(None)?;
    let second_edge = second.driver(None)?;
    assert!(
        matches!(first.driver_for_operation(), Err(error) if error.code() == "hashing_driver_self_reference")
    );
    assert!(Arc::ptr_eq(&first_edge, &first.driver(None)?));
    assert!(Arc::ptr_eq(&second_edge, &second.driver(None)?));
    // 测试显式释放历史缓存环；操作入口不承诺回收历史全缓存强图。
    let removed = first
        .state
        .lock()
        .map_err(|_| failure("test_poisoned", "测试锁失效"))?
        .drivers
        .remove("next");
    drop(removed);
    Ok(())
}

#[test]
fn diamond_shared_tail_is_not_a_cycle() -> Result<()> {
    let left = manager()?;
    let right = manager()?;
    let tail = manager()?;
    tail.extend("next", leaf)?;
    link(&left, &tail)?;
    link(&right, &tail)?;
    assert!(Arc::ptr_eq(
        &left.driver_for_operation()?,
        &right.driver_for_operation()?
    ));
    Ok(())
}

#[test]
fn failing_inner_factory_does_not_publish_outer_and_retries_original_factory() -> Result<()> {
    let root = manager()?;
    let inner = manager()?;
    link(&root, &inner)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&calls);
    inner.extend("next", move || {
        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(failure("test_factory_failure", "测试工厂失败"))
        } else {
            leaf()
        }
    })?;
    assert!(
        matches!(root.driver_for_operation(), Err(error) if error.code() == "test_factory_failure")
    );
    assert_empty(&root)?;
    assert_empty(&inner)?;
    assert!(root.driver_for_operation()?.as_hash_manager().is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn two_roots_constructing_opposite_edges_fail_busy_without_publication() -> Result<()> {
    let first = manager()?;
    let second = manager()?;
    let gate = Arc::new(Barrier::new(2));
    for (from, to) in [(&first, &second), (&second, &first)] {
        let weak = Arc::downgrade(to);
        let barrier = Arc::clone(&gate);
        from.extend("next", move || {
            barrier.wait();
            weak.upgrade()
                .map(|value| value as Arc<dyn Hasher>)
                .ok_or_else(|| failure("test_manager_missing", "测试管理器已释放"))
        })?;
    }
    // 第二道闸使两个原始占位都保持到双方 prepare 已真实返回 busy，避免失败方提前释放后重入工厂。
    let held = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        let run = |root: &HashManager, held: &Barrier| -> Result<()> {
            let prepared = root.prepare("next")?;
            let nested = prepared
                .driver
                .as_hash_manager()
                .ok_or_else(self_reference)?;
            let result = nested.prepare("next");
            assert!(matches!(result, Err(error) if error.code() == "hashing_driver_busy"));
            held.wait();
            drop(prepared);
            Ok(())
        };
        let first = first.as_ref();
        let second = second.as_ref();
        let held = held.as_ref();
        let a = scope.spawn(move || run(first, held));
        let b = scope.spawn(move || run(second, held));
        a.join()
            .map_err(|_| failure("test_thread_failed", "测试线程失败"))??;
        b.join()
            .map_err(|_| failure("test_thread_failed", "测试线程失败"))?
    })?;
    assert_empty(&first)?;
    assert_empty(&second)?;
    Ok(())
}

#[test]
fn concurrent_public_selection_rejects_opposite_roots_and_releases_both() -> Result<()> {
    let first = manager()?;
    let second = manager()?;
    let gate = Arc::new(Barrier::new(2));
    for (from, to) in [(&first, &second), (&second, &first)] {
        let weak = Arc::downgrade(to);
        let barrier = Arc::clone(&gate);
        let calls = AtomicUsize::new(0);
        from.extend("next", move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                barrier.wait();
            }
            weak.upgrade()
                .map(|value| value as Arc<dyn Hasher>)
                .ok_or_else(|| failure("test_manager_missing", "测试管理器已释放"))
        })?;
    }
    std::thread::scope(|scope| {
        let a = scope.spawn(|| first.driver_for_operation());
        let b = scope.spawn(|| second.driver_for_operation());
        let mut busy = 0;
        for joined in [a.join(), b.join()] {
            let outcome = joined.map_err(|_| failure("test_thread_failed", "测试线程失败"))?;
            if let Err(error) = &outcome {
                busy += usize::from(error.code() == "hashing_driver_busy");
            }
            assert!(
                matches!(outcome, Err(error) if matches!(error.code(), "hashing_driver_busy" | "hashing_driver_self_reference"))
            );
        }
        assert!(busy >= 1);
        Ok::<(), crate::core::Error>(())
    })?;
    assert_empty(&first)?;
    assert_empty(&second)?;
    Ok(())
}

struct Projection {
    manager: Option<Arc<HashManager>>,
    delegate: Arc<dyn Hasher>,
    hooks: Arc<AtomicUsize>,
}
impl Hasher for Projection {
    fn as_hash_manager(&self) -> Option<&HashManager> {
        self.hooks.fetch_add(1, Ordering::SeqCst);
        self.manager.as_deref()
    }
    fn info(&self, encoded: &str) -> Result<HashInfo> {
        self.delegate.info(encoded)
    }
    fn make<'a>(
        &'a self,
        value: &'a [u8],
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<String>> {
        self.delegate.make(value, options)
    }
    fn check<'a>(
        &'a self,
        value: &'a [u8],
        encoded: Option<&'a str>,
        options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<bool>> {
        self.delegate.check(value, encoded, options)
    }
    fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
        self.delegate.needs_rehash(encoded, options)
    }
}

#[test]
fn operation_projects_each_candidate_once_on_cold_and_cached_paths() -> Result<()> {
    let root = manager()?;
    let inner = manager()?;
    let wrapper_calls = Arc::new(AtomicUsize::new(0));
    let leaf_calls = Arc::new(AtomicUsize::new(0));
    let concrete: Arc<dyn Hasher> = Arc::new(Projection {
        manager: None,
        delegate: leaf()?,
        hooks: Arc::clone(&leaf_calls),
    });
    let candidate = Arc::clone(&concrete);
    inner.extend("next", move || Ok(Arc::clone(&candidate)))?;
    let wrapper: Arc<dyn Hasher> = Arc::new(Projection {
        manager: Some(Arc::clone(&inner)),
        delegate: inner,
        hooks: Arc::clone(&wrapper_calls),
    });
    let candidate = Arc::clone(&wrapper);
    root.extend("next", move || Ok(Arc::clone(&candidate)))?;
    for expected in 1..=2 {
        assert!(Arc::ptr_eq(&root.driver_for_operation()?, &concrete));
        assert_eq!(wrapper_calls.load(Ordering::SeqCst), expected);
        assert_eq!(leaf_calls.load(Ordering::SeqCst), expected);
    }
    assert!(Arc::ptr_eq(&root.driver(None)?, &wrapper));
    assert_eq!(wrapper_calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn ordinary_named_wrapper_can_project_owner_without_hook_or_self_sentinel() -> Result<()> {
    let owner = manager()?;
    owner.extend("next", leaf)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let wrapper: Arc<dyn Hasher> = Arc::new(Projection {
        manager: Some(Arc::clone(&owner)),
        delegate: Arc::clone(&owner) as Arc<dyn Hasher>,
        hooks: Arc::clone(&calls),
    });
    let candidate = Arc::clone(&wrapper);
    owner.extend("x", move || Ok(Arc::clone(&candidate)))?;
    let actual = owner.driver(Some("x"))?;
    assert!(Arc::ptr_eq(&actual, &wrapper));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(actual.info("not-a-hash")?.algo.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // 普通具名 wrapper 可合法持 owner；本测试显式释放其历史强边。
    let cached = owner
        .state
        .lock()
        .map_err(|_| failure("test_poisoned", "测试锁失效"))?
        .drivers
        .remove("x");
    let factory = owner
        .state
        .lock()
        .map_err(|_| failure("test_poisoned", "测试锁失效"))?
        .factories
        .remove("x");
    drop(cached);
    drop(factory);
    Ok(())
}

type FactoryDropObservations = Arc<Mutex<Vec<Result<Arc<dyn Hasher>>>>>;

struct FactoryDropProbe {
    manager: std::sync::Weak<HashManager>,
    observations: FactoryDropObservations,
}
impl Drop for FactoryDropProbe {
    fn drop(&mut self) {
        let result = self
            .manager
            .upgrade()
            .ok_or_else(|| failure("test_manager_missing", "测试管理器已释放"))
            .and_then(|manager| manager.driver(Some("x")));
        if let Ok(mut observations) = self.observations.lock() {
            observations.push(result);
        }
    }
}

#[test]
fn last_attempted_factory_drop_reenters_published_cache_after_concurrent_extend() -> Result<()> {
    let root = manager()?;
    let concrete = leaf()?;
    let entered = Arc::new(Barrier::new(2));
    let released = Arc::new(Barrier::new(2));
    let observations = Arc::new(Mutex::new(Vec::new()));
    let probe = FactoryDropProbe {
        manager: Arc::downgrade(&root),
        observations: Arc::clone(&observations),
    };
    let candidate = Arc::clone(&concrete);
    let entry = Arc::clone(&entered);
    let release = Arc::clone(&released);
    root.extend("x", move || {
        let _keep = &probe;
        entry.wait();
        release.wait();
        Ok(Arc::clone(&candidate))
    })?;
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| root.driver(Some("x")));
        entered.wait();
        root.extend("x", leaf)?;
        let replacement = root
            .state
            .lock()
            .map_err(|_| failure("test_poisoned", "测试锁失效"))?
            .factories
            .get("x")
            .cloned()
            .ok_or_else(|| failure("test_factory_missing", "测试工厂缺失"))?;
        assert!(
            observations
                .lock()
                .map_err(|_| failure("test_poisoned", "测试锁失效"))?
                .is_empty()
        );
        released.wait();
        let actual = worker
            .join()
            .map_err(|_| failure("test_thread_failed", "测试线程失败"))??;
        assert!(Arc::ptr_eq(&actual, &concrete));
        let results = observations
            .lock()
            .map_err(|_| failure("test_poisoned", "测试锁失效"))?;
        assert_eq!(results.len(), 1);
        let observed = results[0]
            .as_ref()
            .map_err(|_| failure("test_reentry_failed", "析构重入未命中缓存"))?;
        assert!(Arc::ptr_eq(observed, &concrete));
        let state = root
            .state
            .lock()
            .map_err(|_| failure("test_poisoned", "测试锁失效"))?;
        assert!(
            state
                .factories
                .get("x")
                .is_some_and(|current| Arc::ptr_eq(current, &replacement))
        );
        Ok::<(), crate::core::Error>(())
    })?;
    Ok(())
}
