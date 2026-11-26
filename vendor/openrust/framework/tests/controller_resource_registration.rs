use std::{ffi::OsString, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{
        ResourceAction, ResourceController, ResourceRegistration, RouteInfo, Router, RoutingError,
    },
    support::{ProviderResult, ServiceProvider, facades::Route},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Photos;
struct Posts;

impl Photos {
    fn handle<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { Response::json(StatusCode::OK, "photos") })
    }
}

impl Posts {
    fn handle<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { Response::json(StatusCode::OK, "posts") })
    }
}

fn all<C: Send + Sync + 'static>(
    action: for<'a> fn(&'a C, &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>>,
) -> ResourceController<C> {
    use ResourceAction::{Create, Destroy, Edit, Index, Show, Store, Update};
    [Index, Create, Store, Show, Edit, Update, Destroy]
        .into_iter()
        .fold(ResourceController::new(), |controller, key| {
            controller.action(key, action)
        })
}

fn entries(prefix: &str) -> [ResourceRegistration; 2] {
    [
        ResourceRegistration::new(format!("{prefix}photos"), all(Photos::handle)),
        ResourceRegistration::new(format!("{prefix}posts"), all(Posts::handle)),
    ]
}

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/controllers-tutorial/resource-batches-oracle.json"
    ))
    .unwrap()
}

fn compare_route_rows(routes: Vec<RouteInfo>) {
    let observed: Vec<_> = routes
        .into_iter()
        .map(|route| {
            let action = route.action.rsplit("::").next().unwrap();
            json!({
                "methods": route.method,
                "uri": route.uri.trim_start_matches('/'),
                "name": route.name,
                "action": action,
            })
        })
        .collect();
    let expected: Vec<_> = oracle()
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            json!({
                "methods": row["methods"],
                "uri": row["uri"],
                "name": row["name"],
                "action": row["action"],
            })
        })
        .collect();
    assert_eq!(observed, expected);
}

#[test]
fn router_batch_registration_matches_fixed_order_and_stops_at_first_error() {
    use ResourceAction::Index;
    let mut router = Router::new();
    router.resources(entries("")).unwrap();
    router.api_resources(entries("api/")).unwrap();
    router
        .soft_deletable_resources(entries("archive/"))
        .unwrap();
    compare_route_rows(router.get_routes());
    assert!(matches!(
        Route::resources(entries("")),
        Err(RoutingError::NoRegistrationScope)
    ));

    let mut router = Router::new();
    let result = router.resources([
        ResourceRegistration::new("first", all(Photos::handle).only(&[Index])),
        ResourceRegistration::new("broken", ResourceController::<Posts>::new()),
        ResourceRegistration::new("last", all(Photos::handle).only(&[Index])),
    ]);
    assert!(matches!(
        result,
        Err(RoutingError::MissingResourceAction { .. })
    ));
    assert_eq!(router.get_routes().len(), 1);
    assert_eq!(router.get_routes()[0].name.as_deref(), Some("first.index"));
}

struct FixtureProvider;

impl ServiceProvider for FixtureProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .bind::<Photos, _>(|_| Ok(Arc::new(Photos)))?;
            app.container().bind::<Posts, _>(|_| Ok(Arc::new(Posts)))?;
            Ok(())
        })
    }
}

fn web() -> Result<(), RoutingError> {
    Route::resources(entries(""))?;
    Route::api_resources(entries("api/"))?;
    Route::soft_deletable_resources(entries("archive/"))
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

#[tokio::test]
async fn facade_batch_registration_dispatches_distinct_controllers_over_tcp() {
    let temp = tempfile::tempdir().unwrap();
    let application = || {
        Application::configure(temp.path().to_owned())
            .with_providers(vec![Box::new(FixtureProvider)])
            .with_routing(Routing::default().web(web))
            .create()
            .unwrap()
    };
    assert_eq!(
        application()
            .handle_command(&[OsString::from("route:list"), OsString::from("--json")])
            .await
            .unwrap(),
        0
    );
    let mut kernel = Kernel::new(application());
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    for row in oracle().as_array().unwrap() {
        let method = row["methods"].as_str().unwrap().split('|').next().unwrap();
        let path = row["uri"]
            .as_str()
            .unwrap()
            .replace("{photo}", "1")
            .replace("{post}", "1");
        let body = row["action"]
            .as_str()
            .unwrap()
            .split('@')
            .next()
            .unwrap()
            .to_ascii_lowercase();
        timeout(Duration::from_secs(6), async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket.write_all(format!("{method} /{path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            let (head, actual) = response.split_once("\r\n\r\n").unwrap();
            assert!(head.starts_with("HTTP/1.1 200"), "{method} {path}: {response}");
            assert_eq!(serde_json::from_str::<Value>(actual).unwrap(), body, "{method} {path}");
        }).await.unwrap();
    }
    server.abort();
    let _ = server.await;
}

impl Photos {
    fn show_details<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let parameters: serde_json::Map<_, _> =
                ["photo", "comment", "project", "note", "tenant", "operator"]
                    .into_iter()
                    .filter_map(|name| {
                        request
                            .route(name)
                            .map(|value| (name.to_owned(), Value::from(value)))
                    })
                    .collect();
            Response::json(
                StatusCode::OK,
                json!({"action": "show", "parameters": parameters}),
            )
        })
    }
}

