use openrust::container::Container;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct DropProbe(Arc<AtomicUsize>);
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn scoped_future_size_is_bounded_for_large_operations_and_preserves_output_and_drop() {
    let container = Container::default();
    let dropped = Arc::new(AtomicUsize::new(0));
    let payload = std::hint::black_box([7u8; 32 * 1024]);
    let probe = DropProbe(dropped.clone());
    let operation = async move {
        let _probe = probe;
        tokio::task::yield_now().await;
        assert_eq!(payload[0], 7);
        payload[payload.len() - 1]
    };
    assert!(std::mem::size_of_val(&operation) >= 32 * 1024);
    let scoped = container.scope(operation);
    assert!(
        std::mem::size_of_val(&scoped) < 4096,
        "业务Future不能在每层作用域中被内联复制"
    );
    assert_eq!(scoped.await, 7);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelling_an_unpolled_large_scope_drops_the_operation_once() {
    let container = Container::default();
    let dropped = Arc::new(AtomicUsize::new(0));
    let payload = std::hint::black_box([9u8; 32 * 1024]);
    let probe = DropProbe(dropped.clone());
    let operation = async move {
        let _probe = probe;
        std::future::pending::<()>().await;
        payload[0]
    };
    let scoped = container.scope(operation);
    assert!(std::mem::size_of_val(&scoped) < 4096);
    drop(scoped);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn lifecycle_scopes_preserve_support_for_local_non_send_futures() {
    let container = Container::default();
    let value = std::rc::Rc::new(17);
    let borrowed = value.clone();
    let result = container
        .scope(async move {
            tokio::task::yield_now().await;
            *borrowed
        })
        .await;
    assert_eq!(result, 17);
    assert_eq!(std::rc::Rc::strong_count(&value), 1);
}
