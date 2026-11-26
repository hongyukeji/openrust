use axum::handler::Handler as _;
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{ApiResponse, HttpError, HttpFuture, Request, Response},
    support::facades::{App, Route},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
#[path = "../app/http/controllers/api_controller.rs"]
pub(crate) mod api_controller;
#[path = "../app/http/controllers/controller.rs"]
mod controller;
#[path = "../app/models/mod.rs"]
pub(crate) mod models;
mod app {
    pub(crate) use crate::models;
    pub(crate) mod http {
        pub(crate) mod controllers {
            pub(crate) use crate::api_controller;
        }
    }
}
#[path = "../routes/api.rs"]
mod api;
#[path = "../config/mod.rs"]
mod config;
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "openrust-api-info-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Server {
    address: std::net::SocketAddr,
    task: Option<tokio::task::JoinHandle<()>>,
    _root: Root,
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
impl Server {
    async fn new(prefix: &str, extra: &str, enabled: bool) -> Self {
        let root = Root::new();
        std::fs::write(root.0.join(".env"),format!("APP_NAME=测试应用\nDB_DATABASE=api_info\nAPP_VERSION=1.2.3-test\nAPP_LOCALE=en\nAPP_FALLBACK_LOCALE=en\nAPP_SUPPORTED_LOCALES=*\nAPP_DEBUG=false\nAPI_PREFIX={prefix}\nAPI_RESPONSE_ENVELOPE={enabled}\n{extra}")).unwrap();
        let application = Application::configure(root.0.clone())
            .with_config(config::load)
            .with_routing(Routing::default().api(|| {
                api::register()?;
                extra_routes()
            }))
            .with_exceptions(|exceptions| {
                exceptions.should_render_json_when(|request, _| {
                    request.expects_json() || request.is_api_path()
                });
            })
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handler = |axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
                       incoming: axum::extract::Request| async move {
            let (parts, body) = incoming.into_parts();
            let mut request = Request::from_parts(parts, body);
            let (parts, body) = kernel.handle(&mut request).await.into_parts();
            axum::response::Response::from_parts(parts, body)
        };
        let service = handler.with_state(Arc::new(kernel)).into_make_service();
        let task = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        Self {
            address,
            task: Some(task),
            _root: root,
        }
    }
    async fn send(&self, method: &str, path: &str, locale: &str) -> (u16, String, Value) {
        let mut stream = tokio::net::TcpStream::connect(self.address).await.unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nAccept-Language: {locale}\r\nConnection: close\r\n\r\n",self.address).as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            stream.read_to_end(&mut bytes),
        )
        .await
        .unwrap()
        .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let (headers, body) = text.split_once("\r\n\r\n").unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (
            status,
            headers.to_ascii_lowercase(),
            if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(body).unwrap_or_else(|_| panic!("非法JSON响应：{text}"))
            },
        )
    }
    async fn close(mut self) {
        let task = self.task.take().unwrap();
        task.abort();
        let _ = task.await;
    }
}
fn error(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Err(HttpError::Unauthenticated) })
}
fn broken(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: std::io::Error::other("private-sentinel").into(),
        })
    })
}
fn switched(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let response = ApiResponse::empty();
        App::set_locale("zh-Hant").await?;
        response.into_response()
    })
}
struct MissingResource;
impl openrust::http::resources::json::JsonResource for MissingResource {
    fn to_array(&self, _: &Request) -> Result<Value, HttpError> {
        Ok(Value::Null)
    }
    fn status(&self) -> openrust::http::StatusCode {
        openrust::http::StatusCode::NOT_FOUND
    }
}
fn extra_routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/empty", ApiResponse::empty)?;
    Route::get("/null", || ApiResponse::success(Value::Null))?;
    Route::get("/values", || {
        ApiResponse::success(json!({"zero":0,"flag":false,"items":[],"object":{},"text":""}))
    })?;
    Route::get("/fields", || {
        ApiResponse::success(json!({"old":true}))
            .additional(
                json!({"trace":"first","temporary":true})
                    .as_object()
                    .unwrap()
                    .clone(),
            )
            .without_field("temporary")
            .without_field("message")
            .with_field("trace", "last")
            .without_field("data")
            .with_field(
                "data",
                json!({"new":true,"user":{"password":"secret"},"items":[{"id":1}]}),
            )
            .with_field("data.user.name", "张三")
            .without_field("data.user.password")
            .with_field("data.items.0.selected", true)
    })?;
    Route::get("/custom", || {
        ApiResponse::empty().with_message_key(
            "custom.greeting",
            BTreeMap::from([("name".into(), "Ada".into())]),
        )
    })?;
    Route::get("/literal", || {
        ApiResponse::empty().with_message("Literal message")
    })?;
    Route::get("/switch", switched)?;
    Route::get("/unauthenticated", error)?;
    Route::get("/broken", broken)?;
    Route::get("/page", || {
        openrust::http::resources::json::ResourceCollection::new(
            openrust::pagination::LengthAwarePaginator::new(
                vec![Plain(json!({"id":1}))],
                3,
                1,
                Some(1),
                openrust::pagination::PaginatorOptions {
                    path: Some("/api/page"),
                    ..Default::default()
                },
            )
            .unwrap(),
        )
    })?;
    Route::get("/json-api", || ProtocolResource)?;
    Route::get("/missing-resource", || MissingResource)?;
    Route::get("/validation", invalid)?;
    Route::get("/throttled", throttled)?;
    Ok(())
}
#[tokio::test]
async fn real_api_uses_configuration_localizes_and_keeps_top_level_edits() {
    let server = Server::new("/api", "", true).await;
    for (locale, message) in [
        ("en", "Success"),
        ("zh-Hans", "成功"),
        ("zh-Hant", "成功"),
        ("fr", "Succès"),
        ("ja", "成功"),
        ("de", "Erfolgreich"),
        ("es", "Operación completada"),
        ("it", "Operazione riuscita"),
        ("pt", "Sucesso"),
        ("nl", "Geslaagd"),
        ("ru", "Успешно"),
        ("hi", "सफल"),
        ("id", "Berhasil"),
        ("tr", "Başarılı"),
        ("vi", "Thành công"),
        ("ko", "성공"),
        ("ar", "تم بنجاح"),
        ("th", "สำเร็จ"),
    ] {
        let (status, headers, body) = server.send("GET", "/api", locale).await;
        assert_eq!(status, 200);
        assert!(headers.contains("content-type: application/json"));
        assert_eq!(
            body,
            json!({"success":true,"code":"ok","message":message,"data":{"name":"测试应用","version":"1.2.3-test"}})
        );
    }
    assert_eq!(
        server.send("GET", "/api/empty", "en").await.2,
        json!({"success":true,"code":"ok","message":"Success"})
    );
    assert_eq!(
        server.send("GET", "/api/null", "en").await.2,
        server.send("GET", "/api/empty", "en").await.2
    );
    assert_eq!(
        server.send("GET", "/api/values", "en").await.2["data"],
        json!({"zero":0,"flag":false,"items":[],"object":{},"text":""})
    );
    assert_eq!(
        server.send("GET", "/api/fields", "en").await.2,
        json!({"success":true,"code":"ok","trace":"last","data":{"new":true,"user":{"name":"张三"},"items":[{"id":1,"selected":true}]}})
    );
    assert_eq!(
        server.send("GET", "/api/switch", "en").await.2["message"],
        "成功"
    );
    assert_eq!(
        server.send("GET", "/api/empty", "en").await.2["message"],
        "Success"
    );
    assert_eq!(
        server.send("GET", "/api/literal", "zh-Hans").await.2["message"],
        "Literal message"
    );
    assert_eq!(server.send("HEAD", "/api", "en").await.2, Value::Null);
    let (english, chinese, german) = tokio::join!(
        server.send("GET", "/api/empty", "en"),
        server.send("GET", "/api/empty", "zh-Hans"),
        server.send("GET", "/api/empty", "de")
    );
    assert_eq!(english.2["message"], "Success");
    assert_eq!(chinese.2["message"], "成功");
    assert_eq!(german.2["message"], "Erfolgreich");
    server.close().await;
}
#[tokio::test]
async fn api_errors_use_semantic_status_and_do_not_leak_sources() {
    let server = Server::new("api/admin", "", true).await;
    let (status, _, body) = server.send("GET", "/api/admin", "zh-Hans").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        json!({"success":true,"code":"ok","message":"成功","data":{"name":"测试应用","version":"1.2.3-test"}})
    );
    for (method, path, status, message) in [
        ("POST", "/api/admin", 405, "Method Not Allowed"),
        ("GET", "/api/admin/missing", 404, "Not Found"),
        ("GET", "/api/admin/unauthenticated", 401, "Unauthenticated."),
        ("GET", "/api/admin/broken", 500, "Server Error"),
    ] {
        let (actual, headers, body) = server.send(method, path, "zh-Hans").await;
        assert_eq!(actual, status);
        assert_eq!(body, json!({"message":message}));
        if status == 405 {
            assert!(headers.contains("allow:"));
        }
    }
    assert_eq!(server.send("GET", "/api", "en").await.0, 404);
    server.close().await;
    let native = Server::new("api", "", false).await;
    let (status, _, body) = native.send("GET", "/api", "en").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        json!({"success":true,"code":"ok","message":"Success","data":{"name":"测试应用","version":"1.2.3-test"}})
    );
    let (status, _, body) = native.send("GET", "/api/missing", "en").await;
    assert_eq!(status, 404);
    assert_eq!(body, json!({"message":"Not Found"}));
    native.close().await;
}
#[tokio::test]
async fn custom_translation_parameters_and_fallback_use_existing_catalogs() {
    let server = Server::new("api", "", true).await;
    std::fs::create_dir_all(server._root.0.join("lang/en")).unwrap();
    std::fs::write(
        server._root.0.join("lang/en/custom.json"),
        r#"{"greeting":"Hello :name"}"#,
    )
    .unwrap();
    assert_eq!(
        server.send("GET", "/api/custom", "de").await.2["message"],
        "Hello Ada"
    );
    server.close().await;
}

