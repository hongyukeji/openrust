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
    sync::Arc,
    time::Duration,
};
struct Selected(Arc<String>);
struct Defaulted(Arc<String>);
struct SelectedFacade;
impl Facade for SelectedFacade {
    type Service = Selected;
    const cached: bool = false;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Selected>()
    }
}
struct DefaultFacade;
impl Facade for DefaultFacade {
    type Service = Defaulted;
    const cached: bool = false;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Defaulted>()
    }
}
struct Provider(&'static str);
impl ServiceProvider for Provider {
    fn register(&mut self, app: &Application) -> ProviderResult {
        let name = self.0;
        let c = app.container();
        c.singleton::<String, _>(move |_| Ok(Arc::new(format!("{name}:default"))))
            .unwrap();
        c.bind::<Selected, _>(|c| Ok(Arc::new(Selected(c.make()?))))
            .unwrap();
        c.bind::<Defaulted, _>(|c| Ok(Arc::new(Defaulted(c.make()?))))
            .unwrap();
        c.when::<Selected>()?
            .needs::<String>()
            .give(move |_| Ok(Arc::new(format!("{name}:custom"))))?;
        Ok(())
    }
}
fn result() -> String {
    let first = DefaultFacade::getFacadeRoot().unwrap();
    let selected = SelectedFacade::getFacadeRoot().unwrap();
    let second = SelectedFacade::getFacadeRoot().unwrap();
    let last = DefaultFacade::getFacadeRoot().unwrap();
    assert!(!Arc::ptr_eq(&selected.0, &second.0));
    assert!(Arc::ptr_eq(&first.0, &last.0));
    format!("{}|{}|{}", first.0, selected.0, last.0)
}
fn web() -> Result<(), RoutingError> {
    Route::get("/context", request)?;
    Ok(())
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::command("context:probe", |output| {
        output.stdout.push_str(&result());
    })?;
    Ok(())
}
fn request(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Response::new(StatusCode::OK, Body::from(result())))
    })
}
fn application(name: &'static str) -> Application {
    Application::configure(std::env::current_dir().unwrap())
        .withProviders(vec![Box::new(Provider(name))])
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
        "GET /context HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    response.split_once("\r\n\r\n").unwrap().1.to_owned()
}

#[tokio::test]
async fn live_http_contextual_and_default_resolutions_are_isolated_across_applications() {
    let ((first, x), (second, y)) =
        tokio::join!(server(application("A")), server(application("B")));
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
    assert_eq!(
        bodies,
        [
            "A:default|A:custom|A:default",
            "B:default|B:custom|B:default",
            "A:default|A:custom|A:default",
            "B:default|B:custom|B:default"
        ]
    );
    x.abort();
    y.abort();
}
#[tokio::test]
async fn artisan_contextual_selection_keeps_the_default_shared_service() {
    let mut kernel =
        console::Kernel::new(application("CLI"), openrust::console::Application::new());
    for _ in 0..3 {
        let mut output = Output::default();
        assert_eq!(
            kernel.handle(&["context:probe".into()], &mut output).await,
            0
        );
        assert_eq!(output.stdout, "CLI:default|CLI:custom|CLI:default");
        assert!(output.stderr.is_empty());
    }
}
