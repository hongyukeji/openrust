use openrust::{
    foundation::vite::ViteError,
    support::facades::{App, Facade, Vite},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::{fs, sync::Arc};

fn expected() -> Value {
    serde_json::from_str(include_str!("fixtures/vite/contract.json")).unwrap()
}
async fn fixture() -> TestApp {
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"env":"testing","url":"https://app.test"}),
            )]
            .into())
        })
    })
    .unwrap();
    let build = app.root().join("public/build/assets");
    fs::create_dir_all(&build).unwrap();
    fs::write(
        build.parent().unwrap().join("manifest.json"),
        include_bytes!("fixtures/vite/manifest.json"),
    )
    .unwrap();
    fs::write(build.join("app.js"), "console.log('app');\n").unwrap();
    fs::write(build.join("app.css"), "body{color:green}\n").unwrap();
    app.boot().await.unwrap();
    app
}
const ENTRIES: [&str; 2] = ["resources/js/app.js", "resources/css/app.css"];

#[tokio::test]
async fn production_manifest_imports_css_integrity_and_preloads_match_the_fixed_source() {
    let app = fixture().await;
    let expected = expected();
    App::set_facade_application(Some(app.app()), async {
        let html = Vite::render(ENTRIES, None).unwrap();
        assert_eq!(html.to_html(), expected["plain"].as_str().unwrap());
        assert_eq!(
            serde_json::to_value(Vite::preloaded_assets().unwrap()).unwrap(),
            expected["preloaded"]
        );
        assert_eq!(
            Vite::content(ENTRIES[0], None).unwrap(),
            expected["content"].as_str().unwrap().as_bytes()
        );
        assert_eq!(
            Vite::manifest_hash(None).unwrap().as_deref(),
            expected["hash"].as_str()
        );
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            "https://app.test/build/assets/app.js"
        );
        assert!(!Vite::is_running_hot().unwrap());
        assert!(Vite::dev_server_url().unwrap().is_none());
        assert_eq!(Vite::hot_file().unwrap(), app.root().join("public/hot"));
        assert!(matches!(
            Vite::asset("missing", None),
            Err(ViteError::MissingChunk(_))
        ));
    })
    .await;
}

#[tokio::test]
async fn nonce_attribute_merges_html_entrypoints_flush_and_hot_mode_match_the_fixed_source() {
    let app = fixture().await;
    let expected = expected();
    App::set_facade_application(Some(app.app()), async {
        Vite::use_csp_nonce(Some("fixed-nonce")).unwrap();
        Vite::use_script_tag_attributes(
            [
                ("defer".into(), true.into()),
                ("omit".into(), false.into()),
                ("data-number".into(), 0.into()),
            ]
            .into(),
        )
        .unwrap();
        Vite::use_style_tag_attributes([("crossorigin".into(), true.into())].into()).unwrap();
        assert_eq!(
            Vite::render(ENTRIES, None).unwrap().to_html(),
            expected["custom"].as_str().unwrap()
        );
        assert_eq!(
            serde_json::to_value(Vite::preloaded_assets().unwrap()).unwrap(),
            expected["custom_preloaded"]
        );
        Vite::with_entry_points([ENTRIES[0]]).unwrap();
        Vite::merge_entry_points(ENTRIES).unwrap();
        assert_eq!(
            Vite::to_html().unwrap(),
            expected["to_html"].as_str().unwrap()
        );
        Vite::flush().unwrap();
        assert!(Vite::preloaded_assets().unwrap().is_empty());
        assert_eq!(
            Vite::csp_nonce().unwrap().as_deref(),
            expected["nonce"].as_str()
        );
        fs::write(app.root().join("public/hot"), "http://127.0.0.1:5173\n").unwrap();
        assert!(Vite::is_running_hot().unwrap());
        assert_eq!(
            Vite::render(ENTRIES, None).unwrap().to_html(),
            expected["dev"].as_str().unwrap()
        );
        assert_eq!(
            Vite::react_refresh().unwrap().unwrap().to_html(),
            expected["refresh"].as_str().unwrap()
        );
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            expected["dev_asset"].as_str().unwrap()
        );
        assert!(Vite::manifest_hash(None).unwrap().is_none());
        assert_eq!(
            Vite::content(ENTRIES[0], None).unwrap(),
            b"console.log('app');\n"
        );
        fs::remove_file(app.root().join("public/hot")).unwrap();
        assert!(Vite::react_refresh().unwrap().is_none());
    })
    .await;
}

