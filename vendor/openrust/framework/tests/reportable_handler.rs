use openrust::{
    contracts::debug::Exception,
    foundation::{configuration::ReportDecision, exceptions::ReportableHandler},
    support::facades::{App, Exceptions, Facade},
    testing::TestApp,
};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Debug)]
struct Failure;
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("可报告失败")
    }
}
impl Error for Failure {}
#[test]
fn public_handler_checks_types_invokes_callbacks_and_shares_stop_across_clones() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let handler = ReportableHandler::for_type::<Failure>(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        ReportDecision::Continue
    });
    assert!(handler.handles(&Exception::Error(&Failure)));
    assert!(!handler.handles(&Exception::Error(&std::io::Error::other("other"))));
    assert!(
        handler
            .invoke(&Exception::Error(&std::io::Error::other("wrong")))
            .is_err()
    );
    assert!(handler.invoke(&Exception::Error(&Failure)).unwrap());
    let cloned = handler.clone();
    cloned.stop();
    assert!(!handler.invoke(&Exception::Error(&Failure)).unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(
        !ReportableHandler::for_type::<Failure>(|_| ReportDecision::Stop)
            .invoke(&Exception::Error(&Failure))
            .unwrap()
    );
}
#[tokio::test]
async fn facade_stop_changes_the_registered_callback_without_re_registration() {
    let mut app = TestApp::create(|builder| builder).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let first = Arc::new(AtomicUsize::new(0));
        let later = Arc::new(AtomicUsize::new(0));
        let count = first.clone();
        let handle = Exceptions::reportable::<Failure>(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Continue
        })
        .unwrap();
        let count = later.clone();
        Exceptions::reportable::<Failure>(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Stop
        })
        .unwrap();
        Exceptions::report(Failure).await.unwrap();
        assert_eq!(first.load(Ordering::SeqCst), 1);
        assert_eq!(later.load(Ordering::SeqCst), 1);
        handle.stop();
        Exceptions::report(Failure).await.unwrap();
        assert_eq!(first.load(Ordering::SeqCst), 2);
        assert_eq!(later.load(Ordering::SeqCst), 1);
    })
    .await;
}

#[tokio::test]
async fn all_error_callbacks_work_through_the_facade_without_type_specific_registration() {
    let mut app = TestApp::create(|builder| builder).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let seen = Arc::new(AtomicUsize::new(0));
        let observed = seen.clone();
        Exceptions::reportable_all(move |exception| {
            assert!(
                exception.downcast_ref::<Failure>().is_some()
                    || exception.downcast_ref::<std::io::Error>().is_some()
            );
            observed.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Stop
        })
        .unwrap();
        Exceptions::report(Failure).await.unwrap();
        Exceptions::report(std::io::Error::other("other"))
            .await
            .unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    })
    .await;
}
