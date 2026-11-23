use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use openrust::{
    contracts::{
        events::{Event as EventContract, Listener},
        foundation::Application as _,
        http::Kernel as _,
    },
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{
        Application,
        configuration::{Middleware, Routing},
        events::LocaleUpdated,
        http::Kernel,
    },
    http::{
        Body, HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode,
        middleware::SetLocale,
    },
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Config, Event, Lang, Route},
    },
    translation::TranslationError,
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "set-locale-{}-{}",
            std::process::id(),
            openrust::support::Str::random(12)
        ));
        for (locale, text) in [
            ("en", "Hello"),
            ("de", "Hallo"),
            ("fr", "Bonjour"),
            ("zh-Hans", "你好"),
            ("zh-Hant", "您好"),
        ] {
            fs::create_dir_all(path.join("lang").join(locale)).unwrap();
            fs::write(
                path.join("lang").join(locale).join("messages.json"),
                serde_json::json!({"hello":text}).to_string(),
            )
            .unwrap();
        }
        Self(path)
    }

    async fn kernel(
        &self,
        default: &str,
        configure: impl FnOnce(&mut Middleware) + Send + Sync + 'static,
        events: Arc<Events>,
    ) -> Kernel {
        self.kernel_query(default, configure, events, None).await
    }

    async fn kernel_query(
        &self,
        default: &str,
        configure: impl FnOnce(&mut Middleware) + Send + Sync + 'static,
        events: Arc<Events>,
        parameter: Option<serde_json::Value>,
    ) -> Kernel {
        fs::write(self.0.join(".env"), format!("APP_LOCALE={default}\n")).unwrap();
        let parameter_path = self.0.join("locale-query.json");
        if let Some(value) = parameter {
            fs::write(&parameter_path, serde_json::to_vec(&value).unwrap()).unwrap();
        } else {
            let _ = fs::remove_file(&parameter_path);
        }
        let app = Application::configure(self.0.clone())
            .with_config(|base, env| {
                let mut app = serde_json::json!({"locale":env.get_utf8("APP_LOCALE")?.unwrap_or("en"), "fallback_locale":"en"});
                if let Ok(bytes) = fs::read(base.join("locale-query.json")) { app["locale_query_parameter"] = serde_json::from_slice(&bytes).unwrap(); }
                if let Ok(bytes) = fs::read(base.join("locale-header.json")) { app["locale_header"] = serde_json::from_slice(&bytes).unwrap(); }
                Ok(BTreeMap::from([("app".into(), app)]))
            })
            .with_providers(vec![Box::new(Observer(events))])
            .with_middleware(configure)
            .with_routing(Routing::default().web(|| {
                Route::get("/", inspect)?;
                Route::get("/switch", switch)?;
                Route::get("/boundary", language_boundary)?;
                Route::get("/boundary-switch", language_boundary)?;
                Route::get("/metadata", || {
                    let mut response = Response::from("metadata");
                    response.parts_mut().headers.insert("content-language", "it".parse().unwrap());
                    response.parts_mut().headers.append("vary", "Accept-Encoding".parse().unwrap());
                    response.parts_mut().headers.append("vary", "Origin".parse().unwrap());
                    response
                })?;
                Route::get("/star", || Response::from("star").header("vary".parse().unwrap(), "*".parse().unwrap()))?;
                Route::get("/existing", || Response::from("existing").header("vary".parse().unwrap(), "Origin, accept-language".parse().unwrap()))?;
                Route::get("/existing-custom", || Response::from("existing").header("vary".parse().unwrap(), "Origin, X-LANGUAGE".parse().unwrap()))?;
                Route::get("/failure", failure)?;
                Route::get("/replace-config", || {
                    Config::set("app", serde_json::json!({})).unwrap();
                    Response::from("replaced")
                })?;
                Route::group(openrust::routing::RouteGroup::new().middleware(Arc::new(SetLocale::new(&["fr"]).unwrap())), |router| { router.get("/route", inspect)?; Ok(()) })?;
                Ok(())
            }))
            .create().unwrap();
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        kernel
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<String>>);
impl Listener for Events {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventContract,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let locale = &event.downcast_ref::<LocaleUpdated>().unwrap().locale;
            assert_eq!(App::current_locale().unwrap(), *locale);
            assert_eq!(Lang::get_locale().unwrap(), *locale);
            assert_eq!(Config::string("app.locale").unwrap(), *locale);
            self.0.lock().unwrap().push(locale.clone());
            if locale == "de-DE" {
                return Err(openrust::events::EventError::Listener {
                    source: Box::new(std::io::Error::other("语言监听器失败")),
                });
            }
            Ok(None)
        })
    }
}
struct Observer(Arc<Events>);
impl ServiceProvider for Observer {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            Event::listen(LocaleUpdated::NAME, self.0.clone(), Delivery::Sync)?;
            Ok(())
        })
    }
}

fn inspect(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Response::from(format!(
            "{}|{}",
            App::current_locale().unwrap(),
            Lang::get("messages.hello", &BTreeMap::new(), None).unwrap()
        )))
    })
}
fn switch(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        App::set_locale("fr").await.unwrap();
        Ok(Response::from("switched"))
    })
}
fn failure(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: Box::new(std::io::Error::other("处理失败")),
        })
    })
}
async fn send(kernel: &Kernel, path: &str, language: &str) -> (StatusCode, HeaderMap, String) {
    let (parts, body) = http::Request::builder()
        .uri(path)
        .header("host", "localhost")
        .header("accept-language", language)
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 128).await.unwrap();
    (
        parts.status,
        parts.headers,
        String::from_utf8(body.to_vec()).unwrap(),
    )
}
fn browser(middleware: &mut Middleware) {
    middleware.set_locale(&["de", "fr", "en"]).unwrap();
}

