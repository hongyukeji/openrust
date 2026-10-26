#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{convert::Infallible, sync::Arc};

use crate::config::ConfigError;
use crate::core::{App, AppBuilder, AppState, BoxFuture, Error, Plugin, Result};
use crate::db::{Db, DbBackend};
use axum::extract::{Extension, FromRequestParts, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::Route;
use http::request::Parts;
use tower::{Layer, Service};

mod database;
mod flows;
mod jwt;
mod migration;
mod password;
mod pat;
mod policy;
mod refresh;
mod session;
mod verification;

use database::DatabaseAuthenticator;

pub use crate::config::JwtConfig;
pub use flows::{AuthCredentials, AuthFlows, AuthFlowsPlugin, SessionCredentials};
pub use jwt::{JwtPlugin, JwtTokens};
pub use migration::{
    AuthMigration, AuthPolicyMigration, AuthRefreshTokenMigration, AuthVerificationCodeMigration,
};
pub use policy::Policy;

pub(crate) fn resolve_auth_sql(db: &crate::db::Db, sql: &str) -> Result<String> {
    let backend = db.backend()?;
    let mut resolved = sql.to_owned();
    for logical in [
        "users",
        "sessions",
        "personal_access_tokens",
        "auth_refresh_tokens",
        "roles",
        "permissions",
        "user_roles",
        "role_permissions",
    ] {
        let physical = db.quoted_table_name(logical)?;
        resolved = resolved.replace(&format!("tb_{logical}"), &physical);
    }
    if !matches!(
        backend,
        crate::db::sea_orm::DbBackend::Postgres | crate::db::sea_orm::DbBackend::MySql
    ) {
        return Err(Error::custom(
            "unsupported_auth_database",
            "authentication requires PostgreSQL or MySQL",
        ));
    }
    Ok(resolved)
}
pub use crate::config::PasswordConfig;
pub use crate::config::SessionConfig;
pub use password::{EncodedPasswordHash, Passwords};
pub use pat::{IssuedPersonalAccessToken, PersonalAccessTokenInfo, PersonalAccessTokens};
pub use refresh::{IssuedRefreshToken, RefreshRotation, RefreshTokens};
pub use session::{DatabaseSessionStore, SessionPlugin};
pub use tower_sessions::Session;
pub use verification::{
    DatabaseVerificationCodeStore, IssuedVerificationCode, VerificationChallenge,
    VerificationCodeFuture, VerificationCodePolicy, VerificationCodeStore, VerificationCodes,
};

/// The authentication source selected by a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    /// Signed JWT access-token authentication.
    Jwt,
    /// Cookie session authentication.
    Session,
    /// Personal access token authentication.
    PersonalAccessToken,
}

/// Credential selection policy for one route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Authenticate only with an OpenRust JWT access token.
    Jwt,
    /// Authenticate only with the `openrust_session` cookie.
    Session,
    /// Authenticate only with an `Authorization: Bearer` personal access token.
    PersonalAccessToken,
    /// Accept one session cookie or a Bearer personal access token.
    SessionOrToken,
}

/// A validated identity provided to request handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUser {
    id: i64,
    source: AuthSource,
    token_abilities: Option<Arc<[String]>>,
}

impl CurrentUser {
    /// Returns the authenticated account ID.
    pub const fn id(&self) -> i64 {
        self.id
    }

    /// Returns the credential source used for this request.
    pub const fn source(&self) -> AuthSource {
        self.source
    }

    /// Returns personal token abilities, or `None` for session authentication.
    pub fn token_abilities(&self) -> Option<&[String]> {
        self.token_abilities.as_deref()
    }

    /// Returns whether this personal token contains the exact requested ability.
    pub fn has_ability(&self, ability: &str) -> bool {
        self.token_abilities()
            .is_some_and(|abilities| abilities.iter().any(|item| item == ability))
    }
}

/// An optional authenticated identity. `None` means the selected credential was absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalUser(pub Option<CurrentUser>);

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = Error;

    // Axum defines this extractor trait method as async even though this extractor reads only
    // request-local extensions.
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self> {
        match parts.extensions.get::<AuthOutcome>() {
            Some(AuthOutcome(Some(user))) => Ok(user.clone()),
            Some(AuthOutcome(None)) => Err(Error::Unauthorized),
            None => Err(Error::internal(MissingAuthLayer)),
        }
    }
}

