use std::sync::{Arc, Mutex};

use openrust::{
    broadcasting::{BroadcastException, ChannelAuthorization, ChannelParameters},
    contracts::{
        auth::Authenticatable,
        http::{Kernel as _, Middleware},
        routing::Registrar as _,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Method, Next, Request, Response, StatusCode},
    routing::{MiddlewareEntry, RouteGroup, Router},
    support::facades::Broadcast,
};

type Trace = Arc<Mutex<Vec<(&'static str, bool)>>>;

struct Record {
    name: &'static str,
    stop: bool,
}

impl Middleware for Record {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let trace = request.parts().extensions.get::<Trace>().unwrap().clone();
            trace.lock().unwrap().push((self.name, true));
            if self.stop {
                return Ok(Response::empty(StatusCode::FORBIDDEN));
            }
            let response = next.run(request).await;
            trace.lock().unwrap().push((self.name, false));
            Ok(response)
        })
    }
}

fn record(name: &'static str) -> Arc<Record> {
    Arc::new(Record { name, stop: false })
}

fn read_parameter(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        request
            .parts()
            .extensions
            .get::<Trace>()
            .unwrap()
            .lock()
            .unwrap()
            .push(("handler", true));
        let value = request.route("name").unwrap();
        Ok(Response::new(StatusCode::OK, Body::from(value.to_owned())))
    })
}

fn explicit(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

#[tokio::test]
async fn exceptions_negotiate_json_without_changing_status_or_allow() {
    let app = Application::configure(std::env::current_dir().unwrap())
        .withRouting(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/api/page", explicit)?;
            Ok(router)
        }))
        .withExceptions(|exceptions| {
            exceptions
                .shouldRenderJsonWhen(|request, _| request.is("api/*") || request.expectsJson());
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();

    for (path, accept, json) in [
        ("/api/missing", "text/html", true),
        ("/ap%69/missing", "text/html", true),
        ("/api", "text/html", false),
        ("/missing", "text/html", false),
        ("/missing", "application/problem+json", true),
        ("/missing", "application/json;q=0.4, text/html;q=0.9", false),
        (
            "/missing",
            "text/html;profile=\"one,two;q=0\";q=0.4, application/json;q=0.9",
            true,
        ),
    ] {
        let (mut request, _) = request(Method::GET, path);
        request
            .parts_mut()
            .headers
            .insert("accept", accept.parse().unwrap());
        let mut response = kernel.handle(&mut request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.take_body(), 1024)
            .await
            .unwrap();
        if json {
            assert_eq!(response.headers()["content-type"], "application/json");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                serde_json::json!({"message":"Not Found"})
            );
        } else {
            assert_eq!(
                response.headers()["content-type"],
                "text/html; charset=utf-8"
            );
            assert!(String::from_utf8_lossy(&body).contains("404 Not Found"));
        }
    }

    let (mut wrong_method, _) = request(Method::POST, "/api/page");
    let response = kernel.handle(&mut wrong_method).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["allow"], "GET, HEAD");
}

