use openrust::{
    container::Callback,
    contracts::foundation::Application as _,
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::{Config, Route},
    testing::TestApp,
};
use serde_json::{Value, json};

fn assert_ui(expected: Value) {
    assert_eq!(Config::get("app.ui").unwrap(), Some(expected.clone()));
    assert_eq!(Config::get("app").unwrap().unwrap()["ui"], expected);
    assert_eq!(Config::all().unwrap()["app"]["ui"], expected);
    assert!(Config::has("app.ui").unwrap());
    assert_eq!(
        Config::has("app.ui.title").unwrap(),
        expected.get("title").is_some()
    );
    assert_eq!(
        Config::get("app.ui.title").unwrap(),
        expected.get("title").cloned()
    );
}

fn replace(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Config::set("app", json!({})).unwrap();
        Config::set("app.ui.title", json!("旧标题")).unwrap();
        Config::set("app.ui", json!({"theme":"dark"})).unwrap();
        assert_ui(json!({"theme":"dark"}));
        Config::set("app.ui.title", json!("新标题")).unwrap();
        assert_ui(json!({"theme":"dark", "title":"新标题"}));
        for value in [
            json!(null),
            json!(7),
            json!("plain"),
            json!(false),
            json!([]),
            json!(["first", "second"]),
            json!({"theme":"light"}),
        ] {
            Config::set("app.ui.title", json!("应被替换")).unwrap();
            Config::set("app.ui", value.clone()).unwrap();
            assert_ui(value.clone());
            Config::set("app.ui", value.clone()).unwrap();
            assert_ui(value);
        }
        // 相邻前缀不能被父键替换误删。
        Config::set("app.ui_extra.title", json!("保留")).unwrap();
        Config::set("app.ui", json!({})).unwrap();
        assert_ui(json!({}));
        assert_eq!(Config::string("app.ui_extra.title").unwrap(), "保留");
        Config::set("app.ui.theme.name", json!("old")).unwrap();
        Config::set("app.ui.theme", json!({"mode":"dark"})).unwrap();
        assert_ui(json!({"theme":{"mode":"dark"}}));
        tokio::task::yield_now().await;
        Ok(Response::from("consistent"))
    })
}

fn arrays(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Config::set("app", json!({"ui":[{"title":"one"},{"title":"two"}]})).unwrap();
        Config::set("app.ui.0.title", json!("changed")).unwrap();
        assert_ui(json!([{"title":"changed"},{"title":"two"}]));
        assert_eq!(Config::get("app.ui.1.title").unwrap(), Some(json!("two")));
        Config::set("app.ui.0", json!({"theme":"dark"})).unwrap();
        assert_ui(json!([{"theme":"dark"},{"title":"two"}]));
        Config::push("app.ui", json!({"title":"last"})).unwrap();
        Config::prepend("app.ui", json!({"title":"first"})).unwrap();
        assert_ui(json!([{"title":"first"},{"theme":"dark"},{"title":"two"},{"title":"last"}]));
        Config::set("app.ui.1.theme", json!("light")).unwrap();
        assert_ui(json!([{"title":"first"},{"theme":"light"},{"title":"two"},{"title":"last"}]));
        Config::set("app.ui", json!([])).unwrap();
        Config::set("app.ui.0", json!("zero")).unwrap();
        Config::set("app.ui.1", json!("one")).unwrap();
        assert_ui(json!(["zero", "one"]));
        Config::set("app.ui.4", json!("four")).unwrap();
        assert_ui(json!({"0":"zero","1":"one","4":"four"}));
        Config::set("app.ui.01", json!("literal")).unwrap();
        assert_eq!(Config::get("app.ui.01").unwrap(), Some(json!("literal")));
        Config::set("app.ui", json!(null)).unwrap();
        assert!(Config::prepend("app.ui", json!(1)).is_err());
        assert_ui(json!(null));
        Config::push("app.list", json!("tail")).unwrap();
        Config::prepend("app.list", json!("head")).unwrap();
        assert_eq!(
            Config::get("app.list").unwrap(),
            Some(json!(["head", "tail"]))
        );
        Ok(Response::from("consistent"))
    })
}

