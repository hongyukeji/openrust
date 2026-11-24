use openrust::{
    console::{CommandDefinition, CommandInput, ConsoleError, Input, Output},
    foundation::{Application, configuration::Routing},
    support::{
        BackedEnum, IntervalError,
        facades::{App, Artisan, Facade, Route},
    },
};
use serde_json::{Value, json};
use std::{error::Error as _, path::Path, sync::Arc};
#[test]
fn typed_scalar_values_match_fixed_command_input_vectors() {
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/input-oracle.json"
    ))
    .unwrap();
    let bag = CommandInput::new(
        oracle["arguments"].as_object().unwrap().clone(),
        oracle["options"].as_object().unwrap().clone(),
    );
    assert_eq!(bag.get("collision").unwrap(), oracle["collision"]);
    for case in oracle["cases"].as_array().unwrap() {
        let key = case["key"].as_str().unwrap();
        assert_eq!(bag.has(key), case["has"].as_bool().unwrap(), "{key}:has");
        assert_eq!(
            bag.filled(key),
            case["filled"].as_bool().unwrap(),
            "{key}:filled"
        );
        assert_eq!(
            bag.string_or(key, "fallback").to_string(),
            case["string"],
            "{key}:string"
        );
        assert_eq!(
            bag.string_strict(key, "fallback").to_string(),
            case["string_strict"],
            "{key}:strict string"
        );
        assert_eq!(
            bag.integer_or(key, 7),
            case["integer"].as_i64().unwrap(),
            "{key}:integer"
        );
        assert_eq!(
            bag.integer_strict(key, 7),
            case["integer_strict"].as_i64().unwrap(),
            "{key}:strict integer"
        );
        assert_eq!(
            bag.float_or(key, 7.0),
            case["float"].as_f64().unwrap(),
            "{key}:float"
        );
        assert_eq!(
            bag.float_strict(key, 7.0),
            case["float_strict"].as_f64().unwrap(),
            "{key}:strict float"
        );
        assert_eq!(
            bag.boolean_or(key, true),
            case["boolean"].as_bool().unwrap(),
            "{key}:boolean"
        );
        assert_eq!(
            bag.boolean_strict(key, true),
            case["boolean_strict"].as_bool().unwrap(),
            "{key}:strict boolean"
        );
        assert_eq!(
            bag.clamp(key, 1.0, 10.0, 3.0),
            case["clamp"].as_f64().unwrap(),
            "{key}:clamp"
        );
    }
}
#[derive(Debug, PartialEq)]
enum State {
    Ready,
    Done,
}
impl BackedEnum for State {
    type Backing = String;
    fn try_from_backing(value: String) -> Option<Self> {
        match value.as_str() {
            "ready" => Some(Self::Ready),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}
#[test]
fn bag_paths_groups_conditions_and_collections_use_existing_shared_data_rules() {
    let bag=CommandInput::new(json!({"name":"Ada","empty":" ","null":null,"false":false,"zero":"0","user":{"name":"Grace"},"states":["ready","invalid","done"]}).as_object().unwrap().clone(),json!({"name":"overridden","flag":true}).as_object().unwrap().clone());
    assert_eq!(
        bag.all_keys(&["user.name", "absent"]).unwrap(),
        json!({"user":{"name":"Grace"},"absent":null})
    );
    assert_eq!(bag.all_keys(&[]).unwrap(), bag.to_array());
    assert_eq!(bag.arguments()["name"], "Ada");
    assert_eq!(bag.options()["name"], "overridden");
    assert_eq!(bag.all()["name"], "Ada");
    assert_eq!(bag.to_array(), *bag.all());
    assert!(
        bag.exists("null")
            && bag.has_all(&["null", "user.name"])
            && bag.has_any(&["absent", "zero"])
            && bag.missing("absent")
            && bag.missing_any(&["null", "absent"])
    );
    assert!(
        bag.filled("false")
            && bag.filled("zero")
            && bag.filled_all(&["name", "zero"])
            && bag.any_filled(&["null", "false"])
            && bag.is_not_filled_all(&["empty", "null"])
    );
    assert_eq!(bag.when_has("null", |v| v), Some(Value::Null));
    assert_eq!(bag.when_filled("false", |v| v), Some(json!(false)));
    assert_eq!(bag.when_missing("absent", |v| v), Some(Value::Null));
    assert_eq!(
        bag.when_has("absent", |_| panic!("must not execute")),
        None::<()>
    );
    assert_eq!(bag.string("name").to_string(), "Ada");
    assert_eq!(bag.str("name").to_string(), "Ada");
    assert_eq!(bag.str_or("absent", "fallback").to_string(), "fallback");
    assert_eq!(bag.str_strict("false", "fallback").to_string(), "fallback");
    assert!(!bag.boolean("absent"));
    assert_eq!(bag.integer("absent"), 0);
    assert_eq!(bag.float("absent"), 0.0);
    assert_eq!(
        bag.only(&["user.name", "null", "absent"]).unwrap(),
        json!({"user":{"name":"Grace"},"null":null})
    );
    assert_eq!(
        bag.except(&["user.name", "absent"]).unwrap()["user"],
        json!({})
    );
    assert_eq!(bag.get("user.name"), Some(json!("Grace")));
    assert_eq!(bag.array(Some("name")), json!(["Ada"]));
    assert_eq!(bag.array(Some("null")), json!([]));
    assert_eq!(bag.array(None), bag.to_array());
    assert_eq!(
        bag.collect(Some("user")).to_array().unwrap(),
        json!({"name":"Grace"})
    );
    assert_eq!(
        bag.r#enum::<State>("name", Some(State::Done)).unwrap(),
        Some(State::Done)
    );
    assert_eq!(bag.r#enum::<State>("absent", None).unwrap(), None);
    assert_eq!(
        bag.enums::<State>("states")
            .unwrap()
            .iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        vec![&State::Ready, &State::Done]
    );
    assert_eq!(
        bag.when_enum::<State, _>("name", |_| panic!("invalid case"))
            .unwrap(),
        None::<()>
    );
    let input = CommandDefinition::from_signature("read {user=7} {--flag}")
        .unwrap()
        .parse(&[])
        .unwrap();
    assert_eq!(input.input().integer("user"), 7);
    assert_eq!(
        input.input_value("absent", json!("default")),
        json!("default")
    );
}
fn config(
    _: &Path,
    _: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(std::collections::BTreeMap::from([
        (
            "app".into(),
            json!({"env":"testing","timezone":"Asia/Shanghai"}),
        ),
        (
            "logging".into(),
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        ),
    ]))
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with("tutorial:typed {user} {--from=} {--count=12tail} {--kind=ready} {--id=*} {--active}",|input:&Input, output:&mut Output| {
        let bag=input.input();
        let date=bag.date("from",Some("!Y-m-d"),None)?.map(|date|date.format("Y-m-d P")).transpose().map_err(ConsoleError::from_source)?;
        let state=bag.r#enum::<State>("kind",None)?;
        let ids=bag.collect(Some("id")).to_array().map_err(ConsoleError::from_source)?;
        output.stdout.push_str(&json!({"user":bag.string("user").to_string(),"count":bag.integer("count"),"state":format!("{state:?}"),"date":date,"ids":ids,"active":bag.boolean("active")}).to_string());Ok(0)
    })?.purpose("Read typed input")
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/typed", || async {
        let mut output = Output::default();
        assert_eq!(
            Artisan::call(
                "tutorial:typed",
                &["Ada", "--from=2026-11-25", "--id=3", "--id=7", "--active"].map(Into::into),
                &mut output
            )
            .await
            .unwrap(),
            0
        );
        output.stdout
    })
}
#[tokio::test]
async fn real_http_and_independent_artisan_use_dates_enums_arrays_and_scalar_casts() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    fn app(root: &Path) -> Application {
        Application::configure(root.into())
            .with_config(config)
            .with_routing(Routing::default().commands(commands).web(routes))
            .create()
            .unwrap()
    }
    if let Ok(root) = std::env::var("TYPED_COMMAND_ROOT") {
        assert_eq!(
            app(Path::new(&root))
                .handle_command(
                    &[
                        "tutorial:typed",
                        "Ada",
                        "--from=2026-11-25",
                        "--id=3",
                        "--id=7",
                        "--active"
                    ]
                    .map(Into::into)
                )
                .await
                .unwrap(),
            0
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(app(root.path()));
    kernel.bootstrap().await.unwrap();
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    let response = reqwest::get(format!("http://{address}/typed"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    let result: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        result,
        json!({"user":"Ada","count":12,"state":"Some(Ready)","date":"2026-11-25 +08:00","ids":["3","7"],"active":true})
    );
    server.abort();
    let _ = server.await;
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "real_http_and_independent_artisan_use_dates_enums_arrays_and_scalar_casts",
            "--nocapture",
        ])
        .env("TYPED_COMMAND_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8(child.stdout).unwrap().contains(&text));
}
#[tokio::test]
async fn calendar_interval_enum_errors_retain_sources_and_do_not_poison_following_commands() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_config(config)
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app),async {
        let bag=CommandInput::new(json!({"date":"2026-11-25","bad":"not-a-date","interval":"P1DT2H","invalid_interval":"Pbad","states":{},"ready":"ready"}).as_object().unwrap().clone(),Default::default());
        assert_eq!(bag.date("date",None,None).unwrap().unwrap().format("Y-m-d P").unwrap(),"2026-11-25 +08:00");assert!(bag.date("absent",None,None).unwrap().is_none());
        let error=bag.date("bad",None,Some("UTC")).unwrap_err();assert!(error.source().unwrap().is::<openrust::support::DateError>());
        assert!(bag.interval("interval",None).unwrap().is_some());let error=bag.interval("invalid_interval",None).unwrap_err();assert!(error.source().unwrap().is::<IntervalError>());
        let error=bag.r#enum::<State>("states",None).unwrap_err();assert!(error.source().is_some());assert_eq!(bag.when_enum::<State,_>("ready",|v|v==State::Ready).unwrap(),Some(true));
        let mut output=Output::default();assert!(Artisan::call("tutorial:typed",&["Ada".into(),"--from=not-a-date".into()],&mut output).await.is_err());assert_eq!(Artisan::call("tutorial:typed",&["Ada".into(),"--from=2026-11-25".into()],&mut output).await.unwrap(),0);
    }).await;
}
