use openrust::{
    contracts::{
        container::Container as _, foundation::Application as _, http::Kernel as _,
        session::Session as _,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        Body, HttpError, HttpFuture, Method, Request, Response, RouteResolver, StatusCode,
        UserResolver,
    },
    support::facades::{Facade as _, Request as R, RequestHandle, Route},
    validation::{Rule, RuleSet},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

async fn input() -> Result<Value, HttpError> {
    assert_eq!(R::method()?, Method::POST);
    assert_eq!(
        R::header("content-type")?.as_deref(),
        Some("application/json")
    );
    assert!(R::has_header("content-type")?);
    assert_eq!(
        R::headers()?.get("content-type").unwrap(),
        "application/json"
    );
    assert!(R::is_json()?);
    assert_eq!(R::query(Some("q"))?, Some(json!("url")));
    assert_eq!(
        R::get_payload().await?,
        json!({"name":"Ada","n":"7","empty":null,"flag":"yes"})
    );
    assert_eq!(R::input("name").await?, Some(json!("Ada")));
    assert_eq!(R::input_or("missing", "fallback").await?, json!("fallback"));
    assert_eq!(R::input_or("empty", "fallback").await?, Value::Null);
    assert_eq!(R::query("q")?, Some(json!("url")));
    assert_eq!(R::query_or("missing", "fallback")?, json!("fallback"));
    assert_eq!(R::post("name").await?, Some(json!("Ada")));
    assert_eq!(R::post_or("missing", "fallback").await?, json!("fallback"));
    assert_eq!(R::post_or("empty", "fallback").await?, json!("fallback"));
    assert_eq!(R::integer_or("missing", 9).await?, 9);
    assert_eq!(R::integer_strict("name", 9).await?, 9);
    assert_eq!(R::float_or("missing", 1.5).await?, 1.5);
    assert_eq!(R::float_strict("name", 1.5).await?, 1.5);
    assert!(R::boolean_or("missing", true).await?);
    assert!(R::boolean_strict("name", true).await?);
    assert_eq!(
        R::string_or("missing", "fallback").await?.to_string(),
        "fallback"
    );
    assert_eq!(
        R::string_strict("empty", "fallback").await?.to_string(),
        "fallback"
    );
    assert_eq!(R::str("name").await?.to_string(), "Ada");
    assert_eq!(
        R::str_or("missing", "fallback").await?.to_string(),
        "fallback"
    );
    assert_eq!(
        R::str_strict("empty", "fallback").await?.to_string(),
        "fallback"
    );
    assert_eq!(R::integer("n").await?, 7);
    assert_eq!(R::float("n").await?, 7.);
    assert!(R::boolean("flag").await?);
    assert_eq!(R::string("name").await?.to_string(), "Ada");
    assert!(R::has(&["empty"]).await?);
    assert!(R::exists(&["empty"]).await?);
    assert!(!R::filled(&["empty"]).await?);
    assert!(R::is_not_filled(&["empty"]).await?);
    assert!(R::any_filled(&["name", "empty"]).await?);
    assert!(R::has_any(&["none", "name"]).await?);
    assert!(R::missing(&["none"]).await?);
    R::merge(json!({"profile.name":"Grace"})).await?;
    R::merge_if_missing(json!({"name":"Ignored","new":9})).await?;
    assert_eq!(R::input(Some("profile.name")).await?, Some(json!("Grace")));
    assert_eq!(R::input(Some("name")).await?, Some(json!("Ada")));
    assert_eq!(R::clamp("n", 0., 5., 0.).await?, 5.);
    assert_eq!(
        R::when_has("name", async |_| R::input(Some("name")).await)
            .await?
            .unwrap()?,
        Some(json!("Ada"))
    );
    assert_eq!(R::when_filled("empty", async |_| 1).await?, None);
    assert_eq!(R::when_missing("none", async || 7).await?, Some(7));
    let app = openrust::support::facades::App::get_facade_root()?;
    let container_root = app.container().get::<RequestHandle>("request")?;
    assert_eq!(
        container_root.with(|request| request.method().clone())?,
        Method::POST
    );
    let root = R::instance()?;
    assert_eq!(root.with(|request| request.method().clone())?, Method::POST);
    assert_eq!(
        root.with_async(|request| Box::pin(async { request.input(Some("name")).await }))
            .await?,
        Some(json!("Ada"))
    );
    root.with_async(|_| {
        Box::pin(async {
            assert!(R::input(Some("name")).await.is_err());
            Ok(())
        })
    })
    .await?;
    let snapshot = R::duplicate().await?;
    assert!(
        std::str::from_utf8(snapshot.body())
            .unwrap()
            .contains("Ada")
    );
    assert!(R::all().await?.get("profile").is_some());
    assert!(R::only(&["name"]).await?.get("name").is_some());
    assert!(R::except(&["name"]).await?.get("name").is_none());
    let before = R::json(None).await?;
    assert!(R::set_json(json!(123)).is_err());
    assert_eq!(R::json(None).await?, before);
    R::set_json(json!({"name":"Set"}))?;
    assert_eq!(R::input(Some("name")).await?, Some(json!("Set")));
    R::replace(json!({"name":"Lin"})).await?;
    Ok(
        json!({"name": R::input(Some("name")).await?, "q":R::input(Some("q")).await?, "new":R::input(Some("new")).await?}),
    )
}

async fn metadata() -> Result<Value, HttpError> {
    assert_eq!(R::get_host()?, "example.test");
    assert_eq!(R::get_http_host()?, "example.test:8080");
    assert_eq!(R::get_port()?, 8080);
    assert_eq!(R::get_scheme()?, "http");
    assert_eq!(R::get_protocol_version()?, "HTTP/1.1");
    assert!(R::is_method_safe()? && R::is_method_idempotent()? && R::is_method_cacheable()?);
    assert_eq!(R::get_query_string()?.as_deref(), Some("a=1&z=2"));
    assert_eq!(R::normalize_query_string("z=2&a=1")?, "a=1&z=2");
    assert_eq!(
        R::get_uri_for_path("/next")?,
        "http://example.test:8080/next"
    );
    assert_eq!(R::get_relative_uri_for_path("/other")?, "../other");
    assert_eq!(R::get_user()?.as_deref(), Some("user"));
    assert_eq!(R::get_password()?.as_deref(), Some("secret"));
    assert_eq!(R::get_e_tags()?, ["\"a\"", "W/\"b\""]);
    assert!(R::is_no_cache()?);
    assert_eq!(R::get_charsets()?, ["utf-8", "iso-8859-1"]);
    assert_eq!(R::get_encodings()?, ["br", "gzip"]);
    assert!(R::accepts_json()? && R::wants_json()? && R::expects_json()?);
    assert!(!R::accepts_html()?);
    R::set_format("custom", vec!["application/x-custom".into()])?;
    assert_eq!(
        R::get_mime_type("custom")?.as_deref(),
        Some("application/x-custom")
    );
    assert_eq!(
        R::get_format("application/x-custom; charset=utf-8", false)?.as_deref(),
        Some("custom")
    );
    R::set_format("json", vec!["application/x-json-replacement".into()])?;
    assert!(R::get_format("application/json", false)?.is_none());
    R::set_format("z-last", vec!["application/x-shared".into()])?;
    R::set_format("a-later", vec!["application/x-shared".into()])?;
    assert_eq!(
        R::get_format("application/x-shared", false)?.as_deref(),
        Some("a-later")
    );
    R::set_request_format(Some("custom"))?;
    assert_eq!(
        R::get_preferred_format(Some("html"))?.as_deref(),
        Some("custom")
    );
    R::set_default_request_locale("zh-Hans")?;
    assert_eq!(R::get_locale()?, "zh-Hans");
    R::set_request_locale("en")?;
    assert_eq!(R::get_default_locale()?, "zh-Hans");
    assert_eq!(R::get_locale()?, "en");
    assert!(R::set_locale("zh_CN").is_err());
    assert_eq!(R::get_locale()?, "en");
    assert_eq!(
        R::get_format("application/vnd.example+json", false)?.as_deref(),
        Some("json")
    );
    assert_eq!(
        R::get_format("text/x-custom", true)?.as_deref(),
        Some("custom")
    );
    R::set_method(Method::PUT)?;
    assert_eq!(R::get_method()?, Method::PUT);
    assert_eq!(R::get_real_method()?, Method::PUT);
    assert!(!R::is_method_safe()? && R::is_method_idempotent()?);
    Ok(json!({"uri":R::get_uri()?,"path":R::get_path_info()?,"method":R::get_method()?.as_str()}))
}

async fn macros() -> Result<Value, HttpError> {
    R::r#macro("greet", |args| async move {
        Ok(json!({"name":args[0],"path":R::path()?}))
    })?;
    assert!(R::has_macro("greet")?);
    R::mixin(
        [(
            "greet",
            openrust::http::RequestMacro::new(|_| async { Ok(json!("overwritten")) }),
        )],
        false,
    )?;
    let value = R::call("greet", vec![json!("Ada")]).await?;
    R::flush_macros()?;
    assert!(!R::has_macro("greet")?);
    assert!(R::call("greet", vec![]).await.is_err());
    Ok(value)
}