#[tokio::test]
async fn custom_paths_resolvers_and_manifest_cache_preserve_the_actual_failure_boundaries() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        assert!(matches!(
            Vite::render(ENTRIES, Some("absent")),
            Err(ViteError::ManifestNotFound(_))
        ));
        assert!(Vite::manifest_hash(Some("absent")).unwrap().is_none());
        fs::create_dir_all(app.root().join("public/custom")).unwrap();
        fs::write(
            app.root().join("public/custom/custom.json"),
            include_bytes!("fixtures/vite/manifest.json"),
        )
        .unwrap();
        Vite::use_manifest_filename("custom.json").unwrap();
        Vite::use_build_directory("custom").unwrap();
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            "https://app.test/custom/assets/app.js"
        );
        let first_hash = Vite::manifest_hash(None).unwrap();
        fs::write(
            app.root().join("public/custom/custom.json"),
            r#"{"resources/js/app.js":{"file":"new.js"}}"#,
        )
        .unwrap();
        assert_ne!(Vite::manifest_hash(None).unwrap(), first_hash);
        Vite::flush().unwrap();
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            "https://app.test/custom/assets/app.js"
        );
        Vite::create_asset_paths_using(|path, secure| {
            assert_eq!(secure, None);
            assert!(Vite::csp_nonce().unwrap().is_none());
            Ok(format!("https://cdn.test/{path}"))
        })
        .unwrap();
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            "https://cdn.test/custom/assets/app.js"
        );
        Vite::clear_asset_path_resolver().unwrap();
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            "https://app.test/custom/assets/app.js"
        );
        let path = app.root().join("dev-server");
        fs::write(&path, "http://localhost:5173/ \n").unwrap();
        Vite::use_hot_file(&path).unwrap();
        assert_eq!(
            Vite::asset("entry.js", None).unwrap(),
            "http://localhost:5173//entry.js"
        );
        fs::remove_file(path).unwrap();
        Vite::use_manifest_filename("invalid.json").unwrap();
        fs::write(app.root().join("public/custom/invalid.json"), "{wrong").unwrap();
        assert!(matches!(
            Vite::asset("anything", None),
            Err(ViteError::InvalidManifest { .. })
        ));
    })
    .await;
}

