//! 真实OIDC Web测试适配；只使用公开Guard、Provider与请求生命周期。

use axum::{
    Extension, Json,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
    response::IntoResponse,
};
use openrust::auth::fortify::{
    CredentialAuthority, DatabaseFactorAuthority, DatabaseTwoFactorState, FortifyLogin,
    FortifyLoginOutcome, GoogleTwoFactorAuthenticationProvider, TwoFactorActions,
};
use openrust::auth::{
    AuthDrivers, AuthLayer, AuthManager, Credentials, DatabaseUserProvider, ExternalLoginHandle,
    UserProvider,
};
use openrust::auth::{AuthFlows, ExternalIdentityFlows, Session, SessionStoreHandle};
use openrust::config::FrameworkConfig;
use openrust::container::Scope;
use openrust::database::Db;
use openrust::encryption::ContextualEncrypter;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::http::CookieServiceProvider;
use openrust::http::HttpServiceProvider;
use openrust::http::RequestInfo;
use openrust::routing::RouteGroup;
use openrust::session::SessionServiceProvider;
use openrust::support::{BoxFuture, DefaultProviders, ServiceProvider};
use openrust::testing::TestApp;
use openrust::{Error, Result};
use serde_json::{Value, json};
use std::sync::Arc;

#[path = "../../../../app/models/user.rs"]
pub(super) mod application_user;

#[derive(Clone)]
pub(super) struct Parts {
    pub(super) auth: AuthFlows,
    pub(super) actions: TwoFactorActions,
    pub(super) users: Arc<DatabaseUserProvider>,
    db: Db,
    passwords: std::sync::Arc<openrust::hashing::HashManager>,
    encryptor: ContextualEncrypter,
}
impl Parts {
    pub(super) async fn new(
        db: Db,
        passwords: std::sync::Arc<openrust::hashing::HashManager>,
        encryptor: ContextualEncrypter,
        auth: AuthFlows,
    ) -> Result<Self> {
        let mut builder = openrust::foundation::ProviderBuilder::new();
        builder.provider(openrust::events::EventsServiceProvider::new());
        builder.provider(openrust::encryption::EncryptionServiceProvider);
        let runtime = builder
            .start(figment::Figment::new().merge(("app.key", "A".repeat(32))))
            .await?;
        let field_crypto = runtime
            .container()
            .resolve::<dyn openrust::encryption::ApplicationEncrypter>()
            .await?;
        let events = (*runtime
            .container()
            .resolve::<openrust::events::Events>()
            .await?)
            .clone();
        let actions = TwoFactorActions::new(
            Arc::new(GoogleTwoFactorAuthenticationProvider::new(None)),
            field_crypto,
            events,
        );
        let policy = CredentialAuthority::new(db.clone(), false);
        let auth = auth
            .with_factor_authority(policy)?
            .with_factor_actions(actions.clone());
        let model_db = db.clone();
        let users = DatabaseUserProvider::new(db.clone(), passwords.clone(), "users")?
            .with_user_factory(move |attrs| {
                let identifier = attrs.get("id").cloned().ok_or(Error::Unauthorized)?;
                let store =
                    DatabaseTwoFactorState::new(model_db.clone(), "users", "id", identifier)?;
                Ok(Arc::new(
                    application_user::User::new(attrs)
                        .with_two_factor_state(store.clone())
                        .with_factor_authority(DatabaseFactorAuthority::new(store)),
                ) as openrust::auth::AuthUser)
            });
        Ok(Self {
            auth,
            actions,
            users: Arc::new(users),
            db,
            passwords,
            encryptor,
        })
    }
}

