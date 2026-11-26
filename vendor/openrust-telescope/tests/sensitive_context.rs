use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    log::context::SENSITIVE_DATA,
    support::facades::{Context, Db, Facade, Route},
    testing::TestApp,
};
use openrust_telescope::{
    EntryType, IncomingEntry, TelescopeServiceProvider,
    storage::{DatabaseEntriesRepository, EntryQueryOptions},
};
use serde_json::json;
#[path = "../config/telescope.rs"]
mod telescope_config;

fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
fn sensitive(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let context = Context::repository().map_err(internal)?;
        let result = Context::scope(
            async {
                openrust_telescope::facades::Telescope::get_facade_root()
                    .unwrap()
                    .record_event(
                        IncomingEntry::make(
                            json!({"name":"PRIVATE-EVENT"}).as_object().unwrap().clone(),
                        )
                        .unwrap(),
                    );
                Db::connection(None)?
                    .select("SELECT ? AS secret", &["PRIVATE-SQL".into()])
                    .await?;
                Err::<Response, HttpError>(HttpError::AuthorizationDenied("PRIVATE-ERROR".into()))
            },
            Default::default(),
            [(SENSITIVE_DATA.to_owned(), true.into())]
                .into_iter()
                .collect(),
        )
        .await
        .map_err(internal)?;
        assert!(context.get_hidden(SENSITIVE_DATA).is_none());
        result
    })
}
fn cancel(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let context = Context::repository().map_err(internal)?;
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(1),
            Context::scope(
                async {
                    openrust_telescope::facades::Telescope::get_facade_root()
                        .unwrap()
                        .record_event(
                            IncomingEntry::make(
                                json!({"name":"PRIVATE-CANCEL"})
                                    .as_object()
                                    .unwrap()
                                    .clone(),
                            )
                            .unwrap(),
                        );
                    std::future::pending::<()>().await;
                },
                Default::default(),
                [(SENSITIVE_DATA.to_owned(), true.into())]
                    .into_iter()
                    .collect(),
            ),
        )
        .await;
        assert!(result.is_err());
        assert!(context.get_hidden(SENSITIVE_DATA).is_none());
        Response::json(StatusCode::OK, json!({"secret":"PRIVATE-AFTER-CANCEL"}))
    })
}
fn ordinary(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        assert!(
            Context::get_hidden(SENSITIVE_DATA)
                .map_err(internal)?
                .is_none()
        );
        Db::connection(None)?
            .select("SELECT ? AS visible", &["VISIBLE-SQL".into()])
            .await?;
        Response::json(StatusCode::OK, json!({"value":"VISIBLE"}))
    })
}
#[tokio::test]
async fn context_protects_errors_and_cancellation_then_releases_the_next_execution() {
    let mut app = TestApp::create(|builder| builder.with_config(|base, env| {
        let mut telescope=telescope_config::load(env)?;
        telescope["watchers"]["QueryWatcher"]["ignore_packages"]=false.into();
        Ok(std::collections::BTreeMap::from([
            ("app".into(),json!({"env":"testing","key":"01234567890123456789012345678901"})),
            ("database".into(),json!({"default":"testing","connections":{"testing":{"driver":"sqlite","database":base.join("telescope.sqlite")}}})),
            ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
            ("telescope".into(),telescope),
        ]))
    }).with_providers(vec![Box::new(TelescopeServiceProvider)])
      .with_routing(Routing::default().api(|| { Route::get("/context/{token}",sensitive)?;Route::get("/cancel",cancel)?;Route::get("/ordinary",ordinary)?;
      Ok(()) }))).unwrap();
    std::fs::File::create(app.root().join("telescope.sqlite")).unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/context/PRIVATE-PATH?secret=PRIVATE-QUERY")
        .await
        .unwrap()
        .assert_status(StatusCode::FORBIDDEN);
    app.get("/api/cancel").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let requests = repository
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert_eq!(request.content["uri"], "[sensitive operation]");
        assert_eq!(request.content["sensitive"], true);
        assert!(
            !serde_json::to_string(&request.content)
                .unwrap()
                .contains("PRIVATE")
        );
    }
    for kind in [EntryType::Query, EntryType::Event] {
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
    let requests = repository
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request.content["response"]["value"] == "VISIBLE")
    );
}