#[tokio::test]
async fn callbacks_receive_real_metadata_can_reenter_and_may_disable_preloads() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        Vite::use_script_tag_attributes_using(|context| {
            assert!(context.manifest.is_some());
            assert!(context.chunk.is_some());
            assert!(Vite::csp_nonce().unwrap().is_none());
            [
                ("data-entry".into(), context.source.unwrap().into()),
                ("crossorigin".into(), "anonymous".into()),
            ]
            .into()
        })
        .unwrap();
        let html = Vite::render([ENTRIES[0]], None)
            .unwrap()
            .to_html()
            .to_owned();
        assert!(html.contains(r#"data-entry="resources/js/app.js""#));
        assert!(html.contains(r#"crossorigin="anonymous""#));
        Vite::use_preload_tag_attributes(None).unwrap();
        Vite::flush().unwrap();
        let html = Vite::render([ENTRIES[0]], None).unwrap();
        assert!(!html.to_html().contains("modulepreload"));
        assert!(!html.to_html().contains(r#"rel="preload""#));
        assert!(Vite::preloaded_assets().unwrap().is_empty());
        Vite::use_integrity_key(None).unwrap();
        assert!(
            !Vite::render([ENTRIES[0]], None)
                .unwrap()
                .to_html()
                .contains("integrity=")
        );
        Vite::use_script_tag_attributes([("bad".into(), json!([]))].into()).unwrap();
        assert!(matches!(
            Vite::render([ENTRIES[0]], None),
            Err(ViteError::InvalidAttribute(_))
        ));
    })
    .await;
}

#[tokio::test]
async fn service_configuration_and_caches_are_isolated_between_applications() {
    let first = fixture().await;
    let second = fixture().await;
    let service = App::set_facade_application(Some(first.app()), async {
        openrust::support::facades::Config::set("app.asset_url", "https://first.test".into())
            .unwrap();
        Vite::use_csp_nonce(Some("first")).unwrap();
        Vite::create_asset_paths_using(|path, _| Ok(format!("https://first.test/{path}"))).unwrap();
        Vite::get_facade_root().unwrap()
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        assert!(Vite::csp_nonce().unwrap().is_none());
        let generated = Vite::use_csp_nonce(None).unwrap();
        assert_eq!(generated.len(), 40);
        assert!(generated.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            "https://app.test/build/assets/app.js"
        );
        assert_eq!(
            service.asset(ENTRIES[0], None).unwrap(),
            "https://first.test/build/assets/app.js"
        );
        // 离开显式生命周期后，执行期nonce不成为应用默认配置。
        assert!(service.csp_nonce().unwrap().is_none());
        assert!(!Arc::ptr_eq(&service, &Vite::get_facade_root().unwrap()));
    })
    .await;
}

fn page(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        assert!(Vite::csp_nonce()?.is_none());
        assert!(Vite::preloaded_assets()?.is_empty());
        let nonce = request
            .header("x-nonce")?
            .unwrap_or("request-nonce")
            .to_owned();
        Vite::use_csp_nonce(Some(&nonce))?;
        if nonce == "slow" {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await
        } else {
            tokio::task::yield_now().await
        }
        request
            .with_container(async {
                openrust::support::facades::Response::view(
                    "vite",
                    Default::default(),
                    openrust::http::StatusCode::OK,
                    Default::default(),
                )
                .await
            })
            .await
    })
}
fn failure(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        Vite::render(["missing"], None)?;
        unreachable!()
    })
}
struct ViteCommand(openrust::console::CommandDefinition);
impl openrust::console::Command for ViteCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a openrust::foundation::Application,
        _: &'a openrust::console::Input,
        output: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            let result = (|| -> openrust::foundation::vite::ViteResult<String> {
                assert!(Vite::csp_nonce()?.is_none());
                assert!(Vite::preloaded_assets()?.is_empty());
                Vite::asset(ENTRIES[0], None)
            })()
            .map_err(|e| openrust::console::ConsoleError::new(e.to_string()))?;
            output.stdout.push_str(&result);
            Ok(0)
        })
    }
}
fn views(root: &std::path::Path) {
    fs::create_dir_all(root.join("resources/views")).unwrap();
    fs::write(
        root.join("resources/views/vite.blade.html"),
        r#"@VITE(['resources/js/app.js', 'resources/css/app.css'])
<img src="{{ Vite::asset('resources/js/app.js') }}">
<style>{!! Vite::content('resources/css/app.css') !!}</style>
<meta name="nonce" content="{{ Vite::cspNonce() }}">
<span>{{ Vite::manifestHash() }}</span>@viteReactRefresh"#,
    )
    .unwrap();
}
#[tokio::test]
async fn actual_http_artisan_and_blade_helpers_share_public_vite_but_keep_lifecycle_state_separate()
{
    use openrust::{foundation::configuration::Routing, support::facades::Route};
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"env":"testing","url":"https://app.test"}),
            )]
            .into())
        })
        .with_commands(|artisan| {
            artisan.add_command(Box::new(ViteCommand(
                openrust::console::CommandDefinition::new(clap::Command::new("vite:probe")),
            )))
        })
        .with_routing(Routing::default().web(|| {
            Route::get("/vite", page)?;
            Route::get("/vite-error", failure)
        }))
    })
    .unwrap();
    let build = app.root().join("public/build/assets");
    fs::create_dir_all(&build).unwrap();
    fs::write(
        build.parent().unwrap().join("manifest.json"),
        include_bytes!("fixtures/vite/manifest.json"),
    )
    .unwrap();
    fs::write(build.join("app.css"), "body{color:green}\n").unwrap();
    views(app.root());
    app.without_exception_handling(&[]).await.unwrap();
    let response = app.get("/vite").await.unwrap();
    response
        .assert_ok()
        .assert_see_html("nonce=\"request-nonce\"")
        .assert_see_html("body{color:green}")
        .assert_see_html("<script type=\"module\"")
        .assert_see_html("8021de00f215402ef79e0c753b42f3bc");
    app.get("/vite").await.unwrap().assert_ok();
    app.call(
        http::Request::get("https://edge.test/vite")
            .body(openrust::http::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_ok()
    .assert_see_html("https://edge.test/build/assets/app.js");
    app.artisan(["vite:probe"])
        .await
        .assert_successful()
        .expects_output("https://app.test/build/assets/app.js");
    app.with_exception_handling().await.unwrap();
    app.call(
        http::Request::get("/vite-error")
            .header("Accept", "application/json")
            .body(openrust::http::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
    App::set_facade_application(Some(app.app()), async {
        use openrust::support::facades::{Blade, View};
        let directives = Blade::get_custom_directives().unwrap();
        assert!(!directives.contains_key("vite"));
        assert!(!directives.contains_key("viteReactRefresh"));
        Blade::directive("vite", |_| {
            Ok(openrust::view::compilers::DirectiveOutput::Html(
                "custom override".into(),
            ))
        })
        .unwrap();
        let html = View::render_inline("@vite('ignored')", &Default::default()).unwrap();
        assert_eq!(html, "custom override");
    })
    .await;
}

#[tokio::test]
async fn dynamic_resolver_call_order_and_nonce_changes_match_the_fixed_source() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let app = fixture().await;
    let calls = Arc::new(AtomicUsize::new(0));
    App::set_facade_application(Some(app.app()), async {
        let seen = calls.clone();
        Vite::create_asset_paths_using(move |path, _| {
            Ok(format!(
                "https://dynamic.test/{}/{path}",
                seen.fetch_add(1, Ordering::SeqCst) + 1
            ))
        })
        .unwrap();
        Vite::use_script_tag_attributes_using(|_| {
            Vite::use_csp_nonce(Some("changed")).unwrap();
            openrust::foundation::vite::Attributes::new()
        })
        .unwrap();
        assert_eq!(
            Vite::render(ENTRIES, None).unwrap().to_html(),
            expected()["dynamic"].as_str().unwrap()
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            expected()["dynamic_calls"].as_u64().unwrap() as usize
        );
    })
    .await;
}
#[derive(Debug)]
struct CallbackFailure(u32);
impl std::fmt::Display for CallbackFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "callback {}", self.0)
    }
}
impl std::error::Error for CallbackFailure {}
#[tokio::test]
async fn fallible_attribute_callbacks_keep_the_actual_error_instead_of_panicking_or_succeeding() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        Vite::use_script_tag_attributes_using_fallible(
            |_| -> openrust::foundation::vite::ViteResult<openrust::foundation::vite::Attributes> {
                Err(ViteError::callback(CallbackFailure(42)))
            },
        )
        .unwrap();
        match Vite::render(ENTRIES, None) {
            Err(ViteError::Callback(error)) => {
                assert_eq!(error.downcast_ref::<CallbackFailure>().unwrap().0, 42)
            }
            _ => panic!("必须保留回调错误"),
        }
        assert!(Vite::preloaded_assets().unwrap().is_empty());
    })
    .await;
}

