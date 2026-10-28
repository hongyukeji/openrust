//! 固定 OIDC relying-party 配置与同一容器的账户流程；不接收请求提供的 issuer。

use super::ExampleConfig;
use openrust::auth::{
    AuthFlows, BackchannelLogoutOutcome, ExternalIdentityAuthorization, ExternalIdentityFlows,
    ExternalIdentityLink, ExternalIdentityProvider, ExternalLoginHandle, ExternalLoginResult,
    ExternalLogoutRedirect, OidcProvider, Session, SessionStoreHandle,
};
use openrust::core::{BootContext, RegisterContext, ServiceProvider};
use openrust::db::sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use openrust::http::{DependencyPolicy, HttpClient, HttpClientConfig, HttpTargetPolicy};
use openrust::security::Encryptor;
use openrust::{BoxFuture, Db, Error, Result};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};

pub(crate) const PROVIDER_NAME: &str = "example-oidc";
#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExampleOidcConfig {
    pub(crate) issuer: String,
    pub(crate) client_id: String,
    #[serde(default, skip_serializing)]
    pub(crate) client_secret: Option<String>,
    pub(crate) redirect_uri: String,
    #[serde(default)]
    pub(crate) allow_loopback: bool,
}
impl ExampleOidcConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        let issuer = url::Url::parse(&self.issuer).map_err(|_| invalid())?;
        let callback = url::Url::parse(&self.redirect_uri).map_err(|_| invalid())?;
        if callback.path() != "/api/example/oidc/callback"
            || self.client_id.trim().is_empty()
            || self.client_id.len() > 256
            || self
                .client_secret
                .as_ref()
                .is_some_and(|secret| secret.is_empty() || secret.len() > 4096)
        {
            return Err(invalid());
        }
        for url in [&issuer, &callback] {
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid());
            }
            if url.scheme() != "https" && !(self.allow_loopback && loopback(url)) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
