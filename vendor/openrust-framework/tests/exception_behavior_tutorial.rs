use axum::handler::Handler as _;
use openrust::{
    contracts::{
        container::Container as _,
        debug::{ExceptionBehavior, SharedException, ShouldntReport},
        foundation::Application as _,
        http::Kernel as _,
    },
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
        http::Kernel,
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogContext,
    support::{
        facades::{App, Exceptions, Facade, Route},
        report,
    },
};
use std::{
    error::Error,
    ffi::OsString,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
#[derive(Default)]
struct State {
    own: AtomicUsize,
    global: AtomicUsize,
    renders: AtomicUsize,
    global_render: AtomicUsize,
    contexts: AtomicUsize,
}
#[derive(Debug)]
struct Failure {
    kind: String,
    state: Arc<State>,
}
impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("State")
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("private-domain-message")
    }
}
impl Error for Failure {}
impl ExceptionBehavior for Failure {
    fn report(&self) -> ReportDecision {
        self.state.own.fetch_add(1, Ordering::SeqCst);
        if self.kind == "handled" {
            ReportDecision::Stop
        } else {
            ReportDecision::Continue
        }
    }
    fn render(&self, _: &Request) -> Option<Response> {
        self.state.renders.fetch_add(1, Ordering::SeqCst);
        (self.kind == "render").then(|| Response::new(StatusCode::CONFLICT, "own-render".into()))
    }
    fn context(&self) -> LogContext {
        self.state.contexts.fetch_add(1, Ordering::SeqCst);
        [
            ("order_id".into(), 7.into()),
            ("scope".into(), "instance".into()),
        ]
        .into_iter()
        .collect()
    }
}
#[derive(Debug)]
struct Quiet;
impl fmt::Display for Quiet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("quiet-domain")
    }
}
impl Error for Quiet {}
impl ShouldntReport for Quiet {}
#[derive(Debug)]
struct Outer(SharedException);
impl fmt::Display for Outer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("outer-domain")
    }
}
impl Error for Outer {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

fn fail(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let kind = request.route("kind").unwrap().to_owned();
        if kind == "quiet" {
            return Err(HttpError::Internal {
                source: Box::new(Quiet.into_exception()),
            });
        }
        Err(HttpError::Internal {
            source: Box::new(Failure {
                kind,
                state: App::make::<State>().await?,
            }),
        })
    })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/failure/{kind}", fail)
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn type_methods_run_on_the_real_exception_before_global_callbacks_and_keep_context() {
    let root = tempfile::tempdir().unwrap();
    let state = Arc::new(State::default());
    let app = Application::configure(root.path().into())
        .with_exceptions(|exceptions| {
            exceptions
                .report::<Failure>(|e| {
                    e.state.global.fetch_add(1, Ordering::SeqCst);
                    ReportDecision::Continue
                })
                .render::<Failure>(|e, _| {
                    e.state.global_render.fetch_add(1, Ordering::SeqCst);
                    (e.kind == "fallback")
                        .then(|| Response::new(StatusCode::IM_A_TEAPOT, "global-fallback".into()))
                })
                .context(|_| [("scope".into(), "global".into())].into_iter().collect())
                .use_exception::<Failure>()
                .use_exception::<Failure>();
        })
        .with_routing(Routing::default().api(routes))
        .create()
        .unwrap();
    app.container().instance(state.clone()).unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    for (kind, status) in [
        ("handled", 500),
        ("render", 409),
        ("fallback", 418),
        ("default", 500),
        ("quiet", 500),
    ] {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("GET /api/failure/{kind} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{kind}: {response}"
        );
    }
    assert_eq!(state.own.load(Ordering::SeqCst), 4);
    assert_eq!(state.global.load(Ordering::SeqCst), 3);
    assert_eq!(state.renders.load(Ordering::SeqCst), 4);
    assert_eq!(state.global_render.load(Ordering::SeqCst), 3);
    assert_eq!(state.contexts.load(Ordering::SeqCst), 3);
    let log = std::fs::read_to_string(root.path().join("storage/logs/app.log")).unwrap();
    assert_eq!(log.lines().count(), 3);
    assert_eq!(log.matches("\"order_id\":7").count(), 3, "{log}");
    assert_eq!(log.matches("\"scope\":\"global\"").count(), 3, "{log}");
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn ignored_metadata_survives_shared_constructors_but_mapping_can_change_root_policy() {
    let root = tempfile::tempdir().unwrap();
    let seen = Arc::new(AtomicUsize::new(0));
    let observed = seen.clone();
    let mut app = Application::configure(root.path().into())
        .with_exceptions(move |e| {
            e.stop_ignoring::<HttpError>()
                .stop_ignoring::<Quiet>()
                .reportable_all(move |_| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    ReportDecision::Stop
                });
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let error = Quiet.into_exception();
        report(error.clone()).await.unwrap();
        report(SharedException::new(error.clone())).await.unwrap();
        report(SharedException::from_arc(Arc::new(error.clone())))
            .await
            .unwrap();
        report(SharedException::from_typed_arc(Arc::new(error)))
            .await
            .unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 0);
        report(Outer(Quiet.into_exception())).await.unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        Exceptions::map::<Quiet, _>(|_| std::io::Error::other("mapped-root")).unwrap();
        report(Quiet.into_exception()).await.unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    })
    .await;
}
#[tokio::test]
async fn runtime_type_registration_is_application_local_and_context_reads_the_same_instance() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut a = Application::configure(first.path().into())
        .create()
        .unwrap();
    let mut b = Application::configure(second.path().into())
        .create()
        .unwrap();
    a.boot().await.unwrap();
    b.boot().await.unwrap();
    let state = Arc::new(State::default());
    App::set_facade_application(Some(&a), async {
        Exceptions::use_exception::<Failure>().unwrap();
        Exceptions::use_exception::<Failure>().unwrap();
        let e = Failure {
            kind: "handled".into(),
            state: state.clone(),
        };
        let context =
            Exceptions::context_for_exception(&openrust::contracts::debug::Exception::typed(&e))
                .await
                .unwrap();
        assert_eq!(context.fields["order_id"], 7);
        report(e).await.unwrap();
    })
    .await;
    App::set_facade_application(Some(&b), async {
        report(Failure {
            kind: "handled".into(),
            state: state.clone(),
        })
        .await
        .unwrap();
    })
    .await;
    assert_eq!(state.own.load(Ordering::SeqCst), 1);
    assert_eq!(state.contexts.load(Ordering::SeqCst), 1);
}
struct Probe(openrust::console::CommandDefinition);
impl openrust::console::Command for Probe {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a openrust::console::Input,
        _: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            report(Failure {
                kind: "handled".into(),
                state: App::make::<State>().await.unwrap(),
            })
            .await
            .unwrap();
            report(Quiet.into_exception()).await.unwrap();
            Ok(0)
        })
    }
}
fn commands(
    artisan: &mut openrust::console::Application,
) -> Result<(), openrust::console::ConsoleError> {
    artisan.add_command(Box::new(Probe(openrust::console::CommandDefinition::new(
        clap::Command::new("exceptions:own-methods"),
    ))))
}
#[tokio::test]
async fn artisan_reports_via_type_methods_and_markers_without_changing_its_success() {
    let root = tempfile::tempdir().unwrap();
    let state = Arc::new(State::default());
    let app = Application::configure(root.path().into())
        .with_commands(commands)
        .with_exceptions(|e| {
            e.use_exception::<Failure>();
        })
        .create()
        .unwrap();
    app.container().instance(state.clone()).unwrap();
    assert_eq!(
        app.handle_command(&[OsString::from("exceptions:own-methods")])
            .await
            .unwrap(),
        0
    );
    assert_eq!(state.own.load(Ordering::SeqCst), 1);
    assert!(!root.path().join("storage/logs/app.log").exists());
}
