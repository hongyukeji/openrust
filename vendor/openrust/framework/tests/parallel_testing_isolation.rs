use openrust::contracts::cache::Repository as _;
use openrust::{
    support::facades::{App, Blade, Config, Facade, ParallelTesting, View},
    testing::{ParallelTesting as Service, TestApp},
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
fn service(token: &str) -> Service {
    Service::new(BTreeMap::from([
        ("LARAVEL_PARALLEL_TESTING".into(), "1".into()),
        ("TEST_TOKEN".into(), token.into()),
    ]))
    .with_default_isolation()
}
async fn fixture() -> TestApp {
    let mut case = TestApp::create(|builder| {
        builder.with_config(|root, _| {
            Ok([
 ("cache".into(),json!({"prefix":"base_","default":"array","stores":{"array":{"driver":"array"}}})),
 ("view".into(),json!({"paths":[root.join("resources/views")],"compiled":"compiled/base///"}))
 ].into())
        })
    })
    .unwrap();
    std::fs::create_dir_all(case.root().join("resources/views")).unwrap();
    std::fs::write(
        case.root().join("resources/views/hello.blade.html"),
        "hello",
    )
    .unwrap();
    case.boot().await.unwrap();
    case
}
#[tokio::test]
async fn existing_view_and_compiler_follow_original_base_without_nesting_and_cleanup_only_token_directory()
 {
    let mut case = fixture().await;
    let app =
        App::set_facade_application(Some(case.app()), async { App::get_facade_root().unwrap() })
            .await;
    App::set_facade_application(Some(&app), async {
        ParallelTesting::swap(Arc::new(service("one"))).unwrap();
        assert_eq!(View::render("hello", &Default::default()).unwrap(), "hello");
        let mut retained = View::make("hello", Default::default()).unwrap();
        let template = case.root().join("resources/views/hello.blade.html");
        let old = Blade::get_compiled_path(&template).unwrap();
        assert!(old.exists());
        ParallelTesting::call_set_up_process_callbacks()
            .await
            .unwrap();
        let first = case.root().join("compiled/base/test_one");
        assert!(first.is_dir());
        ParallelTesting::call_set_up_test_case_callbacks(&mut case)
            .await
            .unwrap();
        assert_eq!(
            Config::get("cache.prefix").unwrap(),
            Some(json!("base_test_one_"))
        );
        assert_eq!(
            Config::get("view.compiled").unwrap(),
            Some(json!("compiled/base/test_one"))
        );
        assert_eq!(
            Blade::get_compiled_path(&template)
                .unwrap()
                .parent()
                .unwrap(),
            first
        );
        assert_eq!(View::render("hello", &Default::default()).unwrap(), "hello");
        assert_eq!(
            openrust::contracts::view::View::render(&mut retained).unwrap(),
            "hello"
        );
        let compiled = Blade::get_compiled_path(&template).unwrap();
        assert!(compiled.exists());
        ParallelTesting::resolve_token_using(Some(Arc::new(|| Ok(json!("two"))))).unwrap();
        ParallelTesting::call_set_up_process_callbacks()
            .await
            .unwrap();
        ParallelTesting::call_set_up_test_case_callbacks(&mut case)
            .await
            .unwrap();
        assert_eq!(
            Config::get("cache.prefix").unwrap(),
            Some(json!("base_test_two_"))
        );
        assert_eq!(
            Config::get("view.compiled").unwrap(),
            Some(json!("compiled/base/test_two"))
        );
        assert_eq!(View::render("hello", &Default::default()).unwrap(), "hello");
        let second = case.root().join("compiled/base/test_two");
        assert!(Blade::get_compiled_path(&template).unwrap().exists());
        ParallelTesting::call_tear_down_process_callbacks()
            .await
            .unwrap();
        assert!(!second.exists());
        assert!(first.exists());
        assert!(old.exists());
        ParallelTesting::call_tear_down_process_callbacks()
            .await
            .unwrap();
    })
    .await;
}
#[tokio::test]
async fn absent_views_skip_filesystem_and_without_cache_keeps_the_configured_prefix() {
    let mut case = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("cache".into(), json!({"prefix":"keep"}))].into()))
    })
    .unwrap();
    case.boot().await.unwrap();
    let app =
        App::set_facade_application(Some(case.app()), async { App::get_facade_root().unwrap() })
            .await;
    App::set_facade_application(Some(&app), async {
        ParallelTesting::swap(Arc::new(service("1"))).unwrap();
        ParallelTesting::resolve_options_using(Some(Arc::new(|key| {
            Ok(json!(key == "without_cache"))
        })))
        .unwrap();
        ParallelTesting::call_set_up_process_callbacks()
            .await
            .unwrap();
        ParallelTesting::call_set_up_test_case_callbacks(&mut case)
            .await
            .unwrap();
        assert_eq!(Config::get("cache.prefix").unwrap(), Some(json!("keep")));
        assert_eq!(Config::get("view.compiled").unwrap(), None);
        ParallelTesting::call_tear_down_process_callbacks()
            .await
            .unwrap();
    })
    .await;
}
#[tokio::test]
async fn unsafe_path_tokens_fail_before_creating_or_removing_directories() {
    let case = fixture().await;
    App::set_facade_application(Some(case.app()), async {
        ParallelTesting::swap(Arc::new(service("../escape"))).unwrap();
        assert!(
            ParallelTesting::call_set_up_process_callbacks()
                .await
                .is_err()
        );
        assert!(
            ParallelTesting::call_tear_down_process_callbacks()
                .await
                .is_err()
        );
    })
    .await;
}
#[tokio::test]
async fn real_redis_identical_keys_are_isolated_by_token_and_keep_explicit_pre_resolved_store_behavior()
 {
    use openrust::support::facades::Cache;
    fn configured() -> TestApp {
        TestApp::create(|b|b.with_config(|_,_|{
 let url=std::env::var("CACHE_FACADE_REDIS_URL").expect("此用例必须由独立Redis验收夹具提供连接");
 Ok([("database".into(),json!({"redis":{"default":{"url":url}}})),("cache".into(),json!({"prefix":"parallel-isolation:","default":"redis","stores":{"redis":{"driver":"redis"}}}))].into())
 })).unwrap()
    }
    let mut first = configured();
    let mut second = configured();
    first.boot().await.unwrap();
    second.boot().await.unwrap();
    for (case, token) in [(&first, "first"), (&second, "second")] {
        App::set_facade_application(Some(case.app()), async {
            ParallelTesting::swap(Arc::new(service(token))).unwrap();
            Config::set("cache.prefix", json!(format!("app-{token}:"))).unwrap();
        })
        .await;
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let a = barrier.clone();
    let b = barrier.clone();
    let one = first.run_parallel_test(move |_| {
        Box::pin(async move {
            Cache::forever("shared", &"one".into()).await?;
            a.wait().await;
            assert_eq!(Cache::get("shared").await?, Some("one".into()));
            assert_eq!(
                Config::get("cache.prefix")?,
                Some(json!("app-first:test_first_"))
            );
            Ok(())
        })
    });
    let two = second.run_parallel_test(move |_| {
        Box::pin(async move {
            Cache::forever("shared", &"two".into()).await?;
            b.wait().await;
            assert_eq!(Cache::get("shared").await?, Some("two".into()));
            assert_eq!(
                Config::get("cache.prefix")?,
                Some(json!("app-second:test_second_"))
            );
            Ok(())
        })
    });
    let (one, two) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(one, two)
    })
    .await
    .unwrap();
    one.unwrap();
    two.unwrap();
    let mut third = configured();
    third.boot().await.unwrap();
    let app =
        App::set_facade_application(Some(third.app()), async { App::get_facade_root().unwrap() })
            .await;
    App::set_facade_application(Some(&app), async {
        ParallelTesting::swap(Arc::new(service("retained"))).unwrap();
        let previous = Cache::store(None).unwrap();
        Cache::forever("before", &"retained".into()).await.unwrap();
        ParallelTesting::call_set_up_test_case_callbacks(&mut third)
            .await
            .unwrap();
        assert_eq!(Cache::get("before").await.unwrap(), Some("retained".into()));
        Cache::purge(None).unwrap();
        assert_eq!(Cache::get("before").await.unwrap(), None);
        assert_eq!(
            previous.get("before").await.unwrap(),
            Some("retained".into())
        );
    })
    .await;
}

