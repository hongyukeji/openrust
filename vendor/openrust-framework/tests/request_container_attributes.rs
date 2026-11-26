use std::{
    io::{Read as _, Write as _},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{AuthIdentifier, AuthManager},
    container::{
        AttributeParameter, AttributeValue, ClassAttribute, ClassDescriptor, ConstructorParam,
        Container, ParameterValue,
        attributes::{
            Auth, Authenticated, CurrentUser, GuardHandle, RequestAttribute, RouteParameter,
        },
    },
    contracts::{
        auth::Authenticatable,
        container::{Container as _, ContextualAttribute},
        foundation::Application as _,
        http::{Kernel as _, Middleware},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{ApiResponse, HttpError, HttpFuture, Next, Request, Response},
    routing::{ControllerDispatcher, Router, RoutingError, middleware::SubstituteBindings},
};
use serde_json::{Value, json};

struct Item(String);
struct Probe(Arc<Item>, Arc<String>, Arc<String>, bool);
struct Identity(String);

impl Authenticatable for Identity {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.0.as_str().into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}

struct AuthProbe(
    Arc<GuardHandle>,
    Option<Arc<dyn Authenticatable>>,
    Option<Arc<dyn Authenticatable>>,
);

fn auth_descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete(
        vec![
            ConstructorParam::class::<GuardHandle>("guard")
                .with_attribute(ClassAttribute::contextual(|| Ok(Auth::default()))),
            ConstructorParam::class::<dyn Authenticatable>("user")
                .nullable()
                .with_default(|| Ok(ParameterValue::null()))
                .with_attribute(ClassAttribute::contextual(|| Ok(Authenticated::default()))),
            ConstructorParam::class::<dyn Authenticatable>("api_user")
                .nullable()
                .with_default(|| Ok(ParameterValue::null()))
                .with_attribute(ClassAttribute::contextual(|| Ok(CurrentUser::new("api")))),
        ],
        |args| {
            let user = |index| {
                if args.value(index)?.is_null() {
                    Ok(None)
                } else {
                    args.service::<dyn Authenticatable>(index).map(Some)
                }
            };
            Ok(Arc::new(AuthProbe(args.service(0)?, user(1)?, user(2)?)))
        },
    )
    .unwrap()
}

impl AuthProbe {
    fn show<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut other_request = Request::from_parts(
                http::Request::new(()).into_parts().0,
                axum::body::Body::empty(),
            );
            assert!(self.0.bind(&mut other_request).is_err());
            let id = |user: &Option<Arc<dyn Authenticatable>>| {
                user.as_ref()
                    .map(|user| user.get_auth_identifier().to_string())
            };
            let guard_id = self.0.bind(request)?.id().await?.map(|id| id.to_string());
            assert_eq!(guard_id, id(&self.1));
            if self.0.name() == "api"
                && let (Some(default), Some(api)) = (&self.1, &self.2)
            {
                assert!(Arc::ptr_eq(default, api));
            }
            ApiResponse::success(json!([self.0.name(), id(&self.1), id(&self.2)])).into_response()
        })
    }
}

fn descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete(
        vec![
            ConstructorParam::scalar::<Item>("item")
                .with_attribute(ClassAttribute::contextual(|| Ok(RouteParameter::new("id")))),
            ConstructorParam::scalar::<String>("slug")
                .with_attribute(ClassAttribute::contextual(|| Ok(RouteParameter::default()))),
            ConstructorParam::scalar::<String>("user").with_attribute(ClassAttribute::contextual(
                || Ok(RequestAttribute::new("trusted")),
            )),
            ConstructorParam::scalar::<String>("missing")
                .nullable()
                .with_attribute(ClassAttribute::contextual(|| {
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
            if request.is("selected/*") {
                let container = Container::get_instance();
                container
                    .make::<AuthManager>()
                    .unwrap()
                    .should_use(request, "api")?;
            }
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
            let current = Container::get_instance();
            let raw = route_value(&current, "id").downcast::<String>().unwrap();
            assert_eq!(raw.as_str(), binding.value());
            tokio::task::yield_now().await;
            Ok((binding.value() != "missing").then(|| Item(binding.value().to_owned())))
        })
    });
    router
        .get("/items/{id}/{slug}", probe)?
        .middleware(Arc::new(SubstituteBindings));
    router.get("/auth/{id}", ControllerDispatcher::new(AuthProbe::show))?;
    router.get("/selected/{id}", ControllerDispatcher::new(AuthProbe::show))?;
    router.get("/resolve/{id}", resolve_auth)?;
    router.get("/cancel/{id}", resolve_auth)?;
    router.get("/invalid-guard/{id}", invalid_guard)?;
    Ok(router)
}

