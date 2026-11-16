use std::sync::Arc;

use openrust::{
    config::Repository as ConfigRepository,
    container::Callback,
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    foundation::Application,
    support::facades::Config,
};
use serde_json::{Value, json};

#[tokio::test]
async fn config_reads_each_applications_canonical_repository() {
    let root = tempfile::tempdir().unwrap();
    let make = |name: &'static str| {
        let base = root.path().join(name);
        std::fs::create_dir(&base).unwrap();
        Application::configure(base)
            .withConfig(|base, _| {
                let name = base.file_name().unwrap().to_str().unwrap();
                Ok([("app".into(), json!({"name": name, "nullable": null}))].into())
            })
            .create()
            .unwrap()
    };
    let mut first = make("First");
    let mut second = make("Second");
    assert!(Config::get("app.name").is_err());
    first
        .call(
            Callback::new(&[], |_| async {
                assert!(Config::get("app.name").is_err());
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    first.boot().await.unwrap();
    second.boot().await.unwrap();

    let first_check = first.call(
        Callback::new(&[], |_| async {
            tokio::task::yield_now().await;
            assert_eq!(Config::get("app.name")?, Some(json!("First")));
            assert_eq!(Config::get("app.missing")?, None);
            assert_eq!(Config::get("app.nullable")?, Some(Value::Null));
            first.container().instanceNamed::<dyn Repository>(
                "config",
                Arc::new(ConfigRepository::new(
                    [("app".into(), json!({"name": "Replaced"}))].into(),
                )),
            )?;
            assert_eq!(Config::get("app.name")?, Some(json!("Replaced")));
            Ok(())
        }),
        Default::default(),
    );
    let second_check = second.call(
        Callback::new(&[], |_| async {
            tokio::task::yield_now().await;
            assert_eq!(Config::get("app.name")?, Some(json!("Second")));
            Ok(())
        }),
        Default::default(),
    );
    let (first_result, second_result) = tokio::join!(first_check, second_check);
    first_result.unwrap();
    second_result.unwrap();
    assert!(Config::get("app.name").is_err());
}

#[tokio::test]
async fn config_is_available_in_http_route_handlers() {
    use openrust::{
        foundation::configuration::Routing,
        http::{HttpError, HttpFuture, Request, Response, StatusCode},
        support::facades::Route,
        testing::TestApp,
    };

    fn handler(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let value = Config::get("app.name").map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
            Response::json(StatusCode::OK, value)
        })
    }

    let mut app = TestApp::create(|builder| {
        builder
            .withConfig(|_, _| Ok([("app".into(), json!({"name": "HTTP application"}))].into()))
            .withRouting(Routing::default().web(|| Route::get("/config", handler)))
    })
    .unwrap();
    app.get("/config")
        .await
        .unwrap()
        .assertOk()
        .assertContent(r#""HTTP application""#);
}