async fn upload() -> Result<Value, HttpError> {
    assert!(R::has_file("document").await?);
    let file = R::file("document").await?.unwrap();
    assert!(R::all().await?.get("document").is_some());
    assert!(R::file_value("document").await?.is_some());
    Ok(json!({"filename":file.get_client_original_name(),"title":R::input(Some("title")).await?}))
}

async fn validation() -> Result<Value, HttpError> {
    let rules = RuleSet::new().field("name", vec![Rule::required()])?;
    Ok(R::validate_with_bag("profile", rules).await?.into_value())
}

fn borrowed(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        // 借出原请求，保证静态修改恢复后仍能从注入实例观察到。
        let saved = request
            .with_container(async {
                R::merge(json!({"name":"changed"})).await?;
                R::set_route_resolver(RouteResolver(Arc::new(|| openrust::http::ResolvedRoute {
                    name: Some("custom.show".into()),
                    models: BTreeMap::from([(
                        "bound".into(),
                        openrust::container::ParameterValue::new(42_i64),
                    )]),
                    parameters: BTreeMap::from([("id".into(), "42".into())]),
                    ..Default::default()
                })))?;
                R::set_user_resolver(UserResolver(Arc::new(|request, _| {
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        assert!(request.header("content-type")?.is_some());
                        Ok(None)
                    })
                })))?;
                assert!(R::user(None).await?.is_none());
                assert!(R::get_user_resolver()?.is_some());
                assert_eq!(
                    R::route()?
                        .unwrap()
                        .parameters
                        .get("id")
                        .map(String::as_str),
                    Some("42")
                );
                assert!(R::route_is(&["custom.*"])?);
                Ok::<_, HttpError>(R::get_facade_root()?)
            })
            .await?;
        assert!(saved.with(|request| request.path().to_owned()).is_err());
        request
            .with_container(async {
                assert_eq!(saved.with(|request| request.path().to_owned())?, R::path()?);
                Ok::<_, HttpError>(())
            })
            .await?;
        assert_eq!(request.route("id"), Some("42"));
        assert_eq!(request.route_model::<i64>("bound"), Some(&42));
        let Some(openrust::http::RequestField::RouteModel(model)) = request.field("bound").await?
        else {
            panic!("自定义路由模型没有接入请求字段")
        };
        assert_eq!(*model.downcast::<i64>()?, 42);
        Response::json(
            StatusCode::OK,
            json!({"name":request.input(Some("name")).await?}),
        )
    })
}

