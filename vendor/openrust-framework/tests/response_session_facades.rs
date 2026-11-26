use std::{collections::BTreeMap, convert::Infallible};

use openrust::{
    contracts::{
        cache::Repository as _, container::Container as _, foundation::Application as _,
        http::Kernel as _, session::Session as _,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        Body, HeaderMap, HttpError, HttpFuture, Request, Response, ResponseFactory, StatusCode,
        StreamedEvent, StreamedJson,
    },
    routing::{RedirectOptions, RouteParameter, Router},
    support::facades::{Facade, Request as RequestFacade, Response as R, Route, Session},
    view::ViewData,
};
use serde_json::{Value, json};

async fn render() -> Result<Response, HttpError> {
    R::view(
        "card",
        ViewData::new(),
        StatusCode::ACCEPTED,
        HeaderMap::new(),
    )
    .await
}

fn explicit_view(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        R::view_for_request(
            "card",
            ViewData::new(),
            StatusCode::OK,
            HeaderMap::new(),
            request,
        )
        .await
    })
}

fn session_explicit(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        request.with_container(async {
            Session::flash("aged", json!(true))?;
            Session::age_flash_data()?;
            assert_eq!(Session::get("aged")?, Some(json!(true)));
            Session::age_flash_data()?;
            assert!(Session::missing("aged")?);
            Session::put("profile.name", json!("Ada"))?;
            Session::flash("notice", json!("saved"))?;
            Session::now("transient", json!("current"))?;
            assert_eq!(Session::get("transient")?, Some(json!("current")));
            Session::flash_input(json!({"email":"a@example.test"}))?;
            Session::set_previous_route(Some("dashboard"))?;
            Session::set_previous_url("http://example.test/before")?;
            Session::password_confirmed()?;
            assert_eq!(Session::increment("attempts", 2)?, json!(2));
            assert_eq!(Session::decrement("attempts", 1)?, json!(1));
            assert_eq!(Session::remember("memo", || Ok(json!(9)))?, json!(9));
            assert_eq!(Session::remember("memo", || panic!("must not run"))?, json!(9));
            Session::push("items", json!(1))?;
            assert_eq!(Session::previous_uri()?.unwrap().to_string(), "http://example.test/before");
            assert_eq!(Session::previous_route()?, Some("dashboard".into()));
            assert!(Session::has_old_input(Some("email"))?);
            assert!(Session::exists("profile.name")?);
            assert!(Session::has_any(&["missing", "profile.name"])?);
            assert_eq!(Session::get_or("missing", json!("fallback"))?, json!("fallback"));
            let id = Session::id()?;
            assert!(Session::is_valid_id(Some(&id)));
            Session::regenerate(false).await?;
            assert_ne!(Session::get_id()?, id);
            let value = RequestFacade::session(|store| store.get("profile.name").cloned())?;
            R::json(json!({"value":value,"notice":Session::get("notice")?,"memo":Session::pull("memo")?,"gone":Session::missing("memo")?}), StatusCode::OK, HeaderMap::new())
        }).await
    })
}

