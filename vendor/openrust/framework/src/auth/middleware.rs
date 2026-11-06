//! 配置Guard的认证、访客与近期密码确认middleware。

use std::{collections::BTreeMap, fmt, sync::Arc};

use axum::{
    extract::{Extension, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::AuthManager;
use crate::{
    Error, Result,
    core::AppState,
    http::{Redirector, RequestInfo},
};

type RedirectCallback = Arc<dyn Fn(&RequestInfo) -> Result<Option<String>> + Send + Sync>;

/// 每个应用容器独立的认证重定向扩展，不使用跨应用静态回调。
#[derive(Clone, Default)]
pub struct AuthenticationRedirects {
    guests: Option<RedirectCallback>,
    users: Option<RedirectCallback>,
}
impl fmt::Debug for AuthenticationRedirects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthenticationRedirects")
            .finish_non_exhaustive()
    }
}
impl AuthenticationRedirects {
    /// 默认未认证跳login，已认证按dashboard/home/根路径查找。
    pub fn new() -> Self {
        Self::default()
    }
    /// 对应 `Authenticate::redirectUsing`。
    #[must_use]
    pub fn redirect_guests_using(
        mut self,
        callback: impl Fn(&RequestInfo) -> Result<Option<String>> + Send + Sync + 'static,
    ) -> Self {
        self.guests = Some(Arc::new(callback));
        self
    }
    /// 对应 `RedirectIfAuthenticated::redirectUsing`。
    #[must_use]
    pub fn redirect_users_using(
        mut self,
        callback: impl Fn(&RequestInfo) -> Result<Option<String>> + Send + Sync + 'static,
    ) -> Self {
        self.users = Some(Arc::new(callback));
        self
    }
}

#[derive(Clone)]
pub(crate) enum GuardMiddleware {
    Auth(Vec<String>),
    Guest(Vec<String>),
    Password {
        route: Option<String>,
        timeout: Option<i64>,
    },
}

pub(crate) async fn handle(
    State(options): State<GuardMiddleware>,
    Extension(state): Extension<AppState>,
    request: Request,
    next: Next,
) -> Response {
    match check(&options, &state, request).await {
        Ok(Ok(request)) => next.run(request).await,
        Ok(Err(response)) => response,
        Err(error) => error.into_response(),
    }
}

async fn check(
    options: &GuardMiddleware,
    state: &AppState,
    request: Request,
) -> Result<std::result::Result<Request, Response>> {
    let (mut parts, body) = request.into_parts();
    let (scope, info) = super::prepare_named_request(&mut parts, state).await?;
    let redirects = Some(scope.resolve::<AuthenticationRedirects>().await?);
    match options {
        GuardMiddleware::Auth(guards) | GuardMiddleware::Guest(guards) => {
            let manager = scope.resolve::<AuthManager>().await?;
            let guards = if guards.is_empty() {
                vec![None]
            } else {
                guards.iter().map(|s| Some(s.as_str())).collect()
            };
            let mut found = false;
            for guard in guards {
                if manager.guard(guard).await?.check().await? {
                    found = true;
                    if matches!(options, GuardMiddleware::Auth(_)) {
                        let name = guard
                            .filter(|name| !name.is_empty())
                            .map(str::to_owned)
                            .unwrap_or(manager.default_driver()?);
                        // 与既有具名AuthLayer共用完整原始ID/Provider投影，不伪造ID fallback。
                        let request = super::project_named_user(
                            &super::NamedAuthLayerConfig {
                                name,
                                optional: false,
                            },
                            state,
                            Request::from_parts(parts, body),
                        )
                        .await?;
                        return Ok(Ok(request));
                    }
                    break;
                }
            }
            if matches!(options, GuardMiddleware::Auth(_)) && !found {
                if info.expects_json()? {
                    return Err(Error::custom_with_status(
                        axum::http::StatusCode::UNAUTHORIZED,
                        "unauthenticated",
                        "Unauthenticated.",
                    ));
                }
                let target = match redirects.as_ref().and_then(|r| r.guests.as_ref()) {
                    Some(callback) => match callback(&info)? {
                        Some(target) if !target.is_empty() && target != "0" => target,
                        _ => return Ok(Err(axum::http::StatusCode::UNAUTHORIZED.into_response())),
                    },
                    None => info.route_urls()?.route("login", &BTreeMap::new())?,
                };
                return Ok(Err(Redirector::from_info(&info)
                    .guest(&target)?
                    .into_response()));
            }
            if matches!(options, GuardMiddleware::Guest(_)) && found {
                let target = match redirects.as_ref().and_then(|r| r.users.as_ref()) {
                    Some(callback) => callback(&info)?.ok_or_else(|| {
                        Error::custom_with_status(
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            "auth_user_redirect_invalid",
                            "已认证用户重定向回调不能返回null",
                        )
                    })?,
                    None => authenticated_redirect(&info)?,
                };
                return Ok(Err(Redirector::from_info(&info)
                    .to(&target)?
                    .into_response()));
            }
        }
        GuardMiddleware::Password { route, timeout } => {
            if let Some(response) =
                password_confirmation_response(&info, state, route.as_deref(), *timeout)?
            {
                return Ok(Err(response));
            }
        }
    }
    Ok(Ok(Request::from_parts(parts, body)))
}

