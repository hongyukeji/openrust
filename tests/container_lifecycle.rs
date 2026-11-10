use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use openrust::{
    console::{ConsoleError, Output},
    container::ServiceKey,
    contracts::{
        console::Kernel as _,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        http::Kernel as _,
    },
    foundation::{Application, configuration::Routing, console, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan, Facade},
    },
};

struct Message {
    name: &'static str,
    sequence: usize,
}
struct Messages;
impl Facade for Messages {
    type Service = Message;
    fn getFacadeAccessor() -> ServiceKey {
        "message".into()
    }
}
struct Reenter;
impl Drop for Reenter {
    fn drop(&mut self) {
        // A cached service may use another facade while it is being disposed.
        if !std::thread::panicking() {
            let _ = Messages::getFacadeRoot();
        }
    }
}
struct ReentrantFacade;
impl Facade for ReentrantFacade {
    type Service = Reenter;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Reenter>()
    }
}
struct FreshMessages;
impl Facade for FreshMessages {
    type Service = Message;
    const cached: bool = false;
    fn getFacadeAccessor() -> ServiceKey {
        "message".into()
    }
}
struct Services(&'static str);
impl ServiceProvider for Services {
    fn register(&mut self, app: &Application) -> ProviderResult {
        app.container()
            .bind::<Reenter, _>(|_| Ok(Arc::new(Reenter)))
            .unwrap();
        let name = self.0;
        let sequence = AtomicUsize::new(0);
        app.container()
            .bindIf::<Message, _>(move |_| {
                Ok(Arc::new(Message {
                    name,
                    sequence: sequence.fetch_add(1, Ordering::SeqCst) + 1,
                }))
            })
            .unwrap();
        app.container()
            .alias(ServiceKey::typed::<Message>(), "message")?;
        assert_eq!(Messages::getFacadeRoot()?.name, name);
        assert!(app.container().resolved("message")?);
        assert!(!app.container().isShared(ServiceKey::typed::<Message>()));
        Ok(())
    }
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            assert_eq!(Messages::getFacadeRoot()?.sequence, 1);
            Ok(())
        })
    }
}

fn application(name: &'static str) -> Application {
    Application::configure(std::env::current_dir().unwrap())
        .withProviders(vec![Box::new(Services(name))])
        .withRouting(Routing::default().web(web).commands(commands))
        .create()
        .unwrap()
}
fn web() -> Result<(), RoutingError> {
    openrust::support::facades::Route::get("/probe", probe)?;
    openrust::support::facades::Route::get("/cache", cache)?;
    Ok(())
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::command("probe", |output| {
        let first = Messages::getFacadeRoot().unwrap();
        let second = Messages::getFacadeRoot().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        output
            .stdout
            .push_str(&format!("{}:{}", first.name, first.sequence));
    })?;
    Ok(())
}
fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let first = Messages::getFacadeRoot().unwrap();
        tokio::task::yield_now().await;
        let second = Messages::getFacadeRoot().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        Ok(Response::new(
            StatusCode::OK,
            Body::from(format!("{}:{}", first.name, first.sequence)),
        ))
    })
}
fn cache(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let reentrant = ReentrantFacade::getFacadeRoot().unwrap();
        drop(reentrant);
        ReentrantFacade::clearResolvedInstance(None).unwrap();
        let reentrant = ReentrantFacade::getFacadeRoot().unwrap();
        drop(reentrant);
        ReentrantFacade::clearResolvedInstances().unwrap();
        let first = Messages::getFacadeRoot().unwrap();
        Messages::clearResolvedInstance(None).unwrap();
        let second = Messages::getFacadeRoot().unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        Messages::clearResolvedInstances().unwrap();
        let uncached = FreshMessages::getFacadeRoot().unwrap();
        let another = FreshMessages::getFacadeRoot().unwrap();
        assert!(!Arc::ptr_eq(&uncached, &another));
        let replacement = Arc::new(Message {
            name: "swapped",
            sequence: 99,
        });
        Messages::swap(replacement.clone()).unwrap();
        assert!(Arc::ptr_eq(
            &Messages::getFacadeRoot().unwrap(),
            &replacement
        ));
        Ok(Response::new(
            StatusCode::OK,
            Body::from("cache controls work"),
        ))
    })
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let mut request = Request::from_parts(parts, body);
    let response = kernel.handle(&mut request).await;
    let (parts, body) = response.into_parts();
    axum::response::Response::from_parts(parts, body)
}
async fn serve(app: Application) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    use axum::handler::Handler as _;
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (address, server)
}
fn send(address: std::net::SocketAddr, path: &str) -> String {
    use std::io::{Read as _, Write as _};
    let timeout = Duration::from_secs(5);
    let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
    stream.set_read_timeout(Some(timeout)).unwrap();
    stream.set_write_timeout(Some(timeout)).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    response.split_once("\r\n\r\n").unwrap().1.to_owned()
}

