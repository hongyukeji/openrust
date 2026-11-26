use openrust::{
    config::{ConfigError, ConfigKey, Repository},
    contracts::config::Repository as Contract,
    support::facades::{App, Config, Facade},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::{cell::Cell, sync::Arc};
fn values() -> std::collections::BTreeMap<String, Value> {
    serde_json::from_value(json!({"text":"hello","integer":3,"float":3.0,"bool":false,"array":{"named":"value"},"list":[1,2],"null":null,"literal.dot":"literal","literal":{"dot":"nested"}})).unwrap()
}
fn check(result: Result<Value, ConfigError>, row: &Value, calls: u32) {
    assert_eq!(
        calls,
        u32::try_from(row["calls"].as_u64().unwrap()).unwrap(),
        "{row}"
    );
    if row["result"].get("error").is_some() {
        assert!(
            matches!(result, Err(ConfigError::InvalidType { .. })),
            "{row}"
        );
    } else if row["method"] == "float" {
        assert_eq!(
            result.unwrap().as_f64(),
            row["result"]["value"].as_f64(),
            "{row}"
        );
    } else {
        assert_eq!(result.unwrap(), row["result"]["value"], "{row}");
    }
}
#[tokio::test]
async fn typed_defaults_match_original_facade_and_repository_and_run_once_only_for_missing_keys() {
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/config/readers-contract.json")).unwrap();
    let mut app = TestApp::create(|b| b.with_config(|_, _| Ok(values()))).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let repository = Repository::new(values());
        for row in expected["typed"].as_array().unwrap() {
            let key = row["key"].as_str().unwrap();
            let calls = Cell::new(0);
            let other = Cell::new(0);
            let (result, direct) = match row["method"].as_str().unwrap() {
                "string" => (
                    Config::string_or_else(key, || {
                        calls.set(calls.get() + 1);
                        "fallback".into()
                    })
                    .map(Value::from),
                    repository
                        .string_or_else(key, || {
                            other.set(other.get() + 1);
                            "fallback".into()
                        })
                        .map(Value::from),
                ),
                "integer" => (
                    Config::integer_or_else(key, || {
                        calls.set(calls.get() + 1);
                        42
                    })
                    .map(Value::from),
                    repository
                        .integer_or_else(key, || {
                            other.set(other.get() + 1);
                            42
                        })
                        .map(Value::from),
                ),
                "float" => (
                    Config::float_or_else(key, || {
                        calls.set(calls.get() + 1);
                        1.25
                    })
                    .map(Value::from),
                    repository
                        .float_or_else(key, || {
                            other.set(other.get() + 1);
                            1.25
                        })
                        .map(Value::from),
                ),
                "boolean" => (
                    Config::boolean_or_else(key, || {
                        calls.set(calls.get() + 1);
                        true
                    })
                    .map(Value::from),
                    repository
                        .boolean_or_else(key, || {
                            other.set(other.get() + 1);
                            true
                        })
                        .map(Value::from),
                ),
                "array" => (
                    Config::array_or_else(key, || {
                        calls.set(calls.get() + 1);
                        json!({"default":1})
                    }),
                    repository.array_or_else(key, || {
                        other.set(other.get() + 1);
                        json!({"default":1})
                    }),
                ),
                "collection" => (
                    Config::collection_or_else(key, || {
                        calls.set(calls.get() + 1);
                        json!({"default":1})
                    })
                    .map(|value| value.to_array().unwrap()),
                    repository
                        .collection_or_else(key, || {
                            other.set(other.get() + 1);
                            json!({"default":1})
                        })
                        .map(|value| value.to_array().unwrap()),
                ),
                _ => unreachable!(),
            };
            check(result, row, calls.get());
            check(direct, row, other.get());
        }
        assert_eq!(
            Config::string_with_default("missing", "fallback").unwrap(),
            "fallback"
        );
        assert_eq!(Config::integer_with_default("missing", 42).unwrap(), 42);
        assert_eq!(Config::float_with_default("missing", 1.25).unwrap(), 1.25);
        assert!(Config::boolean_with_default("missing", true).unwrap());
        assert_eq!(
            Config::array_with_default("missing", json!([1])).unwrap(),
            json!([1])
        );
        assert_eq!(
            Config::collection_with_default("missing", json!({"a":1}))
                .unwrap()
                .to_array()
                .unwrap(),
            json!({"a":1})
        );
        assert!(Config::string_with_default("null", "fallback").is_err());
        assert!(Config::array_with_default("missing", json!(42)).is_err());
        assert!(Config::collection_with_default("missing", json!(42)).is_err());
        assert_eq!(repository.string("text").unwrap(), "hello");
        assert!(repository.string("missing").is_err());
        assert_eq!(
            repository
                .string_with_default("missing", "fallback".into())
                .unwrap(),
            "fallback"
        );
        assert!(
            Config::float_with_default("missing", f64::NAN)
                .unwrap()
                .is_nan()
        );
        assert_eq!(
            repository
                .integer_or_else("missing", || {
                    repository.set("side_effect", json!(1)).unwrap();
                    3
                })
                .unwrap(),
            3
        );
        assert_eq!(Config::integer("integer").unwrap(), 3);
        assert_eq!(Config::float("float").unwrap(), 3.0);
        assert!(!Config::boolean("bool").unwrap());
        assert_eq!(Config::array("array").unwrap(), json!({"named":"value"}));
        assert_eq!(
            Config::collection("list").unwrap().to_array().unwrap(),
            json!([1, 2])
        );
        assert_eq!(repository.integer("integer").unwrap(), 3);
        assert_eq!(repository.float("float").unwrap(), 3.0);
        assert!(!repository.boolean("bool").unwrap());
        assert_eq!(repository.array("array").unwrap(), json!({"named":"value"}));
        assert_eq!(
            repository.collection("list").unwrap().to_array().unwrap(),
            json!([1, 2])
        );
        assert_eq!(
            Config::array_with_default("array", json!(42)).unwrap(),
            json!({"named":"value"})
        );
        assert_eq!(repository.get("side_effect"), Some(json!(1)));
    })
    .await;
}
#[tokio::test]
async fn raw_and_many_reads_preserve_literal_keys_null_order_lazy_defaults_and_bulk_writes() {
    let mut app = TestApp::create(|b| b.with_config(|_, _| Ok(values()))).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let expected: Value =
            serde_json::from_str(include_str!("fixtures/config/readers-contract.json")).unwrap();
        let calls = Cell::new(0);
        for row in expected["raw"].as_array().unwrap() {
            let key = row["key"].as_str().unwrap();
            assert_eq!(Config::has(key).unwrap(), row["has"]);
            assert_eq!(
                Config::get_or_else(key, || {
                    calls.set(calls.get() + 1);
                    json!("default")
                })
                .unwrap(),
                row["value"]
            );
        }
        assert_eq!(calls.get(), expected["raw_calls"].as_u64().unwrap());
        let many = Config::get_many([
            ConfigKey::from("text"),
            ConfigKey::from(("missing", json!("fallback"))),
            ConfigKey::from(("null", json!("ignore"))),
            ConfigKey::from("bool"),
        ])
        .unwrap();
        assert_eq!(serde_json::to_value(many).unwrap(), expected["many"]);
        let plain = Config::get_many(["text", "missing", "null"]).unwrap();
        assert_eq!(
            plain.keys().map(String::as_str).collect::<Vec<_>>(),
            ["text", "missing", "null"]
        );
        let lazy = Cell::new(0);
        let many = Config::get_many([
            ConfigKey::lazy("missing", || {
                lazy.set(lazy.get() + 1);
                json!(1)
            }),
            ConfigKey::lazy("null", || panic!("显式null不应执行默认值")),
            ConfigKey::lazy("missing", || {
                lazy.set(lazy.get() + 1);
                json!(2)
            }),
        ])
        .unwrap();
        assert_eq!(lazy.get(), 2);
        assert_eq!(
            many.keys().map(String::as_str).collect::<Vec<_>>(),
            ["missing", "null"]
        );
        assert_eq!(many["missing"], json!(2));
        let sequential = Config::get_many([
            ConfigKey::lazy("seed", || {
                Config::set("later", json!(7)).unwrap();
                json!(1)
            }),
            ConfigKey::from("later"),
        ])
        .unwrap();
        assert_eq!(sequential["seed"], json!(1));
        assert_eq!(sequential["later"], json!(7));
        Config::set_many([("bulk.first", json!(1)), ("bulk.second", json!(2))]).unwrap();
        assert_eq!(Config::get("bulk").unwrap().unwrap(), expected["bulk"]);
        let repository = Repository::new(values());
        repository
            .set_many([("bulk.first", json!(1)), ("bulk.second", json!(2))])
            .unwrap();
        assert_eq!(repository.get("bulk").unwrap(), expected["bulk"]);
        assert_eq!(
            repository.get_many(["literal.dot"])["literal.dot"],
            json!("literal")
        );
        assert_eq!(
            repository.get_or_else("null", || panic!("null不能使用默认值")),
            Value::Null
        );
    })
    .await;
}
struct Override(Repository);
impl Contract for Override {
    fn has(&self, key: &str) -> bool {
        self.0.has(key)
    }
    fn get(&self, key: &str) -> Option<Value> {
        self.0.get(key)
    }
    fn all(&self) -> std::collections::BTreeMap<String, Value> {
        self.0.all()
    }
    fn set(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        self.0.set(key, value)
    }
    fn prepend(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        self.0.prepend(key, value)
    }
    fn push(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        self.0.push(key, value)
    }
    fn string_with_default(&self, _: &str, default: String) -> Result<String, ConfigError> {
        Ok(format!("override:{default}"))
    }
    fn string_or_else(
        &self,
        _: &str,
        default: Box<dyn FnOnce() -> String + '_>,
    ) -> Result<String, ConfigError> {
        Ok(format!("override:{}", default()))
    }
}
#[tokio::test]
async fn facade_respects_replaced_repository_typed_default_and_lazy_overrides() {
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Config::swap(Arc::new(Override(Repository::new(Default::default())))).unwrap();
        assert_eq!(
            Config::string_with_default("key", "default").unwrap(),
            "override:default"
        );
        assert_eq!(
            Config::string_or_else("key", || "lazy".into()).unwrap(),
            "override:lazy"
        );
    })
    .await;
}
#[test]
fn unavailable_repository_does_not_invoke_lazy_defaults_or_hide_binding_errors() {
    let called = Cell::new(0);
    assert!(
        Config::string_or_else("missing", || {
            called.set(called.get() + 1);
            "fallback".into()
        })
        .is_err()
    );
    assert!(
        Config::get_or_else("missing", || {
            called.set(called.get() + 1);
            Value::Null
        })
        .is_err()
    );
    assert_eq!(called.get(), 0);
}
fn configured_endpoint(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let result=(||->Result<Value,ConfigError>{
   Config::set_many([("runtime.number",json!(7)),("runtime.flag",json!(false))])?;
   let lazy=Cell::new(0);let value=Config::get_many([ConfigKey::from("app.name"),ConfigKey::lazy("missing",||{lazy.set(lazy.get()+1);json!(42)})])?;
   Ok(json!({"name":Config::string_with_default("app.name","fallback")?,"missing":Config::integer_or_else("missing",||99)?,"many":value,"default_calls":lazy.get(),"number":Config::integer("runtime.number")?,"flag":Config::boolean_with_default("runtime.flag",true)?}))
  })().map_err(|source|openrust::http::HttpError::Internal{source:Box::new(source)})?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, result)
    })
}
fn invalid_endpoint(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let value = Config::integer_with_default("app.name", 99).map_err(|source| {
            openrust::http::HttpError::Internal {
                source: Box::new(source),
            }
        })?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, value)
    })
}
#[tokio::test]
async fn real_http_uses_strict_and_lazy_defaults_and_reports_existing_wrong_types() {
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"name":"Configured","env":"testing","debug":false}),
            )]
            .into())
        })
        .with_routing(
            openrust::foundation::configuration::Routing::default().web(|| {
                openrust::support::facades::Route::get("/configuration", configured_endpoint)?;
                openrust::support::facades::Route::get("/invalid", invalid_endpoint)
            }),
        )
    })
    .unwrap();
    app.get("/configuration").await.unwrap().assert_ok().assert_exact_json(&json!({"name":"Configured","missing":99,"many":{"app.name":"Configured","missing":42},"default_calls":1,"number":7,"flag":false}));
    let response = app
        .call(
            http::Request::get("/invalid")
                .header("accept", "application/json")
                .body(openrust::http::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response.assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(response.json::<Value>().unwrap().is_object());
}
#[tokio::test]
async fn real_artisan_consumer_reads_strict_defaults_and_bulk_values_from_its_own_application() {
    fn commands() -> Result<(), openrust::console::ConsoleError> {
        openrust::support::facades::Artisan::command("fixture:read-config-defaults", |output| {
            let name = Config::string_with_default("app.name", "fallback").unwrap();
            let count = Config::integer_or_else("missing", || 7).unwrap();
            let values = Config::get_many(["app.name", "missing"]).unwrap();
            output.comment(&format!("{name}:{count}:{}", values["missing"]));
        })?
        .purpose("验收配置默认值")
    }
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"name":"Console"}))].into()))
            .with_routing(
                openrust::foundation::configuration::Routing::default().commands(commands),
            )
    })
    .unwrap();
    app.artisan(["fixture:read-config-defaults"])
        .await
        .assert_successful()
        .expects_output("Console:7:null");
}