async fn kernel() -> (tempfile::TempDir, Kernel) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("resources/views")).unwrap();
    std::fs::write(
        root.path().join("resources/views/card.blade.html"),
        "<h1>{{ config('app.name') }}</h1>",
    )
    .unwrap();
    std::fs::write(root.path().join("document.txt"), "download-content").unwrap();
    let app = Application::configure(root.path().to_path_buf())
        .with_config(|root, _| {
            let database = std::env::var("OPENRUST_TEST_FACADE_MYSQL_URL").ok();
            assert!(database.is_some() || std::env::var_os("OPENRUST_REQUIRE_FACADE_MYSQL").is_none(), "MySQL 门面验收缺少独立数据库连接");
            let table = format!("facade_sessions_{}", root.file_name().unwrap().to_string_lossy().chars().filter(char::is_ascii_alphanumeric).collect::<String>());
            let mut config = BTreeMap::from([
                ("app".into(), json!({"name":"Facade App", "env":"testing","url":"http://example.test","key":"a".repeat(32),"locale":"en","supported_locales":[]})),
                ("session".into(), json!({"driver":if database.is_some(){"database"}else{"array"},"cookie":"facade_session","table":table,"lottery":[0,100]})),
                ("probe".into(), json!({"root":root})),
                ("cache".into(), json!({"stores":{"session":{"driver":"session","key":"facade"}}})),
            ]);
            if let Some(url) = database {
                config.insert("database".into(), json!({"default":"facade","connections":{"facade":{"url":url,"use_upsert_alias":true}}}));
            }
            Ok(config)
        })
        .with_middleware(|middleware| { middleware.prevent_request_forgery(["*"], false, false); })
        .with_routing(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/target/{id}", || async { Ok::<_, HttpError>("target") })?.name("target").set_action("TargetController@show")?;
            Ok(router)
        }).web(|| {
            Route::get("/json", || async { R::json(json!({"html":"<a>","name":"中文"}), StatusCode::CREATED, headers()) })?;
            Route::get("/make", || async { R::make(json!({"ok":true}), StatusCode::ACCEPTED, headers()) })?;
            Route::get("/empty", || async { R::no_content(StatusCode::NO_CONTENT, headers()) })?;
            Route::get("/jsonp", || async { R::jsonp("client.done", json!([1]), StatusCode::OK, headers()) })?;
            Route::get("/bad-jsonp", || async { R::jsonp("x();alert", json!([1]), StatusCode::OK, headers()) })?;
            Route::get("/markdown", || async { R::markdown("# Title", StatusCode::OK, headers()) })?;
            Route::get("/view", render)?;
            Route::get("/explicit-view", explicit_view)?;
            Route::get("/view-first", || async { R::view_first(&["absent", "card"], ViewData::new(), StatusCode::OK, headers()).await })?;
            Route::get("/stream", || async { R::stream(futures_util::stream::iter([Ok::<_, Infallible>("one"), Ok("two")]), StatusCode::OK, headers()) })?;
            Route::get("/stream-json", || async { R::stream_json(StreamedJson::items(futures_util::stream::iter([Ok::<_, Infallible>(json!({"id":1}))])), StatusCode::OK, headers()) })?;
            Route::get("/events", || async { R::event_stream(futures_util::stream::iter([Ok::<_, Infallible>(StreamedEvent::message("hello"))]), headers()) })?;
            Route::get("/events-no-end", || async { R::event_stream_with_end(futures_util::stream::empty::<Result<StreamedEvent, Infallible>>(), headers(), None) })?;
            Route::get("/stream-download", || async { R::stream_download(futures_util::stream::iter([Ok::<_, Infallible>("download")]), Some("report.txt"), headers(), "attachment") })?;
            Route::get("/file", || async { let root = openrust::support::facades::Config::string("probe.root")?; R::file(std::path::Path::new(&root).join("document.txt"), headers()).await })?;
            Route::get("/download", || async { let root = openrust::support::facades::Config::string("probe.root")?; R::download(std::path::Path::new(&root).join("document.txt"), Some("report.txt"), headers(), "attachment").await })?;
            Route::get("/redirect", || async { Ok::<_, HttpError>(R::redirect_to("/next", RedirectOptions::default(), Some(true))?) })?;
            Route::get("/redirect-route", || async { Ok::<_, HttpError>(R::redirect_to_route("target", &[RouteParameter::named("id", 7)], RedirectOptions::default())?) })?;
            Route::get("/redirect-action", || async { Ok::<_, HttpError>(R::redirect_to_action("TargetController@show", &[RouteParameter::named("id", 8)], RedirectOptions::default())?) })?;
            Route::get("/guest", || async { R::redirect_guest("/login", RedirectOptions::default(), None) })?;
            Route::get("/intended", || async { R::redirect_to_intended("/fallback", RedirectOptions::default(), None) })?;
            Route::get("/session-cache-write", || async {
                let repository = Session::cache()?;
                let selected = openrust::support::facades::Cache::store(Some("session")).map_err(cache_error)?;
                assert!(std::sync::Arc::ptr_eq(&repository, &selected));
                repository.put("banner", &"ready".into(), None).await.map_err(cache_error)?;
                Ok::<_, HttpError>("stored")
            })?;
            Route::get("/session-cache-read", || async {
                let value = Session::cache()?.get("banner").await.map_err(cache_error)?;
                R::json(value.as_ref().and_then(openrust::cache::Value::as_str), StatusCode::OK, HeaderMap::new())
            })?;
            Route::get("/session", session_explicit)?;
            Route::get("/session-read", || async { R::json(json!({"name":Session::get("profile.name")?,"notice":Session::get("notice")?,"transient":Session::get("transient")?}), StatusCode::OK, HeaderMap::new()) })?;
            Route::get("/direct-factory", || async {
                let facade = R::get_facade_root()?;
                // 类型和命名绑定必须是同一工厂，不能绕过宏或应用依赖。
                let owner = openrust::support::facades::App::get_facade_root()?;
                let typed = owner.container().make::<ResponseFactory>()?;
                let named = owner.container().get::<ResponseFactory>("response")?;
                assert!(std::sync::Arc::ptr_eq(&facade, &typed));
                assert!(std::sync::Arc::ptr_eq(&facade, &named));
                facade.view("card", ViewData::new(), StatusCode::OK, headers()).await
            })?;
            Ok(())
        }))
        .create().unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    if std::env::var_os("OPENRUST_TEST_FACADE_MYSQL_URL").is_some() {
        let owner = kernel.get_application();
        let table = owner
            .config()
            .unwrap()
            .get("session.table")
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        let connection = owner
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        connection.statement(&format!("CREATE TABLE `{table}` (id varchar(255) PRIMARY KEY, user_id bigint NULL, ip_address varchar(45) NULL, user_agent text NULL, payload longtext NOT NULL, last_activity integer NOT NULL)"), &[]).await.unwrap();
    }
    (root, kernel)
}

