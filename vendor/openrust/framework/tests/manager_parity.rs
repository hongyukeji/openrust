use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    mail::MailManager,
    session::SessionManager,
    testing::TestApp,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn forgetting_mailers_releases_resolved_instances_and_recreates_all_names() {
    let mut app=TestApp::create(|builder| builder.with_config(|_,_| Ok([
        ("app".into(),json!({"env":"testing"})),
        ("mail".into(),json!({"default":"first","mailers":{"first":{"transport":"array"},"second":{"transport":"array"}}})),
    ].into()))).unwrap();
    app.boot().await.unwrap();
    let manager = app.app().container().make::<MailManager>().unwrap();
    let first = manager.mailer(None).unwrap();
    let second = manager.mailer(Some("second")).unwrap();
    assert!(Arc::ptr_eq(&first, &manager.mailer(Some("first")).unwrap()));
    let weak_first = Arc::downgrade(&first);
    let weak_second = Arc::downgrade(&second);
    drop(first);
    drop(second);
    assert!(weak_first.upgrade().is_some());
    assert!(std::ptr::eq(manager.as_ref(), manager.forget_mailers()));
    assert!(weak_first.upgrade().is_none());
    assert!(weak_second.upgrade().is_none());
    let first = manager.mailer(None).unwrap();
    let second = manager.mailer(Some("second")).unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    manager.forget_mailers();
    assert!(!Arc::ptr_eq(&first, &manager.mailer(None).unwrap()));
    // 外部持有的 Arc 不因缓存清除而失效。
    assert!(Arc::strong_count(&first) >= 1);
}

#[tokio::test]
async fn session_introspection_matches_defaults_overrides_and_application_isolation() {
    for config in [
        None,
        Some(
            json!({"driver":"array","block":true,"block_store":"file","block_lock_seconds":7,"block_wait_seconds":3,"custom":{"retained":true}}),
        ),
    ] {
        let expected = config.clone();
        let mut app = TestApp::create(|builder| {
            if config.is_none() { return builder; }
            builder.with_config(|_, _| Ok([
                ("app".into(), json!({"env":"testing"})),
                ("session".into(), json!({"driver":"array","block":true,"block_store":"file","block_lock_seconds":7,"block_wait_seconds":3,"custom":{"retained":true}})),
            ].into()))
        }).unwrap();
        app.boot().await.unwrap();
        let manager = app.app().container().make::<SessionManager>().unwrap();
        assert_eq!(manager.should_block(), expected.is_some());
        assert_eq!(manager.block_driver(), expected.as_ref().map(|_| "file"));
        assert_eq!(
            manager.default_route_block_lock_seconds(),
            if expected.is_some() { 7 } else { 10 }
        );
        assert_eq!(
            manager.default_route_block_wait_seconds(),
            if expected.is_some() { 3 } else { 10 }
        );
        assert_eq!(manager.get_session_config(), &expected.unwrap_or_default());
    }
}
