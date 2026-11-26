use openrust::{
    config::{ConfigError, ConfigMacro, Repository},
    contracts::config::Repository as _,
    support::facades::{App, Config, Facade},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[tokio::test]
async fn macros_use_current_repository_and_keep_transient_non_send_object_identity() {
    let mut app =
        TestApp::create(|b| b.with_config(|_, _| Ok([("name".into(), json!("first"))].into())))
            .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()),async{
  Config::r#macro("label",|repo,prefix:String|Ok(format!("{prefix}:{}",repo.string("name")?))).unwrap();
  assert_eq!(Config::macro_call::<String,_>("label","before".to_string()).unwrap(),"before:first");
  Config::swap(Arc::new(Repository::new([("name".into(),json!("second"))].into()))).unwrap();
  assert!(Config::has_macro("label").unwrap());
  assert_eq!(Config::macro_call::<String,_>("label","after".to_string()).unwrap(),"after:second");
  Config::r#macro("identity",|_,value:Rc<Cell<usize>>|{value.set(value.get()+1);Ok(value)}).unwrap();
  let original=Rc::new(Cell::new(1));let result=Config::macro_call::<Rc<Cell<usize>>,_>("identity",original.clone()).unwrap();assert!(Rc::ptr_eq(&result,&original));assert_eq!(original.get(),2);
  assert!(matches!(Config::macro_call::<String,_>("identity",7),Err(ConfigError::MacroType{name,..}) if name=="identity"));
  assert!(matches!(Config::macro_call::<u32,_>("identity",original),Err(ConfigError::MacroType{name,..}) if name=="identity"));
 }).await;
}
#[tokio::test]
async fn lazy_mixins_preserve_order_skip_factories_and_allow_reentry() {
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let calls = Cell::new(0);
        Config::r#macro("same", |_, (): ()| Ok(1_i32)).unwrap();
        for replace in [false, true] {
            Config::mixin_using(
                [("same", || {
                    calls.set(calls.get() + 1);
                    Ok(ConfigMacro::new(|_, (): ()| Ok(2_i32)))
                })],
                replace,
            )
            .unwrap();
            assert_eq!(
                Config::macro_call::<i32, _>("same", ()).unwrap(),
                if replace { 2 } else { 1 }
            );
        }
        assert_eq!(calls.get(), 1);
        type Factory = Box<dyn FnOnce() -> Result<ConfigMacro, ConfigError>>;
        let factories: Vec<(&str, Factory)> = vec![
            (
                "before",
                Box::new(|| Ok(ConfigMacro::new(|_, (): ()| Ok(3_i32)))),
            ),
            (
                "failure",
                Box::new(|| {
                    Err(ConfigError::InvalidKey {
                        key: "factory".into(),
                    })
                }),
            ),
            ("after", Box::new(|| panic!("不应执行失败后的工厂"))),
        ];
        assert!(Config::mixin_using(factories, true).is_err());
        assert!(Config::has_macro("before").unwrap());
        assert!(!Config::has_macro("after").unwrap());
        Config::r#macro("reenter", |repo, (): ()| {
            repo.flush_macros()?;
            repo.register_macro("installed", ConfigMacro::new(|_, (): ()| Ok(4_i32)))?;
            Ok(5_i32)
        })
        .unwrap();
        assert_eq!(Config::macro_call::<i32, _>("reenter", ()).unwrap(), 5);
        assert!(!Config::has_macro("same").unwrap());
        assert_eq!(Config::macro_call::<i32, _>("installed", ()).unwrap(), 4);
        Config::flush_macros().unwrap();
        assert!(matches!(
            Config::macro_call::<i32, _>("installed", ()),
            Err(ConfigError::UnknownMacro(_))
        ));
    })
    .await;
}
#[test]
fn standalone_repository_macros_and_existing_methods_are_independent() {
    let repository = Repository::new([("name".into(), json!("plain"))].into());
    repository
        .r#macro("get", |_, (): ()| Ok("macro".to_string()))
        .unwrap();
    assert_eq!(repository.get("name"), Some(json!("plain")));
    assert_eq!(
        repository.macro_call::<String, _>("get", ()).unwrap(),
        "macro"
    );
    repository
        .mixin(
            [(
                "get",
                ConfigMacro::new(|_, (): ()| Ok("ignored".to_string())),
            )],
            false,
        )
        .unwrap();
    assert_eq!(
        repository.macro_call::<String, _>("get", ()).unwrap(),
        "macro"
    );
    assert!(
        !Repository::new(Default::default())
            .has_macro("get")
            .unwrap()
    );
    repository.flush_macros().unwrap();
    assert!(!repository.has_macro("get").unwrap());
}
fn endpoint(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let label = Config::macro_call::<String, _>("label", ()).unwrap();
        openrust::http::Response::json(openrust::http::StatusCode::OK, json!({"label":label}))
    })
}
#[tokio::test]
async fn concurrent_http_applications_and_real_artisan_read_current_configuration() {
    fn commands() -> Result<(), openrust::console::ConsoleError> {
        openrust::support::facades::Artisan::command("fixture:config-macro", |output| {
            output.comment(&Config::macro_call::<String, _>("label", ()).unwrap());
        })?
        .purpose("验收配置宏")
    }
    fn make() -> TestApp {
        TestApp::create(|b| {
            b.with_routing(
                openrust::foundation::configuration::Routing::default()
                    .web(|| {
                        openrust::support::facades::Route::get("/macro", endpoint)?;
                        Ok(())
                    })
                    .commands(commands),
            )
        })
        .unwrap()
    }
    let mut first = make();
    let mut second = make();
    first.boot().await.unwrap();
    second.boot().await.unwrap();
    for (app, name) in [(&first, "first"), (&second, "second")] {
        App::set_facade_application(Some(app.app()), async {
            Config::set("name", json!(name)).unwrap();
            Config::r#macro("label", |repo, (): ()| repo.string("name")).unwrap();
        })
        .await;
    }
    let (one, two) = tokio::join!(first.get("/macro"), second.get("/macro"));
    one.unwrap()
        .assert_ok()
        .assert_exact_json(&json!({"label":"first"}));
    two.unwrap()
        .assert_ok()
        .assert_exact_json(&json!({"label":"second"}));
    first
        .artisan(["fixture:config-macro"])
        .await
        .assert_successful()
        .expects_output("first");
    second
        .artisan(["fixture:config-macro"])
        .await
        .assert_successful()
        .expects_output("second");
    App::set_facade_application(Some(first.app()), async {
        Config::flush_macros().unwrap();
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        assert!(Config::has_macro("label").unwrap());
    })
    .await;
}
struct Override {
    registry: Arc<openrust::config::ConfigMacroRegistry>,
    calls: Arc<AtomicUsize>,
}
impl openrust::contracts::config::Repository for Override {
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
    fn macro_registry(&self) -> Result<Arc<openrust::config::ConfigMacroRegistry>, ConfigError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.registry.clone())
    }
    fn string(&self, _: &str) -> Result<String, ConfigError> {
        Ok("override".into())
    }
}
#[tokio::test]
async fn replaced_contract_controls_registry_and_macro_receiver() {
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    App::set_facade_application(Some(app.app()), async {
        Config::swap(Arc::new(Override {
            registry: Arc::default(),
            calls: calls.clone(),
        }))
        .unwrap();
        Config::r#macro("label", |repo, (): ()| repo.string("name")).unwrap();
        assert_eq!(
            Config::macro_call::<String, _>("label", ()).unwrap(),
            "override"
        );
        assert!(calls.load(Ordering::SeqCst) >= 2);
    })
    .await;
}

