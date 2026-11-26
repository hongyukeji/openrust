use openrust::{
    foundation::vite::{ViteError, ViteMacro},
    support::facades::{App, Facade, View, Vite},
    testing::TestApp,
};
use serde_json::json;
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
async fn app(url: &str) -> TestApp {
    let mut app = TestApp::create(|builder| {
        builder.with_config(|root, _| {
            Ok([(
                "app".into(),
                serde_json::from_slice(&fs::read(root.join("macro-app.json")).unwrap()).unwrap(),
            )]
            .into())
        })
    })
    .unwrap();
    fs::write(
        app.root().join("macro-app.json"),
        json!({"env":"testing","url":url}).to_string(),
    )
    .unwrap();
    fs::create_dir_all(app.root().join("public/build")).unwrap();
    fs::write(
        app.root().join("public/build/manifest.json"),
        include_bytes!("fixtures/vite/manifest.json"),
    )
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn macro_registration_mixin_replacement_factory_order_and_missing_errors_follow_fixed_trait()
{
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/vite/macros-contract.json")).unwrap();
    let app = app("https://app.test").await;
    App::set_facade_application(Some(app.app()), async {
        Vite::r#macro("marker", |vite, suffix: String| {
            vite.use_csp_nonce(Some(&format!("macro-{suffix}")))
        })
        .unwrap();
        assert!(Vite::has_macro("marker").unwrap());
        assert!(!Vite::has_macro("Marker").unwrap());
        assert_eq!(
            Vite::macro_call::<String, _>("marker", "one".to_owned()).unwrap(),
            expected["value"].as_str().unwrap()
        );
        assert_eq!(Vite::csp_nonce().unwrap().as_deref(), Some("macro-one"));
        let calls = AtomicUsize::new(0);
        Vite::mixin_using(
            [("marker", || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ViteMacro::new(|_, (): ()| Ok("mixed".to_owned())))
            })],
            false,
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            Vite::macro_call::<String, _>("marker", "two".to_owned()).unwrap(),
            "macro-two"
        );
        Vite::mixin(
            [("added", ViteMacro::new(|_, (): ()| Ok("added".to_owned())))],
            false,
        )
        .unwrap();
        assert_eq!(Vite::macro_call::<String, _>("added", ()).unwrap(), "added");
        Vite::mixin_using(
            [("marker", || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ViteMacro::new(|_, (): ()| Ok("mixed".to_owned())))
            })],
            true,
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            Vite::macro_call::<String, _>("marker", ()).unwrap(),
            "mixed"
        );
        Vite::flush().unwrap();
        assert!(Vite::has_macro("marker").unwrap());
        Vite::flush_macros().unwrap();
        assert!(!Vite::has_macro("marker").unwrap());
        assert!(matches!(
            Vite::macro_call::<String, _>("marker", ()),
            Err(ViteError::UnknownMacro(_))
        ));
    })
    .await;
}
#[derive(Debug)]
struct DomainError(u32);
impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for DomainError {}
#[tokio::test]
async fn macros_keep_nonclone_native_values_exact_types_and_the_original_callback_error() {
    struct Owned(String);
    let app = app("https://app.test").await;
    App::set_facade_application(Some(app.app()),async {
  Vite::r#macro("owned",|_,value:Owned|Ok(value)).unwrap();let result=Vite::macro_call::<Owned,_>("owned",Owned("native".into())).unwrap();assert_eq!(result.0,"native");
  let input=Vite::macro_call::<Owned,_>("owned",()).err().unwrap();assert!(matches!(input,ViteError::MacroType{expected,actual,..} if expected.ends_with("Owned") && actual=="()"));
  let output=Vite::macro_call::<u32,_>("owned",Owned("actual".into())).unwrap_err();assert!(matches!(output,ViteError::MacroType{expected:"u32",actual,..} if actual.ends_with("Owned")));
  let error=Arc::new(DomainError(42));let source=error.clone();Vite::r#macro("fail",move|_,():()|->Result<(),ViteError>{Err(ViteError::callback(source.clone()))}).unwrap();
  match Vite::macro_call::<(),_>("fail",()){Err(ViteError::Callback(actual))=>assert!(Arc::ptr_eq(actual.downcast_ref::<Arc<DomainError>>().unwrap(),&error)),_=>panic!("必须返回原始错误")}
 }).await;
}
#[tokio::test]
async fn macros_can_reenter_replace_and_drop_captures_without_holding_the_registry_lock() {
    struct Reenter(std::sync::Weak<openrust::foundation::Vite>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            if let Some(vite) = self.0.upgrade() {
                vite.r#macro("after-drop", |_, (): ()| Ok(7u32))
            }
        }
    }
    let app = app("https://app.test").await;
    App::set_facade_application(Some(app.app()), async {
        Vite::r#macro("outer", |vite, (): ()| {
            vite.r#macro("inner", |_, (): ()| Ok(9u32));
            vite.macro_call::<u32, _>("inner", ())
        })
        .unwrap();
        assert_eq!(Vite::macro_call::<u32, _>("outer", ()).unwrap(), 9);
        let root = Vite::get_facade_root().unwrap();
        let capture = Reenter(Arc::downgrade(&root));
        root.r#macro("dropper", move |_, (): ()| {
            let _ = &capture;
            Ok(())
        });
        root.r#macro("dropper", |_, (): ()| Ok(()));
        assert_eq!(root.macro_call::<u32, _>("after-drop", ()).unwrap(), 7);
        root.r#macro("asset", |_, (): ()| Ok(1u32));
        assert_eq!(
            Vite::asset("resources/js/app.js", None).unwrap(),
            "https://app.test/build/assets/app.js"
        );
    })
    .await;
}
#[tokio::test]
async fn macro_registry_is_application_owned_but_uses_the_current_execution_settings() {
    let first = app("https://first.test").await;
    let second = app("https://second.test").await;
    let held = App::set_facade_application(Some(first.app()), async {
        Vite::r#macro("url", |vite, (): ()| {
            vite.asset("resources/js/app.js", None)
        })
        .unwrap();
        Vite::r#macro("nonce", |vite, (): ()| vite.csp_nonce()).unwrap();
        Vite::use_csp_nonce(Some("registration")).unwrap();
        Vite::get_facade_root().unwrap()
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        assert!(!Vite::has_macro("url").unwrap());
        assert_eq!(
            held.macro_call::<String, _>("url", ()).unwrap(),
            "https://first.test/build/assets/app.js"
        );
    })
    .await;
    App::set_facade_application(Some(first.app()), async {
        assert!(Vite::has_macro("url").unwrap());
        assert_eq!(
            Vite::macro_call::<Option<String>, _>("nonce", ()).unwrap(),
            None
        );
    })
    .await;
}
#[tokio::test]
async fn actual_http_uses_the_registered_macro_with_request_specific_nonce() {
    use openrust::{
        foundation::configuration::Routing,
        http::{HttpError, HttpFuture, Request, Response},
        support::facades::Route,
    };
    fn page(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let nonce = request.header("x-nonce")?.unwrap_or("http").to_owned();
            let html = Vite::macro_call::<String, _>("scripts", nonce)?;
            openrust::support::facades::Response::make(
                html,
                http::StatusCode::OK,
                Default::default(),
            )
        })
    }
    fn page_view(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            request
                .with_container(async {
                    openrust::support::facades::Response::view(
                        "macro",
                        Default::default(),
                        http::StatusCode::OK,
                        Default::default(),
                    )
                    .await
                })
                .await
        })
    }
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                Ok([(
                    "app".into(),
                    json!({"env":"testing","url":"https://app.test"}),
                )]
                .into())
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/macro", page)?;
                Route::get("/macro-view", page_view)?;
                Ok(())
            }))
    })
    .unwrap();
    fs::create_dir_all(app.root().join("public/build")).unwrap();
    fs::write(
        app.root().join("public/build/manifest.json"),
        include_bytes!("fixtures/vite/manifest.json"),
    )
    .unwrap();
    app.boot().await.unwrap();
    fs::create_dir_all(app.root().join("resources/views")).unwrap();
    fs::write(
        app.root().join("resources/views/macro.blade.html"),
        "{{ Vite::view_asset('resources/js/app.js') }}",
    )
    .unwrap();
    App::set_facade_application(Some(app.app()), async {
        Vite::r#macro(
            "view_asset",
            |vite, arguments: Vec<openrust::view::ViewValue>| {
                Ok(openrust::view::ViewValue::String(
                    vite.asset(arguments[0].as_str().unwrap(), None)?,
                ))
            },
        )
        .unwrap();
        Vite::r#macro("scripts", |vite, nonce: String| {
            assert!(vite.csp_nonce()?.is_none());
            vite.use_csp_nonce(Some(&nonce))?;
            Ok(vite
                .render(["resources/js/app.js"], None)?
                .to_html()
                .to_owned())
        })
        .unwrap();
    })
    .await;
    for nonce in ["one", "two"] {
        app.call(
            http::Request::get("/macro")
                .header("x-nonce", nonce)
                .body(openrust::http::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .assert_ok()
        .assert_see_html(format!("nonce=\"{nonce}\""));
    }
    app.call(
        http::Request::get("https://edge.test/macro-view")
            .body(openrust::http::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_ok()
    .assert_see_html("https://edge.test/build/assets/app.js");
}

#[tokio::test]
async fn blade_custom_macros_preserve_typed_values_escaping_and_runtime_registration() {
    use openrust::view::ViewValue;
    let app = app("https://app.test").await;
    App::set_facade_application(Some(app.app()), async {
        let source = "{{ Vite::markup('one') }}|{!! Vite::markup('two') !!}";
        // 尚未注册会返回真实缺失错误，编译产物仍可在稍后注册后复用。
        assert!(View::render_inline(source, &Default::default()).is_err());
        Vite::r#macro("markup", |_, arguments: Vec<ViewValue>| {
            Ok(ViewValue::String(format!(
                "<b>{}</b>",
                arguments[0].as_str().unwrap()
            )))
        })
        .unwrap();
        assert_eq!(
            View::render_inline(source, &Default::default()).unwrap(),
            "&lt;b&gt;one&lt;/b&gt;|<b>two</b>"
        );
        Vite::r#macro("native_only", |_, value: String| Ok(value)).unwrap();
        assert!(
            View::render_inline("{{ Vite::native_only('value') }}", &Default::default()).is_err()
        );
    })
    .await;
}

#[tokio::test]
async fn mixin_factory_failure_retains_prior_registrations_without_publishing_failed_or_later_entries()
 {
    type Factory = Box<dyn FnOnce() -> Result<ViteMacro, ViteError>>;
    let app = app("https://app.test").await;
    App::set_facade_application(Some(app.app()), async {
        let factories: Vec<(&str, Factory)> = vec![
            (
                "first",
                Box::new(|| Ok(ViteMacro::new(|_, (): ()| Ok(1u32)))),
            ),
            (
                "failed",
                Box::new(|| Err(ViteError::callback(DomainError(42)))),
            ),
            (
                "later",
                Box::new(|| Ok(ViteMacro::new(|_, (): ()| Ok(3u32)))),
            ),
        ];
        assert!(matches!(
            Vite::mixin_using(factories, true),
            Err(ViteError::Callback(_))
        ));
        assert_eq!(Vite::macro_call::<u32, _>("first", ()).unwrap(), 1);
        assert!(!Vite::has_macro("failed").unwrap());
        assert!(!Vite::has_macro("later").unwrap());
    })
    .await;
}
