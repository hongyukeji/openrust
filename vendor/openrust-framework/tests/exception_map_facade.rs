use openrust::{
    contracts::debug::Exception,
    foundation::configuration::{ReportDecision, Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{App, Exceptions, Facade, Route},
    testing::TestApp,
};
use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Debug, Clone)]
struct Original(Arc<AtomicUsize>);
impl fmt::Display for Original {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original")
    }
}
impl Error for Original {}
#[derive(Debug)]
struct Mapped(Original);
impl fmt::Display for Mapped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("mapped")
    }
}
impl Error for Mapped {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}
#[derive(Debug)]
struct Final;
impl fmt::Display for Final {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("final")
    }
}
impl Error for Final {}
fn fail(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: Box::new(Original(Arc::new(AtomicUsize::new(7)))),
        })
    })
}
async fn app() -> TestApp {
    let mut app = TestApp::create(|b| {
        b.with_routing(Routing::default().web(|| Route::get("/failure", fail)))
    })
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn report_maps_once_before_rules_and_does_not_match_the_previous_error_again() {
    let app = app().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::map::<Original, Mapped>(move |e| {
            observed.fetch_add(1, Ordering::SeqCst);
            Mapped(e.clone())
        })
        .unwrap();
        Exceptions::map::<Mapped, Final>(|_| panic!("映射不得递归应用")).unwrap();
        Exceptions::dont_report::<Original>().unwrap();
        Exceptions::reportable::<Original>(|_| panic!("映射后不再匹配来源类型")).unwrap();
        let reports = Arc::new(AtomicUsize::new(0));
        let recorded = reports.clone();
        Exceptions::reportable::<Mapped>(move |e| {
            assert_eq!(e.0.0.load(Ordering::SeqCst), 7);
            recorded.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Stop
        })
        .unwrap();
        let error = Original(Arc::new(AtomicUsize::new(7)));
        assert!(
            !Exceptions::should_report(&Exception::Error(&error))
                .await
                .unwrap()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        Exceptions::report(error).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(reports.load(Ordering::SeqCst), 1);
    })
    .await;
}
#[tokio::test]
async fn http_applies_separate_report_and_render_maps_and_preserves_source_diagnostics() {
    let mut app = app().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::map::<Original, Mapped>(move |e| {
            observed.fetch_add(1, Ordering::SeqCst);
            Mapped(e.clone())
        })
        .unwrap();
        Exceptions::renderable::<Original>(|_, _| panic!("渲染不得误匹配映射的 previous")).unwrap();
        Exceptions::renderable::<Mapped>(|e, _| {
            assert!(e.source().unwrap().is::<Original>());
            Some(Response::new(
                StatusCode::CONFLICT,
                openrust::http::Body::from("mapped-response"),
            ))
        })
        .unwrap();
    })
    .await;
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::CONFLICT)
        .assert_content("mapped-response");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn mapping_same_type_replaces_callback_and_is_application_local() {
    let mut first = app().await;
    let mut second = app().await;
    App::set_facade_application(Some(first.app()), async {
        Exceptions::map::<Original, Final>(|_| Final).unwrap();
        Exceptions::map::<Original, Mapped>(|e| Mapped(e.clone())).unwrap();
        Exceptions::renderable::<Mapped>(|_, _| Some(Response::from("replacement"))).unwrap();
    })
    .await;
    first
        .get("/failure")
        .await
        .unwrap()
        .assert_content("replacement");
    second
        .get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}
#[tokio::test]
async fn maps_to_http_errors_keep_status_headers_and_request_response_hooks() {
    let mut app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::map::<Original, HttpError>(|_| {
            openrust::http::abort(StatusCode::GONE, "mapped-http", Default::default())
        })
        .unwrap();
        Exceptions::respond_using(|mut response, _, _| {
            response
                .parts_mut()
                .headers
                .insert("x-mapped", "yes".parse().unwrap());
            response
        })
        .unwrap();
    })
    .await;
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::GONE)
        .assert_header("x-mapped", "yes");
}
#[tokio::test]
async fn fake_records_original_report_while_rendering_delegates_mapping() {
    let mut app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::map::<Original, Mapped>(|e| Mapped(e.clone())).unwrap();
        Exceptions::renderable::<Mapped>(|_, _| Some(Response::from("fake-mapped"))).unwrap();
    })
    .await;
    let fake = app.fake_exceptions(&[]).await.unwrap();
    app.get("/failure")
        .await
        .unwrap()
        .assert_content("fake-mapped");
    fake.assert_reported::<HttpError>();
    fake.assert_not_reported::<Mapped>();
    fake.assert_reported_count(1);
}
#[tokio::test]
async fn mapping_callbacks_can_change_future_configuration_without_locking_the_snapshot() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::map::<Original, Mapped>(|e| {
            Exceptions::map::<Original, Final>(|_| Final).unwrap();
            Mapped(e.clone())
        })
        .unwrap();
        let mapped = Arc::new(AtomicUsize::new(0));
        let recorded = mapped.clone();
        Exceptions::reportable::<Mapped>(move |_| {
            recorded.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Stop
        })
        .unwrap();
        let final_count = Arc::new(AtomicUsize::new(0));
        let recorded = final_count.clone();
        Exceptions::reportable::<Final>(move |_| {
            recorded.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Stop
        })
        .unwrap();
        Exceptions::report(Original(Arc::new(AtomicUsize::new(7))))
            .await
            .unwrap();
        Exceptions::report(Original(Arc::new(AtomicUsize::new(7))))
            .await
            .unwrap();
        assert_eq!(mapped.load(Ordering::SeqCst), 1);
        assert_eq!(final_count.load(Ordering::SeqCst), 1);
    })
    .await;
}

