use std::{collections::BTreeMap, path::Path, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::Repository,
    config::{ConfigError, Environment},
    container::{
        AttributeParameter, ClassAttribute, ClassDescriptor, ConstructorParam, Container,
        attributes::{Cache, DB, Database, Log, Storage},
    },
    contracts::{
        cache::Repository as _,
        container::{BindingResolutionError, Container as _, ContextualAttribute},
        foundation::Application as _,
        http::Kernel as _,
    },
    database::Connection,
    filesystem::FilesystemAdapter,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    log::{LogContext, Logger},
    routing::RoutingError,
    support::facades::Route,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct Probe {
    default_db: Arc<Connection>,
    named_db: Arc<Connection>,
    cache: Arc<Repository>,
    memo: Arc<Repository>,
    storage: Arc<FilesystemAdapter>,
    logger: Arc<Logger>,
    renamed: Arc<Logger>,
}

fn descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete(
        vec![
            ConstructorParam::class::<Connection>("default_db")
                .withAttribute(ClassAttribute::contextual(|| Ok(Database::default()))),
            ConstructorParam::class::<Connection>("named_db")
                .withAttribute(ClassAttribute::contextual(|| Ok(DB::new("named")))),
            ConstructorParam::class::<Repository>("cache")
                .withAttribute(ClassAttribute::contextual(|| Ok(Cache::new("named")))),
            ConstructorParam::class::<Repository>("memo").withAttribute(
                ClassAttribute::contextual(|| Ok(Cache::new("named").memoized())),
            ),
            ConstructorParam::class::<FilesystemAdapter>("storage")
                .withAttribute(ClassAttribute::contextual(|| Ok(Storage::new("private")))),
            ConstructorParam::class::<Logger>("logger")
                .withAttribute(ClassAttribute::contextual(|| Ok(Log::new("audit")))),
            ConstructorParam::class::<Logger>("renamed").withAttribute(ClassAttribute::contextual(
                || Ok(Log::new("audit").named("injected")),
            )),
        ],
        |args| {
            Ok(Arc::new(Probe {
                default_db: args.service(0)?,
                named_db: args.service(1)?,
                cache: args.service(2)?,
                memo: args.service(3)?,
                storage: args.service(4)?,
                logger: args.service(5)?,
                renamed: args.service(6)?,
            }))
        },
    )
    .unwrap()
}

fn configuration(root: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let named_db = if let Some(url) = env.get("OPENRUST_TEST_MYSQL_URL") {
        json!({"driver":"mysql","url":url})
    } else {
        json!({"driver":"sqlite","database":":memory:"})
    };
    Ok([
        ("app".into(), json!({"env":"testing"})),
        ("cache".into(), json!({"default":"local","stores":{"local":{"driver":"array"},"named":{"driver":"array"}}})),
        ("database".into(), json!({"default":"primary","connections":{"primary":{"driver":"sqlite","database":":memory:"},"named":named_db}})),
        ("filesystems".into(), json!({"default":"local","disks":{
            "local":{"driver":"local","root":root.join("local")},
            "private":{"driver":"local","root":root.join("private")}
        }})),
        ("logging".into(), json!({"default":"audit","channels":{"audit":{"driver":"single","path":root.join("audit.log"),"name":"audit"}}})),
    ].into())
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/probe", probe)?;
    Route::get("/memo", memo)
}

fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let value = request.headers().get("x-probe").unwrap().to_str().unwrap();
        let container = Container::getInstance();
        let parameter = AttributeParameter {
            name: "invalid",
            variadic: false,
        };
        assert!(matches!(
            Database::new("missing").resolve(&container, parameter),
            Err(BindingResolutionError::FactoryFailed { service: "db", .. })
        ));
        assert!(matches!(
            Cache::new("missing").resolve(&container, parameter),
            Err(BindingResolutionError::FactoryFailed {
                service: "cache",
                ..
            })
        ));
        assert!(matches!(
            Storage::new("missing").resolve(&container, parameter),
            Err(BindingResolutionError::FactoryFailed {
                service: "filesystem",
                ..
            })
        ));
        let sync = container.make::<Probe>()?;
        let resolved = container.makeAsync::<Probe>().await?;
        assert!(Arc::ptr_eq(&sync.memo, &resolved.memo));
        assert!(Arc::ptr_eq(&sync.storage, &resolved.storage));
        assert_eq!(resolved.logger.name(), "audit");
        assert_eq!(resolved.renamed.name(), "injected");

        assert_eq!(
            resolved
                .named_db
                .select("SELECT 23 AS probe", &[])
                .await?
                .len(),
            1
        );
        resolved
            .storage
            .put("fixture", value)
            .await
            .map_err(internal)?;
        resolved
            .renamed
            .info("attribute probe", &LogContext::new())?;
        assert_eq!(resolved.memo.get("probe").await?, None);
        resolved.cache.put("probe", &value.into(), None).await?;
        // The memoized miss remains local to this request.
        assert_eq!(resolved.memo.get("probe").await?, None);
        Response::json(
            StatusCode::OK,
            json!({
                "default": resolved.default_db.getName(),
                "named": resolved.named_db.getName(),
            }),
        )
    })
}

fn memo(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let service = Container::getInstance().makeAsync::<Probe>().await?;
        let value = service.memo.get("probe").await?;
        Response::json(
            StatusCode::OK,
            value.as_ref().and_then(openrust::cache::Value::as_str),
        )
    })
}

fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(incoming).await
}

async fn server(root: &Path) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(root.to_owned())
        .withConfig(configuration)
        .withRouting(Routing::default().api(routes))
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
        tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }),
    )
}

async fn send(address: std::net::SocketAddr, path: &str, label: &str) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("GET /api/{path} HTTP/1.1\r\nHost: {address}\r\nX-Probe: {label}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("HTTP/1.1 200 "), "{response}");
        serde_json::from_str(body).unwrap()
    }).await.unwrap()
}

#[tokio::test]
async fn manager_attributes_use_the_current_application_and_share_request_memoization() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (left, left_task) = server(first.path()).await;
    let (right, right_task) = server(second.path()).await;
    let (left_probe, right_probe) =
        tokio::join!(send(left, "probe", "First"), send(right, "probe", "Second"));
    let names = json!({"default":"primary","named":"named"});
    assert_eq!(left_probe, names);
    assert_eq!(right_probe, names);
    let (left_memo, right_memo) = tokio::join!(send(left, "memo", ""), send(right, "memo", ""));
    assert_eq!(left_memo, "First");
    assert_eq!(right_memo, "Second");
    for (root, label) in [(first.path(), "First"), (second.path(), "Second")] {
        assert_eq!(
            std::fs::read_to_string(root.join("private/fixture")).unwrap(),
            label
        );
        assert!(!root.join("local/fixture").exists());
        let log = std::fs::read_to_string(root.join("audit.log")).unwrap();
        assert!(log.contains("injected.INFO: attribute probe"), "{log}");
    }
    left_task.abort();
    right_task.abort();
    let _ = left_task.await;
    let _ = right_task.await;
}
