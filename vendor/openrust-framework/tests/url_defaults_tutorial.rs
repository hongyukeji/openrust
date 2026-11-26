use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        container::Container as _,
        foundation::Application as _,
        http::{Kernel as _, Middleware},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::{RouteParameter, Router, SignatureMode, UrlValue, middleware::SubstituteBindings},
    support::{Uri, facades::Url},
};
use serde_json::{Value, json};
use std::{
    any::TypeId,
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
#[derive(Default)]
struct Trace(Mutex<Vec<&'static str>>);
struct Defaults(Arc<Trace>);
struct After(Arc<Trace>);
impl Middleware for Defaults {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push("defaults");
            Url::defaults(request, &[("locale", "fr")]);
            Ok(next.run(request).await)
        })
    }
}
impl Middleware for After {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push("after");
            Ok(next.run(request).await)
        })
    }
}
fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let params = [RouteParameter::named("article", "42")];
        let deadline = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(4_000_000_000);
        let signed = Uri::signed_route("article", &params, None, SignatureMode::Relative)?;
        assert_eq!(
            signed.value(),
            Url::signed_route("article", &params, None, SignatureMode::Relative)?
        );
        let temporary =
            Uri::temporary_signed_route("article", deadline, &params, SignatureMode::Relative)?;
        assert_eq!(
            temporary.value(),
            Url::temporary_signed_route("article", deadline, &params, SignatureMode::Relative)?
        );
        assert_eq!(
            Uri::action("ArticleController@show", &params, false)?.value(),
            "/fr/articles/42"
        );
        assert!(Uri::to("/posts", &[], None)?.value().ends_with("/posts"));
        let components = Uri::of("http://old.test/old")
            .unwrap()
            .with_scheme("https")
            .unwrap()
            .with_host("example.test")
            .unwrap()
            .with_port(Some(8443))
            .unwrap()
            .with_path("/new")
            .unwrap()
            .with_query(&json!({"a":"b"}))
            .unwrap()
            .with_fragment("section")
            .unwrap();
        assert_eq!(
            components.value(),
            "https://example.test:8443/new?a=b#section"
        );
        let link = Url::route("article", &params, false)?;
        let action = Url::action("ArticleController@show", &params, false)?;
        let uri = Uri::route("article", &params, false)?
            .with_query(&json!({"sort":"new"}))
            .unwrap()
            .with_fragment("part")
            .unwrap();
        let query = Url::query(
            "/posts?sort=old",
            &[
                RouteParameter::named("sort", "new"),
                RouteParameter::named(
                    "columns",
                    UrlValue::Array(vec![
                        RouteParameter::positional("title"),
                        RouteParameter::positional("body"),
                    ]),
                ),
            ],
            &[],
            None,
        )?;
        Response::json(
            StatusCode::OK,
            json!({"bound":request.route_model::<bool>("id"),"link":link,"action":action,"uri":uri.value(),"query":query}),
        )
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

async fn start(
    override_priority: bool,
) -> (
    tempfile::TempDir,
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<Trace>,
) {
    let root = tempfile::tempdir().unwrap();
    let trace = Arc::new(Trace::default());
    let app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"url":"https://example.test","key":"a".repeat(32),"locale":"en","supported_locales":[]}),
            )]))
        })
        .with_middleware(move |m| {
            m.prepend_to_priority_list(
                [TypeId::of::<SubstituteBindings>()],
                TypeId::of::<Defaults>(),
            );
            m.append_to_priority_list([TypeId::of::<SubstituteBindings>()], TypeId::of::<After>());
            if override_priority {
                m.priority([TypeId::of::<SubstituteBindings>()]);
            }
        })
        .with_routing(Routing::new(|app| {
            let mut r = Router::new();
            let trace = app.container().make::<Trace>().unwrap();
            let binding_trace = trace.clone();
            r.bind("id", move |_| {
                let trace = binding_trace.clone();
                Box::pin(async move {
                    trace.0.lock().unwrap().push("binding");
                    Ok(Some(true))
                })
            });
            r.get("/check/{id}", inspect)?
                .middleware(Arc::new(After(trace.clone())))
                .middleware(Arc::new(SubstituteBindings))
                .middleware(Arc::new(Defaults(trace)));
            r.get("/{locale}/articles/{article}", || {
                Response::empty(StatusCode::NO_CONTENT)
            })?
            .name("article")
            .set_action("ArticleController@show")?;
            Ok(r)
        }))
        .create()
        .unwrap();
    app.container().instance(trace.clone()).unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (root, addr, server, trace)
}
async fn send(
    address: SocketAddr,
    method: &str,
    target: &str,
    body: &str,
    headers: &str,
) -> (u16, Value, String) {
    let path = if target.starts_with("http") {
        let url = url::Url::parse(target).unwrap();
        format!(
            "{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        )
    } else {
        target.to_owned()
    };
    timeout(Duration::from_secs(5),async {
        let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let data=response.split_once("\r\n\r\n").unwrap().1;
        let value=if data.is_empty(){Value::Null}else{serde_json::from_str(data).unwrap()};(status,value,response)
    }).await.unwrap()
}

#[tokio::test]
async fn defaults_priority_survives_late_priority_configuration_and_reaches_actual_bindings() {
    for override_priority in [false, true] {
        let (_root, address, server, trace) = start(override_priority).await;
        let (status, value, _) = send(address, "GET", "/check/42", "", "").await;
        assert_eq!(status, 200, "{value}");
        assert_eq!(*trace.0.lock().unwrap(), ["defaults", "binding", "after"]);
        assert_eq!(value["bound"], true);
        assert_eq!(value["link"], "/fr/articles/42");
        assert_eq!(value["action"], "/fr/articles/42");
        assert_eq!(value["uri"], "/fr/articles/42?sort=new#part");
        assert_eq!(
            value["query"],
            format!("http://{address}/posts?sort=new&columns%5B0%5D=title&columns%5B1%5D=body")
        );
        server.abort();
        let _ = server.await;
    }
}
