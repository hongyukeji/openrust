//! 验证文件系统视图渲染与根路径集成。

use axum::body::to_bytes;
use axum::extract::FromRequestParts;
use axum::http::{Request, StatusCode, header};
use figment::{
    Figment,
    providers::{Format, Toml},
};
use openrust::config::FrameworkConfig;

use openrust::ServiceExt;
use openrust::http::{HttpServiceProvider, build_router};
use openrust::support::{ArrayKey, Collection, MessageBag, MessageInput};
use openrust::view::{ViewServiceProvider, Views};
use serde::Serialize;
use tempfile::tempdir;

#[cfg(feature = "session")]
struct ViewRoutes;

#[cfg(feature = "session")]
impl openrust::core::ServiceProvider for ViewRoutes {
    fn register<'a>(
        &'a self,
        _app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes
                    .get("/view/flash/{message}", flash_view_error)
                    .get("/view/corrupt", corrupt_view_error)
                    .get("/view/mutate", mutate_view_error)
                    .get("/view/read", read_view_error)
            })?;
            Ok(())
        })
    }
}

#[cfg(feature = "session")]
async fn flash_view_error(
    openrust::http::Path(message): openrust::http::Path<String>,
    session: openrust::session::Session,
) -> openrust::Result<&'static str> {
    let mut bag = MessageBag::new();
    bag.add("email", message)?;
    session.flash_errors("default", &bag)?;
    Ok("stored")
}

#[cfg(feature = "session")]
async fn corrupt_view_error(session: openrust::session::Session) -> openrust::Result<&'static str> {
    session.put(
        "errors",
        serde_json::json!({"kind":"openrust.view_errors","version":999,"bags":[]}),
    )?;
    Ok("stored")
}

#[cfg(feature = "session")]
async fn read_view_error(
    info: openrust::http::RequestInfo,
    openrust::http::Inject(views): openrust::http::Inject<Views>,
) -> openrust::Result<axum::response::Html<String>> {
    VIEW_HANDLER_ENTERED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    views.render_for_request("errors.html", &info, &serde_json::json!({}))
}

#[cfg(feature = "session")]
async fn mutate_view_error(
    info: openrust::http::RequestInfo,
    session: openrust::session::Session,
    openrust::http::Inject(views): openrust::http::Inject<Views>,
) -> openrust::Result<axum::response::Html<String>> {
    let mut bag = MessageBag::new();
    bag.add("email", "mutated")?;
    info.put_view_error_bag("default", Box::new(bag))?;
    if session
        .read_errors()?
        .default_first(Some("email"), None)?
        .as_deref()
        != Some("mutated")
    {
        return Err(openrust::Error::custom(
            "test_session_identity",
            "请求共享修改未同步到Session",
        ));
    }
    // 只有显式keep才延长本次已为old的flash，普通对象put不延长。
    session.keep(&["errors"])?;
    views.render_for_request("errors.html", &info, &serde_json::json!({}))
}

#[cfg(feature = "session")]
static VIEW_HANDLER_ENTERED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

async fn configured_app(
    directory: &std::path::Path,
    with_provider: bool,
    auto_reload: bool,
) -> openrust::Result<openrust::App> {
    let mut config = FrameworkConfig::default();
    "Example & Co".clone_into(&mut config.app.name);
    "view:index.html".clone_into(&mut config.server.root_target);
    let directory = directory
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let figment = Figment::new().merge(Toml::string(&format!(
        "[view]\ndirectory = \"{directory}\"\nauto_reload = {auto_reload}"
    )));
    let sources =
        figment::Figment::from(figment::providers::Serialized::defaults(config)).merge(figment);
    let mut app = openrust::core::ProviderBuilder::new();
    app.provider(HttpServiceProvider);
    if with_provider {
        app.provider(ViewServiceProvider);
    }
    app.start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
}

#[tokio::test]
async fn root_view_is_html_and_escapes_context() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    std::fs::write(
        directory.path().join("index.html"),
        "<!doctype html><title>{{ app_name }}</title>{{ errors.count() }}",
    )?;
    let app = configured_app(directory.path(), true, false).await?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/")
                .body(axum::body::Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    let body = to_bytes(response.into_body(), 4096).await?;
    assert_eq!(body, "<!doctype html><title>Example &amp; Co</title>0");
    Ok(())
}

#[tokio::test]
async fn view_root_requires_plugin_and_existing_template() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempdir()?;
    let app = configured_app(directory.path(), false, false).await?;
    assert!(build_router(&app).is_err());

    let directory = tempdir()?;
    std::fs::write(directory.path().join("index.html"), "{% if %}")?;
    let app = configured_app(directory.path(), true, false).await?;
    assert!(
        build_router(&app).is_err(),
        "invalid template syntax should fail during root validation"
    );
    Ok(())
}

#[derive(Serialize)]
struct PageContext<'a> {
    title: &'a str,
}

