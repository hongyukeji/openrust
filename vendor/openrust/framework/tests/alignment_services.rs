#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
use openrust::{
    contracts::{
        container::Container as _,
        filesystem::{Factory as FsFactory, Filesystem},
        foundation::Application as _,
    },
    events::{Delivery, Dispatcher, EventContext},
    foundation::Application,
    log::{LogContext, LogManager},
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn builder_bindings_and_hooks_follow_provider_lifecycle_and_scopes() {
    let root = tempfile::tempdir().unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let transient = Arc::new(AtomicUsize::new(0));
    let counter = transient.clone();
    let builder = Application::configure(root.path().to_owned())
        .withBindings::<usize, _>(move |_| Ok(Arc::new(counter.fetch_add(1, Ordering::SeqCst))))
        .withSingletons::<String, _>(|_| Ok(Arc::new("shared".into())))
        .withScopedSingletons::<AtomicUsize, _>(|_| Ok(Arc::new(AtomicUsize::new(0))));
    let o = order.clone();
    let builder = builder.registered(move |app| {
        assert!(app.container().make::<String>().is_ok());
        o.lock().unwrap().push("registered");
        Ok(())
    });
    let o = order.clone();
    let builder = builder.booting(move |_| {
        o.lock().unwrap().push("booting");
        Ok(())
    });
    let o = order.clone();
    let mut app = builder
        .booted(move |_| {
            o.lock().unwrap().push("booted");
            Ok(())
        })
        .create()
        .unwrap();
    assert!(order.lock().unwrap().is_empty());
    app.boot().await.unwrap();
    app.boot().await.unwrap();
    assert_eq!(*order.lock().unwrap(), ["registered", "booting", "booted"]);
    let c = app.container();
    assert_ne!(*c.make::<usize>().unwrap(), *c.make::<usize>().unwrap());
    assert!(Arc::ptr_eq(
        &c.make::<String>().unwrap(),
        &c.make::<String>().unwrap()
    ));
    c.scope(async {
        let v = c.make::<AtomicUsize>().unwrap();
        v.store(7, Ordering::SeqCst);
        assert_eq!(c.make::<AtomicUsize>().unwrap().load(Ordering::SeqCst), 7);
    })
    .await;
    c.scope(async {
        assert_eq!(c.make::<AtomicUsize>().unwrap().load(Ordering::SeqCst), 0);
    })
    .await;
}

#[tokio::test]
async fn filesystem_contracts_resolve_real_adapter_and_stream_and_copy_consumers() {
    let root = tempfile::tempdir().unwrap();
    let mut app=Application::configure(root.path().to_owned()).withConfig(|_,_| Ok([("filesystems".into(),json!({"default":"local","cloud":"local","disks":{"local":{"driver":"local","root":"data"}}}))].into())).create().unwrap();
    app.boot().await.unwrap();
    let factory = app.container().make::<dyn FsFactory>().unwrap();
    let filesystem = app.container().make::<dyn Filesystem>().unwrap();
    let disk = factory.disk(None).await.unwrap();
    filesystem
        .put("data.json", b"{\"n\":1}", &Default::default())
        .await
        .unwrap();
    assert_eq!(disk.json("data.json").await.unwrap(), Some(json!({"n":1})));
    assert!(filesystem.copy("data.json", "copy.json").await.unwrap());
    assert!(filesystem.r#move("copy.json", "move.json").await.unwrap());
    assert!(!filesystem.exists("copy.json").await.unwrap());
    assert!(disk.r#move("move.json", "move.json").await.unwrap());
    assert!(filesystem.exists("move.json").await.unwrap());
    disk.put("empty", b"").await.unwrap();
    disk.append("empty", "a", "|").await.unwrap();
    disk.prepend("empty", "b", "|").await.unwrap();
    assert_eq!(disk.get("empty").await.unwrap().unwrap(), b"b||a");
    assert!(!disk.copy("missing", "out").await.unwrap());
    assert!(disk.copy("../bad", "out").await.is_err());
    let mut stream = filesystem.read_stream("data.json").await.unwrap().unwrap();
    filesystem
        .write_stream("stream.json", stream.as_mut(), &Default::default())
        .await
        .unwrap();
    assert_eq!(
        disk.json("stream.json").await.unwrap(),
        Some(json!({"n":1}))
    );
    app.call(
        openrust::container::Callback::new(&[], move |_| async move {
            use openrust::support::facades::Storage;
            let mut input = Storage::readStream("stream.json").await.unwrap().unwrap();
            assert!(
                Storage::writeStream("facade.json", &mut input)
                    .await
                    .unwrap()
            );
            assert_eq!(
                Storage::json("facade.json").await.unwrap(),
                Some(json!({"n":1}))
            );
            assert!(Storage::readStream("missing").await.unwrap().is_none());
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
    disk.put("invalid.json", b"bad").await.unwrap();
    assert_eq!(disk.json("invalid.json").await.unwrap(), None);
    assert!(
        app.container()
            .make::<dyn openrust::contracts::filesystem::Cloud>()
            .is_ok()
    );
}

#[test]
fn views_render_conditions_each_empty_and_shared_data_through_blade() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("row.blade.html"),
        "{{ $key }}={{ $item }};{{ $shared }}",
    )
    .unwrap();
    std::fs::write(root.path().join("empty.blade.html"), "empty {{ $shared }}").unwrap();
    let views = openrust::view::Factory::new(
        openrust::view::FileViewFinder::new([root.path().to_path_buf()]),
        root.path().join("compiled"),
    );
    views.share("shared", "!");
    assert_eq!(views.getShared()["shared"].as_str(), Some("!"));
    assert_eq!(
        views
            .renderWhen(false, "missing", &Default::default())
            .unwrap(),
        ""
    );
    assert_eq!(
        views
            .renderUnless(true, "missing", &Default::default())
            .unwrap(),
        ""
    );
    assert!(
        views
            .renderWhen(true, "missing", &Default::default())
            .is_err()
    );
    assert_eq!(
        views
            .renderEach(
                "row",
                [("x".into(), "A".into()), (2.into(), "B".into())],
                "item",
                "raw|none"
            )
            .unwrap(),
        "x=A;!2=B;!"
    );
    assert_eq!(
        views.renderEach("row", [], "item", "raw|none").unwrap(),
        "none"
    );
    assert_eq!(
        views.renderEach("row", [], "item", "empty").unwrap(),
        "empty !"
    );
}

#[tokio::test]
async fn pushed_events_repeat_flush_forget_and_obey_fake_selection() {
    let dispatcher = Dispatcher::new();
    let calls = Arc::new(AtomicUsize::new(0));
    dispatcher
        .register(|registry| {
            registry.listen(
                "audit",
                Arc::new(CountListener(calls.clone())),
                Delivery::Sync,
            )
        })
        .unwrap();
    dispatcher.push(Arc::new(NamedEvent::new("audit"))).unwrap();
    dispatcher.push(Arc::new(NamedEvent::new("audit"))).unwrap();
    dispatcher
        .flush("audit", &mut EventContext::new())
        .await
        .unwrap();
    dispatcher
        .flush("audit", &mut EventContext::new())
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let fake = dispatcher.fake(&[]);
    dispatcher
        .flush("audit", &mut EventContext::new())
        .await
        .unwrap();
    fake.assertDispatchedTimes("audit_pushed", 1);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    dispatcher.forgetPushed().unwrap();
}

#[test]
fn log_context_keeps_shared_defaults_and_resolved_channel_context_separate() {
    let root = tempfile::tempdir().unwrap();
    let manager=LogManager::new(Arc::new(openrust::config::Repository::new([("logging".into(),json!({"default":"one","channels":{"one":{"driver":"single","path":"one.log"},"two":{"driver":"single","path":"two.log"},"three":{"driver":"single","path":"three.log"}}}))].into())),root.path());
    let records = Arc::new(Mutex::new(Vec::new()));
    let r = records.clone();
    manager
        .listen(move |event| {
            r.lock().unwrap().push(event.context.clone());
            Ok(())
        })
        .unwrap();
    let one = manager.channel(Some("one")).unwrap();
    manager.shareContext([("id".into(), json!(1))].into_iter().collect());
    let two = manager.channel(Some("two")).unwrap();
    one.withContext([("local".into(), json!(true))].into_iter().collect());
    one.info("one", &[("id".into(), json!(2))].into_iter().collect())
        .unwrap();
    two.info("two", &LogContext::new()).unwrap();
    manager.flushSharedContext();
    one.info("retained", &LogContext::new()).unwrap();
    manager
        .channel(Some("three"))
        .unwrap()
        .info("future", &LogContext::new())
        .unwrap();
    manager.withoutContext(Some(&["id"]));
    two.info("cleared", &LogContext::new()).unwrap();
    let r = records.lock().unwrap();
    assert_eq!(r[0]["id"], 2);
    assert_eq!(r[0]["local"], true);
    assert_eq!(r[1]["id"], 1);
    assert!(!r[1].contains_key("local"));
    assert_eq!(r[2]["id"], 1);
    assert!(r[3].is_empty());
    assert!(r[4].is_empty());
    drop(r);
    manager.shareContext([("stack".into(), json!(true))].into_iter().collect());
    manager
        .stack(&["one", "two"], Some("audit-stack"))
        .unwrap()
        .info("stack", &LogContext::new())
        .unwrap();
    manager
        .build(&json!({"driver":"single","path":"ondemand.log"}))
        .unwrap()
        .info("ondemand", &LogContext::new())
        .unwrap();
    let records = records.lock().unwrap();
    assert_eq!(records[5]["stack"], true);
    assert_eq!(records[6]["stack"], true);
}

struct NamedEvent(&'static str);
impl NamedEvent {
    fn new(name: &'static str) -> Self {
        Self(name)
    }
}
impl openrust::contracts::events::Event for NamedEvent {
    fn name(&self) -> &str {
        self.0
    }
}

#[tokio::test]
async fn translation_contract_names_observe_nested_locales_and_fallback() {
    use openrust::{
        contracts::translation::Translator as Contract,
        translation::{FileLoader, Translator},
    };
    let root = tempfile::tempdir().unwrap();
    let mut translator = Translator::new(
        FileLoader::new(root.path().to_owned()),
        "en".into(),
        "en".into(),
    )
    .unwrap();
    {
        let contract: &mut dyn Contract = &mut translator;
        contract.setLocale("zh-CN").unwrap();
        contract.setFallback("en").unwrap();
        assert_eq!(contract.getLocale(), "zh-CN");
        assert_eq!(contract.getFallback(), "en");
    }
    translator
        .withLocale("fr", async {
            assert_eq!(Contract::getLocale(&translator), "fr");
            translator
                .withLocale("de", async {
                    assert_eq!(translator.getLocale(), "de");
                })
                .await
                .unwrap();
            assert_eq!(translator.getLocale(), "fr");
        })
        .await
        .unwrap();
    assert_eq!(translator.getLocale(), "zh-CN");
    let (one, two) = tokio::join!(
        translator.withLocale("en", async {
            tokio::task::yield_now().await;
            translator.getLocale()
        }),
        translator.withLocale("fr", async {
            tokio::task::yield_now().await;
            translator.getLocale()
        })
    );
    assert_eq!(one.unwrap(), "en");
    assert_eq!(two.unwrap(), "fr");
}

#[tokio::test]
async fn url_policies_are_owned_by_each_generator_and_affect_named_and_signed_urls() {
    use openrust::{
        contracts::routing::Registrar as _, contracts::routing::UrlGenerator as _,
        foundation::configuration::Routing, http::StatusCode, routing::UrlGenerator,
    };
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .withConfig(|_, _| {
            Ok([(
                "app".into(),
                json!({"url":"http://example.test/base","key":"signing"}),
            )]
            .into())
        })
        .withRouting(Routing::new(|_| {
            let mut router = openrust::routing::Router::new();
            router
                .add_route(
                    &[openrust::http::Method::GET],
                    "/items/{id}",
                    Arc::new(OkHandler),
                )?
                .name("items.show");
            router.group(
                openrust::routing::RouteGroup::new().domain("{tenant}.example.test"),
                |router| {
                    router
                        .add_route(
                            &[openrust::http::Method::GET],
                            "/tenant/{id}",
                            Arc::new(OkHandler),
                        )?
                        .name("tenant.show");
                    Ok(())
                },
            )?;
            Ok(router)
        }))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let original = app.container().make::<UrlGenerator>().unwrap();
    let mut urls = (*original).clone();
    urls.forceRootUrl(Some("http://forced.test"));
    urls.forceHttps(true);
    assert_eq!(urls.to("/a", &[], None).unwrap(), "https://forced.test/a");
    assert_eq!(
        original.to("/a", &[], None).unwrap(),
        "http://example.test/base/a"
    );
    urls.formatPathUsing(|path, _| format!("/prefix{path}"));
    assert_eq!(
        urls.route(
            "items.show",
            &[openrust::routing::RouteParameter::named("id", "7")],
            true
        )
        .unwrap(),
        "https://forced.test/prefix/items/7"
    );
    let signed = urls
        .signedRoute(
            "items.show",
            &[openrust::routing::RouteParameter::named("id", "7")],
            None,
            openrust::routing::SignatureMode::Absolute,
        )
        .unwrap();
    let request = openrust::http::Request::from_parts(
        http::Request::get(&signed).body(()).unwrap().into_parts().0,
        openrust::http::Body::empty(),
    );
    assert!(
        urls.hasValidSignature(
            &request,
            openrust::routing::SignatureMode::Absolute,
            openrust::routing::SignatureQueryFilter::Keys(&[])
        )
        .unwrap()
    );
    let tampered = signed.replace("items/7", "items/8");
    let request = openrust::http::Request::from_parts(
        http::Request::get(tampered)
            .body(())
            .unwrap()
            .into_parts()
            .0,
        openrust::http::Body::empty(),
    );
    assert!(
        !urls
            .hasValidSignature(
                &request,
                openrust::routing::SignatureMode::Absolute,
                openrust::routing::SignatureQueryFilter::Keys(&[])
            )
            .unwrap()
    );
    assert_eq!(
        urls.route(
            "items.show",
            &[openrust::routing::RouteParameter::named("id", "a b")],
            false
        )
        .unwrap(),
        "/prefix/items/a%20b"
    );
    urls.formatHostUsing(|root, _| root.replace("example.test", "formatted.test"));
    assert_eq!(
        urls.route(
            "tenant.show",
            &[
                openrust::routing::RouteParameter::named("tenant", "team"),
                openrust::routing::RouteParameter::named("id", "a b")
            ],
            true
        )
        .unwrap(),
        "https://team.formatted.test/prefix/tenant/a%20b"
    );
    urls.forceHttps(false);
    assert!(urls.to("/a", &[], None).unwrap().starts_with("http://"));
    let first_request = openrust::http::Request::from_parts(
        http::Request::get("http://first.test/a")
            .body(())
            .unwrap()
            .into_parts()
            .0,
        openrust::http::Body::empty(),
    );
    let second_request = openrust::http::Request::from_parts(
        http::Request::get("http://second.test/a")
            .body(())
            .unwrap()
            .into_parts()
            .0,
        openrust::http::Body::empty(),
    );
    let mut first = original.forRequest(&first_request);
    let second = original.forRequest(&second_request);
    let (first, second) = tokio::join!(
        async move {
            first.forceHttps(true);
            tokio::task::yield_now().await;
            first.to("/value", &[], None).unwrap()
        },
        async move {
            tokio::task::yield_now().await;
            second.to("/value", &[], None).unwrap()
        }
    );
    assert_eq!(first, "https://first.test/value");
    assert_eq!(second, "http://second.test/value");
    assert_eq!(
        original.to("/value", &[], None).unwrap(),
        "http://example.test/base/value"
    );
    let _ = StatusCode::OK;
}

struct OkHandler;
impl openrust::contracts::routing::Handler for OkHandler {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::http::Request,
    ) -> openrust::http::HttpFuture<'a, Result<openrust::http::Response, openrust::http::HttpError>>
    {
        Box::pin(async { Ok(openrust::http::Response::from("ok")) })
    }
}

struct CountListener(Arc<AtomicUsize>);
impl openrust::contracts::events::Listener for CountListener {
    fn handle<'a>(
        &'a self,
        _: &'a dyn openrust::contracts::events::Event,
        _: &'a mut EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(None) })
    }
}

#[tokio::test]
#[ignore = "需要本批隔离 Redis 8；显式运行，缺配置必须失败"]
async fn redis_purge_rebuilds_named_connections_and_custom_connectors_keep_old_arcs_usable() {
    use openrust::redis::{RedisManager, connections::Connection};
    let url = std::env::var("ALIGNMENT_REDIS_URL").expect("缺 Redis 隔离连接");
    let config=Arc::new(openrust::config::Repository::new([("database".into(),json!({"redis":{"client":"audit","options":{"prefix":"alignment:"},"default":{"url":url},"second":{"url":url}}}))].into()));
    let manager = RedisManager::new(config);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    manager.extend("audit", move |config, prefix| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(Connection::new(
            redis::Client::open(config["url"].as_str().unwrap()).unwrap(),
            Default::default(),
            prefix.into(),
        ))
    });
    let events = Arc::new(Dispatcher::new());
    let event_count = Arc::new(AtomicUsize::new(0));
    events
        .register(|registry| {
            registry.listen(
                "OpenRust\\Redis\\Events\\CommandExecuted",
                Arc::new(CountListener(event_count.clone())),
                Delivery::Sync,
            )
        })
        .unwrap();
    manager.setEventDispatcher(events);
    manager.enableEvents();
    let old = manager.connection(None).unwrap();
    old.set("key", b"one").await.unwrap();
    assert_eq!(event_count.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&old, &manager.connection(None).unwrap()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    manager.purge(None);
    let new = manager.connection(None).unwrap();
    assert!(!Arc::ptr_eq(&old, &new));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(old.get("key").await.unwrap(), Some(b"one".to_vec()));
    new.set("key", b"two").await.unwrap();
    assert_eq!(old.get("key").await.unwrap(), Some(b"two".to_vec()));
    let second = manager.connection(Some("second")).unwrap();
    manager.purge(Some("second"));
    assert!(!Arc::ptr_eq(
        &second,
        &manager.connection(Some("second")).unwrap()
    ));
    assert!(manager.connection(Some("missing")).is_err());
    let before = event_count.load(Ordering::SeqCst);
    manager
        .connection(Some("second"))
        .unwrap()
        .get("key")
        .await
        .unwrap();
    assert_eq!(event_count.load(Ordering::SeqCst), before + 1);
    manager.disableEvents();
    old.get("key").await.unwrap();
    assert_eq!(event_count.load(Ordering::SeqCst), before + 1);
    let replacement = Arc::new(Dispatcher::new());
    let replacement_count = Arc::new(AtomicUsize::new(0));
    replacement
        .register(|registry| {
            registry.listen(
                "OpenRust\\Redis\\Events\\CommandExecuted",
                Arc::new(CountListener(replacement_count.clone())),
                Delivery::Sync,
            )
        })
        .unwrap();
    manager.setEventDispatcher(replacement);
    manager.enableEvents();
    old.get("key").await.unwrap();
    new.get("key").await.unwrap();
    assert_eq!(replacement_count.load(Ordering::SeqCst), 2);
    assert_eq!(event_count.load(Ordering::SeqCst), before + 1);
    manager.extend("audit", |_, _| {
        Err(openrust::redis::RedisError::InvalidConfiguration)
    });
    manager.purge(None);
    assert!(manager.connection(None).is_err());
}

#[tokio::test]
async fn builder_schedule_registers_lazily_and_long_task_thresholds_cover_success_failure_and_missing_runtime()
 {
    use openrust::{
        console::events::{ScheduledTaskFailed, ScheduledTaskFinished},
        container::Callback,
        contracts::events::Dispatcher as _,
        support::facades::{Event, Facade},
        testing::TestApp,
    };
    use std::time::Duration;
    let tasks = Arc::new(Mutex::new(Vec::new()));
    let captured = tasks.clone();
    let configured = Arc::new(AtomicUsize::new(0));
    let registered = configured.clone();
    let mut app = TestApp::create(|builder| {
        builder.withSchedule(move |schedule| {
            registered.fetch_add(1, Ordering::SeqCst);
            let captured = captured.clone();
            schedule
                .whenTaskRunsLongerThan(Duration::ZERO, move |task, runtime| {
                    captured.lock().unwrap().push((task.clone(), runtime));
                    Ok(())
                })
                .map_err(|error| openrust::console::ConsoleError::new(error.to_string()))?;
            schedule.exec("true", []).everyMinute();
            Ok(())
        })
    })
    .unwrap();
    app.boot().await.unwrap();
    assert_eq!(configured.load(Ordering::SeqCst), 0);
    app.artisan(["schedule:run"]).await.assertSuccessful();
    assert_eq!(configured.load(Ordering::SeqCst), 1);
    let task = tasks.lock().unwrap()[0].0.clone();
    let exact = Arc::new(AtomicUsize::new(0));
    let count = exact.clone();
    app.app()
        .call(
            Callback::new(&[], move |_| async move {
                let schedule = openrust::console::scheduling::Schedule::default();
                schedule
                    .whenTaskRunsLongerThan(Duration::from_secs(1), move |_, _| {
                        count.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .unwrap();
                let dispatcher = Event::getFacadeRoot().unwrap();
                for runtime in [
                    Duration::from_millis(500),
                    Duration::from_secs(1),
                    Duration::from_millis(1001),
                ] {
                    dispatcher
                        .dispatch(
                            Arc::new(ScheduledTaskFinished {
                                task: task.clone(),
                                runtime,
                            }),
                            &mut EventContext::new(),
                        )
                        .await
                        .unwrap();
                }
                for runtime in [
                    None,
                    Some(Duration::from_secs(1)),
                    Some(Duration::from_secs(2)),
                ] {
                    dispatcher
                        .dispatch(
                            Arc::new(ScheduledTaskFailed {
                                task: task.clone(),
                                runtime,
                                exception: Arc::new(std::io::Error::other("失败路径")),
                            }),
                            &mut EventContext::new(),
                        )
                        .await
                        .unwrap();
                }
                // 固定上游只监听 Finished / Failed；后台完成事件不能重复触发时长回调。
                dispatcher
                    .dispatch(
                        Arc::new(openrust::console::events::ScheduledBackgroundTaskFinished {
                            task: task.clone(),
                            exit_code: 0,
                        }),
                        &mut EventContext::new(),
                    )
                    .await
                    .unwrap();
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(exact.load(Ordering::SeqCst), 2);
}

struct AddressMail;
impl openrust::mail::Mailable for AddressMail {
    fn envelope(&self) -> openrust::mail::MailResult<openrust::mail::mailables::Envelope> {
        Ok(openrust::mail::mailables::Envelope {
            subject: "地址回归".into(),
            to: vec!["to@example.test".into()],
            cc: vec!["cc@example.test".into()],
            bcc: vec!["bcc@example.test".into()],
            ..Default::default()
        })
    }
    fn content(&self) -> openrust::mail::MailResult<openrust::mail::mailables::Content> {
        Ok(openrust::mail::mailables::Content {
            text: Some("消息".into()),
            ..Default::default()
        })
    }
}
#[tokio::test]
async fn mail_global_addresses_inherit_boot_defaults_without_leaking_between_requests() {
    use openrust::mail::MailManager;
    let root = tempfile::tempdir().unwrap();
    let mut app=Application::configure(root.path().to_owned()).withConfig(|_,_|Ok([("mail".into(),json!({"default":"one","from":{"address":"from@example.test"},"mailers":{"one":{"transport":"array"},"two":{"transport":"array"}}}))].into())).create().unwrap();
    app.boot().await.unwrap();
    let manager = app.container().make::<MailManager>().unwrap();
    let mailer = manager.mailer(None).unwrap();
    mailer.alwaysFrom("boot@example.test").unwrap();
    mailer.alwaysReplyTo("reply@example.test").unwrap();
    mailer.alwaysReturnPath("bounce@example.test").unwrap();
    let container = app.container();
    container
        .scope(async {
            mailer.alwaysTo("scope@example.test").unwrap();
            let sent = mailer.sendNow(&AddressMail).await.unwrap().unwrap();
            let envelope = sent.message.envelope();
            assert_eq!(envelope.from.as_ref().unwrap().address, "boot@example.test");
            assert_eq!(envelope.to[0].address, "scope@example.test");
            assert!(envelope.cc.is_empty() && envelope.bcc.is_empty());
            assert_eq!(envelope.reply_to[0].address, "reply@example.test");
            assert_eq!(envelope.return_path.as_deref(), Some("bounce@example.test"));
            assert!(
                String::from_utf8(sent.message.formatted())
                    .unwrap()
                    .contains("Return-Path: <bounce@example.test>")
            );
            assert_eq!(
                manager
                    .mailer(Some("two"))
                    .unwrap()
                    .sendNow(&AddressMail)
                    .await
                    .unwrap()
                    .unwrap()
                    .message
                    .envelope()
                    .to[0]
                    .address,
                "to@example.test"
            );
        })
        .await;
    container
        .scope(async {
            let sent = mailer.sendNow(&AddressMail).await.unwrap().unwrap();
            assert_eq!(sent.message.envelope().to[0].address, "to@example.test");
            assert_eq!(
                sent.message.envelope().from.as_ref().unwrap().address,
                "boot@example.test"
            );
            assert_eq!(sent.message.envelope().cc.len(), 1);
        })
        .await;
}

struct OrderedListener(&'static str, Arc<Mutex<Vec<&'static str>>>);
impl openrust::contracts::events::Listener for OrderedListener {
    fn handle<'a>(
        &'a self,
        _: &'a dyn openrust::contracts::events::Event,
        _: &'a mut EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        self.1.lock().unwrap().push(self.0);
        Box::pin(async { Ok(None) })
    }
}
#[tokio::test]
async fn pushed_events_preserve_listener_order_and_forget_only_the_current_request() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    dispatcher
        .register(|r| {
            r.listen(
                "item",
                Arc::new(OrderedListener("item", calls.clone())),
                Delivery::Sync,
            )?;
            r.listen(
                "item_pushed",
                Arc::new(OrderedListener("first", calls.clone())),
                Delivery::Sync,
            )
        })
        .unwrap();
    app.container()
        .scope(async {
            dispatcher.push(Arc::new(NamedEvent("item"))).unwrap();
            dispatcher
                .register(|r| {
                    r.listen(
                        "item_pushed",
                        Arc::new(OrderedListener("last", calls.clone())),
                        Delivery::Sync,
                    )?;
                    r.listen(
                        "*_pushed",
                        Arc::new(OrderedListener("wild", calls.clone())),
                        Delivery::Sync,
                    )
                })
                .unwrap();
            assert!(
                dispatcher
                    .listing()
                    .iter()
                    .any(|(name, labels)| name == "item_pushed" && labels.len() == 3)
            );
            dispatcher
                .flush("item", &mut EventContext::new())
                .await
                .unwrap();
            assert_eq!(*calls.lock().unwrap(), ["first", "item", "last", "wild"]);
            calls.lock().unwrap().clear();
            dispatcher.forgetPushed().unwrap();
            dispatcher
                .flush("item", &mut EventContext::new())
                .await
                .unwrap();
            assert_eq!(*calls.lock().unwrap(), ["wild"]);
            calls.lock().unwrap().clear();
            dispatcher.push(Arc::new(NamedEvent("item"))).unwrap();
            dispatcher
                .flush("item", &mut EventContext::new())
                .await
                .unwrap();
            assert_eq!(*calls.lock().unwrap(), ["item", "wild"]);
        })
        .await;
    calls.lock().unwrap().clear();
    app.container()
        .scope(async {
            dispatcher
                .flush("item", &mut EventContext::new())
                .await
                .unwrap();
            assert_eq!(*calls.lock().unwrap(), ["first", "last", "wild"]);
        })
        .await;
}

struct CancelMail {
    stop: bool,
    sent: Arc<AtomicUsize>,
}
impl openrust::contracts::events::Listener for CancelMail {
    fn handle<'a>(
        &'a self,
        event: &'a dyn openrust::contracts::events::Event,
        _: &'a mut EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        let response = if event.name() == "MessageSending" {
            Some(if self.stop {
                openrust::events::ListenerResponse::Stop
            } else {
                openrust::events::ListenerResponse::Value(Box::new(false))
            })
        } else {
            self.sent.fetch_add(1, Ordering::SeqCst);
            None
        };
        Box::pin(async move { Ok(response) })
    }
}
#[tokio::test]
async fn mail_cancellation_returns_none_through_contract_facade_and_pending_without_transport_or_sent_event()
 {
    use openrust::{contracts::mail::Mailer as Contract, support::facades::Mail};
    for stop in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut app = Application::configure(root.path().to_owned()).withConfig(|_,_|Ok([("mail".into(),json!({"default":"one","from":{"address":"from@example.test"},"mailers":{"one":{"transport":"array"}}}))].into())).create().unwrap();
        app.boot().await.unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let listener = Arc::new(CancelMail {
            stop,
            sent: count.clone(),
        });
        app.container()
            .make::<Dispatcher>()
            .unwrap()
            .register(|r| {
                r.listen("MessageSending", listener.clone(), Delivery::Sync)?;
                r.listen("MessageSent", listener.clone(), Delivery::Sync)
            })
            .unwrap();
        app.call(
            openrust::container::Callback::new(&[], move |_| async move {
                let mailer = Mail::mailer(None).unwrap();
                let contract: &dyn Contract = mailer.as_ref();
                assert!(contract.send(&AddressMail).await.unwrap().is_none());
                assert!(contract.sendNow(&AddressMail).await.unwrap().is_none());
                assert!(Mail::sendNow(&AddressMail).await.unwrap().is_none());
                assert!(
                    Mail::to("pending@example.test")
                        .unwrap()
                        .sendNow(&AddressMail)
                        .await
                        .unwrap()
                        .is_none()
                );
                assert!(mailer.sent().unwrap().is_empty());
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn log_boot_context_is_inherited_and_request_context_is_isolated_even_with_shared_channels() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .withConfig(|_, _| {
            Ok([(
                "logging".into(),
                json!({"default":"one","channels":{"one":{"driver":"single","path":"one.log"}}}),
            )]
            .into())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().make::<LogManager>().unwrap();
    manager.shareContext([("boot".into(), json!(true))].into_iter().collect());
    let one = manager.channel(None).unwrap();
    let records = Arc::new(Mutex::new(Vec::new()));
    let captured = records.clone();
    manager
        .listen(move |event| {
            captured.lock().unwrap().push(event.context.clone());
            Ok(())
        })
        .unwrap();
    let container = app.container();
    tokio::join!(
        container.scope(async {
            one.withContext([("request".into(), json!(1))].into_iter().collect());
            tokio::task::yield_now().await;
            one.info("one", &LogContext::new()).unwrap();
        }),
        container.scope(async {
            one.withContext([("request".into(), json!(2))].into_iter().collect());
            tokio::task::yield_now().await;
            one.info("two", &LogContext::new()).unwrap();
        })
    );
    container
        .scope(async {
            one.info("clean", &LogContext::new()).unwrap();
        })
        .await;
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 3);
    for record in records.iter() {
        assert_eq!(record["boot"], true);
    }
    let mut requests = records[..2]
        .iter()
        .map(|r| r["request"].as_i64().unwrap())
        .collect::<Vec<_>>();
    requests.sort();
    assert_eq!(requests, [1, 2]);
    assert!(!records[2].contains_key("request"));
}
#[tokio::test]
async fn s3_copy_and_move_use_encoded_source_preserve_acl_and_do_not_delete_on_copy_failure() {
    use axum::{
        body::Body,
        extract::State,
        http::{Request, Response, StatusCode},
    };
    type Captured = Arc<Mutex<Vec<(String, String, http::HeaderMap)>>>;
    async fn service(State(calls): State<Captured>, request: Request<Body>) -> Response<Body> {
        calls.lock().unwrap().push((
            request.method().to_string(),
            request.uri().to_string(),
            request.headers().clone(),
        ));
        if request.uri().path().ends_with("denied") {
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::from(
                    "<Error><Code>AccessDenied</Code><Message>denied</Message></Error>",
                ))
                .unwrap();
        }
        let body = if request.method() == http::Method::GET {
            "<AccessControlPolicy><Owner><ID>owner</ID></Owner><AccessControlList><Grant><Grantee xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"Group\"><URI>http://acs.amazonaws.com/groups/global/AllUsers</URI></Grantee><Permission>READ</Permission></Grant></AccessControlList></AccessControlPolicy>"
        } else {
            "<CopyObjectResult><ETag>\"copy\"</ETag><LastModified>2026-11-21T00:00:00Z</LastModified></CopyObjectResult>"
        };
        Response::builder()
            .status(if request.method() == http::Method::DELETE {
                204
            } else {
                200
            })
            .header("content-type", "application/xml")
            .body(if request.method() == http::Method::DELETE {
                Body::empty()
            } else {
                Body::from(body)
            })
            .unwrap()
    }
    let calls: Captured = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let captured = calls.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().fallback(service).with_state(captured),
        )
        .await
        .unwrap();
    });
    let manager=openrust::filesystem::FilesystemManager::new(Arc::new(openrust::config::Repository::new([("filesystems".into(),json!({"default":"s3","disks":{"s3":{"driver":"s3","bucket":"bucket","root":"root","region":"us-east-1","key":"key","secret":"secret","endpoint":format!("http://{address}"),"use_path_style_endpoint":true,"throw":true}}}))].into())),".".into());
    let disk = manager.disk(None).unwrap();
    assert!(disk.copy("dir/a b", "copy").await.unwrap());
    assert!(disk.r#move("dir/a b", "moved").await.unwrap());
    assert!(disk.r#move("dir/a b", "denied").await.is_err());
    let calls = calls.lock().unwrap();
    let copies = calls
        .iter()
        .filter(|(method, _, _)| method == "PUT")
        .collect::<Vec<_>>();
    assert_eq!(copies.len(), 3);
    for (_, _, headers) in copies {
        assert_eq!(
            percent_encoding::percent_decode_str(headers["x-amz-copy-source"].to_str().unwrap())
                .decode_utf8()
                .unwrap(),
            "bucket/root/dir/a b"
        );
        assert_eq!(headers["x-amz-acl"], "public-read");
    }
    assert_eq!(
        calls
            .iter()
            .filter(|(method, _, _)| method == "DELETE")
            .count(),
        1
    );
    server.abort();
}

#[tokio::test]
async fn view_and_translation_facades_keep_boot_defaults_and_concurrent_scopes_separate() {
    use openrust::support::facades::{Lang, View};
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("resources/views")).unwrap();
    std::fs::write(
        root.path().join("resources/views/row.blade.html"),
        "{{ $shared }}:{{ $item }}",
    )
    .unwrap();
    std::fs::create_dir_all(root.path().join("lang/en")).unwrap();
    std::fs::write(
        root.path().join("lang/en/messages.json"),
        r#"{"only":"fallback"}"#,
    )
    .unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.container()
        .make::<openrust::view::Factory>()
        .unwrap()
        .share("shared", "boot");
    let translator = app
        .container()
        .make::<openrust::translation::Translator>()
        .unwrap();
    translator.setLocale("en").unwrap();
    translator.setFallback("en").unwrap();
    let [one, two] = ["fr", "de"].map(|expected| {
        let expected = expected.to_owned();
        app.container().scope(app.call(
            openrust::container::Callback::new(&[], move |_| async move {
                assert_eq!(Lang::getLocale().unwrap(), "en");
                assert_eq!(View::getShared().unwrap()["shared"].as_str(), Some("boot"));
                Lang::setLocale(&expected).unwrap();
                Lang::setFallback("en").unwrap();
                View::share("shared", expected.clone()).unwrap();
                tokio::task::yield_now().await;
                assert_eq!(Lang::getLocale().unwrap(), expected);
                assert_eq!(Lang::getFallback().unwrap(), "en");
                assert_eq!(
                    Lang::get("messages.only", &Default::default(), None).unwrap(),
                    "fallback"
                );
                assert!(Lang::setLocale("../bad").is_err());
                assert_eq!(
                    View::renderWhen(false, "missing", &Default::default()).unwrap(),
                    ""
                );
                assert_eq!(
                    View::renderUnless(true, "missing", &Default::default()).unwrap(),
                    ""
                );
                assert_eq!(
                    View::renderEach("row", [(0.into(), "x".into())], "item", "raw|none").unwrap(),
                    format!("{expected}:x")
                );
                Ok(())
            }),
            Default::default(),
        ))
    });
    let (one, two) = tokio::join!(one, two);
    one.unwrap();
    two.unwrap();
    assert_eq!(translator.getLocale(), "en");
    assert_eq!(
        app.container()
            .make::<openrust::view::Factory>()
            .unwrap()
            .getShared()["shared"]
            .as_str(),
        Some("boot")
    );
}

#[tokio::test]
async fn failed_registered_hook_prevents_boot_phases_and_repeated_boot() {
    let root = tempfile::tempdir().unwrap();
    let later = Arc::new(AtomicUsize::new(0));
    let called = later.clone();
    let mut app = Application::configure(root.path().to_owned())
        .registered(|_| Err(std::io::Error::other("注册失败").into()))
        .booting(move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .create()
        .unwrap();
    assert!(app.boot().await.is_err());
    assert!(app.boot().await.is_err());
    assert_eq!(later.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn translation_defaults_come_from_cached_configuration_without_reloading_environment() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("storage/framework")).unwrap();
    std::fs::write(
        root.path().join("storage/framework/config.json"),
        json!({"app":{"locale":"zh-CN","fallback_locale":"en"}}).to_string(),
    )
    .unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .withConfig(|_, _| panic!("缓存命中不能执行配置加载器"))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let translator = app
        .container()
        .make::<openrust::translation::Translator>()
        .unwrap();
    assert_eq!(translator.getLocale(), "zh-CN");
    assert_eq!(translator.getFallback(), "en");
    app.container()
        .scope(async {
            translator.setLocale("fr").unwrap();
            translator.setFallback("de").unwrap();
            assert_eq!(translator.getLocale(), "fr");
        })
        .await;
    assert_eq!(translator.getLocale(), "zh-CN");
    assert_eq!(translator.getFallback(), "en");
}

#[tokio::test]
async fn pushed_events_stay_in_concurrent_scopes_and_defer_original_delivery() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    dispatcher
        .register(|registry| {
            registry.listen(
                "audit",
                Arc::new(CountListener(calls.clone())),
                Delivery::Sync,
            )
        })
        .unwrap();
    let barrier = tokio::sync::Barrier::new(2);
    let run = || {
        app.container().scope(async {
            dispatcher.push(Arc::new(NamedEvent("audit"))).unwrap();
            barrier.wait().await;
            dispatcher
                .defer(Some(&["audit"]), async {
                    dispatcher.flush("audit", &mut EventContext::new()).await?;
                    // 当前作用域的原始事件尚未派发；另一作用域最多完成一次派发。
                    assert!(calls.load(Ordering::SeqCst) <= 1);
                    Ok(())
                })
                .await
                .unwrap();
        })
    };
    tokio::join!(run(), run());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    app.container()
        .scope(async {
            dispatcher
                .flush("audit", &mut EventContext::new())
                .await
                .unwrap();
        })
        .await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
