use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

use openrust::{
    container::Container,
    contracts::container::{BindingResolutionError, Container as _},
};

#[test]
fn singleton_retries_after_failure_and_shares_the_first_success() {
    let container = Container::default();
    let calls = AtomicUsize::new(0);
    container.singleton::<usize, _>(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(BindingResolutionError::NotBound { service: "probe" });
        }
        Ok(Arc::new(42))
    });
    assert!(container.make::<usize>().is_err());

    let gate = Barrier::new(4);
    let instances = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    gate.wait();
                    container.make::<usize>().unwrap()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(
        instances
            .iter()
            .all(|instance| Arc::ptr_eq(instance, &instances[0]))
    );
    assert_eq!(*instances[0], 42);
    let replacement = Arc::new(7);
    container.instance(replacement.clone());
    assert!(Arc::ptr_eq(
        &container.make::<usize>().unwrap(),
        &replacement
    ));
}

#[test]
fn transient_factory_and_cycles() {
    let container = Container::default();
    container.bind::<usize, _>(|_| Ok(Arc::new(7)));
    assert!(!Arc::ptr_eq(
        &container.make::<usize>().unwrap(),
        &container.make::<usize>().unwrap()
    ));
    container.singleton::<usize, _>(|container| container.make::<usize>());
    assert!(matches!(
        container.make::<usize>(),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
}

#[test]
fn cross_thread_singleton_cycles_report_an_error() {
    let container = Arc::new(Container::default());
    let gate = Arc::new(Barrier::new(2));
    for binding in [false, true] {
        let gate = Arc::clone(&gate);
        let attempts = AtomicUsize::new(0);
        if binding {
            container.singleton::<u32, _>(move |container| {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    gate.wait();
                }
                container.make::<u64>().map(|_| Arc::new(0))
            });
        } else {
            container.singleton::<u64, _>(move |container| {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    gate.wait();
                }
                container.make::<u32>().map(|_| Arc::new(0))
            });
        }
    }
    let (send, receive) = std::sync::mpsc::channel();
    let other_container = Arc::clone(&container);
    let other_send = send.clone();
    let first = std::thread::spawn(move || {
        other_send
            .send(other_container.make::<u32>().map(|_| ()))
            .unwrap()
    });
    let second =
        std::thread::spawn(move || send.send(container.make::<u64>().map(|_| ())).unwrap());
    for _ in 0..2 {
        assert!(matches!(
            receive
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            Err(BindingResolutionError::CircularDependency { .. })
        ));
    }
    first.join().unwrap();
    second.join().unwrap();
}
