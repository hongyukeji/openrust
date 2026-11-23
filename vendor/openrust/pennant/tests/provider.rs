use openrust::contracts::foundation::Application as _;
use openrust::{
    foundation::{Application, configuration::Routing},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::{ProviderResult, ServiceProvider, facades::Route},
    testing::TestApp,
};
use openrust_pennant::{
    Feature, FeatureManager, PennantServiceProvider, middleware::EnsureFeaturesAreActive,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Definitions(Arc<AtomicUsize>);
impl ServiceProvider for Definitions {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<openrust::auth::AuthManager>()
                .await?
                .via_request("pennant-test", |_| Box::pin(async { Ok(None) }));
            let manager = app.make::<FeatureManager>().await?;
            let store = manager.store(None)?;
            let count = self.0.clone();
            store.define(
                "counter",
                Arc::new(move |_| {
                    let value = count.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async move { Ok(json!(value)) })
                }),
            )?;
            store.defineValue("off", json!(false))?;
            store.defineValue("zero", json!(0))?;
            Ok(())
        })
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/counter", counter)?;
    for (path, middleware) in [
        ("/blocked", EnsureFeaturesAreActive::using(["off"])),
        ("/allowed", EnsureFeaturesAreActive::using(["zero"])),
        (
            "/custom",
            EnsureFeaturesAreActive::using(["off"]).whenInactive(|_, _| {
                Box::pin(async { Ok(Response::empty(StatusCode::NOT_FOUND)) })
            }),
        ),
    ] {
        Route::group(
            openrust::routing::RouteGroup::new().middleware(Arc::new(middleware)),
            |router| {
                router.get(path, counter)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}
fn counter(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let one = Feature::value("counter").await?;
        let two = Feature::value("counter").await?;
        Ok(Response::new(
            StatusCode::OK,
            Body::from(json!([one, two]).to_string()),
        ))
    })
}
fn application() -> TestApp {
    TestApp::create(|builder| builder.with_config(|_, _| Ok(std::collections::BTreeMap::from([
        ("app".into(), json!({"debug":false})), ("auth".into(), json!({"defaults":{"guard":"test"},"guards":{"test":{"driver":"pennant-test"}}})), ("pennant".into(), json!({"default":"array"}))
    ]))).with_providers(vec![Box::new(PennantServiceProvider), Box::new(Definitions(Arc::new(AtomicUsize::new(0))))])
        .with_routing(Routing::default().api(routes))).unwrap()
}

#[tokio::test]
async fn http_scopes_preserve_definitions_and_isolate_array_state_and_middleware() {
    let mut one = application();
    let mut two = application();
    assert_eq!(
        one.get("/api/counter").await.unwrap().assert_ok().body(),
        b"[0,0]"
    );
    assert_eq!(
        one.get("/api/counter").await.unwrap().assert_ok().body(),
        b"[1,1]"
    );
    assert_eq!(
        two.get("/api/counter").await.unwrap().assert_ok().body(),
        b"[0,0]"
    );
    one.get("/api/blocked")
        .await
        .unwrap()
        .assert_status(StatusCode::BAD_REQUEST);
    one.get("/api/custom")
        .await
        .unwrap()
        .assert_status(StatusCode::NOT_FOUND);
    one.get("/api/allowed").await.unwrap().assert_ok();
    std::fs::write(
        one.root().join("Cargo.toml"),
        "[package]\nname=\"pennant-fixture\"\nversion=\"0.0.0\"\n",
    )
    .unwrap();
    one.artisan(["pennant:feature", "NewApi"])
        .await
        .assert_successful();
    let feature = one.root().join("app/features/new_api.rs");
    assert!(
        std::fs::read_to_string(&feature)
            .unwrap()
            .contains("impl FeatureDefinition for NewApi")
    );
    std::fs::write(&feature, "// customized").unwrap();
    one.artisan(["pennant:feature", "NewApi"])
        .await
        .assert_exit_code(1);
    assert_eq!(std::fs::read_to_string(&feature).unwrap(), "// customized");
    one.artisan(["pennant:feature", "../Outside"])
        .await
        .assert_exit_code(1);
    one.artisan(["pennant:feature", "NewApi", "--force"])
        .await
        .assert_successful();
    one.artisan(["help", "pennant:purge"])
        .await
        .assert_successful();
    let cleared = one.artisan(["pennant:clear", "--except-registered"]).await;
    cleared.assert_successful();
    assert!(
        cleared
            .stdout
            .contains("No features to purge from storage.")
    );
    one.artisan(["pennant:purge", "--store=missing"])
        .await
        .assert_exit_code(1);
    one.artisan(["vendor:publish", "--tag=pennant-config"])
        .await
        .assert_successful();
    let path = one.root().join("config/pennant.rs");
    std::fs::write(&path, "// application override").unwrap();
    one.artisan(["vendor:publish", "--tag=pennant-config"])
        .await
        .assert_successful();
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "// application override"
    );
}

#[tokio::test]
async fn concurrent_native_container_scopes_do_not_clear_each_others_values() {
    let mut test = application();
    test.boot().await.unwrap();
    let app = test.app();
    let read = || async {
        let manager = app.make::<FeatureManager>().await.unwrap();
        let user = manager.r#for("same-user").unwrap();
        let first = user.value("counter").await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(first, user.value("counter").await.unwrap());
        first
    };
    let (one, two) = tokio::join!(app.container().scope(read()), app.container().scope(read()));
    assert_ne!(one, two);
}
