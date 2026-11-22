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
            ("zh-Hans", "你好，:name"),
            ("zh-Hant", "您好，:name"),
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
            path.join("lang/zh-Hans.json"),
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
            .withMiddleware(|middleware| {
                middleware.setLocale(&[]).unwrap();
            })
            .withProviders(vec![Box::new(Observer(recorder))])
            .withRouting(Routing::default().web(|| {
                Route::get("/default", defaults)?;
                Route::get("/invalid", invalid)?;
                Route::get("/parent", parent_override)?;
                Route::get("/first-config", first_config)?;
                Route::get("/listener-failure", listener_failure)?;
                Route::get("/same", same_locale)?;
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
            Event::listen(
                LocaleUpdated::NAME,
                Arc::new(FailingListener),
                Delivery::Sync,
            )?;
            Ok(())
        })
    }
}
fn defaults(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let locale = App::currentLocale()?;
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
        if !["en", "zh-Hans", "zh-Hant"].contains(&locale.as_str()) {
            return Err(openrust::http::abort(
                StatusCode::BAD_REQUEST,
                "不支持的语言。",
                Default::default(),
            ));
        }
        App::setLocale(&locale).await?;
        tokio::task::yield_now().await;
        assert!(App::isLocale(&locale).unwrap());
        if locale == "zh-Hans" {
            assert!(!App::isLocale("zh-CN").unwrap());
            assert!(App::isLocale("zh-Hans").unwrap());
            assert!(!App::isLocale("zh-Hant").unwrap());
            assert!(App::isLocale("../bad").is_err());
        }
        assert_eq!(Config::array("app").unwrap()["locale"], locale);
        assert_eq!(Config::all().unwrap()["app"]["locale"], locale);
        App::setFallbackLocale("en")?;
        assert_eq!(Lang::getFallback().unwrap(), "en");
        App::setFallbackLocale("zh-Hant")?;
        assert_eq!(App::getFallbackLocale().unwrap(), "zh-Hant");
        assert_eq!(Config::string("app.fallback_locale").unwrap(), "zh-Hant");
        assert_eq!(Lang::getFallback().unwrap(), "zh-Hant");
        App::setFallbackLocale("en")?;
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
        } else if path == "/listener-failure" {
            StatusCode::INTERNAL_SERVER_ERROR
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
        send(&kernel, "/greeting/zh-Hans", "en").await,
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
    assert_eq!(recorded[0].locale, "zh-Hans");
    assert_eq!(recorded[0].previousLocale.as_deref(), Some("en"));
}