fn cancellation(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let root = Arc::new(std::sync::Mutex::new(None));
        let saved = root.clone();
        let timeout = tokio::time::timeout(
            std::time::Duration::from_millis(10),
            request.with_container(async move {
                *saved.lock().unwrap() = Some(R::instance()?);
                R::instance()?
                    .with_async(|request| {
                        Box::pin(async {
                            request.merge(json!({"name":"cancelled"})).await?;
                            std::future::pending::<Result<(), HttpError>>().await
                        })
                    })
                    .await
            }),
        )
        .await;
        assert!(timeout.is_err());
        assert!(
            root.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .with(|request| request.path().to_owned())
                .is_err()
        );
        Response::json(
            StatusCode::OK,
            json!({"name":request.input(Some("name")).await?}),
        )
    })
}

fn session(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut store = openrust::session::Store::new(
            "custom_session",
            Box::new(openrust::session::ArraySessionHandler::new(
                std::time::Duration::from_secs(60),
            )),
        )?;
        store.start().await?;
        request
            .with_container(async move {
                R::set_laravel_session(store)?;
                assert!(R::has_session()?);
                assert!(R::has_previous_session()?);
                R::session(|session| session.put("visited", json!(true)))??;
                assert_eq!(
                    R::get_session(|session| session.get_name().to_owned())?,
                    "custom_session"
                );
                R::flash_only(&["name"]).await?;
                assert_eq!(R::old(Some("name"))?, Some(json!("Ada")));
                R::flush()?;
                assert!(R::old(Some("name"))?.is_none());
                R::flash().await?;
                R::flash_except(&["secret"]).await?;
                assert!(R::old(Some("secret"))?.is_none());
                Ok::<_, HttpError>(())
            })
            .await?;
        assert_eq!(request.session()?.get("visited"), Some(&json!(true)));
        Response::json(
            StatusCode::OK,
            json!({"name":request.old(Some("name")),"secret":request.old(Some("secret"))}),
        )
    })
}