#[tokio::test]
async fn standard_prefix_and_compiled_configuration_match_fixed_provider_helpers() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/parallel-testing/isolation-contract.json"
    ))
    .unwrap();
    for row in cases.as_array().unwrap() {
        let mut case = TestApp::create(|b| b).unwrap();
        case.boot().await.unwrap();
        let app = App::set_facade_application(Some(case.app()), async {
            App::get_facade_root().unwrap()
        })
        .await;
        App::set_facade_application(Some(&app), async {
            if row["missing"] != true {
                Config::set("cache.prefix", row["prefix"].clone()).unwrap();
                Config::set("view.compiled", row["compiled"].clone()).unwrap();
            }
            ParallelTesting::swap(Arc::new(service(row["token"].as_str().unwrap()))).unwrap();
            // 这里只验证路径配置，不创建目录；根路径样本不会写入项目外。
            ParallelTesting::call_set_up_test_case_callbacks(&mut case)
                .await
                .unwrap();
            assert_eq!(
                Config::get("cache.prefix").unwrap(),
                Some(row["result"]["prefix"].clone())
            );
            assert_eq!(
                Config::get("view.compiled")
                    .unwrap()
                    .unwrap_or(serde_json::Value::Null),
                row["result"]["compiled"]
            );
            if row["compiled"].is_null() {
                Config::set("view.compiled", json!("changed/views")).unwrap();
            }
            let next = format!("{}-next", row["token"].as_str().unwrap());
            ParallelTesting::resolve_token_using(Some(Arc::new(move || Ok(json!(next.clone())))))
                .unwrap();
            ParallelTesting::call_set_up_test_case_callbacks(&mut case)
                .await
                .unwrap();
            assert_eq!(
                Config::get("cache.prefix").unwrap().unwrap(),
                row["repeat"]["prefix"]
            );
            assert_eq!(
                Config::get("view.compiled")
                    .unwrap()
                    .unwrap_or(serde_json::Value::Null),
                row["repeat"]["compiled"]
            );
        })
        .await;
    }
}