impl<S> FromRequestParts<S> for OptionalUser
where
    S: Send + Sync,
{
    type Rejection = Error;

    // Axum defines this extractor trait method as async even though this extractor reads only
    // request-local extensions.
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self> {
        match parts.extensions.get::<AuthOutcome>() {
            Some(AuthOutcome(user)) => Ok(Self(user.clone())),
            None => Err(Error::internal(MissingAuthLayer)),
        }
    }
}

#[derive(Clone)]
struct AuthOutcome(Option<CurrentUser>);

/// A selected request credential passed to an [`Authenticator`].
///
/// The value intentionally has no `Debug` or serialization implementation because it contains
/// the raw session or bearer secret.
pub struct AuthCredential<'a> {
    source: AuthSource,
    value: Option<&'a str>,
    session: Option<Session>,
    csrf_token: Option<&'a str>,
    method: &'a Method,
}

impl AuthCredential<'_> {
    /// Returns which credential type was selected.
    pub const fn source(&self) -> AuthSource {
        self.source
    }

    /// Returns the selected personal access token. Cookie Session IDs are never exposed here.
    pub const fn value(&self) -> Option<&str> {
        self.value
    }

    /// Returns the shared Session for Cookie authentication, if this request selected it.
    pub const fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Returns the request's CSRF header for session credentials, if present.
    pub const fn csrf_token(&self) -> Option<&str> {
        self.csrf_token
    }

    /// Returns the HTTP method associated with this credential.
    pub const fn method(&self) -> &Method {
        self.method
    }
}

/// A credential-verified account identity returned by an [`Authenticator`].
pub struct AuthPrincipal {
    user_id: i64,
    token_abilities: Option<Vec<String>>,
}

impl AuthPrincipal {
    /// Creates a principal after the authenticator has verified the credential and account.
    ///
    /// Session authentication uses `None`; personal access tokens use `Some`, including an
    /// empty vector when the token has no restricted abilities.
    pub fn new(user_id: i64, token_abilities: Option<Vec<String>>) -> Self {
        Self {
            user_id,
            token_abilities,
        }
    }
}

/// Verifies one already-selected session or personal access token.
pub trait Authenticator: Send + Sync + 'static {
    /// Validates the credential and returns its account ID and token abilities.
    fn authenticate<'a>(
        &'a self,
        credential: AuthCredential<'a>,
    ) -> BoxFuture<'a, Result<AuthPrincipal>>;
}

struct AuthService {
    authenticator: Arc<dyn Authenticator>,
}

/// Registers the default database authenticator or an application supplied implementation.
pub struct AuthPlugin {
    authenticator: Arc<dyn Authenticator>,
    database_authenticator: Option<Arc<DatabaseAuthenticator>>,
    password_config: PasswordConfig,
}

impl AuthPlugin {
    /// Creates the default database-backed authenticator.
    pub fn new() -> Self {
        let authenticator = Arc::new(DatabaseAuthenticator::default());
        Self {
            authenticator: authenticator.clone(),
            database_authenticator: Some(authenticator),
            password_config: PasswordConfig::default(),
        }
    }

    /// Creates an authenticator plugin using an application or third-party implementation.
    pub fn with_authenticator(authenticator: impl Authenticator) -> Self {
        Self {
            authenticator: Arc::new(authenticator),
            database_authenticator: None,
            password_config: PasswordConfig::default(),
        }
    }

    /// Overrides the validated Argon2id parameters used by the password service.
    #[must_use]
    pub fn with_password_config(mut self, config: PasswordConfig) -> Self {
        self.password_config = config;
        self
    }
}