struct FacadeController {
    request: Arc<RequestHandle>,
}
impl FacadeController {
    fn index(&self) -> HttpFuture<'_, Result<Value, HttpError>> {
        Box::pin(async {
            assert_eq!(
                self.request.with(|request| request.path().to_owned())?,
                R::path()?
            );
            Ok(json!({"path":R::path()?,"query":R::query(Some("name"))?}))
        })
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/reinitialize/{id}", reinitialize)?;
    Route::get(
        "/controller",
        openrust::routing::ControllerDispatcher::without_request(FacadeController::index),
    )?;
    Route::post("/input", input)?;
    Route::any("/override", || async {
        Ok::<_, HttpError>(
            json!({"method":R::method()?.as_str(),"real":R::get_real_method()?.as_str(),"parameter":R::get_http_method_parameter_override()?}),
        )
    })?;
    Route::get("/macro-register", || async {
        R::r#macro("isolated", |_| async { Ok(Value::Null) })?;
        Ok::<_, HttpError>(json!(true))
    })?;
    Route::get("/macro-has", || async {
        Ok::<_, HttpError>(json!(R::has_macro("isolated")?))
    })?;
    Route::post("/session", session)?;
    Route::get("/metadata", metadata)?;
    Route::get("/macro", macros)?;
    Route::post("/upload", upload)?;
    Route::post("/validate", validation)?;
    Route::post("/borrowed", borrowed)?;
    Route::post("/cancel", cancellation)?;
    Route::get("/swap", || async {
        let replacement = R::create(
            http::Request::builder()
                .uri("/replacement")
                .header("host", "example.test:8080")
                .body(Body::empty())
                .unwrap(),
        );
        R::swap(Arc::new(RequestHandle::from_request(replacement)))?;
        R::clear_resolved_instances()?;
        assert!(
            openrust::support::facades::Url::current()
                .unwrap()
                .ends_with("/replacement")
        );
        Ok::<_, HttpError>(json!({"path":R::path()?}))
    })?;
    Ok(())
}

async fn kernel() -> (tempfile::TempDir, Kernel) {
    kernel_with_policy(openrust::http::MethodOverride::default()).await
}
async fn kernel_with_policy(policy: openrust::http::MethodOverride) -> (tempfile::TempDir, Kernel) {
    let root = tempfile::tempdir().unwrap();
    let app=Application::configure(root.path().to_path_buf()).with_config(|_,_|Ok(BTreeMap::from([
        ("app".into(),json!({"env":"testing","key":"a".repeat(32),"locale":"en","supported_locales":[]})),
        ("session".into(),json!({"driver":"array"})),
    ]))).with_middleware(move |middleware| { middleware.method_override(policy); middleware.prevent_request_forgery(["*"], false, false); })
        .with_routing(Routing::default().api(routes).web(web_routes)).create().unwrap();
    app.container()
        .bind::<FacadeController, _>(|container| {
            Ok(Arc::new(FacadeController {
                request: container.get::<RequestHandle>("request")?,
            }))
        })
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    (root, kernel)
}
async fn call(
    kernel: &Kernel,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (StatusCode, Value) {
    let mut builder = http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "example.test:8080");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let mut request = Request::from(builder.body(Body::from(body.to_owned())).unwrap());
    let response = kernel.handle(&mut request).await;
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_parts().1, 65536)
        .await
        .unwrap();
    assert!(R::instance().is_err());
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)})),
    )
}