#[tokio::test]
async fn registry_trace_matches_fixed_original_config_facade() {
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/config/macros-contract.json")).unwrap();
    let mut app =
        TestApp::create(|b| b.with_config(|_, _| Ok([("name".into(), json!("first"))].into())))
            .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let mut out = Vec::<Value>::new();
        Config::flush_macros().unwrap();
        Config::r#macro("label", |repo, prefix: String| {
            Ok(format!("{prefix}:{}", repo.string("name")?))
        })
        .unwrap();
        out.push(json!(
            Config::macro_call::<String, _>("label", "before".to_string()).unwrap()
        ));
        Config::swap(Arc::new(Repository::new(
            [("name".into(), json!("second"))].into(),
        )))
        .unwrap();
        out.push(json!(Config::has_macro("label").unwrap()));
        out.push(json!(
            Config::macro_call::<String, _>("label", "after".to_string()).unwrap()
        ));
        let calls = Cell::new(0);
        for replace in [false, true] {
            Config::mixin_using(
                [("label", 0), ("extra", 1)].map(|(name, kind)| {
                    (name, {
                        let calls = &calls;
                        move || {
                            calls.set(calls.get() + 1);
                            Ok(if kind == 0 {
                                ConfigMacro::new(|_, prefix: String| Ok(format!("{prefix}:mixed")))
                            } else {
                                ConfigMacro::new(|_, (): ()| Ok(9_i32))
                            })
                        }
                    })
                }),
                replace,
            )
            .unwrap();
            out.push(json!([
                calls.get(),
                Config::macro_call::<String, _>(
                    "label",
                    if replace { "replaced" } else { "kept" }.to_string()
                )
                .unwrap(),
                Config::macro_call::<i32, _>("extra", ()).unwrap()
            ]));
        }
        type Factory = Box<dyn FnOnce() -> Result<ConfigMacro, ConfigError>>;
        let factories: Vec<(&str, Factory)> = vec![
            (
                "before",
                Box::new(|| Ok(ConfigMacro::new(|_, (): ()| Ok(3_i32)))),
            ),
            (
                "failure",
                Box::new(|| {
                    Err(ConfigError::InvalidKey {
                        key: "factory".into(),
                    })
                }),
            ),
        ];
        assert!(Config::mixin_using(factories, true).is_err());
        out.push(json!("factory"));
        out.push(json!([
            Config::macro_call::<i32, _>("before", ()).unwrap(),
            Config::has_macro("after").unwrap()
        ]));
        Config::r#macro("reenter", |repo, (): ()| {
            repo.flush_macros()?;
            repo.register_macro("installed", ConfigMacro::new(|_, (): ()| Ok(4_i32)))?;
            Ok(5_i32)
        })
        .unwrap();
        out.push(json!([
            Config::macro_call::<i32, _>("reenter", ()).unwrap(),
            Config::has_macro("label").unwrap(),
            Config::macro_call::<i32, _>("installed", ()).unwrap()
        ]));
        Config::r#macro("get", |_, (): ()| Ok("macro".to_string())).unwrap();
        out.push(Config::get("name").unwrap().unwrap());
        out.push(json!(Config::macro_call::<String, _>("get", ()).unwrap()));
        Config::flush_macros().unwrap();
        assert!(matches!(
            Config::macro_call::<Value, _>("missing", ()),
            Err(ConfigError::UnknownMacro(_))
        ));
        out.push(json!("BadMethodCallException"));
        assert_eq!(json!(out), expected);
    })
    .await;
}