impl Default for AuthPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for AuthPlugin {
    fn name(&self) -> &'static str {
        "auth"
    }

    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        if app.get::<AuthService>().is_some() {
            return Err(Error::custom(
                "auth_service_already_registered",
                "register only one authentication plugin per application",
            ));
        }
        app.insert(AuthService {
            authenticator: self.authenticator.clone(),
        });
        let passwords = Passwords::new(self.password_config.clone())?;
        app.insert(passwords.clone());
        if let Some(db) = app.get::<Db>() {
            let db = (*db).clone();
            app.insert(RefreshTokens::new(db.clone()));
            app.insert(PersonalAccessTokens::new(db.clone()));
            if app.get::<VerificationCodes>().is_none() {
                app.insert(VerificationCodes::new(
                    DatabaseVerificationCodeStore::new(db.clone()),
                    passwords.clone(),
                ));
            }
            app.insert(Policy::new(db));
        }
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let service = app.state().try_get::<AuthService>()?;
            if !Arc::ptr_eq(&self.authenticator, &service.authenticator) {
                return Err(Error::custom(
                    "auth_service_replaced",
                    "a later plugin replaced the authentication service",
                ));
            }
            if let Some(authenticator) = &self.database_authenticator {
                let db = app.state().try_get::<Db>()?;
                if !matches!(db.backend()?, DbBackend::Postgres | DbBackend::MySql) {
                    return Err(Error::Config(ConfigError::Invalid(
                        "the default authenticator supports PostgreSQL and MySQL only".into(),
                    )));
                }
                authenticator.initialize((*db).clone())?;
            }
            Ok(())
        })
    }
}

/// Applies required or optional authentication to routes using the existing Tower layer API.
pub struct AuthLayer;

impl AuthLayer {
    /// Requires the route's selected credential mode.
    pub fn required(
        mode: AuthMode,
    ) -> impl Layer<
        Route,
        Service: Service<
            Request,
            Response = Response,
            Error = Infallible,
            Future: Send + 'static,
        > + Clone
                     + Send
                     + Sync
                     + 'static,
    > + Clone
    + Send
    + Sync
    + 'static {
        middleware::from_fn_with_state::<_, _, (State<AuthLayerConfig>, Extension<AppState>, Request)>(
            AuthLayerConfig {
                mode,
                optional: false,
            },
            authenticate,
        )
    }

    /// Allows an absent selected credential while rejecting malformed or invalid credentials.
    pub fn optional(
        mode: AuthMode,
    ) -> impl Layer<
        Route,
        Service: Service<
            Request,
            Response = Response,
            Error = Infallible,
            Future: Send + 'static,
        > + Clone
                     + Send
                     + Sync
                     + 'static,
    > + Clone
    + Send
    + Sync
    + 'static {
        middleware::from_fn_with_state::<_, _, (State<AuthLayerConfig>, Extension<AppState>, Request)>(
            AuthLayerConfig {
                mode,
                optional: true,
            },
            authenticate,
        )
    }
}

#[derive(Clone, Copy)]
struct AuthLayerConfig {
    mode: AuthMode,
    optional: bool,
}

async fn authenticate(
    State(config): State<AuthLayerConfig>,
    Extension(state): Extension<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let selected = match select_credential(config.mode, request.headers(), request.method()) {
        Ok(selected) => selected,
        Err(error) => return error.into_response(),
    };
    let Some((source, value, csrf_token)) = selected else {
        if !config.optional {
            return Error::Unauthorized.into_response();
        }
        let mut request = request;
        request.extensions_mut().insert(AuthOutcome(None));
        return next.run(request).await;
    };
    let service = match state.try_get::<AuthService>() {
        Ok(service) => service,
        Err(error) => return error.into_response(),
    };
    let credential = AuthCredential {
        source,
        value: (source != AuthSource::Session).then_some(value),
        session: if source == AuthSource::Session {
            let Some(session) = request.extensions().get::<Session>().cloned() else {
                return Error::internal(MissingSessionLayer).into_response();
            };
            if session.id().map(|id| id.to_string()).as_deref() != Some(value) {
                return Error::Unauthorized.into_response();
            }
            Some(session)
        } else {
            None
        },
        csrf_token,
        method: request.method(),
    };
    let principal = if source == AuthSource::Jwt {
        let tokens = match state.try_get::<JwtTokens>() {
            Ok(tokens) => tokens,
            Err(error) => return error.into_response(),
        };
        match tokens.verify_access_token(value) {
            Ok(user_id) => AuthPrincipal::new(user_id, None),
            Err(error) => return error.into_response(),
        }
    } else {
        match service.authenticator.authenticate(credential).await {
            Ok(principal) => principal,
            Err(error) => return error.into_response(),
        }
    };
    let token_abilities = match (source, principal.token_abilities) {
        (AuthSource::Jwt | AuthSource::Session, None) => None,
        (AuthSource::PersonalAccessToken, Some(abilities)) => Some(Arc::from(abilities)),
        _ => return Error::internal(InvalidPrincipal).into_response(),
    };
    if principal.user_id <= 0 {
        return Error::internal(InvalidPrincipal).into_response();
    }
    let user = CurrentUser {
        id: principal.user_id,
        source,
        token_abilities,
    };
    let mut request = request;
    request.extensions_mut().insert(AuthOutcome(Some(user)));
    next.run(request).await
}

