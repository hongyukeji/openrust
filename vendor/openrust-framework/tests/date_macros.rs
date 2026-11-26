use openrust::{
    support::{
        Date as DateValue, DateFactory, DateGenericMacro, DateMacro, DateMacroError,
        facades::{App, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
fn stamp() -> DateMacro {
    DateMacro::new(|factory, prefix: String| {
        Ok(format!("{prefix}{}", factory.now(None)?.format("Y-m-d")?))
    })
}
#[tokio::test]
async fn original_named_generic_priority_mixin_removal_reset_and_default_date_conversion_are_preserved()
 {
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/date/macros-contract.json")).unwrap();
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"UTC"}))].into()))
    })
    .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Date::set_test_now(Some(Date::parse("2024-01-02 03:04:05", None).unwrap())).unwrap();
        let factory = Date::get_facade_root().unwrap();
        Date::r#macro("stamp", stamp()).unwrap();
        assert_eq!(
            Date::macro_call::<String, _>("stamp", "date:".to_owned()).unwrap(),
            expected["named"]
        );
        assert_eq!(Date::has_macro("stamp").unwrap(), expected["has"]);
        let raw = Date::get_macro("stamp").unwrap().unwrap();
        assert!(Date::get_macro("absent").unwrap().is_none());
        assert_eq!(
            raw.call::<String, _>(&factory, "raw:".to_owned()).unwrap(),
            "raw:2024-01-02"
        );
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let low = log.clone();
        Date::generic_macro(
            DateGenericMacro::new(move |factory, name, args: &String| {
                low.lock().unwrap().push("low".into());
                if name == "generic" {
                    assert_eq!(args, "arg");
                    Ok(factory.tomorrow(None)?)
                } else {
                    Err(DateMacroError::UnknownMethod(name.into()))
                }
            }),
            -10,
        )
        .unwrap();
        let high = log.clone();
        Date::generic_macro(
            DateGenericMacro::new::<String, DateValue>(move |_, name, _| {
                high.lock().unwrap().push("high-first".into());
                Err(DateMacroError::UnknownMethod(name.into()))
            }),
            10,
        )
        .unwrap();
        let high = log.clone();
        Date::generic_macro(
            DateGenericMacro::new(move |factory, name, _: &String| {
                high.lock().unwrap().push("high-second".into());
                if name == "generic" {
                    Ok(factory.tomorrow(None)?)
                } else {
                    Err(DateMacroError::UnknownMethod(name.into()))
                }
            }),
            10,
        )
        .unwrap();
        // PHP同一闭包的日期/标量分支，Rust以两个具类型回调表达相同分派结果。
        Date::generic_macro(
            DateGenericMacro::new(|_, name, _: &String| {
                if name == "genericScalar" {
                    Ok("scalar".to_owned())
                } else {
                    Err(DateMacroError::UnknownMethod(name.into()))
                }
            }),
            10,
        )
        .unwrap();
        let generic = Date::macro_call::<DateValue, _>("generic", "arg".to_owned()).unwrap();
        assert_eq!(generic.format("Y-m-d").unwrap(), expected["generic"]);
        assert_eq!(json!(*log.lock().unwrap()), expected["generic_log"]);
        log.lock().unwrap().clear();
        assert!(matches!(
            Date::macro_call::<String, _>("genericScalar", "arg".to_owned()),
            Err(DateMacroError::InvalidGeneratedDate)
        ));
        assert_eq!(json!(*log.lock().unwrap()), expected["scalar_log"]);
        log.lock().unwrap().clear();
        assert_eq!(
            Date::macro_call::<String, _>("stamp", "named:".to_owned()).unwrap(),
            expected["named_again"]
        );
        assert_eq!(json!(*log.lock().unwrap()), expected["named_log"]);
        Date::mixin_using(|_| {
            Ok([
                (
                    "stamp",
                    DateMacro::new(|_, prefix: String| Ok(format!("replaced:{prefix}"))),
                ),
                ("additional", DateMacro::new(|_, (): ()| Ok(42_u32))),
            ])
        })
        .unwrap();
        assert_eq!(
            Date::macro_call::<String, _>("stamp", "x".to_owned()).unwrap(),
            expected["mixin"]
        );
        assert_eq!(Date::macro_call::<u32, _>("additional", ()).unwrap(), 42);
        Date::r#macro("stamp", None).unwrap();
        assert!(!Date::has_macro("stamp").unwrap());
        assert!(Date::get_macro("stamp").unwrap().is_none());
        Date::use_strict_mode(true).unwrap();
        assert!(matches!(
            Date::macro_call::<(), _>("absent", "arg".to_owned()),
            Err(DateMacroError::UnknownMethod(_))
        ));
        Date::use_strict_mode(false).unwrap();
        assert!(matches!(
            Date::macro_call::<(), _>("absent", "arg".to_owned()),
            Err(DateMacroError::InvalidGeneratedDate)
        ));
        Date::use_strict_mode(true).unwrap();
        Date::reset_macros().unwrap();
        assert!(!Date::has_macro("additional").unwrap());
        assert!(matches!(
            Date::macro_call::<(), _>("generic", "arg".to_owned()),
            Err(DateMacroError::UnknownMethod(_))
        ));
        // 原始闭包句柄在注册表删除后仍可调用。
        assert_eq!(
            raw.call::<String, _>(&factory, "retained:".to_owned())
                .unwrap(),
            "retained:2024-01-02"
        );
    })
    .await;
}
#[test]
fn macro_callbacks_are_reentrant_and_accept_non_send_arguments_and_results_without_copying_identity()
 {
    use std::{cell::Cell, rc::Rc};
    let factory = DateFactory::default();
    factory.r#macro(
        "identity",
        DateMacro::new(|factory, value: Rc<Cell<u32>>| {
            value.set(7);
            factory.reset_macros();
            factory.r#macro("inner", DateMacro::new(|_, (): ()| Ok(9_u32)));
            Ok(value)
        }),
    );
    let value = Rc::new(Cell::new(1));
    let actual = factory
        .macro_call::<Rc<Cell<u32>>, _>("identity", value.clone())
        .unwrap();
    assert!(Rc::ptr_eq(&value, &actual));
    assert_eq!(value.get(), 7);
    assert_eq!(factory.macro_call::<u32, _>("inner", ()).unwrap(), 9);
    assert!(matches!(
        factory.macro_call::<String, _>("inner", ()),
        Err(DateMacroError::TypeMismatch { .. })
    ));
    assert!(matches!(
        factory.macro_call::<u32, _>("inner", 1_u32),
        Err(DateMacroError::TypeMismatch { .. })
    ));
    factory.generic_macro(
        DateGenericMacro::new::<(), DateValue>(|_, _, _| {
            Err(DateMacroError::Callback(
                std::io::Error::other("failure").into(),
            ))
        }),
        1,
    );
    factory.generic_macro(
        DateGenericMacro::new(|factory, _, _: &()| Ok(factory.now(None)?)),
        0,
    );
    assert!(matches!(
        factory.macro_call::<DateValue, _>("not_named", ()),
        Err(DateMacroError::Callback(_))
    ));
}
fn macro_endpoint(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let value =
            Date::macro_call::<String, _>("stamp", "http:".to_owned()).map_err(|source| {
                openrust::http::HttpError::Internal {
                    source: Box::new(source),
                }
            })?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, value)
    })
}
async fn http_app() -> TestApp {
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"UTC"}))].into()))
            .with_routing(
                openrust::foundation::configuration::Routing::default()
                    .web(|| openrust::support::facades::Route::get("/macro", macro_endpoint)),
            )
    })
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn real_http_consumers_and_independent_applications_do_not_share_macro_registrations() {
    let mut first = http_app().await;
    let mut second = http_app().await;
    first.travel_to(DateFactory::default().parse("2024-01-02", None).unwrap());
    App::set_facade_application(Some(first.app()), async {
        Date::r#macro("stamp", stamp()).unwrap();
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        assert!(!Date::has_macro("stamp").unwrap());
        Date::r#macro(
            "stamp",
            DateMacro::new(|_, prefix: String| Ok(format!("second:{prefix}"))),
        )
        .unwrap();
    })
    .await;
    let (left, right) = tokio::join!(first.get("/macro"), second.get("/macro"));
    left.unwrap()
        .assert_ok()
        .assert_exact_json(&json!("http:2024-01-02"));
    right
        .unwrap()
        .assert_ok()
        .assert_exact_json(&json!("second:http:"));
    App::set_facade_application(Some(second.app()), async {
        Date::reset_macros().unwrap();
    })
    .await;
    first
        .get("/macro")
        .await
        .unwrap()
        .assert_ok()
        .assert_exact_json(&json!("http:2024-01-02"));
    assert!(!DateFactory::default().has_macro("stamp"));
}
struct MacroJob(Arc<Mutex<Vec<String>>>);
impl openrust::contracts::queue::JobHandler for MacroJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        _: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async move {
            let value = Date::macro_call::<String, _>("stamp", "worker:".to_owned())
                .map_err(openrust::queue::QueueError::execution)?;
            self.0.lock().unwrap().push(value);
            Ok(())
        })
    }
}
#[tokio::test]
async fn actual_artisan_sqlite_worker_consumes_registered_date_macro_through_the_public_facade() {
    use openrust::{
        queue::{DispatchOptions, Payload},
        support::facades::Queue,
    };
    let mut app=TestApp::sqlite(|b|b.with_config(|_,_|Ok([
  ("app".into(),json!({"env":"testing","timezone":"UTC"})),
  ("queue".into(),json!({"default":"database","connections":{"database":{"driver":"database","table":"date_macro_jobs","retry_after":30}},"failed":{"driver":"null"}}))
 ].into()))).unwrap();
    app.travel_to(DateFactory::default().parse("2024-01-02", None).unwrap());
    app.boot().await.unwrap();
    app.database()
        .await
        .unwrap()
        .get_schema_builder()
        .create("date_macro_jobs", |t| {
            t.id();
            t.string("queue", 255).index();
            t.long_text("payload");
            t.unsigned_integer("attempts");
            t.unsigned_big_integer("reserved_at").nullable();
            t.unsigned_big_integer("available_at");
            t.unsigned_big_integer("created_at");
        })
        .await
        .unwrap();
    let completed = Arc::new(Mutex::new(Vec::new()));
    App::set_facade_application(Some(app.app()), async {
        Date::r#macro("stamp", stamp()).unwrap();
        Queue::register("date-macro", Arc::new(MacroJob(completed.clone()))).unwrap();
        Queue::without_interruption_polling().unwrap();
        for _ in 0..2 {
            Queue::dispatch(
                Payload::new("date-macro", ()).unwrap(),
                DispatchOptions {
                    queue: Some("date-macro-isolated".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        }
    })
    .await;
    app.artisan([
        "queue:work",
        "database",
        "--queue=date-macro-isolated",
        "--stop-when-empty",
        "--sleep=0",
        "--timeout=0",
        "--tries=1",
    ])
    .await
    .assert_successful();
    assert_eq!(*completed.lock().unwrap(), vec!["worker:2024-01-02"; 2]);
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(
            Queue::connection(None)
                .unwrap()
                .size(Some("date-macro-isolated"))
                .await
                .unwrap(),
            0
        );
    })
    .await;
}
