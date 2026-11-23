mod support;
use openrust::{
    container::ServiceKey,
    contracts::{
        config::Repository as _, console::Kernel as _, container::Container as _,
        foundation::Application as _, http::Kernel as _, queue::JobHandler,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{DispatchOptions, JobContext, Payload, QueueFuture, QueueManager, QueueResult},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade, Route},
    },
};
use openrust_settings::{Settings, SettingsRepository, SettingsServiceProvider};
use serde_json::json;
use std::sync::{Arc, Mutex};
use support::{DeliverySettings, Fixture, Site};

struct SiteFacade;
impl Facade for SiteFacade {
    type Service = Settings<Site>;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Settings<Site>>()
    }
}
struct Host {
    repository: Arc<dyn SettingsRepository>,
    database: serde_json::Value,
    queue_table: String,
}
impl ServiceProvider for Host {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app
                .container()
                .get::<dyn openrust::contracts::config::Repository>("config")?;
            config.set("database", self.database.clone())?;
            config.set("queue",json!({"default":"database","connections":{"database":{"driver":"database","connection":"testing","table":self.queue_table,"queue":"default","retry_after":90}},"failed":{"driver":"null"}}))?;
            app.container()
                .instance::<dyn SettingsRepository>(self.repository.clone())?;
            SettingsServiceProvider::register_settings::<Site>(app).await?;
            SettingsServiceProvider::register_settings::<DeliverySettings>(app).await?;
            Ok(())
        })
    }
}
fn application(f: &Fixture) -> Application {
    Application::configure(std::env::temp_dir())
        .with_providers(vec![
            Box::new(SettingsServiceProvider),
            Box::new(Host {
                repository: f.repository.clone(),
                database: f.config.get("database").unwrap(),
                queue_table: format!("{}_jobs", f.table),
            }),
        ])
        .with_routing(Routing::default().web(routes))
        .create()
        .unwrap()
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/settings", read)?;
    Ok(())
}
fn read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let a = SiteFacade::get_facade_root_async().await.unwrap();
        let b = SiteFacade::get_facade_root_async().await.unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        Ok(Response::new(
            StatusCode::OK,
            Body::from(a.get().await.name),
        ))
    })
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = request.into_parts();
    let mut request = Request::from_parts(parts, body);
    let response = kernel.handle(&mut request).await;
    let (parts, body) = response.into_parts();
    axum::response::Response::from_parts(parts, body)
}
async fn serve(f: &Fixture) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    use axum::handler::Handler as _;
    let mut kernel = Kernel::new(application(f));
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }),
    )
}
async fn send(address: std::net::SocketAddr) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /settings HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    response.split_once("\r\n\r\n").unwrap().1.into()
}

#[tokio::test]
async fn real_http_requests_refresh_and_two_apps_remain_isolated() {
    let a = Fixture::new().await;
    let b = Fixture::new().await;
    let settings = a.site().await;
    settings.fill(json!({"name":"A"})).await.unwrap();
    settings.save().await.unwrap();
    let settings_b = b.site().await;
    settings_b.fill(json!({"name":"B"})).await.unwrap();
    settings_b.save().await.unwrap();
    let (a_addr, a_task) = serve(&a).await;
    let (b_addr, b_task) = serve(&b).await;
    let (one, two) = tokio::join!(send(a_addr), send(b_addr));
    assert_eq!((one.as_str(), two.as_str()), ("A", "B"));
    settings.fill(json!({"name":"A changed"})).await.unwrap();
    settings.save().await.unwrap();
    assert_eq!(send(a_addr).await, "A changed");
    assert_eq!(send(b_addr).await, "B");
    a_task.abort();
    b_task.abort();
    let _ = a_task.await;
    let _ = b_task.await;
    a.close().await;
    b.close().await;
}