#[tokio::test]
async fn default_stack_detects_languages_without_application_registration() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root.kernel("en", |_| {}, events.clone()).await;
    for (header, expected) in [
        ("zh-CN", "zh-Hans|你好"),
        ("zh-TW", "zh-Hant|您好"),
        ("de", "de|Hallo"),
        ("zh-TW-1901", "zh-Hant|您好"),
    ] {
        let (status, headers, body) = send(&kernel, "/", header).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, expected);
        assert_eq!(
            headers["content-language"],
            expected.split('|').next().unwrap()
        );
        assert_eq!(headers["vary"], "Accept-Language");
    }
    assert_eq!(
        *events.0.lock().unwrap(),
        ["zh-Hans", "zh-Hant", "de", "zh-Hant"]
    );
}

#[tokio::test]
async fn minimal_applications_without_locale_configuration_still_render() {
    let root = Root::new();
    let app = Application::configure(root.0.clone())
        .with_routing(Routing::default().web(|| Route::get("/", inspect)))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    for (header, expected) in [("", "en|Hello"), ("zh-CN", "zh-Hans|你好")] {
        let (status, _, body) = send(&kernel, "/", header).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, expected);
    }
    assert_eq!(
        kernel.get_application().config().unwrap().get("app.locale"),
        None
    );
}

#[tokio::test]
async fn disabling_the_default_keeps_configured_locale_and_allows_route_specific_use() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root
        .kernel(
            "de",
            |middleware| {
                middleware.set_locale(&[]).unwrap();
            },
            events.clone(),
        )
        .await;
    let (status, headers, body) = send(&kernel, "/", "en").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "de|Hallo");
    assert!(!headers.contains_key("vary"));
    assert!(!headers.contains_key("content-language"));
    assert!(events.0.lock().unwrap().is_empty());
    let (_, headers, body) = send(&kernel, "/route", "en").await;
    assert_eq!(body, "fr|Bonjour");
    assert_eq!(headers["content-language"], "fr");
    assert_eq!(headers["vary"], "Accept-Language");
    assert_eq!(send(&kernel, "/", "fr").await.2, "de|Hallo");
}

#[tokio::test]
async fn configuration_is_atomic_and_updates_all_consumers_before_events_once_per_request() {
    let root = Root::new();
    let recorded = Arc::new(Events::default());
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware.set_locale(&["fr"]).unwrap();
                middleware.set_locale(&["DE", "fr", "en"]).unwrap();
                assert!(middleware.set_locale(&["it", "zh_CN"]).is_err());
            },
            recorded.clone(),
        )
        .await;
    for _ in 0..2 {
        let (status, headers, body) = send(&kernel, "/", "en;q=0.1, de;q=0.9").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "de|Hallo");
        assert_eq!(headers["content-language"], "de");
    }
    assert_eq!(*recorded.0.lock().unwrap(), ["de", "de"]);
    assert_eq!(
        kernel.get_application().config().unwrap().get("app.locale"),
        Some(serde_json::json!("en"))
    );
    assert!(matches!(
        SetLocale::new(&["../en"]),
        Err(TranslationError::InvalidLocale)
    ));
}

#[tokio::test]
async fn response_metadata_preserves_existing_headers_and_uses_downstream_final_locale() {
    let root = Root::new();
    let kernel = root.kernel("en", browser, Arc::default()).await;
    let (_, headers, _) = send(&kernel, "/metadata", "de").await;
    assert_eq!(headers["content-language"], "it");
    assert_eq!(
        headers
            .get_all("vary")
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>(),
        ["Accept-Encoding", "Origin", "Accept-Language"]
    );
    for (path, expected) in [("/star", "*"), ("/existing", "Origin, accept-language")] {
        let (_, headers, _) = send(&kernel, path, "de").await;
        assert_eq!(headers["vary"], expected);
        assert_eq!(headers.get_all("vary").iter().count(), 1);
    }
    let (_, headers, _) = send(&kernel, "/switch", "de").await;
    assert_eq!(headers["content-language"], "fr");
}

#[tokio::test]
async fn errors_and_missing_routes_keep_selected_language_and_next_request_recovers() {
    let root = Root::new();
    let kernel = root.kernel("en", browser, Arc::default()).await;
    for (path, expected) in [
        ("/missing", StatusCode::NOT_FOUND),
        ("/failure", StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let (status, headers, _) = send(&kernel, path, "de").await;
        assert_eq!(status, expected);
        assert_eq!(headers["content-language"], "de");
        assert_eq!(headers["vary"], "Accept-Language");
    }
    assert_eq!(send(&kernel, "/", "en").await.2, "en|Hello");
    let (status, headers, body) = send(&kernel, "/replace-config", "de").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "replaced");
    assert_eq!(headers["content-language"], "de");
    assert_eq!(send(&kernel, "/", "en").await.2, "en|Hello");
}

#[tokio::test]
async fn listener_errors_stop_dispatch_without_polluting_next_requests() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware.set_locale(&["en", "de-DE"]).unwrap();
            },
            events.clone(),
        )
        .await;
    let (status, _, body) = send(&kernel, "/", "de-DE").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!body.contains("en|Hello"));
    assert_eq!(send(&kernel, "/", "en").await.2, "en|Hello");
    assert_eq!(*events.0.lock().unwrap(), ["de-DE", "en"]);
    assert_eq!(
        kernel.get_application().config().unwrap().get("app.locale"),
        Some(serde_json::json!("en"))
    );
}

#[tokio::test]
async fn policy_and_locale_are_isolated_between_concurrent_requests_and_applications() {
    let root = Root::new();
    let selected = root.kernel("en", browser, Arc::default()).await;
    let untouched = root
        .kernel(
            "fr",
            |middleware| {
                middleware.set_locale(&[]).unwrap();
            },
            Arc::default(),
        )
        .await;
    for _ in 0..4 {
        let (de, fr, en, other) = tokio::join!(
            send(&selected, "/", "de"),
            send(&selected, "/", "fr"),
            send(&selected, "/", "en"),
            send(&untouched, "/", "de")
        );
        assert_eq!(de.2, "de|Hallo");
        assert_eq!(fr.2, "fr|Bonjour");
        assert_eq!(en.2, "en|Hello");
        assert_eq!(other.2, "fr|Bonjour");
        assert!(!other.1.contains_key("vary"));
    }
}