#[derive(Clone)]
struct Services {
    parts: Parts,
    external: ExternalIdentityFlows,
}
struct WebApplication;
impl ApplicationDefinition for WebApplication {
    type Config = openrust::config::NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        vec![
            Box::new(HttpServiceProvider),
            Box::new(openrust::auth::AuthServiceProvider::new()),
            Box::new(SessionServiceProvider::new()),
            Box::new(CookieServiceProvider::contextual()),
            Box::new(WebRoutes),
        ]
    }
}
struct WebRoutes;
impl ServiceProvider for WebRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // 普通 Web JSON 错误可以只有 message；在实际渲染前观察 code，继续默认渲染。
            app.exceptions(|handler| {
                handler.render::<Error, _>(|error, _| {
                    callback_error("web_exception", error);
                    None
                });
            })?;
            let services = app.resolve::<Services>().await?;
            let users = services.parts.users.clone();
            app.resolve::<AuthDrivers>()
                .await?
                .provider("oidc-test-users", move |_, _| {
                    let users = users.clone();
                    Box::pin(async move { Ok(users as Arc<dyn UserProvider>) })
                })?;
            app.routes(|routes| {
                routes.group(
                    RouteGroup::new()
                        .middleware_group("web")
                        .middleware(AuthLayer::optional_guard("web")),
                    |routes| {
                        routes
                            .get("/csrf", csrf)
                            .post("/begin", begin)
                            .post("/complete", complete)
                    },
                )
            })?;
            Ok(())
        })
    }
}
async fn csrf(session: Session) -> Result<Json<Value>> {
    if session.token()?.is_none() {
        session.regenerate_token()?;
    }
    Ok(Json(json!({"token":session.token()?})))
}
async fn login(scope: &Scope, session: Session) -> Result<(FortifyLogin, Arc<Services>)> {
    let services = scope.resolve::<Services>().await?;
    let manager = scope.resolve::<AuthManager>().await?;
    let pipeline = FortifyLogin::new(
        &manager,
        "web",
        session,
        Arc::new(services.parts.actions.clone()),
    )
    .await?
    .with_factor_authority_required(true);
    Ok((pipeline, services))
}
fn callback_error(stage: &'static str, error: &Error) {
    // 回调包含授权码、Cookie 与凭据；只记录框架稳定错误码和状态。
    eprintln!(
        "Keycloak Web callback stage={stage} code={} status={}",
        error.code(),
        error.status().as_u16()
    );
}

async fn begin(
    Extension(scope): Extension<Arc<Scope>>,
    session: Session,
    info: RequestInfo,
    Json(body): Json<Value>,
) -> Result<axum::response::Response> {
    let user_id = body["user_id"].as_i64().ok_or(Error::Unauthorized)?;
    let handle =
        ExternalLoginHandle::from_exposed(body["handle"].as_str().ok_or(Error::Unauthorized)?)?;
    let (pipeline, services) = login(&scope, session.clone())
        .await
        .inspect_err(|error| callback_error("resolve_web_guard", error))?;
    let users = services.parts.users.clone();
    // 只有已经通过真实外部Provider回调的fixture会调用此内部端点；同一Provider重新取模型。
    let pipeline = pipeline.with_authenticate_using(Arc::new(move |_, _| {
        let users = users.clone();
        Box::pin(async move { users.retrieve_by_id(&json!(user_id)).await })
    }));
    session.put_typed("oidc.test.external_handle", handle.expose())?;
    session.put_typed("oidc.test.pending_user", user_id)?;
    let outcome = pipeline
        .begin(&info, &Credentials::new(serde_json::Map::new()), false)
        .await
        .inspect_err(|error| callback_error("begin_web_login", error))?;
    let result = if outcome == FortifyLoginOutcome::TwoFactorChallenge {
        (StatusCode::ACCEPTED, Json(Value::Null)).into_response()
    } else {
        finish_or_invalidate(
            &scope,
            &session,
            &services,
            body.get("existing").filter(|value| !value.is_null()),
        )
        .await?
    };
    with_csrf(result, &session)
}
async fn complete(
    Extension(scope): Extension<Arc<Scope>>,
    session: Session,
    Json(body): Json<Value>,
) -> Result<axum::response::Response> {
    let (pipeline, services) = login(&scope, session.clone()).await?;
    pipeline
        .complete(body["code"].as_str(), body["recovery"].as_str())
        .await?;
    let result = finish_or_invalidate(&scope, &session, &services, None).await?;
    with_csrf(result, &session)
}
async fn finish_or_invalidate(
    scope: &Scope,
    session: &Session,
    services: &Services,
    existing: Option<&Value>,
) -> Result<axum::response::Response> {
    match finish(scope, session, services, existing).await {
        Ok(response) => Ok(response),
        Err(error) => {
            let manager = scope
                .resolve::<AuthManager>()
                .await
                .inspect_err(|issue| callback_error("cleanup_resolve_manager", issue))?;
            let guard = manager
                .guard(Some("web"))
                .await
                .inspect_err(|issue| callback_error("cleanup_resolve_guard", issue))?;
            guard
                .as_stateful()
                .ok_or(Error::Unauthorized)
                .inspect_err(|issue| callback_error("cleanup_stateful_guard", issue))?
                .logout()
                .await
                .inspect_err(|issue| callback_error("cleanup_logout", issue))?;
            // Fortify 已轮换 ID；仅在清空 Guard、因子证明和外部 handle 后落盘此中间 ID。
            session
                .flush()
                .inspect_err(|issue| callback_error("cleanup_flush", issue))?;
            if guard
                .check()
                .await
                .inspect_err(|issue| callback_error("cleanup_check_guard", issue))?
                || !session
                    .all_json()
                    .inspect_err(|issue| callback_error("cleanup_check_attributes", issue))?
                    .as_array()
                    .is_some_and(Vec::is_empty)
            {
                return Err(Error::custom(
                    "oidc_fixture_cleanup_not_anonymous",
                    "拒绝保存仍含身份或属性的临时 Session",
                ));
            }
            session
                .save()
                .await
                .inspect_err(|issue| callback_error("cleanup_save_guest", issue))?;
            session
                .invalidate()
                .await
                .inspect_err(|issue| callback_error("cleanup_invalidate", issue))?;
            // 与 Fortify Web 注销相同，访客 Session 需要新 CSRF token 才能返回原拒绝状态。
            session
                .regenerate_token()
                .inspect_err(|issue| callback_error("cleanup_regenerate_token", issue))?;
            callback_error("cleanup_complete", &error);
            Err(error)
        }
    }
}
async fn finish(
    scope: &Scope,
    session: &Session,
    services: &Services,
    existing: Option<&Value>,
) -> Result<axum::response::Response> {
    let user_id = session
        .get_typed::<i64>("oidc.test.pending_user")?
        .ok_or(Error::Unauthorized)?;
    let handle = ExternalLoginHandle::from_exposed(
        session
            .get_typed::<String>("oidc.test.external_handle")?
            .ok_or(Error::Unauthorized)?,
    )?;
    let manager = scope.resolve::<AuthManager>().await?;
    let selected = manager.guard(Some("web")).await?;
    let guard = selected.as_stateful().ok_or(Error::Unauthorized)?;
    if selected.id().await? != Some(json!(user_id)) {
        return Err(Error::Unauthorized);
    }
    // 关联先核当前实际Web身份，失败不向外发布凭据或SessionCookie。
    services
        .external
        .attach_session_with_guard(user_id, &handle, session, guard)
        .await
        .inspect_err(|error| callback_error("attach_guard_session", error))?;
    let (access_token, refresh_token) = if let Some(existing) = existing {
        // 值只由begin_authenticated的真实AuthCredentials传入，复用回调已绑定的家族。
        (
            existing["access_token"]
                .as_str()
                .ok_or(Error::Unauthorized)?
                .to_owned(),
            existing["refresh_token"]
                .as_str()
                .ok_or(Error::Unauthorized)?
                .to_owned(),
        )
    } else {
        let credentials = services
            .parts
            .auth
            .issue_credentials_from_session(guard, "web")
            .await
            .inspect_err(|error| callback_error("derive_refresh_family", error))?;
        if let Err(error) = services
            .external
            .attach_refresh_credentials(user_id, &handle, credentials.refresh_token().expose())
            .await
        {
            callback_error("attach_refresh_family", &error);
            services
                .parts
                .auth
                .logout(
                    credentials.refresh_token().expose(),
                    "oidc-web-attach-failed",
                )
                .await?;
            return Err(error);
        }
        (
            credentials.access_token().to_owned(),
            credentials.refresh_token().expose().to_owned(),
        )
    };
    session.forget(&["oidc.test.pending_user", "oidc.test.external_handle"])?;
    Ok((StatusCode::OK, Json(json!({"user_id":user_id,"session_id":session.id()?.as_str(),"access_token":access_token,"refresh_token":refresh_token,"handle":handle.expose()}))).into_response())
}
fn with_csrf(
    mut response: axum::response::Response,
    session: &Session,
) -> Result<axum::response::Response> {
    if let Some(token) = session.token()? {
        response.headers_mut().insert(
            "x-csrf-token",
            token
                .as_str()
                .ok_or(Error::Unauthorized)?
                .parse()
                .map_err(Error::internal)?,
        );
    }
    Ok(response)
}

