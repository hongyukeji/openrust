use openrust::{
    foundation::vite::ViteError,
    support::facades::{App, Config, Facade, View, Vite},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::{fs, path::Path};
fn manifest(root: &Path, path: &str, value: &Value) {
    let p = root.join("public").join(path);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, value.to_string()).unwrap();
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/vite/fonts.json")).unwrap()
}
fn expected() -> Value {
    serde_json::from_str(include_str!("fixtures/vite/fonts-contract.json")).unwrap()
}
async fn app() -> TestApp {
    let mut app = TestApp::create(|builder| {
        builder.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"env":"testing", "url":"https://app.test"}),
            )]
            .into())
        })
    })
    .unwrap();
    manifest(app.root(), "build/fonts-manifest.json", &fixture());
    app.boot().await.unwrap();
    app
}

#[tokio::test]
async fn actual_font_tags_alias_order_callbacks_and_hot_paths_match_fixed_source() {
    let app = app().await;
    let expected = expected();
    App::set_facade_application(Some(app.app()),async {
  Vite::use_csp_nonce(Some("font-nonce")).unwrap();
  assert_eq!(Vite::fonts(None).unwrap().to_html(),expected["all"].as_str().unwrap());
  assert_eq!(serde_json::to_value(Vite::preloaded_assets().unwrap()).unwrap(),expected["loaded"]);
  assert_eq!(Vite::fonts(None).unwrap().to_html(),expected["twice"].as_str().unwrap());
  Vite::flush().unwrap();
  assert_eq!(Vite::fonts(Some(&["sans","mono"])).unwrap().to_html(),expected["aliases"].as_str().unwrap());
  assert_eq!(Vite::fonts(Some(&[])).unwrap().to_html(),expected["none"].as_str().unwrap());
  assert!(matches!(Vite::fonts(Some(&["missing"])),Err(ViteError::InvalidFonts(_))));
  Vite::use_preload_tag_attributes_using(|context|{
   assert_eq!(context.source,Some("fonts"));assert_eq!(context.chunk,Some(&json!([])));assert!(context.manifest.unwrap().is_empty());
   Vite::use_csp_nonce(Some("font-nonce")).unwrap();
   Some([("data-font".into(),true.into())].into())
  }).unwrap();
  Vite::flush().unwrap();assert_eq!(Vite::fonts(Some(&["mono"])).unwrap().to_html(),expected["custom"].as_str().unwrap());
  manifest(app.root(),"build/alternate.json",&json!({"version":1,"families":{},"style":{"inline":"/* alternate */"}}));
  Vite::use_fonts_manifest_filename("alternate.json").unwrap();assert_eq!(Vite::fonts(None).unwrap().to_html(),expected["alternate"].as_str().unwrap());
  manifest(app.root(),"dev/fonts-manifest.dev.json",&json!({"version":1,"families":{"mono":{}},"preloads":[{"alias":"mono","url":"https://fonts.test/font?a=1&b=2"}],"style":{"inline":"/* dev */"}}));
  fs::write(app.root().join("public/dev/hot"),"http://dev.test:5173").unwrap();Vite::use_hot_file(app.root().join("public/dev/hot")).unwrap();
  assert_eq!(Vite::fonts(None).unwrap().to_html(),expected["hot"].as_str().unwrap());
 }).await;
}
#[tokio::test]
async fn font_cache_flush_missing_files_and_custom_build_use_the_same_public_service() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Vite::use_build_directory("bundle").unwrap();
        Vite::use_fonts_manifest_filename("custom.json").unwrap();
        assert_eq!(Vite::fonts(None).unwrap().to_html(), "");
        manifest(
            app.root(),
            "bundle/custom.json",
            &json!({"version":1,"families":{},"style":{"file":"font.css"}}),
        );
        fs::write(
            app.root().join("public/bundle/font.css"),
            ".one{color:green}",
        )
        .unwrap();
        assert!(Vite::fonts(None).unwrap().to_html().contains(".one"));
        manifest(
            app.root(),
            "bundle/custom.json",
            &json!({"version":1,"families":{},"style":{"inline":".two{}"}}),
        );
        assert!(Vite::fonts(None).unwrap().to_html().contains(".one"));
        Vite::flush().unwrap();
        assert!(Vite::fonts(None).unwrap().to_html().contains(".two"));
        Vite::use_fonts_manifest_filename("invalid.json").unwrap();
        fs::write(app.root().join("public/bundle/invalid.json"), "{").unwrap();
        assert!(matches!(
            Vite::fonts(None),
            Err(ViteError::InvalidManifest { .. })
        ));
        manifest(
            app.root(),
            "bundle/invalid.json",
            &json!({"version":1,"families":[]}),
        );
        assert_eq!(Vite::fonts(None).unwrap().to_html(), "");
    })
    .await;
}
#[tokio::test]
async fn validation_filters_before_preloads_and_preserves_failures_without_false_success() {
    let app = app().await;
    App::set_facade_application(Some(app.app()),async {
  manifest(app.root(),"build/fonts-manifest.json",&json!({"version":1,"families":{"good":{},"bad":{}},"preloads":[{"alias":"good","file":"good.woff2"},{"alias":"bad"}],"style":{"familyStyles":[],"variables":[]}}));
  assert!(Vite::fonts(None).is_err());assert!(Vite::preloaded_assets().unwrap().is_empty());
  assert!(Vite::fonts(Some(&["good"])).unwrap().to_html().contains("good.woff2"));
  Vite::use_preload_tag_attributes(None).unwrap();Vite::flush().unwrap();assert_eq!(Vite::fonts(Some(&["good"])).unwrap().to_html(),"");assert!(Vite::preloaded_assets().unwrap().is_empty());
  manifest(app.root(),"build/fonts-manifest.json",&json!({"version":1,"families":{},"style":{"inline":"</STYLE><img>"}}));Vite::flush().unwrap();assert!(matches!(Vite::fonts(None),Err(ViteError::InvalidFonts(_))));
 }).await;
}
#[tokio::test]
async fn blade_and_facade_share_preload_deduplication_and_each_request_gets_its_own_fonts() {
    use openrust::{
        foundation::configuration::Routing,
        http::{HttpError, HttpFuture, Request, Response},
        support::facades::Route,
    };
    fn page(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            Vite::use_csp_nonce(Some("http-font")).unwrap();
            request
                .with_container(async {
                    openrust::support::facades::Response::view(
                        "fonts",
                        Default::default(),
                        http::StatusCode::OK,
                        Default::default(),
                    )
                    .await
                })
                .await
        })
    }
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"env":"testing","url":"https://app.test"}),
            )]
            .into())
        })
        .with_routing(Routing::default().web(|| Route::get("/fonts", page)))
    })
    .unwrap();
    manifest(app.root(), "build/fonts-manifest.json", &fixture());
    fs::create_dir_all(app.root().join("resources/views")).unwrap();
    fs::write(
        app.root().join("resources/views/fonts.blade.html"),
        "@fonts('mono')|{{ Vite::fonts(['mono','sans']) }}",
    )
    .unwrap();
    for host in ["one", "two"] {
        let response = app
            .call(
                http::Request::get(format!("https://{host}.test/fonts"))
                    .body(openrust::http::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        response.assert_ok();
        let html = std::str::from_utf8(response.body()).unwrap();
        assert_eq!(html.matches("mono.woff2").count(), 1);
        assert_eq!(html.matches("sans.woff2").count(), 1);
        assert!(html.contains(&format!("https://{host}.test/build/mono.woff2")));
        assert!(html.contains("nonce=\"http-font\""));
    }
    App::set_facade_application(Some(app.app()), async {
        assert!(Vite::preloaded_assets().unwrap().is_empty());
        Config::set("app.asset_url", "https://cdn.test".into()).unwrap();
        assert!(
            View::render_inline("@fonts('mono')", &Default::default())
                .unwrap()
                .contains("https://cdn.test/")
        );
    })
    .await;
}

#[tokio::test]
async fn null_filtered_style_values_and_shared_asset_links_follow_the_actual_contract() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        manifest(app.root(), "build/fonts-manifest.json", &json!({
            "version":1,"families":{"mono":null},"preloads":[{"alias":"mono","file":"mono.woff2"}],
            "style":{"familyStyles":{"mono":null},"variables":{"mono":null}}
        }));
        assert!(Vite::fonts(Some(&["mono"])).unwrap().to_html().contains("mono.woff2"));
        assert!(Vite::fonts(Some(&["mono"])).unwrap().to_html().is_empty());
        Vite::flush().unwrap();
        Vite::use_preload_tag_attributes_using_fallible(|_| {
            Err(ViteError::callback(std::io::Error::other("font callback")))
        }).unwrap();
        assert!(matches!(Vite::fonts(None), Err(ViteError::Callback(_))));
        assert!(Vite::preloaded_assets().unwrap().is_empty());
    }).await;
}

#[tokio::test]
async fn callback_configuration_changes_affect_the_following_font_urls_and_style_nonce() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Vite::use_csp_nonce(Some("before")).unwrap();
        Vite::use_preload_tag_attributes_using(|_| {
            Vite::use_build_directory("other").unwrap();
            Vite::use_csp_nonce(Some("after")).unwrap();
            Some(Default::default())
        })
        .unwrap();
        assert_eq!(
            Vite::fonts(None).unwrap().to_html(),
            expected()["dynamic"].as_str().unwrap()
        );
    })
    .await;
}
