use openrust::{
    config::{ConfigError, Repository},
    contracts::config::Repository as Contract,
    support::facades::{App, Config, Facade},
    testing::TestApp,
};
use serde_json::{Value, json};
fn initial() -> std::collections::BTreeMap<String, Value> {
    serde_json::from_value(json!({"app":{"name":"Old","null":null},"literal.dot":"literal","literal":{"dot":"nested"},"items":["a","b"]})).unwrap()
}
#[tokio::test]
async fn public_array_access_and_empty_literal_paths_match_original_facade_and_repository() {
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/config/offsets-contract.json")).unwrap();
    let mut app = TestApp::create(|b| b.with_config(|_, _| Ok(initial()))).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let repository = Repository::new(initial());
        for row in expected.as_array().unwrap() {
            let key = row["key"].as_str().unwrap();
            let value = row["value"].clone();
            let (result, direct) = match row["op"].as_str().unwrap() {
                "exists" => (
                    Value::Bool(Config::offset_exists(key).unwrap()),
                    Value::Bool(repository.offset_exists(key)),
                ),
                "get" => (
                    Config::offset_get(key).unwrap().unwrap_or(Value::Null),
                    repository.offset_get(key).unwrap_or(Value::Null),
                ),
                "set" => {
                    Config::offset_set(key, value.clone()).unwrap();
                    repository.offset_set(key, value).unwrap();
                    (Value::Null, Value::Null)
                }
                "unset" => {
                    Config::offset_unset(key).unwrap();
                    repository.offset_unset(key).unwrap();
                    (Value::Null, Value::Null)
                }
                "push" => {
                    Config::push(key, value.clone()).unwrap();
                    repository.push(key, value).unwrap();
                    (Value::Null, Value::Null)
                }
                "prepend" => {
                    Config::prepend(key, value.clone()).unwrap();
                    repository.prepend(key, value).unwrap();
                    (Value::Null, Value::Null)
                }
                _ => unreachable!(),
            };
            assert_eq!(result, row["result"]["value"], "{row}");
            assert_eq!(direct, row["result"]["direct"], "{row}");
            assert_eq!(
                serde_json::to_value(Config::all().unwrap()).unwrap(),
                row["result"]["all"],
                "{row}"
            );
            assert_eq!(
                serde_json::to_value(repository.all()).unwrap(),
                row["result"]["all"],
                "{row}"
            );
            assert_eq!(Config::has(key).unwrap(), row["result"]["has"]);
            assert_eq!(
                Config::get(key).unwrap().unwrap_or(Value::Null),
                row["result"]["read"]
            );
        }
    })
    .await;
}
struct Override {
    count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl Contract for Override {
    fn has(&self, _: &str) -> bool {
        false
    }
    fn get(&self, _: &str) -> Option<Value> {
        None
    }
    fn all(&self) -> std::collections::BTreeMap<String, Value> {
        Default::default()
    }
    fn set(&self, _: &str, _: Value) -> Result<(), ConfigError> {
        Ok(())
    }
    fn prepend(&self, _: &str, _: Value) -> Result<(), ConfigError> {
        Ok(())
    }
    fn push(&self, _: &str, _: Value) -> Result<(), ConfigError> {
        Ok(())
    }
    fn offset_exists(&self, _: &str) -> bool {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    }
    fn offset_get(&self, _: &str) -> Option<Value> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Some(json!(42))
    }
    fn offset_set(&self, _: &str, _: Value) -> Result<(), ConfigError> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn offset_unset(&self, _: &str) -> Result<(), ConfigError> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}
#[tokio::test]
async fn array_access_facade_forwards_each_actual_repository_override() {
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    App::set_facade_application(Some(app.app()), async {
        Config::swap(std::sync::Arc::new(Override {
            count: count.clone(),
        }))
        .unwrap();
        assert!(Config::offset_exists("x").unwrap());
        assert_eq!(Config::offset_get("x").unwrap(), Some(json!(42)));
        Config::offset_set("x", json!(7)).unwrap();
        Config::offset_unset("x").unwrap();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 4);
    })
    .await;
}

fn clear_locale(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        Config::offset_unset("app.locale").unwrap();
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"exists":Config::offset_exists("app.locale").unwrap(),"value":Config::offset_get("app.locale").unwrap()}),
        )
    })
}
#[tokio::test]
async fn http_unset_keeps_null_presence_and_restores_request_scoped_locale() {
    use openrust::contracts::foundation::Application as _;
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"locale":"en","fallback_locale":"en","supported_locales":[]}),
            )]
            .into())
        })
        .with_routing(
            openrust::foundation::configuration::Routing::default()
                .web(|| openrust::support::facades::Route::get("/clear", clear_locale)),
        )
    })
    .unwrap();
    for _ in 0..2 {
        app.get("/clear")
            .await
            .unwrap()
            .assert_ok()
            .assert_exact_json(&json!({"exists":true,"value":null}));
        assert_eq!(
            app.app().config().unwrap().get("app.locale"),
            Some(json!("en"))
        );
    }
}
#[tokio::test]
async fn artisan_uses_array_access_without_dropping_null_keys() {
    fn commands() -> Result<(), openrust::console::ConsoleError> {
        openrust::support::facades::Artisan::command("fixture:config-offsets", |output| {
            Config::offset_set("runtime..value", json!(7)).unwrap();
            assert_eq!(
                Config::offset_get("runtime..value").unwrap(),
                Some(json!(7))
            );
            Config::offset_unset("runtime..value").unwrap();
            output.comment(&format!(
                "{}:{}",
                Config::offset_exists("runtime..value").unwrap(),
                Config::offset_get("runtime..value").unwrap().unwrap()
            ));
        })?
        .purpose("验收配置下标入口")
    }
    let mut app = TestApp::create(|b| {
        b.with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
    })
    .unwrap();
    app.artisan(["fixture:config-offsets"])
        .await
        .assert_successful()
        .expects_output("true:null");
}
