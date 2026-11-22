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
        fs::write(self.0.join(".env"), format!("APP_LOCALE={default}\n")).unwrap();
        let app = Application::configure(self.0.clone())
            .withConfig(|_, env| Ok(BTreeMap::from([("app".into(), serde_json::json!({"locale":env.get_utf8("APP_LOCALE")?.unwrap_or("en"), "fallback_locale":"en"}))])))
            .withProviders(vec![Box::new(Observer(events))])
            .withMiddleware(configure)
            .withRouting(Routing::default().web(|| {
                Route::get("/", inspect)?;
                Route::get("/switch", switch)?;
                Route::get("/metadata", || {
                    let mut response = Response::from("metadata");
                    response.parts_mut().headers.insert("content-language", "it".parse().unwrap());
                    response.parts_mut().headers.append("vary", "Accept-Encoding".parse().unwrap());
                    response.parts_mut().headers.append("vary", "Origin".parse().unwrap());
                    response
                })?;
                Route::get("/star", || Response::from("star").header("vary".parse().unwrap(), "*".parse().unwrap()))?;
                Route::get("/existing", || Response::from("existing").header("vary".parse().unwrap(), "Origin, accept-language".parse().unwrap()))?;
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
            assert_eq!(App::currentLocale().unwrap(), *locale);
            assert_eq!(Lang::getLocale().unwrap(), *locale);
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
            App::currentLocale().unwrap(),
            Lang::get("messages.hello", &BTreeMap::new(), None).unwrap()
        )))
    })
}
fn switch(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        App::setLocale("fr").await.unwrap();
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
    middleware.setLocale(&["de", "fr", "en"]).unwrap();
}

#[tokio::test]
async fn default_stack_detects_languages_without_application_registration() {
    let root = Root::new();
    let events = Arc::new(Events::default());
    let kernel = root.kernel("en", |_| {}, events.clone()).await;
    for (header, expected) in [
        ("zh-CN", "zh-Hans|你好"),
        ("zh-TW", "zh-Hant|您好"),
        ("de", "en|Hello"),
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
        ["zh-Hans", "zh-Hant", "en", "zh-Hant"]
    );
}

#[tokio::test]
async fn minimal_applications_without_locale_configuration_still_render() {
    let root = Root::new();
    let app = Application::configure(root.0.clone())
        .withRouting(Routing::default().web(|| Route::get("/", inspect)))
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
                middleware.setLocale(&[]).unwrap();
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
                middleware.setLocale(&["fr"]).unwrap();
                middleware.setLocale(&["DE", "fr", "en"]).unwrap();
                assert!(middleware.setLocale(&["it", "zh_CN"]).is_err());
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
                middleware.setLocale(&["en", "de-DE"]).unwrap();
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
                middleware.setLocale(&[]).unwrap();
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