fn header_locale(request: &Request) -> Result<Option<String>, HttpError> {
    Ok(request.header("x-locale")?.map(str::to_owned))
}

async fn send_selected(
    kernel: &Kernel,
    path: &str,
    selected: Option<&str>,
    browser: &str,
) -> (StatusCode, HeaderMap, String) {
    let mut builder = http::Request::builder()
        .uri(path)
        .header("host", "localhost")
        .header("accept-language", browser);
    if let Some(locale) = selected {
        builder = builder.header("x-locale", locale);
    }
    let (parts, body) = builder.body(Body::empty()).unwrap().into_parts();
    let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, 1024 * 128).await.unwrap();
    (
        parts.status,
        parts.headers,
        String::from_utf8(body.to_vec()).unwrap(),
    )
}

#[tokio::test]
async fn callback_selects_exact_canonical_candidate_and_none_uses_browser() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware.locale_using(header_locale);
            },
            events.clone(),
        )
        .await;
    for (selected, browser, expected) in [
        (Some("ZH-hANT"), "en", "zh-Hant|您好"),
        (Some("en"), "zh-CN", "en|Hello"),
        (None, "zh-CN", "zh-Hans|你好"),
        (None, "xx", "en|Hello"),
    ] {
        let (status, headers, body) = send_selected(&kernel, "/", selected, browser).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, expected);
        assert_eq!(
            headers["content-language"],
            expected.split('|').next().unwrap()
        );
    }
    assert_eq!(
        *events.0.lock().unwrap(),
        ["zh-Hant", "en", "zh-Hans", "en"]
    );
}

#[tokio::test]
async fn invalid_or_unsupported_callback_values_fail_without_dispatch_or_event() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware.locale_using(header_locale);
            },
            events.clone(),
        )
        .await;
    for selected in ["zh_CN", "", "it", "zh-CN", "en-US", "en-u-ca-gregory"] {
        let (status, _, body) = send_selected(&kernel, "/", Some(selected), "en").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{selected}");
        assert!(!body.contains("en|Hello"));
    }
    assert!(events.0.lock().unwrap().is_empty());
    assert_eq!(send(&kernel, "/", "en").await.2, "en|Hello");
    assert_eq!(
        kernel.get_application().config().unwrap().get("app.locale"),
        Some(serde_json::json!("en"))
    );
}

#[tokio::test]
async fn callback_errors_keep_the_original_http_status_and_do_not_update_locale() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware.locale_using(|request| {
                    if request.header("x-locale")?.is_some() {
                        Err(openrust::http::abort(
                            StatusCode::IM_A_TEAPOT,
                            "语言选择失败",
                            HeaderMap::new(),
                        ))
                    } else {
                        Ok(None)
                    }
                });
            },
            events.clone(),
        )
        .await;
    let (status, _, body) = send_selected(&kernel, "/", Some("fr"), "en").await;
    assert_eq!(status, StatusCode::IM_A_TEAPOT);
    assert!(!body.contains("en|Hello"));
    assert!(events.0.lock().unwrap().is_empty());
    assert_eq!(send(&kernel, "/", "en").await.2, "en|Hello");
}

#[tokio::test]
async fn candidates_and_callback_survive_both_configuration_orders_and_failed_updates() {
    let root = Root::new();
    for callback_first in [true, false] {
        let kernel = root
            .kernel(
                "en",
                move |middleware| {
                    if callback_first {
                        middleware.locale_using(header_locale);
                    }
                    middleware.set_locale(&["en", "fr"]).unwrap();
                    if !callback_first {
                        middleware.locale_using(header_locale);
                    }
                    assert!(middleware.set_locale(&["de", "zh_CN"]).is_err());
                },
                Arc::default(),
            )
            .await;
        assert_eq!(
            send_selected(&kernel, "/", Some("fr"), "en").await.2,
            "fr|Bonjour"
        );
        assert_eq!(send(&kernel, "/", "fr").await.2, "fr|Bonjour");
        assert_eq!(
            send_selected(&kernel, "/", Some("de"), "en").await.0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}

#[tokio::test]
async fn empty_candidates_skip_callback_but_keep_it_for_reenabling() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let disabled = root
        .kernel(
            "de",
            |middleware| {
                middleware.locale_using(|_| panic!("关闭后不得执行回调"));
                middleware.set_locale(&[]).unwrap();
            },
            events.clone(),
        )
        .await;
    let (status, headers, body) = send(&disabled, "/", "en").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "de|Hallo");
    assert!(!headers.contains_key("content-language"));
    assert!(!headers.contains_key("vary"));
    assert!(events.0.lock().unwrap().is_empty());
    let enabled = root
        .kernel(
            "en",
            |middleware| {
                middleware.locale_using(header_locale);
                middleware.set_locale(&[]).unwrap();
                assert!(middleware.set_locale(&["zh_CN"]).is_err());
                middleware.set_locale(&["en", "fr"]).unwrap();
            },
            Arc::default(),
        )
        .await;
    assert_eq!(
        send_selected(&enabled, "/", Some("fr"), "en").await.2,
        "fr|Bonjour"
    );
}

#[tokio::test]
async fn custom_sources_keep_downstream_language_and_existing_response_headers() {
    let root = Root::new();
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                browser(middleware);
                middleware.locale_using(header_locale);
            },
            Arc::default(),
        )
        .await;
    let (_, headers, _) = send_selected(&kernel, "/switch", Some("de"), "en").await;
    assert_eq!(headers["content-language"], "fr");
    let (_, headers, _) = send_selected(&kernel, "/metadata", Some("de"), "en").await;
    assert_eq!(headers["content-language"], "it");
    assert_eq!(
        headers
            .get_all("vary")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect::<Vec<_>>(),
        ["Accept-Encoding", "Origin", "Accept-Language"]
    );
    for (path, expected) in [("/star", "*"), ("/existing", "Origin, accept-language")] {
        let (_, headers, _) = send_selected(&kernel, path, Some("de"), "en").await;
        assert_eq!(headers["vary"], expected);
        assert_eq!(headers.get_all("vary").iter().count(), 1);
    }
}