fn write_build(root: &std::path::Path) {
    let assets = root.join("public/build/assets");
    fs::create_dir_all(&assets).unwrap();
    fs::write(
        assets.parent().unwrap().join("manifest.json"),
        include_bytes!("fixtures/vite/manifest.json"),
    )
    .unwrap();
    fs::write(assets.join("app.css"), "body{color:green}\n").unwrap();
    views(root);
}
#[tokio::test]
async fn concurrent_real_http_requests_do_not_share_nonces_attributes_or_preloaded_assets() {
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{Body, Request},
        support::facades::Route,
    };
    let root = tempfile::tempdir().unwrap();
    write_build(root.path());
    let app = Application::configure(root.path().to_path_buf())
        .with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"env":"testing","url":"https://app.test"}),
            )]
            .into())
        })
        .with_routing(Routing::default().web(|| Route::get("/vite", page)))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let incoming = |nonce: &str| {
        let (parts, _) = http::Request::get(format!("https://{nonce}.test/vite"))
            .header("x-nonce", nonce)
            .body(())
            .unwrap()
            .into_parts();
        Request::from_parts(parts, Body::empty())
    };
    let mut first = incoming("slow");
    let mut second = incoming("fast");
    let (first_response, second_response) =
        tokio::join!(kernel.handle(&mut first), kernel.handle(&mut second));
    let (first_parts, first_body) = first_response.into_parts();
    let (second_parts, second_body) = second_response.into_parts();
    assert_eq!(first_parts.status, http::StatusCode::OK);
    assert_eq!(second_parts.status, http::StatusCode::OK);
    let a = String::from_utf8(
        axum::body::to_bytes(first_body, 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    let b = String::from_utf8(
        axum::body::to_bytes(second_body, 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(a.contains(r#"nonce="slow""#));
    assert!(!a.contains(r#"nonce="fast""#));
    assert!(b.contains(r#"nonce="fast""#));
    assert!(!b.contains(r#"nonce="slow""#));
    assert!(a.contains("https://slow.test/build/assets/app.js"));
    assert!(!a.contains("https://fast.test/"));
    assert!(b.contains("https://fast.test/build/assets/app.js"));
    assert!(!b.contains("https://slow.test/"));
    kernel.terminate(first.parts(), &first_parts).await;
    kernel.terminate(second.parts(), &second_parts).await;
}

struct BootPreset;
impl openrust::support::ServiceProvider for BootPreset {
    fn boot<'a>(
        &'a mut self,
        _: &'a openrust::foundation::Application,
    ) -> openrust::http::HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async {
            Vite::use_csp_nonce(Some("boot-default"))?;
            Vite::use_build_directory("custom")?;
            Vite::with_entry_points([ENTRIES[0]])?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn provider_defaults_are_inherited_by_scopes_but_scope_overrides_do_not_change_defaults() {
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({"env":"testing","url":"https://app.test"}),
            )]
            .into())
        })
        .with_providers(vec![Box::new(BootPreset)])
    })
    .unwrap();
    let build = app.root().join("public/custom");
    fs::create_dir_all(&build).unwrap();
    fs::write(
        build.join("manifest.json"),
        include_bytes!("fixtures/vite/manifest.json"),
    )
    .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(Vite::csp_nonce().unwrap().as_deref(), Some("boot-default"));
        assert!(
            Vite::to_html()
                .unwrap()
                .contains("https://app.test/custom/assets/app.js")
        );
        Vite::use_csp_nonce(Some("scope-override")).unwrap();
        Vite::use_build_directory("build").unwrap();
        assert_eq!(
            Vite::csp_nonce().unwrap().as_deref(),
            Some("scope-override")
        );
    })
    .await;
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(Vite::csp_nonce().unwrap().as_deref(), Some("boot-default"));
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            "https://app.test/custom/assets/app.js"
        );
        assert!(Vite::preloaded_assets().unwrap().is_empty());
    })
    .await;
}