fn headers() -> HeaderMap {
    HeaderMap::from_iter([(
        http::HeaderName::from_static("x-facade"),
        http::HeaderValue::from_static("yes"),
    )])
}

async fn call(
    kernel: &Kernel,
    path: &str,
    cookie: Option<&str>,
) -> (http::response::Parts, String) {
    let mut request = http::Request::builder()
        .uri(path)
        .header("host", "example.test");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    let mut request = Request::from(request.body(Body::empty()).unwrap());
    let response = kernel.handle(&mut request).await;
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, 65536).await.unwrap();
    (parts, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn response_facade_builds_real_content_and_validates_jsonp() {
    let (_root, kernel) = kernel().await;
    for (path, status, content_type, body) in [
        (
            "/json",
            201,
            "application/json",
            r#"{"html":"<a>","name":"\u4e2d\u6587"}"#,
        ),
        ("/make", 202, "application/json", r#"{"ok":true}"#),
        (
            "/jsonp",
            200,
            "text/javascript; charset=utf-8",
            "/**/client.done([1]);",
        ),
        ("/markdown", 200, "text/markdown; charset=utf-8", "# Title"),
    ] {
        let (parts, actual) = call(&kernel, path, None).await;
        assert_eq!(parts.status.as_u16(), status, "{path}: {actual}");
        assert_eq!(parts.headers["content-type"], content_type);
        assert_eq!(parts.headers["x-facade"], "yes");
        assert_eq!(actual, body);
    }
    let (parts, body) = call(&kernel, "/empty", None).await;
    assert_eq!(parts.status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    assert_eq!(
        call(&kernel, "/bad-jsonp", None).await.0.status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn response_facade_renders_current_application_views_and_direct_factory() {
    let (_root, kernel) = kernel().await;
    for path in ["/view", "/explicit-view", "/view-first", "/direct-factory"] {
        let (parts, body) = call(&kernel, path, None).await;
        assert_eq!(
            parts.status,
            if path == "/view" {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            },
            "{path}: {body}"
        );
        assert_eq!(body, "<h1>Facade App</h1>");
    }
}

#[tokio::test]
async fn response_facade_streams_json_events_and_downloads() {
    let (_root, kernel) = kernel().await;
    assert_eq!(call(&kernel, "/stream", None).await.1, "onetwo");
    assert_eq!(call(&kernel, "/stream-json", None).await.1, r#"[{"id":1}]"#);
    let (parts, body) = call(&kernel, "/events", None).await;
    assert!(
        parts.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    assert!(body.contains("hello") && body.contains("</stream>"));
    assert!(call(&kernel, "/events-no-end", None).await.1.is_empty());
    for (path, expected) in [
        ("/stream-download", "download"),
        ("/download", "download-content"),
        ("/file", "download-content"),
    ] {
        let (parts, body) = call(&kernel, path, None).await;
        assert_eq!(parts.status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body, expected);
        if path != "/file" {
            assert!(
                parts.headers["content-disposition"]
                    .to_str()
                    .unwrap()
                    .contains("report.txt")
            );
        }
    }
}

#[tokio::test]
async fn response_facade_redirects_and_intended_url_share_the_live_session() {
    let (_root, kernel) = kernel().await;
    for (path, location) in [
        ("/redirect", "https://example.test/next"),
        ("/redirect-route", "http://example.test/target/7"),
        ("/redirect-action", "http://example.test/target/8"),
    ] {
        let (parts, body) = call(&kernel, path, None).await;
        assert_eq!(parts.status, StatusCode::FOUND, "{path}: {body}");
        assert_eq!(parts.headers["location"], location);
    }
    let (parts, _) = call(&kernel, "/guest", None).await;
    assert_eq!(parts.headers["location"], "http://example.test/login");
    let cookie = parts
        .headers
        .get_all("set-cookie")
        .iter()
        .find_map(|value| {
            value
                .to_str()
                .ok()
                .filter(|value| value.starts_with("facade_session="))
                .map(|value| value.split(';').next().unwrap().to_owned())
        })
        .unwrap();
    let (parts, _) = call(&kernel, "/intended", Some(&cookie)).await;
    assert_eq!(parts.headers["location"], "http://example.test/guest");
    assert_eq!(
        call(&kernel, "/intended", Some(&cookie)).await.0.headers["location"],
        "http://example.test/fallback"
    );
}

#[tokio::test]
async fn session_facade_mutates_the_request_store_and_persists_across_requests() {
    let (_root, kernel) = kernel().await;
    let (parts, body) = call(&kernel, "/session", None).await;
    assert_eq!(parts.status, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"value":"Ada","notice":"saved","memo":9,"gone":true})
    );
    let cookie = parts
        .headers
        .get_all("set-cookie")
        .iter()
        .find_map(|value| {
            value
                .to_str()
                .ok()
                .filter(|value| value.starts_with("facade_session="))
                .map(|value| value.split(';').next().unwrap().to_owned())
        })
        .unwrap();
    let (_, body) = call(&kernel, "/session-read", Some(&cookie)).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"name":"Ada","notice":"saved","transient":null})
    );
    let (_, body) = call(&kernel, "/session-read", Some(&cookie)).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"name":"Ada","notice":null,"transient":null})
    );
    assert!(Session::get("profile.name").is_err());
    assert!(R::json(json!({}), StatusCode::OK, HeaderMap::new()).is_err());
}