fn invalid_guard(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let container = Container::get_instance();
        let attribute = ClassAttribute::contextual(|| Ok(Auth::new("unknown")));
        request
            .with_container(container.resolve_from_attribute_async(
                &attribute,
                AttributeParameter {
                    name: "guard",
                    variadic: false,
                },
            ))
            .await?;
        Ok(Response::empty(http::StatusCode::OK))
    })
}

fn resolve_auth(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let container = Container::get_instance();
        if request.is("cancel/*") {
            let started = Arc::new(tokio::sync::Notify::new());
            request
                .attributes()
                .insert("block-auth", ParameterValue::service(started.clone()));
            tokio::select! {
                result = request.with_container(container.make_async::<AuthProbe>()) => {
                    panic!("expected the request driver to block, resolved={}", result.is_ok());
                }
                () = started.notified() => {}
            }
            request.attributes().remove("block-auth");
        }
        let probe = request
            .with_container(container.make_async::<AuthProbe>())
            .await?;
        probe.show(request).await
    })
}

fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let current = Container::get_instance();
        let sync = current.make::<Probe>()?;
        let injected = current.make_async::<Probe>().await?;
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
        .with_config(|_, _| {
            Ok([(
                "auth".into(),
                json!({
                    "defaults": {"guard": "web"},
                    "guards": {
                        "web": {"driver": "header-web"},
                        "api": {"driver": "header-api"}
                    }
                }),
            )]
            .into())
        })
        .with_middleware(move |middleware| {
            middleware.append(Arc::new(Attach(label)));
        })
        .with_routing(Routing::new(routes))
        .with_exceptions(|exceptions| {
            exceptions.should_render_json_when(|_, _| true);
        })
        .create()
        .unwrap();
    app.container().register_class(descriptor()).unwrap();
    app.container().register_class(auth_descriptor()).unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let auth = kernel
        .get_application()
        .container()
        .make::<AuthManager>()
        .unwrap();
    for (driver, name) in [("header-web", "web"), ("header-api", "api")] {
        auth.via_request(driver, move |request| {
            Box::pin(async move {
                if let Some(started) = request.attributes().get("block-auth") {
                    started
                        .downcast::<Arc<tokio::sync::Notify>>()
                        .unwrap()
                        .notify_one();
                    std::future::pending::<()>().await;
                }
                tokio::task::yield_now().await;
                let id = request.route("id").unwrap();
                if id == "guest" {
                    return Ok(None);
                }
                let identity = format!("{label}-{name}-{id}");
                Ok(Some(
                    Box::new(Identity(identity)) as Box<dyn Authenticatable>
                ))
            })
        });
    }
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

#[tokio::test]
async fn authentication_attributes_use_the_live_request_and_selected_guard() {
    let (first, a) = server("first").await;
    let (second, b) = server("second").await;
    let (one, two, three, guest, cancelled) = tokio::join!(
        send(first, "/auth/one", 200),
        send(first, "/selected/two", 200),
        send(second, "/resolve/three", 200),
        send(first, "/auth/guest", 200),
        send(first, "/cancel/four", 200),
    );
    assert_eq!(
        one["data"],
        json!(["web", "first-web-one", "first-api-one"])
    );
    assert_eq!(
        two["data"],
        json!(["api", "first-api-two", "first-api-two"])
    );
    assert_eq!(
        three["data"],
        json!(["web", "second-web-three", "second-api-three"])
    );
    assert_eq!(guest["data"], json!(["web", null, null]));
    assert_eq!(
        cancelled["data"],
        json!(["web", "first-web-four", "first-api-four"])
    );
    send(first, "/invalid-guard/one", 500).await;
    for task in [a, b] {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}
