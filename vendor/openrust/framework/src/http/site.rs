//! 面向浏览器的公开静态站点与可配置根路径。

use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use axum::Router;
use axum::body::Body;
#[cfg(feature = "view")]
use axum::extract::FromRequestParts;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
#[cfg(feature = "view")]
use serde::Serialize;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

use crate::config::ServerConfig;
use crate::core::Error;
use crate::core::Result;
use crate::foundation::ApplicationState;

#[derive(Debug, Clone)]
enum RootTarget {
    File(PathBuf),
    Forward(String),
    Redirect(String),
    #[cfg(feature = "view")]
    View(String),
}

#[derive(Clone)]
pub(crate) struct PendingRootTarget;

#[derive(Clone)]
struct ForwardedUri(Uri);

pub(crate) fn routing_uri(request: &Request) -> &Uri {
    // fallback 需要完整的转发目的路径；OriginalUri 仍保留客户端 URL 给 RequestInfo。
    request
        .extensions()
        .get::<ForwardedUri>()
        .map(|uri| &uri.0)
        .or_else(|| {
            request
                .extensions()
                .get::<axum::extract::OriginalUri>()
                .map(|uri| &uri.0)
        })
        .unwrap_or_else(|| request.uri())
}

#[cfg(feature = "view")]
#[derive(Serialize)]
struct RootContext<'a> {
    app_name: &'a str,
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
        "view" => {
            #[cfg(feature = "view")]
            {
                if target.is_empty()
                    || target.starts_with('/')
                    || Path::new(target)
                        .components()
                        .any(|component| !matches!(component, Component::Normal(_)))
                {
                    Err(invalid_config(
                        "server.root_target",
                        "view target must be a relative template name",
                    ))
                } else {
                    Ok(RootTarget::View(target.to_owned()))
                }
            }
            #[cfg(not(feature = "view"))]
            Err(invalid_config(
                "server.root_target",
                "view target requires the `view` feature and ViewServiceProvider",
            ))
        }
        _ => Err(invalid_config(
            "server.root_target",
            "supported modes are file, forward, redirect and view",
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

#[cfg(feature = "view")]
fn root_views(
    target: &RootTarget,
    state: &ApplicationState,
) -> Result<Option<std::sync::Arc<crate::view::Views>>> {
    if let RootTarget::View(name) = target {
        let views = state.get::<crate::view::Views>().ok_or_else(|| {
            if state.container().contains::<crate::view::Views>() {
                Error::custom(
                    "service_requires_async_resolution",
                    "Views 服务必须在构建 Router 前解析",
                )
            } else {
                invalid_config(
                    "server.root_target",
                    "view target requires the `view` feature and ViewServiceProvider",
                )
            }
        })?;
        views.validate_template(name)?;
        Ok(Some(views))
    } else {
        Ok(None)
    }
}

pub(crate) fn mount(
    router: Router,
    config: &ServerConfig,
    state: ApplicationState,
) -> Result<Router> {
    validate_config(&config.public_dir, &config.root_target)?;
    let target = parse_target(&config.root_target)?;
    #[cfg(feature = "view")]
    let views = root_views(&target, &state)?;
    // 默认首页仍参与静态资源→用户 fallback；缺少 index 不能吞掉根 fallback。
    let root_override =
        !matches!(&target, RootTarget::File(path) if path == Path::new("index.html"));
    let public_dir = config.public_dir.clone();
    let service = tower::service_fn(move |mut request: Request| {
        let router = router.clone();
        let target = target.clone();
        let public_dir = public_dir.clone();
        #[cfg(feature = "view")]
        let views = views.clone();
        let state = state.clone();
        let head = *request.method() == Method::HEAD;
        let response = async move {
            // 业务路由仍先分派；只有站点层挂载的根请求才在 miss 后优先执行根目标。
            if root_override
                && request.uri().path() == "/"
                && matches!(*request.method(), Method::GET | Method::HEAD)
            {
                request.extensions_mut().insert(PendingRootTarget);
            }
            let mut response = router.clone().oneshot(request).await?;
            let Some(request) = crate::routing::router::take_unmatched(&mut response) else {
                return Ok(response);
            };
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
                        let mut forwarded = router.oneshot(request).await?;
                        return Ok(
                            match crate::routing::router::take_unmatched(&mut forwarded) {
                                Some(request) => fallback_response(state, request).await,
                                None => forwarded,
                            },
                        );
                    }
                    RootTarget::Redirect(target) => {
                        return Ok(Redirect::temporary(&target).into_response());
                    }
                    #[cfg(feature = "view")]
                    RootTarget::View(name) => {
                        let Some(views) = views else {
                            return Ok(invalid_config(
                                "server.root_target",
                                "view renderer is unavailable",
                            )
                            .into_response());
                        };
                        let (mut parts, _) = request.into_parts();
                        let info =
                            match crate::http::RequestInfo::from_request_parts(&mut parts, &())
                                .await
                            {
                                Ok(info) => info,
                                Err(error) => return Ok(error.into_response()),
                            };
                        return Ok(
                            match views.render_for_request(
                                &name,
                                &info,
                                &RootContext {
                                    app_name: &state.config().app.name,
                                },
                            ) {
                                Ok(response) => response.into_response(),
                                Err(error) => error.into_response(),
                            },
                        );
                    }
                }
            }
            Ok(fallback(State(state), request).await)
        };
        async move {
            response
                .await
                .map(|response| finalize_site_response(head, response))
        }
    });
    Ok(Router::new().fallback_service(service))
}