fn resource_options(router: &mut Router) -> Result<(), RoutingError> {
    use ResourceAction::{Create, Destroy, Index, Show, Store, Update};

    let resource = || all(Photos::show_details);
    router.resource("only/photos", resource().only(&[Index, Show]))?;
    router.resource(
        "except/photos",
        resource().except(&[Create, Store, Update, Destroy]),
    )?;
    router.resource("projects.notes", resource().only(&[Index, Show]))?;
    router.resource("photos.comments", resource().shallow())?;
    router.resource(
        "titles/photos",
        resource()
            .only(&[Index, Create, Show])
            .names(&[(Create, "old.build"), (Index, "old.home")])
            .names(&[(Create, "photos.build")])
            .name(Show, "photos.exhibit"),
    )?;
    router.resource(
        "admin/users",
        resource()
            .only(&[Show])
            .parameters(&[("users", "ignored")])
            .parameters(&[]),
    )?;
    router.resource(
        "custom/accounts.users",
        resource()
            .only(&[Show])
            .parameters(&[("users", "admin_user"), ("accounts", "tenant")])
            .parameter("users", "operator"),
    )?;
    let supplemental = || async {
        Response::json(
            StatusCode::OK,
            json!({"action": "Closure", "parameters": []}),
        )
    };
    router.get("/early/photos/popular", supplemental)?;
    router.resource("early/photos", resource().only(&[Show]))?;
    router.resource("late/photos", resource().only(&[Show]))?;
    router.get("/late/photos/popular", supplemental)?;
    Ok(())
}

fn options_oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/controllers-tutorial/resource-route-options-oracle.json"
    ))
    .unwrap()
}

#[test]
fn resource_route_options_follow_fixed_metadata() {
    let expected = options_oracle();
    let mut router = Router::new();
    resource_options(&mut router).unwrap();
    let rows: Vec<_> = router
        .get_routes()
        .into_iter()
        .map(|route| {
            let action = route
                .action
                .rsplit_once('@')
                .map_or("Closure", |(_, action)| action);
            json!({"methods": route.method, "uri": route.uri, "name": route.name, "action": action})
        })
        .collect();
    assert_eq!(Value::Array(rows), expected["routes"]);
}

fn options_facade() -> Result<(), RoutingError> {
    Route::group(openrust::routing::RouteGroup::new(), resource_options)
}

#[tokio::test]
async fn resource_route_options_keep_supplemental_order_and_nested_parameters_over_tcp() {
    let temp = tempfile::tempdir().unwrap();
    let application = || {
        Application::configure(temp.path().to_owned())
            .with_providers(vec![Box::new(FixtureProvider)])
            .with_routing(Routing::default().web(options_facade))
            .create()
            .unwrap()
    };
    assert_eq!(
        application()
            .handle_command(&[OsString::from("route:list"), OsString::from("--json")])
            .await
            .unwrap(),
        0
    );
    let mut kernel = Kernel::new(application());
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    for row in options_oracle()["requests"].as_array().unwrap() {
        let path = row["uri"].as_str().unwrap();
        timeout(Duration::from_secs(6), async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket
                .write_all(
                    format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            let (header, body) = response.split_once("\r\n\r\n").unwrap();
            assert!(header.starts_with("HTTP/1.1 200"), "{path}: {response}");
            assert_eq!(
                serde_json::from_str::<Value>(body).unwrap(),
                json!({"action":row["action"],"parameters":row["parameters"]}),
                "{path}"
            );
        })
        .await
        .unwrap();
    }
    server.abort();
    let _ = server.await;
}

impl openrust::routing::InvokableController for Photos {
    type Output = Response;

    fn invoke<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Self::Output, HttpError>> {
        self.handle(request)
    }
}

#[test]
fn invokable_dispatcher_registers_a_controller_type_without_a_method_pointer() {
    let mut router = Router::new();
    router
        .post(
            "/provision",
            openrust::routing::ControllerDispatcher::<Photos>::invokable(),
        )
        .unwrap();
    let routes = router.get_routes();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].action, std::any::type_name::<Photos>());
    assert_eq!(routes[0].method, "POST");
    assert_eq!(routes[0].uri, "provision");
}
