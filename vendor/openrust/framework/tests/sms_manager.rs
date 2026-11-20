use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

use openrust::{
    container::Callback,
    contracts::config::Repository,
    foundation::Application,
    notifications::{
        NotificationFuture, NotificationResult,
        sms::{SmsDriver, SmsManager, SmsMessage},
    },
    support::facades::Sms,
};
use serde_json::{Value, json};

struct Driver(Value);
impl SmsDriver for Driver {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move { Ok(json!({"config":self.0,"content":message.content()})) })
    }
}

async fn application() -> (
    tempfile::TempDir,
    Application,
    Arc<dyn Repository>,
    Arc<SmsManager>,
) {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_path_buf())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.get::<dyn Repository>("config").await.unwrap();
    config.set("sms", json!({"default":"primary","drivers":{
        "primary":{"driver":"example","token":"first","templates":{"code":{"text":"Code {code}"}}},
        "secondary":{"driver":"example","token":"second"}
    }})).unwrap();
    let manager = app.make::<SmsManager>().await.unwrap();
    (root, app, config, manager)
}

#[tokio::test]
async fn named_drivers_are_reused_and_configuration_refresh_is_explicit() {
    let (_root, _app, config, manager) = application().await;
    let builds = Arc::new(AtomicUsize::new(0));
    let count = builds.clone();
    manager.extend("example", move |profile| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(Driver(profile.clone())))
    });
    assert_eq!(manager.getDefaultDriver().unwrap(), "primary");
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    let first = manager.default_driver().unwrap();
    manager.driver("primary").unwrap();
    manager.driver("secondary").unwrap();
    assert_eq!(builds.load(Ordering::SeqCst), 2);
    config
        .set("sms.drivers.primary.token", json!("new"))
        .unwrap();
    config
        .set(
            "sms.drivers.primary.templates.code.text",
            json!("Updated {code}"),
        )
        .unwrap();
    let cached = manager.driver("primary").unwrap();
    let send = || SmsMessage::named_template("+12025550123", "code", [("code", "42")]).unwrap();
    assert_eq!(cached.send(send()).await.unwrap()["content"], "Code 42");
    assert_eq!(
        cached.send(send()).await.unwrap()["config"]["token"],
        "first"
    );
    config.set("sms.default", json!("secondary")).unwrap();
    assert_eq!(manager.getDefaultDriver().unwrap(), "secondary");
    assert_eq!(
        manager
            .default_driver()
            .unwrap()
            .send(SmsMessage::new("+12025550123", "text").unwrap())
            .await
            .unwrap()["config"]["token"],
        "second"
    );
    manager.forgetDrivers();
    let fresh = manager.driver("primary").unwrap();
    assert_eq!(fresh.send(send()).await.unwrap()["content"], "Updated 42");
    assert_eq!(fresh.send(send()).await.unwrap()["config"]["token"], "new");
    assert_eq!(builds.load(Ordering::SeqCst), 3);
    assert_eq!(
        first.send(send()).await.unwrap()["config"]["token"],
        "first"
    );
}

#[tokio::test]
async fn concurrent_resolution_builds_one_driver_per_named_profile() {
    let (_root, _app, _config, manager) = application().await;
    let builds = Arc::new(AtomicUsize::new(0));
    let count = builds.clone();
    manager.extend("example", move |profile| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(Driver(profile.clone())))
    });
    let barrier = Arc::new(Barrier::new(12));
    std::thread::scope(|scope| {
        for _ in 0..12 {
            let barrier = barrier.clone();
            let manager = &manager;
            scope.spawn(move || {
                barrier.wait();
                manager.driver("primary").unwrap();
            });
        }
    });
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_factories_can_retry_and_replacements_require_forgetting_drivers() {
    let (_root, _app, _config, manager) = application().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    manager.extend("example", move |profile| {
        if count.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(openrust::notifications::NotificationError::Delivery {
                source: Box::new(std::io::Error::other("Temporary factory failure")),
            });
        }
        Ok(Arc::new(Driver(profile.clone())))
    });
    assert!(manager.driver("primary").is_err());
    let old = manager.driver("primary").unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    manager.extend("example", |_| {
        Ok(Arc::new(Driver(json!({"token":"replacement"}))))
    });
    let send = || SmsMessage::new("+12025550123", "test").unwrap();
    assert_eq!(
        manager
            .driver("primary")
            .unwrap()
            .send(send())
            .await
            .unwrap()["config"]["token"],
        "first"
    );
    manager.forgetDrivers();
    assert_eq!(
        manager
            .driver("primary")
            .unwrap()
            .send(send())
            .await
            .unwrap()["config"]["token"],
        "replacement"
    );
    assert_eq!(old.send(send()).await.unwrap()["config"]["token"], "first");
}

#[tokio::test]
async fn custom_factories_can_resolve_another_profile_without_a_global_cache_lock() {
    let (_root, _app, config, manager) = application().await;
    config
        .set("sms.drivers.secondary.driver", json!("other"))
        .unwrap();
    manager.extend("other", |profile| Ok(Arc::new(Driver(profile.clone()))));
    let owner = Arc::downgrade(&manager);
    manager.extend("example", move |profile| {
        owner.upgrade().unwrap().driver("secondary")?;
        Ok(Arc::new(Driver(profile.clone())))
    });
    manager.driver("primary").unwrap();
}

#[tokio::test]
async fn managers_and_facade_cache_resets_are_application_local() {
    let (_first_root, first, first_config, first_manager) = application().await;
    let (_second_root, second, second_config, second_manager) = application().await;
    first_config.set("sms.default", json!("log")).unwrap();
    second_config.set("sms.default", json!("log")).unwrap();
    first_config
        .set("sms.drivers.log", json!({"driver":"log"}))
        .unwrap();
    second_config
        .set("sms.drivers.log", json!({"driver":"log"}))
        .unwrap();
    first_manager.extend("example", |_| Ok(Arc::new(Driver(json!("first")))));
    second_manager.extend("example", |_| Ok(Arc::new(Driver(json!("second")))));
    for (manager, expected) in [(&first_manager, "first"), (&second_manager, "second")] {
        assert_eq!(
            manager
                .driver("primary")
                .unwrap()
                .send(SmsMessage::new("+12025550123", "test").unwrap())
                .await
                .unwrap()["config"],
            expected
        );
    }
    for app in [&first, &second, &first] {
        app.call(
            Callback::new(&[], |_| async {
                assert_eq!(Sms::getDefaultDriver().unwrap(), "log");
                Sms::forgetDrivers().unwrap();
                Sms::send_text("+12025550123", "local").await.unwrap();
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    }
}
