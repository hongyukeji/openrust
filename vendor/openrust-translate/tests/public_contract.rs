use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

use openrust::{config::Repository, contracts::config::Repository as _};
use openrust_translate::{
    TranslateManager, Translation, TranslationError, TranslationFuture, TranslationRequest,
    Translator,
};
use serde_json::json;

struct Driver(String);
impl Translator for Driver {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move { Ok(Translation::new(format!("{}{}", self.0, request.text()))) })
    }
}

#[test]
fn validates_tags_and_hides_both_texts_in_debug() {
    assert!(matches!(
        TranslationRequest::new("   ", "fr"),
        Err(TranslationError::EmptyText)
    ));
    assert!(matches!(
        TranslationRequest::new("secret source", "zh_CN"),
        Err(TranslationError::InvalidLanguage)
    ));
    let request = TranslationRequest::new(" secret source ", "EN-us")
        .unwrap()
        .with_source("ZH-hans")
        .unwrap();
    assert_eq!(request.target(), "en-US");
    assert_eq!(request.source(), Some("zh-Hans"));
    assert_eq!(request.text(), " secret source ");
    assert!(!format!("{request:?}").contains("secret source"));
    assert!(!format!("{:?}", Translation::new("secret result")).contains("secret result"));
}

#[tokio::test]
async fn concurrent_profiles_retry_replace_and_keep_previous_handles() {
    let repository = Arc::new(Repository::new(Default::default()));
    let manager = Arc::new(TranslateManager::new(repository.clone()));
    assert!(matches!(
        manager.default_driver(),
        Err(TranslationError::DefaultNotConfigured)
    ));
    assert!(matches!(
        manager.driver("different"),
        Err(TranslationError::ProfileNotConfigured)
    ));
    repository
        .set(
            "translate",
            json!({
                "default": "a", "drivers": {
                    "a": {"driver":"example","label":"first"},
                    "b": {"driver":"other"}
                }
            }),
        )
        .unwrap();
    assert!(matches!(
        manager.default_driver(),
        Err(TranslationError::DriverNotRegistered)
    ));

    let calls = Arc::new(AtomicUsize::new(0));
    manager.extend("other", |_| Ok(Arc::new(Driver("nested".into()))));
    let weak = Arc::downgrade(&manager);
    let factory_calls = calls.clone();
    manager.extend("example", move |profile| {
        let attempt = factory_calls.fetch_add(1, Ordering::Relaxed);
        if attempt == 0 {
            return Err(TranslationError::Unavailable);
        }
        // 解析其他 profile 时不占用管理器的全局锁。
        weak.upgrade().unwrap().driver("b")?;
        Ok(Arc::new(Driver(profile["label"].as_str().unwrap().into())))
    });

    assert!(matches!(
        manager.default_driver(),
        Err(TranslationError::Unavailable)
    ));
    let first = std::thread::scope(|scope| {
        let barrier = Arc::new(Barrier::new(12));
        let threads: Vec<_> = (0..12)
            .map(|_| {
                let barrier = barrier.clone();
                let manager = manager.clone();
                scope.spawn(move || {
                    barrier.wait();
                    manager.default_driver().unwrap()
                })
            })
            .collect();
        let drivers: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(
            drivers
                .iter()
                .all(|driver| Arc::ptr_eq(driver, &drivers[0]))
        );
        drivers[0].clone()
    });
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(Arc::ptr_eq(&first, &manager.driver("a").unwrap()));
    assert!(!Arc::ptr_eq(&first, &manager.driver("b").unwrap()));

    let other_app = TranslateManager::new(repository.clone());
    assert!(matches!(
        other_app.default_driver(),
        Err(TranslationError::DriverNotRegistered)
    ));
    repository
        .set("translate.drivers.a.label", json!("second"))
        .unwrap();
    manager.extend("example", |profile| {
        assert_eq!(profile["label"], "second");
        Ok(Arc::new(Driver("replacement".into())))
    });
    assert!(Arc::ptr_eq(&first, &manager.default_driver().unwrap()));
    manager.forget_drivers();
    let updated = manager.default_driver().unwrap();
    assert!(!Arc::ptr_eq(&first, &updated));
    let request = TranslationRequest::new("text", "en").unwrap();
    assert_eq!(
        first.translate(request.clone()).await.unwrap().text(),
        "firsttext"
    );
    assert_eq!(
        updated.translate(request).await.unwrap().text(),
        "replacementtext"
    );
}
