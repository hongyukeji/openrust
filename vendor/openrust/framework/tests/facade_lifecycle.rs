use openrust::{
    container::ServiceKey,
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    support::facades::{App, Facade},
    testing::TestApp,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Item(usize);
struct ItemFacade;
impl Facade for ItemFacade {
    type Service = Item;
    fn get_facade_accessor() -> ServiceKey {
        "facade.item".into()
    }
}

async fn fixture() -> TestApp {
    let mut app = TestApp::create(|builder| builder).unwrap();
    app.boot().await.unwrap();
    let sequence = Arc::new(AtomicUsize::new(0));
    app.app()
        .container()
        .bind_named("facade.item", move |_| {
            Ok(Arc::new(Item(sequence.fetch_add(1, Ordering::SeqCst) + 1)))
        })
        .unwrap();
    app
}

#[tokio::test]
async fn resolved_runs_lazily_then_immediately_and_observes_real_container_resolution() {
    let fixture = fixture().await;
    let trace = Arc::new(Mutex::new(Vec::new()));
    let retained = Arc::new(Mutex::new(None));
    App::set_facade_application(Some(fixture.app()), async {
        let trace_first = trace.clone();
        let observed = retained.clone();
        ItemFacade::resolved(move |service, container| {
            assert!(container.has("facade.item"));
            *observed.lock().unwrap() = Some(service.clone());
            trace_first.lock().unwrap().push(("first", service.0));
            Ok(())
        })
        .unwrap();
        assert!(trace.lock().unwrap().is_empty());
        assert_eq!(ItemFacade::get_facade_root().unwrap().0, 1);
        assert!(Arc::ptr_eq(
            &ItemFacade::get_facade_root().unwrap(),
            retained.lock().unwrap().as_ref().unwrap()
        ));
        assert_eq!(ItemFacade::get_facade_root().unwrap().0, 1);
        let trace_second = trace.clone();
        ItemFacade::resolved(move |service, _| {
            trace_second.lock().unwrap().push(("second", service.0));
            Ok(())
        })
        .unwrap();
        assert_eq!(*trace.lock().unwrap(), [("first", 1), ("second", 1)]);
        ItemFacade::clear_resolved_instance(None).unwrap();
        assert_eq!(ItemFacade::get_facade_root().unwrap().0, 2);
        assert_eq!(
            *trace.lock().unwrap(),
            [("first", 1), ("second", 1), ("first", 2), ("second", 2)]
        );
    })
    .await;
    assert!(App::get_facade_application().unwrap().is_none());
    assert!(ItemFacade::resolved(|_, _| Ok(())).is_err());
}

#[tokio::test]
async fn resolved_propagates_callback_failure_without_registering_an_immediate_failed_hook() {
    let fixture = fixture().await;
    App::set_facade_application(Some(fixture.app()), async {
        ItemFacade::get_facade_root().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let recorded = calls.clone();
        let error = ItemFacade::resolved(move |_, _| {
            recorded.fetch_add(1, Ordering::SeqCst);
            Err(BindingResolutionError::FacadeNotSet)
        });
        assert!(error.is_err());
        ItemFacade::clear_resolved_instance(None).unwrap();
        ItemFacade::get_facade_root().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    })
    .await;
}

#[tokio::test]
async fn application_replacement_is_nested_isolated_and_restored_on_unwind_and_cancellation() {
    let first = fixture().await;
    let second = fixture().await;
    App::set_facade_application(Some(first.app()), async {
        let first_container = App::get_container().unwrap();
        assert_eq!(ItemFacade::get_facade_root().unwrap().0, 1);
        App::set_facade_application(Some(second.app()), async {
            assert!(!Arc::ptr_eq(
                &first_container,
                &App::get_container().unwrap()
            ));
            assert_eq!(ItemFacade::get_facade_root().unwrap().0, 1);
            assert!(App::get_facade_application().unwrap().is_some());
            App::set_facade_application(None, async {
                assert!(App::get_facade_application().unwrap().is_none());
                assert!(ItemFacade::get_facade_root().is_err());
            })
            .await;
            assert_eq!(ItemFacade::get_facade_root().unwrap().0, 1);
        })
        .await;
        assert!(Arc::ptr_eq(
            &first_container,
            &App::get_container().unwrap()
        ));
        assert_eq!(ItemFacade::get_facade_root().unwrap().0, 1);
        let timeout = tokio::time::timeout(
            std::time::Duration::from_millis(1),
            App::set_facade_application(Some(second.app()), std::future::pending::<()>()),
        )
        .await;
        assert!(timeout.is_err());
        assert!(Arc::ptr_eq(
            &first_container,
            &App::get_container().unwrap()
        ));
        use futures_util::FutureExt as _;
        let result =
            std::panic::AssertUnwindSafe(App::set_facade_application(Some(second.app()), async {
                panic!("fixture panic")
            }))
            .catch_unwind()
            .await;
        assert!(result.is_err());
        assert!(Arc::ptr_eq(
            &first_container,
            &App::get_container().unwrap()
        ));
    })
    .await;
    assert!(App::get_facade_application().unwrap().is_none());
}

#[tokio::test]
async fn parallel_applications_do_not_share_roots_or_resolution_callbacks() {
    let first = fixture().await;
    let second = fixture().await;
    second
        .app()
        .container()
        .bind_named("facade.item", |_| Ok(Arc::new(Item(21))))
        .unwrap();
    // 由公开应用入口取得所属应用的共享句柄，避免构造平行容器。
    let one = App::set_facade_application(Some(first.app()), async {
        App::get_facade_application().unwrap().unwrap()
    })
    .await;
    let two = App::set_facade_application(Some(second.app()), async {
        App::get_facade_application().unwrap().unwrap()
    })
    .await;
    let one = tokio::spawn(async move {
        App::set_facade_application(Some(&one), async {
            ItemFacade::resolved(|item, _| {
                assert_eq!(item.0, 1);
                Ok(())
            })
            .unwrap();
            tokio::task::yield_now().await;
            assert_eq!(ItemFacade::get_facade_root().unwrap().0, 1);
        })
        .await;
    });
    let two = tokio::spawn(async move {
        App::set_facade_application(Some(&two), async {
            ItemFacade::resolved(|item, _| {
                assert_eq!(item.0, 21);
                Ok(())
            })
            .unwrap();
            tokio::task::yield_now().await;
            assert_eq!(ItemFacade::get_facade_root().unwrap().0, 21);
        })
        .await;
    });
    one.await.unwrap();
    two.await.unwrap();
}

#[tokio::test]
async fn resolved_supports_real_config_facade_and_its_unsized_repository_contract() {
    use openrust::support::facades::Config;
    let fixture = fixture().await;
    App::set_facade_application(Some(fixture.app()), async {
        Config::set("facade_probe", serde_json::json!("ready")).unwrap();
        let application_callbacks = Arc::new(AtomicUsize::new(0));
        let registered = application_callbacks.clone();
        App::resolved(move |application, container| {
            assert!(std::ptr::eq(application.container(), container));
            registered.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert_eq!(application_callbacks.load(Ordering::SeqCst), 1);
        assert!(App::is_resolved("app").unwrap());
        assert!(Arc::ptr_eq(
            &App::get_facade_application().unwrap().unwrap(),
            &App::get_instance().unwrap()
        ));
        let count = Arc::new(AtomicUsize::new(0));
        let recorded = count.clone();
        Config::resolved(move |repository, container| {
            assert!(container.has("config"));
            assert_eq!(
                repository.get("facade_probe"),
                Some(serde_json::json!("ready"))
            );
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(
            Config::get("facade_probe").unwrap(),
            Some(serde_json::json!("ready"))
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
    })
    .await;
}

#[tokio::test]
async fn resolved_observes_asynchronous_factory_and_does_not_cache_a_failed_callback() {
    let fixture = fixture().await;
    fixture
        .app()
        .container()
        .bind_async_named::<Item, _>("facade.item", |_| {
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(Item(42)))
            })
        })
        .await
        .unwrap();
    App::set_facade_application(Some(fixture.app()), async {
        let calls = Arc::new(AtomicUsize::new(0));
        let recorded = calls.clone();
        ItemFacade::resolved(move |item, _| {
            assert_eq!(item.0, 42);
            if recorded.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(BindingResolutionError::FacadeNotSet)
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert!(ItemFacade::get_facade_root_async().await.is_err());
        let ready = ItemFacade::get_facade_root_async().await.unwrap();
        assert_eq!(ready.0, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(Arc::ptr_eq(
            &ready,
            &ItemFacade::get_facade_root_async().await.unwrap()
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    })
    .await;
}

#[tokio::test]
async fn is_fake_inspects_cached_roots_and_distinguishes_marker_fakes_from_stubs() {
    use openrust::support::facades::{Cache, Event, Http, Mail, Notification, Process};
    assert!(!ItemFacade::is_fake().unwrap());
    let current = fixture().await;
    App::set_facade_application(Some(current.app()), async {
        assert!(!ItemFacade::is_fake().unwrap());
        assert_eq!(
            ItemFacade::get_facade_root().unwrap().0,
            1,
            "is_fake 不应提前解析服务"
        );
        assert!(!ItemFacade::is_fake().unwrap());
        assert!(!Mail::is_fake().unwrap());
        Mail::fake().unwrap();
        assert!(Mail::is_fake().unwrap());
        Mail::clear_resolved_instance(None).unwrap();
        assert!(!Mail::is_fake().unwrap());
        Mail::get_facade_root().unwrap();
        assert!(Mail::is_fake().unwrap());
        assert!(!Notification::is_fake().unwrap());
        Notification::fake().unwrap();
        assert!(Notification::is_fake().unwrap());
        assert!(!Event::is_fake().unwrap());
        Event::fake_for(
            async {
                assert!(Event::is_fake().unwrap());
                Ok(())
            },
            &[],
        )
        .await
        .unwrap();
        assert!(!Event::is_fake().unwrap());
        Event::fake(&[]).unwrap();
        assert!(Event::is_fake().unwrap());
        Http::fake(|_| None).unwrap();
        assert!(!Http::is_fake().unwrap());
        Process::fake(|_| None).unwrap();
        assert!(!Process::is_fake().unwrap());
        assert!(!Cache::is_fake().unwrap());
    })
    .await;
    assert!(!Mail::is_fake().unwrap());
    let isolated = fixture().await;
    App::set_facade_application(Some(isolated.app()), async {
        assert!(!Mail::is_fake().unwrap());
        assert!(!Event::is_fake().unwrap());
    })
    .await;
}

#[tokio::test]
async fn constructed_application_is_resolved_without_a_strong_container_cycle() {
    let fixture = fixture().await;
    assert!(fixture.app().container().resolved("app").unwrap());
    let owner = App::set_facade_application(Some(fixture.app()), async {
        Arc::downgrade(&App::get_container().unwrap())
    })
    .await;
    assert!(owner.upgrade().is_some());
    drop(fixture);
    assert!(owner.upgrade().is_none());
}