#[tokio::test]
async fn renderer_loads_named_views() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    std::fs::create_dir(directory.path().join("pages"))?;
    std::fs::write(
        directory.path().join("pages/home.html"),
        "<h1>{{ title }}</h1>",
    )?;
    let app = configured_app(directory.path(), true, false).await?;
    let views = app.state().resolve::<Views>().await?;
    let response = views.render("pages/home.html", &PageContext { title: "Hello" })?;
    assert_eq!(response.0, "<h1>Hello</h1>");
    assert!(
        views
            .render("../secret.html", &PageContext { title: "x" })
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn request_errors_are_isolated_and_explicit_data_wins()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    std::fs::write(
        directory.path().join("errors.html"),
        "{{ errors.count() }}|{{ errors.first('email') }}|{{ errors.getBag('other').first('email') }}|{% if errors.any() %}yes{% endif %}{% for value in errors.all() %}[{{ value }}]{% endfor %}|{% if errors.first('wild.*') is none %}null{% endif %}",
    )?;
    std::fs::write(
        directory.path().join("override.html"),
        "{{ errors is none }}|{{ marker }}",
    )?;
    std::fs::write(
        directory.path().join("arguments.html"),
        "{% if errors.has(['email','missing']) %}all{% endif %}|{% if errors.hasAny({'alias':'email'}, 'ignored') %}any{% endif %}|{{ errors.first(none,none) }}|{{ errors.get('email',none)[0] }}",
    )?;
    let app = configured_app(directory.path(), true, false).await?;
    let views = app.state().resolve::<Views>().await?;
    let empty = request_info().await?;
    let mut default = MessageBag::try_from_messages([(
        ArrayKey::from("wild.one"),
        MessageInput::Many(Collection::default()),
    )])?;
    default.add("email", "<private>")?;
    let current = request_info().await?;
    let same_request = current.clone();
    same_request.put_view_error_bag("default", Box::new(default))?;
    let mut named = MessageBag::new();
    named.add("email", "other")?;
    current.put_view_error_bag("other", Box::new(named))?;
    assert_eq!(
        views
            .render_for_request("errors.html", &empty, &serde_json::json!({}))?
            .0,
        "0||||"
    );
    assert_eq!(
        views
            .render_for_request("errors.html", &current, &serde_json::json!({}))?
            .0,
        "1|&lt;private&gt;|other|yes[&lt;private&gt;]|null"
    );
    assert_eq!(
        views
            .render_for_request("errors.html", &empty, &serde_json::json!({}))?
            .0,
        "0||||"
    );
    assert!(
        views
            .render_for_request(
                "errors.html",
                &current,
                &serde_json::json!({"errors": null})
            )
            .is_err(),
        "显式 null 必须覆盖共享对象，模板方法调用随之失败"
    );
    assert_eq!(
        views
            .render_for_request(
                "override.html",
                &current,
                &serde_json::json!({"errors": null, "marker": "<kept>"})
            )?
            .0,
        "True|&lt;kept&gt;"
    );
    assert_eq!(
        views
            .render_for_request("arguments.html", &current, &serde_json::json!({}))?
            .0,
        "|any|&lt;private&gt;|&lt;private&gt;"
    );
    let invalid = views.render_for_request("errors.html", &current, &42);
    assert_eq!(
        invalid
            .err()
            .map(|error| error.code().to_string())
            .as_deref(),
        Some("view_context_invalid")
    );
    assert!(
        views.render("errors.html", &serde_json::json!({})).is_err(),
        "显式 render 不隐式注入请求数据"
    );
    let wire = Request::builder()
        .uri("/")
        .body(axum::body::Body::empty())?;
    let (mut parts, _) = wire.into_parts();
    let first_info = openrust::http::RequestInfo::from_request_parts(&mut parts, &()).await?;
    let second_info = openrust::http::RequestInfo::from_request_parts(&mut parts, &()).await?;
    let mut later = MessageBag::new();
    later.add("email", "later")?;
    first_info.put_view_error_bag("default", Box::new(later))?;
    assert_eq!(
        views
            .render_for_request("errors.html", &second_info, &serde_json::json!({}))?
            .0,
        "1|later||yes[later]|"
    );
    Ok(())
}

async fn request_info() -> Result<openrust::http::RequestInfo, Box<dyn std::error::Error>> {
    let request = Request::builder()
        .uri("/")
        .body(axum::body::Body::empty())?;
    let (mut parts, _) = request.into_parts();
    Ok(openrust::http::RequestInfo::from_request_parts(&mut parts, &()).await?)
}