#[tokio::test]
async fn store_now_and_explicit_flash_aging_share_the_save_algorithm() {
    use openrust::session::{ArraySessionHandler, Store};
    let mut store = Store::new(
        "flash",
        Box::new(ArraySessionHandler::new(std::time::Duration::from_secs(
            120,
        ))),
    )
    .unwrap();
    store.start().await.unwrap();
    store.flash("next", json!("next request")).unwrap();
    store.now("current", json!("current request")).unwrap();
    store.age_flash_data();
    assert!(store.get("current").is_none());
    assert_eq!(store.get("next"), Some(&json!("next request")));
    store.age_flash_data();
    assert!(store.get("next").is_none());
}

#[tokio::test]
async fn factory_streams_are_lazy_and_keep_source_failures() {
    use futures_util::StreamExt as _;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let stream = futures_util::stream::iter([
        Ok::<_, std::io::Error>("part"),
        Err(std::io::Error::other("source failed")),
    ])
    .inspect(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
    });
    let response = ResponseFactory::default().stream(stream, StatusCode::OK, HeaderMap::new());
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert!(
        axum::body::to_bytes(response.into_parts().1, 1024)
            .await
            .is_err()
    );
    assert_eq!(polls.load(Ordering::SeqCst), 2);
}

fn cache_error(source: openrust::cache::CacheError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}

#[tokio::test]
async fn session_facade_cache_uses_the_same_request_repository_and_payload() {
    let (_root, kernel) = kernel().await;
    let (parts, body) = call(&kernel, "/session-cache-write", None).await;
    assert_eq!(parts.status, StatusCode::OK, "{body}");
    let cookie = parts
        .headers
        .get_all("set-cookie")
        .iter()
        .find_map(|value| {
            value
                .to_str()
                .ok()
                .filter(|value| value.starts_with("facade_session="))
                .map(|value| value.split(';').next().unwrap().to_owned())
        })
        .unwrap();
    assert_eq!(
        call(&kernel, "/session-cache-read", Some(&cookie)).await.1,
        "\"ready\""
    );
    assert_eq!(call(&kernel, "/session-cache-read", None).await.1, "null");
}