#[tokio::test]
async fn callback_policy_is_isolated_between_requests_instances_and_applications() {
    let root = Root::new();
    let selected = root
        .kernel(
            "en",
            |middleware| {
                browser(middleware);
                middleware.locale_using(header_locale);
            },
            Arc::default(),
        )
        .await;
    let constant = root
        .kernel(
            "en",
            |middleware| {
                middleware.set_locale(&["en", "fr"]).unwrap();
                middleware.locale_using(|_| Ok(Some("fr".into())));
            },
            Arc::default(),
        )
        .await;
    let default = root.kernel("en", |_| {}, Arc::default()).await;
    for _ in 0..4 {
        let (de, fr, fallback, other, unchanged) = tokio::join!(
            send_selected(&selected, "/", Some("de"), "en"),
            send_selected(&selected, "/", Some("fr"), "en"),
            send(&selected, "/", "en"),
            send(&constant, "/", "de"),
            send(&default, "/", "de")
        );
        assert_eq!(de.2, "de|Hallo");
        assert_eq!(fr.2, "fr|Bonjour");
        assert_eq!(fallback.2, "en|Hello");
        assert_eq!(other.2, "fr|Bonjour");
        assert_eq!(unchanged.2, "de|Hallo");
    }
    for kernel in [&selected, &constant, &default] {
        assert_eq!(
            kernel.get_application().config().unwrap().get("app.locale"),
            Some(serde_json::json!("en"))
        );
    }
}

#[tokio::test]
async fn exact_callback_tags_keep_regions_and_extensions_without_browser_mapping() {
    let root = Root::new();
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware
                    .set_locale(&["en", "zh-Hans", "zh-CN", "en-US-u-ca-gregory"])
                    .unwrap();
                middleware.locale_using(header_locale);
            },
            Arc::default(),
        )
        .await;
    for (selected, expected) in [
        ("ZH-cn", "zh-CN"),
        ("EN-us-u-CA-gregory", "en-US-u-ca-gregory"),
    ] {
        let (status, headers, body) = send_selected(&kernel, "/", Some(selected), "zh-CN").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-language"], expected);
        assert_eq!(body, format!("{expected}|Hello"));
    }
    assert_eq!(send(&kernel, "/", "zh-CN").await.2, "zh-Hans|你好");
}

#[tokio::test]
async fn cloned_route_instances_keep_independent_candidates_and_callbacks() {
    let root = Root::new();
    let app = Application::configure(root.0.clone())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                serde_json::json!({"locale":"en"}),
            )]))
        })
        .with_middleware(|middleware| {
            middleware.set_locale(&[]).unwrap();
        })
        .with_routing(Routing::default().web(|| {
            let mut first = SetLocale::new(&["en", "fr"]).unwrap();
            first.locale_using(|_| Ok(Some("fr".into())));
            let mut second = first.clone();
            second.set_locale(&["en", "de"]).unwrap();
            assert!(second.set_locale(&["zh_CN"]).is_err());
            second.locale_using(|_| Ok(Some("de".into())));
            Route::group(
                openrust::routing::RouteGroup::new().middleware(Arc::new(first)),
                |router| {
                    router.get("/first", inspect)?;
                    Ok(())
                },
            )?;
            Route::group(
                openrust::routing::RouteGroup::new().middleware(Arc::new(second)),
                |router| {
                    router.get("/second", inspect)?;
                    Ok(())
                },
            )?;
            Ok(())
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    for _ in 0..3 {
        let (first, second) = tokio::join!(
            send(&kernel, "/first", "en"),
            send(&kernel, "/second", "en")
        );
        assert_eq!(first.2, "fr|Bonjour");
        assert_eq!(second.2, "de|Hallo");
    }
}

fn session_locale(request: &Request) -> Result<Option<String>, HttpError> {
    use openrust::contracts::session::Session;
    Ok(request
        .session()?
        .get("locale")
        .and_then(|value| value.as_str().map(str::to_owned)))
}

fn seed_session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::contracts::session::Session;
        request
            .session_mut()?
            .put("locale", serde_json::json!("fr"))?;
        Ok(Response::from("saved"))
    })
}

fn private_session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        assert!(request.has_session());
        let response = inspect(request).await?;
        Ok(response
            .header(
                "cache-control".parse().unwrap(),
                "private, no-store".parse().unwrap(),
            )
            .header(
                "vary".parse().unwrap(),
                "Cookie, accept-language".parse().unwrap(),
            ))
    })
}

async fn session_kernel(root: &Root, global: bool, locale_before_session: bool) -> Kernel {
    use openrust::{
        cookie::middleware::EncryptCookies,
        encryption::{Cipher, Encrypter},
        routing::MiddlewareEntry,
        session::middleware::StartSession,
    };
    let app = Application::configure(root.0.clone())
        .with_config(|_, _| Ok(BTreeMap::from([
            ("app".into(), serde_json::json!({"locale":"en", "fallback_locale":"en", "key":"a".repeat(32)})),
            ("session".into(), serde_json::json!({"driver":"array", "cookie":"locale_session", "lottery":[0,100]})),
        ])))
        .with_middleware(move |middleware| {
            if global {
                middleware.set_locale(&["en", "fr"]).unwrap();
                middleware.locale_using(session_locale);
            } else { middleware.set_locale(&[]).unwrap(); }
            let mut locale = SetLocale::new(&["en", "fr"]).unwrap();
            locale.locale_using(session_locale);
            let locale = MiddlewareEntry::Instance(Arc::new(locale));
            let mut entries = vec![MiddlewareEntry::Instance(Arc::new(EncryptCookies::new(Arc::new(Encrypter::new([b'a';32], Cipher::Aes256Cbc).unwrap()))))];
            if locale_before_session { entries.push(locale.clone()); }
            entries.push(MiddlewareEntry::Instance(Arc::new(StartSession)));
            if !locale_before_session { entries.push(locale); }
            middleware.group("web", entries);
        })
        .with_routing(Routing::default().web(|| {
            Route::get("/seed", seed_session)?;
            Route::get("/", private_session)?;
            Ok(())
        })).create().unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    kernel
}

