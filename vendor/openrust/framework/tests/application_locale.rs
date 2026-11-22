use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use openrust::{
    contracts::{
        container::Container as _,
        events::{Event as EventContract, Listener},
        foundation::Application as _,
        http::Kernel as _,
    },
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{Application, configuration::Routing, events::LocaleUpdated, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Config, Event, Lang, Route},
    },
    translation::TranslationError,
    view::view,
};

struct Root(PathBuf);
impl Root {
    fn new(locale: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "locale-tutorial-{}-{}",
            std::process::id(),
            openrust::support::Str::random(8)
        ));
        fs::create_dir_all(path.join("resources/views")).unwrap();
        fs::write(
            path.join(".env"),
            format!("APP_LOCALE={locale}\nAPP_FALLBACK_LOCALE=en\n"),
        )
        .unwrap();
        for (locale, greeting) in [
            ("en", "Welcome, :Name"),
            ("zh_CN", "你好，:name"),
            ("zh_TW", "您好，:name"),
        ] {
            fs::create_dir_all(path.join("lang").join(locale)).unwrap();
            fs::write(
                path.join("lang").join(locale).join("messages.json"),
                serde_json::json!({"welcome":greeting}).to_string(),
            )
            .unwrap();
        }
        fs::write(
            path.join("lang/en/messages.json"),
            r#"{"welcome":"Welcome, :Name","fallback":"English fallback"}"#,
        )
        .unwrap();
        fs::write(
            path.join("lang/zh_CN.json"),
            r#"{"I love programming.":"我喜欢编程。"}"#,
        )
        .unwrap();
        fs::write(path.join("resources/views/greeting.blade.html"), "{{ __('messages.welcome', ['name' => $name]) }}|{{ trans('messages.fallback') }}|{{ __('I love programming.') }}|{{ __('missing.key') }}").unwrap();
        Self(path)
    }
    async fn kernel(&self, recorder: Arc<Recorder>) -> Kernel {
        let application = Application::configure(self.0.clone())
            .withConfig(|_, env| {
                Ok(BTreeMap::from([(
                    "app".into(),
                    serde_json::json!({
                        "locale": env.get_utf8("APP_LOCALE")?.unwrap_or("en"),
                        "fallback_locale": env.get_utf8("APP_FALLBACK_LOCALE")?.unwrap_or("en")
                    }),
                )]))
            })
            .withProviders(vec![Box::new(Observer(recorder))])
            .withRouting(Routing::default().web(|| {
                Route::get("/default", defaults)?;
                Route::get("/invalid", invalid)?;
                Route::get("/parent", parent_override)?;
                Route::get("/greeting/{locale}", greeting)?;
                Ok(())
            }))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
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
struct Recorder(Mutex<Vec<LocaleUpdated>>);
impl Listener for Recorder {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventContract,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event.downcast_ref::<LocaleUpdated>().unwrap();
            // 与上游测试一致：监听器看到的配置和译器必须已完成切换。
            assert_eq!(App::currentLocale().unwrap(), event.locale);
            assert_eq!(Config::string("app.locale").unwrap(), event.locale);
            assert_eq!(Lang::getLocale().unwrap(), event.locale);
            self.0.lock().unwrap().push(event.clone());
            Ok(None)
        })
    }
}
struct Observer(Arc<Recorder>);
impl ServiceProvider for Observer {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            Event::listen(LocaleUpdated::NAME, self.0.clone(), Delivery::Sync)?;
            Ok(())
        })
    }
}
fn error(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
fn defaults(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let locale = App::currentLocale().map_err(error)?;
        assert_eq!(App::getLocale().unwrap(), locale);
        assert!(App::isLocale(&locale).unwrap());
        assert_eq!(Config::string("app.locale").unwrap(), locale);
        assert_eq!(Lang::getLocale().unwrap(), locale);
        assert_eq!(App::getFallbackLocale().unwrap(), "en");
        Response::json(StatusCode::OK, serde_json::json!({"locale":locale}))
    })
}
fn invalid(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let locale = App::currentLocale().unwrap();
        assert!(matches!(
            App::setLocale("../bad").await,
            Err(TranslationError::InvalidLocale)
        ));
        assert!(matches!(
            App::setFallbackLocale("../bad"),
            Err(TranslationError::InvalidLocale)
        ));
        assert_eq!(App::currentLocale().unwrap(), locale);
        assert_eq!(Config::string("app.locale").unwrap(), locale);
        assert_eq!(Lang::getLocale().unwrap(), locale);
        assert_eq!(App::getFallbackLocale().unwrap(), "en");
        Ok(Response::from("unchanged"))
    })
}
fn greeting(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let locale = request.route("locale").unwrap().to_owned();
    Box::pin(async move {
        if !["en", "zh_CN", "zh_TW"].contains(&locale.as_str()) {
            return Err(openrust::http::abort(
                StatusCode::BAD_REQUEST,
                "不支持的语言。",
                Default::default(),
            ));
        }
        App::setLocale(&locale).await.map_err(error)?;
        tokio::task::yield_now().await;
        assert!(App::isLocale(&locale).unwrap());
        if locale == "zh_CN" {
            assert!(App::isLocale("zh-CN").unwrap());
            assert!(App::isLocale("zh-Hans").unwrap());
            assert!(!App::isLocale("zh_TW").unwrap());
            assert!(App::isLocale("../bad").is_err());
        }
        assert_eq!(Config::array("app").unwrap()["locale"], locale);
        assert_eq!(Config::all().unwrap()["app"]["locale"], locale);
        App::setFallbackLocale("en").map_err(error)?;
        assert_eq!(Lang::getFallback().unwrap(), "en");
        App::setFallbackLocale("zh_TW").map_err(error)?;
        assert_eq!(App::getFallbackLocale().unwrap(), "zh_TW");
        assert_eq!(Config::string("app.fallback_locale").unwrap(), "zh_TW");
        assert_eq!(Lang::getFallback().unwrap(), "zh_TW");
        App::setFallbackLocale("en").map_err(error)?;
        Response::view(
            StatusCode::OK,
            view("greeting").with("name", "ada"),
            Default::default(),
            request,
        )
        .await
    })
}
async fn send(kernel: &Kernel, path: &str, accept: &str) -> String {
    let raw = axum::extract::Request::builder()
        .uri(path)
        .header("accept-language", accept)
        .body(Body::empty())
        .unwrap();
    let (parts, body) = raw.into_parts();
    let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
    assert_eq!(
        response.status(),
        if path == "/greeting/de" {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        }
    );
    String::from_utf8(
        axum::body::to_bytes(response.into_parts().1, 16384)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn official_tutorial_configuration_and_app_facade_work_without_browser_middleware() {
    let root = Root::new("en");
    let events = Arc::new(Recorder::default());
    let kernel = root.kernel(events.clone()).await;
    assert_eq!(
        send(&kernel, "/default", "zh-TW").await,
        r#"{"locale":"en"}"#
    );
    assert_eq!(
        send(&kernel, "/greeting/zh_CN", "en").await,
        "你好，ada|English fallback|我喜欢编程。|missing.key"
    );
    assert_eq!(
        send(&kernel, "/default", "zh-TW").await,
        r#"{"locale":"en"}"#
    );
    assert_eq!(send(&kernel, "/invalid", "en").await, "unchanged");
    send(&kernel, "/greeting/de", "en").await;
    let recorded = events.0.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].locale, "zh_CN");
    assert_eq!(recorded[0].previousLocale.as_deref(), Some("en"));
}