fn password_confirmation_response(
    info: &RequestInfo,
    state: &AppState,
    route: Option<&str>,
    timeout: Option<i64>,
) -> Result<Option<Response>> {
    let configured = {
        let configured = state
            .config_repository()
            .section::<i64>("auth.password_timeout")?;
        if configured == 0 { 10_800 } else { configured }
    };
    let timeout = timeout.unwrap_or(configured);
    info.session()?.require_authenticated_storage()?;
    let confirmed = info
        .session()?
        .get_typed::<i64>("auth.password_confirmed_at")?
        .unwrap_or(0);
    let elapsed = chrono::Utc::now().timestamp().saturating_sub(confirmed);
    if elapsed > timeout {
        if info.expects_json()? {
            return Ok(Some(
                (
                    axum::http::StatusCode::LOCKED,
                    axum::Json(serde_json::json!({"message":"Password confirmation required."})),
                )
                    .into_response(),
            ));
        }
        let route = route
            .filter(|s| !s.is_empty() && *s != "0")
            .unwrap_or("password.confirm");
        let target = info.route_urls()?.route(route, &BTreeMap::new())?;
        return Ok(Some(
            Redirector::from_info(info).guest(&target)?.into_response(),
        ));
    }
    Ok(None)
}

fn authenticated_redirect(info: &RequestInfo) -> Result<String> {
    let urls = info.route_urls()?;
    for name in ["dashboard", "home"] {
        if urls.has(name) {
            return urls.route(name, &BTreeMap::new());
        }
    }
    for path in ["/dashboard", "/home"] {
        if urls.has_get_path(path) {
            return Ok(path.into());
        }
    }
    Ok("/".into())
}

pub(crate) fn guards(arguments: Vec<String>, guest: bool) -> GuardMiddleware {
    if guest {
        GuardMiddleware::Guest(arguments)
    } else {
        GuardMiddleware::Auth(arguments)
    }
}
pub(crate) fn password(arguments: Vec<String>) -> Result<GuardMiddleware> {
    if arguments.len() > 2 {
        return Err(Error::custom(
            "password_confirm_parameters_invalid",
            "密码确认最多接受route与timeout参数",
        ));
    }
    let mut arguments = arguments.into_iter();
    let route = arguments.next().filter(|s| !s.is_empty());
    let timeout = arguments
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<i64>().map_err(|_| {
                Error::custom(
                    "password_confirm_timeout_invalid",
                    "密码确认timeout必须为整数",
                )
            })
        })
        .transpose()?;
    Ok(GuardMiddleware::Password { route, timeout })
}

#[cfg(test)]
mod tests;