#[tokio::test]
async fn facade_input_mutates_original_body_and_releases_borrows_before_callbacks() {
    let (_root, k) = kernel().await;
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/input?q=url",
        &[("content-type", "application/json")],
        r#"{"name":"Ada","n":"7","empty":null,"flag":"yes"}"#,
    )
    .await;
    assert_eq!(
        (status, value),
        (StatusCode::OK, json!({"name":"Lin","q":"url","new":null}))
    );
}
#[tokio::test]
async fn facade_protocol_url_format_and_locale_apis_are_live() {
    let (_root, k) = kernel().await;
    let (status, value) = call(
        &k,
        Method::GET,
        "/api/metadata?z=2&a=1",
        &[
            ("authorization", "Basic dXNlcjpzZWNyZXQ="),
            ("accept", "application/json"),
            ("cache-control", "no-cache"),
            ("if-none-match", "\"a\", W/\"b\""),
            ("accept-charset", "iso-8859-1;q=0.5,utf-8"),
            ("accept-encoding", "gzip;q=0.5,br"),
        ],
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["method"], "PUT");
}
#[tokio::test]
async fn request_macros_can_reenter_facade_and_do_not_hold_registry_locks() {
    let (_root, k) = kernel().await;
    let (status, value) = call(&k, Method::GET, "/api/macro", &[], "").await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value, json!({"name":"Ada","path":"api/macro"}));
}
#[tokio::test]
async fn injected_request_observes_facade_changes_and_handles_expire() {
    let (_root, k) = kernel().await;
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/borrowed",
        &[("content-type", "application/json")],
        "{}",
    )
    .await;
    assert_eq!((status, value), (StatusCode::OK, json!({"name":"changed"})));
}
#[tokio::test]
async fn cancellation_restores_live_request_after_inflight_facade_borrow() {
    let (_root, k) = kernel().await;
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/cancel",
        &[("content-type", "application/json")],
        "{}",
    )
    .await;
    assert_eq!(
        (status, value),
        (StatusCode::OK, json!({"name":"cancelled"}))
    );
}
#[tokio::test]
async fn typed_request_replacement_and_facade_cache_clear_are_effective() {
    let (_root, k) = kernel().await;
    let (status, value) = call(&k, Method::GET, "/api/swap", &[], "").await;
    assert_eq!(
        (status, value),
        (StatusCode::OK, json!({"path":"replacement"}))
    );
}
#[tokio::test]
async fn validation_errors_keep_json_422_and_named_bag() {
    let (_root, k) = kernel().await;
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/validate",
        &[
            ("content-type", "application/json"),
            ("accept", "application/json"),
        ],
        "{}",
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    assert!(value["errors"]["name"].is_array());
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/validate",
        &[("content-type", "application/json")],
        r#"{"name":"Ada"}"#,
    )
    .await;
    assert_eq!((status, value), (StatusCode::OK, json!({"name":"Ada"})));
}
#[tokio::test]
async fn uploads_are_read_from_original_multipart_body() {
    let (_root, k) = kernel().await;
    let body = "--boundary\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nReport\r\n--boundary\r\nContent-Disposition: form-data; name=\"document\"; filename=\"report.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--boundary--\r\n";
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/upload",
        &[("content-type", "multipart/form-data; boundary=boundary")],
        body,
    )
    .await;
    assert_eq!(
        (status, value),
        (
            StatusCode::OK,
            json!({"filename":"report.txt","title":"Report"})
        )
    );
}

#[tokio::test]
async fn facade_session_and_flash_modify_the_injected_session() {
    let (_root, k) = kernel().await;
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/session",
        &[
            ("content-type", "application/json"),
            ("cookie", "custom_session=previous"),
        ],
        r#"{"name":"Ada","secret":"hidden"}"#,
    )
    .await;
    assert_eq!(
        (status, value),
        (StatusCode::OK, json!({"name":"Ada","secret":null}))
    );
}