#[tokio::test]
async fn tutorial_language_switches_are_isolated_between_requests_and_applications() {
    let en = Root::new("en");
    let tw = Root::new("zh-Hant");
    let events = Arc::new(Recorder::default());
    let (one, two) = tokio::join!(en.kernel(events.clone()), tw.kernel(Arc::default()));
    for _ in 0..8 {
        let (cn, en, unchanged) = tokio::join!(
            send(&one, "/greeting/zh-Hans", "en"),
            send(&one, "/greeting/en", "zh-CN"),
            send(&two, "/default", "zh-CN")
        );
        assert!(cn.starts_with("你好，ada"));
        assert!(en.starts_with("Welcome, Ada"));
        assert_eq!(unchanged, r#"{"locale":"zh-Hant"}"#);
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
    application.setLocale("zh-Hans").await.unwrap();
    assert_eq!(application.currentLocale().unwrap(), "zh-Hans");
    application.setFallbackLocale("zh-Hant").unwrap();
    assert_eq!(application.getFallbackLocale().unwrap(), "zh-Hant");
    let translator = application
        .container()
        .get::<openrust::translation::Translator>("translator")
        .unwrap();
    assert_eq!(translator.getLocale(), "zh-Hans");
    assert_eq!(translator.getFallback(), "zh-Hant");
    application
        .container()
        .scope(async {
            application.setLocale("en").await.unwrap();
            application.setFallbackLocale("en").unwrap();
            assert_eq!(application.currentLocale().unwrap(), "en");
            assert_eq!(translator.getLocale(), "en");
        })
        .await;
    assert_eq!(application.currentLocale().unwrap(), "zh-Hans");
    assert_eq!(application.getFallbackLocale().unwrap(), "zh-Hant");
    assert_eq!(translator.getLocale(), "zh-Hans");
    assert_eq!(
        translator
            .string("messages.welcome", &BTreeMap::new(), None)
            .unwrap(),
        "你好，:name"
    );
}

fn parent_override(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        App::setLocale("ZH-cn").await?;
        assert!(App::isLocale("zh-cn").unwrap());
        assert!(!App::isLocale("zh-Hans").unwrap());
        assert!(!App::isLocale("zh-Hant").unwrap());
        assert!(App::isLocale("../bad").is_err());
        Config::set(
            "app",
            serde_json::json!({"locale":"zh-Hant", "fallback_locale":"en", "name":"local"}),
        )
        .unwrap();
        assert_eq!(Config::string("app.locale").unwrap(), "zh-Hant");
        assert_eq!(App::currentLocale().unwrap(), "zh-Hant");
        assert_eq!(Config::array("app").unwrap()["locale"], "zh-Hant");
        assert_eq!(Config::all().unwrap()["app"]["locale"], "zh-Hant");
        App::setLocale("zh-Hant").await?;
        Config::set("app.name", "changed".into()).unwrap();
        assert_eq!(Config::string("app.name").unwrap(), "changed");
        Config::set("app", serde_json::json!({"name":"only"})).unwrap();
        assert!(!Config::has("app.locale").unwrap());
        assert!(Config::get("app.fallback_locale").unwrap().is_none());
        assert_eq!(
            Config::all().unwrap()["app"],
            serde_json::json!({"name":"only"})
        );
        App::setLocale("en").await?;
        assert_eq!(Config::array("app").unwrap()["locale"], "en");
        App::setLocale("ZH-cn").await?;
        assert_eq!(App::currentLocale().unwrap(), "zh-CN");
        assert!(App::isLocale("zh-cn").unwrap());
        assert!(!App::isLocale("zh-Hans").unwrap());
        assert!(!App::isLocale("zh-Hant").unwrap());
        Config::push("app.candidates", "en".into()).unwrap();
        Config::prepend("app.candidates", "zh-Hans".into()).unwrap();
        assert_eq!(
            Config::array("app.candidates").unwrap(),
            serde_json::json!(["zh-Hans", "en"])
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

struct FailingListener;
impl Listener for FailingListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventContract,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if event.downcast_ref::<LocaleUpdated>().unwrap().locale == "fr" {
                tokio::task::yield_now().await;
                assert_eq!(App::currentLocale().unwrap(), "fr");
                assert_eq!(Lang::getLocale().unwrap(), "fr");
                return Err(openrust::events::EventError::Listener {
                    source: Box::new(std::io::Error::other("语言监听器测试失败")),
                });
            }
            Ok(None)
        })
    }
}
fn listener_failure(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let failure = App::setLocale("fr").await.unwrap_err();
        assert!(failure.to_string().contains("event listener failed"));
        let source = std::error::Error::source(&failure).unwrap();
        let event = source
            .downcast_ref::<openrust::events::EventError>()
            .unwrap();
        assert!(
            matches!(event, openrust::events::EventError::Listener { source } if source.to_string() == "语言监听器测试失败")
        );
        // 错误在更新后传播，不回滚本次作用域中的语言。
        assert_eq!(Config::string("app.locale").unwrap(), "fr");
        assert_eq!(Lang::getLocale().unwrap(), "fr");
        Err(failure.into())
    })
}
fn same_locale(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        App::setLocale("en").await?;
        App::setLocale("en").await?;
        Ok(Response::from("twice"))
    })
}
#[tokio::test]
async fn locale_events_include_unchanged_sets_and_propagate_listener_failures_after_update() {
    let root = Root::new("en");
    let events = Arc::new(Recorder::default());
    let kernel = root.kernel(events.clone()).await;
    assert_eq!(send(&kernel, "/same", "en").await, "twice");
    send(&kernel, "/listener-failure", "en").await;
    assert_eq!(send(&kernel, "/default", "en").await, r#"{"locale":"en"}"#);
    let recorded = events.0.lock().unwrap();
    assert_eq!(recorded.len(), 3);
    assert_eq!(
        recorded
            .iter()
            .map(|event| event.locale.as_str())
            .collect::<Vec<_>>(),
        ["en", "en", "fr"]
    );
    assert!(
        recorded
            .iter()
            .all(|event| event.previousLocale.as_deref() == Some("en"))
    );
}

#[tokio::test]
async fn unavailable_locale_services_fail_at_the_upstream_update_step() {
    let root = Root::new("en");
    let kernel = root.kernel(Arc::default()).await;
    let application = kernel.get_application();
    let translator = application
        .container()
        .get::<openrust::translation::Translator>("translator")
        .unwrap();
    application
        .container()
        .instanceNamed("events", Arc::new("invalid event service".to_owned()))
        .unwrap();
    assert!(matches!(
        application.setLocale("zh-Hant").await,
        Err(TranslationError::Unavailable(_))
    ));
    assert_eq!(application.getLocale().unwrap(), "zh-Hant");
    assert_eq!(translator.getLocale(), "zh-Hant");

    let other = Root::new("en");
    let kernel = other.kernel(Arc::default()).await;
    let application = kernel.get_application();
    application
        .container()
        .instanceNamed("translator", Arc::new("invalid translator".to_owned()))
        .unwrap();
    assert!(matches!(
        application.setLocale("zh-Hans").await,
        Err(TranslationError::Unavailable(_))
    ));
    assert_eq!(application.getLocale().unwrap(), "zh-Hans");
    assert!(matches!(
        application.setFallbackLocale("zh-Hant"),
        Err(TranslationError::Unavailable(_))
    ));
    assert_eq!(application.getFallbackLocale().unwrap(), "zh-Hant");
}

fn first_config(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Config::set(
            "app",
            serde_json::json!({"locale":"zh-Hant", "fallback_locale":"zh-Hans"}),
        )
        .unwrap();
        assert_eq!(Lang::getLocale().unwrap(), "zh-Hant");
        assert_eq!(Lang::getFallback().unwrap(), "zh-Hans");
        assert_eq!(
            Lang::get("messages.welcome", &BTreeMap::new(), None).unwrap(),
            "您好，:name"
        );
        Ok(Response::from("scoped"))
    })
}