struct MacroJob(Arc<std::sync::Mutex<Vec<String>>>);
impl openrust::contracts::queue::JobHandler for MacroJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        _: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async move {
            let value = Config::macro_call::<String, _>("stamp", "worker:".to_owned())
                .map_err(openrust::queue::QueueError::execution)?;
            self.0.lock().unwrap().push(value);
            Ok(())
        })
    }
}
#[tokio::test]
async fn actual_artisan_sqlite_worker_consumes_registered_config_macro_through_the_public_facade() {
    use openrust::{
        queue::{DispatchOptions, Payload},
        support::facades::Queue,
    };
    let mut app=TestApp::sqlite(|b|b.with_config(|_,_|Ok([
  ("app".into(),json!({"env":"testing","timezone":"UTC","locale":"en","fallback_locale":"en"})),
  ("queue".into(),json!({"default":"database","connections":{"database":{"driver":"database","table":"config_macro_jobs","retry_after":30}},"failed":{"driver":"null"}}))
 ].into()))).unwrap();
    app.boot().await.unwrap();
    app.database()
        .await
        .unwrap()
        .get_schema_builder()
        .create("config_macro_jobs", |t| {
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
    let completed = Arc::new(std::sync::Mutex::new(Vec::new()));
    App::set_facade_application(Some(app.app()), async {
        Config::r#macro("stamp", |repository, prefix: String| {
            let locale = repository.string("app.locale")?;
            repository.set("app.locale", json!("changed"))?;
            Ok(format!("{prefix}{locale}"))
        })
        .unwrap();
        Queue::register("config-macro", Arc::new(MacroJob(completed.clone()))).unwrap();
        Queue::without_interruption_polling().unwrap();
        for _ in 0..2 {
            Queue::dispatch(
                Payload::new("config-macro", ()).unwrap(),
                DispatchOptions {
                    queue: Some("config-macro-isolated".into()),
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
        "--queue=config-macro-isolated",
        "--stop-when-empty",
        "--sleep=0",
        "--timeout=0",
        "--tries=1",
    ])
    .await
    .assert_successful();
    assert_eq!(*completed.lock().unwrap(), vec!["worker:en"; 2]);
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(
            Queue::connection(None)
                .unwrap()
                .size(Some("config-macro-isolated"))
                .await
                .unwrap(),
            0
        );
    })
    .await;
}