#[tokio::test]
async fn resolved_view_factory_moves_its_live_compiler_without_resolving_the_blade_alias() {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    let mut case = fixture().await;
    let application =
        App::set_facade_application(Some(case.app()), async { App::get_facade_root().unwrap() })
            .await;
    App::set_facade_application(Some(&application), async {
        ParallelTesting::swap(Arc::new(service("view-only"))).unwrap();
        assert_eq!(View::render("hello", &Default::default()).unwrap(), "hello");
        assert!(!case.app().container().resolved("blade.compiler").unwrap());
        let mut retained = View::make("hello", Default::default()).unwrap();
        ParallelTesting::call_set_up_process_callbacks()
            .await
            .unwrap();
        ParallelTesting::call_set_up_test_case_callbacks(&mut case)
            .await
            .unwrap();
        assert_eq!(
            openrust::contracts::view::View::render(&mut retained).unwrap(),
            "hello"
        );
        let path = case.root().join("compiled/base/test_view-only");
        assert!(std::fs::read_dir(&path).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|v| v == "json")
        }));
        assert!(!case.app().container().resolved("blade.compiler").unwrap());
        ParallelTesting::call_tear_down_process_callbacks()
            .await
            .unwrap();
        assert!(!path.exists());
        assert!(case.root().join("compiled/base").is_dir());
    })
    .await;
}