#[tokio::test]
async fn first_translation_after_scoped_configuration_does_not_replace_application_defaults() {
    let root = Root::new("en");
    let kernel = root.kernel(Arc::default()).await;
    assert_eq!(send(&kernel, "/first-config", "en").await, "scoped");
    assert_eq!(send(&kernel, "/default", "en").await, r#"{"locale":"en"}"#);
    let application = kernel.get_application();
    let translator = application
        .container()
        .get::<openrust::translation::Translator>("translator")
        .unwrap();
    assert_eq!(translator.getLocale(), "en");
    assert_eq!(translator.getFallback(), "en");

    // 队列等任务共用相同容器生命周期，首次解析同样不能污染默认值。
    let task_root = Root::new("en");
    let task_kernel = task_root.kernel(Arc::default()).await;
    let application = task_kernel.get_application();
    application
        .container()
        .scope(async {
            application
                .config()
                .unwrap()
                .set(
                    "app",
                    serde_json::json!({"locale":"zh-Hans", "fallback_locale":"zh-Hant"}),
                )
                .unwrap();
            let translator = application
                .container()
                .get::<openrust::translation::Translator>("translator")
                .unwrap();
            assert_eq!(translator.getLocale(), "zh-Hans");
            assert_eq!(translator.getFallback(), "zh-Hant");
            tokio::task::yield_now().await;
            assert_eq!(translator.getLocale(), "zh-Hans");
        })
        .await;
    assert_eq!(application.getLocale().unwrap(), "en");
    let translator = application
        .container()
        .get::<openrust::translation::Translator>("translator")
        .unwrap();
    assert_eq!(translator.getLocale(), "en");
    assert_eq!(translator.getFallback(), "en");
}