#[tokio::test]
async fn session_rules_require_correct_group_order_and_keep_private_cache_policy() {
    let root = Root::new();
    for (global, before_session) in [(true, false), (false, true)] {
        let kernel = session_kernel(&root, global, before_session).await;
        let (status, _, body) = send(&kernel, "/seed", "en").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_ne!(body, "saved");
    }
    let kernel = session_kernel(&root, false, false).await;
    let (status, headers, _) = send(&kernel, "/seed", "en").await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers
        .get_all("set-cookie")
        .iter()
        .find_map(|v| {
            let value = v.to_str().unwrap();
            value
                .starts_with("locale_session=")
                .then(|| value.split(';').next().unwrap().to_owned())
        })
        .unwrap();
    let (parts, body) = http::Request::builder()
        .uri("/")
        .header("host", "localhost")
        .header("cookie", cookie)
        .header("accept-language", "en")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-language"], "fr");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["vary"], "Cookie, accept-language");
    assert_eq!(response.headers().get_all("vary").iter().count(), 1);
    assert_eq!(
        axum::body::to_bytes(response.into_parts().1, 1024)
            .await
            .unwrap(),
        "fr|Bonjour"
    );
    assert_eq!(send(&kernel, "/", "en").await.2, "en|Hello");
}

#[tokio::test]
async fn callback_source_errors_are_visible_to_registered_exception_renderers() {
    let root = Root::new();
    let app = Application::configure(root.0.clone())
        .with_middleware(|middleware| {
            middleware.locale_using(|_| {
                Err(HttpError::Internal {
                    source: Box::new(std::io::Error::other("语言来源暂不可用")),
                })
            });
        })
        .with_exceptions(|exceptions| {
            exceptions.render::<std::io::Error>(|error, _| {
                Some(Response::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    Body::from(error.to_string()),
                ))
            });
        })
        .with_routing(Routing::default().web(|| Route::get("/", inspect)))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let (status, _, body) = send(&kernel, "/", "en").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, "语言来源暂不可用");
}

#[tokio::test]
async fn query_is_exact_and_overrides_browser_with_one_event_per_request() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root.kernel("en", |_| {}, events.clone()).await;
    for (path, expected) in [
        ("/?locale=en", "en"),
        ("/?locale=zh-Hans", "zh-Hans"),
        ("/?locale=ZH-hANT", "zh-Hant"),
        ("/?locale=en&locale=zh-Hans", "zh-Hans"),
        ("/?other=fr", "zh-Hant"),
    ] {
        let (status, headers, body) = send(&kernel, path, "zh-TW").await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(headers["content-language"], expected);
        assert!(body.starts_with(expected));
    }
    assert_eq!(
        events.0.lock().unwrap().as_slice(),
        ["en", "zh-Hans", "zh-Hant", "zh-Hans", "zh-Hant"]
    );
    for path in [
        "/?locale=",
        "/?locale",
        "/?locale=zh_CN",
        "/?locale=it",
        "/?locale=zh-CN",
        "/?locale[]=en",
        "/?locale[x]=en",
        "/?locale=en&locale[]=fr",
    ] {
        assert_eq!(
            send(&kernel, path, "en").await.0,
            StatusCode::BAD_REQUEST,
            "{path}"
        );
    }
    assert_eq!(events.0.lock().unwrap().len(), 5);
    assert_eq!(send(&kernel, "/", "en").await.2, "en|Hello");
    assert_eq!(
        send(&kernel, "/switch?locale=zh-Hans", "en").await.1["content-language"],
        "fr"
    );
    let (_, headers, _) = send(&kernel, "/metadata?locale=zh-Hans", "en").await;
    assert_eq!(headers["content-language"], "it");
    assert_eq!(headers.get_all("vary").iter().count(), 3);
}

#[tokio::test]
async fn query_configuration_missing_null_custom_invalid_and_empty_candidates() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root
        .kernel_query("en", |_| {}, events.clone(), Some("language".into()))
        .await;
    assert!(
        send(&kernel, "/?language=zh-Hans&locale=fr", "en")
            .await
            .2
            .starts_with("zh-Hans|")
    );
    assert_eq!(send(&kernel, "/?locale=zh-Hans", "en").await.2, "en|Hello");
    let disabled = root
        .kernel_query("en", |_| {}, events.clone(), Some(serde_json::Value::Null))
        .await;
    assert!(
        send(&disabled, "/?locale[]=bad", "zh-TW")
            .await
            .2
            .starts_with("zh-Hant|")
    );
    for value in [
        serde_json::json!(""),
        serde_json::json!(" "),
        serde_json::json!("lang.name"),
        serde_json::json!("lang[]"),
        serde_json::json!(42),
        serde_json::json!(false),
        serde_json::json!([]),
    ] {
        let invalid = root
            .kernel_query("en", |_| {}, events.clone(), Some(value))
            .await;
        assert_eq!(
            send(&invalid, "/", "en").await.0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    let empty = root
        .kernel_query(
            "en",
            |middleware| {
                middleware.set_locale(&[]).unwrap();
                middleware.locale_using(|_| panic!("停用实例不能执行回调"));
            },
            events,
            Some(false.into()),
        )
        .await;
    let (status, headers, body) = send(&empty, "/?locale[]=bad", "zh-TW").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "en|Hello");
    assert!(!headers.contains_key("content-language"));
}