fn select_credential<'a>(
    mode: AuthMode,
    headers: &'a HeaderMap,
    method: &'a Method,
) -> Result<Option<(AuthSource, &'a str, Option<&'a str>)>> {
    let session = if matches!(mode, AuthMode::Session | AuthMode::SessionOrToken) {
        session_cookie(headers)?
    } else {
        None
    };
    let selects_token = matches!(
        mode,
        AuthMode::Jwt | AuthMode::PersonalAccessToken | AuthMode::SessionOrToken
    );
    let authorization_present = selects_token
        && headers
            .get_all(header::AUTHORIZATION)
            .iter()
            .next()
            .is_some();
    if mode == AuthMode::SessionOrToken && session.is_some() && authorization_present {
        return Err(ambiguous_credentials());
    }
    let authorization = if selects_token {
        authorization_header(headers)?
    } else {
        None
    };
    if let Some(session) = session {
        if session.is_empty() {
            return Err(Error::Unauthorized);
        }
        let csrf = if is_safe_method(method) {
            None
        } else {
            Some(
                csrf_header(headers)?
                    .ok_or_else(|| Error::Forbidden("missing or invalid CSRF token".into()))?,
            )
        };
        return Ok(Some((AuthSource::Session, session, csrf)));
    }
    if let Some(header) = authorization {
        let mut segments = header.split_ascii_whitespace();
        let scheme = segments.next().unwrap_or_default();
        let token = segments.next().unwrap_or_default();
        if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() || segments.next().is_some() {
            return Err(Error::Unauthorized);
        }
        let source = match mode {
            AuthMode::Jwt => AuthSource::Jwt,
            AuthMode::PersonalAccessToken | AuthMode::SessionOrToken => {
                AuthSource::PersonalAccessToken
            }
            AuthMode::Session => return Err(Error::Unauthorized),
        };
        return Ok(Some((source, token, None)));
    }
    Ok(None)
}

fn is_safe_method(method: &Method) -> bool {
    method == Method::GET
        || method == Method::HEAD
        || method == Method::OPTIONS
        || method == Method::TRACE
}

fn session_cookie(headers: &HeaderMap) -> Result<Option<&str>> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().map_err(|_| Error::Unauthorized)?;
        for part in value.split(';') {
            let Some((name, cookie_value)) = part.trim().split_once('=') else {
                continue;
            };
            if name == "openrust_session" {
                if found.is_some() {
                    return Err(ambiguous_credentials());
                }
                found = Some(cookie_value.trim());
            }
        }
    }
    Ok(found)
}

fn authorization_header(headers: &HeaderMap) -> Result<Option<&str>> {
    let values = headers.get_all(header::AUTHORIZATION);
    let mut iter = values.iter();
    let Some(value) = iter.next() else {
        return Ok(None);
    };
    if iter.next().is_some() {
        return Err(ambiguous_credentials());
    }
    value.to_str().map(Some).map_err(|_| Error::Unauthorized)
}