fn loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    }
}
fn invalid() -> Error {
    Error::custom(
        "example_oidc_config_invalid",
        "example.oidc 配置必须是固定、无凭据的可信 issuer/client/callback",
    )
}
fn configured(config: ExampleConfig) -> Result<ExampleOidcConfig> {
    config
        .oidc
        .ok_or_else(|| Error::custom("example_oidc_unconfigured", "外部身份登录尚未配置"))
}
struct Discovered(Arc<OidcProvider>);
pub(crate) struct ExampleOidcServiceProvider;
impl ServiceProvider for ExampleOidcServiceProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.singleton::<Discovered, _>(|resolver| {
            Box::pin(async move {
                let config = configured(resolver.section::<ExampleConfig>("example")?)?;
                config.validate()?;
                let environment = resolver.section::<openrust::config::AppConfig>("app")?.env;
                if environment == openrust::config::Environment::Production && config.allow_loopback
                {
                    return Err(invalid());
                }
                let issuer = url::Url::parse(&config.issuer).map_err(|_| invalid())?;
                let host = match issuer.host().ok_or_else(invalid)? {
                    url::Host::Domain(host) => host.to_owned(),
                    url::Host::Ipv4(ip) => ip.to_string(),
                    url::Host::Ipv6(ip) => ip.to_string(),
                };
                let port = issuer.port_or_known_default().ok_or_else(invalid)?;
                let mut policy = HttpTargetPolicy::new();
                if config.allow_loopback && loopback(&issuer) {
                    let network = if host
                        .parse::<std::net::IpAddr>()
                        .map_err(|_| invalid())?
                        .is_ipv4()
                    {
                        "127.0.0.0/8"
                    } else {
                        "::1/128"
                    };
                    policy
                        .allow_internal(&host, network.parse().map_err(|_| invalid())?, [port])
                        .map_err(|_| invalid())?;
                } else {
                    policy.allow_host(&host).map_err(|_| invalid())?;
                    policy.allow_port(port).map_err(|_| invalid())?;
                }
                let client = HttpClient::restricted(
                    HttpClientConfig {
                        connect_timeout: Duration::from_secs(5),
                        timeout: Duration::from_secs(15),
                        max_response_bytes: 1024 * 1024,
                        proxy: None,
                    },
                    policy,
                )
                .map_err(|_| invalid())?;
                let dependency = client
                    .dependency(
                        "example-oidc",
                        DependencyPolicy {
                            max_in_flight: 4,
                            max_waiting: 4,
                            circuit_breaker: None,
                        },
                    )
                    .map_err(|_| invalid())?;
                let provider = OidcProvider::discover(
                    &config.issuer,
                    PROVIDER_NAME,
                    config.client_id,
                    config.client_secret,
                    &config.redirect_uri,
                    ["profile".into(), "email".into()],
                    Duration::from_secs(30),
                    dependency,
                )
                .await?;
                Ok(Arc::new(Discovered(Arc::new(provider))))
            })
        });
        app.scoped::<OidcAccount, _>(|resolver| {
            Box::pin(async move {
                let provider = resolver.resolve::<Discovered>().await?;
                let db = resolver.resolve::<Db>().await?;
                let auth = resolver.resolve::<AuthFlows>().await?;
                let encryptor = resolver.resolve::<Encryptor>().await?;
                let issuer = provider.0.issuer().to_owned();
                let redirect_uri =
                    configured(resolver.section::<ExampleConfig>("example")?)?.redirect_uri;
                let origin =
                    url::Url::parse(&resolver.section::<openrust::config::AppConfig>("app")?.url)
                        .map_err(|_| invalid())?
                        .origin()
                        .ascii_serialization();
                let external: Arc<dyn ExternalIdentityProvider> = provider.0.clone();
                Ok(Arc::new(OidcAccount {
                    flows: ExternalIdentityFlows::new(
                        (*db).clone(),
                        (*auth).clone(),
                        (*encryptor).clone(),
                        [external],
                    )?,
                    issuer,
                    db,
                    auth,
                    origin,
                    redirect_uri,
                }))
            })
        });
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if app.section::<ExampleConfig>("example")?.oidc.is_some() {
                app.resolve::<Discovered>().await?;
                app.require::<OidcAccount>();
                app.require::<SessionStoreHandle>();
            }
            Ok(())
        })
    }
}
pub(crate) struct OidcAccount {
    flows: ExternalIdentityFlows,
    issuer: String,
    db: Arc<Db>,
    auth: Arc<AuthFlows>,
    origin: String,
    redirect_uri: String,
}
impl OidcAccount {
    pub(crate) async fn authorize(&self, source: &str) -> Result<ExternalIdentityAuthorization> {
        self.flows.authorize(PROVIDER_NAME, source).await
    }
    pub(crate) async fn complete(
        &self,
        state: &str,
        code: &str,
        binding: &str,
        source: &str,
    ) -> Result<ExternalLoginResult> {
        self.flows
            .complete_with_handle(PROVIDER_NAME, state, code, binding, source)
            .await
    }
    pub(crate) async fn linked(&self, user: i64) -> Result<Vec<ExternalIdentityLink>> {
        self.flows.linked_identities(user).await
    }
    pub(crate) async fn bind(
        &self,
        user: i64,
        input: crate::modules::example::http::requests::oidc::BindInput,
        binding: &str,
        source: &str,
    ) -> Result<()> {
        self.flows
            .bind(
                user,
                &input.email,
                &input.password,
                source,
                PROVIDER_NAME,
                &input.state,
                &input.code,
                binding,
            )
            .await
    }
    pub(crate) async fn unbind(
        &self,
        user: i64,
        input: crate::modules::example::http::requests::oidc::UnbindInput,
        source: &str,
    ) -> Result<bool> {
        self.flows
            .unbind(
                user,
                &input.email,
                &input.password,
                source,
                PROVIDER_NAME,
                &self.issuer,
                &input.subject,
            )
            .await
    }
}