#[tokio::test]
async fn callback_precedes_query_and_none_continues_query() {
    let root = Root::new();
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware.locale_using(header_locale);
            },
            Arc::new(Events::default()),
        )
        .await;
    assert!(
        send_selected(&kernel, "/?locale[]=bad", Some("zh-Hant"), "en")
            .await
            .2
            .starts_with("zh-Hant|")
    );
    assert!(
        send_selected(&kernel, "/?locale=zh-Hans", None, "en")
            .await
            .2
            .starts_with("zh-Hans|")
    );
    assert_eq!(
        send_selected(&kernel, "/?locale=en", Some("it"), "en")
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let failed = root
        .kernel(
            "en",
            |middleware| {
                middleware.locale_using(|_| Err(HttpError::NotFound));
            },
            Arc::new(Events::default()),
        )
        .await;
    assert_eq!(
        send(&failed, "/?locale=zh-Hans", "en").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn query_selection_ignores_body_and_isolated_concurrent_applications() {
    let first = Root::new();
    let second = Root::new();
    let first_kernel = first
        .kernel("en", |_| {}, Arc::new(Events::default()))
        .await;
    let second_kernel = second
        .kernel_query(
            "en",
            |_| {},
            Arc::new(Events::default()),
            Some("language".into()),
        )
        .await;
    let (a, b, c) = tokio::join!(
        send(&first_kernel, "/?locale=zh-Hans", "en"),
        send(&first_kernel, "/?locale=zh-Hant", "en"),
        send(&second_kernel, "/?locale=zh-Hans", "en")
    );
    assert!(a.2.starts_with("zh-Hans|"));
    assert!(b.2.starts_with("zh-Hant|"));
    assert_eq!(c.2, "en|Hello");
    let raw = http::Request::builder()
        .uri("/")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept-language", "en")
        .body(Body::from(r#"{"locale":"zh-Hant"}"#))
        .unwrap();
    let (parts, body) = raw.into_parts();
    let response = first_kernel
        .handle(&mut Request::from_parts(parts, body))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, body) = response.into_parts();
    assert_eq!(axum::body::to_bytes(body, 4096).await.unwrap(), "en|Hello");

    let raw = http::Request::builder()
        .uri("/")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept-language", "en")
        .body(Body::from(r#"{"locale":"zh-Hans"}"#))
        .unwrap();
    let (parts, body) = first_kernel
        .handle(&mut Request::from(raw))
        .await
        .into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    assert_eq!(axum::body::to_bytes(body, 65536).await.unwrap(), "en|Hello");
}

#[tokio::test]
async fn query_parameter_uses_cached_configuration_without_reloading_source() {
    let root = Root::new();
    fs::create_dir_all(root.0.join("storage/framework")).unwrap();
    fs::write(
        root.0.join("storage/framework/config.json"),
        r#"{"app":{"locale":"en","fallback_locale":"en","locale_query_parameter":"language"}}"#,
    )
    .unwrap();
    let kernel = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
    assert!(
        send(&kernel, "/?language=zh-Hans&locale=bad", "en")
            .await
            .2
            .starts_with("zh-Hans|")
    );
    fs::write(
        root.0.join("storage/framework/config.json"),
        r#"{"app":{"locale":"en","locale_query_parameter":null}}"#,
    )
    .unwrap();
    assert!(
        send(&kernel, "/?language=zh-Hant", "en")
            .await
            .2
            .starts_with("zh-Hant|")
    );
    let disabled = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
    assert_eq!(send(&disabled, "/?language=bad", "en").await.2, "en|Hello");
}

#[tokio::test]
async fn explicit_query_preserves_exact_region_and_extension_candidates() {
    let root = Root::new();
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware
                    .set_locale(&["en", "en-US", "zh-CN", "en-US-u-nu-arab"])
                    .unwrap();
            },
            Arc::new(Events::default()),
        )
        .await;
    for locale in ["en-US", "zh-CN", "en-US-u-nu-arab"] {
        let (status, headers, body) = send(&kernel, &format!("/?locale={locale}"), "zh-TW").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(headers["content-language"], locale);
        assert!(body.starts_with(&format!("{locale}|")));
    }
}

async fn send_headers(
    kernel: &Kernel,
    path: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, String) {
    let mut raw = http::Request::builder()
        .uri(path)
        .header("host", "localhost");
    for (name, value) in headers {
        raw = raw.header(*name, *value);
    }
    let mut request = Request::from(raw.body(Body::empty()).unwrap());
    // 先读取标准接口的缓存，确保配置的其他来源不会复用它。
    let standard = request.get_languages().to_vec();
    assert_eq!(
        request.get_preferred_language(None).unwrap(),
        standard.first().cloned()
    );
    let response = kernel.handle(&mut request).await;
    assert_eq!(request.get_languages(), standard);
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        String::from_utf8(axum::body::to_bytes(body, 131072).await.unwrap().to_vec()).unwrap(),
    )
}

fn configure_header(root: &Root, value: serde_json::Value) {
    fs::write(
        root.0.join("locale-header.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn configured_header_reuses_negotiation_and_preserves_standard_request_api() {
    let root = Root::new();
    configure_header(&root, "X-LaNgUaGe".into());
    let kernel = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
    for (values, expected) in [
        (
            vec![("accept-language", "zh-CN"), ("x-language", "zh-TW")],
            "zh-Hant",
        ),
        (vec![("accept-language", "zh-TW")], "en"),
        (
            vec![
                ("x-language", "zh-CN;q=0.3, en;q=0.2"),
                ("X-LANGUAGE", "zh-TW;q=0.9"),
            ],
            "zh-Hant",
        ),
        (
            vec![
                (
                    "x-language",
                    "zh-TW;q=0, invalid_tag, zh-CN;q=NaN, en;q=1.001",
                ),
                ("accept-language", "zh-TW"),
            ],
            "en",
        ),
        (
            vec![("x-language", "*"), ("accept-language", "zh-TW")],
            "en",
        ),
    ] {
        let (status, headers, body) = send_headers(&kernel, "/", &values).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.starts_with(&format!("{expected}|")), "{body}");
        assert_eq!(headers["content-language"], expected);
        assert_eq!(
            headers["vary"].to_str().unwrap().to_ascii_lowercase(),
            "x-language"
        );
    }
    let (a, b) = tokio::join!(
        send_headers(&kernel, "/", &[("x-language", "zh-CN")]),
        send_headers(&kernel, "/", &[("x-language", "zh-TW")])
    );
    assert!(a.2.starts_with("zh-Hans|"));
    assert!(b.2.starts_with("zh-Hant|"));
    assert_eq!(
        send_headers(&kernel, "/?locale=zh-Hans", &[("x-language", "zh-TW")])
            .await
            .2,
        "zh-Hans|你好"
    );
    let callback = root
        .kernel(
            "en",
            |m| {
                m.locale_using(header_locale);
            },
            Arc::new(Events::default()),
        )
        .await;
    assert!(
        send_headers(
            &callback,
            "/?locale=zh-Hans",
            &[("x-locale", "zh-Hant"), ("x-language", "en")]
        )
        .await
        .2
        .starts_with("zh-Hant|")
    );
    assert!(
        send_headers(&callback, "/?locale=zh-Hans", &[("x-language", "zh-TW")])
            .await
            .2
            .starts_with("zh-Hans|")
    );
    assert!(
        send_headers(&callback, "/", &[("x-language", "zh-TW")])
            .await
            .2
            .starts_with("zh-Hant|")
    );
}

#[tokio::test]
async fn header_null_errors_empty_candidates_and_vary_contract() {
    let root = Root::new();
    configure_header(&root, serde_json::Value::Null);
    let disabled = root
        .kernel("zh-Hans", |_| {}, Arc::new(Events::default()))
        .await;
    let (_, headers, body) = send(&disabled, "/", "zh-TW").await;
    assert_eq!(body, "zh-Hans|你好");
    assert!(!headers.contains_key("vary"));
    let (_, existing, _) = send(&disabled, "/existing", "zh-TW").await;
    assert_eq!(existing["vary"], "Origin, accept-language");

    assert!(
        send(&disabled, "/?locale=zh-Hant", "en")
            .await
            .2
            .starts_with("zh-Hant|")
    );
    let callback = root
        .kernel(
            "en",
            |m| {
                m.locale_using(|_| Ok(Some("zh-Hant".into())));
            },
            Arc::new(Events::default()),
        )
        .await;
    assert!(
        send(&callback, "/?locale=bad", "en")
            .await
            .2
            .starts_with("zh-Hant|")
    );
    for value in [
        serde_json::json!(""),
        serde_json::json!(" "),
        serde_json::json!("X Language"),
        serde_json::json!("X:Language"),
        serde_json::json!("头"),
        serde_json::json!("X\r\nOther"),
        serde_json::json!(42),
        serde_json::json!(false),
        serde_json::json!([]),
        serde_json::json!({}),
    ] {
        configure_header(&root, value);
        let invalid = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
        assert_eq!(
            send(&invalid, "/?locale=zh-Hans", "en").await.0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    let invalid_callback = root
        .kernel(
            "en",
            |m| {
                m.locale_using(|_| panic!("错误配置须先于回调报告"));
            },
            Arc::new(Events::default()),
        )
        .await;
    assert_eq!(
        send(&invalid_callback, "/", "en").await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let empty = root
        .kernel(
            "en",
            |m| {
                m.set_locale(&[]).unwrap();
                m.locale_using(|_| panic!("停用实例不能执行回调"));
            },
            Arc::new(Events::default()),
        )
        .await;
    let (_, headers, body) = send(&empty, "/?locale[]=bad", "zh-TW").await;
    assert_eq!(body, "en|Hello");
    assert!(!headers.contains_key("vary"));
    configure_header(&root, "X-Language".into());
    let kernel = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
    let (_, headers, _) = send_headers(&kernel, "/metadata", &[]).await;
    let varies: Vec<_> = headers
        .get_all("vary")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(varies, ["Accept-Encoding", "Origin", "x-language"]);
    assert_eq!(headers["content-language"], "it");
    let (_, headers, _) = send_headers(&kernel, "/star", &[]).await;
    assert_eq!(headers["vary"], "*");
    let (_, headers, _) = send_headers(&kernel, "/existing-custom", &[]).await;
    assert_eq!(headers.get_all("vary").iter().count(), 1);
    assert_eq!(headers["vary"], "Origin, X-LANGUAGE");
    let (_, headers, body) = send_headers(&kernel, "/switch", &[]).await;
    assert_eq!(body, "switched");
    assert_eq!(headers["content-language"], "fr");
    let second = Root::new();
    let standard = second
        .kernel("en", |_| {}, Arc::new(Events::default()))
        .await;
    let (a, b) = tokio::join!(
        send_headers(
            &kernel,
            "/",
            &[("accept-language", "zh-TW"), ("x-language", "zh-CN")]
        ),
        send_headers(
            &standard,
            "/",
            &[("accept-language", "zh-TW"), ("x-language", "zh-CN")]
        )
    );
    assert!(a.2.starts_with("zh-Hans|"));
    assert!(b.2.starts_with("zh-Hant|"));
}

#[tokio::test]
async fn header_configuration_uses_cache_snapshot_and_preserves_null() {
    let root = Root::new();
    fs::create_dir_all(root.0.join("storage/framework")).unwrap();
    let cache = root.0.join("storage/framework/config.json");
    fs::write(
        &cache,
        r#"{"app":{"locale":"en","fallback_locale":"en","locale_header":"X-Language"}}"#,
    )
    .unwrap();
    let kernel = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
    fs::write(
        &cache,
        r#"{"app":{"locale":"en","fallback_locale":"en","locale_header":null}}"#,
    )
    .unwrap();
    assert!(
        send_headers(&kernel, "/", &[("x-language", "zh-TW")])
            .await
            .2
            .starts_with("zh-Hant|")
    );
    let disabled = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
    let (_, headers, body) = send_headers(
        &disabled,
        "/",
        &[("x-language", "zh-TW"), ("accept-language", "zh-TW")],
    )
    .await;
    assert_eq!(body, "en|Hello");
    assert!(!headers.contains_key("vary"));
}

#[tokio::test]
async fn custom_header_rereads_changed_request_without_standard_cache_contamination() {
    let root = Root::new();
    configure_header(&root, "X-Language".into());
    let kernel = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
    let mut request = Request::from(
        http::Request::builder()
            .uri("/")
            .header("host", "localhost")
            .header("accept-language", "en")
            .header("x-language", "zh-CN")
            .body(Body::empty())
            .unwrap(),
    );
    for (value, expected) in [
        ("zh-CN", "zh-Hans"),
        ("zh-TW", "zh-Hant"),
        ("invalid_tag", "en"),
    ] {
        request
            .parts_mut()
            .headers
            .insert("x-language", value.parse().unwrap());
        assert_eq!(request.get_languages(), ["en"]);
        let (parts, body) = kernel.handle(&mut request).await.into_parts();
        assert_eq!(parts.headers["content-language"], expected);
        let body = axum::body::to_bytes(body, 4096).await.unwrap();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .starts_with(&format!("{expected}|"))
        );
        assert_eq!(request.get_languages(), ["en"]);
    }
}

// 在真实 Kernel 下同时消费标准偏好、应用语言与译器，覆盖协商后首次读取标准头。
fn language_boundary(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if request.path() == "boundary-switch" {
            App::set_locale("fr").await.unwrap();
        }
        tokio::task::yield_now().await;
        Ok(Response::from(serde_json::json!({
            "languages": request.get_languages(),
            "preferred": request.get_preferred_language(Some(&["en", "zh-Hans", "zh-Hant"])).unwrap(),
            "current": App::current_locale().unwrap(),
            "translator": Lang::get_locale().unwrap(),
            "text": Lang::get("messages.hello", &BTreeMap::new(), None).unwrap(),
        }).to_string()))
    })
}

#[tokio::test]
async fn standard_preferences_and_actual_locale_are_independent_in_http_consumers() {
    let root = Root::new();
    configure_header(&root, "X-Language".into());
    let custom = root.kernel("en", |_| {}, Arc::new(Events::default())).await;
    let other = Root::new();
    let standard = other
        .kernel("en", |_| {}, Arc::new(Events::default()))
        .await;
    for warm_standard in [false, true] {
        for (kernel, path, expected, text, vary) in [
            (&custom, "/boundary", "zh-Hant", "您好", "x-language"),
            (&custom, "/boundary?locale=en", "en", "Hello", "x-language"),
            (&custom, "/boundary-switch", "fr", "Bonjour", "x-language"),
            (&standard, "/boundary", "zh-Hans", "你好", "Accept-Language"),
        ] {
            let mut request = Request::from(
                http::Request::builder()
                    .uri(path)
                    .header("host", "localhost")
                    .header("accept-language", "zh-Hans;q=0.9, en;q=0.1")
                    .header("x-language", "zh-Hant")
                    .body(Body::empty())
                    .unwrap(),
            );
            if warm_standard {
                assert_eq!(request.get_languages(), ["zh-Hans", "en"]);
                assert_eq!(
                    request.get_preferred_language(None).unwrap().as_deref(),
                    Some("zh-Hans")
                );
            }
            let (parts, body) = kernel.handle(&mut request).await.into_parts();
            assert_eq!(parts.status, StatusCode::OK);
            assert_eq!(parts.headers["content-language"], expected);
            assert_eq!(parts.headers["vary"], vary);
            let value: serde_json::Value =
                serde_json::from_slice(&axum::body::to_bytes(body, 65536).await.unwrap()).unwrap();
            assert_eq!(
                value,
                serde_json::json!({"languages":["zh-Hans","en"], "preferred":"zh-Hans", "current":expected, "translator":expected,"text":text})
            );
            assert_eq!(request.get_languages(), ["zh-Hans", "en"]);
        }
    }
    assert_eq!(
        custom.get_application().config().unwrap().get("app.locale"),
        Some(serde_json::json!("en"))
    );
    assert_eq!(
        standard
            .get_application()
            .config()
            .unwrap()
            .get("app.locale"),
        Some(serde_json::json!("en"))
    );
}

#[tokio::test]
async fn protected_cookie_selection_emits_one_synchronized_event_per_request() {
    use openrust::{
        cookie::{
            CookieValuePrefix,
            middleware::{AddQueuedCookiesToResponse, EncryptCookies},
        },
        encryption::{Cipher, Encrypter},
        routing::MiddlewareEntry,
    };
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root
        .kernel(
            "en",
            |middleware| {
                middleware.group(
                    "web",
                    [
                        MiddlewareEntry::Instance(Arc::new(EncryptCookies::new(Arc::new(
                            Encrypter::new([b'a'; 32], Cipher::Aes256Cbc).unwrap(),
                        )))),
                        MiddlewareEntry::Instance(Arc::new(AddQueuedCookiesToResponse)),
                    ],
                );
            },
            events.clone(),
        )
        .await;
    let encrypter = Encrypter::new([b'a'; 32], Cipher::Aes256Cbc).unwrap();
    let mut clear = CookieValuePrefix::create("locale", &[b'a'; 32]).to_vec();
    clear.extend_from_slice(b"zh-Hans");
    let cipher = encrypter.encrypt_string(&clear).unwrap();
    for (path, locale) in [
        ("/", "zh-Hans"),
        ("/?locale=zh-Hant", "zh-Hant"),
        ("/", "zh-Hans"),
    ] {
        let mut request = Request::from(
            http::Request::builder()
                .uri(path)
                .header("cookie", format!("locale={cipher}"))
                .header("accept-language", "en")
                .body(Body::empty())
                .unwrap(),
        );
        let (parts, body) = kernel.handle(&mut request).await.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(parts.headers["content-language"], locale);
        assert!(
            String::from_utf8(axum::body::to_bytes(body, 4096).await.unwrap().to_vec())
                .unwrap()
                .starts_with(locale)
        );
    }
    assert_eq!(
        events.0.lock().unwrap().as_slice(),
        ["zh-Hans", "zh-Hant", "zh-Hans"]
    );
}