#[test]
fn native_construction_preserves_explicit_transport_state() {
    let incoming = http::Request::builder()
        .method(Method::PATCH)
        .uri("/created?q=1")
        .header("x-test", "explicit")
        .body(Body::from("body"))
        .unwrap();
    let request = R::create_from(R::create_from_base(incoming));
    assert_eq!(request.method(), Method::PATCH);
    assert_eq!(request.header("x-test").unwrap(), Some("explicit"));
    assert_eq!(request.query(Some("q")).unwrap(), Some(json!("1")));
}

#[tokio::test]
async fn invalid_json_in_get_payload_is_an_http_400_not_a_panic() {
    let (_root, k) = kernel().await;
    let (status, _) = call(
        &k,
        Method::POST,
        "/api/input",
        &[
            ("content-type", "application/json"),
            ("accept", "application/json"),
        ],
        "{broken",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn typed_controllers_without_request_arguments_can_use_the_facade() {
    let (_root, k) = kernel().await;
    let (status, value) = call(&k, Method::GET, "/api/controller?name=Ada", &[], "").await;
    assert_eq!(
        (status, value),
        (
            StatusCode::OK,
            json!({"path":"api/controller","query":"Ada"})
        )
    );
}

fn web_routes() -> Result<(), openrust::routing::RoutingError> {
    Route::post("/form", validation)?;
    Route::get("/form", || async {
        Ok::<_, HttpError>(json!({"errors":R::session(|session| session.get("errors").cloned())?}))
    })?;
    Ok(())
}

#[tokio::test]
async fn method_override_policy_is_per_application_and_headers_remain_available() {
    let (_root, k) = kernel_with_policy(
        openrust::http::MethodOverride::new(false, Some(vec![Method::PUT])).unwrap(),
    )
    .await;
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/override",
        &[("content-type", "application/x-www-form-urlencoded")],
        "_method=PUT",
    )
    .await;
    assert_eq!(
        (status, value),
        (
            StatusCode::OK,
            json!({"method":"POST","real":"POST","parameter":false})
        )
    );
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/override",
        &[("x-http-method-override", "PUT")],
        "",
    )
    .await;
    assert_eq!(
        (status, value),
        (
            StatusCode::OK,
            json!({"method":"PUT","real":"POST","parameter":false})
        )
    );
    let (status, value) = call(
        &k,
        Method::POST,
        "/api/override",
        &[("x-http-method-override", "DELETE")],
        "",
    )
    .await;
    assert_eq!(
        (status, value),
        (
            StatusCode::OK,
            json!({"method":"POST","real":"POST","parameter":false})
        )
    );
    assert!(openrust::http::MethodOverride::new(true, Some(vec![Method::GET])).is_err());
    let (_root, default) = kernel().await;
    let (_, value) = call(
        &default,
        Method::POST,
        "/api/override",
        &[("content-type", "application/x-www-form-urlencoded")],
        "_method=DELETE",
    )
    .await;
    assert_eq!(
        value,
        json!({"method":"DELETE","real":"POST","parameter":true})
    );
}

#[tokio::test]
async fn macros_do_not_leak_between_application_containers() {
    let (_root, a) = kernel().await;
    let (_other, b) = kernel().await;
    assert_eq!(
        call(&a, Method::GET, "/api/macro-register", &[], "")
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&b, Method::GET, "/api/macro-has", &[], "").await.1,
        json!(false)
    );
    assert_eq!(
        call(&a, Method::GET, "/api/macro-has", &[], "").await.1,
        json!(true)
    );
}

#[tokio::test]
async fn facade_validation_redirects_web_requests_and_flashes_named_errors() {
    let (_root, k) = kernel().await;
    let mut request = Request::from(
        http::Request::builder()
            .method(Method::POST)
            .uri("/form")
            .header("host", "example.test")
            .header("referer", "http://example.test/form")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("other=value"))
            .unwrap(),
    );
    let response = k.handle(&mut request).await;
    assert!(response.status().is_redirection(), "{}", response.status());
    assert!(response.headers().contains_key("location"));
    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(!cookie.is_empty());
    let (status, value) = call(&k, Method::GET, "/form", &[("cookie", &cookie)], "").await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert!(value.to_string().contains("profile"), "{value}");
    assert!(value.to_string().contains("name"), "{value}");
}