#[derive(Debug)]
struct Inner(openrust::contracts::debug::SharedException);
impl fmt::Display for Inner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("inner-wrapper")
    }
}
impl Error for Inner {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}
#[tokio::test]
async fn explicit_inner_exception_adapter_precedes_mappers_and_preserves_owned_identity() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        let original = Original(Arc::new(AtomicUsize::new(21)));
        let identity = original.0.clone();
        let original = openrust::contracts::debug::SharedException::new(original);
        let pointer = Exception::Error(&original)
            .downcast_ref::<Original>()
            .unwrap() as *const Original as usize;
        Exceptions::map::<Inner, Final>(|_| panic!("内层异常优先于映射")).unwrap();
        Exceptions::map::<Original, Final>(|_| panic!("解包后不再次映射")).unwrap();
        Exceptions::inner_exception::<Inner>(|wrapper| Some(wrapper.0.clone())).unwrap();
        let observed = identity.clone();
        Exceptions::reportable::<Original>(move |error| {
            assert!(Arc::ptr_eq(&error.0, &observed));
            assert_eq!(error as *const Original as usize, pointer);
            error.0.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Stop
        })
        .unwrap();
        Exceptions::report(Inner(original)).await.unwrap();
        assert_eq!(identity.load(Ordering::SeqCst), 22);
    })
    .await;
}

#[tokio::test]
async fn json_policy_receives_the_actual_mapped_domain_error_without_a_synthetic_http_error() {
    let mut app = app().await;
    let policy_calls = Arc::new(AtomicUsize::new(0));
    let calls = policy_calls.clone();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::map::<Original, Mapped>(|e| Mapped(e.clone())).unwrap();
        Exceptions::should_render_json_when(move |_, exception| {
            assert!(exception.downcast_ref::<Mapped>().is_some());
            assert!(exception.downcast_ref::<Original>().is_none());
            assert!(exception.downcast_ref::<HttpError>().is_none());
            calls.fetch_add(1, Ordering::SeqCst);
            true
        })
        .unwrap();
    })
    .await;
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR)
        .assert_header("content-type", "application/json");
    assert_eq!(policy_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn nullable_inner_adapter_falls_back_to_normal_mapping() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::inner_exception::<Original>(|_| None).unwrap();
        Exceptions::map::<Original, Final>(|_| Final).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        Exceptions::reportable::<Final>(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Stop
        })
        .unwrap();
        Exceptions::report(Original(Arc::new(AtomicUsize::new(0))))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    })
    .await;
}
#[tokio::test]
async fn native_dump_control_is_not_transformed_by_error_mapping_with_or_without_fake() {
    fn dump(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async { Err(HttpError::DumpAndDie("dump-control".into())) })
    }
    let mut app =
        TestApp::create(|b| b.with_routing(Routing::default().web(|| Route::get("/dump", dump))))
            .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::map::<HttpError, Final>(|_| panic!("原生调试控制不应进入异常映射")).unwrap();
    })
    .await;
    app.get("/dump")
        .await
        .unwrap()
        .assert_content("dump-control");
    let fake = app.fake_exceptions(&[]).await.unwrap();
    app.get("/dump")
        .await
        .unwrap()
        .assert_content("dump-control");
    fake.assert_nothing_reported();
}

#[tokio::test]
async fn optional_mapping_selects_enum_variants_without_replacing_unmatched_errors() {
    let mut app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::map_optional::<HttpError, HttpError>(|error| {
            matches!(error, HttpError::NotFound).then(|| {
                openrust::http::abort(StatusCode::GONE, "mapped-missing", Default::default())
            })
        })
        .unwrap();
    })
    .await;
    app.get("/missing")
        .await
        .unwrap()
        .assert_status(StatusCode::GONE);
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}
