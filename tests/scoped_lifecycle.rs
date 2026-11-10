use openrust::{
    console::{ConsoleError, Output},
    container::ServiceKey,
    contracts::{
        console::Kernel as _, container::Container as _, foundation::Application as _,
        http::Kernel as _,
    },
    foundation::{Application, configuration::Routing, console, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan, Facade, Route},
    },
};
use std::{
    io::{Read as _, Write as _},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
struct Stats {
    created: AtomicUsize,
    dropped: AtomicUsize,
}
struct Session {
    name: &'static str,
    id: usize,
    stats: Arc<Stats>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stats.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
struct Sessions;
impl Facade for Sessions {
    type Service = Session;
    const cached: bool = false;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Session>()
    }
}
struct Provider(&'static str, Arc<Stats>);
impl ServiceProvider for Provider {
    fn register(&mut self, app: &Application) -> ProviderResult {
        let name = self.0;
        let stats = self.1.clone();
        app.container().scoped::<Session, _>(move |_| {
            Ok(Arc::new(Session {
                name,
                id: stats.created.fetch_add(1, Ordering::SeqCst) + 1,
                stats: stats.clone(),
            }))
        });
        Ok(())
    }
}
fn web() -> Result<(), RoutingError> {
    Route::get("/scope", request)?;
    Ok(())
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::command("scope:probe", |output| {
        let a = Sessions::getFacadeRoot().unwrap();
        let b = Sessions::getFacadeRoot().unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        output.stdout.push_str(&format!("{}:{}", a.name, a.id));
    })?;
    Ok(())
}
fn request(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let a = Sessions::getFacadeRoot().unwrap();
        tokio::task::yield_now().await;
        let b = Sessions::getFacadeRoot().unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        Ok(Response::new(
            StatusCode::OK,
            Body::from(format!("{}:{}", a.name, a.id)),
        ))
    })
}
fn application(name: &'static str, stats: Arc<Stats>) -> Application {
    Application::configure(std::env::current_dir().unwrap())
        .withProviders(vec![Box::new(Provider(name, stats))])
        .withRouting(Routing::default().web(web).commands(commands))
        .create()
        .unwrap()
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let mut incoming = Request::from_parts(parts, body);
    let (parts, body) = kernel.handle(&mut incoming).await.into_parts();
    axum::response::Response::from_parts(parts, body)
}
async fn server(app: Application) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    use axum::handler::Handler as _;
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}
fn send(address: std::net::SocketAddr) -> String {
    let timeout = Duration::from_secs(5);
    let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
    stream.set_read_timeout(Some(timeout)).unwrap();
    stream.set_write_timeout(Some(timeout)).unwrap();
    write!(
        stream,
        "GET /scope HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    response.split_once("\r\n\r\n").unwrap().1.to_owned()
}

#[tokio::test]
async fn real_http_scoped_instances_are_shared_within_requests_and_released_between_requests() {
    let a = Arc::new(Stats::default());
    let b = Arc::new(Stats::default());
    let ((first, x), (second, y)) = tokio::join!(
        server(application("A", a.clone())),
        server(application("B", b.clone()))
    );
    assert_eq!(a.created.load(Ordering::SeqCst), 0);
    let bodies = tokio::task::spawn_blocking(move || {
        std::thread::scope(|scope| {
            let requests: Vec<_> = [first, second, first, second]
                .into_iter()
                .map(|address| scope.spawn(move || send(address)))
                .collect();
            requests
                .into_iter()
                .map(|request| request.join().unwrap())
                .collect::<Vec<_>>()
        })
    })
    .await
    .unwrap();
    assert!(bodies[0].starts_with("A:"));
    assert!(bodies[1].starts_with("B:"));
    assert!(bodies[2].starts_with("A:"));
    assert!(bodies[3].starts_with("B:"));
    assert_ne!(bodies[0], bodies[2]);
    assert_ne!(bodies[1], bodies[3]);
    assert_eq!(a.created.load(Ordering::SeqCst), 2);
    assert_eq!(a.dropped.load(Ordering::SeqCst), 2);
    assert_eq!(b.created.load(Ordering::SeqCst), 2);
    assert_eq!(b.dropped.load(Ordering::SeqCst), 2);
    x.abort();
    y.abort();
}

#[tokio::test]
async fn repeated_artisan_dispatches_get_distinct_scoped_instances() {
    let stats = Arc::new(Stats::default());
    let mut kernel = console::Kernel::new(
        application("CLI", stats.clone()),
        openrust::console::Application::new(),
    );
    let args = ["scope:probe".into()];
    for index in 1..=3 {
        let mut output = Output::default();
        assert_eq!(kernel.handle(&args, &mut output).await, 0);
        assert_eq!(output.stdout, format!("CLI:{index}"));
        assert!(output.stderr.is_empty());
        assert_eq!(stats.dropped.load(Ordering::SeqCst), index);
    }
}
