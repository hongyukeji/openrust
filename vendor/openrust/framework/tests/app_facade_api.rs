use openrust::{
    container::{EnvironmentQuery, EnvironmentResult, ServiceKey},
    contracts::http::Kernel as _,
    foundation::{configuration::Routing, http::Kernel},
    http::{Body, HttpError, Request, StatusCode},
    support::facades::{App, Route},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

async fn inspect() -> Result<Value, HttpError> {
    assert!(App::is_booted()?);
    assert!(App::running_unit_tests()?);
    assert!(!App::is_local()?);
    assert!(!App::is_production()?);
    assert_eq!(App::environment(None)?, EnvironmentResult::from("testing"));
    assert_eq!(
        App::environment(Some(EnvironmentQuery::Name("test*")))?,
        EnvironmentResult::from(true)
    );
    assert!(!App::configuration_is_cached()?);
    assert!(!App::routes_are_cached()?);

    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    App::before_resolving(Some(ServiceKey::typed::<u32>()), move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })?;
    App::singleton::<u32, _>(|_| Ok(Arc::new(7)))?;
    App::singleton_if::<u32, _>(|_| panic!("existing binding must be retained"))?;
    App::alias(ServiceKey::typed::<u32>(), "number")?;
    assert!(
        App::bound::<u32>()?
            && App::has("number")?
            && App::is_alias("number")?
            && App::is_shared(ServiceKey::typed::<u32>())?
    );
    // 上游 isShared 不展开别名；resolved/getAlias 则会展开。
    assert!(!App::is_shared("number")?);
    assert!(!App::resolved("number")?);
    assert_eq!(App::get_alias("number")?, ServiceKey::typed::<u32>());
    assert_eq!(*App::make::<u32>().await?, 7);
    assert_eq!(*App::get::<u32>("number").await?, 7);
    assert!(App::resolved("number")?);
    assert!(calls.load(Ordering::SeqCst) > 0);
    App::extend::<u32, _>("number", |value, _| Ok(Arc::new(*value + 1)))?;
    assert_eq!(*App::make::<u32>().await?, 8);
    App::forget_instance(ServiceKey::typed::<u32>())?;
    // 扩展已缓存实例只更新该实例，不注册后续构造装饰器。
    assert_eq!(*App::make::<u32>().await?, 7);
    App::forget_instance(ServiceKey::typed::<u32>())?;
    App::extend::<u32, _>("number", |value, _| Ok(Arc::new(*value + 2)))?;
    assert_eq!(*App::make::<u32>().await?, 9);
    App::forget_extenders("number")?;
    App::forget_instance(ServiceKey::typed::<u32>())?;
    assert_eq!(*App::make::<u32>().await?, 7);
    App::instance_named("message", Arc::<str>::from("hello"))?;
    assert_eq!(App::get::<str>("message").await?.as_ref(), "hello");
    App::bind_named("transient", |_| Ok(Arc::new(42u16)))?;
    App::bind_if_named::<u16, _>("transient", |_| panic!("existing binding"))?;
    assert_eq!(*App::get::<u16>("transient").await?, 42);
    assert!(!App::is_shared("transient")?);
    App::tag(["message"], ["labels"])?;
    let tagged = App::tagged::<str, _>("labels", |services| {
        services
            .into_iter()
            .map(|service| service.map(|value| value.to_string()))
            .collect::<Result<Vec<_>, _>>()
    })??;
    assert_eq!(tagged, ["hello"]);
    let factory = App::factory::<u32>()?;
    assert_eq!(*factory().await?, 7);
    let factory = App::factory_named::<str>(ServiceKey::named("message"))?;
    assert_eq!(factory().await?.as_ref(), "hello");
    let borrowed = "borrowed value".to_owned();
    let callback = openrust::container::Callback::new(&[], |_| async { Ok(borrowed.as_str()) });
    assert_eq!(
        App::call_callback(callback, Default::default()).await?,
        "borrowed value"
    );
    assert_eq!(
        App::call(
            openrust::container::Callback::new(&[], |_| async { Ok(11u32) }),
            Default::default()
        )
        .await?,
        11
    );
    App::bind_method::<u32, u32, _>("double", |value, _| Box::pin(async move { Ok(*value * 2) }))?;
    assert!(App::has_method_binding::<u32>("double")?);
    assert_eq!(
        App::call_method_binding::<u32, u32>("double", Arc::new(6)).await?,
        12
    );
    assert!(App::currently_resolving()?.is_none());
    Ok(
        json!({"base":App::base_path("")?,"public":App::public_path("images")?,"storage":App::storage_path("logs")?,"booted":App::is_booted()?}),
    )
}

async fn scoped() -> Result<Value, HttpError> {
    App::scoped_if::<AtomicUsize, _>(|_| Ok(Arc::new(AtomicUsize::new(0))))?;
    let first = App::make::<AtomicUsize>().await?;
    let second = App::make::<AtomicUsize>().await?;
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.fetch_add(1, Ordering::SeqCst), 0);
    App::forget_scoped_instances()?;
    let third = App::make::<AtomicUsize>().await?;
    assert!(!Arc::ptr_eq(&first, &third));
    assert_eq!(third.load(Ordering::SeqCst), 0);
    Ok(json!({"scope":"fresh"}))
}

async fn build_kernel() -> (tempfile::TempDir, Kernel) {
    let root = tempfile::tempdir().unwrap();
    let app = App::configure(root.path().to_path_buf())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"env":"testing","locale":"en","supported_locales":[]}),
            )]))
        })
        .with_routing(Routing::default().api(|| {
            Route::get("/inspect", inspect)?;
            Route::get("/scoped", scoped)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    (root, kernel)
}

async fn call(kernel: &Kernel, path: &str) -> Value {
    let mut request = Request::from(
        http::Request::builder()
            .uri(path)
            .header("host", "example.test")
            .body(Body::empty())
            .unwrap(),
    );
    let response = kernel.handle(&mut request).await;
    let status = response.status();
    let body = axum::body::to_bytes(response.into_parts().1, 65536)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn app_facade_resolves_and_modifies_the_real_application_container() {
    let (root, kernel) = build_kernel().await;
    let value = call(&kernel, "/api/inspect").await;
    assert_eq!(value["base"], json!(root.path()));
    assert_eq!(value["public"], json!(root.path().join("public/images")));
    assert_eq!(value["storage"], json!(root.path().join("storage/logs")));
    assert!(App::get::<str>("message").await.is_err());
    let (_other_root, other) = build_kernel().await;
    assert_eq!(call(&other, "/api/inspect").await["booted"], json!(true));
}

#[tokio::test]
async fn app_facade_scoped_bindings_are_isolated_and_can_be_forgotten() {
    let (_root, kernel) = build_kernel().await;
    for _ in 0..2 {
        assert_eq!(call(&kernel, "/api/scoped").await, json!({"scope":"fresh"}));
    }
}