struct Probe(Arc<Mutex<Vec<String>>>, Mutex<Option<Arc<Settings<Site>>>>);
impl JobHandler for Probe {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        _: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let a = SiteFacade::get_facade_root_async().await.unwrap();
            let b = SiteFacade::get_facade_root_async().await.unwrap();
            assert!(Arc::ptr_eq(&a, &b));
            let name = a.get().await.name;
            self.0.lock().unwrap().push(name);
            {
                let mut previous = self.1.lock().unwrap();
                if let Some(old) = previous.as_ref() {
                    assert!(!Arc::ptr_eq(old, &a));
                }
                *previous = Some(a.clone());
            }
            if a.get().await.name == "First" {
                a.fill(json!({"name":"Next"})).await.unwrap();
                a.save().await.unwrap();
            }
            Ok(())
        })
    }
}
#[tokio::test]
async fn real_worker_jobs_get_new_settings_on_the_next_attempt_scope() {
    let f = Fixture::new().await;
    let jobs = format!("{}_jobs", f.table);
    f.connection
        .get_schema_builder()
        .create(&jobs, |t| {
            t.id();
            t.string("queue", None);
            t.text("payload");
            t.unsigned_tiny_integer("attempts");
            t.unsigned_integer("reserved_at").nullable();
            t.unsigned_integer("available_at");
            t.unsigned_integer("created_at");
        })
        .await
        .unwrap();
    let mut app = application(&f);
    app.boot().await.unwrap();
    let queue = app.make::<QueueManager>().await.unwrap();
    queue.without_interruption_polling();
    let observed = Arc::new(Mutex::new(Vec::new()));
    queue
        .register(
            "settings-probe",
            Arc::new(Probe(observed.clone(), Mutex::new(None))),
        )
        .unwrap();
    let site = f.site().await;
    site.fill(json!({"name":"First"})).await.unwrap();
    site.save().await.unwrap();
    for _ in 0..2 {
        queue
            .dispatch(
                Payload::new("settings-probe", json!({})).unwrap(),
                DispatchOptions::default(),
                None,
            )
            .await
            .unwrap();
    }
    let args = [
        "queue:work",
        "--force",
        "--stop-when-empty",
        "--sleep=0",
        "--timeout=0",
        "--memory=0",
    ]
    .map(Into::into);
    assert_eq!(app.handle_command(&args).await.unwrap(), 0);
    assert_eq!(*observed.lock().unwrap(), vec!["First", "Next"]);
    f.connection
        .get_schema_builder()
        .drop_if_exists(&jobs)
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn provider_registration_and_artisan_help_do_not_query_missing_tables() {
    let f = Fixture::new().await;
    f.connection
        .get_schema_builder()
        .drop_if_exists(&f.table)
        .await
        .unwrap();
    let app = application(&f);
    let mut kernel =
        openrust::foundation::console::Kernel::new(app, openrust::console::Application::new());
    let mut output = openrust::console::Output::default();
    assert_eq!(kernel.handle(&["list".into()], &mut output).await, 0);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    f.close().await;
}

#[tokio::test]
async fn scoped_async_bindings_coalesce_and_duplicate_groups_are_rejected() {
    let f = Fixture::new().await;
    let mut app = application(&f);
    app.boot().await.unwrap();
    app.container()
        .scope(async {
            let (a, b) = tokio::join!(app.make::<Settings<Site>>(), app.make::<Settings<Site>>());
            assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
        })
        .await;
    assert!(
        SettingsServiceProvider::register_settings::<Site>(&app)
            .await
            .is_err()
    );
    let first = app
        .container()
        .scope(app.make::<Settings<Site>>())
        .await
        .unwrap();
    let second = app
        .container()
        .scope(app.make::<Settings<Site>>())
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    app.container()
        .scope(async {
            let module = app.make::<Settings<DeliverySettings>>().await.unwrap();
            module.fill(json!({"enabled": false})).await.unwrap();
            module.save().await.unwrap();
        })
        .await;
    app.container()
        .scope(async {
            let module = app.make::<Settings<DeliverySettings>>().await.unwrap();
            assert!(!module.get().await.enabled);
            assert!(
                app.make::<Settings<Site>>()
                    .await
                    .unwrap()
                    .get()
                    .await
                    .enabled
            );
        })
        .await;
    f.close().await;
}