fn test_app() -> TestApp {
    TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                Ok([(
                    "app".into(),
                    json!({"locale":"en", "fallback_locale":"en", "ui":{"title":"default"}}),
                )]
                .into())
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/replace", replace)?;
                Route::get("/arrays", arrays)?;
                Route::get("/mixed-arrays", mixed_arrays)
            }))
    })
    .unwrap()
}

#[tokio::test]
async fn nested_parent_replacements_follow_write_order_in_http() {
    let mut app = test_app();
    for _ in 0..2 {
        app.get("/replace")
            .await
            .unwrap()
            .assertOk()
            .assertContent("consistent");
        assert_eq!(
            app.app().config().unwrap().get("app.ui"),
            Some(json!({"title":"default"}))
        );
    }
}

#[tokio::test]
async fn scoped_array_indexes_and_list_mutations_preserve_siblings() {
    let mut app = test_app();
    app.get("/arrays")
        .await
        .unwrap()
        .assertOk()
        .assertContent("consistent");
    assert_eq!(
        app.app().config().unwrap().get("app.ui"),
        Some(json!({"title":"default"}))
    );
    assert!(!app.app().config().unwrap().has("app.list"));
}

#[tokio::test]
async fn task_lifecycles_are_isolated_and_nested_calls_share_the_current_lifecycle() {
    let mut test = test_app();
    test.boot().await.unwrap();
    let app = test.app();
    app.call(
        Callback::new(&[], |_| async {
            app.container()
                .scope(async {
                    Config::set("app", json!({"ui":{"title":"outer"}})).unwrap();
                    app.container()
                        .scope(async {
                            Config::set("app", json!({})).unwrap();
                            Config::set("app.ui.title", json!("old")).unwrap();
                            Config::set("app.ui", json!({"theme":"inner"})).unwrap();
                            assert_ui(json!({"theme":"inner"}));
                        })
                        .await;
                    assert_ui(json!({"theme":"inner"}));
                })
                .await;
            assert_ui(json!({"title":"default"}));
            let task = |title: &'static str| {
                app.container().scope(async move {
                    Config::set("app", json!({"ui":{"title":title}})).unwrap();
                    tokio::task::yield_now().await;
                    assert_ui(json!({"title":title}));
                })
            };
            tokio::join!(task("one"), task("two"));
            assert_ui(json!({"title":"default"}));
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        app.config().unwrap().get("app.ui"),
        Some(json!({"title":"default"}))
    );
}

fn mixed_arrays(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Config::set("app", json!({"ui":null})).unwrap();
        assert!(Config::prepend("app.ui", json!("head")).is_err());
        assert_ui(json!(null));
        Config::push("app.ui", json!("first")).unwrap();
        assert_ui(json!(["first"]));
        Config::set("app.ui", json!({"name":"kept", "4":"four"})).unwrap();
        Config::push("app.ui", json!("tail")).unwrap();
        assert_ui(json!({"name":"kept", "4":"four", "5":"tail"}));
        Config::prepend("app.ui", json!("head")).unwrap();
        assert_ui(json!({"0":"head", "name":"kept", "1":"four", "2":"tail"}));
        for value in [json!(true), json!(false), json!(7), json!("text")] {
            Config::set("app.ui", value.clone()).unwrap();
            assert!(Config::push("app.ui", json!(1)).is_err());
            assert!(Config::prepend("app.ui", json!(1)).is_err());
            assert_ui(value);
        }
        Ok(Response::from("consistent"))
    })
}
#[tokio::test]
async fn null_push_and_associative_arrays_preserve_configuration_contracts() {
    let mut app = test_app();
    app.get("/mixed-arrays")
        .await
        .unwrap()
        .assertOk()
        .assertContent("consistent");
    assert_eq!(
        app.app().config().unwrap().get("app.ui"),
        Some(json!({"title":"default"}))
    );
}
