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
            .with_config(|base, _| {
                let name = base.file_name().unwrap().to_str().unwrap();
                Ok([("app".into(), json!({"name": name, "nullable": null}))].into())
            })
            .create()
            .unwrap()
    };
    let mut first = make("First");
    let mut second = make("Second");
    assert!(Config::string("app.name").is_err());
    assert_eq!(Config::string_or("app.name", "Fallback"), "Fallback");
    first
        .call(
            Callback::new(&[], |_| async {
                assert!(Config::string("app.name").is_err());
                assert_eq!(Config::string_or("app.name", "Fallback"), "Fallback");
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
            first.container().instance_named::<dyn Repository>(
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
            let value = Config::get("app.name")?;
            Response::json(StatusCode::OK, value)
        })
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|_, _| Ok([("app".into(), json!({"name": "HTTP application"}))].into()))
            .with_routing(Routing::default().web(|| Route::get("/config", handler)))
    })
    .unwrap();
    app.get("/config")
        .await
        .unwrap()
        .assert_ok()
        .assert_content(r#""HTTP application""#);
}

#[tokio::test]
async fn config_defaults_and_serde_distinguish_missing_null_and_invalid_values() {
    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Mail {
        host: String,
        port: u16,
    }

    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_config(|_, _| {
            Ok([
                ("mail".into(), json!({"host":"localhost","port":2525})),
                (
                    "mixed".into(),
                    json!({"nullable":null,"number":7,"list":["first"],"empty":""}),
                ),
            ]
            .into())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.call(
        Callback::new(&[], |_| async {
            assert_eq!(Config::get_or("missing", json!(42))?, json!(42));
            assert_eq!(Config::get_or("mixed.nullable", json!(42))?, Value::Null);
            let values = Config::get_many([
                ("mixed.list.0", json!("default")),
                ("mixed.nullable", json!(false)),
                ("missing", json!("fallback")),
            ])?;
            assert_eq!(
                values.into_iter().collect::<Vec<_>>(),
                vec![
                    ("mixed.list.0".into(), json!("first")),
                    ("mixed.nullable".into(), Value::Null),
                    ("missing".into(), json!("fallback")),
                ]
            );
            assert_eq!(Config::string_or("mixed.list.0", "demo"), "first");
            assert_eq!(Config::string_or("mixed.empty", "demo"), "");
            for key in ["missing", "mixed.nullable", "mixed.number"] {
                assert_eq!(Config::string_or(key, "demo"), "demo");
                assert!(Config::string(key).is_err());
            }
            assert_eq!(
                Config::deserialize::<Mail>("mail").unwrap(),
                Mail {
                    host: "localhost".into(),
                    port: 2525
                }
            );
            assert!(Config::deserialize::<Mail>("mixed").is_err());
            assert_eq!(Config::deserialize_or::<u16>("missing", 5).unwrap(), 5);
            assert!(Config::deserialize_or::<u16>("mixed.nullable", 5).is_err());
            assert!(Config::deserialize_or::<u16>("mail.host", 5).is_err());
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
}