struct Plain(Value);
impl openrust::http::resources::json::JsonResource for Plain {
    fn to_array(&self, _: &Request) -> openrust::http::resources::json::ResourceResult<Value> {
        Ok(self.0.clone())
    }
}
struct ProtocolResource;
impl openrust::http::resources::json::JsonResource for ProtocolResource {
    const JSON_API: bool = true;
    fn to_array(&self, _: &Request) -> openrust::http::resources::json::ResourceResult<Value> {
        Ok(json!({"type":"things","id":"1","attributes":{"name":"Example"}}))
    }
}
fn invalid(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let message = openrust::support::facades::Lang::get(
            "validation.required",
            &BTreeMap::from([("attribute".into(), "email".into())]),
            None,
        )?;
        let errors = openrust::support::MessageBag::new([("email", [message])]);
        Err(HttpError::Validation(Box::new(
            openrust::validation::ValidationException::new(errors),
        )))
    })
}
fn throttled(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut headers = openrust::http::HeaderMap::new();
        headers.insert("retry-after", "7".parse().unwrap());
        Err(HttpError::Throttled { headers })
    })
}
#[tokio::test]
async fn resource_pagination_errors_and_protocol_boundaries_share_real_http() {
    for legacy_enabled in [true, false] {
        let server = Server::new("api", "", legacy_enabled).await;
        let (status, _, page) = server.send("GET", "/api/page", "zh-Hans").await;
        assert_eq!(status, 200);
        assert!(page.get("message").is_none());
        assert!(page.get("code").is_none());
        assert!(page.get("success").is_none());
        assert_eq!(page["data"], json!([{"id":1}]));
        assert_eq!(page["meta"]["total"], 3);
        assert_eq!(page["meta"]["last_page"], 3);
        assert!(page["links"]["next"].is_string());
        let (status, _, body) = server.send("GET", "/api/missing-resource", "zh-Hans").await;
        assert_eq!(status, 404);
        assert_eq!(body, json!({"data":null}));
        let (status, _, body) = server.send("GET", "/api/validation", "zh-Hans").await;
        assert_eq!(status, 422);
        assert_eq!(
            body,
            json!({"message":"email 不能为空。","errors":{"email":["email 不能为空。"]}})
        );
        let (status, headers, body) = server.send("GET", "/api/throttled", "en").await;
        assert_eq!(status, 429);
        assert_eq!(body, json!({"message":"Too Many Attempts."}));
        assert!(headers.contains("retry-after: 7"));
        let (status, headers, body) = server.send("GET", "/api/json-api", "en").await;
        assert_eq!(status, 200);
        assert!(headers.contains("application/vnd.api+json"));
        assert!(body.get("success").is_none());
        assert_eq!(body["data"]["type"], "things");
        server.close().await;
    }
}

#[tokio::test]
async fn invalid_translation_source_becomes_a_safe_http_error() {
    let server = Server::new("api", "", true).await;
    std::fs::create_dir_all(server._root.0.join("lang/en")).unwrap();
    std::fs::write(
        server._root.0.join("lang/en/response.json"),
        "private-invalid-translation",
    )
    .unwrap();
    let (status, _, body) = server.send("GET", "/api/empty", "en").await;
    assert_eq!(status, 500);
    assert_eq!(body, json!({"message":"Server Error"}));
    server.close().await;
}