fn csrf_header(headers: &HeaderMap) -> Result<Option<&str>> {
    let values = headers.get_all("x-csrf-token");
    let mut iter = values.iter();
    let Some(value) = iter.next() else {
        return Ok(None);
    };
    if iter.next().is_some() {
        return Err(Error::Forbidden("invalid CSRF token".into()));
    }
    value
        .to_str()
        .map(Some)
        .map_err(|_| Error::Forbidden("invalid CSRF token".into()))
}

fn ambiguous_credentials() -> Error {
    Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "ambiguous_credentials",
        "request contains ambiguous authentication credentials",
    )
}

#[derive(Debug)]
struct MissingAuthLayer;

impl std::fmt::Display for MissingAuthLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CurrentUser or OptionalUser requires an AuthLayer on the route")
    }
}

impl std::error::Error for MissingAuthLayer {}

#[derive(Debug)]
struct MissingSessionLayer;

impl std::fmt::Display for MissingSessionLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cookie authentication requires the shared Session middleware")
    }
}

impl std::error::Error for MissingSessionLayer {}

#[derive(Debug)]
struct InvalidPrincipal;

impl std::fmt::Display for InvalidPrincipal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("authenticator returned principal metadata incompatible with the credential")
    }
}

impl std::error::Error for InvalidPrincipal {}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderValue, Method, header};

    use super::{AuthMode, AuthSource, select_credential};

    fn code(error: crate::core::Error) -> String {
        match error {
            crate::core::Error::Custom { code, .. } => code,
            crate::core::Error::Unauthorized => "unauthorized".into(),
            crate::core::Error::Forbidden(_) => "forbidden".into(),
            other => format!("unexpected: {other}"),
        }
    }

    fn error_code<T>(result: crate::core::Result<T>) -> String {
        match result {
            Err(error) => code(error),
            Ok(_) => "unexpected_success".into(),
        }
    }

    #[test]
    fn route_mode_ignores_unselected_credentials_and_selects_one_source() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=session-secret"),
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic ignored"),
        );

        let session = select_credential(AuthMode::Session, &headers, &Method::GET);
        assert!(matches!(
            session,
            Ok(Some((AuthSource::Session, "session-secret", _)))
        ));

        assert_eq!(
            error_code(select_credential(
                AuthMode::PersonalAccessToken,
                &headers,
                &Method::GET
            )),
            "unauthorized"
        );
    }

    #[test]
    fn optional_missing_credentials_are_distinct_from_malformed_credentials() {
        let headers = axum::http::HeaderMap::new();
        assert!(matches!(
            select_credential(AuthMode::SessionOrToken, &headers, &Method::GET),
            Ok(None)
        ));

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer   "));
        assert_eq!(
            error_code(select_credential(
                AuthMode::PersonalAccessToken,
                &headers,
                &Method::GET
            )),
            "unauthorized"
        );
    }

    #[test]
    fn combined_mode_rejects_any_two_sources_and_repeated_credentials() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=a"),
        );
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("malformed"));
        assert_eq!(
            error_code(select_credential(
                AuthMode::SessionOrToken,
                &headers,
                &Method::GET
            )),
            "ambiguous_credentials"
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.append(header::AUTHORIZATION, HeaderValue::from_static("Bearer a"));
        headers.append(header::AUTHORIZATION, HeaderValue::from_static("Bearer b"));
        assert_eq!(
            error_code(select_credential(
                AuthMode::PersonalAccessToken,
                &headers,
                &Method::GET
            )),
            "ambiguous_credentials"
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=a; openrust_session=b"),
        );
        assert_eq!(
            error_code(select_credential(AuthMode::Session, &headers, &Method::GET)),
            "ambiguous_credentials"
        );
    }

    #[test]
    fn unsafe_session_requests_require_one_csrf_header_before_authentication() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("openrust_session=session-secret"),
        );
        assert_eq!(
            error_code(select_credential(
                AuthMode::Session,
                &headers,
                &Method::POST
            )),
            "forbidden"
        );
        headers.insert("x-csrf-token", HeaderValue::from_static("csrf-secret"));
        let selected = select_credential(AuthMode::Session, &headers, &Method::POST);
        assert!(matches!(
            selected,
            Ok(Some((AuthSource::Session, _, Some("csrf-secret"))))
        ));
    }
}