#[tokio::test]
async fn exception_callbacks_match_sources_and_finalize_custom_and_default_responses() {
    use openrust::foundation::configuration::ReportDecision;

    fn fail(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            Err(HttpError::Internal {
                source: Box::new(std::io::Error::other("private exception detail")),
            })
        })
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = events.clone();
    let app = Application::configure(std::env::current_dir().unwrap())
        .withRouting(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/api/custom", fail)?;
            router.get("/api/fallback", fail)?;
            Ok(router)
        }))
        .withExceptions(move |exceptions| {
            let report_events = observed.clone();
            let render_events = observed.clone();
            exceptions
                .report::<std::io::Error>(move |_| {
                    report_events.lock().unwrap().push("report");
                    ReportDecision::Stop
                })
                .report::<HttpError>(|_| panic!("reporting should stop"))
                .render::<std::io::Error>(|_, _| None)
                .render::<std::io::Error>(move |_, request| {
                    render_events.lock().unwrap().push("render");
                    request.is("api/custom").then(|| {
                        Response::new(StatusCode::IM_A_TEAPOT, Body::from("custom response"))
                    })
                })
                .respond(move |mut response, _, _| {
                    observed.lock().unwrap().push("respond");
                    response
                        .parts_mut()
                        .headers
                        .insert("x-exception", "finalized".parse().unwrap());
                    response
                })
                .shouldRenderJsonWhen(|request, _| request.is("api/*") || request.expectsJson());
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();

    for (method, path, status, body) in [
        (
            Method::GET,
            "/api/custom",
            StatusCode::IM_A_TEAPOT,
            "custom response",
        ),
        (Method::HEAD, "/api/custom", StatusCode::IM_A_TEAPOT, ""),
        (
            Method::GET,
            "/api/fallback",
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"message":"Server Error"}"#,
        ),
    ] {
        let (mut request, _) = request(method, path);
        let (parts, response_body) = kernel.handle(&mut request).await.into_parts();
        assert_eq!(parts.status, status);
        assert_eq!(parts.headers["x-exception"], "finalized");
        assert_eq!(
            axum::body::to_bytes(response_body, 1024).await.unwrap(),
            body
        );
    }
    let (mut request, _) = request(Method::POST, "/api/custom");
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()["allow"], "GET, HEAD");
    assert_eq!(response.headers()["x-exception"], "finalized");
    assert_eq!(
        *events.lock().unwrap(),
        [
            "report", "render", "respond", "report", "render", "respond", "report", "render",
            "respond", "respond"
        ]
    );
}

fn request(method: Method, uri: &str) -> (Request, Trace) {
    let trace = Trace::default();
    let (parts, body) = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .extension(trace.clone())
        .body(Body::empty())
        .unwrap()
        .into_parts();
    (Request::from_parts(parts, body), trace)
}