#[cfg(feature = "session")]
#[tokio::test]
async fn session_errors_are_shared_before_handler_and_age_per_cookie()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::config::SessionConfig;
    use openrust::http::CookieServiceProvider;
    use openrust::session::{MemorySessionStore, SessionServiceProvider};

    let directory = tempdir()?;
    std::fs::write(
        directory.path().join("errors.html"),
        "{{ errors.first('email') }}",
    )?;
    let mut config = FrameworkConfig::default();
    config.server.root_target = "view:errors.html".into();
    let path = directory.path().to_string_lossy().replace('\\', "\\\\");
    let sources = Figment::from(figment::providers::Serialized::defaults(config))
        .merge(Toml::string(&format!("[view]\ndirectory = \"{path}\"")));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(CookieServiceProvider::plain())
        .provider(ViewServiceProvider)
        .provider(
            SessionServiceProvider::with_store(MemorySessionStore::default()).with_cookie_config(
                SessionConfig {
                    driver: Some("array".into()),
                    cookie: Some("view_session".into()),
                    secure: Some(false),
                    lottery: [0, 1],
                    ..SessionConfig::default()
                },
            ),
        )
        .provider(ViewRoutes);
    let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
    let router = build_router(&app)?;
    let first = send_view(&router, "/view/flash/%3Cfirst%3E", None).await?;
    assert_eq!(first.status(), StatusCode::OK);
    let first_cookie = view_cookie(&first)?;
    let second = send_view(&router, "/view/flash/second", None).await?;
    assert_eq!(second.status(), StatusCode::OK);
    let second_cookie = view_cookie(&second)?;
    for (cookie, expected) in [(&first_cookie, "&lt;first&gt;"), (&second_cookie, "second")] {
        let response = send_view(&router, "/view/read", Some(cookie)).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await?;
        assert_eq!(body, expected);
    }
    let aged = send_view(&router, "/view/read", Some(&first_cookie)).await?;
    assert_eq!(to_bytes(aged.into_body(), 4096).await?, "");
    let reseed = send_view(&router, "/view/flash/seed", Some(&first_cookie)).await?;
    assert_eq!(reseed.status(), StatusCode::OK);
    let mutated = send_view(&router, "/view/mutate", Some(&first_cookie)).await?;
    assert_eq!(mutated.status(), StatusCode::OK);
    assert_eq!(to_bytes(mutated.into_body(), 4096).await?, "mutated");
    let persisted = send_view(&router, "/view/read", Some(&first_cookie)).await?;
    assert_eq!(persisted.status(), StatusCode::OK);
    assert_eq!(to_bytes(persisted.into_body(), 4096).await?, "mutated");
    let corrupt = send_view(&router, "/view/corrupt", Some(&second_cookie)).await?;
    assert_eq!(corrupt.status(), StatusCode::OK);
    let entered = VIEW_HANDLER_ENTERED.load(std::sync::atomic::Ordering::SeqCst);
    let rejected = send_view(&router, "/view/read", Some(&second_cookie)).await?;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert_eq!(rejected.headers()[header::CACHE_CONTROL], "no-store");
    let rejected_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(rejected.into_body(), 4096).await?)?;
    assert_eq!(rejected_body["code"], "error_bag_version_unknown");
    assert_eq!(
        VIEW_HANDLER_ENTERED.load(std::sync::atomic::Ordering::SeqCst),
        entered,
        "损坏的已识别错误包必须在handler前拒绝"
    );
    Ok(())
}

#[cfg(feature = "session")]
async fn send_view(
    router: &axum::Router,
    path: &str,
    cookie: Option<&str>,
) -> Result<axum::response::Response, Box<dyn std::error::Error>> {
    let mut request = Request::builder()
        .uri(path)
        .header(header::HOST, "localhost");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    Ok(router
        .clone()
        .oneshot(request.body(axum::body::Body::empty())?)
        .await?)
}

#[cfg(feature = "session")]
fn view_cookie(response: &axum::response::Response) -> Result<String, Box<dyn std::error::Error>> {
    Ok(response.headers()[header::SET_COOKIE]
        .to_str()?
        .split(';')
        .next()
        .ok_or_else(|| std::io::Error::other("missing cookie"))?
        .to_owned())
}

#[tokio::test]
async fn development_reload_reads_updated_template() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let path = directory.path().join("page.html");
    std::fs::write(&path, "first")?;
    let app = configured_app(directory.path(), true, true).await?;
    let views = app.state().resolve::<Views>().await?;
    assert_eq!(views.render("page.html", &())?.0, "first");
    std::fs::write(&path, "second")?;
    assert_eq!(views.render("page.html", &())?.0, "second");
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn template_symlink_cannot_escape_template_directory()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let directory = tempdir()?;
    let outside = tempdir()?;
    let secret = outside.path().join("secret.html");
    std::fs::write(&secret, "secret")?;
    symlink(&secret, directory.path().join("linked.html"))?;
    let app = configured_app(directory.path(), true, false).await?;
    let views = app.state().resolve::<Views>().await?;
    assert!(views.render("linked.html", &()).is_err());
    Ok(())
}
