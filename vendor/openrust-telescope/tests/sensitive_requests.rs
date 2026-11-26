use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Db, Facade, Route},
    testing::TestApp,
};
use openrust_telescope::{
    EntryType, IncomingEntry, TelescopeServiceProvider,
    storage::{DatabaseEntriesRepository, EntryQueryOptions},
};
use serde_json::json;

#[path = "../config/telescope.rs"]
mod telescope_config;

fn handler(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let telescope = openrust_telescope::facades::Telescope::get_facade_root().unwrap();
        telescope.record_event(
            IncomingEntry::make(
                json!({"name":"secret-event","payload":"PRIVATE-EVENT"})
                    .as_object()
                    .unwrap()
                    .clone(),
            )
            .unwrap(),
        );
        if openrust::http::middleware::ProtectSensitiveData::current_path().is_some() {
            telescope.record_update(openrust_telescope::EntryUpdate::make(
                uuid::Uuid::now_v7(),
                EntryType::Mail,
                json!({"html":"PRIVATE-UPDATE"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ));
        }
        Db::connection(None)
            .unwrap()
            .select("SELECT ? AS secret", &["PRIVATE-QUERY".into()])
            .await
            .unwrap();
        let mut response = Response::json(StatusCode::OK, json!({"token":"PRIVATE-RESPONSE"}))?;
        response
            .parts_mut()
            .headers
            .insert("set-cookie", "session=PRIVATE-COOKIE".parse().unwrap());
        Ok(response)
    })
}

#[tokio::test]
async fn sensitive_routes_keep_only_safe_request_metadata_and_ordinary_requests_still_record() {
    let mut app = TestApp::create(|builder| builder
        .with_config(|base, env| Ok(std::collections::BTreeMap::from([
            ("app".into(), json!({"env":"testing","locale":"en","key":"01234567890123456789012345678901"})),
            ("database".into(), json!({"default":"testing","connections":{"testing":{"driver":"sqlite","database":base.join("telescope.sqlite")}}})),
            ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
            ("telescope".into(), { let mut config=telescope_config::load(env)?; config["watchers"]["QueryWatcher"]["ignore_packages"]=false.into(); config }),
        ])))
        .with_providers(vec![Box::new(TelescopeServiceProvider)])
        .with_routing(Routing::default().api(|| {
            Route::group(openrust::routing::RouteGroup::new(), |router| {
                router.post("/reset/{token}", handler)?.middleware_name("sensitive");
                Ok(())
            })?;
            Route::get("/ordinary", handler)?;
            Ok(())
        }))
    ).unwrap();
    std::fs::File::create(app.root().join("telescope.sqlite")).unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.call(
        http::Request::builder()
            .method("POST")
            .uri("/api/reset/PRIVATE-PATH?code=PRIVATE-CODE")
            .header("content-type", "application/json")
            .header("authorization", "Bearer PRIVATE-TOKEN")
            .header("cookie", "session=PRIVATE-SESSION")
            .header("x-auth-confirmation", "PRIVATE-PROOF")
            .body(openrust::http::Body::from(
                json!({"code":"PRIVATE-BODY","password":"PRIVATE-PASSWORD"}).to_string(),
            ))
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    let body = &entries[0].content;
    assert_eq!(body["sensitive"], true);
    assert!(body["uri"].as_str().unwrap().ends_with("reset/{token}"));
    assert_eq!(body["response"], "[redacted]");
    assert!(!serde_json::to_string(body).unwrap().contains("PRIVATE"));
    for kind in [
        EntryType::Query,
        EntryType::Event,
        EntryType::Model,
        EntryType::Mail,
    ] {
        assert!(
            repository
                .get(Some(kind), EntryQueryOptions::default())
                .await
                .unwrap()
                .is_empty()
        );
    }
    app.get("/api/ordinary").await.unwrap().assert_ok();
    assert!(
        !repository
            .get(Some(EntryType::Query), EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    let entries = repository
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry.content["response"]["token"] == "PRIVATE-RESPONSE")
    );
}