#[tokio::test]
async fn tutorial_language_switches_are_isolated_between_requests_and_applications() {
    let en = Root::new("en");
    let tw = Root::new("zh_TW");
    let events = Arc::new(Recorder::default());
    let (one, two) = tokio::join!(en.kernel(events.clone()), tw.kernel(Arc::default()));
    for _ in 0..8 {
        let (cn, en, unchanged) = tokio::join!(
            send(&one, "/greeting/zh_CN", "en"),
            send(&one, "/greeting/en", "zh-CN"),
            send(&two, "/default", "zh-CN")
        );
        assert!(cn.starts_with("你好，ada"));
        assert!(en.starts_with("Welcome, Ada"));
        assert_eq!(unchanged, r#"{"locale":"zh_TW"}"#);
    }
    assert_eq!(send(&one, "/default", "zh-CN").await, r#"{"locale":"en"}"#);
    assert!(
        events
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|event| event.previousLocale.as_deref() == Some("en"))
    );
}

#[tokio::test]
async fn application_instance_and_facade_share_locale_and_fallback_state() {
    let root = Root::new("en");
    let application = Application::configure(root.0.clone()).create().unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    let application = kernel.get_application();
    application.setLocale("zh_CN").await.unwrap();
    assert_eq!(application.currentLocale().unwrap(), "zh_CN");
    application.setFallbackLocale("zh_TW").unwrap();
    assert_eq!(application.getFallbackLocale().unwrap(), "zh_TW");
    let translator = application
        .container()
        .get::<openrust::translation::Translator>("translator")
        .unwrap();
    assert_eq!(translator.getLocale(), "zh_CN");
    assert_eq!(translator.getFallback(), "zh_TW");
    application
        .container()
        .scope(async {
            application.setLocale("en").await.unwrap();
            application.setFallbackLocale("en").unwrap();
            assert_eq!(application.currentLocale().unwrap(), "en");
            assert_eq!(translator.getLocale(), "en");
        })
        .await;
    assert_eq!(application.currentLocale().unwrap(), "zh_CN");
    assert_eq!(application.getFallbackLocale().unwrap(), "zh_TW");
    assert_eq!(translator.getLocale(), "zh_CN");
    assert_eq!(
        translator
            .string("messages.welcome", &BTreeMap::new(), None)
            .unwrap(),
        "你好，:name"
    );
}

fn parent_override(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        App::setLocale("zh-CN").await.map_err(error)?;
        assert!(App::isLocale("zh-CN").unwrap());
        assert!(App::isLocale("zh_CN").unwrap());
        assert!(!App::isLocale("zh_TW").unwrap());
        assert!(App::isLocale("../bad").is_err());
        Config::set(
            "app",
            serde_json::json!({"locale":"zh_TW", "fallback_locale":"en", "name":"local"}),
        )
        .unwrap();
        assert_eq!(Config::string("app.locale").unwrap(), "zh_TW");
        assert_eq!(App::currentLocale().unwrap(), "zh_TW");
        assert_eq!(Config::array("app").unwrap()["locale"], "zh_TW");
        assert_eq!(Config::all().unwrap()["app"]["locale"], "zh_TW");
        App::setLocale("zh_TW").await.map_err(error)?;
        Config::set("app.name", "changed".into()).unwrap();
        assert_eq!(Config::string("app.name").unwrap(), "changed");
        Config::set("app", serde_json::json!({"name":"only"})).unwrap();
        assert!(!Config::has("app.locale").unwrap());
        assert!(Config::get("app.fallback_locale").unwrap().is_none());
        assert_eq!(
            Config::all().unwrap()["app"],
            serde_json::json!({"name":"only"})
        );
        App::setLocale("en").await.map_err(error)?;
        assert_eq!(Config::array("app").unwrap()["locale"], "en");
        App::setLocale("zh-CN").await.map_err(error)?;
        assert_eq!(App::currentLocale().unwrap(), "zh_CN");
        assert!(App::isLocale("zh-CN").unwrap());
        assert!(App::isLocale("zh_CN").unwrap());
        assert!(!App::isLocale("zh_TW").unwrap());
        Config::push("app.candidates", "en".into()).unwrap();
        Config::prepend("app.candidates", "zh_CN".into()).unwrap();
        assert_eq!(
            Config::array("app.candidates").unwrap(),
            serde_json::json!(["zh_CN", "en"])
        );
        Ok(Response::from("consistent"))
    })
}

#[tokio::test]
async fn language_aliases_and_parent_configuration_replacement_remain_consistent() {
    let root = Root::new("en");
    let kernel = root.kernel(Arc::default()).await;
    assert_eq!(send(&kernel, "/parent", "en").await, "consistent");
    assert_eq!(send(&kernel, "/default", "en").await, r#"{"locale":"en"}"#);
}