fn browser_page(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        request
            .with_container(async {
                openrust::support::facades::Response::view(
                    "browser",
                    Default::default(),
                    http::StatusCode::OK,
                    Default::default(),
                )
                .await
            })
            .await
    })
}
#[tokio::test]
async fn browser_acceptance_writes_the_page_from_the_actual_http_kernel() {
    let Ok(output) = std::env::var("OPENRUST_VITE_HTML") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_VITE_BROWSER").is_none(),
            "缺少Vite浏览器夹具"
        );
        return;
    };
    use openrust::{foundation::configuration::Routing, support::facades::Route};
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| {
            Ok([(
                "app".into(),
                json!({
                    "env":"testing",
                    "url":std::env::var("OPENRUST_VITE_ORIGIN").unwrap(),
                    "public_path":std::env::var("OPENRUST_VITE_PUBLIC").unwrap(),
                }),
            )]
            .into())
        })
        .with_routing(Routing::default().web(|| Route::get("/browser", browser_page)))
    })
    .unwrap();
    fs::create_dir_all(app.root().join("resources/views")).unwrap();
    fs::write(app.root().join("resources/views/browser.blade.html"),r#"<!doctype html><html><head>@vite('src/main.js')</head><body><div id="message"></div></body></html>"#).unwrap();
    let response = app.get("/browser").await.unwrap();
    response.assert_ok().assert_see_html("<script");
    fs::write(output, response.body()).unwrap();
}

#[tokio::test]
async fn empty_build_directory_uses_the_public_root_and_hot_hash_does_not_decode_the_hot_file() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        fs::write(
            app.root().join("public/manifest.json"),
            include_bytes!("fixtures/vite/manifest.json"),
        )
        .unwrap();
        fs::create_dir_all(app.root().join("public/assets")).unwrap();
        fs::write(app.root().join("public/assets/app.js"), b"root-asset").unwrap();
        Vite::use_build_directory("").unwrap();
        assert_eq!(
            Vite::asset(ENTRIES[0], None).unwrap(),
            "https://app.test/assets/app.js"
        );
        assert_eq!(Vite::content(ENTRIES[0], None).unwrap(), b"root-asset");
        fs::write(app.root().join("public/hot"), [0xff]).unwrap();
        assert!(Vite::is_running_hot().unwrap());
        assert!(Vite::manifest_hash(None).unwrap().is_none());
        assert!(matches!(Vite::dev_server_url(), Err(ViteError::Io { .. })));
        assert_eq!(Vite::content(ENTRIES[0], None).unwrap(), b"root-asset");
    })
    .await;
}