pub(super) struct Outcome {
    pub(super) user_id: i64,
    pub(super) session_id: openrust::session::SessionId,
    pub(super) access_token: String,
    pub(super) refresh_token: String,
    pub(super) handle: ExternalLoginHandle,
}
pub(super) struct Reply {
    pub(super) status: StatusCode,
    pub(super) headers: HeaderMap,
    pub(super) outcome: Option<Outcome>,
}
pub(super) struct Fixture {
    app: TestApp<WebApplication>,
}
impl Fixture {
    pub(super) async fn new(
        parts: Parts,
        external: ExternalIdentityFlows,
        sessions: SessionStoreHandle,
    ) -> Result<Self> {
        let mut config = FrameworkConfig::default();
        config.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config)).merge(figment::providers::Serialized::defaults(json!({
                "app":{"env":"local","key":"oidc-owned-web-fixture-key"},
                "hashing":{"driver":"argon2id","argon":{"memory":19456,"time":1,"threads":1,"verify":true}},
                "auth":{"defaults":{"guard":"web","passwords":"users","provider":"accounts"},"guards":{"web":{"driver":"session","provider":"accounts"}},"providers":{"accounts":{"driver":"oidc-test-users"}}},
                "session":{"driver":"array","cookie":"openrust_session","secure":false,"lifetime":5,"lottery":[0,100]},
                "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
            })));
        let mut builder = TestApp::<WebApplication>::builder(sources);
        builder.configure_after_register(move |container| {
            let parts = parts.clone();
            let external = external.clone();
            let sessions = sessions.clone();
            Box::pin(async move {
                container.instance(Arc::new(parts.db.clone())).await?;
                container.instance(parts.passwords.clone()).await?;
                container
                    .instance(Arc::new(parts.encryptor.clone()))
                    .await?;
                container.instance(Arc::new(sessions)).await?;
                container
                    .instance(Arc::new(Services { parts, external }))
                    .await?;
                Ok(())
            })
        });
        Ok(Self {
            app: builder.build().await?,
        })
    }
    pub(super) async fn begin(
        &self,
        challenge: &openrust::auth::TwoFactorLoginChallenge,
        handle: &ExternalLoginHandle,
    ) -> Result<Reply> {
        self.begin_request(challenge.user_id(), handle, None).await
    }
    pub(super) async fn begin_authenticated(
        &self,
        credentials: &openrust::auth::AuthCredentials,
        handle: &ExternalLoginHandle,
    ) -> Result<Reply> {
        self.begin_request(credentials.user_id(), handle, Some(json!({"access_token":credentials.access_token(),"refresh_token":credentials.refresh_token().expose()}))).await
    }
    async fn begin_request(
        &self,
        user_id: i64,
        handle: &ExternalLoginHandle,
        existing: Option<Value>,
    ) -> Result<Reply> {
        let csrf = self.request(Method::GET, "/csrf", None, None, None).await?;
        if csrf.status != StatusCode::OK {
            return Err(Error::Unauthorized);
        }
        let cookie = cookie_header(&csrf.headers)?;
        let token = csrf
            .headers
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok());
        self.request(
            Method::POST,
            "/begin",
            Some(&cookie),
            token,
            Some(json!({"user_id":user_id,"handle":handle.expose(),"existing":existing})),
        )
        .await
    }
    pub(super) async fn complete(
        &self,
        cookie: &str,
        csrf: Option<&str>,
        code: Option<&str>,
        recovery: Option<&str>,
    ) -> Result<Reply> {
        self.request(
            Method::POST,
            "/complete",
            Some(cookie),
            csrf,
            Some(json!({"code":code,"recovery":recovery})),
        )
        .await
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        cookie: Option<&str>,
        csrf: Option<&str>,
        body: Option<Value>,
    ) -> Result<Reply> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost")
            .header("accept", "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = csrf {
            request = request.header("x-csrf-token", csrf);
        }
        let body = if let Some(body) = body {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&body).map_err(Error::internal)?)
        } else {
            Body::empty()
        };
        let response = self
            .app
            .raw_response(request.body(body).map_err(Error::internal)?)
            .await?;
        let status = response.status();
        let mut headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .map_err(Error::internal)?;
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if status.is_client_error() || status.is_server_error() {
            let stage = match path {
                "/csrf" => "csrf_http_response",
                "/begin" => "begin_http_response",
                "/complete" => "complete_http_response",
                _ => "web_http_response",
            };
            let code = value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("no_envelope_code");
            eprintln!(
                "Keycloak Web callback stage={stage} code={code} status={}",
                status.as_u16()
            );
        }
        // csrf GET的实际Session值由标准handler返回；只用于首次受保护begin。
        if path == "/csrf"
            && let Some(token) = value["token"].as_str()
        {
            headers.insert("x-csrf-token", token.parse().map_err(Error::internal)?);
        }
        let outcome = if status == StatusCode::OK && path != "/csrf" {
            Some(Outcome {
                user_id: value["user_id"].as_i64().ok_or(Error::Unauthorized)?,
                session_id: value["session_id"]
                    .as_str()
                    .ok_or(Error::Unauthorized)?
                    .parse()?,
                access_token: value["access_token"]
                    .as_str()
                    .ok_or(Error::Unauthorized)?
                    .into(),
                refresh_token: value["refresh_token"]
                    .as_str()
                    .ok_or(Error::Unauthorized)?
                    .into(),
                handle: ExternalLoginHandle::from_exposed(
                    value["handle"].as_str().ok_or(Error::Unauthorized)?,
                )?,
            })
        } else {
            None
        };
        Ok(Reply {
            status,
            headers,
            outcome,
        })
    }
    pub(super) async fn shutdown(&mut self) -> Result<()> {
        self.app
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await
    }
}
pub(super) fn cookie_header(headers: &HeaderMap) -> Result<String> {
    let cookies = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| {
            value
                .to_str()
                .map_err(Error::internal)
                .map(|value| value.split(';').next().unwrap_or_default().to_owned())
        })
        .collect::<Result<Vec<_>>>()?;
    if cookies.is_empty() {
        return Err(Error::Unauthorized);
    }
    Ok(cookies.join("; "))
}
