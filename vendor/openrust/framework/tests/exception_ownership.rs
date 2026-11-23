use openrust::{
    container::Container,
    contracts::{
        container::Container as _,
        debug::{Exception, SharedException},
    },
    queue::QueueError,
};
use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug)]
struct OpaqueCause {
    value: Mutex<String>,
    drops: Arc<AtomicUsize>,
}
impl std::fmt::Display for OpaqueCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("原始不透明错误")
    }
}
impl Error for OpaqueCause {}
impl Drop for OpaqueCause {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn retained_queue_errors_keep_the_real_nonclone_source_and_release_it_with_the_last_owner() {
    let drops = Arc::new(AtomicUsize::new(0));
    let original = Arc::new(OpaqueCause {
        value: Mutex::new("before".into()),
        drops: drops.clone(),
    });
    let error = QueueError::Execution {
        source: original.clone(),
    };
    let retained = SharedException::new(error.clone());
    let exception = Exception::Error(&retained);
    let cause = exception.downcast_ref::<OpaqueCause>().unwrap();
    assert!(std::ptr::eq(cause, original.as_ref()));
    *original.value.lock().unwrap() = "after".into();
    assert_eq!(*cause.value.lock().unwrap(), "after");
    assert!(
        Exception::Error(&retained)
            .downcast_ref::<QueueError>()
            .is_some()
    );
    drop(original);
    drop(error);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(retained);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
#[test]
fn cloned_queue_binding_errors_keep_the_original_resolution_error_and_factories_accept_nonclone_causes()
 {
    let container = Container::default();
    let error = QueueError::from(container.make::<String>().unwrap_err());
    let cloned = error.clone();
    assert!(std::ptr::eq(
        error.source().unwrap(),
        cloned.source().unwrap()
    ));
    assert!(
        cloned
            .source()
            .unwrap()
            .is::<openrust::contracts::container::BindingResolutionError>()
    );
    let execution = QueueError::execution(std::io::Error::other("job-source"));
    assert!(execution.clone().source().unwrap().is::<std::io::Error>());
    let backend = QueueError::backend(std::io::Error::other("backend-source"));
    assert_eq!(
        backend.clone().source().unwrap().to_string(),
        "backend-source"
    );
}
