//! 面向浏览器的公开静态站点与可配置根路径。

use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

use crate::config::ServerConfig;
use crate::core::{AppState, Error, Result};

#[derive(Debug, Clone)]
enum RootTarget {
    File(PathBuf),
    Forward(String),
    Redirect(String),
}

pub(crate) fn validate_config(public_dir: &Path, target: &str) -> Result<()> {
    if public_dir.as_os_str().is_empty() {
        return Err(invalid_config("server.public_dir", "must not be empty"));
    }
    parse_target(target).map(|_| ())
}

fn parse_target(value: &str) -> Result<RootTarget> {
    let (kind, target) = value.split_once(':').ok_or_else(|| {
        invalid_config(
            "server.root_target",
            "expected file:/path, forward:/path or redirect:/path",
        )
    })?;
    let target = target.trim();
    match kind {
        "file" => {
            let path = safe_site_path(target)?;
            Ok(RootTarget::File(path))
        }
        "forward" => {
            validate_internal_target(target)?;
            Ok(RootTarget::Forward(target.to_owned()))
        }
        "redirect" => {
            validate_redirect_target(target)?;
            Ok(RootTarget::Redirect(target.to_owned()))
        }
        _ => Err(invalid_config(
            "server.root_target",
            "supported modes are file, forward and redirect",
        )),
    }
}

fn safe_site_path(value: &str) -> Result<PathBuf> {
    let path = value
        .strip_prefix('/')
        .ok_or_else(|| invalid_config("server.root_target", "file target must start with `/`"))?;
    if path.is_empty()
        || Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid_config(
            "server.root_target",
            "file target must be a non-empty site-relative path without traversal",
        ));
    }
    Ok(PathBuf::from(path))
}

fn validate_internal_target(target: &str) -> Result<()> {
    if !target.starts_with('/')
        || target.starts_with("//")
        || target.contains(['\r', '\n', '\0', '#'])
        || Uri::from_str(target).is_err()
    {
        return Err(invalid_config(
            "server.root_target",
            "forward target must be a valid absolute application path",
        ));
    }
    Ok(())
}

fn validate_redirect_target(target: &str) -> Result<()> {
    if target.starts_with('/') {
        return validate_internal_target(target);
    }
    let uri = Uri::from_str(target).map_err(|_| {
        invalid_config(
            "server.root_target",
            "redirect target must be a rooted path or an absolute HTTP(S) URL",
        )
    })?;
    let allowed_scheme = matches!(uri.scheme_str(), Some("http" | "https"));
    let authority = uri.authority().map(http::uri::Authority::as_str);
    if !allowed_scheme || authority.is_none_or(|value| value.contains('@')) {
        return Err(invalid_config(
            "server.root_target",
            "external redirect target must use HTTP(S) and must not contain user information",
        ));
    }
    Ok(())
}

fn invalid_config(key: &str, reason: &str) -> Error {
    crate::config::ConfigError::Invalid(format!("{key} {reason}")).into()
}

pub(crate) fn mount(router: Router, config: &ServerConfig) -> Result<Router> {
    validate_config(&config.public_dir, &config.root_target)?;
    let target = parse_target(&config.root_target)?;
    let public_dir = config.public_dir.clone();
    let service = tower::service_fn(move |request: Request| {
        let router = router.clone();
        let target = target.clone();
        let public_dir = public_dir.clone();
        async move {
            if request.uri().path() == "/"
                && matches!(*request.method(), Method::GET | Method::HEAD)
            {
                match target {
                    RootTarget::File(path) => {
                        let service = ServeFile::new(resolve_site_path(&public_dir, &path));
                        let response = service.oneshot(request).await.map(|response| {
                            if response.status() == StatusCode::NOT_FOUND {
                                Error::NotFound("site index file not found".into()).into_response()
                            } else {
                                response.map(Body::new)
                            }
                        });
                        return response;
                    }
                    RootTarget::Forward(target) => {
                        let request = match rewrite_uri(request, &target) {
                            Ok(request) => request,
                            Err(error) => return Ok(error.into_response()),
                        };
                        return router.oneshot(request).await;
                    }
                    RootTarget::Redirect(target) => {
                        return Ok(Redirect::temporary(&target).into_response());
                    }
                }
            }
            router.oneshot(request).await
        }
    });
    Ok(Router::new().fallback_service(service))
}

fn rewrite_uri(mut request: Request, target: &str) -> Result<Request> {
    let query = request.uri().query().map(|query| format!("?{query}"));
    let path_and_query = format!("{target}{}", query.unwrap_or_default());
    match Uri::builder().path_and_query(path_and_query).build() {
        Ok(uri) => {
            *request.uri_mut() = uri;
            Ok(request)
        }
        Err(_) => Err(Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_root_target",
            "configured root route target is invalid",
        )),
    }
}

pub(crate) async fn fallback(State(state): State<AppState>, request: Request) -> Response {
    let path = request.uri().path();
    if is_reserved_path(path) {
        return Error::NotFound("route not found".into()).into_response();
    }

    let service =
        ServeDir::new(&state.config().server.public_dir).append_index_html_on_directories(true);
    match service.oneshot(request).await {
        Ok(response)
            if matches!(
                response.status(),
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ) =>
        {
            Error::NotFound("route not found".into()).into_response()
        }
        Ok(response) => response.map(Body::new),
        Err(never) => match never {},
    }
}

fn is_reserved_path(path: &str) -> bool {
    ["/api", "/ws"]
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
}

fn resolve_site_path(public_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        public_dir.join(path)
    }
}