#[tokio::test]
async fn providers_and_facades_isolate_real_http_requests_and_clear_the_cache() {
    assert!(matches!(
        Messages::getFacadeRoot(),
        Err(BindingResolutionError::FacadeNotSet)
    ));
    let ((a, first), (b, second)) = tokio::join!(serve(application("A")), serve(application("B")));
    let bodies = tokio::task::spawn_blocking(move || {
        std::thread::scope(|scope| {
            let clients: Vec<_> = [a, b, a, b]
                .into_iter()
                .map(|address| scope.spawn(move || send(address, "/probe")))
                .collect();
            clients
                .into_iter()
                .map(|client| client.join().unwrap())
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
    for body in &bodies {
        assert!(!body.ends_with(":1"), "provider cache leaked: {body}");
    }
    let body = tokio::task::spawn_blocking(move || send(a, "/cache"))
        .await
        .unwrap();
    assert_eq!(body, "cache controls work");
    first.abort();
    second.abort();
    assert!(matches!(
        Messages::getFacadeRoot(),
        Err(BindingResolutionError::FacadeNotSet)
    ));
}

#[tokio::test]
async fn artisan_facades_use_their_application_and_start_a_fresh_command_scope() {
    let mut a = console::Kernel::new(application("CLI-A"), openrust::console::Application::new());
    let mut b = console::Kernel::new(application("CLI-B"), openrust::console::Application::new());
    let mut first = Output::default();
    let mut second = Output::default();
    let args = ["probe".into()];
    let (x, y) = tokio::join!(a.handle(&args, &mut first), b.handle(&args, &mut second));
    assert_eq!((x, y), (0, 0));
    assert_eq!(first.stdout, "CLI-A:1");
    assert_eq!(second.stdout, "CLI-B:1");
    let mut next = Output::default();
    assert_eq!(a.handle(&args, &mut next).await, 0);
    assert_eq!(next.stdout, "CLI-A:2");
    assert!(matches!(
        Messages::getFacadeRoot(),
        Err(BindingResolutionError::FacadeNotSet)
    ));
}

struct EmptyProvider;
impl ServiceProvider for EmptyProvider {}
#[tokio::test]
async fn providers_may_omit_both_hooks_and_configuration_keeps_its_contract_alias() {
    let mut app = Application::configure(std::env::current_dir().unwrap())
        .withProviders(vec![Box::new(EmptyProvider)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let alias = app
        .container()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .unwrap();
    let typed = app
        .container()
        .make::<dyn openrust::contracts::config::Repository>()
        .unwrap();
    assert!(Arc::ptr_eq(&alias, &typed));
}

#[tokio::test]
async fn failed_provider_registration_stops_boot_and_preserves_the_error_source() {
    struct Fail;
    impl ServiceProvider for Fail {
        fn register(&mut self, _: &Application) -> ProviderResult {
            Err(std::io::Error::other("private provider detail").into())
        }
        fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async { panic!("boot must not run after a failed registration") })
        }
    }
    let mut app = Application::configure(std::env::current_dir().unwrap())
        .withProviders(vec![Box::new(Fail)])
        .create()
        .unwrap();
    let failure = app.boot().await.unwrap_err();
    assert!(matches!(
        failure,
        openrust::foundation::BootstrapError::ProviderRegistrationFailed { .. }
    ));
    assert!(!failure.to_string().contains("private provider detail"));
    assert!(
        std::error::Error::source(&failure)
            .unwrap()
            .to_string()
            .contains("private provider detail")
    );
    assert!(!app.is_booted());
    assert!(matches!(
        app.boot().await,
        Err(openrust::foundation::BootstrapError::PreviousBootFailed)
    ));
    assert!(matches!(
        Messages::getFacadeRoot(),
        Err(BindingResolutionError::FacadeNotSet)
    ));
}
