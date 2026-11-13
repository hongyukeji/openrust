use std::{collections::BTreeMap, path::Path};

use openrust::foundation::Application;
use serde_json::{Map, json};

async fn file_application(root: &Path) -> Application {
    let mut app = Application::configure(root.into()).create().unwrap();
    app.boot().await.unwrap();
    app
}

#[tokio::test]
async fn file_maintenance_is_atomic_shared_by_root_and_reports_invalid_payload() {
    let root = tempfile::tempdir().unwrap();
    let unrelated = tempfile::tempdir().unwrap();
    let first = file_application(root.path()).await;
    let second = file_application(root.path()).await;
    let unrelated_app = file_application(unrelated.path()).await;
    let first_mode = first.maintenanceMode().unwrap();
    let second_mode = second.maintenanceMode().unwrap();

    let initial = Map::from_iter([("status".into(), json!(503))]);
    first_mode.activate(&initial).await.unwrap();
    assert!(second.isDownForMaintenance().await.unwrap());
    assert_eq!(second_mode.data().await.unwrap(), Some(initial));
    assert!(!unrelated_app.isDownForMaintenance().await.unwrap());

    let next = Map::from_iter([("status".into(), json!(503)), ("retry".into(), json!(20))]);
    let alternate = Map::from_iter([("status".into(), json!(502)), ("retry".into(), json!(30))]);
    let (one, two) = tokio::join!(first_mode.activate(&next), second_mode.activate(&alternate));
    one.unwrap();
    two.unwrap();
    let current = first_mode.data().await.unwrap().unwrap();
    assert!(current == next || current == alternate);

    std::fs::write(root.path().join("storage/framework/down"), b"{").unwrap();
    assert!(second.isDownForMaintenance().await.unwrap());
    assert!(second_mode.data().await.is_err());
    first_mode.deactivate().await.unwrap();
    assert!(!second.isDownForMaintenance().await.unwrap());
}

#[tokio::test]
async fn array_maintenance_is_application_local_even_with_the_same_root() {
    let root = tempfile::tempdir().unwrap();
    let app = || {
        Application::configure(root.path().into())
            .withConfig(|_, _| {
                Ok(BTreeMap::from([(
                    "app".into(),
                    json!({"maintenance":{"driver":"array"}}),
                )]))
            })
            .create()
            .unwrap()
    };
    let mut first = app();
    let mut second = app();
    first.boot().await.unwrap();
    second.boot().await.unwrap();
    first
        .maintenanceMode()
        .unwrap()
        .activate(&Map::new())
        .await
        .unwrap();
    assert!(first.isDownForMaintenance().await.unwrap());
    assert!(!second.isDownForMaintenance().await.unwrap());
    assert!(!root.path().join("storage/framework/down").exists());
}

#[tokio::test]
async fn artisan_dispatches_maintenance_events_and_http_exclusions_are_application_local() {
    use openrust::{foundation::configuration::Routing, testing::TestApp};

    let mut excluded = TestApp::create(|builder| {
        builder
            .withRouting(Routing::default().health("/up"))
            .withMiddleware(|middleware| {
                middleware.preventRequestsDuringMaintenance(["status/*"]);
            })
    })
    .unwrap();
    let mut normal =
        TestApp::create(|builder| builder.withRouting(Routing::default().health("/ready")))
            .unwrap();
    let events = excluded.fakeEvents(&[]).await.unwrap();
    let enabled = "foundation.maintenance_mode_enabled";
    let disabled = "foundation.maintenance_mode_disabled";

    assert_eq!(excluded.artisan(["up"]).await.status, 0);
    assert!(events.dispatched(disabled).is_empty());
    assert_eq!(excluded.artisan(["down"]).await.status, 0);
    assert_eq!(excluded.artisan(["down"]).await.status, 0);
    assert_eq!(events.dispatched(enabled).len(), 2);

    let payload = excluded
        .app()
        .maintenanceMode()
        .unwrap()
        .data()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(payload["except"], json!(["status/*", "up"]));
    assert_eq!(normal.artisan(["down"]).await.status, 0);
    assert_eq!(excluded.get("/up").await.unwrap().status(), 200);
    assert_eq!(excluded.get("/status/missing").await.unwrap().status(), 404);
    assert_eq!(excluded.get("/other").await.unwrap().status(), 503);
    assert_eq!(normal.get("/up").await.unwrap().status(), 503);
    assert_eq!(normal.get("/ready").await.unwrap().status(), 200);
    assert_eq!(normal.get("/status/missing").await.unwrap().status(), 503);
    assert_eq!(events.dispatched(enabled).len(), 2);

    assert_eq!(excluded.artisan(["up"]).await.status, 0);
    assert_eq!(excluded.artisan(["up"]).await.status, 0);
    assert_eq!(events.dispatched(disabled).len(), 1);
    assert_eq!(excluded.get("/other").await.unwrap().status(), 404);
    assert_eq!(normal.get("/up").await.unwrap().status(), 503);
}
