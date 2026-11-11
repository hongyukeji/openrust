use std::{
    io::{Read as _, Write as _},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    container::{
        AttributeParameter, AttributeValue, ClassAttribute, ClassDescriptor, ConstructorParam,
        Container, ParameterValue,
        attributes::{RequestAttribute, RouteParameter},
    },
    contracts::{
        container::{Container as _, ContextualAttribute},
        foundation::Application as _,
        http::{Kernel as _, Middleware},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{ApiResponse, HttpError, HttpFuture, Next, Request, Response},
    routing::{Router, RoutingError, middleware::SubstituteBindings},
};
use serde_json::{Value, json};

struct Item(String);
struct Probe(Arc<Item>, Arc<String>, Arc<String>, bool);

fn descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete(
        vec![
            ConstructorParam::scalar::<Item>("item")
                .withAttribute(ClassAttribute::contextual(|| Ok(RouteParameter::new("id")))),
            ConstructorParam::scalar::<String>("slug")
                .withAttribute(ClassAttribute::contextual(|| Ok(RouteParameter::default()))),
            ConstructorParam::scalar::<String>("user").withAttribute(ClassAttribute::contextual(
                || Ok(RequestAttribute::new("trusted")),
            )),
            ConstructorParam::scalar::<String>("missing")
                .nullable()
                .withAttribute(ClassAttribute::contextual(|| {
                    Ok(RequestAttribute::new("missing"))
                })),
        ],
        |args| {
            Ok(Arc::new(Probe(
                args.get(0)?,
                args.get(1)?,
                args.get(2)?,
                args.value(3)?.is_null(),
            )))
        },
    )
    .unwrap()
}

struct Attach(&'static str);

impl Middleware for Attach {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            assert!(request.route("id").is_none());
            request
                .attributes()
                .insert("trusted", ParameterValue::new(self.0.to_owned()));
            request
                .attributes()
                .insert("missing", ParameterValue::null());
            Ok(next.run(request).await)
        })
    }
}

fn route_value(container: &Container, name: &'static str) -> ParameterValue {
    let AttributeValue::One(value) = RouteParameter::default()
        .resolve(
            container,
            AttributeParameter {
                name,
                variadic: false,
            },
        )
        .unwrap()
    else {
        panic!("a route parameter supplies a single value");
    };
    value
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.bind("id", |binding| {
        Box::pin(async move {
            let current = Container::getInstance();
            let raw = route_value(&current, "id").downcast::<String>().unwrap();
            assert_eq!(raw.as_str(), binding.value());
            tokio::task::yield_now().await;
            Ok((binding.value() != "missing").then(|| Item(binding.value().to_owned())))
        })
    });
    router
        .get("/items/{id}/{slug}", probe)?
        .middleware(Arc::new(SubstituteBindings));
    Ok(router)
}

fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let current = Container::getInstance();
        let sync = current
            .make::<Probe>()
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        let injected =
            current
                .makeAsync::<Probe>()
                .await
                .map_err(|source| HttpError::Internal {
                    source: source.into(),
                })?;
        assert!(Arc::ptr_eq(&sync.0, &injected.0));
        assert!(std::ptr::eq(
            injected.0.as_ref(),
            request.route_model::<Item>("id").unwrap()
        ));
        assert!(route_value(&Container::default(), "id").is_null());
        assert!(route_value(&current, "absent").is_null());
        assert!(injected.3);
        ApiResponse::success(json!([
            injected.0.0,
            injected.1.as_str(),
            injected.2.as_str()
        ]))
        .into_response()
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let mut request = Request::from_parts(parts, body);
    let (parts, body) = kernel.handle(&mut request).await.into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn server(label: &'static str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .withMiddleware(move |middleware| {
            middleware.append(Arc::new(Attach(label)));
        })
        .withRouting(Routing::new(routes))
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|_, _| true);
        })
        .create()
        .unwrap();
    app.container().registerClass(descriptor()).unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

async fn send(address: SocketAddr, path: &'static str, status: u16) -> Value {
    tokio::task::spawn_blocking(move || {
        let timeout = Duration::from_secs(5);
        let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\nTrusted: spoofed\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with(&format!("HTTP/1.1 {status} ")), "{response}");
        serde_json::from_str(body).unwrap()
    }).await.unwrap()
}

#[tokio::test]
async fn contextual_attributes_observe_request_values_and_isolate_parallel_requests() {
    let (first, a) = server("first").await;
    let (second, b) = server("second").await;
    let (one, two, three) = tokio::join!(
        send(first, "/items/one/caf%C3%A9?trusted=spoofed", 200),
        send(first, "/items/two/beta", 200),
        send(second, "/items/three/gamma", 200)
    );
    assert_eq!(one["data"], json!(["one", "café", "first"]));
    assert_eq!(two["data"], json!(["two", "beta", "first"]));
    assert_eq!(three["data"], json!(["three", "gamma", "second"]));
    assert_eq!(
        send(first, "/items/missing/slug", 404).await["message"],
        "Not Found"
    );
    assert_eq!(
        send(first, "/items/one/again", 200).await["data"],
        json!(["one", "again", "first"])
    );
    for task in [a, b] {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}

#[test]
fn missing_http_scope_supplies_null() {
    let container = Container::default();
    let parameter = AttributeParameter {
        name: "trusted",
        variadic: false,
    };
    assert!(route_value(&container, "id").is_null());
    assert!(matches!(
        RequestAttribute::new("trusted").resolve(&container, parameter).unwrap(),
        AttributeValue::One(value) if value.is_null()
    ));
}
