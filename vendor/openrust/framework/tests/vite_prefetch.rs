use openrust::{
    foundation::vite::ViteError,
    support::facades::{App, Facade, Vite},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::fs;
fn expected() -> Value {
    serde_json::from_str(include_str!("fixtures/vite/prefetch-contract.json")).unwrap()
}
async fn app() -> TestApp {
    let mut app = TestApp::create(|builder| {
        builder.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"env":"testing","url":"https://app.test"}),
            )]
            .into())
        })
    })
    .unwrap();
    fs::create_dir_all(app.root().join("public/build")).unwrap();
    fs::write(
        app.root().join("public/build/manifest.json"),
        include_bytes!("fixtures/vite/prefetch-manifest.json"),
    )
    .unwrap();
    app.boot().await.unwrap();
    app
}
fn render() -> Result<String, ViteError> {
    Vite::render(["resources/js/app.js"], None).map(|html| html.to_html().to_owned())
}
#[tokio::test]
async fn waterfall_aggressive_event_switches_escaping_and_cycle_order_match_fixed_source() {
    let app = app().await;
    let expected = expected();
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(render().unwrap(), expected["plain"].as_str().unwrap());
        Vite::use_csp_nonce(Some("prefetch-nonce")).unwrap();
        Vite::use_waterfall_prefetching(None).unwrap();
        assert_eq!(render().unwrap(), expected["waterfall"].as_str().unwrap());
        Vite::use_aggressive_prefetching().unwrap();
        assert_eq!(render().unwrap(), expected["aggressive"].as_str().unwrap());
        Vite::prefetch(Some(2), Some("vite:ready")).unwrap();
        assert_eq!(render().unwrap(), expected["customEvent"].as_str().unwrap());
        Vite::use_prefetch_strategy(None, None).unwrap();
        assert_eq!(render().unwrap(), expected["disabled"].as_str().unwrap());
        Vite::use_waterfall_prefetching(None).unwrap();
        assert_eq!(
            render().unwrap(),
            expected["retainedConcurrency"].as_str().unwrap()
        );
        Vite::use_preload_tag_attributes(Some(
            [("data-test".into(), "O'Reilly & </script> 字".into())].into(),
        ))
        .unwrap();
        assert_eq!(render().unwrap(), expected["escaped"].as_str().unwrap());
        Vite::use_prefetch_strategy(Some("invalid"), None).unwrap();
        assert!(matches!(
            render(),
            Err(ViteError::InvalidPrefetchStrategy(_))
        ));
        fs::write(app.root().join("public/hot"), "http://dev.test:5173").unwrap();
        assert_eq!(render().unwrap(), expected["hot"].as_str().unwrap());
    })
    .await;
}
#[tokio::test]
async fn disabled_attributes_fail_explicitly_and_new_scopes_do_not_inherit_prefetch_settings() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Vite::prefetch(None, None).unwrap();
        Vite::use_preload_tag_attributes(None).unwrap();
        assert!(matches!(
            render(),
            Err(ViteError::DisabledPrefetchAttributes)
        ));
    })
    .await;
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(render().unwrap(), expected()["plain"].as_str().unwrap());
    })
    .await;
}
#[tokio::test]
async fn empty_imports_and_nonpositive_concurrency_keep_the_exact_requested_strategy() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        for count in [0, -1] {
            Vite::prefetch(Some(count), None).unwrap();
            let html = render().unwrap();
            assert!(html.contains(&format!(", {count})")));
        }
        Vite::prefetch(None, None).unwrap();
        assert!(render().unwrap().contains(".forEach((asset)"));
        Vite::use_manifest_filename("empty.json").unwrap();
        fs::write(
            app.root().join("public/build/empty.json"),
            r#"{"resources/js/app.js":{"file":"app.js"}}"#,
        )
        .unwrap();
        assert!(render().unwrap().contains("[].forEach"));
    })
    .await;
}

#[tokio::test]
async fn browser_prefetch_page_is_generated_by_the_real_http_kernel() {
    let Ok(output) = std::env::var("OPENRUST_PREFETCH_HTML") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_PREFETCH_BROWSER").is_none(),
            "缺少预取浏览器夹具"
        );
        return;
    };
    use openrust::{
        foundation::configuration::Routing,
        http::{HttpError, HttpFuture, Request, Response},
        support::facades::Route,
    };
    fn page(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            Vite::use_csp_nonce(Some("browser-nonce"))?;
            Vite::prefetch(
                if std::env::var("OPENRUST_PREFETCH_MODE").unwrap() == "waterfall" {
                    Some(2)
                } else {
                    None
                },
                Some("vite:ready"),
            )?;
            request
                .with_container(async {
                    openrust::support::facades::Response::view(
                        "prefetch",
                        Default::default(),
                        http::StatusCode::OK,
                        Default::default(),
                    )
                    .await
                })
                .await
        })
    }
    let mut app=TestApp::create(|builder| {
        builder.with_config(|_,_| {
            Ok([("app".into(),json!({"env":"testing","url":std::env::var("OPENRUST_PREFETCH_ORIGIN").unwrap(),"public_path":std::env::var("OPENRUST_PREFETCH_PUBLIC").unwrap()}))].into())
        }).with_routing(Routing::default().web(||Route::get("/prefetch",page)))
    }).unwrap();
    fs::create_dir_all(app.root().join("resources/views")).unwrap();
    fs::write(app.root().join("resources/views/prefetch.blade.html"),"<!doctype html><html><head>@vite('resources/js/app.js')</head><body>prefetch</body></html>").unwrap();
    let response = app.get("/prefetch").await.unwrap();
    response
        .assert_ok()
        .assert_see_html("window.addEventListener('vite:ready'");
    fs::write(output, response.body()).unwrap();
}
