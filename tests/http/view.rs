//! 验证文件系统视图渲染与根路径集成。

use axum::body::to_bytes;
use axum::http::{Request, StatusCode, header};
use figment::{
    Figment,
    providers::{Format, Toml},
};
use openrust::config::FrameworkConfig;
use openrust::core::lifecycle::finalize;
use openrust::http::{HttpPlugin, build_router};
use openrust::view::{ViewPlugin, Views};
use openrust::{AppBuilder, ServiceExt};
use serde::Serialize;
use tempfile::tempdir;

fn configured_app(directory: &std::path::Path, with_plugin: bool, auto_reload: bool) -> AppBuilder {
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
    let mut app = AppBuilder::new(config, figment);
    app.plugin(HttpPlugin);
    if with_plugin {
        app.plugin(ViewPlugin);
    }
    app
}

#[tokio::test]
async fn root_view_is_html_and_escapes_context() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    std::fs::write(
        directory.path().join("index.html"),
        "<!doctype html><title>{{ app_name }}</title>",
    )?;
    let app = finalize(configured_app(directory.path(), true, false)).await?;
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
    assert_eq!(body, "<!doctype html><title>Example &amp; Co</title>");
    Ok(())
}

#[tokio::test]
async fn view_root_requires_plugin_and_existing_template() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempdir()?;
    let app = finalize(configured_app(directory.path(), false, false)).await?;
    assert!(build_router(&app).is_err());

    let directory = tempdir()?;
    std::fs::write(directory.path().join("index.html"), "{% if %}")?;
    let app = finalize(configured_app(directory.path(), true, false)).await?;
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
    let app = configured_app(directory.path(), true, false);
    let app = finalize(app).await?;
    let views = app.state().try_get::<Views>()?;
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
async fn development_reload_reads_updated_template() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let path = directory.path().join("page.html");
    std::fs::write(&path, "first")?;
    let app = finalize(configured_app(directory.path(), true, true)).await?;
    let views = app.state().try_get::<Views>()?;
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
    let app = finalize(configured_app(directory.path(), true, false)).await?;
    let views = app.state().try_get::<Views>()?;
    assert!(views.render("linked.html", &()).is_err());
    Ok(())
}
