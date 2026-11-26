use std::{ffi::OsString, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    container::ParameterValue,
    contracts::{
        container::Container as _,
        foundation::Application as _,
        http::{Kernel as _, Middleware},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::{ResourceAction, ResourceController, RouteGroup, RoutingError},
    support::{ProviderResult, ServiceProvider, facades::Route},
};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn trail(request: &Request) -> Vec<&'static str> {
    request
        .attributes()
        .get("trail")
        .map(|value| (*value.downcast::<Vec<&'static str>>().unwrap()).clone())
        .unwrap_or_default()
}

struct Trace(&'static str);
impl Middleware for Trace {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut path = trail(request);
            path.push(self.0);
            request
                .attributes()
                .insert("trail", ParameterValue::new(path));
            Ok(next.run(request).await)
        })
    }
}

struct FixtureProvider;
impl ServiceProvider for FixtureProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .bind::<Controller, _>(|_| Ok(Arc::new(Controller)))?;
            Ok(())
        })
    }
}

struct Controller;
impl Controller {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move { Response::json(StatusCode::OK, trail(request)) })
    }
}

fn actions(actions: &[ResourceAction]) -> ResourceController<Controller> {
    actions.iter().copied().fold(
        ResourceController::new().only(actions),
        |resource, action| resource.action(action, Controller::handle),
    )
}

fn web() -> Result<(), RoutingError> {
    use ResourceAction::{Destroy, Edit, Index, Show, Update};
    Route::group(
        RouteGroup::new()
            .middleware_name("group")
            .middleware_name("filtered")
            .middleware_name("removed"),
        |router| {
            router.resource(
                "entries",
                actions(&[Index, Show, Update, Destroy])
                    .middleware_for(&[Show], &["before", "common"])
                    .middleware(&["common", "tail"])
                    .middleware_for(&[Update, Destroy], &["early"])
                    .middleware_for(&[Update], &["late"])
                    .without_middleware(&["removed", "global"])
                    .without_middleware_for(&[Show, Update], &["filtered", "common"])
                    .without_middleware_for(&[Update], &["tail"]),
            )?;
            router.api_resource(
                "api-entries",
                actions(&[Index, Show])
                    .middleware(&["common"])
                    .middleware_for(&[Show], &["before"])
                    .without_middleware_for(&[Index], &["filtered"]),
            )?;
            router.singleton(
                "profile",
                actions(&[Show, Edit, Update])
                    .middleware(&["common"])
                    .middleware_for(&[Show, Update], &["before"])
                    .without_middleware_for(&[Edit], &["filtered"]),
            )?;
            router.api_singleton(
                "account",
                actions(&[Show, Update])
                    .middleware(&["common"])
                    .middleware_for(&[Show], &["before"])
                    .without_middleware_for(&[Update], &["group", "filtered"]),
            )
        },
    )
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

#[tokio::test]
async fn resource_middleware_matches_fixed_stack_and_route_only_exclusions_over_http() {
    let directory = tempfile::tempdir().unwrap();
    let application = || {
        Application::configure(directory.path().to_owned())
            .with_providers(vec![Box::new(FixtureProvider)])
            .with_routing(Routing::default().web(web))
            .with_middleware(|middleware| {
                let global = Arc::new(Trace("global"));
                middleware.alias("global", global.clone()).append(global);
                for name in [
                    "group", "filtered", "removed", "before", "common", "tail", "early", "late",
                ] {
                    middleware.alias(name, Arc::new(Trace(name)));
                }
            })
            .create()
            .unwrap()
    };
    assert_eq!(
        application()
            .handle_command(&[
                OsString::from("route:list"),
                OsString::from("--json"),
                OsString::from("-vv")
            ])
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
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/controllers-tutorial/resource-middleware-oracle.json"
    ))
    .unwrap();
    for row in oracle.as_array().unwrap() {
        let verb = row["methods"].as_str().unwrap().split('|').next().unwrap();
        let path = row["uri"]
            .as_str()
            .unwrap()
            .replace("{entry}", "1")
            .replace("{api_entry}", "1");
        let mut expected = row["middleware"].as_array().unwrap().clone();
        expected.insert(0, Value::String("global".into()));
        timeout(Duration::from_secs(6),async {
            let mut connection=TcpStream::connect(address).await.unwrap();
            connection.write_all(format!("{verb} /{path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut response=String::new();
            connection.read_to_string(&mut response).await.unwrap();
            let (header,body)=response.split_once("\r\n\r\n").unwrap();
            assert!(header.starts_with("HTTP/1.1 200"),"{verb} /{path}: {response}");
            assert_eq!(serde_json::from_str::<Value>(body).unwrap(),Value::Array(expected),"{verb} /{path}");
        }).await.unwrap();
    }
    server.abort();
    let _ = server.await;
}