#[tokio::test]
async fn signature_facade_uses_current_request_and_query_exclusions() {
    use openrust::{
        routing::{Router, SignatureMode},
        support::facades::Url,
    };
    fn register(_: &Application) -> Result<Router, openrust::routing::RoutingError> {
        let mut router = Router::new();
        router.get("/sign",|| async {
            Ok::<_,HttpError>(json!({
                "absolute":Url::signed_route("signed",&[],None,SignatureMode::Absolute).map_err(|source|HttpError::Internal{source:source.into()})?,
                "relative":Url::signed_route("signed",&[],None,SignatureMode::Relative).map_err(|source|HttpError::Internal{source:source.into()})?
            }))
        })?;
        router
            .get("/signed", || async {
                Ok::<_, HttpError>(json!({
                    "absolute":R::has_valid_signature(true)?,
                    "relative":R::has_valid_relative_signature()?,
                    "ignore_absolute":R::has_valid_signature_while_ignoring(&["utm"],true)?,
                    "ignore_relative":R::has_valid_relative_signature_while_ignoring(&["utm"])?
                }))
            })?
            .name("signed");
        Ok(router)
    }
    let root = tempfile::tempdir().unwrap();
    let app=Application::configure(root.path().to_path_buf()).with_config(|_,_|Ok(BTreeMap::from([
        ("app".into(),json!({"env":"testing","key":"a".repeat(32),"locale":"en","supported_locales":[]})),
        ("session".into(),json!({"driver":"array"})),
    ]))).with_routing(Routing::new(register)).create().unwrap();
    let mut k = Kernel::new(app);
    k.bootstrap().await.unwrap();
    let (_, urls) = call(&k, Method::GET, "/sign", &[], "").await;
    for mode in ["absolute", "relative"] {
        let url = urls[mode].as_str().unwrap();
        let (status, value) = call(&k, Method::GET, url, &[], "").await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value[mode], true, "{value}");
        let (_, tampered) = call(&k, Method::GET, &format!("{url}&utm=source"), &[], "").await;
        assert_eq!(tampered[mode], false, "{tampered}");
        assert_eq!(tampered[format!("ignore_{mode}")], true, "{tampered}");
    }
}

fn reinitialize(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::{
            container::{
                AttributeParameter, AttributeValue,
                attributes::{RequestAttribute, RouteParameter},
            },
            contracts::container::ContextualAttribute as _,
        };
        request.attributes().insert(
            "sentinel",
            openrust::container::ParameterValue::new("old".to_owned()),
        );
        request
            .with_container(async {
                assert!(R::route_parameter("id")?.is_some());
                R::initialize(
                    http::Request::builder()
                        .uri("/fresh")
                        .header("host", "fresh.test")
                        .extension(axum::extract::ConnectInfo(
                            "198.51.100.44:1234"
                                .parse::<std::net::SocketAddr>()
                                .unwrap(),
                        ))
                        .body(Body::empty())
                        .unwrap(),
                )?;
                assert!(R::route()?.is_none());
                Ok::<_, HttpError>(())
            })
            .await?;
        assert_eq!(R::ip()?.unwrap().to_string(), "198.51.100.44");
        let app = openrust::support::facades::App::get_facade_root()?;
        let parameter = AttributeParameter {
            name: "id",
            variadic: false,
        };
        for value in [
            RequestAttribute::new("sentinel").resolve(app.container(), parameter)?,
            RouteParameter::new("id").resolve(app.container(), parameter)?,
        ] {
            let AttributeValue::One(value) = value else {
                panic!("参数应是单个值")
            };
            assert!(value.is_null());
        }
        let url = openrust::support::facades::Url::current().unwrap();
        assert_eq!(url, "http://fresh.test/fresh");
        Response::json(StatusCode::OK, json!({"url":url,"path":request.path()}))
    })
}

#[tokio::test]
async fn replacement_refreshes_url_ip_and_container_consumers_after_restoration() {
    let (_root, k) = kernel().await;
    let (status, value) = call(&k, Method::GET, "/api/reinitialize/original", &[], "").await;
    assert_eq!(
        (status, value),
        (
            StatusCode::OK,
            json!({"url":"http://fresh.test/fresh","path":"fresh"})
        )
    );
}