#[tokio::test]
async fn declared_route_sources_get_their_framework_groups() {
    let application = Application::configure(std::env::current_dir().unwrap())
        .withMiddleware(|middleware| {
            middleware.group("web", [MiddlewareEntry::Instance(record("web"))]);
            middleware.group("api", [MiddlewareEntry::Instance(record("api"))]);
        })
        .withRouting(
            Routing::default()
                .web(|| openrust::support::facades::Route::get("/page", explicit))
                .api(|router| {
                    router.get("/page", explicit)?;
                    Ok(())
                })
                .health("/up"),
        )
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    for (path, group) in [("/page", "web"), ("/api/page", "api")] {
        let (mut request, trace) = request(Method::GET, path);
        assert_eq!(
            kernel.handle(&mut request).await.status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(*trace.lock().unwrap(), [(group, true), (group, false)]);
    }
    let (mut request, trace) = request(Method::GET, "/up");
    assert_eq!(kernel.handle(&mut request).await.status(), StatusCode::OK);
    assert!(trace.lock().unwrap().is_empty());
}

#[tokio::test]
async fn api_prefix_can_be_overridden_without_changing_the_middleware_group() {
    for (prefix, path) in [("/v1/api/", "/v1/api/page"), ("", "/page")] {
        let application = Application::configure(std::env::current_dir().unwrap())
            .withMiddleware(|middleware| {
                middleware.group("api", [MiddlewareEntry::Instance(record("api"))]);
            })
            .withRouting(
                Routing::default()
                    .api(|router| {
                        router.get("/page", explicit)?;
                        Ok(())
                    })
                    .apiPrefix(prefix),
            )
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();

        let (mut api_request, trace) = request(Method::GET, path);
        assert_eq!(
            kernel.handle(&mut api_request).await.status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(*trace.lock().unwrap(), [("api", true), ("api", false)]);

        let (mut default_request, _) = request(Method::GET, "/api/page");
        assert_eq!(
            kernel.handle(&mut default_request).await.status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn broadcasting_loads_channel_rules_and_denies_without_a_driver() {
    let register = || {
        Broadcast::channel(
            "App.Models.User.{id}",
            |_: &dyn Authenticatable, _: &ChannelParameters| Ok(ChannelAuthorization::Denied),
        )
    };
    assert!(matches!(
        register(),
        Err(BroadcastException::NoRegistrationScope)
    ));

    let application = Application::configure(std::env::current_dir().unwrap())
        .withBroadcasting(register)
        .withRouting(Routing::default())
        .withMiddleware(|middleware| {
            middleware.group("web", [MiddlewareEntry::Instance(record("web"))]);
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    for method in [Method::GET, Method::POST] {
        let (mut request, trace) = request(method, "/broadcasting/auth");
        assert_eq!(
            kernel.handle(&mut request).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(*trace.lock().unwrap(), [("web", true), ("web", false)]);
    }
}

async fn kernel() -> Kernel {
    let application = Application::configure(std::env::current_dir().unwrap())
        .withMiddleware(|middleware| {
            middleware
                .append(record("appended"))
                .prepend(record("prepended"));
        })
        .withRouting(Routing::new(|_| {
            let mut router = Router::new();
            router.group(
                RouteGroup::new()
                    .prefix("/api/")
                    .middleware(record("outer")),
                |router| {
                    router.group(
                        RouteGroup::new().prefix("v1").middleware(record("inner")),
                        |router| {
                            router
                                .get("/users/{name}", read_parameter)?
                                .middleware(record("route"));
                            router
                                .get("/blocked/{name}", read_parameter)?
                                .middleware(Arc::new(Record {
                                    name: "block",
                                    stop: true,
                                }))
                                .middleware(record("unreached"));
                            router.add_route(
                                &[Method::POST],
                                "/users/{name}",
                                Arc::new(explicit),
                            )?;
                            router.add_route(
                                &[Method::OPTIONS],
                                "/explicit",
                                Arc::new(explicit),
                            )?;
                            Ok(())
                        },
                    )
                },
            )?;
            router.get("/outside/{name}", read_parameter)?;
            Ok(router)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    kernel
}

#[tokio::test]
async fn decodes_parameters_and_runs_nested_middleware_in_order() {
    let kernel = kernel().await;
    let (mut request, trace) = request(Method::GET, "/api/v1/users/caf%C3%A9+tea/?ignored=yes");
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(request.route("name"), Some("café+tea"));
    assert_eq!(request.route("ignored"), None);
    let body = axum::body::to_bytes(response.into_parts().1, 1024)
        .await
        .unwrap();
    assert_eq!(&body[..], "café+tea".as_bytes());
    assert_eq!(
        *trace.lock().unwrap(),
        [
            ("prepended", true),
            ("appended", true),
            ("outer", true),
            ("inner", true),
            ("route", true),
            ("handler", true),
            ("route", false),
            ("inner", false),
            ("outer", false),
            ("appended", false),
            ("prepended", false),
        ]
    );
}

#[tokio::test]
async fn route_middleware_can_short_circuit_and_group_scope_ends() {
    let kernel = kernel().await;
    let (mut blocked, trace) = request(Method::GET, "/api/v1/blocked/alice");
    assert_eq!(
        kernel.handle(&mut blocked).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        *trace.lock().unwrap(),
        [
            ("prepended", true),
            ("appended", true),
            ("outer", true),
            ("inner", true),
            ("block", true),
            ("inner", false),
            ("outer", false),
            ("appended", false),
            ("prepended", false),
        ]
    );

    let (mut outside, trace) = request(Method::GET, "/outside/alice");
    assert_eq!(kernel.handle(&mut outside).await.status(), StatusCode::OK);
    assert_eq!(
        *trace.lock().unwrap(),
        [
            ("prepended", true),
            ("appended", true),
            ("handler", true),
            ("appended", false),
            ("prepended", false),
        ]
    );
}

#[tokio::test]
async fn reports_missing_routes_and_available_methods_through_global_middleware() {
    let kernel = kernel().await;
    for (method, path, expected, allow) in [
        (Method::POST, "/missing", StatusCode::NOT_FOUND, None),
        (
            Method::GET,
            "/api/v1/users/alice%2Fbob",
            StatusCode::NOT_FOUND,
            None,
        ),
        (
            Method::GET,
            "/api/v1/users/%FF",
            StatusCode::BAD_REQUEST,
            None,
        ),
        (
            Method::DELETE,
            "/api/v1/users/alice",
            StatusCode::METHOD_NOT_ALLOWED,
            Some("GET, HEAD, POST"),
        ),
        (
            Method::OPTIONS,
            "/api/v1/users/alice",
            StatusCode::OK,
            Some("GET,HEAD,POST"),
        ),
    ] {
        let (mut request, trace) = request(method, path);
        let response = kernel.handle(&mut request).await;
        assert_eq!(response.status(), expected, "{path}");
        assert_eq!(
            response
                .headers()
                .get("allow")
                .map(|value| value.to_str().unwrap()),
            allow
        );
        assert_eq!(
            *trace.lock().unwrap(),
            [
                ("prepended", true),
                ("appended", true),
                ("appended", false),
                ("prepended", false),
            ]
        );
    }
    let (mut request, _) = request(Method::OPTIONS, "/api/v1/explicit");
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.headers().get("allow").is_none());
}

#[tokio::test]
async fn head_runs_the_get_handler_and_suppresses_the_body() {
    let kernel = kernel().await;
    let (mut request, trace) = request(Method::HEAD, "/outside/alice");
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-length"], "5");
    assert!(
        axum::body::to_bytes(response.into_parts().1, 1024)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(trace.lock().unwrap().contains(&("handler", true)));
}

#[tokio::test]
async fn registration_order_and_per_method_replacement_are_preserved() {
    let mut router = Router::new();
    router.get("/items/{name}", read_parameter).unwrap();
    router.get("/items/special", explicit).unwrap();
    router
        .add_route(
            &[Method::GET, Method::GET],
            "/items/{name}",
            Arc::new(explicit),
        )
        .unwrap();
    router
        .get("/items/{name?}/{name?}", explicit)
        .err()
        .expect("reject duplicate optional parameter");

    let (mut get, _) = request(Method::GET, "/items/special");
    assert_eq!(
        router.dispatch(&mut get).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(get.route("name"), Some("special"));

    let (mut head, trace) = request(Method::HEAD, "/items/special");
    assert_eq!(
        router.dispatch(&mut head).await.unwrap().status(),
        StatusCode::OK
    );
    assert!(trace.lock().unwrap().contains(&("handler", true)));
}

struct Ranked<const ID: u8>(Record);

impl<const ID: u8> Middleware for Ranked<ID> {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        self.0.handle(request, next)
    }
}

fn ranked<const ID: u8>(name: &'static str, stop: bool) -> Arc<Ranked<ID>> {
    Arc::new(Ranked(Record { name, stop }))
}

fn show_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        request
            .parts()
            .extensions
            .get::<Trace>()
            .unwrap()
            .lock()
            .unwrap()
            .push(("handler", true));
        let body = ["id", "tab", "path"]
            .map(|name| request.route(name).unwrap_or("-"))
            .join("|");
        Ok(Response::new(StatusCode::OK, Body::from(body)))
    })
}

async fn network_kernel() -> Kernel {
    use openrust::routing::MiddlewareEntry;
    use std::any::TypeId;

    let app = Application::configure(std::env::current_dir().unwrap())
        .withMiddleware(|middleware| {
            middleware.append(ranked::<2>("global", false));
            middleware.alias("late", ranked::<2>("late", false));
            middleware.alias("stop", ranked::<1>("stop", true));
            middleware.group(
                "inner",
                [
                    MiddlewareEntry::named("late"),
                    MiddlewareEntry::Instance(record("unranked-a")),
                ],
            );
            middleware.group(
                "outer",
                [
                    MiddlewareEntry::named("inner"),
                    MiddlewareEntry::Instance(record("unranked-b")),
                ],
            );
            middleware.priority([
                TypeId::of::<Ranked<0>>(),
                TypeId::of::<Ranked<1>>(),
                TypeId::of::<Ranked<2>>(),
            ]);
        })
        .withRouting(Routing::new(|_| {
            let mut router = Router::new();
            router.pattern("id", "^[0-9]+$")?;
            router.group(
                RouteGroup::new()
                    .prefix("profiles")
                    .middleware_name("outer"),
                |router| {
                    router
                        .get("/{id?}/{tab?}", show_route)?
                        .defaults("tab", "overview")
                        .where_in("tab", &["activity", "settings"])?
                        .middleware_name("late")
                        .middleware(ranked::<0>("first", false));
                    router
                        .get("/blocked/{id?}", show_route)?
                        .middleware_name("stop")
                        .middleware(ranked::<0>("first", false));
                    Ok(())
                },
            )?;
            router
                .get("/format/{id}.{tab?}", show_route)?
                .where_alpha("id")?
                .where_alpha_numeric("tab")?;
            router
                .get("/files/{path?}", show_route)?
                .r#where("path", ".*")?;
            router.get("/choice/{id}", show_route)?;
            router
                .get("/choice/{path}", show_route)?
                .where_alpha("path")?;
            router
                .add_route(&[Method::POST], "/choice/{path}", Arc::new(explicit))?
                .where_alpha("path")?;
            router.get("/fixed/{id?}/end", show_route)?;
            router.get("/uuid/{id}", show_route)?.where_uuid("id")?;
            router.get("/ulid/{id}", show_route)?.where_ulid("id")?;
            router.get("/{id?}", show_route)?.where_number("id")?;
            Ok(router)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    kernel
}

async fn network_handler(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let trace = Trace::default();
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(trace.clone());
    let mut request = Request::from_parts(parts, body);
    let (mut parts, body) = kernel.handle(&mut request).await.into_parts();
    let events = trace
        .lock()
        .unwrap()
        .iter()
        .map(|(name, enter)| {
            if *enter {
                format!("+{name}")
            } else {
                format!("-{name}")
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    parts.headers.insert("x-trace", events.parse().unwrap());
    axum::response::Response::from_parts(parts, body)
}

#[tokio::test]
async fn real_http_requests_cover_optional_constraints_and_named_middleware_priority() {
    use axum::handler::Handler as _;
    use std::{
        io::{Read as _, Write as _},
        net::TcpStream,
        time::Duration,
    };

    let kernel = network_kernel().await;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = network_handler
        .with_state(Arc::new(kernel))
        .into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });

    tokio::task::spawn_blocking(move || {
        let send = |method: &str, path: &str| {
            let timeout = Duration::from_secs(5);
            let mut socket = TcpStream::connect_timeout(&address, timeout).unwrap();
            socket.set_read_timeout(Some(timeout)).unwrap();
            socket.set_write_timeout(Some(timeout)).unwrap();
            write!(socket, "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n").unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).unwrap();
            response
        };
        for (path, expected) in [
            ("/", "-|-|-"),
            ("/007", "007|-|-"),
            ("/profiles", "-|overview|-"),
            ("/profiles/7", "7|overview|-"),
            ("/profiles/%37/activity/?tab=ignored", "7|activity|-"),
            ("/format/Html", "Html|-|-"),
            ("/format/Html.v2", "Html|v2|-"),
            ("/choice/7", "7|-|-"),
            ("/choice/guest", "-|-|guest"),
            ("/fixed/3/end", "3|-|-"),
            ("/files/a%2Fb", "-|-|a/b"),
            ("/uuid/550e8400-e29b-41d4-a716-446655440000", "550e8400-e29b-41d4-a716-446655440000|-|-"),
            ("/ulid/01ARZ3NDEKTSV4RRFFQ69G5FAV", "01ARZ3NDEKTSV4RRFFQ69G5FAV|-|-"),
        ] {
            let response = send("GET", path);
            assert!(response.starts_with("HTTP/1.1 200 "), "{path}: {response}");
            assert_eq!(response.split_once("\r\n\r\n").unwrap().1, expected, "{path}");
        }
        for path in ["/profiles/nope", "/profiles/activity", "/profiles/7/unknown", "/profiles/7/overview", "/fixed/end", "/format/Html.v_2", "/uuid/7", "/ulid/81ARZ3NDEKTSV4RRFFQ69G5FAV"] {
            assert!(send("GET", path).starts_with("HTTP/1.1 404 "), "{path}");
            let response = send("POST", path);
            assert!(response.starts_with("HTTP/1.1 404 "), "{path}: {response}");
            assert!(!response.contains("\r\nallow:"));
        }

        let response = send("GET", "/profiles/7");
        assert!(response.contains("\r\nx-trace: +global,+first,+late,+unranked-a,+unranked-b,+handler,-unranked-b,-unranked-a,-late,-first,-global\r\n"), "{response}");
        let response = send("GET", "/profiles/blocked/7");
        assert!(response.starts_with("HTTP/1.1 403 "), "{response}");
        assert!(response.contains("\r\nx-trace: +global,+first,+stop,-first,-global\r\n"), "{response}");

        let response = send("POST", "/profiles/7");
        assert!(response.starts_with("HTTP/1.1 405 "), "{response}");
        assert!(response.contains("\r\nallow: GET, HEAD\r\n"), "{response}");
        assert!(response.contains("\r\nx-trace: +global,-global\r\n"), "{response}");
        for (path, allow) in [("/choice/7", "GET, HEAD"), ("/choice/guest", "GET, HEAD, POST")] {
            let response = send("DELETE", path);
            assert!(response.starts_with("HTTP/1.1 405 "), "{response}");
            assert!(response.contains(&format!("\r\nallow: {allow}\r\n")), "{response}");
        }
        let response = send("OPTIONS", "/profiles");
        assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        assert!(response.contains("\r\nallow: GET,HEAD\r\n"), "{response}");
    }).await.unwrap();
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn middleware_configuration_errors_fail_before_serving() {
    use openrust::{
        foundation::BootstrapError,
        routing::{MiddlewareEntry, RoutingError},
    };

    let routing = || {
        Routing::new(|_| {
            let mut router = Router::new();
            router.get("/", explicit)?.middleware_name("outer");
            Ok(router)
        })
    };

    let configure =
        || Application::configure(std::env::current_dir().unwrap()).withRouting(routing());
    let missing = configure()
        .withMiddleware(|middleware| {
            middleware.group("outer", [MiddlewareEntry::named("missing")]);
        })
        .create()
        .unwrap();
    assert!(
        matches!(Kernel::new(missing).bootstrap().await, Err(BootstrapError::RoutingFailed {
        source: RoutingError::UnknownMiddleware { name }
    }) if name == "missing")
    );

    let cyclic = configure()
        .withMiddleware(|middleware| {
            middleware.group("outer", [MiddlewareEntry::named("inner")]);
            middleware.group("inner", [MiddlewareEntry::named("outer")]);
        })
        .create()
        .unwrap();
    assert!(matches!(
        Kernel::new(cyclic).bootstrap().await,
        Err(BootstrapError::RoutingFailed {
            source: RoutingError::MiddlewareGroupCycle { .. }
        })
    ));
}

#[tokio::test]
async fn invalid_constraint_leaves_the_previous_pattern_active() {
    let mut router = Router::new();
    router.pattern("id", "[0-9]+").unwrap();
    let route = router.get("/items/{id}", explicit).unwrap();
    assert!(route.r#where("id", "(").is_err());
    let (mut matched, _) = request(Method::GET, "/items/7");
    assert_eq!(
        router.dispatch(&mut matched).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    let (mut invalid, _) = request(Method::GET, "/items/name");
    assert!(matches!(
        router.dispatch(&mut invalid).await,
        Err(HttpError::NotFound)
    ));
}