fn finalize_site_response(head: bool, mut response: Response) -> Response {
    if head {
        // fallback_service 不经过 Axum 的顶层 HEAD 去体；保留响应各部分，不推断表示长度。
        // 正常 drop 原 body 以运行其清理逻辑，不能为了 HEAD 读取或 drain 流。
        *response.body_mut() = Body::empty();
    }
    response
}

fn rewrite_uri(mut request: Request, target: &str) -> Result<Request> {
    let query = request.uri().query().map(|query| format!("?{query}"));
    let path_and_query = format!("{target}{}", query.unwrap_or_default());
    let mut parts = request.uri().clone().into_parts();
    parts.path_and_query = Some(path_and_query.parse().map_err(|_| {
        Error::custom_with_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_root_target",
            "configured root route target is invalid",
        )
    })?);
    match Uri::from_parts(parts) {
        Ok(uri) => {
            request.extensions_mut().insert(ForwardedUri(uri.clone()));
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

pub(crate) async fn fallback(State(state): State<ApplicationState>, request: Request) -> Response {
    fallback_response(state, request).await
}

pub(crate) async fn fallback_response(state: ApplicationState, request: Request) -> Response {
    let path = request.uri().path();
    if is_reserved_path(path) {
        return Error::NotFound("route not found".into()).into_response();
    }

    try_static_file(state, request)
        .await
        .unwrap_or_else(|| Error::NotFound("route not found".into()).into_response())
}

pub(crate) async fn try_static_file(state: ApplicationState, request: Request) -> Option<Response> {
    let service =
        ServeDir::new(&state.config().server.public_dir).append_index_html_on_directories(true);
    match service.oneshot(request).await {
        Ok(response)
            if matches!(
                response.status(),
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ) =>
        {
            None
        }
        Ok(response) => Some(response.map(Body::new)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::pin::Pin;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Context, Poll};

    #[derive(Clone)]
    struct ResponseProof(u8);

    #[tokio::test]
    async fn head_finalization_preserves_response_parts_without_inferring_length() -> Result<()> {
        for status in [
            StatusCode::OK,
            StatusCode::CONTINUE,
            StatusCode::NO_CONTENT,
            StatusCode::NOT_MODIFIED,
            StatusCode::BAD_REQUEST,
        ] {
            for extra in [
                None,
                Some(("content-length", "777")),
                Some(("transfer-encoding", "chunked")),
            ] {
                let mut response = Response::new(Body::from("HEAD-specific body"));
                *response.status_mut() = status;
                response.headers_mut().insert(
                    "x-head-proof",
                    axum::http::HeaderValue::from_static("preserved"),
                );
                if let Some((header, value)) = extra {
                    response.headers_mut().insert(
                        axum::http::HeaderName::from_static(header),
                        axum::http::HeaderValue::from_static(value),
                    );
                }
                response.extensions_mut().insert(ResponseProof(7));
                let headers = response.headers().clone();
                let response = finalize_site_response(true, response);
                assert_eq!(response.status(), status);
                assert_eq!(response.headers(), &headers);
                assert_eq!(
                    response
                        .extensions()
                        .get::<ResponseProof>()
                        .map(|proof| proof.0),
                    Some(7)
                );
                assert!(
                    axum::body::to_bytes(response.into_body(), 64)
                        .await
                        .map_err(Error::internal)?
                        .is_empty()
                );
            }
        }
        Ok(())
    }

    struct ProbeStream {
        polled: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
        yielded: bool,
    }

    impl futures_util::Stream for ProbeStream {
        type Item = std::result::Result<bytes::Bytes, Infallible>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.polled.fetch_add(1, Ordering::SeqCst);
            if self.yielded {
                Poll::Ready(None)
            } else {
                self.yielded = true;
                Poll::Ready(Some(Ok(bytes::Bytes::from_static(b"stream"))))
            }
        }
    }

    impl Drop for ProbeStream {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn head_finalization_drops_unpolled_stream_and_leaves_get_stream_intact() -> Result<()> {
        for head in [true, false] {
            let polled = Arc::new(AtomicUsize::new(0));
            let dropped = Arc::new(AtomicUsize::new(0));
            let response = Response::new(Body::from_stream(ProbeStream {
                polled: Arc::clone(&polled),
                dropped: Arc::clone(&dropped),
                yielded: false,
            }));
            let response = finalize_site_response(head, response);
            assert_eq!(polled.load(Ordering::SeqCst), 0);
            assert_eq!(dropped.load(Ordering::SeqCst), usize::from(head));
            assert!(
                !response
                    .headers()
                    .contains_key(axum::http::header::CONTENT_LENGTH)
            );
            let body = axum::body::to_bytes(response.into_body(), 64)
                .await
                .map_err(Error::internal)?;
            assert_eq!(
                body.as_ref(),
                if head {
                    b"".as_slice()
                } else {
                    b"stream".as_slice()
                }
            );
            assert_eq!(polled.load(Ordering::SeqCst), if head { 0 } else { 2 });
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
        }
        Ok(())
    }
}