pub(crate) fn state_hash(state: &str) -> String {
    format!("{:x}", Sha256::digest(state.as_bytes()))
}
impl OidcAccount {
    pub(crate) fn require_browser_origin(
        &self,
        headers: &openrust::http::axum::http::HeaderMap,
    ) -> Result<()> {
        if headers.get_all("origin").iter().count() != 1
            || headers.get("origin").and_then(|value| value.to_str().ok())
                != Some(self.origin.as_str())
            || headers.get_all("x-example-request").iter().count() != 1
            || headers
                .get("x-example-request")
                .and_then(|value| value.to_str().ok())
                != Some("same-origin")
        {
            return Err(Error::Forbidden("绑定请求必须来自应用同源页面".into()));
        }
        Ok(())
    }
    fn intent_statement(&self, sql: &str, values: Vec<Value>) -> Result<Statement> {
        let backend = self.db.backend()?;
        // 模块迁移使用固定业务表名；框架 database.table_prefix 只映射框架表。
        let table = match backend {
            DbBackend::Postgres => "\"example_oidc_bind_intents\"",
            DbBackend::MySql => "`example_oidc_bind_intents`",
            _ => return Err(invalid()),
        };
        let sql = sql.replace("{table}", table);
        let sql = match backend {
            DbBackend::Postgres => {
                let mut index = 0;
                sql.chars()
                    .map(|ch| {
                        if ch == '?' {
                            index += 1;
                            format!("${index}")
                        } else {
                            ch.to_string()
                        }
                    })
                    .collect::<String>()
            }
            DbBackend::MySql => sql,
            _ => return Err(invalid()),
        };
        Ok(Statement::from_sql_and_values(backend, sql, values))
    }
    pub(crate) async fn begin_browser_bind(
        &self,
        owner: i64,
        email: &str,
        password: &str,
        source: &str,
    ) -> Result<ExternalIdentityAuthorization> {
        if self.auth.reauthenticate(email, password, source).await? != owner {
            return Err(Error::Unauthorized);
        }
        let authorization = self.authorize(source).await?;
        let state = authorization
            .url()
            .query_pairs()
            .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
            .ok_or_else(invalid)?;
        let statement = self.intent_statement(
            "INSERT INTO {table} (state_hash,owner_id,browser_binding_hash,expires_at_ms,phase) VALUES (?,?,?,?,'pending')",
            vec![state_hash(&state).into(), owner.into(), state_hash(authorization.browser_binding_cookie()).into(), (chrono::Utc::now().timestamp_millis() + 600_000).into()])?;
        self.db
            .connection()?
            .execute_raw(statement)
            .await
            .map_err(Error::internal)?;
        Ok(authorization)
    }
    /// 保留已消费标记，旧绑定 state 永远不能退回普通登录流程。
    pub(crate) async fn is_browser_bind(&self, state: &str) -> Result<bool> {
        let statement = self.intent_statement(
            "SELECT state_hash FROM {table} WHERE state_hash = ?",
            vec![state_hash(state).into()],
        )?;
        Ok(self
            .db
            .connection()?
            .query_one_raw(statement)
            .await
            .map_err(Error::internal)?
            .is_some())
    }
    pub(crate) async fn transition_browser_bind(
        &self,
        owner: i64,
        state: &str,
        binding: &str,
        from: &str,
        to: &str,
    ) -> Result<()> {
        let statement = self.intent_statement("UPDATE {table} SET phase = ? WHERE state_hash = ? AND owner_id = ? AND browser_binding_hash = ? AND phase = ? AND expires_at_ms > ?",
            vec![to.into(), state_hash(state).into(), owner.into(), state_hash(binding).into(), from.into(), chrono::Utc::now().timestamp_millis().into()])?;
        let changed = self
            .db
            .connection()?
            .execute_raw(statement)
            .await
            .map_err(Error::internal)?;
        if changed.rows_affected() != 1 {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
}

impl OidcAccount {
    pub(crate) async fn logout(
        &self,
        user: i64,
        handle: String,
        sessions: &SessionStoreHandle,
    ) -> Result<Option<ExternalLogoutRedirect>> {
        self.flows
            .begin_logout(
                user,
                &ExternalLoginHandle::from_exposed(handle)?,
                &self.redirect_uri,
                sessions,
            )
            .await
    }
    pub(crate) async fn complete_logout(&self, state: &str) -> Result<()> {
        let destination = self.flows.complete_logout(PROVIDER_NAME, state).await?;
        if destination.as_str() != self.redirect_uri {
            return Err(Error::Unauthorized);
        }
        // 回调即固定登录 callback；仅完成状态，不再次重定向到同一 URI。
        Ok(())
    }
    pub(crate) async fn attach_session(
        &self,
        user: i64,
        handle: String,
        email: &str,
        password: &str,
        source: &str,
        session: &Session,
    ) -> Result<()> {
        if self.auth.reauthenticate(email, password, source).await? != user {
            return Err(Error::Unauthorized);
        }
        self.flows
            .attach_session(user, &ExternalLoginHandle::from_exposed(handle)?, session)
            .await
    }
    pub(crate) async fn backchannel(
        &self,
        token: &str,
        sessions: &SessionStoreHandle,
    ) -> Result<BackchannelLogoutOutcome> {
        self.flows
            .process_backchannel_logout(PROVIDER_NAME, token, sessions)
            .await
    }
}
